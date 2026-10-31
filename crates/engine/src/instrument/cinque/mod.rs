//! Fermata Cinque: a five-voice polyphonic synth, modelled on the architecture and behaviour of
//! the Sequential Prophet-5 (this project isn't affiliated with Sequential, and the product name
//! stays clear of theirs). Its reference is what's publicly documented about the instrument; nothing
//! here was compared with the hardware. See docs/ARCHITECTURE.md.
//!
//! **Signal path, per voice, at twice the output rate below 64 kHz:** two VCOs (A: saw and pulse,
//! with sync to B; B: saw, triangle and pulse, with a low range and a keyboard switch) and noise
//! into a mixer whose sum saturates gently, a 4-pole OTA cascade with a choice of two chips' character,
//! and a VCA, with an ADSR for the filter and another for the amplifier.
//!
//! **Poly Mod is what makes it itself:** the filter envelope and Osc B, per voice and at the audio
//! rate, modulate Osc A's pitch, Osc A's pulse width and the filter. B keeps running when its mixer
//! knob is down, so it can be a pure modulator.
//!
//! **Wheel Mod:** one LFO, shared by the voices, blended with noise, through the wheel to five
//! switches. **Slop:** per-voice drift and tolerances, from the instance's seed.
//!
//! Velocity is off unless asked for: the instrument it's modelled on has none.

use vst3_host::MidiEvent;

use super::common::{dc_pole, hz_to_note, OutStage, Rest};
use super::{FxContext, Instrument, InstrumentInfo, Midi};
use crate::synth_dsp::alloc::{Priority, VoiceAction, VoiceAllocator, VoiceMode};
use crate::synth_dsp::analog::sub_seed;
use crate::synth_dsp::env::EnvShape;
use crate::synth_dsp::osc::Noise;
use crate::synth_dsp::smooth::Smoothed;

pub mod params;
mod presets;
mod voice;

use params::param::*;
pub use params::{PARAMS, SECTIONS};
pub use presets::PRESETS;
use voice::{Frame, Patch, Voice, VOICES};

const SMOOTH_MS: f32 = 6.0;
const BEND_MS: f32 = 2.0;
const FAST_RELEASE_SECS: f32 = 0.012;
/// The commonly cited LFO range, on an exponential knob.
const LFO_MIN_HZ: f32 = 0.04;
const LFO_MAX_HZ: f32 = 20.0;
/// Glide at 10. A guess.
const GLIDE_MAX_S: f32 = 4.0;
/// A mixer knob at 10: one oscillator stays clean, and two bend gently. A guess.
const MIX_GAIN: f32 = 0.055;
const NOISE_GAIN: f32 = 0.05;
/// Filter Env Amount at 10. A guess.
const ENV_ST: f32 = 72.0;
/// Resonance at 10, self-oscillating from about 9.
const K_MAX: f32 = 4.4;
/// The wheel's reach at full: guesses.
const WM_FREQ_ST: f32 = 12.0;
const WM_PW: f32 = 0.45;
const WM_FILTER_ST: f32 = 48.0;
const OUTPUT_GAIN: f32 = 1.2;
const DC_HZ: f32 = 6.0;
/// The two chips, as `OtaCascade`'s slew and bass: guesses. The early one bends sooner and keeps
/// more of the bottom end; the later one is cleaner, and thins out a little with resonance.
const CHIP_EARLY: (f32, f32) = (1.5, 0.55);
const CHIP_LATE: (f32, f32) = (0.8, 0.2);
const HOLD_SECS: f32 = 0.05;

