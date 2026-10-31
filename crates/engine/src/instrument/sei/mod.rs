//! Fermata Sei: a six-voice polyphonic synth, modelled on the architecture and behaviour of the
//! Roland Juno-106 family (this project isn't affiliated with Roland, and the product name stays
//! clear of theirs). Its reference is what's publicly documented about the instrument; nothing
//! here was compared with the hardware. See docs/ARCHITECTURE.md for what's documented behaviour
//! and what's a guess.
//!
//! **Signal path:** per voice, at twice the output rate below 64 kHz: one DCO's saw, pulse and sub
//! square (all from one clock, so they stay phase-locked, and with no pitch drift) and noise, into
//! a resonant 4-pole OTA filter, then a VCA that follows the one envelope or the gate. The voices
//! are then mixed, and at the output rate go through the four-position high-pass, the level and the
//! stereo bucket-brigade chorus.
//!
//! **What it hasn't got, on purpose:** a second oscillator, a second envelope, or the LFO per voice.
//! One envelope drives the filter and the VCA. One LFO, triangle only, drives everything, in
//! step, its delay starting from the first key down. Velocity is off unless asked for: the
//! instrument has none.
//!
//! **Determinism:** the seed sets the voices' filter and envelope differences, phases, noise and
//! the chorus's LFO phase, and `reset` puts them all back.

use vst3_host::MidiEvent;

use super::common::{dc_pole, hz_to_note, OutStage, Rest};
use super::{FxContext, Instrument, InstrumentInfo, Midi};
use crate::synth_dsp::alloc::{Priority, VoiceAction, VoiceAllocator, VoiceMode};
use crate::synth_dsp::smooth::Smoothed;

pub mod chorus;
pub mod params;
mod presets;
mod voice;

use chorus::Chorus;
use params::param::*;
pub use params::{PARAMS, SECTIONS};
pub use presets::PRESETS;
use voice::{EnvRates, Frame, Patch, Voice, MIX_GAIN, VOICES};

const SMOOTH_MS: f32 = 6.0;
const BEND_MS: f32 = 2.0;
/// The knob ranges, documented only in words, so guesses: LFO rate from 0.1 to 30 Hz on an
/// exponential knob; delay to 1.5 s; the vibrato's reach ±3 semitones; the filter LFO's ±30
/// semitones; the envelope's reach 7 octaves; the lever's filter bend ±24 semitones.
const LFO_MIN_HZ: f32 = 0.1;
const LFO_MAX_HZ: f32 = 30.0;
const DELAY_MAX_S: f32 = 1.5;
const LFO_DCO_ST: f32 = 3.0;
const LFO_VCF_ST: f32 = 30.0;
const ENV_ST: f32 = 84.0;
const BEND_VCF_ST: f32 = 24.0;
/// Resonance at 10. Self-oscillation starts at k = 4, so at about 9 on the knob.
const K_MAX: f32 = 4.4;
/// The narrowest pulse the PWM knob reaches: a guess, narrow but not vanishing.
const WIDTH_MIN: f32 = 0.05;
/// Sub and noise against the saw at their knobs' top.
const SUB_GAIN: f32 = 0.9;
const NOISE_GAIN: f32 = 0.7;
const OUTPUT_GAIN: f32 = 1.0;
const DC_HZ: f32 = 6.0;
/// The high-pass switch's corners and the bass boost's shelf: guesses from what's commonly said of
/// the circuit (position 2 about 240 Hz, 3 about 720 Hz, position 0 lifting the bass).
const HPF2_HZ: f32 = 240.0;
const HPF3_HZ: f32 = 720.0;
const SHELF_HZ: f32 = 120.0;
const SHELF_GAIN: f32 = 0.9;
/// The chorus's tail and hiss after the last voice has ended, before the instance stops work.
const HOLD_SECS: f32 = 0.25;

