//! Fermata Uno: a one-oscillator bass line synth, modelled on the architecture and behaviour of
//! the Roland TB-303 (this project isn't affiliated with Roland, and the product name stays clear
//! of theirs). Its reference is what's publicly documented about the circuit, Robin Whittle's and
//! Tim Stinchcombe's analyses above all; nothing here was compared with the hardware. See
//! docs/ARCHITECTURE.md.
//!
//! **Signal path, at twice the output rate below 64 kHz:** one oscillator (saw, or a square shaped
//! from it) into a diode ladder, then a VCA. Two envelopes, neither with a knob of its own bar
//! Decay: the Main one (MEG) decays the filter, always to nothing, whether the key is up or not; the
//! Volume one decays the VCA over seconds, and the key coming up closes it fast.
//!
//! **How a part maps onto it from a MIDI region (there's no sequencer in the instrument):**
//! - A note is a step. Notes that **overlap** slide: the second is reached by the slide lag, and
//!   neither envelope restarts, as the hardware's tied steps do. A gap, however short, restarts them
//!   and jumps the pitch.
//! - A note with velocity at or above **Accent at Velocity** is accented. A slid-to note can be
//!   accented too: it sets the accent without restarting the envelope, like an accented tied step.
//! - Low-note and high-note rules don't apply: the last key held sounds, and letting it go slides
//!   back to one still held.
//!
//! **The accent circuit:** an accented note forces the MEG to its shortest decay, whatever the knob
//! says. Its envelope then does two extra things: it lifts the VCA, and through a capacitor whose
//! charging time the Resonance pot's other half sets (the "sweep") it lifts the cutoff. At low
//! resonance the sweep follows the envelope; at high, it rises late and falls slowly, so accents
//! close together stack up and climb.

use vst3_host::MidiEvent;

use super::common::{dc_pole, hz_to_note, OutStage, Rest};
use super::{FxContext, Instrument, InstrumentInfo, Midi};
use crate::synth_dsp::alloc::{Priority, VoiceAction, VoiceAllocator, VoiceMode};
use crate::synth_dsp::analog::{sub_seed, Drift, Rng};
use crate::synth_dsp::filter::{cutoff_g, DiodeLadder, ZdfFilter};
use crate::synth_dsp::osc::{Osc, Wave};
use crate::synth_dsp::smooth::{Glide, Smoothed};
use crate::synth_dsp::{note_hz, tanh_fast};

pub mod params;
mod presets;

use params::param::*;
pub use params::{PARAMS, SECTIONS};
pub use presets::PRESETS;

const SMOOTH_MS: f32 = 6.0;
const BEND_MS: f32 = 2.0;
/// The MEG's attack, and its decay when accented: figures from Whittle's documentation.
const MEG_ATTACK_S: f32 = 0.003;
const MEG_ACCENT_DECAY_S: f32 = 0.2;
/// The Volume envelope: attack, its long decay with the key down, and how fast the key coming up
/// closes it. The decay is from Whittle's documentation (it depends a little on the unit); the rest
/// are chosen not to click.
const VEG_ATTACK_S: f32 = 0.003;
const VEG_DECAY_S: f32 = 3.2;
const VEG_RELEASE_S: f32 = 0.012;
/// What Env Mod does to the cutoff, in semitones at the knob's top. A guess. The knob's second
/// job on the hardware: turning it up also pulls the base cutoff down, so the sweep moves about
/// the same place. Another guess, at how far.
const ENV_REACH_ST: f32 = 40.0;
const ENV_OFFSET_ST: f32 = 14.0;
/// The accent's lift, at the knob's top: cutoff in semitones by the sweep, and the VCA's gain. Guesses.
const ACCENT_SWEEP_ST: f32 = 36.0;
const ACCENT_VCA: f32 = 1.1;
/// The sweep capacitor's lag: the charging resistance is a fixed part plus the Resonance pot. A guess
/// at the values, from reading the schematic: 1 µF through 47 k and up to 100 k more.
const SWEEP_MIN_S: f32 = 0.047;
const SWEEP_RANGE_S: f32 = 0.100;
/// Resonance at 10, as `DiodeLadder`'s 0..1 where 1 would self-oscillate. The unmodified machine
/// comes close and doesn't get there.
const RES_MAX: f32 = 0.965;
/// The high-pass in the feedback path, and the output's, in Hz: guesses from the published analyses.
const FEEDBACK_HP_HZ: f32 = 150.0;
const DC_HZ: f32 = 25.0;
/// The square's shaping from the saw on the hardware's one-transistor stage: the high stays near
/// flat with a droop, the corners rounded by the stage's speed. Guesses at the corners.
const SQUARE_DROOP_HZ: f32 = 50.0;
const SQUARE_SLEW_HZ: f32 = 9000.0;
const SQUARE_LEVEL: f32 = 0.75;
/// How hard the oscillator hits the ladder's first rung.
const INPUT_GAIN: f32 = 0.42;
const OUTPUT_GAIN: f32 = 6.0;
const NOISE_FLOOR: f32 = 2e-5;
const HOLD_SECS: f32 = 0.05;

