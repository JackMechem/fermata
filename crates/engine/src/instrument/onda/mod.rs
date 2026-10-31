//! Fermata Onda: a modern wavetable synth of the kind that Serum made the standard (this project isn't
//! affiliated with Xfer Records, and the product name stays clear of theirs). It isn't a copy of any
//! one instrument's panel: it's the architecture: two wavetable oscillators with position scan and
//! unison, a sub and noise, a choice of filters, three envelopes, two LFOs and a modulation matrix.
//! Its wavetables are generated in code (`tables.rs`).
//!
//! **Signal path, per voice, at twice the output rate below 64 kHz:** each oscillator is up to 7
//! detuned copies playing a mip level of two neighbouring frames, crossfaded by the position. The
//! level is picked so nothing lies past the voice's Nyquist, and what lands above the audio band is
//! taken out by the decimator: so aliasing is measured rather than assumed. The copies pan into two
//! buses, each with its own filter (stereo for real, per voice).
//!
//! **Modulation** is worked out every [`CONTROL_TICKS`] ticks and ramped between, so a scanning
//! position has no steps in it: the matrix's slots each take one source, one destination and an
//! amount. Env 1 is the VCA. Env 2, Env 3 and the LFOs do nothing until a slot routes them.
//!
//! **Free-running LFOs** count ticks from the last reset, and keep doing so at rest, without work.
//! **Tempo-synced LFOs** read the song position at the start of a block (`crate::synth_dsp::lfo`'s
//! way), and advance sample by sample between, so their values may differ across block sizes by
//! rounding (measured under 1e-5 in the output), not bit for bit. All else is bit-identical.

use vst3_host::MidiEvent;

use super::common::{dc_pole, hz_to_note, OutStage, Rest};
use super::{FxContext, Instrument, InstrumentInfo, Midi};
use crate::synth_dsp::alloc::{unison_spread, Priority, VoiceAction, VoiceAllocator, VoiceMode};
use crate::synth_dsp::analog::{sub_seed, Rng};
use crate::synth_dsp::env::{EnvShape, Envelope};
use crate::synth_dsp::filter::{cutoff_g, DiodeLadder, Ladder, Svf, ZdfFilter};
use crate::synth_dsp::osc::{Noise, Osc, Wave};
use crate::synth_dsp::smooth::{Glide, Smoothed};
use crate::synth_dsp::{note_hz, tanh_fast};

pub mod params;
mod presets;
pub mod tables;

use params::param::*;
pub use params::{PARAMS, SECTIONS};
pub use presets::PRESETS;
use tables::{bank, level_for, Bank, FRAMES};

pub const VOICES: usize = 16;
const UNISON_MAX: usize = 7;
/// Ticks between modulation updates.
pub const CONTROL_TICKS: u32 = 16;
const FILTER_REF_NOTE: f32 = 60.0;
/// Detune at 100 %, the outermost copy from the centre, in semitones. A judgement, wide as the
/// genre likes it.
const DETUNE_ST: f32 = 0.6;
/// A slot's reach at amount 1, for each destination.
const POS_REACH: f32 = 1.0;
const PITCH_REACH_ST: f32 = 48.0;
const CUTOFF_REACH_ST: f32 = 84.0;
/// FM from B to A at 100 %, in cycles of A's phase. A judgement.
const FM_CYCLES: f32 = 2.0;
const OSC_GAIN: f32 = 0.45;
const NOISE_GAIN: f32 = 0.4;
const SUB_GAIN: f32 = 0.5;
const OUTPUT_GAIN: f32 = 1.25;
const DC_HZ: f32 = 5.0;
const HOLD_SECS: f32 = 0.05;
const SMOOTH_MS: f32 = 8.0;

/// A source's value this control tick, by the slot's source index.
#[derive(Debug, Clone, Copy, Default)]
struct Sources([f32; 9]);