// Smoothed values, by index.
const S_SAW: usize = 0;
const S_PULSE: usize = 1;
const S_SUB: usize = 2;
const S_NOISE: usize = 3;
const S_CUTOFF: usize = 4;
const S_K: usize = 5;
const S_ENV: usize = 6;
const S_KBD: usize = 7;
const S_VCF_LFO: usize = 8;
const S_DCO_LFO: usize = 9;
const S_PWM: usize = 10;
const S_PWM_LFO: usize = 11;
const S_VCA: usize = 12;
const S_LEVEL: usize = 13;
const S_VARIATION: usize = 14;
const S_BEND: usize = 15;
const S_BEND_VCF: usize = 16;
const S_WHEEL: usize = 17;
const S_EXPR: usize = 18;
const S_H2: usize = 19;
const S_H3: usize = 20;
const S_SHELF: usize = 21;
const S_COUNT: usize = 22;

/// An instance.
pub struct Sei {
    os: usize,
    sr: f32,
    seed: u64,
    values: [f32; params::param::COUNT],
    dirty: bool,
    patch: Patch,
    smooth: [Smoothed; S_COUNT],
    voices: Box<[Voice; VOICES]>,
    alloc: VoiceAllocator<VOICES>,
    dec: OutStage,
    out: [OutStage; 2],
    dc_r: f32,
    chorus: Chorus,
    /// The high-pass switch's low-pass tracks.
    lp: [f32; 3],
    lp_a: [f32; 3],
    lfo_phase: f64,
    lfo_inc: f64,
    /// The LFO's delay: how far it has come in, and by how much per sample.
    lfo_gain: f32,
    lfo_gain_inc: f32,
    rest: Rest,
    hold_len: u32,
}

impl Sei {
    pub fn new(sample_rate: f32, seed: u64) -> Sei {
        let os = if sample_rate < 64000.0 { 2 } else { 1 };
        let rate = sample_rate * os as f32;
        let patch = Patch {
            rate,
            env: EnvRates::default(),
            glide_on: false,
            glide_secs: 0.0,
            velocity: 0.0,
            unison_st: 0.0,
            group_gain: 1.0,
            noise_scale: 1.0 / (os as f32).sqrt(),
            gate_coef: 1.0 - (-1.0 / (0.002 * rate)).exp(),
        };
        let voices: Box<[Voice; VOICES]> = Box::new(std::array::from_fn(|i| Voice::new(seed, i)));
        let lp_coef = |hz: f32| 1.0 - (-std::f32::consts::TAU * hz / sample_rate).exp();
        let mut smooth = [Smoothed::new(0.0, SMOOTH_MS, sample_rate); S_COUNT];
        smooth[S_BEND] = Smoothed::new(0.0, BEND_MS, sample_rate);
        smooth[S_BEND_VCF] = Smoothed::new(0.0, BEND_MS, sample_rate);
        smooth[S_EXPR].target = 1.0;
        let mut t = Sei {
            os,
            sr: sample_rate,
            seed,
            values: params::defaults(),
            dirty: true,
            patch,
            smooth,
            voices,
            alloc: VoiceAllocator::new(),
            dec: OutStage::new(),
            out: [OutStage::new(); 2],
            dc_r: dc_pole(DC_HZ, sample_rate),
            chorus: Chorus::new(sample_rate, seed),
            lp: [0.0; 3],
            lp_a: [lp_coef(HPF2_HZ), lp_coef(HPF3_HZ), lp_coef(SHELF_HZ)],
            lfo_phase: 0.0,
            lfo_inc: 0.0,
            lfo_gain: 1.0,
            lfo_gain_inc: 1.0,
            rest: Rest::new(),
            hold_len: (HOLD_SECS * sample_rate) as u32,
        };
        t.apply_patch();
        t.all_targets();
        for s in &mut t.smooth {
            s.snap();
        }
        t.chorus.snap_settings();
        t
    }

    fn all_targets(&mut self) {
        for p in PARAMS {
            self.smooth_target(p.id);
        }
    }

