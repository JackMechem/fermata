//! Fermata Sedici: six-operator FM with sixteen voices, modelled on the architecture and behaviour
//! of the Yamaha DX7 (this project isn't affiliated with Yamaha, and the product name stays clear
//! of theirs). Its reference is what's publicly documented about the instrument and the published
//! reverse engineering of its chips; nothing here was compared with the hardware. See
//! docs/ARCHITECTURE.md.
//!
//! **The engine runs at the instrument's own rate, 49.096 kHz, whatever the session's is.** That's
//! part of its sound: FM makes partials past that rate's Nyquist, and on this instrument they fold.
//! A session at 96 kHz gets the same folding as one at 44.1, because the result comes through one
//! resampler at the output, which adds no aliasing of its own in the audio band.
//!
//! **Signal path:** per voice, six sine operators, in one of the 32 algorithms, with one feedback
//! loop. Envelopes, pitch and LFO update per 64 ticks with the operator gains ramped between, as the
//! chips do. Then a mono output with an (optional) 12-bit floating converter's quantization.
//!
//! **The grit:** with Converter Grit on, an operator's output is 14 bits, the sine table's 4096 points
//! aren't interpolated, and the output is quantized to a 12-bit mantissa with an 8-step exponent.
//! That's what's documented of the chips and the converter. Off, it's clean in all three.
//!
//! **Changed from the hardware on purpose:** it clicks when a voice with key sync is stolen; this
//! fades 2 ms first, as the instrument contract asks. A voice that has reached its fourth level at
//! above zero drones until the transport stops (the hardware's would go on till stolen).

use std::sync::OnceLock;

use vst3_host::MidiEvent;

use super::common::{dc_pole, OutStage, Rest};
use super::{FxContext, Instrument, InstrumentInfo, Midi};
use crate::synth_dsp::alloc::{Priority, VoiceAction, VoiceAllocator, VoiceMode};
use crate::synth_dsp::analog::{sub_seed, Rng};
use crate::synth_dsp::pow2;
use crate::synth_dsp::smooth::Smoothed;

pub mod algorithms;
pub mod env;
pub mod params;
mod presets;

use algorithms::{Algorithm, ALGORITHMS};
use env::{DxEnv, TOP};
use params::param::*;
pub use params::{PARAMS, SECTIONS};
pub use presets::PRESETS;

pub const VOICES: usize = 16;
/// The instrument's sample rate (its 9.4265 MHz clock over 192). Published in the reverse
/// engineering of its chips.
pub const DX_RATE: f64 = 49096.0;
const BLOCK: u32 = 64;
const SIN_BITS: u32 = 12;
const SIN_SIZE: usize = 1 << SIN_BITS;
/// A voice stolen with key sync fades this many ticks first.
const FADE_TICKS: u32 = 98;
/// One carrier at the top of its level, at the output, before the volume knob.
const CARRIER_GAIN: f32 = 0.25;
/// An operator's output at the top of its level, as modulation, in cycles (a published figure:
/// modulation index 4π at level 99).
const MOD_CYCLES: f32 = 2.0;
/// A voice is done when its carriers are this low.
const END_GAIN: f32 = 2e-5;
/// Attenuation at full amp-mod sensitivity and depth, in level units: a guess at about 48 dB.
const AM_RANGE: f32 = 2048.0;
/// The published sensitivity tables.
const PMS: [f32; 8] = [0.0, 10.0, 20.0, 33.0, 55.0, 92.0, 158.0, 255.0];
const AMS: [f32; 4] = [0.0, 66.0, 109.0, 255.0];
/// The vibrato's reach at full depth and sensitivity. A guess: the panel doesn't say.
const PM_RANGE_ST: f32 = 12.0;
/// The resampler's size.
const TAPS: usize = 48;
const PHASES: usize = 256;
const HOLD_SECS: f32 = 0.01;
/// Where the converter runs out, against a carrier at the top of its level (`CARRIER_GAIN`): six
/// of them together would pass this. It clips rather than wraps: a guess.
const CONVERTER_TOP: f32 = 1.5;

