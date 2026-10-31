//! The parameters of Fermata Cinque. The ids are stable forever: projects and presets store them.

use crate::fx::{ParamSpec, Unit};
use crate::instrument::common::{choice, cont, percent, ten, time_ms, toggle};
use crate::instrument::Section;

pub mod param {
    // Poly Mod: its two sources and three destinations.
    pub const PM_FENV: u32 = 0;
    pub const PM_OSCB: u32 = 1;
    pub const PM_FREQ_A: u32 = 2;
    pub const PM_PW_A: u32 = 3;
    pub const PM_FILTER: u32 = 4;
    // LFO.
    pub const LFO_RATE: u32 = 5;
    pub const LFO_SAW: u32 = 6;
    pub const LFO_TRI: u32 = 7;
    pub const LFO_SQUARE: u32 = 8;
    // Wheel Mod.
    pub const WM_MIX: u32 = 9;
    pub const WM_FREQ_A: u32 = 10;
    pub const WM_FREQ_B: u32 = 11;
    pub const WM_PW_A: u32 = 12;
    pub const WM_PW_B: u32 = 13;
    pub const WM_FILTER: u32 = 14;
    // Oscillator A.
    pub const A_FREQ: u32 = 15;
    pub const A_SAW: u32 = 16;
    pub const A_PULSE: u32 = 17;
    pub const A_PW: u32 = 18;
    pub const A_SYNC: u32 = 19;
    // Oscillator B.
    pub const B_FREQ: u32 = 20;
    pub const B_FINE: u32 = 21;
    pub const B_SAW: u32 = 22;
    pub const B_TRI: u32 = 23;
    pub const B_PULSE: u32 = 24;
    pub const B_PW: u32 = 25;
    pub const B_LO: u32 = 26;
    pub const B_KBD: u32 = 27;
    // Mixer.
    pub const MIX_A: u32 = 28;
    pub const MIX_B: u32 = 29;
    pub const MIX_NOISE: u32 = 30;
    // Filter.
    pub const CUTOFF: u32 = 31;
    pub const RESONANCE: u32 = 32;
    pub const ENV_AMOUNT: u32 = 33;
    pub const KEYBOARD: u32 = 34;
    pub const CHIP: u32 = 35;
    // Envelopes.
    pub const F_ATTACK: u32 = 36;
    pub const F_DECAY: u32 = 37;
    pub const F_SUSTAIN: u32 = 38;
    pub const F_RELEASE: u32 = 39;
    pub const A_ATTACK: u32 = 40;
    pub const A_DECAY: u32 = 41;
    pub const A_SUSTAIN: u32 = 42;
    pub const A_RELEASE: u32 = 43;
    /// Whether key-up uses the release knobs, or ends almost at once.
    pub const RELEASE_ON: u32 = 44;
    // Performance.
    pub const GLIDE: u32 = 45;
    pub const UNISON: u32 = 46;
    pub const UNISON_RETRIG: u32 = 47;
    pub const UNISON_DETUNE: u32 = 48;
    pub const BEND_RANGE: u32 = 49;
    pub const MOD_REST: u32 = 50;
    pub const VOLUME: u32 = 51;
    pub const TUNE: u32 = 52;
    pub const SLOP: u32 = 53;
    pub const VEL_FILTER: u32 = 54;
    pub const VEL_AMP: u32 = 55;

    /// One past the highest id.
    pub const COUNT: usize = 56;
}

use param::*;

