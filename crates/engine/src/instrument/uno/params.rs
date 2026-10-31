//! The parameters of Fermata Uno. The ids are stable forever: projects and presets store them.

use crate::fx::{ParamSpec, Unit};
use crate::instrument::common::{choice, cont, percent, ten, time_ms};
use crate::instrument::Section;

pub mod param {
    pub const WAVE: u32 = 0;
    pub const TUNE: u32 = 1;
    pub const CUTOFF: u32 = 2;
    pub const RESONANCE: u32 = 3;
    pub const ENV_MOD: u32 = 4;
    pub const DECAY: u32 = 5;
    pub const ACCENT: u32 = 6;
    pub const VOLUME: u32 = 7;
    /// Velocity at and above which a note is accented.
    pub const ACCENT_VELOCITY: u32 = 8;
    pub const SLIDE_TIME: u32 = 9;
    pub const BEND_RANGE: u32 = 10;
    pub const DRIFT: u32 = 11;

    /// One past the highest id.
    pub const COUNT: usize = 12;
}

use param::*;

pub const PARAMS: &[ParamSpec] = &[
    choice(WAVE, "Waveform", &["Saw", "Square"], 0.0),
    cont(TUNE, "Tuning", -12.0, 12.0, 0.0, Unit::Semitones, false),
    cont(CUTOFF, "Cut Off Freq", 100.0, 5000.0, 650.0, Unit::Hz, true),
    ten(RESONANCE, "Resonance", 5.0),
    ten(ENV_MOD, "Env Mod", 5.0),
    time_ms(DECAY, "Decay", 200.0, 2000.0, 600.0),
    ten(ACCENT, "Accent", 5.0),
    ten(VOLUME, "Volume", 7.0),
    cont(ACCENT_VELOCITY, "Accent at Velocity", 1.0, 127.0, 110.0, Unit::None, false),
    time_ms(SLIDE_TIME, "Slide Time", 10.0, 500.0, 60.0),
    cont(BEND_RANGE, "Bend Range", 0.0, 12.0, 2.0, Unit::Semitones, false),
    percent(DRIFT, "Drift", 0.2),
];

pub const SECTIONS: &[Section] = &[
    Section { title: "Oscillator", params: &[WAVE, TUNE] },
    Section { title: "Filter", params: &[CUTOFF, RESONANCE, ENV_MOD, DECAY] },
    Section { title: "Accent", params: &[ACCENT, ACCENT_VELOCITY] },
    Section { title: "Performance", params: &[SLIDE_TIME, BEND_RANGE, DRIFT, VOLUME] },
];

/// Every parameter's default, by id.
pub fn defaults() -> [f32; COUNT] {
    let mut out = [0.0; COUNT];
    for p in PARAMS {
        out[p.id as usize] = p.default;
    }
    out
}