/// The sine table, once. Read-only after it's made; made from the constructor, not the audio
/// thread.
fn sine_table() -> &'static [f32; SIN_SIZE + 1] {
    static TABLE: OnceLock<Box<[f32; SIN_SIZE + 1]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = Box::new([0.0f32; SIN_SIZE + 1]);
        for (i, v) in t.iter_mut().enumerate() {
            *v = (std::f64::consts::TAU * i as f64 / SIN_SIZE as f64).sin() as f32;
        }
        t
    })
}

/// What an operator takes from the panel, worked out once per change.
#[derive(Debug, Clone, Copy, Default)]
struct OpPatch {
    on: bool,
    level: i32,
    fixed: bool,
    coarse: i32,
    fine: i32,
    detune: i32,
    vel: i32,
    ams: f32,
    rate_scale: i32,
    break_point: i32,
    l_depth: i32,
    r_depth: i32,
    l_curve: i32,
    r_curve: i32,
    rates: [i32; 4],
    levels: [i32; 4],
}

impl OpPatch {
    /// Multiplier on the note's frequency, or (fixed) a frequency in Hz.
    fn ratio(&self) -> f32 {
        if self.fixed {
            // 1, 10, 100 or 1000 Hz, times up to nearly 10.
            10f32.powi(self.coarse & 3) * 10f32.powf(self.fine as f32 / 100.0)
        } else {
            let c = if self.coarse == 0 { 0.5 } else { self.coarse as f32 };
            c * (1.0 + self.fine as f32 / 100.0)
        }
    }

    /// Its level scale for a key and a velocity.
    fn out_level(&self, note: i32, velocity: f32) -> i32 {
        let level = env::scale_out_level(self.level) + env::keyboard_level_scale(note, self.break_point, self.l_depth, self.r_depth, self.l_curve, self.r_curve);
        ((level.min(127) << 5) + env::velocity_scale(velocity, self.vel)).max(0)
    }

    /// The operator's frequency at a note, before pitch modulation. Detune is a small step, larger
    /// at the bottom of the keyboard: the published fit.
    fn hz(&self, note: f32) -> f32 {
        if self.fixed {
            return self.ratio();
        }
        let lf = note / 12.0 + 3.03;
        let step = 0.0209 * (-0.396 * lf).exp() / 7.0 * lf;
        crate::synth_dsp::note_hz(note) * self.ratio() * pow2(step * self.detune as f32)
    }
}

/// All the panel, in the voice's terms.
#[derive(Debug, Clone, Copy)]
struct Patch {
    ops: [OpPatch; 6],
    algo: Algorithm,
    /// Cycles of phase per unit of a feedback output.
    fb_gain: f32,
    grit: bool,
    key_sync: bool,
    pitch_rates: [f32; 4],
    pitch_levels: [f32; 4],
    glide_secs: f32,
    transpose: f32,
}

/// One operator in one voice.
#[derive(Debug, Clone, Copy, Default)]
struct Op {
    phase: u32,
    inc: u32,
    gain: f32,
    gain_step: f32,
    env: DxEnv,
    /// Its frequency before pitch modulation.
    hz: f32,
}

/// The voice's pitch envelope.
#[derive(Debug, Clone, Copy, Default)]
struct PitchEnv {
    st: f32,
    ix: usize,
    down: bool,
}

struct Voice {
    active: bool,
    gated: bool,
    note: f32,
    target_note: f32,
    velocity: f32,
    key: u8,
    ops: [Op; 6],
    out: [f32; 6],
    fb: [f32; 2],
    pitch_env: PitchEnv,
    block_left: u32,
    fade: u32,
    pending: Option<(u8, f32)>,
    pending_off: bool,
    /// Stops at its fourth level rather than droning on: the transport stopped.
    force_end: bool,
    force_gain: f32,
}

impl Voice {
    fn new(seed: u64, index: usize) -> Voice {
        let mut rng = Rng::new(sub_seed(sub_seed(seed, index as u64), 11));
        let mut ops = [Op::default(); 6];
        for o in &mut ops {
            o.phase = rng.next_u64() as u32;
        }
        Voice {
            active: false,
            gated: false,
            note: 60.0,
            target_note: 60.0,
            velocity: 1.0,
            key: 60,
            ops,
            out: [0.0; 6],
            fb: [0.0; 2],
            pitch_env: PitchEnv::default(),
            block_left: 0,
            fade: 0,
            pending: None,
            pending_off: false,
            force_end: false,
            force_gain: 1.0,
        }
    }