pub const PARAMS: &[ParamSpec] = &[
    ten(PM_FENV, "Filter Env", 0.0),
    ten(PM_OSCB, "Osc B", 0.0),
    toggle(PM_FREQ_A, "Freq A", 0.0),
    toggle(PM_PW_A, "PW A", 0.0),
    toggle(PM_FILTER, "Filter", 0.0),
    ten(LFO_RATE, "Rate", 5.0),
    toggle(LFO_SAW, "Saw", 0.0),
    toggle(LFO_TRI, "Triangle", 1.0),
    toggle(LFO_SQUARE, "Square", 0.0),
    cont(WM_MIX, "LFO / Noise", 0.0, 10.0, 0.0, Unit::None, false),
    toggle(WM_FREQ_A, "Freq A", 1.0),
    toggle(WM_FREQ_B, "Freq B", 1.0),
    toggle(WM_PW_A, "PW A", 0.0),
    toggle(WM_PW_B, "PW B", 0.0),
    toggle(WM_FILTER, "Filter", 0.0),
    cont(A_FREQ, "Frequency", -24.0, 36.0, 0.0, Unit::Semitones, false),
    toggle(A_SAW, "Saw", 1.0),
    toggle(A_PULSE, "Pulse", 0.0),
    cont(A_PW, "Pulse Width", 0.01, 0.99, 0.5, Unit::None, false),
    toggle(A_SYNC, "Sync", 0.0),
    cont(B_FREQ, "Frequency", -24.0, 36.0, 0.0, Unit::Semitones, false),
    cont(B_FINE, "Fine", -1.0, 1.0, 0.07, Unit::Semitones, false),
    toggle(B_SAW, "Saw", 1.0),
    toggle(B_TRI, "Triangle", 0.0),
    toggle(B_PULSE, "Pulse", 0.0),
    cont(B_PW, "Pulse Width", 0.01, 0.99, 0.5, Unit::None, false),
    toggle(B_LO, "Lo Freq", 0.0),
    toggle(B_KBD, "Keyboard", 1.0),
    ten(MIX_A, "Osc A", 8.0),
    ten(MIX_B, "Osc B", 8.0),
    ten(MIX_NOISE, "Noise", 0.0),
    cont(CUTOFF, "Cutoff", 20.0, 20000.0, 1200.0, Unit::Hz, true),
    ten(RESONANCE, "Resonance", 1.0),
    ten(ENV_AMOUNT, "Env Amount", 4.0),
    choice(KEYBOARD, "Keyboard", &["Off", "Half", "Full"], 2.0),
    choice(CHIP, "Filter Chip", &["Early", "Late"], 1.0),
    time_ms(F_ATTACK, "Attack", 1.0, 10000.0, 2.0),
    time_ms(F_DECAY, "Decay", 2.0, 10000.0, 500.0),
    ten(F_SUSTAIN, "Sustain", 4.0),
    time_ms(F_RELEASE, "Release", 2.0, 10000.0, 300.0),
    time_ms(A_ATTACK, "Attack", 1.0, 10000.0, 2.0),
    time_ms(A_DECAY, "Decay", 2.0, 10000.0, 800.0),
    ten(A_SUSTAIN, "Sustain", 8.0),
    time_ms(A_RELEASE, "Release", 2.0, 10000.0, 300.0),
    toggle(RELEASE_ON, "Release On", 1.0),
    ten(GLIDE, "Glide", 0.0),
    toggle(UNISON, "Unison", 0.0),
    toggle(UNISON_RETRIG, "Retrigger", 0.0),
    percent(UNISON_DETUNE, "Unison Detune", 0.2),
    cont(BEND_RANGE, "Bend Range", 0.0, 12.0, 2.0, Unit::Semitones, false),
    ten(MOD_REST, "Mod Amount", 0.0),
    ten(VOLUME, "Volume", 7.0),
    cont(TUNE, "Tune", -2.5, 2.5, 0.0, Unit::Semitones, false),
    percent(SLOP, "Slop", 0.25),
    percent(VEL_FILTER, "Velocity to Filter", 0.0),
    percent(VEL_AMP, "Velocity to Loudness", 0.0),
];

pub const SECTIONS: &[Section] = &[
    Section { title: "Poly Mod Sources", params: &[PM_FENV, PM_OSCB] },
    Section { title: "Poly Mod To", params: &[PM_FREQ_A, PM_PW_A, PM_FILTER] },
    Section { title: "LFO", params: &[LFO_RATE, LFO_SAW, LFO_TRI, LFO_SQUARE] },
    Section { title: "Wheel Mod", params: &[WM_MIX, MOD_REST] },
    Section { title: "Wheel Mod To", params: &[WM_FREQ_A, WM_PW_A, WM_FREQ_B, WM_PW_B, WM_FILTER] },
    Section { title: "Oscillator A", params: &[A_FREQ, A_PW, A_SAW, A_PULSE, A_SYNC] },
    Section { title: "Oscillator B", params: &[B_FREQ, B_FINE, B_PW, B_LO, B_SAW, B_TRI, B_PULSE, B_KBD] },
    Section { title: "Mixer", params: &[MIX_A, MIX_B, MIX_NOISE] },
    Section { title: "Filter", params: &[CUTOFF, RESONANCE, ENV_AMOUNT, KEYBOARD, CHIP] },
    Section { title: "Filter Envelope", params: &[F_ATTACK, F_DECAY, F_SUSTAIN, F_RELEASE] },
    Section { title: "Amp Envelope", params: &[A_ATTACK, A_DECAY, A_SUSTAIN, A_RELEASE] },
    Section { title: "Performance", params: &[RELEASE_ON, GLIDE, UNISON, UNISON_DETUNE, UNISON_RETRIG, BEND_RANGE] },
    Section { title: "Output", params: &[VOLUME, TUNE, SLOP] },
    Section { title: "Touch", params: &[VEL_FILTER, VEL_AMP] },
];

/// Every parameter's default, by id.
pub fn defaults() -> [f32; COUNT] {
    let mut out = [0.0; COUNT];
    for p in PARAMS {
        out[p.id as usize] = p.default;
    }
    out
}
