//! The parameters of Fermata Sampler, numeric settings of the whole instrument. Each zone's own
//! settings are in its state (`state.rs`). The ids are stable forever: projects and presets store
//! them.

use crate::fx::{FactoryPreset, ParamKind, ParamSpec, Unit};
use crate::instrument::Section;

pub mod param {
    pub const VOLUME: u32 = 0;
    pub const TUNE: u32 = 1;
    pub const TRANSPOSE: u32 = 2;
    pub const MODE: u32 = 3;
    pub const POLYPHONY: u32 = 4;
    pub const GLIDE: u32 = 5;
    pub const BEND: u32 = 6;
    pub const VEL_AMP: u32 = 7;
    pub const A_ATTACK: u32 = 8;
    pub const A_DECAY: u32 = 9;
    pub const A_SUSTAIN: u32 = 10;
    pub const A_RELEASE: u32 = 11;
    pub const F_TYPE: u32 = 12;
    pub const CUTOFF: u32 = 13;
    pub const RESONANCE: u32 = 14;
    pub const KEY_TRACK: u32 = 15;
    pub const F_ENV: u32 = 16;
    pub const VEL_CUTOFF: u32 = 17;
    pub const F_ATTACK: u32 = 18;
    pub const F_DECAY: u32 = 19;
    pub const F_SUSTAIN: u32 = 20;
    pub const F_RELEASE: u32 = 21;
    pub const LFO_WAVE: u32 = 22;
    pub const LFO_RATE: u32 = 23;
    pub const LFO_SYNC: u32 = 24;
    pub const LFO_PITCH: u32 = 25;
    pub const LFO_FILTER: u32 = 26;
    pub const WHEEL_PITCH: u32 = 27;
}
use param::*;

pub const POLY_COUNTS: [usize; 11] = [1, 2, 4, 6, 8, 12, 16, 24, 32, 48, 64];
pub const LFO_SYNC_PER_BEAT: [f64; 6] = [0.0, 0.25, 0.5, 1.0, 2.0, 4.0];

const fn cont(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind: ParamKind::Continuous, log }
}

