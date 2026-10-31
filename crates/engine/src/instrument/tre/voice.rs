//! One voice of Fermata Tre: three oscillators, noise, the mixer, a ladder and two contours,
//! ticking at the oversampled rate. Everything that is shared by the voices is in [`Frame`]
//! (changes smoothly, updated once per output sample) and [`Patch`] (changes in steps).

use super::params::VOICE_COUNTS;
use crate::synth_dsp::alloc::unison_spread;
use crate::synth_dsp::analog::{sub_seed, Drift, Rng, Tolerance};
use crate::synth_dsp::env::{EnvShape, Envelope};
use crate::synth_dsp::filter::{cutoff_g, Ladder, ZdfFilter};
use crate::synth_dsp::osc::{Noise, Osc, Wave};
use crate::synth_dsp::smooth::Glide;
use crate::synth_dsp::{note_hz, pow2, tanh_fast};

/// How many voices an instance has. Mono uses one, or a unison group.
pub const MAX_VOICES: usize = {
    let mut m = 0;
    let mut k = 0;
    while k < VOICE_COUNTS.len() {
        if VOICE_COUNTS[k] > m {
            m = VOICE_COUNTS[k];
        }
        k += 1;
    }
    m
};

/// How often (in oversampled ticks) the drift and tolerances are worked out.
const CONTROL_TICKS: u32 = 32;
/// Keyboard reference for the filter: at middle C, keyboard tracking changes nothing.
const FILTER_REF_NOTE: f32 = 60.0;
/// The mixer's noise floor, about -90 dB against a full oscillator.
const NOISE_FLOOR: f32 = 3e-5;
/// Osc 3's note when it doesn't follow the keyboard, before its range.
const OSC3_REF_NOTE: f32 = 60.0;

/// The values shared by all voices that move smoothly. In the units the voices want.
#[derive(Debug, Clone, Copy, Default)]
pub struct Frame {
    /// Each oscillator's gain into the mixer, and noise and feedback's.
    pub osc_gain: [f32; 3],
    pub noise_gain: f32,
    pub feedback_gain: f32,
    /// The cutoff as a note number, the ladder's feedback, and the contour's reach in semitones.
    pub cutoff_note: f32,
    pub k: f32,
    pub contour_st: f32,
    /// Master tune plus bend, in semitones.
    pub tune_st: f32,
    /// Osc 2 and 3's frequency knobs in semitones.
    pub tune2_st: f32,
    pub tune3_st: f32,
    /// The modulation mix, 0 all osc 3 to 1 all noise.
    pub mod_mix: f32,
    /// The modulation's reach in semitones, after the wheel and its routing switches.
    pub mod_osc_st: f32,
    pub mod_filter_st: f32,
    /// 0..1: how much of the voice's drift and tolerances to use.
    pub drift: f32,
}

/// The values shared by all voices that change in steps, and anything costly to work out.
#[derive(Debug, Clone, Copy)]
pub struct Patch {
    pub rate: f32,
    /// Oscillator range offsets in semitones, for each oscillator.
    pub range_st: [f32; 3],
    /// Oscillator ranges that are Lo, which get a wider frequency knob.
    pub lo: [bool; 3],
    pub osc3_kbd: bool,
    pub pink: bool,
    /// How much the cutoff follows the keyboard (0..1).
    pub kbd_track: f32,
    pub filter_env: EnvShape,
    pub amp_env: EnvShape,
    pub glide_on: bool,
    /// The glide coefficient's time in seconds.
    pub glide_secs: f32,
    pub vel_loudness: f32,
    /// Semitones of cutoff at full velocity.
    pub vel_filter_st: f32,
    /// How much noise power falls with the oversampling (equal power in the audio band).
    pub noise_scale: f32,
    /// Semitones of detune at the outer unison voices.
    pub unison_st: f32,
    /// -1..1 pan reach of the unison voices.
    pub spread: f32,
    /// Level of each voice in a unison group.
    pub group_gain: f32,
}