/// What the panel's slots do, in the voice's terms.
#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    source: usize,
    dest: usize,
    amount: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct OscPatch {
    on: bool,
    table: usize,
    pos: f32,
    /// Octave, semitone and fine together.
    st: f32,
    level: f32,
    pan: f32,
    copies: usize,
    detune: f32,
    blend: f32,
    spread: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FilterKind {
    #[default]
    Low12,
    Low24,
    Band12,
    High12,
    Notch,
    Ladder24,
    Diode,
}

#[derive(Debug, Clone, Copy)]
struct Patch {
    rate: f32,
    osc: [OscPatch; 2],
    sub: f32,
    noise: f32,
    fm: f32,
    filter_on: bool,
    filter: FilterKind,
    cutoff_note: f32,
    res: f32,
    drive: f32,
    key_track: f32,
    env: [EnvShape; 3],
    slots: [Slot; MOD_SLOTS as usize],
    glide_secs: f32,
    noise_scale: f32,
}

/// A voice-wise LFO: the value is worked out from a phase each voice can hold itself.
#[derive(Debug, Clone, Copy)]
struct LfoState {
    phase: f64,
    held: f32,
    rng: Rng,
}

impl LfoState {
    fn new(seed: u64) -> LfoState {
        let mut rng = Rng::new(seed);
        let held = rng.bipolar();
        LfoState { phase: 0.0, held, rng }
    }

    #[inline]
    fn value(&self, wave: usize) -> f32 {
        let p = self.phase as f32;
        match wave {
            0 => (p * std::f32::consts::TAU).sin(),
            1 => 1.0 - 4.0 * (p - 0.5).abs(),
            2 => 2.0 * p - 1.0,
            3 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            _ => self.held,
        }
    }

    #[inline]
    fn advance(&mut self, inc: f64) {
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.held = self.rng.bipolar();
        }
    }
}

/// One copy in an oscillator's unison stack.
#[derive(Debug, Clone, Copy, Default)]
struct Copy1 {
    phase: f64,
    /// Offset in semitones from the unison, ±1.
    x: f32,
    gain_l: f32,
    gain_r: f32,
}

/// One oscillator in a voice.
#[derive(Debug, Clone, Copy)]
struct OscV {
    copies: [Copy1; UNISON_MAX],
    count: usize,
    /// Its position and level, ramped per control tick.
    pos: f32,
    pos_step: f32,
    level: f32,
    level_step: f32,
    /// This control tick's frequency per copy, as a tick's phase increment.
    inc_center: f32,
    inc_spread: f32,
    mip: usize,
}

impl OscV {
    fn new(rng: &mut Rng) -> OscV {
        let mut copies = [Copy1::default(); UNISON_MAX];
        for c in &mut copies {
            c.phase = rng.unit() as f64;
        }
        OscV { copies, count: 1, pos: 0.0, pos_step: 0.0, level: 0.0, level_step: 0.0, inc_center: 0.0, inc_spread: 0.0, mip: 0 }
    }

    /// Lays out the copies for the patch: detune by place, level by blend, pan by width and pan.
    fn lay_out(&mut self, p: &OscPatch) {
        self.count = p.copies.clamp(1, UNISON_MAX);
        let n = self.count;
        let power = 1.0 / (1.0 + p.blend * (n as f32 - 1.0)).sqrt();
        for (k, c) in self.copies.iter_mut().take(n).enumerate() {
            let (x, side) = unison_spread(k as u8, n as u8);
            c.x = x;
            // The centre copies are "the note" and the others come in with the blend.
            let centre = x.abs() < 0.34 || n == 1;
            let weight = if centre { 1.0 } else { p.blend };
            let pan = (p.pan + side * p.spread).clamp(-1.0, 1.0);
            let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
            c.gain_l = angle.cos() * std::f32::consts::SQRT_2 * weight * power;
            c.gain_r = angle.sin() * std::f32::consts::SQRT_2 * weight * power;
        }
    }

    /// One tick of the stack through a bank's table. `fm` is in cycles.
    #[inline]
    fn tick(&mut self, bank: &Bank, table: usize, fm: f32, l: &mut f32, r: &mut f32) {
        self.pos += self.pos_step;
        self.level += self.level_step;
        let at = (self.pos.clamp(0.0, 1.0)) * (FRAMES - 1) as f32;
        let f0 = at as usize;
        let frac = at - f0 as f32;
        let f1 = (f0 + 1).min(FRAMES - 1);
        let (a, b) = (bank.cycle(table, f0, self.mip).samples, bank.cycle(table, f1, self.mip).samples);
        let len = a.len();
        let mask = len - 1;
        let lf = len as f32;
        for c in self.copies.iter_mut().take(self.count) {
            let inc = (self.inc_center + self.inc_spread * c.x) as f64;
            // Phase in f64 for exact pitch; the read itself needs less.
            let p = c.phase;
            c.phase += inc;
            if c.phase >= 1.0 {
                c.phase -= 1.0;
            }
            // Into 0..1 by hand: the library's remainder is a call, this is a few cycles.
            let x = p as f32 + fm;
            let x = x - x as i32 as f32;
            let read = (if x < 0.0 { x + 1.0 } else { x }) * lf;
            let i = read as usize;
            let t = read - i as f32;
            // The two frames crossfaded, four points of it, then a Catmull-Rom cubic between them.
            let at = |k: usize| {
                let j = k & mask;
                a[j] + (b[j] - a[j]) * frac
            };
            let (p0, p1, p2, p3) = (at(i.wrapping_add(mask)), at(i), at(i + 1), at(i + 2));
            let c1 = 0.5 * (p2 - p0);
            let c2 = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
            let c3 = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
            let v = (p1 + t * (c1 + t * (c2 + t * c3))) * self.level;
            *l += v * c.gain_l;
            *r += v * c.gain_r;
        }
    }
}