const fn choice(id: u32, name: &'static str, labels: &'static [&'static str], default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 0.0, max: (labels.len() - 1) as f32, default, unit: Unit::None, kind: ParamKind::Choice(labels), log: false }
}

pub const PARAMS: &[ParamSpec] = &[
    cont(VOLUME, "Volume", -60.0, 12.0, 0.0, Unit::Db, false),
    cont(TUNE, "Tune", -100.0, 100.0, 0.0, Unit::None, false),
    cont(TRANSPOSE, "Transpose", -36.0, 36.0, 0.0, Unit::Semitones, false),
    choice(MODE, "Voices", &["Poly", "Mono", "Legato"], 0.0),
    choice(POLYPHONY, "Polyphony", &["1", "2", "4", "6", "8", "12", "16", "24", "32", "48", "64"], 8.0),
    cont(GLIDE, "Glide", 0.0, 2000.0, 0.0, Unit::Ms, false),
    cont(BEND, "Bend Range", 0.0, 24.0, 2.0, Unit::Semitones, false),
    cont(VEL_AMP, "Velocity", 0.0, 1.0, 1.0, Unit::Percent, false),
    cont(A_ATTACK, "Attack", 0.1, 20000.0, 0.1, Unit::Ms, true),
    cont(A_DECAY, "Decay", 1.0, 20000.0, 500.0, Unit::Ms, true),
    cont(A_SUSTAIN, "Sustain", 0.0, 1.0, 1.0, Unit::Percent, false),
    cont(A_RELEASE, "Release", 1.0, 30000.0, 60.0, Unit::Ms, true),
    choice(F_TYPE, "Filter", &["Off", "Low 12", "Band 12", "High 12", "Ladder 24"], 0.0),
    cont(CUTOFF, "Cutoff", 20.0, 20000.0, 20000.0, Unit::Hz, true),
    cont(RESONANCE, "Resonance", 0.0, 1.0, 0.0, Unit::Percent, false),
    cont(KEY_TRACK, "Key Track", 0.0, 1.0, 0.0, Unit::Percent, false),
    cont(F_ENV, "Env Amount", -96.0, 96.0, 0.0, Unit::Semitones, false),
    cont(VEL_CUTOFF, "Velocity", 0.0, 60.0, 0.0, Unit::Semitones, false),
    cont(F_ATTACK, "Attack", 0.1, 20000.0, 1.0, Unit::Ms, true),
    cont(F_DECAY, "Decay", 1.0, 20000.0, 400.0, Unit::Ms, true),
    cont(F_SUSTAIN, "Sustain", 0.0, 1.0, 1.0, Unit::Percent, false),
    cont(F_RELEASE, "Release", 1.0, 30000.0, 200.0, Unit::Ms, true),
    choice(LFO_WAVE, "Wave", &["Sine", "Triangle", "Saw", "Square", "Random"], 0.0),
    cont(LFO_RATE, "Rate", 0.05, 30.0, 5.0, Unit::Hz, true),
    choice(LFO_SYNC, "Sync", &["Off", "1 Bar", "1/2", "1/4", "1/8", "1/16"], 0.0),
    cont(LFO_PITCH, "To Pitch", 0.0, 1200.0, 0.0, Unit::None, false),
    cont(LFO_FILTER, "To Filter", 0.0, 48.0, 0.0, Unit::Semitones, false),
    cont(WHEEL_PITCH, "Wheel to Pitch", 0.0, 200.0, 40.0, Unit::None, false),
];

pub const SECTIONS: &[Section] = &[
    Section { title: "Main", params: &[VOLUME, TUNE, TRANSPOSE, MODE, POLYPHONY, GLIDE, BEND, VEL_AMP] },
    Section { title: "Amp Envelope", params: &[A_ATTACK, A_DECAY, A_SUSTAIN, A_RELEASE] },
    Section { title: "Filter", params: &[F_TYPE, CUTOFF, RESONANCE, KEY_TRACK, F_ENV, VEL_CUTOFF] },
    Section { title: "Filter Envelope", params: &[F_ATTACK, F_DECAY, F_SUSTAIN, F_RELEASE] },
    Section { title: "LFO", params: &[LFO_WAVE, LFO_RATE, LFO_SYNC, LFO_PITCH, LFO_FILTER, WHEEL_PITCH] },
];

/// Factory presets shape what's mapped: they change these parameters and leave the zones alone.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Slow Pad", values: &[(A_ATTACK, 600.0), (A_RELEASE, 1500.0)] },
    FactoryPreset { name: "Short Pluck", values: &[(A_DECAY, 260.0), (A_SUSTAIN, 0.0), (A_RELEASE, 120.0)] },
    FactoryPreset { name: "Soft Keys", values: &[(VEL_AMP, 0.6), (A_RELEASE, 300.0), (F_TYPE, 1.0), (CUTOFF, 6000.0), (VEL_CUTOFF, 24.0)] },
    FactoryPreset { name: "Warm Ladder", values: &[(F_TYPE, 4.0), (CUTOFF, 900.0), (RESONANCE, 0.25), (F_ENV, 36.0), (F_DECAY, 500.0), (F_SUSTAIN, 0.2), (A_RELEASE, 250.0)] },
    FactoryPreset { name: "Tape Vibrato", values: &[(LFO_RATE, 4.5), (LFO_PITCH, 18.0), (F_TYPE, 1.0), (CUTOFF, 9000.0)] },
    FactoryPreset { name: "Filter Wobble", values: &[(F_TYPE, 4.0), (CUTOFF, 700.0), (RESONANCE, 0.45), (LFO_SYNC, 4.0), (LFO_FILTER, 30.0)] },
    FactoryPreset { name: "Mono Glide", values: &[(MODE, 2.0), (GLIDE, 180.0), (A_RELEASE, 120.0)] },
];