const S_TUNE: usize = 0;
const S_CUTOFF: usize = 1;
const S_RES: usize = 2;
const S_ENV: usize = 3;
const S_ACCENT: usize = 4;
const S_VOLUME: usize = 5;
const S_WAVE: usize = 6;
const S_BEND: usize = 7;
const S_EXPR: usize = 8;
const S_DRIFT: usize = 9;
const S_SWEEP_COEF: usize = 10;
const S_COUNT: usize = 11;

/// An instance. Monophonic: there's one voice, and this is it.
pub struct Uno {
    os: usize,
    rate: f32,
    seed: u64,
    values: [f32; params::param::COUNT],
    dirty: bool,
    smooth: [Smoothed; S_COUNT],
    alloc: VoiceAllocator<1>,
    osc_saw: Osc,
    osc_square: Osc,
    square_lp: f32,
    square_hp: f32,
    square_lp_a: f32,
    square_hp_a: f32,
    filter: DiodeLadder,
    /// The MEG and the VEG: levels, and coefficients.
    meg: f32,
    meg_rising: bool,
    meg_attack: f32,
    meg_decay: f32,
    meg_accent_decay: f32,
    veg: f32,
    veg_rising: bool,
    gate: bool,
    veg_attack: f32,
    veg_decay: f32,
    veg_release: f32,
    /// The sweep capacitor.
    sweep: f32,
    accent: bool,
    /// How long it stays on with the accent flag gone: a level that follows it, not to click.
    accent_gain: f32,
    accent_coef: f32,
    active: bool,
    pitch: Glide,
    drift: Drift,
    drift_st: f32,
    ctl: u32,
    noise: Rng,
    out: OutStage,
    dc_r: f32,
    rest: Rest,
    hold_len: u32,
    last_cut: f32,
}

const CONTROL_TICKS: u32 = 32;