    /// Starts the note: the operators take their levels and their envelopes start from wherever
    /// they are.
    fn begin(&mut self, note: u8, velocity: f32, patch: &Patch, glide_from: Option<f32>) {
        self.gated = true;
        self.active = true;
        self.velocity = velocity;
        self.force_end = false;
        self.force_gain = 1.0;
        let n = note as f32 + patch.transpose;
        self.target_note = n;
        self.note = glide_from.unwrap_or(n);
        self.key = note;
        for (o, p) in self.ops.iter_mut().zip(&patch.ops) {
            let nn = note as i32;
            o.env.rates = p.rates;
            o.env.levels = p.levels;
            o.env.out_level = p.out_level(nn, velocity);
            o.env.rate_scale = env::keyboard_rate_scale(nn, p.rate_scale);
            o.env.key_on();
            if patch.key_sync {
                o.phase = 0;
            }
        }
        self.pitch_env.ix = 0;
        self.pitch_env.down = true;
        if patch.key_sync {
            self.fb = [0.0; 2];
        }
        // The envelopes' next block starts at once.
        self.block_left = 0;
    }

    fn start(&mut self, note: u8, velocity: f32, stolen: bool, patch: &Patch) {
        let was_on = self.active;
        if stolen && was_on && patch.key_sync {
            // Fade the old note out for 2 ms, then jump the phases where nobody can hear them.
            self.pending = Some((note, velocity));
            self.pending_off = false;
            if self.fade == 0 {
                self.fade = FADE_TICKS;
            }
        } else {
            let from = if patch.glide_secs > 0.0 && self.note > 0.0 && was_on { Some(self.note) } else { None };
            self.begin(note, velocity, patch, from);
        }
    }

    /// The panel changed under this voice.
    fn refresh(&mut self, patch: &Patch) {
        let nn = self.key as i32;
        for (o, p) in self.ops.iter_mut().zip(&patch.ops) {
            o.env.retarget(p.rates, p.levels, p.out_level(nn, self.velocity), env::keyboard_rate_scale(nn, p.rate_scale));
        }
    }

    fn glide_to(&mut self, note: u8, patch: &Patch) {
        self.target_note = note as f32 + patch.transpose;
    }

    fn release(&mut self) {
        if self.pending.is_some() {
            self.pending_off = true;
            return;
        }
        self.gated = false;
        for o in &mut self.ops {
            o.env.key_off();
        }
        self.pitch_env.down = false;
        if self.pitch_env.ix < 3 {
            self.pitch_env.ix = 3;
        }
    }

    /// Per 64 ticks: the envelopes, pitch, and each operator's next gain and rate.
    fn control(&mut self, patch: &Patch, c: &Control) {
        // Portamento: a steady fraction of the way per block.
        if self.note != self.target_note {
            if patch.glide_secs <= 0.0 {
                self.note = self.target_note;
            } else {
                let k = 1.0 - (-3.0 * BLOCK as f32 / (patch.glide_secs * DX_RATE as f32)).exp();
                self.note += (self.target_note - self.note) * k;
                if (self.target_note - self.note).abs() < 1e-3 {
                    self.note = self.target_note;
                }
            }
        }
        // The pitch envelope, to the next level at its rate.
        let e = &mut self.pitch_env;
        if e.ix < 3 || (e.ix < 4 && !e.down) {
            let target = patch.pitch_levels[e.ix];
            // Octaves a second to semitones a block.
            let step = patch.pitch_rates[e.ix] * 12.0 * BLOCK as f32 / DX_RATE as f32;
            if (target - e.st).abs() <= step {
                e.st = target;
                e.ix += 1;
            } else {
                e.st += step * (target - e.st).signum();
            }
        }
        let pitch = c.pitch_st + self.pitch_env.st;
        let ratio = pow2(pitch / 12.0);
        let mut carriers_done = true;
        for (i, (o, p)) in self.ops.iter_mut().zip(&patch.ops).enumerate() {
            o.hz = p.hz(self.note);
            let hz = if p.fixed { o.hz } else { o.hz * ratio };
            o.inc = (hz as f64 / DX_RATE * 4_294_967_296.0) as u32;
            let level = o.env.block();
            // Amp mod takes its level down, as attenuation in the same log scale.
            let l = level - p.ams * c.am_units;
            let target = if p.on { pow2((l - TOP) / 256.0) } else { 0.0 };
            let target = if l <= 16.0 { 0.0 } else { target };
            o.gain_step = (target - o.gain) / BLOCK as f32;
            if patch.algo.carriers & (1 << i) != 0 && p.on && (!o.env.finished() || o.gain > END_GAIN || target > END_GAIN) {
                carriers_done = false;
            }
        }
        if !self.gated && carriers_done && self.pending.is_none() {
            self.active = false;
            for o in &mut self.ops {
                o.gain = 0.0;
                o.gain_step = 0.0;
            }
            self.out = [0.0; 6];
            self.fb = [0.0; 2];
        }
    }