// Smoothed values, by index.
const S_TUNE: usize = 0;
const S_BEND: usize = 1;
const S_A_ST: usize = 2;
const S_B_ST: usize = 3;
const S_B_FINE: usize = 4;
const S_A_PW: usize = 5;
const S_B_PW: usize = 6;
const S_A_SAW: usize = 7;
const S_A_PULSE: usize = 8;
const S_B_SAW: usize = 9;
const S_B_TRI: usize = 10;
const S_B_PULSE: usize = 11;
const S_MIX_A: usize = 12;
const S_MIX_B: usize = 13;
const S_NOISE: usize = 14;
const S_CUTOFF: usize = 15;
const S_K: usize = 16;
const S_ENV: usize = 17;
const S_PM_FENV: usize = 18;
const S_PM_OSCB: usize = 19;
const S_PM_FREQ_A: usize = 20;
const S_PM_PW_A: usize = 21;
const S_PM_FILTER: usize = 22;
const S_WM_MIX: usize = 23;
const S_WM_FREQ_A: usize = 24;
const S_WM_FREQ_B: usize = 25;
const S_WM_PW_A: usize = 26;
const S_WM_PW_B: usize = 27;
const S_WM_FILTER: usize = 28;
const S_WHEEL: usize = 29;
const S_REST: usize = 30;
const S_VOLUME: usize = 31;
const S_EXPR: usize = 32;
const S_SLOP: usize = 33;
const S_LFO_SAW: usize = 34;
const S_LFO_TRI: usize = 35;
const S_LFO_SQUARE: usize = 36;
const S_COUNT: usize = 37;

/// An instance.
pub struct Cinque {
    os: usize,
    seed: u64,
    values: [f32; params::param::COUNT],
    dirty: bool,
    patch: Patch,
    smooth: [Smoothed; S_COUNT],
    voices: Box<[Voice; VOICES]>,
    alloc: VoiceAllocator<VOICES>,
    out: OutStage,
    dc_r: f32,
    lfo_phase: f64,
    lfo_inc: f64,
    lfo_noise: Noise,
    rest: Rest,
    hold_len: u32,
    sr: f32,
}