impl Uno {
    pub fn new(sample_rate: f32, seed: u64) -> Uno {
        let os = if sample_rate < 64000.0 { 2 } else { 1 };
        let rate = sample_rate * os as f32;
        let coef = |secs: f32| 1.0 - (-1.0 / (secs * rate)).exp();
        let mut smooth = [Smoothed::new(0.0, SMOOTH_MS, sample_rate); S_COUNT];
        smooth[S_BEND] = Smoothed::new(0.0, BEND_MS, sample_rate);
        smooth[S_EXPR].target = 1.0;
        let mut filter = DiodeLadder::default();
        filter.set_feedback_highpass(cutoff_g(FEEDBACK_HP_HZ, rate));
        let mut rng = Rng::new(sub_seed(seed, 9));
        let phase = rng.unit() as f64;
        let (mut saw, mut square) = (Osc::new(Wave::Saw), Osc::new(Wave::Pulse { width: 0.5 }));
        saw.reset(phase);
        square.reset(phase);
        let mut t = Uno {
            os,
            rate,
            seed,
            values: params::defaults(),
            dirty: true,
            smooth,
            alloc: VoiceAllocator::new(),
            osc_saw: saw,
            osc_square: square,
            square_lp: 0.0,
            square_hp: 0.0,
            square_lp_a: 1.0 - (-std::f32::consts::TAU * SQUARE_SLEW_HZ / rate).exp(),
            square_hp_a: 1.0 - (-std::f32::consts::TAU * SQUARE_DROOP_HZ / rate).exp(),
            filter,
            meg: 0.0,
            meg_rising: false,
            meg_attack: coef(MEG_ATTACK_S),
            meg_decay: 0.0,
            meg_accent_decay: fall(MEG_ACCENT_DECAY_S, rate),
            veg: 0.0,
            veg_rising: false,
            gate: false,
            veg_attack: coef(VEG_ATTACK_S),
            veg_decay: fall(VEG_DECAY_S, rate),
            veg_release: fall(VEG_RELEASE_S, rate),
            sweep: 0.0,
            accent: false,
            accent_gain: 0.0,
            accent_coef: coef(0.002),
            active: false,
            pitch: Glide::default(),
            drift: Drift::new(sub_seed(seed, 3), rate / CONTROL_TICKS as f32, 3.1),
            drift_st: 0.0,
            ctl: 0,
            noise: Rng::new(sub_seed(seed, 4)),
            out: OutStage::new(),
            dc_r: dc_pole(DC_HZ, sample_rate),
            rest: Rest::new(),
            hold_len: (HOLD_SECS * sample_rate) as u32,
            last_cut: 0.0,
        };
        t.apply_patch();
        for p in PARAMS {
            t.smooth_target(p.id);
        }
        for s in &mut t.smooth {
            s.snap();
        }
        t
    }

    fn smooth_target(&mut self, id: u32) {
        let g = |id: u32| self.values[id as usize];
        match id {
            TUNE => self.smooth[S_TUNE].target = g(TUNE),
            CUTOFF => self.smooth[S_CUTOFF].target = hz_to_note(g(CUTOFF)),
            RESONANCE => {
                let r = g(RESONANCE) / 10.0;
                self.smooth[S_RES].target = r * RES_MAX;
                // The pot's other half: the sweep's lag, turned into a coefficient per tick.
                self.smooth[S_SWEEP_COEF].target = 1.0 / ((SWEEP_MIN_S + SWEEP_RANGE_S * r) * self.rate);
            }
            ENV_MOD => self.smooth[S_ENV].target = g(ENV_MOD) / 10.0,
            ACCENT => self.smooth[S_ACCENT].target = g(ACCENT) / 10.0,
            VOLUME => {
                let l = g(VOLUME) / 10.0;
                self.smooth[S_VOLUME].target = l * l * OUTPUT_GAIN;
            }
            WAVE => self.smooth[S_WAVE].target = g(WAVE),
            DRIFT => self.smooth[S_DRIFT].target = g(DRIFT),
            _ => {}
        }
    }

    fn apply_patch(&mut self) {
        self.dirty = false;
        self.meg_decay = fall(self.values[DECAY as usize] * 0.001, self.rate);
        self.pitch.set_time(self.values[SLIDE_TIME as usize] * 0.001, self.rate);
        self.alloc.set_mode(VoiceMode::Mono { priority: Priority::Last, multi_trigger: false }, 1, 1, |_| {});
    }

    fn handle(&mut self, m: Midi) {
        match m {
            Midi::NoteOn { note, velocity } => {
                if self.rest.resting {
                    self.rest.wake();
                }
                let vel = (velocity * 127.0).round();
                let threshold = self.values[ACCENT_VELOCITY as usize];
                let accent = vel >= threshold;
                let mut action = None;
                self.alloc.note_on(note, velocity, |a| action = Some(a));
                self.act(action, accent);
            }
            Midi::NoteOff { note } => {
                let mut action = None;
                self.alloc.note_off(note, |a| action = Some(a));
                let accent = self.accent;
                self.act(action, accent);
            }
            Midi::Sustain(down) => {
                let mut action = None;
                self.alloc.set_pedal(down, |a| action = Some(a));
                let accent = self.accent;
                self.act(action, accent);
            }
            Midi::AllNotesOff => self.let_go(),
            Midi::PitchBend(b) => self.smooth[S_BEND].target = b * self.values[BEND_RANGE as usize],
            Midi::Expression(e) => self.smooth[S_EXPR].target = e,
            Midi::ResetControllers => self.reset_controllers(),
            Midi::ModWheel(_) | Midi::Aftertouch(_) | Midi::Other => {}
        }
    }