/// The voice's filter pair: stereo.
#[derive(Debug, Clone, Copy, Default)]
struct FilterPair {
    a: [Svf; 2],
    b: [Svf; 2],
    ladder: [Ladder; 2],
    diode: [DiodeLadder; 2],
}

impl FilterPair {
    #[inline]
    fn tick(&mut self, ch: usize, kind: FilterKind, g: f32, res: f32, x: f32) -> f32 {
        match kind {
            FilterKind::Low12 => {
                self.a[ch].set(g, res);
                self.a[ch].tick(x)
            }
            FilterKind::Low24 => {
                self.a[ch].set(g, res * 0.5);
                self.b[ch].set(g, res);
                let y = self.a[ch].tick(x);
                self.b[ch].tick(y)
            }
            FilterKind::Band12 => {
                self.a[ch].set(g, res);
                self.a[ch].tick(x);
                self.a[ch].bp
            }
            FilterKind::High12 => {
                self.a[ch].set(g, res);
                self.a[ch].tick(x);
                self.a[ch].hp
            }
            FilterKind::Notch => {
                self.a[ch].set(g, res);
                self.a[ch].tick(x);
                self.a[ch].lp + self.a[ch].hp
            }
            FilterKind::Ladder24 => {
                self.ladder[ch].set(g, res * 4.3);
                self.ladder[ch].tick(x)
            }
            FilterKind::Diode => {
                self.diode[ch].set(g, res * 0.98);
                // It sits lower than the others at the same input: brought up to meet them.
                self.diode[ch].tick(x) * 3.0
            }
        }
    }
}

struct Voice {
    active: bool,
    gated: bool,
    osc: [OscV; 2],
    sub: Osc,
    noise: Noise,
    filter: FilterPair,
    env: [Envelope; 3],
    lfo: [LfoState; 2],
    glide: Glide,
    played: bool,
    velocity: f32,
    note: u8,
    ctl: u32,
    /// Ramped per control tick, from the slots: the filter's note and resonance, FM and the pan.
    cut: f32,
    cut_step: f32,
    res: f32,
    fm: f32,
    fm_step: f32,
    pan: f32,
}

/// What every voice's control tick shares.
#[derive(Debug, Clone, Copy, Default)]
struct Shared {
    bend: f32,
    wheel: f32,
    touch: f32,
    /// The LFOs that aren't per voice: their value (retrigger off), or their increment.
    lfo_inc: [f64; 2],
    lfo_wave: [usize; 2],
    lfo_retrig: [bool; 2],
    lfo_global: [f32; 2],
}

impl Voice {
    fn new(seed: u64, index: usize, patch: &Patch) -> Voice {
        let vs = sub_seed(seed, index as u64);
        let mut rng = Rng::new(sub_seed(vs, 1));
        let mut sub = Osc::new(Wave::Sine);
        sub.reset(rng.unit() as f64);
        let mut filter = FilterPair::default();
        let hp = cutoff_g(120.0, patch.rate);
        filter.diode[0].set_feedback_highpass(hp);
        filter.diode[1].set_feedback_highpass(hp);
        Voice {
            active: false,
            gated: false,
            osc: [OscV::new(&mut rng), OscV::new(&mut rng)],
            sub,
            noise: Noise::new(sub_seed(vs, 2)),
            filter,
            env: [Envelope::default(); 3],
            lfo: [LfoState::new(sub_seed(vs, 3)), LfoState::new(sub_seed(vs, 4))],
            glide: Glide::default(),
            played: false,
            velocity: 1.0,
            note: 60,
            ctl: 0,
            cut: 0.0,
            cut_step: 0.0,
            res: 0.0,
            fm: 0.0,
            fm_step: 0.0,
            pan: 0.0,
        }
    }