impl Cinque {
    pub fn new(sample_rate: f32, seed: u64) -> Cinque {
        let os = if sample_rate < 64000.0 { 2 } else { 1 };
        let rate = sample_rate * os as f32;
        let patch = Patch {
            rate,
            b_lo: false,
            b_kbd: true,
            a_sync: false,
            kbd: 1.0,
            filter_env: EnvShape::default(),
            amp_env: EnvShape::default(),
            glide_secs: 0.0,
            unison_st: 0.0,
            group_gain: 1.0,
            noise_scale: 1.0 / (os as f32).sqrt(),
            vel_filter_st: 0.0,
            vel_amp: 0.0,
            slew: CHIP_LATE.0,
            bass: CHIP_LATE.1,
        };
        let voices: Box<[Voice; VOICES]> = Box::new(std::array::from_fn(|i| Voice::new(seed, i, &patch)));
        let mut smooth = [Smoothed::new(0.0, SMOOTH_MS, sample_rate); S_COUNT];
        smooth[S_BEND] = Smoothed::new(0.0, BEND_MS, sample_rate);
        smooth[S_EXPR].target = 1.0;
        let mut t = Cinque {
            os,
            seed,
            values: params::defaults(),
            dirty: true,
            patch,
            smooth,
            voices,
            alloc: VoiceAllocator::new(),
            out: OutStage::new(),
            dc_r: dc_pole(DC_HZ, sample_rate),
            lfo_phase: 0.0,
            lfo_inc: 0.0,
            lfo_noise: Noise::new(sub_seed(seed, 777)),
            rest: Rest::new(),
            hold_len: (HOLD_SECS * sample_rate) as u32,
            sr: sample_rate,
        };
        t.alloc.set_rotate(true);
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
        let (t, v) = match id {
            TUNE => (S_TUNE, g(TUNE)),
            A_FREQ => (S_A_ST, g(A_FREQ)),
            B_FREQ => (S_B_ST, g(B_FREQ)),
            B_FINE => (S_B_FINE, g(B_FINE)),
            A_PW => (S_A_PW, g(A_PW)),
            B_PW => (S_B_PW, g(B_PW)),
            A_SAW => (S_A_SAW, g(A_SAW)),
            A_PULSE => (S_A_PULSE, g(A_PULSE)),
            B_SAW => (S_B_SAW, g(B_SAW)),
            B_TRI => (S_B_TRI, g(B_TRI)),
            B_PULSE => (S_B_PULSE, g(B_PULSE)),
            MIX_A => (S_MIX_A, g(MIX_A) * MIX_GAIN),
            MIX_B => (S_MIX_B, g(MIX_B) * MIX_GAIN),
            MIX_NOISE => (S_NOISE, g(MIX_NOISE) * NOISE_GAIN),
            CUTOFF => (S_CUTOFF, hz_to_note(g(CUTOFF))),
            RESONANCE => (S_K, g(RESONANCE) / 10.0 * K_MAX),
            ENV_AMOUNT => (S_ENV, g(ENV_AMOUNT) / 10.0 * ENV_ST),
            PM_FENV => (S_PM_FENV, g(PM_FENV) / 10.0),
            PM_OSCB => (S_PM_OSCB, g(PM_OSCB) / 10.0),
            PM_FREQ_A => (S_PM_FREQ_A, g(PM_FREQ_A)),
            PM_PW_A => (S_PM_PW_A, g(PM_PW_A)),
            PM_FILTER => (S_PM_FILTER, g(PM_FILTER)),
            WM_MIX => (S_WM_MIX, g(WM_MIX) / 10.0),
            WM_FREQ_A => (S_WM_FREQ_A, g(WM_FREQ_A)),
            WM_FREQ_B => (S_WM_FREQ_B, g(WM_FREQ_B)),
            WM_PW_A => (S_WM_PW_A, g(WM_PW_A)),
            WM_PW_B => (S_WM_PW_B, g(WM_PW_B)),
            WM_FILTER => (S_WM_FILTER, g(WM_FILTER)),
            MOD_REST => (S_REST, g(MOD_REST) / 10.0),
            VOLUME => {
                let l = g(VOLUME) / 10.0;
                (S_VOLUME, l * l * OUTPUT_GAIN)
            }
            SLOP => (S_SLOP, g(SLOP)),
            LFO_SAW => (S_LFO_SAW, g(LFO_SAW)),
            LFO_TRI => (S_LFO_TRI, g(LFO_TRI)),
            LFO_SQUARE => (S_LFO_SQUARE, g(LFO_SQUARE)),
            _ => return,
        };
        self.smooth[t].target = v;
    }