pub struct Voice {
    pub active: bool,
    pub osc: [Osc; 3],
    noise: Noise,
    filter: Ladder,
    filter_env: Envelope,
    amp_env: Envelope,
    glide: Glide,
    /// Whether the voice has played yet, so a first note doesn't glide from nowhere.
    played: bool,
    /// What the velocity does to this note.
    vel_gain: f32,
    vel_st: f32,
    /// Its place in a unison group: detune -1..1, and left and right gains.
    unison_x: f32,
    gain_l: f32,
    gain_r: f32,
    /// The modulation signal and the output, from the last tick.
    last_mod: f32,
    last_out: f32,
    /// Slow wander and fixed differences.
    tol: Tolerance,
    drift: [Drift; 4],
    drift_st: [f32; 4],
    ctl: u32,
}

impl Voice {
    pub fn new(seed: u64, index: usize, patch: &Patch) -> Voice {
        let voice_seed = sub_seed(seed, index as u64);
        let mut rng = Rng::new(sub_seed(voice_seed, 99));
        let control = patch.rate / CONTROL_TICKS as f32;
        // Free-running oscillators are somewhere in their cycle when a note starts.
        let mut osc = [Osc::new(Wave::Saw); 3];
        for o in &mut osc {
            o.reset(rng.unit() as f64);
        }
        let drift_at = |k: u64, secs: f32| Drift::new(sub_seed(voice_seed, 200 + k), control, secs);
        let tol = Tolerance::new(sub_seed(voice_seed, 1));
        Voice {
            active: false,
            osc,
            noise: Noise::new(sub_seed(voice_seed, 2)),
            filter: Ladder::default(),
            filter_env: Envelope::default(),
            amp_env: Envelope::default(),
            glide: Glide::default(),
            played: false,
            vel_gain: 1.0,
            vel_st: 0.0,
            unison_x: 0.0,
            gain_l: 1.0,
            gain_r: 1.0,
            last_mod: 0.0,
            last_out: 0.0,
            tol,
            drift: [drift_at(0, 2.3), drift_at(1, 2.9), drift_at(2, 3.7), drift_at(3, 4.4)],
            drift_st: [0.0; 4],
            ctl: 0,
        }
    }

    pub fn set_glide(&mut self, patch: &Patch) {
        self.glide.set_time(if patch.glide_on { patch.glide_secs } else { 0.0 }, patch.rate);
    }

    /// Key down.
    pub fn start(&mut self, note: u8, velocity: f32, unison: (u8, u8), patch: &Patch) {
        let n = note as f32;
        if patch.glide_on && self.played {
            self.glide.target = n;
        } else {
            self.glide.jump(n);
        }
        self.played = true;
        self.active = true;
        // Velocity shapes level by a gentle square law, the way a keyboard's strike feels.
        self.vel_gain = 1.0 - patch.vel_loudness * (1.0 - velocity * velocity);
        self.vel_st = patch.vel_filter_st * velocity;
        if unison.0 < unison.1 {
            let (x, pan) = unison_spread(unison.0, unison.1);
            self.unison_x = x;
            let p = pan * patch.spread;
            self.gain_l = (1.0 - p).sqrt() * patch.group_gain;
            self.gain_r = (1.0 + p).sqrt() * patch.group_gain;
        }
        self.filter_env.gate_on();
        self.amp_env.gate_on();
    }

    /// The note changes with the keys held: no new contour.
    pub fn glide_to(&mut self, note: u8) {
        self.glide.target = note as f32;
    }

    /// Key up.
    pub fn release(&mut self) {
        self.filter_env.gate_off();
        self.amp_env.gate_off();
    }