    /// One tick at the instrument's rate. Returns what it adds to the output.
    #[inline]
    fn tick(&mut self, patch: &Patch, c: &Control, table: &[f32; SIN_SIZE + 1]) -> f32 {
        if self.block_left == 0 {
            self.block_left = BLOCK;
            self.control(patch, c);
            if !self.active {
                return 0.0;
            }
        }
        self.block_left -= 1;
        let a = &patch.algo;
        let mut sum = 0.0f32;
        let grit = patch.grit;
        let fb = (self.fb[0] + self.fb[1]) * 0.5 * patch.fb_gain;
        for i in (0..6).rev() {
            let o = &mut self.ops[i];
            o.gain += o.gain_step;
            let mut m = 0.0f32;
            for k in 0..a.mod_count[i] as usize {
                m += self.out[a.mods[i][k] as usize];
            }
            m *= MOD_CYCLES;
            if a.fb_to as usize == i {
                m += fb;
            }
            let phase = o.phase.wrapping_add(((m * 16_777_216.0) as i32 as u32).wrapping_shl(8));
            o.phase = o.phase.wrapping_add(o.inc);
            let idx = (phase >> (32 - SIN_BITS)) as usize;
            let s = if grit {
                table[idx]
            } else {
                let frac = (phase & ((1 << (32 - SIN_BITS)) - 1)) as f32 / (1u32 << (32 - SIN_BITS)) as f32;
                table[idx] + (table[idx + 1] - table[idx]) * frac
            };
            let mut y = s * o.gain;
            if grit {
                // An operator's 14 bits.
                y = (y * 8192.0) as i32 as f32 / 8192.0;
            }
            self.out[i] = y;
            if a.carriers & (1 << i) != 0 {
                sum += y;
            }
        }
        // The loop reads the operator it's from, after this tick.
        self.fb[1] = self.fb[0];
        self.fb[0] = self.out[a.fb_from as usize];
        let mut y = sum;
        if self.fade > 0 {
            y *= self.fade as f32 / FADE_TICKS as f32;
            self.fade -= 1;
            if self.fade == 0 {
                if let Some((note, velocity)) = self.pending.take() {
                    self.begin(note, velocity, patch, None);
                    if self.pending_off {
                        self.pending_off = false;
                        self.release();
                    }
                }
            }
        }
        if self.force_end {
            self.force_gain *= 0.9997;
            y *= self.force_gain;
            if self.force_gain < 1e-3 {
                self.active = false;
            }
        }
        y
    }
}

/// What the instance works out per 64 ticks for every voice.
#[derive(Debug, Clone, Copy, Default)]
struct Control {
    pitch_st: f32,
    am_units: f32,
}

/// The one LFO.
#[derive(Debug, Clone, Copy)]
struct DxLfo {
    phase: f64,
    held: f32,
    rng: Rng,
    delay: f32,
}

impl DxLfo {
    fn new(seed: u64) -> DxLfo {
        DxLfo { phase: 0.0, held: 0.0, rng: Rng::new(sub_seed(seed, 55)), delay: 1.0 }
    }

