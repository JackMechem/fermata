//! Fermata Tre: a three-oscillator monophonic synth, modelled on the architecture and behaviour of
//! the Minimoog Model D (this project isn't affiliated with Moog Music, and the product name stays
//! clear of theirs). The model's reference is what's publicly documented about the instrument and
//! its circuit; nothing here was compared with the hardware. See docs/ARCHITECTURE.md for what is
//! modelled from documented behaviour and what's a guess.
//!
//! **Signal path, per voice, at twice the output rate below 64 kHz:** three oscillators and
//! noise into a mixer whose sum saturates (`tanh`), so its top half overdrives the next stage; the
//! output fed back into the mixer, as players did by patching the headphones into the external
//! input; a 4-pole transistor ladder, self-oscillating from Emphasis 8 and in tune; a VCA.
//! Filter and Loudness contours, Attack-Decay-Sustain, with one switch for whether key-up releases
//! at the Decay time or almost at once (the instrument's Decay switch).
//!
//! **Controllers:** Osc 3 and noise blend (Mod Mix) into the modulation, whose amount is the wheel
//! (plus Mod Amount, where the wheel rests, and aftertouch), which goes to the oscillators and
//! the filter at the audio rate by its two switches. Osc 3 can leave the keyboard to become an LFO
//! in its Lo range. Glide is a lag on the keyboard's voltage. In Mono, low-note priority and
//! single trigger, as the instrument.
//!
//! **Beyond the instrument:** Unison and Poly, velocity, aftertouch and note priority, from the
//! shared allocator. All default to what the instrument does, and all are off unless a preset
//! says otherwise.
//!
//! **Determinism:** the instance's seed (the slot's id) sets each voice's drift, tolerances, noise
//! and starting phases, `reset` puts all of them back, and nothing else varies.

use vst3_host::MidiEvent;

use super::{FxContext, Instrument, InstrumentInfo, Midi};
use crate::synth_dsp::alloc::{Priority, VoiceAction, VoiceAllocator, VoiceMode};
use crate::synth_dsp::env::EnvShape;
use crate::synth_dsp::osc::Wave;
use crate::synth_dsp::resample::Decimator2x;
use crate::synth_dsp::smooth::Smoothed;

pub mod params;
mod presets;
mod voice;

use params::param::*;
pub use params::{PARAMS, SECTIONS};
pub use presets::PRESETS;
use voice::{Frame, Patch, Voice, MAX_VOICES};

/// A released note with the Decay switch off: fast, but long enough not to click.
const FAST_RELEASE_SECS: f32 = 0.012;
/// Parameters' smoothing, in ms: enough that a knob or lane doesn't zipper.
const SMOOTH_MS: f32 = 6.0;
const BEND_MS: f32 = 2.0;
/// The modulation's reach at full, in semitones. Guesses: wide enough for what the instrument is
/// known for (from a vibrato to wild sweeps), since the documented range is only qualitative.
const MOD_OSC_ST: f32 = 12.0;
const MOD_FILTER_ST: f32 = 30.0;
/// Contour Amount at 10: four octaves up from the cutoff knob.
const CONTOUR_ST: f32 = 48.0;
/// Emphasis at 10; 4 is where a ladder starts to self-oscillate.
const K_MAX: f32 = 5.0;
/// A mixer knob at 10. At this level one oscillator bends slightly, and two or three overdrive.
const MIX_GAIN: f32 = 0.09;
const NOISE_GAIN: f32 = 0.09;
/// Feedback at 10.
const FEEDBACK_GAIN: f32 = 0.25;
/// The output's level at full volume.
const OUTPUT_GAIN: f32 = 1.5;
/// The DC blocker's corner, in Hz (the instrument's output is AC coupled).
const DC_HZ: f32 = 6.0;

/// The output stage of one channel: decimator and DC blocker.
#[derive(Clone, Copy)]
struct Out {
    dec: Decimator2x,
    x1: f32,
    y1: f32,
}

impl Out {
    fn new() -> Out {
        Out { dec: Decimator2x::new(), x1: 0.0, y1: 0.0 }
    }

    /// True when nothing inside it is still ringing: it would only turn zeros into zeros.
    fn at_rest(&self) -> bool {
        self.dec.at_rest() && self.x1 == 0.0 && self.y1 == 0.0
    }