    fn start(&mut self, note: u8, velocity: f32, patch: &Patch, shared: &Shared, stolen: bool) {
        let n = note as f32;
        if patch.glide_secs > 0.0 && self.played {
            self.glide.target = n;
        } else {
            self.glide.jump(n);
        }
        self.played = true;
        self.active = true;
        self.gated = true;
        self.note = note;
        self.velocity = velocity;
        for e in &mut self.env {
            e.gate_on();
        }
        for k in 0..2 {
            if shared.lfo_retrig[k] {
                self.lfo[k].phase = 0.0;
            }
        }
        for o in 0..2 {
            self.osc[o].lay_out(&patch.osc[o]);
        }
        // The control tick lands now, and a new voice lands at its values, with no ramp from nothing.
        self.ctl = 0;
        if !stolen {
            self.cut = f32::NAN;
        }
    }

    fn glide_to(&mut self, note: u8) {
        self.glide.target = note as f32;
        self.note = note;
    }

    fn release(&mut self) {
        self.gated = false;
        for e in &mut self.env {
            e.gate_off();
        }
    }

    /// Works out the slots.
    #[inline]
    fn control(&mut self, patch: &Patch, sh: &Shared) {
        let n = self.note as f32;
        let mut src = Sources::default();
        src.0[1] = self.env[1].level;
        src.0[2] = self.env[2].level;
        for k in 0..2 {
            src.0[3 + k] = if sh.lfo_retrig[k] { self.lfo[k].value(sh.lfo_wave[k]) } else { sh.lfo_global[k] };
            if sh.lfo_retrig[k] {
                self.lfo[k].advance(sh.lfo_inc[k]);
            }
        }
        src.0[5] = self.velocity;
        src.0[6] = (n - FILTER_REF_NOTE) / 60.0;
        src.0[7] = sh.wheel;
        src.0[8] = sh.touch;
        // Each destination's sum.
        let mut d = [0.0f32; params::MOD_DESTS.len()];
        for s in &patch.slots {
            if s.source != 0 && s.dest != 0 {
                d[s.dest] += src.0[s.source] * s.amount;
            }
        }
        let note = self.glide.value;
        let steps = CONTROL_TICKS as f32;
        for o in 0..2 {
            let p = &patch.osc[o];
            let (pos_d, pitch_d, level_d) = if o == 0 { (d[1], d[3], d[5]) } else { (d[2], d[4], d[6]) };
            let v = &mut self.osc[o];
            let pos = (p.pos + pos_d * POS_REACH).clamp(0.0, 1.0);
            let level = if p.on { (p.level * (1.0 + level_d)).clamp(0.0, 2.0) } else { 0.0 };
            if self.cut.is_nan() {
                v.pos = pos;
                v.level = level;
            }
            v.pos_step = (pos - v.pos) / steps;
            v.level_step = (level - v.level) / steps;
            let centre = note + sh.bend + p.st + pitch_d * PITCH_REACH_ST;
            let hz = note_hz(centre);
            let inc = hz / patch.rate;
            v.inc_center = inc;
            let detune = (p.detune + d[9]).clamp(0.0, 1.0) * DETUNE_ST;
            // A small offset: the first-order term of 2^(st/12) is good enough, and costs nothing.
            v.inc_spread = inc * detune * (std::f32::consts::LN_2 / 12.0);
            let top = inc + v.inc_spread.abs();
            v.mip = level_for(top);
        }
        let cut = patch.cutoff_note + (note - FILTER_REF_NOTE) * patch.key_track + d[7] * CUTOFF_REACH_ST;
        if self.cut.is_nan() {
            self.cut = cut;
        }
        self.cut_step = (cut - self.cut) / steps;
        self.res = (patch.res + d[8]).clamp(0.0, 1.0);
        let fm = (patch.fm + d[10]).clamp(0.0, 1.0) * FM_CYCLES;
        self.fm_step = (fm - self.fm) / steps;
        self.pan = d[11].clamp(-1.0, 1.0);
    }