    /// One value per 64 ticks, -1..1.
    fn tick(&mut self, wave: i32, inc: f64, delay_inc: f32) -> f32 {
        let p = self.phase as f32;
        let v = match wave {
            0 => 1.0 - 4.0 * (p - 0.5).abs(),
            1 => 1.0 - 2.0 * p,
            2 => 2.0 * p - 1.0,
            3 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            4 => (p * std::f32::consts::TAU).sin(),
            _ => self.held,
        };
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.held = self.rng.bipolar();
        }
        self.delay = (self.delay + delay_inc).min(1.0);
        v * self.delay
    }
}

/// Brings the instrument's rate to the session's: a Kaiser-windowed sinc, its corner under both
/// rates' Nyquist, per phase and interpolated between phases.
struct Resampler {
    table: Vec<f32>,
    ring: [f32; TAPS],
    w: usize,
    /// How far into the next input tick the output sample lies, 0..1.
    pos: f64,
    step: f64,
}

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0f64, 1.0f64);
    for k in 1..40 {
        term *= (x / (2.0 * k as f64)) * (x / (2.0 * k as f64));
        sum += term;
    }
    sum
}

impl Resampler {
    fn new(sample_rate: f32) -> Resampler {
        let host = sample_rate as f64;
        // In cycles per tick of the instrument's rate.
        let corner = (0.45 * host.min(DX_RATE)) / DX_RATE;
        let beta = 7.0;
        let half = TAPS as f64 / 2.0;
        let mut table = vec![0.0f32; (PHASES + 1) * TAPS];
        for p in 0..=PHASES {
            let frac = p as f64 / PHASES as f64;
            for t in 0..TAPS {
                // Tap t is `t + 1 - half` ticks before... the ring's newest is at the end, and the
                // output lies `frac` of a tick ahead of the middle.
                let x = t as f64 - half + 1.0 - frac;
                let s = if x.abs() < 1e-9 { 2.0 * corner } else { (std::f64::consts::TAU * corner * x).sin() / (std::f64::consts::PI * x) };
                let r = (x / half).clamp(-1.0, 1.0);
                let w = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta);
                table[p * TAPS + t] = (s * w) as f32;
            }
            // Unity at DC at every phase.
            let row = &mut table[p * TAPS..(p + 1) * TAPS];
            let sum: f32 = row.iter().sum();
            for v in row.iter_mut() {
                *v /= sum;
            }
        }
        Resampler { table, ring: [0.0; TAPS], w: 0, pos: 0.0, step: DX_RATE / host }
    }

    #[inline]
    fn push(&mut self, x: f32) {
        self.ring[self.w] = x;
        self.w = (self.w + 1) % TAPS;
    }

    #[inline]
    fn read(&self) -> f32 {
        let at = self.pos * PHASES as f64;
        let p = at as usize;
        let f = (at - p as f64) as f32;
        let (a, b) = (&self.table[p * TAPS..(p + 1) * TAPS], &self.table[(p + 1) * TAPS..(p + 2) * TAPS]);
        let mut sum = 0.0f32;
        for t in 0..TAPS {
            let v = self.ring[(self.w + t) % TAPS];
            sum += v * (a[t] + (b[t] - a[t]) * f);
        }
        sum
    }

    fn clear(&mut self) {
        self.ring = [0.0; TAPS];
        self.w = 0;
        self.pos = 0.0;
    }
}

/// An instance.
pub struct Sedici {
    sr: f32,
    seed: u64,
    values: Box<[f32; params::param::COUNT]>,
    dirty: bool,
    patch: Patch,
    voices: Box<[Voice; VOICES]>,
    alloc: VoiceAllocator<VOICES>,
    table: &'static [f32; SIN_SIZE + 1],
    lfo: DxLfo,
    lfo_inc: f64,
    lfo_delay_inc: f32,
    lfo_wave: i32,
    lfo_pmd: f32,
    lfo_amd: f32,
    pms: f32,
    wheel_pitch: f32,
    wheel_amp: f32,
    touch_pitch: f32,
    touch_amp: f32,
    control: Control,
    block_left: u32,
    bend: f32,
    wheel: f32,
    touch: f32,
    expression: Smoothed,
    volume: Smoothed,
    res: Resampler,
    out: OutStage,
    dc_r: f32,
    rest: Rest,
    hold_len: u32,
}

