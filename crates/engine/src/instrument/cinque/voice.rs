//! One voice of Fermata Cinque: two VCOs, noise, a 4-pole filter, two envelopes and Poly Mod,
//! ticking at the oversampled rate.

use crate::synth_dsp::alloc::unison_spread;
use crate::synth_dsp::analog::{sub_seed, Drift, Rng, Tolerance};
use crate::synth_dsp::env::{EnvShape, Envelope};
use crate::synth_dsp::filter::{cutoff_g, OtaCascade, ZdfFilter};
use crate::synth_dsp::osc::{Noise, Osc, Wave};
use crate::synth_dsp::smooth::Glide;
use crate::synth_dsp::{note_hz, tanh_fast};

pub const VOICES: usize = 5;
const CONTROL_TICKS: u32 = 32;
const FILTER_REF_NOTE: f32 = 60.0;
const NOISE_FLOOR: f32 = 3e-5;
/// Osc B's note when it's off the keyboard.
const B_REF_NOTE: f32 = 60.0;
/// Lo Freq takes B seven octaves down: roughly 0.3 to 30 Hz across its knob. A guess at the range.
const B_LO_ST: f32 = -84.0;

/// Poly Mod's reach at full, from each source. The documents speak only in words here: guesses,
/// wide enough for what the instrument is known for, from a shimmer to clangorous sweeps.
pub const PM_FREQ_ST: f32 = 60.0;
pub const PM_PW: f32 = 0.48;
pub const PM_FILTER_ST: f32 = 72.0;

/// The values shared by all voices that move smoothly.
#[derive(Debug, Clone, Copy, Default)]
pub struct Frame {
    /// Master tune plus bend.
    pub tune_st: f32,
    pub a_st: f32,
    pub b_st: f32,
    pub b_fine: f32,
    pub a_pw: f32,
    pub b_pw: f32,
    /// The wave switches, smoothed so they don't click.
    pub a_saw: f32,
    pub a_pulse: f32,
    pub b_saw: f32,
    pub b_tri: f32,
    pub b_pulse: f32,
    /// The mixer's gains.
    pub mix_a: f32,
    pub mix_b: f32,
    pub noise: f32,
    pub cutoff_note: f32,
    pub k: f32,
    pub env_st: f32,
    /// The Poly Mod sources' amounts, 0..1, and destinations' switches, smoothed to not click.
    pub pm_fenv: f32,
    pub pm_oscb: f32,
    pub pm_freq_a: f32,
    pub pm_pw_a: f32,
    pub pm_filter: f32,
    /// The wheel's modulation already weighed by its switches.
    pub wm_freq_a: f32,
    pub wm_freq_b: f32,
    pub wm_pw_a: f32,
    pub wm_pw_b: f32,
    pub wm_filter: f32,
    pub slop: f32,
}

/// The values shared by all voices that change in steps.
#[derive(Debug, Clone, Copy)]
pub struct Patch {
    pub rate: f32,
    pub b_lo: bool,
    pub b_kbd: bool,
    pub a_sync: bool,
    pub kbd: f32,
    pub filter_env: EnvShape,
    pub amp_env: EnvShape,
    pub glide_secs: f32,
    pub unison_st: f32,
    pub group_gain: f32,
    pub noise_scale: f32,
    pub vel_filter_st: f32,
    pub vel_amp: f32,
    pub slew: f32,
    pub bass: f32,
}

pub struct Voice {
    pub active: bool,
    a_saw: Osc,
    a_pulse: Osc,
    b_saw: Osc,
    b_tri: Osc,
    b_pulse: Osc,
    noise: Noise,
    filter: OtaCascade,
    filter_env: Envelope,
    amp_env: Envelope,
    glide: Glide,
    played: bool,
    vel_gain: f32,
    vel_st: f32,
    unison_x: f32,
    gain: [f32; 2],
    tol: Tolerance,
    drift: [Drift; 3],
    drift_st: [f32; 3],
    ctl: u32,
}

impl Voice {
    pub fn new(seed: u64, index: usize, patch: &Patch) -> Voice {
        let voice_seed = sub_seed(seed, index as u64);
        let mut rng = Rng::new(sub_seed(voice_seed, 99));
        let (pa, pb) = (rng.unit() as f64, rng.unit() as f64);
        let osc = |w: Wave, p: f64| {
            let mut o = Osc::new(w);
            o.reset(p);
            o
        };
        let control = patch.rate / CONTROL_TICKS as f32;
        let drift_at = |k: u64, secs: f32| Drift::new(sub_seed(voice_seed, 200 + k), control, secs);
        Voice {
            active: false,
            a_saw: osc(Wave::Saw, pa),
            a_pulse: osc(Wave::Pulse { width: 0.5 }, pa),
            b_saw: osc(Wave::Saw, pb),
            b_tri: osc(Wave::Triangle { width: 0.5 }, pb),
            b_pulse: osc(Wave::Pulse { width: 0.5 }, pb),
            noise: Noise::new(sub_seed(voice_seed, 2)),
            filter: OtaCascade::new(patch.slew, patch.bass),
            filter_env: Envelope::default(),
            amp_env: Envelope::default(),
            glide: Glide::default(),
            played: false,
            vel_gain: 1.0,
            vel_st: 0.0,
            unison_x: 0.0,
            gain: [1.0, 1.0],
            tol: Tolerance::new(sub_seed(voice_seed, 1)),
            drift: [drift_at(0, 2.6), drift_at(1, 3.1), drift_at(2, 4.3)],
            drift_st: [0.0; 3],
            ctl: 0,
        }
    }

