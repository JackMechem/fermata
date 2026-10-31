//! The parameters of Fermata Tre. The ids are stable forever: projects and presets store them.

use crate::fx::{ParamKind, ParamSpec, Unit};
use crate::instrument::Section;

pub mod param {
    pub const RANGE1: u32 = 0;
    pub const WAVE1: u32 = 1;
    pub const RANGE2: u32 = 2;
    pub const TUNE2: u32 = 3;
    pub const WAVE2: u32 = 4;
    pub const RANGE3: u32 = 5;
    pub const TUNE3: u32 = 6;
    pub const WAVE3: u32 = 7;
    /// Osc 3 follows the keyboard.
    pub const OSC3_KBD: u32 = 8;

    pub const ON1: u32 = 9;
    pub const LEVEL1: u32 = 10;
    pub const ON2: u32 = 11;
    pub const LEVEL2: u32 = 12;
    pub const ON3: u32 = 13;
    pub const LEVEL3: u32 = 14;
    pub const NOISE_ON: u32 = 15;
    pub const NOISE: u32 = 16;
    pub const NOISE_COLOR: u32 = 17;
    pub const FEEDBACK_ON: u32 = 18;
    pub const FEEDBACK: u32 = 19;

    pub const CUTOFF: u32 = 20;
    pub const EMPHASIS: u32 = 21;
    pub const CONTOUR: u32 = 22;
    pub const KBD1: u32 = 23;
    pub const KBD2: u32 = 24;
    pub const FILTER_MOD: u32 = 25;

    pub const F_ATTACK: u32 = 26;
    pub const F_DECAY: u32 = 27;
    pub const F_SUSTAIN: u32 = 28;
    pub const A_ATTACK: u32 = 29;
    pub const A_DECAY: u32 = 30;
    pub const A_SUSTAIN: u32 = 31;
    /// Whether key-up releases at the Decay time.
    pub const DECAY_ON: u32 = 32;

    pub const TUNE: u32 = 33;
    pub const GLIDE_ON: u32 = 34;
    pub const GLIDE: u32 = 35;
    pub const MOD_MIX: u32 = 36;
    pub const MOD_AMOUNT: u32 = 37;
    pub const OSC_MOD: u32 = 38;
    pub const BEND_RANGE: u32 = 39;
    pub const VOLUME: u32 = 40;
    pub const DRIFT: u32 = 41;

    pub const VOICE_MODE: u32 = 42;
    pub const VOICES: u32 = 43;
    pub const PRIORITY: u32 = 44;
    pub const TRIGGER: u32 = 45;
    pub const UNISON_DETUNE: u32 = 46;
    pub const SPREAD: u32 = 47;
    pub const VEL_LOUDNESS: u32 = 48;
    pub const VEL_FILTER: u32 = 49;
    pub const TOUCH_MOD: u32 = 50;

    /// One past the highest id.
    pub const COUNT: usize = 51;
}

use param::*;

pub const RANGES: &[&str] = &["Lo", "32'", "16'", "8'", "4'", "2'"];
pub const WAVES12: &[&str] = &["Triangle", "Shark", "Saw", "Square", "Wide", "Narrow"];
pub const WAVES3: &[&str] = &["Triangle", "Shark", "Rev Saw", "Saw", "Square", "Wide"];
pub const VOICE_MODES: &[&str] = &["Mono", "Unison", "Poly"];
/// What each VOICES choice means.
pub const VOICE_COUNTS: [usize; 5] = [2, 3, 4, 6, 8];

const fn cont(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind: ParamKind::Continuous, log }
}

/// A panel knob marked 0 to 10.
const fn ten(id: u32, name: &'static str, default: f32) -> ParamSpec {
    cont(id, name, 0.0, 10.0, default, Unit::None, false)
}

const fn toggle(id: u32, name: &'static str, default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 0.0, max: 1.0, default, unit: Unit::None, kind: ParamKind::Toggle, log: false }
}