    /// Sets the smoothed value a parameter drives, if it drives one.
    fn smooth_target(&mut self, id: u32) {
        let g = |id: u32| self.values[id as usize];
        let (t, v) = match id {
            SAW_ON => (S_SAW, g(SAW_ON) * MIX_GAIN),
            PULSE_ON => (S_PULSE, g(PULSE_ON) * MIX_GAIN),
            SUB => (S_SUB, g(SUB) / 10.0 * SUB_GAIN * MIX_GAIN),
            NOISE => (S_NOISE, g(NOISE) / 10.0 * NOISE_GAIN * MIX_GAIN),
            CUTOFF => (S_CUTOFF, hz_to_note(g(CUTOFF))),
            RESONANCE => (S_K, g(RESONANCE) / 10.0 * K_MAX),
            ENV_AMOUNT | ENV_POLARITY => (S_ENV, g(ENV_AMOUNT) / 10.0 * ENV_ST * if g(ENV_POLARITY) >= 0.5 { -1.0 } else { 1.0 }),
            KEYBOARD => (S_KBD, g(KEYBOARD) / 10.0),
            VCF_LFO => (S_VCF_LFO, g(VCF_LFO) / 10.0 * LFO_VCF_ST),
            DCO_LFO => (S_DCO_LFO, g(DCO_LFO) / 10.0),
            PWM | PWM_MODE => {
                // Manual, the knob is the width; LFO, the knob is how far the LFO takes it.
                let k = g(PWM) / 10.0;
                let lfo = g(PWM_MODE) < 0.5;
                self.smooth[S_PWM_LFO].target = if lfo { k } else { 0.0 };
                (S_PWM, if lfo { 0.0 } else { k })
            }
            VCA_MODE => (S_VCA, 1.0 - g(VCA_MODE)),
            LEVEL => {
                let l = g(LEVEL) / 10.0;
                (S_LEVEL, l * l * OUTPUT_GAIN)
            }
            VARIATION => (S_VARIATION, g(VARIATION)),
            WHEEL_LFO => return,
            HPF => {
                let p = g(HPF) as usize;
                self.smooth[S_H3].target = if p == 3 { 1.0 } else { 0.0 };
                self.smooth[S_SHELF].target = if p == 0 { SHELF_GAIN } else { 0.0 };
                (S_H2, if p == 2 { 1.0 } else { 0.0 })
            }
            _ => return,
        };
        self.smooth[t].target = v;
    }

    /// Works out everything stepped or costly from the parameters.
    fn apply_patch(&mut self) {
        self.dirty = false;
        let g = |id: u32| self.values[id as usize];
        let (rate, sr) = (self.patch.rate, self.sr);
        let ms = 0.001;
        self.patch.env = EnvRates::new(g(ATTACK) * ms, g(DECAY) * ms, g(SUSTAIN) / 10.0, g(RELEASE) * ms, rate);
        self.patch.glide_on = g(PORTA_ON) >= 0.5;
        self.patch.glide_secs = g(PORTA) * ms;
        self.patch.velocity = g(VELOCITY);
        self.patch.unison_st = g(UNISON_DETUNE) * 0.3;
        let hz = LFO_MIN_HZ * (LFO_MAX_HZ / LFO_MIN_HZ).powf(g(LFO_RATE) / 10.0);
        self.lfo_inc = hz as f64 / sr as f64;
        let delay = g(LFO_DELAY) / 10.0 * DELAY_MAX_S;
        self.lfo_gain_inc = if delay > 0.0 { 1.0 / (delay * sr) } else { 1.0 };
        self.chorus.set_mode(g(CHORUS) as usize);
        let assign = g(ASSIGN) as usize;
        let (mode, unison) = match assign {
            2 => (VoiceMode::Mono { priority: Priority::Last, multi_trigger: false }, VOICES),
            _ => (VoiceMode::Poly, 1),
        };
        // A unison stack is loud; it's quieter per voice, by power, but still bigger.
        self.patch.group_gain = 1.0 / (unison as f32).powf(0.35);
        self.alloc.set_rotate(assign == 0);
        let voices = &mut self.voices;
        self.alloc.set_mode(mode, unison, VOICES, |a| {
            if let VoiceAction::Release { voice } = a {
                voices[voice].release();
            }
        });
        for v in self.voices.iter_mut() {
            v.set_glide(&self.patch);
        }
    }

    fn act(voices: &mut [Voice; VOICES], patch: &Patch, action: VoiceAction) {
        match action {
            VoiceAction::Start { voice, note, velocity, unison, .. } => voices[voice].start(note, velocity, unison, patch),
            VoiceAction::Glide { voice, note } => voices[voice].glide_to(note),
            VoiceAction::Release { voice } => voices[voice].release(),
        }
    }