    /// One tick. Adds to the two buses.
    #[inline]
    fn tick(&mut self, patch: &Patch, sh: &Shared, bank: &Bank, bus_l: &mut f32, bus_r: &mut f32) {
        if self.ctl == 0 {
            self.ctl = CONTROL_TICKS;
            // Env 2 and 3 move here; they lead the audio by a tick at most, and the ramps smooth it.
            self.env[1].tick(&patch.env[1], CONTROL_TICKS as f32);
            self.env[2].tick(&patch.env[2], CONTROL_TICKS as f32);
            self.control(patch, sh);
        }
        self.ctl -= 1;
        self.glide.tick();

        let (mut xl, mut xr) = (0.0f32, 0.0f32);
        // B first: it's the modulator.
        let table_b = patch.osc[1].table;
        let (mut bl, mut br) = (0.0f32, 0.0f32);
        if patch.osc[1].on || self.fm > 0.0 || self.fm_step != 0.0 {
            self.osc[1].tick(bank, table_b, 0.0, &mut bl, &mut br);
        }
        self.fm += self.fm_step;
        let fm = self.fm * 0.5 * (bl + br);
        if patch.osc[0].on {
            self.osc[0].tick(bank, patch.osc[0].table, fm, &mut xl, &mut xr);
        }
        if patch.osc[1].on {
            xl += bl;
            xr += br;
        }
        if patch.sub > 0.0 {
            let inc = (note_hz(self.glide.value + sh.bend - 12.0) / patch.rate) as f64;
            let s = self.sub.tick(inc, None).0 * patch.sub;
            xl += s;
            xr += s;
        }
        if patch.noise > 0.0 {
            let n = self.noise.white() * patch.noise * patch.noise_scale;
            xl += n;
            xr += n;
        }
        xl *= OSC_GAIN;
        xr *= OSC_GAIN;
        self.cut += self.cut_step;
        let (mut yl, mut yr) = (xl, xr);
        if patch.filter_on {
            if patch.drive > 0.0 {
                let d = 1.0 + patch.drive * 6.0;
                yl = tanh_fast(yl * d);
                yr = tanh_fast(yr * d);
            }
            let g = cutoff_g(note_hz(self.cut.clamp(-24.0, 150.0)), patch.rate);
            yl = self.filter.tick(0, patch.filter, g, self.res, yl);
            yr = self.filter.tick(1, patch.filter, g, self.res, yr);
        }
        let a = self.env[0].tick(&patch.env[0], 1.0);
        let angle = (self.pan + 1.0) * std::f32::consts::FRAC_PI_4;
        // cos/sin only when a slot moves it.
        let (pl, pr) = if self.pan == 0.0 { (1.0, 1.0) } else { (angle.cos() * std::f32::consts::SQRT_2, angle.sin() * std::f32::consts::SQRT_2) };
        *bus_l += yl * a * pl;
        *bus_r += yr * a * pr;
        if !self.env[0].active() {
            self.active = false;
            self.gated = false;
            for e in &mut self.env {
                e.reset();
            }
        }
    }
}

const fn sync_rate(i: usize) -> f64 {
    // Cycles per beat.
    match i {
        1 => 1.0 / 16.0,
        2 => 1.0 / 8.0,
        3 => 1.0 / 4.0,
        4 => 0.5,
        5 => 1.0,
        6 => 2.0,
        7 => 4.0,
        8 => 3.0,
        9 => 6.0,
        _ => 0.0,
    }
}

/// An instance.
pub struct Onda {
    os: usize,
    sr: f32,
    seed: u64,
    values: Box<[f32; params::param::COUNT]>,
    dirty: bool,
    patch: Patch,
    voices: Box<[Voice; VOICES]>,
    alloc: VoiceAllocator<VOICES>,
    bank: &'static Bank,
    wheel: Smoothed,
    touch: Smoothed,
    bend_s: Smoothed,
    expr: Smoothed,
    volume: Smoothed,
    /// LFOs that run for the whole instance.
    lfo: [LfoState; 2],
    lfo_hz: [f32; 2],
    lfo_sync: [usize; 2],
    shared: Shared,
    out: [OutStage; 2],
    dc_r: f32,
    rest: Rest,
    hold_len: u32,
}