    /// One tick at the voice's rate. Adds the voice to the two buses.
    #[inline]
    pub fn tick(&mut self, f: &Frame, patch: &Patch, l: &mut f32, r: &mut f32) {
        if self.ctl == 0 {
            self.ctl = CONTROL_TICKS;
            let d = f.drift;
            // The oscillators wander by up to ±4 cents, the filter by about a sixth of a semitone.
            self.drift_st[0] = self.drift[0].tick() * d * 0.04 + self.tol.osc[0] * d * 0.02;
            self.drift_st[1] = self.drift[1].tick() * d * 0.04 + self.tol.osc[1] * d * 0.02;
            self.drift_st[2] = self.drift[2].tick() * d * 0.04 + self.tol.osc[2] * d * 0.02;
            self.drift_st[3] = self.drift[3].tick() * d * 0.17 + self.tol.cutoff * d * 0.3;
            let s = |x: f32| 1.0 + x * d * 0.015;
            self.filter.set_tolerance([s(self.tol.stage[0]), s(self.tol.stage[1]), s(self.tol.stage[2]), s(self.tol.stage[3])]);
        }
        self.ctl -= 1;

        let note = self.glide.tick();
        let keyed = note + f.tune_st + self.unison_x * patch.unison_st;

        // Oscillators, modulated by last tick's modulation signal.
        let m = self.last_mod;
        let mo = m * f.mod_osc_st;
        let lo3 = if patch.lo[2] { 4.0 } else { 1.0 };
        let lo2 = if patch.lo[1] { 4.0 } else { 1.0 };
        let hz = 440.0 / patch.rate as f64;
        let base = keyed + mo;
        let inc = |n: f32| hz * pow2((n - 69.0) / 12.0) as f64;
        let i1 = inc(base + patch.range_st[0] + self.drift_st[0]);
        let i2 = inc(base + patch.range_st[1] + f.tune2_st * lo2 + self.drift_st[1]);
        // Osc 3 off the keyboard keeps the tune and bend out too: it's a modulator.
        let n3 = if patch.osc3_kbd { base } else { OSC3_REF_NOTE + mo };
        let i3 = inc(n3 + patch.range_st[2] + f.tune3_st * lo3 + self.drift_st[2]);
        let o1 = self.osc[0].tick(i1, None).0;
        let o2 = self.osc[1].tick(i2, None).0;
        let o3 = self.osc[2].tick(i3, None).0;

        let noise = patch.noise_scale * if patch.pink { self.noise.pink() } else { self.noise.white() };
        let mod_now = o3 + (noise - o3) * f.mod_mix;
        self.last_mod = mod_now;

        // The mixer's sum overdrives the filter's input: that is the fat end of the mixer knobs.
        let sum = o1 * f.osc_gain[0] + o2 * f.osc_gain[1] + o3 * f.osc_gain[2] + noise * f.noise_gain + self.last_out * f.feedback_gain;
        // A floor of circuit noise, far below hearing, is what a self-oscillating filter starts from.
        let driven = tanh_fast(sum + noise * NOISE_FLOOR);

        // The filter, whose cutoff is modulated by the same signal at the audio rate.
        let fe = self.filter_env.tick(&patch.filter_env, 1.0 + self.tol.env[0] * f.drift * 0.04);
        let cut = f.cutoff_note + (note - FILTER_REF_NOTE) * patch.kbd_track + fe * f.contour_st + self.vel_st + mod_now * f.mod_filter_st + self.drift_st[3];
        self.filter.set(cutoff_g(note_hz(cut.clamp(-24.0, 150.0)), patch.rate), f.k);
        let y = self.filter.tick(driven);

        let a = self.amp_env.tick(&patch.amp_env, 1.0 + self.tol.env[1] * f.drift * 0.04);
        let level = 1.0 + self.tol.level * f.drift * 0.03;
        let out = y * a * self.vel_gain * level;
        self.last_out = out;
        *l += out * self.gain_l;
        *r += out * self.gain_r;
        if !self.amp_env.active() {
            self.active = false;
        }
    }

    pub fn set_wave(&mut self, k: usize, wave: Wave) {
        self.osc[k].set_wave(wave);
    }
}