    pub fn configure(&mut self, patch: &Patch) {
        self.glide.set_time(patch.glide_secs, patch.rate);
        self.filter.set_character(patch.slew, patch.bass);
    }

    pub fn start(&mut self, note: u8, velocity: f32, unison: (u8, u8), patch: &Patch) {
        let n = note as f32;
        if patch.glide_secs > 0.0 && self.played {
            self.glide.target = n;
        } else {
            self.glide.jump(n);
        }
        self.played = true;
        self.active = true;
        self.vel_gain = 1.0 - patch.vel_amp * (1.0 - velocity * velocity);
        self.vel_st = patch.vel_filter_st * velocity;
        if unison.0 < unison.1 {
            let (x, _) = unison_spread(unison.0, unison.1);
            self.unison_x = x;
            self.gain = [patch.group_gain; 2];
        }
        self.filter_env.gate_on();
        self.amp_env.gate_on();
    }

    pub fn glide_to(&mut self, note: u8) {
        self.glide.target = note as f32;
    }

    pub fn release(&mut self) {
        self.filter_env.gate_off();
        self.amp_env.gate_off();
    }

    /// The filter envelope's level, for a test.
    #[cfg(test)]
    pub fn fenv(&self) -> f32 {
        self.filter_env.level
    }

    /// One tick at the voice's rate. Adds the voice to the bus.
    #[inline]
    pub fn tick(&mut self, f: &Frame, patch: &Patch, bus: &mut f32) {
        if self.ctl == 0 {
            self.ctl = CONTROL_TICKS;
            let d = f.slop;
            // ±5 cents of wander and ±3 of fixed offset at full slop; the filter ±1/3 semitone.
            self.drift_st[0] = self.drift[0].tick() * d * 0.05 + self.tol.osc[0] * d * 0.03;
            self.drift_st[1] = self.drift[1].tick() * d * 0.05 + self.tol.osc[1] * d * 0.03;
            self.drift_st[2] = self.drift[2].tick() * d * 0.1 + self.tol.cutoff * d * 0.33;
        }
        self.ctl -= 1;

        let note = self.glide.tick();
        let keyed = note + f.tune_st + self.unison_x * patch.unison_st;
        let fe = self.filter_env.tick(&patch.filter_env, 1.0 + self.tol.env[0] * f.slop * 0.05);
        let hz = 1.0 / patch.rate as f64;

        // Osc B first: it's the source of the sync and of half of Poly Mod. It always runs, whether
        // it's in the mixer or not.
        let b_note = if patch.b_kbd { keyed } else { B_REF_NOTE + f.tune_st };
        let b_lo = if patch.b_lo { B_LO_ST } else { 0.0 };
        let b_inc = hz * note_hz(b_note + b_lo + f.b_st + f.b_fine + f.wm_freq_b + self.drift_st[1]) as f64;
        self.b_pulse.set_wave(Wave::Pulse { width: (f.b_pw + f.wm_pw_b).clamp(0.02, 0.98) });
        let (bs, wrap) = self.b_saw.tick(b_inc, None);
        let bt = self.b_tri.tick(b_inc, None).0;
        let bp = self.b_pulse.tick(b_inc, None).0;
        let b = bs * f.b_saw + bt * f.b_tri + bp * f.b_pulse;

        // Poly Mod: the filter envelope and Osc B, summed, to three destinations, per voice and at
        // the audio rate. Osc B's level here depends on which waves are on, not on its mixer knob.
        let pm = fe * f.pm_fenv + b * f.pm_oscb;

        let a_inc = hz * note_hz(keyed + f.a_st + f.wm_freq_a + pm * f.pm_freq_a * PM_FREQ_ST + self.drift_st[0]) as f64;
        self.a_pulse.set_wave(Wave::Pulse { width: (f.a_pw + f.wm_pw_a + pm * f.pm_pw_a * PM_PW).clamp(0.02, 0.98) });
        let sync = if patch.a_sync { wrap } else { None };
        let a = self.a_saw.tick(a_inc, sync).0 * f.a_saw + self.a_pulse.tick(a_inc, sync).0 * f.a_pulse;

        let n = self.noise.white() * patch.noise_scale;
        let sum = a * f.mix_a + b * f.mix_b + n * (f.noise + NOISE_FLOOR);
        let driven = tanh_fast(sum);

        let cut = f.cutoff_note + (note - FILTER_REF_NOTE) * patch.kbd + fe * f.env_st + self.vel_st + f.wm_filter + pm * f.pm_filter * PM_FILTER_ST + self.drift_st[2];
        self.filter.set(cutoff_g(note_hz(cut.clamp(-24.0, 150.0)), patch.rate), f.k);
        let y = self.filter.tick(driven);

        let amp = self.amp_env.tick(&patch.amp_env, 1.0 + self.tol.env[1] * f.slop * 0.05);
        *bus += y * amp * self.vel_gain * self.gain[0] * (1.0 + self.tol.level * f.slop * 0.04);
        if !self.amp_env.active() {
            self.active = false;
        }
    }
}