    fn act(&mut self, action: Option<VoiceAction>, accent: bool) {
        match action {
            Some(VoiceAction::Start { note, .. }) => {
                // A step: the pitch jumps, the envelopes restart, from where they are.
                self.pitch.jump(note as f32);
                self.active = true;
                self.gate = true;
                self.accent = accent;
                self.meg_rising = true;
                self.veg_rising = true;
            }
            Some(VoiceAction::Glide { note, .. }) => {
                // A tied step: the slide lag, and no envelope restarts.
                self.pitch.target = note as f32;
                self.accent = accent;
            }
            Some(VoiceAction::Release { .. }) => self.gate = false,
            None => {}
        }
    }

    fn let_go(&mut self) {
        let mut actions = None;
        self.alloc.release_all(|a| actions = Some(a));
        self.gate = false;
        let _ = actions;
    }

    fn reset_controllers(&mut self) {
        self.smooth[S_BEND].target = 0.0;
        self.smooth[S_EXPR].target = 1.0;
    }

    /// The cutoff's current note, for a test.
    #[cfg(test)]
    fn cutoff_now(&self) -> f32 {
        self.last_cut
    }

    /// One tick at the voice's rate.
    #[inline]
    fn tick(&mut self, v: &[f32; S_COUNT]) -> f32 {
        let (tune, cutoff, res, env, accent_knob, square, drift, sweep_coef) = (v[S_TUNE] + v[S_BEND], v[S_CUTOFF], v[S_RES], v[S_ENV], v[S_ACCENT], v[S_WAVE], v[S_DRIFT], v[S_SWEEP_COEF]);
        if self.ctl == 0 {
            self.ctl = CONTROL_TICKS;
            self.drift_st = self.drift.tick() * drift * 0.06;
        }
        self.ctl -= 1;
        let note = self.pitch.tick() + tune + self.drift_st;
        let inc = note_hz(note) as f64 / self.rate as f64;
        let saw = self.osc_saw.tick(inc, None).0;
        // The square keeps the saw's clock, and goes through its shaper: rounded edges and a droop.
        let pulse = self.osc_square.tick(inc, None).0;
        self.square_lp += (pulse - self.square_lp) * self.square_lp_a;
        self.square_hp += (self.square_lp - self.square_hp) * self.square_hp_a;
        let shaped = (self.square_lp - self.square_hp) * SQUARE_LEVEL;
        let x = saw + (shaped - saw) * square;

        // The MEG decays whether the key's up or not. An accent forces its shortest decay.
        if self.meg_rising {
            self.meg += (1.2 - self.meg) * self.meg_attack;
            if self.meg >= 1.0 {
                self.meg = 1.0;
                self.meg_rising = false;
            }
        } else {
            let d = if self.accent { self.meg_accent_decay } else { self.meg_decay };
            self.meg -= self.meg * d;
            if self.meg < 1e-6 {
                self.meg = 0.0;
            }
        }
        // The VEG: a long decay with the key down, a fast close with it up.
        if self.gate {
            if self.veg_rising {
                self.veg += (1.2 - self.veg) * self.veg_attack;
                if self.veg >= 1.0 {
                    self.veg = 1.0;
                    self.veg_rising = false;
                }
            } else {
                self.veg -= self.veg * self.veg_decay;
            }
        } else {
            self.veg -= self.veg * self.veg_release;
            if self.veg < 1e-5 {
                self.veg = 0.0;
                self.active = false;
            }
        }
        let accent_target = if self.accent { 1.0 } else { 0.0 };
        self.accent_gain += (accent_target - self.accent_gain) * self.accent_coef;

        // The sweep capacitor charges from the accented envelope and falls back through the same
        // resistance, so a slow one lags and stacks.
        let charge = self.meg * self.accent_gain * accent_knob;
        self.sweep += (charge - self.sweep) * sweep_coef;
        if self.sweep < 1e-7 {
            self.sweep = 0.0;
        }

        // No keyboard tracking: the cutoff doesn't follow the note on this instrument.
        let cut = cutoff - env * ENV_OFFSET_ST + self.meg * env * ENV_REACH_ST + self.sweep * ACCENT_SWEEP_ST;
        self.last_cut = cut;
        self.filter.set(cutoff_g(note_hz(cut.clamp(-24.0, 140.0)), self.rate), res);
        let floor = self.noise.bipolar() * NOISE_FLOOR;
        let y = self.filter.tick(tanh_fast(x * INPUT_GAIN + floor));
        y * self.veg * (1.0 + charge * ACCENT_VCA)
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
            for s in &mut self.smooth {
                s.snap();
            }
            return;
        }
        for i in 0..n {
            let mut v = [0.0f32; S_COUNT];
            for (o, x) in v.iter_mut().zip(self.smooth.iter_mut()) {
                *o = x.tick();
            }
            let was_active = self.active;
            let mut bus = 0.0f32;
            if self.os == 2 {
                let (mut a0, mut a1) = (0.0f32, 0.0f32);
                if was_active {
                    a0 = self.tick(&v);
                    a1 = self.tick(&v);
                }
                bus = self.out.dec.process(a0, a1);
            } else if was_active {
                bus = self.tick(&v);
            }
            let y = self.out.block_dc(bus, self.dc_r) * v[S_VOLUME] * v[S_EXPR];
            l[i] = y;
            r[i] = y;
            if !self.active {
                if self.alloc.is_busy(0) {
                    self.alloc.voice_finished(0);
                }
                self.rest.hold = self.rest.hold.saturating_sub(1);
                if self.rest.hold == 0 {
                    self.rest.resting = true;
                    self.out = OutStage::new();
                    self.filter.reset();
                    self.sweep = 0.0;
                    self.meg = 0.0;
                    self.square_lp = 0.0;
                    self.square_hp = 0.0;
                    l[i + 1..].fill(0.0);
                    r[i + 1..].fill(0.0);
                    self.rest.samples += (n - i - 1) as u64;
                    return;
                }
            } else {
                self.rest.hold = self.hold_len;
            }
        }
    }
}