    /// Takes the block's value through the DC blocker.
    #[inline]
    fn block_dc(&mut self, x: f32, r: f32) -> f32 {
        let y = x - self.x1 + r * self.y1;
        self.x1 = x;
        self.y1 = if y.abs() < 1e-15 { 0.0 } else { y };
        y
    }
}

/// Everything that moves smoothly.
struct Smooth {
    level: [Smoothed; 3],
    noise: Smoothed,
    feedback: Smoothed,
    cutoff: Smoothed,
    emphasis: Smoothed,
    contour: Smoothed,
    tune: Smoothed,
    tune2: Smoothed,
    tune3: Smoothed,
    mod_mix: Smoothed,
    mod_amount: Smoothed,
    /// The routing switches, so throwing one under a held note doesn't click.
    osc_mod: Smoothed,
    filter_mod: Smoothed,
    volume: Smoothed,
    drift: Smoothed,
    /// In semitones.
    bend: Smoothed,
    wheel: Smoothed,
    touch: Smoothed,
    expression: Smoothed,
}

fn hz_to_note(hz: f32) -> f32 {
    69.0 + 12.0 * (hz.max(1.0) / 440.0).log2()
}

impl Smooth {
    fn new(rate: f32) -> Smooth {
        let s = |v| Smoothed::new(v, SMOOTH_MS, rate);
        Smooth {
            level: [s(0.0), s(0.0), s(0.0)],
            noise: s(0.0),
            feedback: s(0.0),
            cutoff: s(0.0),
            emphasis: s(0.0),
            contour: s(0.0),
            tune: s(0.0),
            tune2: s(0.0),
            tune3: s(0.0),
            mod_mix: s(0.0),
            mod_amount: s(0.0),
            osc_mod: s(0.0),
            filter_mod: s(0.0),
            volume: s(0.0),
            drift: s(0.0),
            bend: Smoothed::new(0.0, BEND_MS, rate),
            wheel: s(0.0),
            touch: s(0.0),
            expression: Smoothed::new(1.0, SMOOTH_MS, rate),
        }
    }

    fn all(&mut self) -> [&mut Smoothed; 21] {
        let [l1, l2, l3] = &mut self.level;
        [
            l1,
            l2,
            l3,
            &mut self.noise,
            &mut self.feedback,
            &mut self.cutoff,
            &mut self.emphasis,
            &mut self.contour,
            &mut self.tune,
            &mut self.tune2,
            &mut self.tune3,
            &mut self.mod_mix,
            &mut self.mod_amount,
            &mut self.osc_mod,
            &mut self.filter_mod,
            &mut self.volume,
            &mut self.drift,
            &mut self.bend,
            &mut self.wheel,
            &mut self.touch,
            &mut self.expression,
        ]
    }

    fn snap(&mut self) {
        for s in self.all() {
            s.snap();
        }
    }

    fn settled(&mut self) -> bool {
        self.all().iter().all(|s| s.value == s.target)
    }
}

/// An instance.
pub struct Tre {
    /// Ticks at the voices' rate per output sample: 2, or 1 from 64 kHz up.
    os: usize,
    seed: u64,
    values: [f32; COUNT],
    /// Stepped or costly settings changed: work out `patch` at the next block.
    dirty: bool,
    patch: Patch,
    smooth: Smooth,
    voices: Box<[Voice; MAX_VOICES]>,
    alloc: VoiceAllocator<MAX_VOICES>,
    out: [Out; 2],
    dc_r: f32,
}

/// An oscillator's wave. Shark is a guess at the shape's proportions: a triangle with its peak well
/// past the middle. Wide and narrow are the common reading of the panel's pictures, 30 % and 10 %.
fn wave(osc: usize, choice: f32) -> Wave {
    let i = choice as usize;
    let tri = Wave::Triangle { width: 0.5 };
    let shark = Wave::Triangle { width: 0.8 };
    if osc < 2 {
        match i {
            0 => tri,
            1 => shark,
            2 => Wave::Saw,
            3 => Wave::Pulse { width: 0.5 },
            4 => Wave::Pulse { width: 0.3 },
            _ => Wave::Pulse { width: 0.1 },
        }
    } else {
        match i {
            0 => tri,
            1 => shark,
            2 => Wave::RevSaw,
            3 => Wave::Saw,
            4 => Wave::Pulse { width: 0.5 },
            _ => Wave::Pulse { width: 0.3 },
        }
    }
}

