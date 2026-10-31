//! One voice of Fermata Sei: a DCO's saw, pulse and sub from one clock, noise, an OTA 4-pole filter
//! and a VCA, ticking at the oversampled rate.

use crate::synth_dsp::alloc::unison_spread;
use crate::synth_dsp::analog::{sub_seed, Rng, Tolerance};
use crate::synth_dsp::filter::{cutoff_g, OtaCascade, ZdfFilter};
use crate::synth_dsp::osc::{Noise, Osc, Wave};
use crate::synth_dsp::smooth::Glide;
use crate::synth_dsp::note_hz;

pub const VOICES: usize = 6;
const FILTER_REF_NOTE: f32 = 60.0;
/// The filter's noise floor against a full oscillator: what its self-oscillation starts from.
const NOISE_FLOOR: f32 = 3e-5;
/// A guess at how the mixer drives the filter: gently. The instrument stays clean.
pub const MIX_GAIN: f32 = 0.55;
/// The OTA's constants here (`OtaCascade`): guesses for a chip known for staying clean and for
/// losing some bass with resonance.
const OTA_SLEW: f32 = 0.7;
const OTA_BASS: f32 = 0.25;
/// Filter tolerance in semitones at full Voice Variation. The DCOs have none: that's the point of
/// them, and what separates this instrument's voices is the analog after them.
const VARIATION_ST: f32 = 0.6;
const VARIATION_LEVEL: f32 = 0.06;
const VARIATION_ENV: f32 = 0.1;

/// The values shared by all voices that move smoothly.
#[derive(Debug, Clone, Copy, Default)]
pub struct Frame {
    /// Semitones on every voice's DCO: range, bend and LFO together.
    pub pitch_st: f32,
    pub width: f32,
    pub saw: f32,
    pub pulse: f32,
    pub sub: f32,
    pub noise: f32,
    pub cutoff_note: f32,
    /// Semitones from the shared LFO and the lever.
    pub cutoff_mod_st: f32,
    pub k: f32,
    /// The envelope's reach in semitones, negative when inverted.
    pub env_st: f32,
    pub kbd: f32,
    /// 1 is the envelope's VCA, 0 the gate's.
    pub vca_env: f32,
    pub variation: f32,
}

/// Linear attack, exponential decay and release. The instrument's envelope is made by its CPU
/// rather than a capacitor (one of the ways it's not its predecessors), and players know it for its
/// snap. The shape is the usual reading of its firmware's behaviour, not a measurement.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvRates {
    /// Rise per tick.
    pub attack: f32,
    pub decay: f32,
    pub release: f32,
    pub sustain: f32,
}