impl Onda {
    pub fn new(sample_rate: f32, seed: u64) -> Onda {
        let os = if sample_rate < 64000.0 { 2 } else { 1 };
        let rate = sample_rate * os as f32;
        let patch = Patch {
            rate,
            osc: [OscPatch::default(); 2],
            sub: 0.0,
            noise: 0.0,
            fm: 0.0,
            filter_on: true,
            filter: FilterKind::Low24,
            cutoff_note: 100.0,
            res: 0.0,
            drive: 0.0,
            key_track: 0.0,
            env: [EnvShape::default(); 3],
            slots: [Slot::default(); MOD_SLOTS as usize],
            glide_secs: 0.0,
            noise_scale: 1.0 / (os as f32).sqrt(),
        };
        let voices: Box<[Voice; VOICES]> = Box::new(std::array::from_fn(|i| Voice::new(seed, i, &patch)));
        let mut t = Onda {
            os,
            sr: sample_rate,
            seed,
            values: Box::new(params::defaults()),
            dirty: true,
            patch,
            voices,
            alloc: VoiceAllocator::new(),
            bank: bank(),
            wheel: Smoothed::new(0.0, SMOOTH_MS, sample_rate),
            touch: Smoothed::new(0.0, SMOOTH_MS, sample_rate),
            bend_s: Smoothed::new(0.0, 2.0, sample_rate),
            expr: Smoothed::new(1.0, SMOOTH_MS, sample_rate),
            volume: Smoothed::new(0.0, SMOOTH_MS, sample_rate),
            lfo: [LfoState::new(sub_seed(seed, 71)), LfoState::new(sub_seed(seed, 72))],
            lfo_hz: [1.0; 2],
            lfo_sync: [0; 2],
            shared: Shared::default(),
            out: [OutStage::new(); 2],
            dc_r: dc_pole(DC_HZ, sample_rate),
            rest: Rest::new(),
            hold_len: (HOLD_SECS * sample_rate) as u32,
        };
        t.apply_patch();
        t.volume.snap();
        t
    }