    fn apply_patch(&mut self) {
        self.dirty = false;
        let g = |id: u32| self.values[id as usize];
        let (rate, ms) = (self.patch.rate, 0.001);
        let release = |id: u32| if g(RELEASE_ON) >= 0.5 { g(id) * ms } else { FAST_RELEASE_SECS };
        self.patch.filter_env = EnvShape::new(g(F_ATTACK) * ms, g(F_DECAY) * ms, g(F_SUSTAIN) / 10.0, release(F_RELEASE), rate);
        self.patch.amp_env = EnvShape::new(g(A_ATTACK) * ms, g(A_DECAY) * ms, g(A_SUSTAIN) / 10.0, release(A_RELEASE), rate);
        self.patch.b_lo = g(B_LO) >= 0.5;
        self.patch.b_kbd = g(B_KBD) >= 0.5;
        self.patch.a_sync = g(A_SYNC) >= 0.5;
        self.patch.kbd = g(KEYBOARD) * 0.5;
        let glide = g(GLIDE) / 10.0;
        self.patch.glide_secs = glide * glide * GLIDE_MAX_S;
        self.patch.unison_st = g(UNISON_DETUNE) * 0.25;
        self.patch.vel_filter_st = g(VEL_FILTER) * 36.0;
        self.patch.vel_amp = g(VEL_AMP);
        let (slew, bass) = if g(CHIP) < 0.5 { CHIP_EARLY } else { CHIP_LATE };
        self.patch.slew = slew;
        self.patch.bass = bass;
        let hz = LFO_MIN_HZ * (LFO_MAX_HZ / LFO_MIN_HZ).powf(g(LFO_RATE) / 10.0);
        self.lfo_inc = hz as f64 / self.sr as f64;
        let unison = g(UNISON) >= 0.5;
        // In Unison, the lowest key and all five voices: the instrument's way.
        let (mode, size) = if unison { (VoiceMode::Mono { priority: Priority::Low, multi_trigger: g(UNISON_RETRIG) >= 0.5 }, VOICES) } else { (VoiceMode::Poly, 1) };
        self.patch.group_gain = 1.0 / (size as f32).powf(0.4);
        let voices = &mut self.voices;
        self.alloc.set_mode(mode, size, VOICES, |a| {
            if let VoiceAction::Release { voice } = a {
                voices[voice].release();
            }
        });
        for v in self.voices.iter_mut() {
            v.configure(&self.patch);
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
        let (voices, patch) = (&mut self.voices, &self.patch);
        match m {
            Midi::NoteOn { note, velocity } => {
                if self.rest.resting {
                    let gone = self.rest.wake();
                    self.lfo_phase = (self.lfo_phase + gone as f64 * self.lfo_inc).fract();
                }
                self.alloc.note_on(note, velocity, |a| Self::act(voices, patch, a));
            }
            Midi::NoteOff { note } => self.alloc.note_off(note, |a| Self::act(voices, patch, a)),
            Midi::Sustain(down) => self.alloc.set_pedal(down, |a| Self::act(voices, patch, a)),
            Midi::AllNotesOff => self.alloc.release_all(|a| Self::act(voices, patch, a)),
            Midi::PitchBend(b) => self.smooth[S_BEND].target = b * self.values[BEND_RANGE as usize],
            Midi::ModWheel(w) => self.smooth[S_WHEEL].target = w,
            Midi::Expression(e) => self.smooth[S_EXPR].target = e,
            Midi::ResetControllers => self.reset_controllers(),
            Midi::Aftertouch(_) | Midi::Other => {}
        }
    }

    fn reset_controllers(&mut self) {
        self.smooth[S_BEND].target = 0.0;
        self.smooth[S_WHEEL].target = 0.0;
        self.smooth[S_EXPR].target = 1.0;
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
            // The one LFO: any of its waves, together, and noise through the same knob.
            let p = self.lfo_phase as f32;
            self.lfo_phase += self.lfo_inc;
            if self.lfo_phase >= 1.0 {
                self.lfo_phase -= 1.0;
            }
            let shapes = v[S_LFO_SAW] * (2.0 * p - 1.0) + v[S_LFO_TRI] * (1.0 - 4.0 * (p - 0.5).abs()) + v[S_LFO_SQUARE] * if p < 0.5 { 1.0 } else { -1.0 };
            let source = shapes + (self.lfo_noise.white() - shapes) * v[S_WM_MIX];
            // The wheel squared, so the first part of its travel is a usable vibrato.
            let amount = (v[S_WHEEL] + v[S_REST]).min(1.0);
            let m = source * amount * amount;
            let frame = Frame {
                tune_st: v[S_TUNE] + v[S_BEND],
                a_st: v[S_A_ST],
                b_st: v[S_B_ST],
                b_fine: v[S_B_FINE],
                a_pw: v[S_A_PW],
                b_pw: v[S_B_PW],
                a_saw: v[S_A_SAW],
                a_pulse: v[S_A_PULSE],
                b_saw: v[S_B_SAW],
                b_tri: v[S_B_TRI],
                b_pulse: v[S_B_PULSE],
                mix_a: v[S_MIX_A],
                mix_b: v[S_MIX_B],
                noise: v[S_NOISE],
                cutoff_note: v[S_CUTOFF],
                k: v[S_K],
                env_st: v[S_ENV],
                pm_fenv: v[S_PM_FENV],
                pm_oscb: v[S_PM_OSCB],
                pm_freq_a: v[S_PM_FREQ_A],
                pm_pw_a: v[S_PM_PW_A],
                pm_filter: v[S_PM_FILTER],
                wm_freq_a: m * v[S_WM_FREQ_A] * WM_FREQ_ST,
                wm_freq_b: m * v[S_WM_FREQ_B] * WM_FREQ_ST,
                wm_pw_a: m * v[S_WM_PW_A] * WM_PW,
                wm_pw_b: m * v[S_WM_PW_B] * WM_PW,
                wm_filter: m * v[S_WM_FILTER] * WM_FILTER_ST,
                slop: v[S_SLOP],
            };
            let any = self.voices.iter().any(|x| x.active);
            let mut bus = 0.0f32;
            if self.os == 2 {
                let (mut a0, mut a1) = (0.0f32, 0.0f32);
                if any {
                    for x in self.voices.iter_mut().filter(|x| x.active) {
                        x.tick(&frame, &self.patch, &mut a0);
                    }
                    for x in self.voices.iter_mut().filter(|x| x.active) {
                        x.tick(&frame, &self.patch, &mut a1);
                    }
                }
                bus = self.out.dec.process(a0, a1);
            } else if any {
                for x in self.voices.iter_mut().filter(|x| x.active) {
                    x.tick(&frame, &self.patch, &mut bus);
                }
            }
            let y = self.out.block_dc(bus, self.dc_r) * v[S_VOLUME] * v[S_EXPR];
            l[i] = y;
            r[i] = y;
            for (k, voice) in self.voices.iter().enumerate() {
                if !voice.active && self.alloc.is_busy(k) {
                    self.alloc.voice_finished(k);
                }
            }
            if any {
                self.rest.hold = self.hold_len;
            } else {
                self.rest.hold = self.rest.hold.saturating_sub(1);
                if self.rest.hold == 0 {
                    self.rest.resting = true;
                    self.out = OutStage::new();
                    l[i + 1..].fill(0.0);
                    r[i + 1..].fill(0.0);
                    self.rest.samples += (n - i - 1) as u64;
                    return;
                }
            }
        }
    }
}