impl Sedici {
    pub fn new(sample_rate: f32, seed: u64) -> Sedici {
        let values: Box<[f32; params::param::COUNT]> = Box::new(params::defaults());
        let patch = Patch {
            ops: [OpPatch::default(); 6],
            algo: ALGORITHMS[0],
            fb_gain: 0.0,
            grit: true,
            key_sync: true,
            pitch_rates: [99.0; 4],
            pitch_levels: [0.0; 4],
            glide_secs: 0.0,
            transpose: 0.0,
        };
        let voices: Box<[Voice; VOICES]> = Box::new(std::array::from_fn(|i| Voice::new(seed, i)));
        let mut t = Sedici {
            sr: sample_rate,
            seed,
            values,
            dirty: true,
            patch,
            voices,
            alloc: VoiceAllocator::new(),
            table: sine_table(),
            lfo: DxLfo::new(seed),
            lfo_inc: 0.0,
            lfo_delay_inc: 1.0,
            lfo_wave: 0,
            lfo_pmd: 0.0,
            lfo_amd: 0.0,
            pms: 0.0,
            wheel_pitch: 0.0,
            wheel_amp: 0.0,
            touch_pitch: 0.0,
            touch_amp: 0.0,
            control: Control::default(),
            block_left: 0,
            bend: 0.0,
            wheel: 0.0,
            touch: 0.0,
            expression: Smoothed::new(1.0, 6.0, sample_rate),
            volume: Smoothed::new(0.0, 6.0, sample_rate),
            res: Resampler::new(sample_rate),
            out: OutStage::new(),
            dc_r: dc_pole(6.0, sample_rate),
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
        let gi = |id: u32| g(id).round() as i32;
        let base = |n: usize| O1 + n as u32 * OP_STRIDE;
        let mut ops = [OpPatch::default(); 6];
        for (n, o) in ops.iter_mut().enumerate() {
            let b = base(n);
            *o = OpPatch {
                on: g(b + ON) >= 0.5,
                level: gi(b + LEVEL),
                fixed: g(b + MODE) >= 0.5,
                coarse: gi(b + COARSE),
                fine: gi(b + FINE),
                detune: gi(b + DETUNE),
                vel: gi(b + VEL),
                ams: AMS[gi(b + AMS_FIELD) as usize & 3] / 255.0,
                rate_scale: gi(b + RATE_SCALE),
                break_point: gi(b + BREAK),
                l_depth: gi(b + L_DEPTH),
                r_depth: gi(b + R_DEPTH),
                l_curve: gi(b + L_CURVE),
                r_curve: gi(b + R_CURVE),
                rates: [gi(b + R1), gi(b + R2), gi(b + R3), gi(b + R4)],
                levels: [gi(b + L1), gi(b + L2), gi(b + L3), gi(b + L4)],
            };
        }
        let fb = gi(FEEDBACK).clamp(0, 7);
        let algo = ALGORITHMS[(gi(ALGORITHM) - 1).clamp(0, 31) as usize];
        // Porta on the panel's 0..99, to a time; a fit to how it's described, slow at the top.
        let porta = g(PORTA);
        let glide_secs = if porta < 0.5 { 0.0 } else { 0.005 * (0.075 * porta).exp() };
        let rate_of = |id: u32| env::pitch_rate_oct_per_sec(g(id));
        self.patch = Patch {
            ops,
            algo,
            // At 7, one op's full swing is one cycle either way. Each step down halves it, and 0
            // is off.
            fb_gain: if fb == 0 { 0.0 } else { 2f32.powi(fb - 7) },
            grit: g(GRIT) >= 0.5,
            key_sync: g(KEY_SYNC) >= 0.5,
            pitch_rates: [rate_of(PR1), rate_of(PR2), rate_of(PR3), rate_of(PR4)],
            pitch_levels: [env::pitch_level_st(g(PL1)), env::pitch_level_st(g(PL2)), env::pitch_level_st(g(PL3)), env::pitch_level_st(g(PL4))],
            glide_secs,
            transpose: g(TRANSPOSE).round(),
        };
        self.lfo_inc = env::lfo_hz(gi(LFO_SPEED)) as f64 * BLOCK as f64 / DX_RATE;
        let d = g(LFO_DELAY);
        let delay_secs = if d < 0.5 { 0.0 } else { 0.02 * (0.065 * d).exp() };
        self.lfo_delay_inc = if delay_secs > 0.0 { (BLOCK as f64 / (delay_secs as f64 * DX_RATE)) as f32 } else { 1.0 };
        self.lfo_wave = gi(LFO_WAVE);
        self.lfo_pmd = g(LFO_PMD) / 99.0;
        self.lfo_amd = g(LFO_AMD) / 99.0;
        self.pms = PMS[gi(LFO_PMS) as usize & 7] / 255.0;
        self.wheel_pitch = g(WHEEL_PITCH) / 99.0;
        self.wheel_amp = g(WHEEL_AMP) / 99.0;
        self.touch_pitch = g(TOUCH_PITCH) / 99.0;
        self.touch_amp = g(TOUCH_AMP) / 99.0;
        let l = g(VOLUME) / 10.0;
        self.volume.target = l * l * CARRIER_GAIN * 2.0;
        let mode = if g(POLY) >= 0.5 { VoiceMode::Mono { priority: Priority::Last, multi_trigger: false } } else { VoiceMode::Poly };
        let polyphony = if g(POLY) >= 0.5 { 1 } else { VOICES };
        let voices = &mut self.voices;
        self.alloc.set_mode(mode, 1, polyphony, |a| {
            if let VoiceAction::Release { voice } = a {
                voices[voice].release();
            }
        });
        // Whatever's sounding hears it too: a knob or a lane moves the note it's playing.
        for v in self.voices.iter_mut().filter(|v| v.active && v.pending.is_none()) {
            v.refresh(&self.patch);
        }
    }

    fn act(voices: &mut [Voice; VOICES], patch: &Patch, action: VoiceAction) {
        match action {
            VoiceAction::Start { voice, note, velocity, stolen, .. } => voices[voice].start(note, velocity, stolen, patch),
            VoiceAction::Glide { voice, note } => voices[voice].glide_to(note, patch),
            VoiceAction::Release { voice } => voices[voice].release(),
        }
    }

    fn handle(&mut self, m: Midi) {
        match m {
            Midi::NoteOn { note, velocity } => {
                if self.rest.resting {
                    let gone = self.rest.wake();
                    // Whole blocks of the LFO went by.
                    let blocks = (gone as f64 / self.sr as f64 * DX_RATE) / BLOCK as f64;
                    self.lfo.phase = (self.lfo.phase + blocks * self.lfo_inc).fract();
                }
                if self.values[LFO_SYNC as usize] >= 0.5 {
                    self.lfo.phase = 0.0;
                }
                // The delay runs from the first key down.
                if !self.voices.iter().any(|v| v.gated) {
                    self.lfo.delay = 0.0;
                }
                let (voices, patch) = (&mut self.voices, &self.patch);
                self.alloc.note_on(note, velocity, |a| Self::act(voices, patch, a));
            }
            Midi::NoteOff { note } => {
                let (voices, patch) = (&mut self.voices, &self.patch);
                self.alloc.note_off(note, |a| Self::act(voices, patch, a));
            }
            Midi::Sustain(down) => {
                let (voices, patch) = (&mut self.voices, &self.patch);
                self.alloc.set_pedal(down, |a| Self::act(voices, patch, a));
            }
            Midi::AllNotesOff => self.let_go(),
            Midi::PitchBend(b) => self.bend = b * self.values[BEND_RANGE as usize],
            Midi::ModWheel(w) => self.wheel = w,
            Midi::Aftertouch(a) => self.touch = a,
            Midi::Expression(e) => self.expression.target = e,
            Midi::ResetControllers => self.reset_controllers(),
            Midi::Other => {}
        }
    }

    fn let_go(&mut self) {
        let (voices, patch) = (&mut self.voices, &self.patch);
        self.alloc.release_all(|a| Self::act(voices, patch, a));
        // Nothing drones past a stop.
        for v in self.voices.iter_mut() {
            if v.active {
                v.force_end = v.ops.iter().any(|o| o.env.levels[3] > 0);
            }
        }
    }

    fn reset_controllers(&mut self) {
        self.bend = 0.0;
        self.wheel = 0.0;
        self.touch = 0.0;
        self.expression.target = 1.0;
    }

    /// One tick at the instrument's rate.
    #[inline]
    fn engine_tick(&mut self) -> f32 {
        if self.block_left == 0 {
            self.block_left = BLOCK;
            let lfo = self.lfo.tick(self.lfo_wave, self.lfo_inc, self.lfo_delay_inc);
            let vib = (self.lfo_pmd + self.wheel * self.wheel_pitch + self.touch * self.touch_pitch).min(1.0) * self.pms * PM_RANGE_ST * lfo;
            let trem = (self.lfo_amd + self.wheel * self.wheel_amp + self.touch * self.touch_amp).min(1.0) * (0.5 - 0.5 * lfo);
            self.control = Control { pitch_st: self.bend + vib, am_units: trem * AM_RANGE };
        }
        self.block_left -= 1;
        let (table, control) = (self.table, self.control);
        let mut sum = 0.0f32;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            sum += v.tick(&self.patch, &control, table);
        }
        sum
    }