/// A range switch position's offset from 8', in semitones. Lo sits 6 octaves below 8': about 4 Hz
/// at middle C (a guess, chosen to be useful as an LFO, which is what it's for).
fn range_st(choice: f32) -> f32 {
    match choice as usize {
        0 => -72.0,
        1 => -24.0,
        2 => -12.0,
        3 => 0.0,
        4 => 12.0,
        _ => 24.0,
    }
}

impl Tre {
    pub fn new(sample_rate: f32, seed: u64) -> Tre {
        let os = if sample_rate < 64000.0 { 2 } else { 1 };
        let rate = sample_rate * os as f32;
        let patch = Patch {
            rate,
            range_st: [0.0; 3],
            lo: [false; 3],
            osc3_kbd: true,
            pink: false,
            kbd_track: 0.0,
            filter_env: EnvShape::default(),
            amp_env: EnvShape::default(),
            glide_on: false,
            glide_secs: 0.0,
            vel_loudness: 0.0,
            vel_filter_st: 0.0,
            // Noise power spreads over the whole of the faster rate.
            noise_scale: 1.0 / (os as f32).sqrt(),
            unison_st: 0.0,
            spread: 0.0,
            group_gain: 1.0,
        };
        let voices: Box<[Voice; MAX_VOICES]> = Box::new(std::array::from_fn(|i| Voice::new(seed, i, &patch)));
        let mut t = Tre {
            os,
            seed,
            values: params::defaults(),
            dirty: true,
            patch,
            smooth: Smooth::new(sample_rate),
            voices,
            alloc: VoiceAllocator::new(),
            out: [Out::new(); 2],
            dc_r: 1.0 - std::f32::consts::TAU * DC_HZ / sample_rate,
        };
        for p in PARAMS {
            t.smooth_target(p.id, p.default);
        }
        t.apply_patch();
        t.smooth.snap();
        t
    }

    /// Sets the smoothed value a parameter drives, if it drives one.
    fn smooth_target(&mut self, id: u32, v: f32) {
        let s = &mut self.smooth;
        match id {
            LEVEL1 | ON1 => s.level[0].target = self.values[LEVEL1 as usize] * self.values[ON1 as usize] * MIX_GAIN,
            LEVEL2 | ON2 => s.level[1].target = self.values[LEVEL2 as usize] * self.values[ON2 as usize] * MIX_GAIN,
            LEVEL3 | ON3 => s.level[2].target = self.values[LEVEL3 as usize] * self.values[ON3 as usize] * MIX_GAIN,
            NOISE | NOISE_ON => s.noise.target = self.values[NOISE as usize] * self.values[NOISE_ON as usize] * NOISE_GAIN,
            FEEDBACK | FEEDBACK_ON => s.feedback.target = self.values[FEEDBACK as usize] * self.values[FEEDBACK_ON as usize] * FEEDBACK_GAIN,
            CUTOFF => s.cutoff.target = hz_to_note(v),
            EMPHASIS => s.emphasis.target = v / 10.0 * K_MAX,
            CONTOUR => s.contour.target = v / 10.0 * CONTOUR_ST,
            TUNE => s.tune.target = v,
            TUNE2 => s.tune2.target = v,
            TUNE3 => s.tune3.target = v,
            MOD_MIX => s.mod_mix.target = v / 10.0,
            MOD_AMOUNT => s.mod_amount.target = v / 10.0,
            OSC_MOD => s.osc_mod.target = v,
            FILTER_MOD => s.filter_mod.target = v,
            VOLUME => {
                let g = v / 10.0;
                s.volume.target = g * g * OUTPUT_GAIN;
            }
            DRIFT => s.drift.target = v,
            _ => {}
        }
    }