    fn handle(&mut self, m: Midi) {
        match m {
            Midi::NoteOn { note, velocity } => {
                if self.rest.resting {
                    let gone = self.rest.wake();
                    self.lfo_phase = (self.lfo_phase + gone as f64 * self.lfo_inc).fract();
                    self.chorus.skip(gone);
                }
                // The delay runs from the first key down.
                if !self.voices.iter().any(|v| v.gated) {
                    self.lfo_gain = 0.0;
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
            Midi::AllNotesOff => {
                let (voices, patch) = (&mut self.voices, &self.patch);
                self.alloc.release_all(|a| Self::act(voices, patch, a));
            }
            Midi::PitchBend(b) => {
                self.smooth[S_BEND].target = b * self.values[BEND_DCO as usize];
                self.smooth[S_BEND_VCF].target = b * self.values[BEND_VCF as usize] / 10.0 * BEND_VCF_ST;
            }
            Midi::ModWheel(w) => self.smooth[S_WHEEL].target = w,
            Midi::Expression(e) => self.smooth[S_EXPR].target = e,
            Midi::ResetControllers => self.reset_controllers(),
            Midi::Aftertouch(_) | Midi::Other => {}
        }
    }

    fn reset_controllers(&mut self) {
        self.smooth[S_BEND].target = 0.0;
        self.smooth[S_BEND_VCF].target = 0.0;
        self.smooth[S_WHEEL].target = 0.0;
        self.smooth[S_EXPR].target = 1.0;
    }

    fn enter_rest(&mut self) {
        self.rest.resting = true;
        self.chorus.clear();
        self.dec = OutStage::new();
        self.out = [OutStage::new(); 2];
        self.lp = [0.0; 3];
    }

    /// Plays a stretch with no events in it.
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        if self.rest.resting {
            l.fill(0.0);
            r.fill(0.0);
            self.rest.samples += n as u64;
            // Nothing's sounding: knobs and controllers land where they're going.
            for s in &mut self.smooth {
                s.snap();
            }
            self.chorus.snap_settings();
            return;
        }
        let wheel_lfo = self.values[WHEEL_LFO as usize] / 10.0;
        let noise_knob = self.values[CHORUS_NOISE as usize];
        let group = self.patch.group_gain;
        for i in 0..n {
            let s = &mut self.smooth;
            let mut v = [0.0f32; S_COUNT];
            for (o, x) in v.iter_mut().zip(s.iter_mut()) {
                *o = x.tick();
            }
            // The LFO: one, for everything.
            let tri = 1.0 - 4.0 * (self.lfo_phase as f32 - 0.5).abs();
            self.lfo_phase += self.lfo_inc;
            if self.lfo_phase >= 1.0 {
                self.lfo_phase -= 1.0;
            }
            self.lfo_gain = (self.lfo_gain + self.lfo_gain_inc).min(1.0);
            let lfo = tri * self.lfo_gain;
            let range = (self.values[RANGE as usize] as i32 - 1) as f32 * 12.0;
            let vibrato = (v[S_DCO_LFO] + v[S_WHEEL] * wheel_lfo).min(1.5) * LFO_DCO_ST * lfo;
            let width = 0.5 - (0.5 - WIDTH_MIN) * (v[S_PWM] + v[S_PWM_LFO] * (0.5 + 0.5 * lfo));
            let frame = Frame {
                pitch_st: range + v[S_BEND] + vibrato,
                width,
                saw: v[S_SAW],
                pulse: v[S_PULSE],
                sub: v[S_SUB],
                noise: v[S_NOISE],
                cutoff_note: v[S_CUTOFF],
                cutoff_mod_st: v[S_VCF_LFO] * lfo + v[S_BEND_VCF],
                k: v[S_K],
                env_st: v[S_ENV],
                kbd: v[S_KBD],
                vca_env: v[S_VCA],
                variation: v[S_VARIATION],
            };
            let mut bus = 0.0f32;
            let any = self.voices.iter().any(|v| v.active);
            if any {
                if self.os == 2 {
                    let (mut a0, mut a1) = (0.0f32, 0.0f32);
                    for v in self.voices.iter_mut().filter(|v| v.active) {
                        v.tick(&frame, &self.patch, &mut a0);
                    }
                    for v in self.voices.iter_mut().filter(|v| v.active) {
                        v.tick(&frame, &self.patch, &mut a1);
                    }
                    bus = self.dec.dec.process(a0, a1);
                } else {
                    for v in self.voices.iter_mut().filter(|v| v.active) {
                        v.tick(&frame, &self.patch, &mut bus);
                    }
                }
                bus *= group;
                self.rest.hold = self.hold_len;
            } else if self.os == 2 {
                bus = self.dec.dec.process(0.0, 0.0);
            }
            // The high-pass switch, after the voices: each position is the plain signal less (or,
            // at 0, plus) a low-pass of it, so moving it glides rather than clicks.
            self.lp[0] += (bus - self.lp[0]) * self.lp_a[0];
            self.lp[1] += (bus - self.lp[1]) * self.lp_a[1];
            self.lp[2] += (bus - self.lp[2]) * self.lp_a[2];
            let shaped = bus - v[S_H2] * self.lp[0] - v[S_H3] * self.lp[1] + v[S_SHELF] * self.lp[2];
            let leveled = shaped * v[S_LEVEL] * v[S_EXPR];
            let (cl, cr) = self.chorus.tick(leveled, any || self.rest.hold > self.hold_len / 2, noise_knob);
            l[i] = self.out[0].block_dc(cl, self.dc_r);
            r[i] = self.out[1].block_dc(cr, self.dc_r);
            for (k, voice) in self.voices.iter().enumerate() {
                if !voice.active && self.alloc.is_busy(k) {
                    self.alloc.voice_finished(k);
                }
            }
            if !any {
                self.rest.hold = self.rest.hold.saturating_sub(1);
                if self.rest.hold == 0 && !self.voices.iter().any(|v| v.active) {
                    self.enter_rest();
                    l[i + 1..].fill(0.0);
                    r[i + 1..].fill(0.0);
                    self.rest.samples += (n - i - 1) as u64;
                    return;
                }
            }
        }
    }
}

impl Instrument for Sei {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.values.get_mut(id as usize) else { return };
        *slot = value;
        self.smooth_target(id);
        if matches!(id, ATTACK | DECAY | SUSTAIN | RELEASE | PORTA_ON | PORTA | VELOCITY | UNISON_DETUNE | LFO_RATE | LFO_DELAY | CHORUS | ASSIGN) {
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
        let (voices, patch) = (&mut self.voices, &self.patch);
        self.alloc.release_all(|a| Self::act(voices, patch, a));
    }