    /// The 12-bit mantissa and 8-step exponent of the instrument's converter.
    #[inline]
    fn quantize(x: f32) -> f32 {
        const TOP_OF_STEPS: f32 = 1.0 / 128.0;
        if x.abs() < TOP_OF_STEPS {
            // Below its smallest exponent it's plain fixed point.
            return (x * 262_144.0).round() / 262_144.0;
        }
        f32::from_bits(x.to_bits() & 0xFFFF_E000)
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
            self.volume.snap();
            self.expression.snap();
            return;
        }
        let grit = self.patch.grit;
        for i in 0..n {
            let any = self.voices.iter().any(|v| v.active);
            self.res.pos += self.res.step;
            while self.res.pos >= 1.0 {
                self.res.pos -= 1.0;
                // Scaled at the converter: a voice stack's level is what it quantizes.
                let x = (self.engine_tick() * CARRIER_GAIN).clamp(-CONVERTER_TOP, CONVERTER_TOP);
                self.res.push(if grit { Self::quantize(x) } else { x });
            }
            let y = self.res.read() * 4.0;
            let y = self.out.block_dc(y, self.dc_r) * self.volume.tick() * self.expression.tick();
            l[i] = y;
            r[i] = y;
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
                    self.res.clear();
                    self.out = OutStage::new();
                    self.block_left = 0;
                    l[i + 1..].fill(0.0);
                    r[i + 1..].fill(0.0);
                    self.rest.samples += (n - i - 1) as u64;
                    return;
                }
            }
        }
    }
}