impl Instrument for Cinque {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.values.get_mut(id as usize) else { return };
        *slot = value;
        self.smooth_target(id);
        if matches!(id, F_ATTACK | F_DECAY | F_SUSTAIN | F_RELEASE | A_ATTACK | A_DECAY | A_SUSTAIN | A_RELEASE | RELEASE_ON | B_LO | B_KBD | A_SYNC | KEYBOARD | GLIDE | UNISON_DETUNE | VEL_FILTER | VEL_AMP | CHIP | LFO_RATE | UNISON | UNISON_RETRIG) {
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
        let (seed, patch) = (self.seed, self.patch);
        for (i, v) in self.voices.iter_mut().enumerate() {
            *v = Voice::new(seed, i, &patch);
            v.configure(&patch);
        }
        self.alloc.reset();
        self.out = OutStage::new();
        self.lfo_phase = 0.0;
        self.lfo_noise = Noise::new(sub_seed(seed, 777));
        self.rest = Rest::new();
        self.reset_controllers();
        for s in &mut self.smooth {
            s.snap();
        }
    }

    fn tail_secs(&self) -> f32 {
        if self.values[RELEASE_ON as usize] >= 0.5 {
            (self.values[A_RELEASE as usize] * 0.001 * 3.84).min(60.0) + HOLD_SECS + 0.05
        } else {
            0.15
        }
    }
}

pub fn info() -> InstrumentInfo {
    InstrumentInfo {
        id: "builtin:cinque",
        name: "Fermata Cinque",
        group: "Vintage Synths",
        about: "Five voices of two oscillators each, a 4-pole filter, and Poly Mod from the filter envelope and Osc B",
        params: PARAMS,
        sections: SECTIONS,
        meters: &[],
        presets: PRESETS,
        new: |sample_rate, seed| Box::new(Cinque::new(sample_rate, seed)),
    }
}

#[cfg(test)]
mod tests;