const fn choice(id: u32, name: &'static str, labels: &'static [&'static str], default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 0.0, max: (labels.len() - 1) as f32, default, unit: Unit::None, kind: ParamKind::Choice(labels), log: false }
}

pub const PARAMS: &[ParamSpec] = &[
    choice(RANGE1, "Osc 1 Range", RANGES, 3.0),
    choice(WAVE1, "Osc 1 Wave", WAVES12, 2.0),
    choice(RANGE2, "Osc 2 Range", RANGES, 3.0),
    cont(TUNE2, "Osc 2 Frequency", -7.0, 7.0, 0.06, Unit::Semitones, false),
    choice(WAVE2, "Osc 2 Wave", WAVES12, 2.0),
    choice(RANGE3, "Osc 3 Range", RANGES, 3.0),
    cont(TUNE3, "Osc 3 Frequency", -7.0, 7.0, -0.05, Unit::Semitones, false),
    choice(WAVE3, "Osc 3 Wave", WAVES3, 3.0),
    toggle(OSC3_KBD, "Osc 3 Keyboard", 1.0),
    toggle(ON1, "Osc 1 On", 1.0),
    ten(LEVEL1, "Osc 1", 7.0),
    toggle(ON2, "Osc 2 On", 1.0),
    ten(LEVEL2, "Osc 2", 7.0),
    toggle(ON3, "Osc 3 On", 0.0),
    ten(LEVEL3, "Osc 3", 5.0),
    toggle(NOISE_ON, "Noise On", 0.0),
    ten(NOISE, "Noise", 5.0),
    choice(NOISE_COLOR, "Noise Color", &["White", "Pink"], 0.0),
    toggle(FEEDBACK_ON, "Feedback On", 0.0),
    ten(FEEDBACK, "Feedback", 3.0),
    cont(CUTOFF, "Cutoff", 20.0, 20000.0, 800.0, Unit::Hz, true),
    ten(EMPHASIS, "Emphasis", 2.0),
    ten(CONTOUR, "Contour Amount", 5.0),
    toggle(KBD1, "Keyboard 1/3", 1.0),
    toggle(KBD2, "Keyboard 2/3", 0.0),
    toggle(FILTER_MOD, "Filter Mod", 0.0),
    cont(F_ATTACK, "Filter Attack", 1.0, 10000.0, 5.0, Unit::Ms, true),
    cont(F_DECAY, "Filter Decay", 4.0, 24000.0, 400.0, Unit::Ms, true),
    ten(F_SUSTAIN, "Filter Sustain", 3.0),
    cont(A_ATTACK, "Loudness Attack", 1.0, 10000.0, 2.0, Unit::Ms, true),
    cont(A_DECAY, "Loudness Decay", 4.0, 24000.0, 300.0, Unit::Ms, true),
    ten(A_SUSTAIN, "Loudness Sustain", 8.0),
    toggle(DECAY_ON, "Decay", 1.0),
    cont(TUNE, "Tune", -2.5, 2.5, 0.0, Unit::Semitones, false),
    toggle(GLIDE_ON, "Glide On", 0.0),
    cont(GLIDE, "Glide", 1.0, 10000.0, 120.0, Unit::Ms, true),
    ten(MOD_MIX, "Mod Mix", 0.0),
    ten(MOD_AMOUNT, "Mod Amount", 0.0),
    toggle(OSC_MOD, "Osc Mod", 0.0),
    cont(BEND_RANGE, "Bend Range", 0.0, 12.0, 2.0, Unit::Semitones, false),
    ten(VOLUME, "Volume", 7.0),
    cont(DRIFT, "Drift", 0.0, 1.0, 0.25, Unit::Percent, false),
    choice(VOICE_MODE, "Voice Mode", VOICE_MODES, 0.0),
    choice(VOICES, "Voices", &["2", "3", "4", "6", "8"], 2.0),
    choice(PRIORITY, "Priority", &["Low", "Last", "High"], 0.0),
    choice(TRIGGER, "Trigger", &["Single", "Multi"], 0.0),
    cont(UNISON_DETUNE, "Unison Detune", 0.0, 1.0, 0.25, Unit::Percent, false),
    cont(SPREAD, "Stereo Spread", 0.0, 1.0, 0.5, Unit::Percent, false),
    cont(VEL_LOUDNESS, "Velocity to Loudness", 0.0, 1.0, 0.0, Unit::Percent, false),
    cont(VEL_FILTER, "Velocity to Filter", 0.0, 1.0, 0.0, Unit::Percent, false),
    cont(TOUCH_MOD, "Aftertouch to Mod", 0.0, 1.0, 0.0, Unit::Percent, false),
];

pub const SECTIONS: &[Section] = &[
    Section { title: "Oscillator 1", params: &[RANGE1, WAVE1] },
    Section { title: "Oscillator 2", params: &[RANGE2, WAVE2, TUNE2] },
    Section { title: "Oscillator 3", params: &[RANGE3, WAVE3, TUNE3, OSC3_KBD] },
    Section { title: "Mixer", params: &[ON1, LEVEL1, ON2, LEVEL2, ON3, LEVEL3, NOISE_ON, NOISE, FEEDBACK_ON, FEEDBACK, NOISE_COLOR] },
    Section { title: "Filter", params: &[CUTOFF, EMPHASIS, CONTOUR, KBD1, KBD2, FILTER_MOD] },
    Section { title: "Filter Contour", params: &[F_ATTACK, F_DECAY, F_SUSTAIN] },
    Section { title: "Loudness Contour", params: &[A_ATTACK, A_DECAY, A_SUSTAIN, DECAY_ON] },
    Section { title: "Controllers", params: &[TUNE, GLIDE_ON, GLIDE, MOD_MIX, MOD_AMOUNT, OSC_MOD, BEND_RANGE] },
    Section { title: "Output", params: &[VOLUME, DRIFT] },
    Section { title: "Voices", params: &[VOICE_MODE, VOICES, PRIORITY, TRIGGER, UNISON_DETUNE, SPREAD] },
    Section { title: "Touch", params: &[VEL_LOUDNESS, VEL_FILTER, TOUCH_MOD] },
];

/// Every parameter's default, by id.
pub fn defaults() -> [f32; COUNT] {
    let mut out = [0.0; COUNT];
    for p in PARAMS {
        out[p.id as usize] = p.default;
    }
    out
}