    /// Works out everything stepped or costly from the parameters.
    fn apply_patch(&mut self) {
        self.dirty = false;
        let v = &self.values;
        let g = |id: u32| v[id as usize];
        let rate = self.patch.rate;
        for (k, (wave_id, range_id)) in [(WAVE1, RANGE1), (WAVE2, RANGE2), (WAVE3, RANGE3)].into_iter().enumerate() {
            let w = wave(k, g(wave_id));
            for voice in self.voices.iter_mut() {
                voice.set_wave(k, w);
            }
            self.patch.range_st[k] = range_st(g(range_id));
            self.patch.lo[k] = g(range_id) as usize == 0;
        }
        self.patch.osc3_kbd = g(OSC3_KBD) >= 0.5;
        self.patch.pink = g(NOISE_COLOR) >= 0.5;
        self.patch.kbd_track = g(KBD1) / 3.0 + g(KBD2) * 2.0 / 3.0;
        let ms = 0.001;
        let release = |id: u32| if g(DECAY_ON) >= 0.5 { g(id) * ms } else { FAST_RELEASE_SECS };
        self.patch.filter_env = EnvShape::new(g(F_ATTACK) * ms, g(F_DECAY) * ms, g(F_SUSTAIN) / 10.0, release(F_DECAY), rate);
        self.patch.amp_env = EnvShape::new(g(A_ATTACK) * ms, g(A_DECAY) * ms, g(A_SUSTAIN) / 10.0, release(A_DECAY), rate);
        self.patch.glide_on = g(GLIDE_ON) >= 0.5;
        self.patch.glide_secs = g(GLIDE) * ms;
        self.patch.vel_loudness = g(VEL_LOUDNESS);
        self.patch.vel_filter_st = g(VEL_FILTER) * 36.0;
        self.patch.unison_st = g(UNISON_DETUNE) * 0.5;
        self.patch.spread = g(SPREAD);
        let count = params::VOICE_COUNTS[(g(VOICES) as usize).min(params::VOICE_COUNTS.len() - 1)];
        let priority = match g(PRIORITY) as usize {
            0 => Priority::Low,
            1 => Priority::Last,
            _ => Priority::High,
        };
        let mono = VoiceMode::Mono { priority, multi_trigger: g(TRIGGER) >= 0.5 };
        let (mode, unison, polyphony) = match g(VOICE_MODE) as usize {
            0 => (mono, 1, 1),
            1 => (mono, count, count),
            _ => (VoiceMode::Poly, 1, count),
        };
        // A group shares the level of one voice, by power.
        self.patch.group_gain = 1.0 / (unison as f32).sqrt();
        let voices = &mut self.voices;
        self.alloc.set_mode(mode, unison, polyphony, |a| {
            if let VoiceAction::Release { voice } = a {
                voices[voice].release();
            }
        });
        for voice in self.voices.iter_mut() {
            voice.set_glide(&self.patch);
        }
    }

    fn act(voices: &mut [Voice; MAX_VOICES], patch: &Patch, action: VoiceAction) {
        match action {
            VoiceAction::Start { voice, note, velocity, unison, .. } => voices[voice].start(note, velocity, unison, patch),
            VoiceAction::Glide { voice, note } => voices[voice].glide_to(note),
            VoiceAction::Release { voice } => voices[voice].release(),
        }
    }

    fn handle(&mut self, m: Midi) {
        let (voices, patch) = (&mut self.voices, &self.patch);
        match m {
            Midi::NoteOn { note, velocity } => self.alloc.note_on(note, velocity, |a| Self::act(voices, patch, a)),
            Midi::NoteOff { note } => self.alloc.note_off(note, |a| Self::act(voices, patch, a)),
            Midi::Sustain(down) => self.alloc.set_pedal(down, |a| Self::act(voices, patch, a)),
            Midi::AllNotesOff => self.alloc.release_all(|a| Self::act(voices, patch, a)),
            Midi::PitchBend(b) => self.smooth.bend.target = b * self.values[BEND_RANGE as usize],
            Midi::ModWheel(w) => self.smooth.wheel.target = w,
            Midi::Aftertouch(a) => self.smooth.touch.target = a,
            Midi::Expression(e) => self.smooth.expression.target = e,
            Midi::ResetControllers => {
                self.smooth.bend.target = 0.0;
                self.smooth.wheel.target = 0.0;
                self.smooth.touch.target = 0.0;
                self.smooth.expression.target = 1.0;
            }
            Midi::Other => {}
        }
    }

    /// Whether playing another stretch would only produce silence and change nothing.
    fn at_rest(&mut self) -> bool {
        self.voices.iter().all(|v| !v.active) && self.out.iter().all(|o| o.at_rest()) && self.smooth.settled()
    }