    fn apply_patch(&mut self) {
        self.dirty = false;
        let g = |id: u32| self.values[id as usize];
        let ms = 0.001;
        let rate = self.patch.rate;
        let osc = |first: u32| -> OscPatch {
            let o = |id: u32| g(first + id - A_ON);
            OscPatch {
                on: o(A_ON) >= 0.5,
                table: o(A_TABLE) as usize,
                pos: o(A_POS),
                st: o(A_OCT).round() * 12.0 + o(A_SEMI).round() + o(A_FINE),
                level: o(A_LEVEL),
                pan: o(A_PAN),
                copies: o(A_VOICES) as usize + 1,
                detune: o(A_DETUNE),
                blend: o(A_BLEND),
                spread: o(A_SPREAD),
            }
        };
        self.patch.osc = [osc(A_ON), osc(B_ON)];
        self.patch.sub = g(SUB_LEVEL) * SUB_GAIN;
        self.patch.noise = g(NOISE_LEVEL) * NOISE_GAIN;
        self.patch.fm = g(FM_AMOUNT);
        self.patch.filter_on = g(FILTER_ON) >= 0.5;
        self.patch.filter = match g(FILTER_TYPE) as usize {
            0 => FilterKind::Low12,
            1 => FilterKind::Low24,
            2 => FilterKind::Band12,
            3 => FilterKind::High12,
            4 => FilterKind::Notch,
            5 => FilterKind::Ladder24,
            _ => FilterKind::Diode,
        };
        self.patch.cutoff_note = hz_to_note(g(CUTOFF));
        self.patch.res = g(RESONANCE);
        self.patch.drive = g(DRIVE);
        self.patch.key_track = g(KEY_TRACK);
        let env = |a: u32, d: u32, s: u32, r: u32| EnvShape::new(g(a) * ms, g(d) * ms, g(s), g(r) * ms, rate);
        self.patch.env = [env(E1_ATTACK, E1_DECAY, E1_SUSTAIN, E1_RELEASE), env(E2_ATTACK, E2_DECAY, E2_SUSTAIN, E2_RELEASE), env(E3_ATTACK, E3_DECAY, E3_SUSTAIN, E3_RELEASE)];
        for s in 0..MOD_SLOTS {
            let b = MOD_BASE + 3 * s;
            self.patch.slots[s as usize] = Slot { source: g(b) as usize, dest: g(b + 1) as usize, amount: g(b + 2) };
        }
        let glide = g(GLIDE);
        self.patch.glide_secs = if glide <= 1.0 { 0.0 } else { glide * ms };
        self.lfo_hz = [g(L1_RATE), g(L2_RATE)];
        self.lfo_sync = [g(L1_SYNC) as usize, g(L2_SYNC) as usize];
        self.shared.lfo_wave = [g(L1_WAVE) as usize, g(L2_WAVE) as usize];
        self.shared.lfo_retrig = [g(L1_RETRIG) >= 0.5, g(L2_RETRIG) >= 0.5];
        let l = g(VOLUME) / 10.0;
        self.volume.target = l * l * OUTPUT_GAIN;
        let (mode, poly) = match g(VOICE_MODE) as usize {
            0 => (VoiceMode::Poly, (g(POLYPHONY).round() as usize).clamp(1, VOICES)),
            1 => (VoiceMode::Mono { priority: Priority::Last, multi_trigger: true }, 1),
            _ => (VoiceMode::Mono { priority: Priority::Last, multi_trigger: false }, 1),
        };
        let voices = &mut self.voices;
        self.alloc.set_mode(mode, 1, poly, |a| {
            if let VoiceAction::Release { voice } = a {
                voices[voice].release();
            }
        });
        for v in self.voices.iter_mut() {
            v.glide.set_time(self.patch.glide_secs, rate);
        }
        // A held voice takes a new unison layout at once.
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for o in 0..2 {
                v.osc[o].lay_out(&self.patch.osc[o]);
            }
        }
    }

    fn act(voices: &mut [Voice; VOICES], patch: &Patch, shared: &Shared, action: VoiceAction) {
        match action {
            VoiceAction::Start { voice, note, velocity, stolen, .. } => voices[voice].start(note, velocity, patch, shared, stolen),
            VoiceAction::Glide { voice, note } => voices[voice].glide_to(note),
            VoiceAction::Release { voice } => voices[voice].release(),
        }
    }

    fn handle(&mut self, m: Midi) {
        let (voices, patch, shared) = (&mut self.voices, &self.patch, &self.shared);
        match m {
            Midi::NoteOn { note, velocity } => {
                if self.rest.resting {
                    let gone = self.rest.wake() as f64;
                    // Free LFOs ran on while nothing played.
                    for k in 0..2 {
                        if self.lfo_sync[k] == 0 {
                            let inc = self.lfo_hz[k] as f64 / self.sr as f64;
                            let turned = self.lfo[k].phase + inc * gone;
                            self.lfo[k].phase = turned.fract();
                            if turned >= 1.0 {
                                self.lfo[k].held = self.lfo[k].rng.bipolar();
                            }
                        }
                    }
                }
                self.alloc.note_on(note, velocity, |a| Self::act(voices, patch, shared, a));
            }
            Midi::NoteOff { note } => self.alloc.note_off(note, |a| Self::act(voices, patch, shared, a)),
            Midi::Sustain(down) => self.alloc.set_pedal(down, |a| Self::act(voices, patch, shared, a)),
            Midi::AllNotesOff => self.alloc.release_all(|a| Self::act(voices, patch, shared, a)),
            Midi::PitchBend(b) => self.bend_s.target = b * self.values[BEND_RANGE as usize],
            Midi::ModWheel(w) => self.wheel.target = w,
            Midi::Aftertouch(a) => self.touch.target = a,
            Midi::Expression(e) => self.expr.target = e,
            Midi::ResetControllers => self.reset_controllers(),
            Midi::Other => {}
        }
    }

    fn reset_controllers(&mut self) {
        self.bend_s.target = 0.0;
        self.wheel.target = 0.0;
        self.touch.target = 0.0;
        self.expr.target = 1.0;
    }

    /// Per block: the sync'd LFOs read the song position.
    fn begin_block(&mut self, ctx: &FxContext) {
        for k in 0..2 {
            let per_beat = sync_rate(self.lfo_sync[k]);
            let inc = if per_beat > 0.0 {
                let inc = per_beat * ctx.tempo / 60.0 / self.sr as f64;
                if ctx.playing {
                    let want = (ctx.position * per_beat).rem_euclid(1.0);
                    if want < self.lfo[k].phase && self.lfo[k].phase - want > 0.5 {
                        self.lfo[k].held = self.lfo[k].rng.bipolar();
                    }
                    self.lfo[k].phase = want;
                }
                inc
            } else {
                self.lfo_hz[k] as f64 / self.sr as f64
            };
            // Per voice control tick.
            self.shared.lfo_inc[k] = inc * CONTROL_TICKS as f64 / self.os as f64;
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        if self.rest.resting {
            l.fill(0.0);
            r.fill(0.0);
            self.rest.samples += n as u64;
            self.wheel.snap();
            self.touch.snap();
            self.bend_s.snap();
            self.expr.snap();
            self.volume.snap();
            return;
        }
        let os = self.os;
        // An LFO moves at the rate of the voice's control tick, one per CONTROL_TICKS / os samples.
        let lfo_samples_per = CONTROL_TICKS as f64 / os as f64;
        for i in 0..n {
            self.shared.bend = self.bend_s.tick();
            self.shared.wheel = self.wheel.tick();
            self.shared.touch = self.touch.tick();
            // The instance-wide LFOs, every sample.
            for k in 0..2 {
                self.shared.lfo_global[k] = self.lfo[k].value(self.shared.lfo_wave[k]);
                self.lfo[k].advance(self.shared.lfo_inc[k] / lfo_samples_per);
            }
            let any = self.voices.iter().any(|v| v.active);
            let (mut al, mut ar) = (0.0f32, 0.0f32);
            if os == 2 {
                let (mut l0, mut r0, mut l1, mut r1) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
                if any {
                    for v in self.voices.iter_mut().filter(|v| v.active) {
                        v.tick(&self.patch, &self.shared, self.bank, &mut l0, &mut r0);
                    }
                    for v in self.voices.iter_mut().filter(|v| v.active) {
                        v.tick(&self.patch, &self.shared, self.bank, &mut l1, &mut r1);
                    }
                }
                al = self.out[0].dec.process(l0, l1);
                ar = self.out[1].dec.process(r0, r1);
            } else if any {
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.tick(&self.patch, &self.shared, self.bank, &mut al, &mut ar);
                }
            }
            let g = self.volume.tick() * self.expr.tick();
            l[i] = self.out[0].block_dc(al, self.dc_r) * g;
            r[i] = self.out[1].block_dc(ar, self.dc_r) * g;
            for (k, v) in self.voices.iter().enumerate() {
                if !v.active && self.alloc.is_busy(k) {
                    self.alloc.voice_finished(k);
                }
            }
            if any {
                self.rest.hold = self.hold_len;
            } else {
                self.rest.hold = self.rest.hold.saturating_sub(1);
                if self.rest.hold == 0 {
                    self.rest.resting = true;
                    self.out = [OutStage::new(); 2];
                    for v in self.voices.iter_mut() {
                        v.filter = {
                            let mut f = FilterPair::default();
                            let hp = cutoff_g(120.0, self.patch.rate);
                            f.diode[0].set_feedback_highpass(hp);
                            f.diode[1].set_feedback_highpass(hp);
                            f
                        };
                    }
                    l[i + 1..].fill(0.0);
                    r[i + 1..].fill(0.0);
                    self.rest.samples += (n - i - 1) as u64;
                    return;
                }
            }
        }
    }
}