    fn reset(&mut self) {
        if self.dirty {
            self.apply_patch();
        }
        let seed = self.seed;
        for (i, v) in self.voices.iter_mut().enumerate() {
            *v = Voice::new(seed, i);
            v.set_glide(&self.patch);
        }
        self.alloc.reset();
        self.dec = OutStage::new();
        self.out = [OutStage::new(); 2];
        self.lp = [0.0; 3];
        self.chorus.reset();
        self.lfo_phase = 0.0;
        self.lfo_gain = 1.0;
        self.rest = Rest::new();
        self.reset_controllers();
        for s in &mut self.smooth {
            s.snap();
        }
    }

    fn tail_secs(&self) -> f32 {
        // Falling to -100 dB takes 3.84 times the 95 % time.
        let release = self.values[RELEASE as usize] * 0.001;
        (release * 3.84).min(60.0) + HOLD_SECS + 0.05
    }
}

pub fn info() -> InstrumentInfo {
    InstrumentInfo {
        id: "builtin:sei",
        name: "Fermata Sei",
        group: "Vintage Synths",
        about: "Six digitally controlled voices, one envelope, one filter and a stereo bucket-brigade chorus",
        params: PARAMS,
        sections: SECTIONS,
        meters: &[],
        presets: PRESETS,
        new: |sample_rate, seed| Box::new(Sei::new(sample_rate, seed)),
    }
}

#[cfg(test)]
mod tests;