    /// Plays a stretch with no events in it.
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.at_rest() {
            l.fill(0.0);
            r.fill(0.0);
            return;
        }
        let touch_mod = self.values[TOUCH_MOD as usize];
        let dc_r = self.dc_r;
        for (out_l, out_r) in l.iter_mut().zip(r.iter_mut()) {
            let s = &mut self.smooth;
            // The wheel squared, so the first part of its travel is a usable vibrato.
            let wheel = s.wheel.tick();
            let rest = s.mod_amount.tick();
            let touch = s.touch.tick() * touch_mod;
            let amount = (wheel + rest + touch).min(1.0);
            let amount = amount * amount;
            let frame = Frame {
                osc_gain: [s.level[0].tick(), s.level[1].tick(), s.level[2].tick()],
                noise_gain: s.noise.tick(),
                feedback_gain: s.feedback.tick(),
                cutoff_note: s.cutoff.tick(),
                k: s.emphasis.tick(),
                contour_st: s.contour.tick(),
                tune_st: s.tune.tick() + s.bend.tick(),
                tune2_st: s.tune2.tick(),
                tune3_st: s.tune3.tick(),
                mod_mix: s.mod_mix.tick(),
                mod_osc_st: s.osc_mod.tick() * amount * MOD_OSC_ST,
                mod_filter_st: s.filter_mod.tick() * amount * MOD_FILTER_ST,
                drift: s.drift.tick(),
            };
            let gain = s.volume.tick() * s.expression.tick();
            let (mut a, mut b) = (0.0f32, 0.0f32);
            if self.os == 2 {
                let (mut a0, mut b0) = (0.0f32, 0.0f32);
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.tick(&frame, &self.patch, &mut a0, &mut b0);
                }
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.tick(&frame, &self.patch, &mut a, &mut b);
                }
                a = self.out[0].dec.process(a0, a);
                b = self.out[1].dec.process(b0, b);
            } else {
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.tick(&frame, &self.patch, &mut a, &mut b);
                }
            }
            *out_l = self.out[0].block_dc(a, dc_r) * gain;
            *out_r = self.out[1].block_dc(b, dc_r) * gain;
            // Finished voices go back to the pool, on the sample they finished.
            for (i, v) in self.voices.iter().enumerate() {
                if !v.active && self.alloc.is_busy(i) {
                    self.alloc.voice_finished(i);
                }
            }
        }
    }
}

impl Instrument for Tre {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.values.get_mut(id as usize) else { return };
        *slot = value;
        // A mixer knob's value also goes through its switch, so both read `values`.
        self.smooth_target(id, value);
        if !matches!(id, LEVEL1 | LEVEL2 | LEVEL3 | NOISE | FEEDBACK | CUTOFF | EMPHASIS | CONTOUR | TUNE | TUNE2 | TUNE3 | MOD_MIX | MOD_AMOUNT | VOLUME | DRIFT | TOUCH_MOD | FILTER_MOD | OSC_MOD) {
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
        }
        self.alloc.reset();
        self.out = [Out::new(); 2];
        let s = &mut self.smooth;
        s.bend.target = 0.0;
        s.wheel.target = 0.0;
        s.touch.target = 0.0;
        s.expression.target = 1.0;
        self.smooth.snap();
        // New voices are at their defaults.
        self.dirty = true;
        self.apply_patch();
    }

    fn tail_secs(&self) -> f32 {
        // Falling to -100 dB takes 3.84 times the 95 % time.
        let g = |id: u32| self.values[id as usize];
        if g(DECAY_ON) >= 0.5 {
            (g(A_DECAY) * 0.001 * 3.84).min(60.0) + 0.05
        } else {
            0.1
        }
    }
}

pub fn info() -> InstrumentInfo {
    InstrumentInfo {
        id: "builtin:tre",
        name: "Fermata Tre",
        group: "Vintage Synths",
        about: "Three oscillators, a transistor ladder and two contours: a classic monophonic synth",
        params: PARAMS,
        sections: SECTIONS,
        meters: &[],
        presets: PRESETS,
        new: |sample_rate, seed| Box::new(Tre::new(sample_rate, seed)),
    }
}

#[cfg(test)]
mod tests;