impl Instrument for Onda {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.values.get_mut(id as usize) else { return };
        *slot = value;
        self.dirty = true;
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], midi: &[MidiEvent], ctx: &FxContext) {
        if self.dirty {
            self.apply_patch();
        }
        self.begin_block(ctx);
        let n = l.len().min(r.len());
        let mut at = 0;
        for e in midi {
            let to = (e.offset as usize).clamp(at, n);
            self.render(&mut l[at..to], &mut r[at..to]);
            at = to;
            self.handle(Midi::parse(e.data));
        }
        self.render(&mut l[at..n], &mut r[at..n]);
    }

    fn all_notes_off(&mut self) {
        let (voices, patch, shared) = (&mut self.voices, &self.patch, &self.shared);
        self.alloc.release_all(|a| Self::act(voices, patch, shared, a));
    }

    fn reset(&mut self) {
        if self.dirty {
            self.apply_patch();
        }
        let (seed, patch) = (self.seed, self.patch);
        for (i, v) in self.voices.iter_mut().enumerate() {
            *v = Voice::new(seed, i, &patch);
            v.glide.set_time(patch.glide_secs, patch.rate);
        }
        self.alloc.reset();
        self.lfo = [LfoState::new(sub_seed(seed, 71)), LfoState::new(sub_seed(seed, 72))];
        self.out = [OutStage::new(); 2];
        self.rest = Rest::new();
        self.reset_controllers();
        self.wheel.snap();
        self.touch.snap();
        self.bend_s.snap();
        self.expr.snap();
        self.volume.snap();
    }

    fn tail_secs(&self) -> f32 {
        (self.values[E1_RELEASE as usize] * 0.001 * 3.84).min(60.0) + HOLD_SECS + 0.05
    }
}

pub fn info() -> InstrumentInfo {
    InstrumentInfo {
        id: "builtin:onda",
        name: "Fermata Onda",
        group: "Modern Synths",
        about: "Two wavetable oscillators with position scan and unison, seven filters, three envelopes, two LFOs and a modulation matrix",
        params: PARAMS,
        sections: SECTIONS,
        meters: &[],
        presets: PRESETS,
        new: |sample_rate, seed| Box::new(Onda::new(sample_rate, seed)),
    }
}

#[cfg(test)]
mod tests;