fn fall(secs: f32, rate: f32) -> f32 {
    1.0 - (-3.0 / (secs.max(1e-4) * rate)).exp()
}

impl Instrument for Uno {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.values.get_mut(id as usize) else { return };
        *slot = value;
        self.smooth_target(id);
        if matches!(id, DECAY | SLIDE_TIME) {
            self.dirty = true;
        }
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
        let mut rng = Rng::new(sub_seed(self.seed, 9));
        let phase = rng.unit() as f64;
        self.osc_saw.reset(phase);
        self.osc_square.reset(phase);
        self.square_lp = 0.0;
        self.square_hp = 0.0;
        self.filter.reset();
        self.meg = 0.0;
        self.meg_rising = false;
        self.veg = 0.0;
        self.veg_rising = false;
        self.gate = false;
        self.sweep = 0.0;
        self.accent = false;
        self.accent_gain = 0.0;
        self.active = false;
        self.pitch.jump(0.0);
        self.drift = Drift::new(sub_seed(self.seed, 3), self.rate / CONTROL_TICKS as f32, 3.1);
        self.drift_st = 0.0;
        self.ctl = 0;
        self.noise = Rng::new(sub_seed(self.seed, 4));
        self.alloc.reset();
        self.out = OutStage::new();
        self.rest = Rest::new();
        self.reset_controllers();
        for s in &mut self.smooth {
            s.snap();
        }
    }

    fn tail_secs(&self) -> f32 {
        0.15
    }
}

pub fn info() -> InstrumentInfo {
    InstrumentInfo {
        id: "builtin:uno",
        name: "Fermata Uno",
        group: "Vintage Synths",
        about: "One oscillator, a diode ladder, accent and slide: a bass line machine. Overlapping notes slide, high velocity accents",
        params: PARAMS,
        sections: SECTIONS,
        meters: &[],
        presets: PRESETS,
        new: |sample_rate, seed| Box::new(Uno::new(sample_rate, seed)),
    }
}

#[cfg(test)]
mod tests;