impl Instrument for Sedici {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.values.get_mut(id as usize) else { return };
        *slot = value;
        self.dirty = true;
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], midi: &[MidiEvent], _ctx: &FxContext) {
        if self.dirty {
            self.apply_patch();
        }
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
        self.let_go();
    }

    fn reset(&mut self) {
        if self.dirty {
            self.apply_patch();
        }
        let seed = self.seed;
        for (i, v) in self.voices.iter_mut().enumerate() {
            *v = Voice::new(seed, i);
        }
        self.alloc.reset();
        self.lfo = DxLfo::new(seed);
        self.control = Control::default();
        self.block_left = 0;
        self.reset_controllers();
        self.expression.snap();
        self.volume.snap();
        self.res.clear();
        self.out = OutStage::new();
        self.rest = Rest::new();
    }

    fn tail_secs(&self) -> f32 {
        // The slowest release among the carriers.
        let g = |id: u32| self.values[id as usize];
        let a = &self.patch.algo;
        let mut t = 0.0f32;
        for n in 0..6u32 {
            if a.carriers & (1 << n) != 0 {
                t = t.max(env::release_secs(g(O1 + n * OP_STRIDE + R4).round() as i32));
            }
        }
        t.min(60.0) + 0.1
    }
}

pub fn info() -> InstrumentInfo {
    InstrumentInfo {
        id: "builtin:sedici",
        name: "Fermata Sedici",
        group: "Vintage Synths",
        about: "Six-operator FM, 32 algorithms, 16 voices, at the original's own sample rate",
        params: PARAMS,
        sections: SECTIONS,
        meters: &[],
        presets: PRESETS,
        new: |sample_rate, seed| Box::new(Sedici::new(sample_rate, seed)),
    }
}

#[cfg(test)]
mod tests;
