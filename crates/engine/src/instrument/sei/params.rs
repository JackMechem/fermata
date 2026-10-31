//! The parameters of Fermata Sei. The ids are stable forever: projects and presets store them.

use crate::fx::{ParamSpec, Unit};
use crate::instrument::common::{choice, cont, percent, ten, time_ms, toggle};
use crate::instrument::Section;

pub mod param {
    pub const LFO_RATE: u32 = 0;
    pub const LFO_DELAY: u32 = 1;
    pub const DCO_LFO: u32 = 2;
    pub const PWM: u32 = 3;
    /// Whether the PWM knob is a manual width, or the LFO's depth.
    pub const PWM_MODE: u32 = 4;
    pub const RANGE: u32 = 5;
    pub const PULSE_ON: u32 = 6;
    pub const SAW_ON: u32 = 7;
    pub const SUB: u32 = 8;
    pub const NOISE: u32 = 9;
    pub const HPF: u32 = 10;
    pub const CUTOFF: u32 = 11;
    pub const RESONANCE: u32 = 12;
    /// Which way the envelope moves the filter.
    pub const ENV_POLARITY: u32 = 13;
    pub const ENV_AMOUNT: u32 = 14;
    pub const VCF_LFO: u32 = 15;
    pub const KEYBOARD: u32 = 16;
    /// The VCA follows the envelope, or the key.
    pub const VCA_MODE: u32 = 17;
    pub const LEVEL: u32 = 18;
    pub const ATTACK: u32 = 19;
    pub const DECAY: u32 = 20;
    pub const SUSTAIN: u32 = 21;
    pub const RELEASE: u32 = 22;
    pub const CHORUS: u32 = 23;
    pub const CHORUS_NOISE: u32 = 24;
    pub const BEND_DCO: u32 = 25;
    pub const BEND_VCF: u32 = 26;
    /// How much vibrato the lever pushed forward adds.
    pub const WHEEL_LFO: u32 = 27;
    pub const ASSIGN: u32 = 28;
    pub const PORTA_ON: u32 = 29;
    pub const PORTA: u32 = 30;
    pub const UNISON_DETUNE: u32 = 31;
    pub const VARIATION: u32 = 32;
    pub const VELOCITY: u32 = 33;

    /// One past the highest id.
    pub const COUNT: usize = 34;
}

use param::*;

pub const RANGES: &[&str] = &["16'", "8'", "4'"];
pub const HPF_POSITIONS: &[&str] = &["0 (Bass)", "1 (Flat)", "2", "3"];
pub const CHORUS_MODES: &[&str] = &["Off", "I", "II", "I+II"];
pub const ASSIGN_MODES: &[&str] = &["Poly 1", "Poly 2", "Unison"];

pub const PARAMS: &[ParamSpec] = &[
    ten(LFO_RATE, "LFO Rate", 5.0),
    ten(LFO_DELAY, "LFO Delay", 0.0),
    ten(DCO_LFO, "DCO LFO", 0.0),
    ten(PWM, "PWM", 3.0),
    choice(PWM_MODE, "PWM Mode", &["LFO", "Manual"], 1.0),
    choice(RANGE, "Range", RANGES, 1.0),
    toggle(PULSE_ON, "Pulse", 0.0),
    toggle(SAW_ON, "Saw", 1.0),
    ten(SUB, "Sub", 0.0),
    ten(NOISE, "Noise", 0.0),
    choice(HPF, "HPF", HPF_POSITIONS, 1.0),
    cont(CUTOFF, "Frequency", 20.0, 20000.0, 2500.0, Unit::Hz, true),
    ten(RESONANCE, "Resonance", 0.0),
    choice(ENV_POLARITY, "Env Polarity", &["Normal", "Inverted"], 0.0),
    ten(ENV_AMOUNT, "Env", 3.0),
    ten(VCF_LFO, "VCF LFO", 0.0),
    ten(KEYBOARD, "Keyboard", 5.0),
    choice(VCA_MODE, "VCA", &["Env", "Gate"], 0.0),
    ten(LEVEL, "Level", 7.0),
    time_ms(ATTACK, "Attack", 1.5, 3000.0, 1.5),
    time_ms(DECAY, "Decay", 1.5, 12000.0, 500.0),
    ten(SUSTAIN, "Sustain", 7.0),
    time_ms(RELEASE, "Release", 1.5, 12000.0, 180.0),
    choice(CHORUS, "Chorus", CHORUS_MODES, 1.0),
    percent(CHORUS_NOISE, "Chorus Noise", 0.5),
    cont(BEND_DCO, "Bend DCO", 0.0, 12.0, 2.0, Unit::Semitones, false),
    ten(BEND_VCF, "Bend VCF", 0.0),
    ten(WHEEL_LFO, "Wheel LFO", 5.0),
    choice(ASSIGN, "Assign", ASSIGN_MODES, 0.0),
    toggle(PORTA_ON, "Portamento On", 0.0),
    time_ms(PORTA, "Portamento", 1.0, 5000.0, 150.0),
    percent(UNISON_DETUNE, "Unison Detune", 0.0),
    percent(VARIATION, "Voice Variation", 0.25),
    percent(VELOCITY, "Velocity", 0.0),
];

pub const SECTIONS: &[Section] = &[
    Section { title: "LFO", params: &[LFO_RATE, LFO_DELAY] },
    Section { title: "DCO", params: &[RANGE, DCO_LFO, SAW_ON, SUB, PULSE_ON, NOISE, PWM, PWM_MODE] },
    Section { title: "HPF", params: &[HPF] },
    Section { title: "VCF", params: &[CUTOFF, RESONANCE, ENV_AMOUNT, ENV_POLARITY, VCF_LFO, KEYBOARD] },
    Section { title: "Envelope", params: &[ATTACK, DECAY, SUSTAIN, RELEASE] },
    Section { title: "VCA", params: &[VCA_MODE, LEVEL] },
    Section { title: "Chorus", params: &[CHORUS, CHORUS_NOISE] },
    Section { title: "Lever", params: &[BEND_DCO, BEND_VCF, WHEEL_LFO] },
    Section { title: "Voices", params: &[ASSIGN, VELOCITY, PORTA_ON, PORTA, UNISON_DETUNE, VARIATION] },
];

/// Every parameter's default, by id.
pub fn defaults() -> [f32; COUNT] {
    let mut out = [0.0; COUNT];
    for p in PARAMS {
        out[p.id as usize] = p.default;
    }
    out
}