impl EnvRates {
    pub fn new(attack_s: f32, decay_s: f32, sustain: f32, release_s: f32, rate: f32) -> EnvRates {
        let fall = |s: f32| 1.0 - (-3.0 / (s.max(1e-4) * rate)).exp();
        EnvRates { attack: 1.0 / (attack_s.max(1e-4) * rate), decay: fall(decay_s), release: fall(release_s), sustain }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Stage {
    #[default]
    Idle,
    Attack,
    Decay,
    Release,
}

/// The values shared by all voices that change in steps.
#[derive(Debug, Clone, Copy)]
pub struct Patch {
    pub rate: f32,
    pub env: EnvRates,
    pub glide_on: bool,
    pub glide_secs: f32,
    pub velocity: f32,
    pub unison_st: f32,
    pub group_gain: f32,
    pub noise_scale: f32,
    pub gate_coef: f32,
}

pub struct Voice {
    pub active: bool,
    pub gated: bool,
    /// Saw and pulse from the DCO's clock, and the sub an octave down, so they stay locked.
    saw: Osc,
    pulse: Osc,
    sub: Osc,
    noise: Noise,
    filter: OtaCascade,
    env_level: f32,
    stage: Stage,
    gate_level: f32,
    glide: Glide,
    played: bool,
    vel_gain: f32,
    unison_x: f32,
    tol: Tolerance,
}

impl Voice {
    pub fn new(seed: u64, index: usize) -> Voice {
        let voice_seed = sub_seed(seed, index as u64);
        let mut rng = Rng::new(sub_seed(voice_seed, 99));
        let p = rng.unit() as f64;
        let (mut saw, mut pulse, mut sub) = (Osc::new(Wave::Saw), Osc::new(Wave::Pulse { width: 0.5 }), Osc::new(Wave::Pulse { width: 0.5 }));
        saw.reset(p);
        pulse.reset(p);
        sub.reset(p * 0.5);
        let mut filter = OtaCascade::new(OTA_SLEW, OTA_BASS);
        filter.reset();
        Voice {
            active: false,
            gated: false,
            saw,
            pulse,
            sub,
            noise: Noise::new(sub_seed(voice_seed, 2)),
            filter,
            env_level: 0.0,
            stage: Stage::Idle,
            gate_level: 0.0,
            glide: Glide::default(),
            played: false,
            vel_gain: 1.0,
            unison_x: 0.0,
            tol: Tolerance::new(sub_seed(voice_seed, 1)),
        }
    }

    pub fn set_glide(&mut self, patch: &Patch) {
        self.glide.set_time(if patch.glide_on { patch.glide_secs } else { 0.0 }, patch.rate);
    }

    pub fn start(&mut self, note: u8, velocity: f32, unison: (u8, u8), patch: &Patch) {
        let n = note as f32;
        if patch.glide_on && self.played {
            self.glide.target = n;
        } else {
            self.glide.jump(n);
        }
        self.played = true;
        self.active = true;
        self.gated = true;
        self.vel_gain = 1.0 - patch.velocity * (1.0 - velocity * velocity);
        if unison.0 < unison.1 {
            self.unison_x = unison_spread(unison.0, unison.1).0;
        }
        self.stage = Stage::Attack;
    }

    pub fn glide_to(&mut self, note: u8) {
        self.glide.target = note as f32;
    }

    pub fn release(&mut self) {
        self.gated = false;
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// One tick at the voice's rate. Adds the voice to the bus.
    #[inline]
    pub fn tick(&mut self, f: &Frame, patch: &Patch, bus: &mut f32) {
        let note = self.glide.tick();
        let hz = note_hz(note + f.pitch_st + self.unison_x * patch.unison_st);
        let inc = hz as f64 / patch.rate as f64;
        self.pulse.set_wave(Wave::Pulse { width: f.width });
        let saw = self.saw.tick(inc, None).0;
        let pulse = self.pulse.tick(inc, None).0;
        let sub = self.sub.tick(inc * 0.5, None).0;
        let noise = self.noise.white();
        let sum = saw * f.saw + pulse * f.pulse + sub * f.sub + noise * patch.noise_scale * (f.noise + NOISE_FLOOR);

        // The envelope.
        let speed = 1.0 + self.tol.env[0] * f.variation * VARIATION_ENV;
        let e = &patch.env;
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.env_level += e.attack * speed;
                if self.env_level >= 1.0 {
                    self.env_level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => self.env_level += (e.sustain - self.env_level) * e.decay * speed,
            Stage::Release => {
                self.env_level -= self.env_level * e.release * speed;
                if self.env_level < 1e-5 {
                    self.env_level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        // The gate's VCA: fast either way, but not a click.
        let gate = if self.gated { 1.0 } else { 0.0 };
        self.gate_level += (gate - self.gate_level) * patch.gate_coef;

        let cut = f.cutoff_note + (note - FILTER_REF_NOTE) * f.kbd + self.env_level * f.env_st + f.cutoff_mod_st + self.tol.cutoff * f.variation * VARIATION_ST;
        self.filter.set(cutoff_g(note_hz(cut.clamp(-24.0, 150.0)), patch.rate), f.k);
        let y = self.filter.tick(sum);

        let amp = self.env_level + (self.gate_level - self.env_level) * (1.0 - f.vca_env);
        *bus += y * amp * self.vel_gain * (1.0 + self.tol.level * f.variation * VARIATION_LEVEL);
        if !self.gated && amp < 1e-5 && (self.stage == Stage::Idle || f.vca_env == 0.0) {
            self.active = false;
            self.stage = Stage::Idle;
            self.env_level = 0.0;
            self.gate_level = 0.0;
        }
    }
}

