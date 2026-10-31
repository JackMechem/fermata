//! The parameters of Fermata Onda. The ids are stable forever: projects and presets store them.

use super::tables::TABLE_NAMES;
use crate::fx::{ParamSpec, Unit};
use crate::instrument::common::{choice, cont, percent, ten, time_ms, toggle};
use crate::instrument::Section;

pub mod param {
    pub const A_ON: u32 = 0;
    pub const A_TABLE: u32 = 1;
    pub const A_POS: u32 = 2;
    pub const A_OCT: u32 = 3;
    pub const A_SEMI: u32 = 4;
    pub const A_FINE: u32 = 5;
    pub const A_LEVEL: u32 = 6;
    pub const A_PAN: u32 = 7;
    pub const A_VOICES: u32 = 8;
    pub const A_DETUNE: u32 = 9;
    pub const A_BLEND: u32 = 10;
    pub const A_SPREAD: u32 = 11;
    pub const B_ON: u32 = 12;
    pub const B_TABLE: u32 = 13;
    pub const B_POS: u32 = 14;
    pub const B_OCT: u32 = 15;
    pub const B_SEMI: u32 = 16;
    pub const B_FINE: u32 = 17;
    pub const B_LEVEL: u32 = 18;
    pub const B_PAN: u32 = 19;
    pub const B_VOICES: u32 = 20;
    pub const B_DETUNE: u32 = 21;
    pub const B_BLEND: u32 = 22;
    pub const B_SPREAD: u32 = 23;
    pub const SUB_LEVEL: u32 = 24;
    pub const NOISE_LEVEL: u32 = 25;
    pub const FM_AMOUNT: u32 = 26;
    pub const FILTER_ON: u32 = 27;
    pub const FILTER_TYPE: u32 = 28;
    pub const CUTOFF: u32 = 29;
    pub const RESONANCE: u32 = 30;
    pub const DRIVE: u32 = 31;
    pub const KEY_TRACK: u32 = 32;
    pub const E1_ATTACK: u32 = 33;
    pub const E1_DECAY: u32 = 34;
    pub const E1_SUSTAIN: u32 = 35;
    pub const E1_RELEASE: u32 = 36;
    pub const E2_ATTACK: u32 = 37;
    pub const E2_DECAY: u32 = 38;
    pub const E2_SUSTAIN: u32 = 39;
    pub const E2_RELEASE: u32 = 40;
    pub const E3_ATTACK: u32 = 41;
    pub const E3_DECAY: u32 = 42;
    pub const E3_SUSTAIN: u32 = 43;
    pub const E3_RELEASE: u32 = 44;
    pub const L1_RATE: u32 = 45;
    pub const L1_SYNC: u32 = 46;
    pub const L1_WAVE: u32 = 47;
    pub const L1_RETRIG: u32 = 48;
    pub const L2_RATE: u32 = 49;
    pub const L2_SYNC: u32 = 50;
    pub const L2_WAVE: u32 = 51;
    pub const L2_RETRIG: u32 = 52;
    pub const GLIDE: u32 = 53;
    pub const VOICE_MODE: u32 = 54;
    pub const POLYPHONY: u32 = 55;
    pub const BEND_RANGE: u32 = 56;
    pub const VOLUME: u32 = 57;
    /// The modulation slots: source, destination and amount, in threes.
    pub const MOD_BASE: u32 = 80;
    pub const MOD_SLOTS: u32 = 8;
    pub const COUNT: usize = (MOD_BASE + 3 * MOD_SLOTS) as usize;
}

use param::*;

pub const MOD_SOURCES: &[&str] = &["Off", "Env 2", "Env 3", "LFO 1", "LFO 2", "Velocity", "Key", "Wheel", "Aftertouch"];
pub const MOD_DESTS: &[&str] = &["Off", "A Position", "B Position", "A Pitch", "B Pitch", "A Level", "B Level", "Cutoff", "Resonance", "Unison Detune", "FM Amount", "Pan"];
pub const FILTERS: &[&str] = &["Low 12", "Low 24", "Band 12", "High 12", "Notch", "Ladder 24", "Diode"];
pub const LFO_WAVES: &[&str] = &["Sine", "Triangle", "Saw", "Square", "Random"];
pub const LFO_SYNCS: &[&str] = &["Free", "4 bars", "2 bars", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/8 T", "1/16 T"];
pub const VOICE_MODES: &[&str] = &["Poly", "Mono", "Legato"];
pub const UNISONS: &[&str] = &["1", "2", "3", "4", "5", "6", "7"];

const fn steps(id: u32, name: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    cont(id, name, min, max, default, Unit::None, false)
}

pub const PARAMS: &[ParamSpec] = &[
    toggle(A_ON, "A On", 1.0),
    choice(A_TABLE, "A Table", TABLE_NAMES, 0.0),
    percent(A_POS, "A Position", 0.67),
    steps(A_OCT, "A Octave", -3.0, 3.0, 0.0),
    cont(A_SEMI, "A Semi", -12.0, 12.0, 0.0, Unit::Semitones, false),
    cont(A_FINE, "A Fine", -1.0, 1.0, 0.0, Unit::Semitones, false),
    percent(A_LEVEL, "A Level", 0.75),
    cont(A_PAN, "A Pan", -1.0, 1.0, 0.0, Unit::None, false),
    choice(A_VOICES, "A Unison", UNISONS, 0.0),
    percent(A_DETUNE, "A Detune", 0.2),
    percent(A_BLEND, "A Blend", 0.75),
    percent(A_SPREAD, "A Width", 1.0),
    toggle(B_ON, "B On", 0.0),
    choice(B_TABLE, "B Table", TABLE_NAMES, 1.0),
    percent(B_POS, "B Position", 0.3),
    steps(B_OCT, "B Octave", -3.0, 3.0, 0.0),
    cont(B_SEMI, "B Semi", -12.0, 12.0, 0.0, Unit::Semitones, false),
    cont(B_FINE, "B Fine", -1.0, 1.0, 0.0, Unit::Semitones, false),
    percent(B_LEVEL, "B Level", 0.75),
    cont(B_PAN, "B Pan", -1.0, 1.0, 0.0, Unit::None, false),
    choice(B_VOICES, "B Unison", UNISONS, 0.0),
    percent(B_DETUNE, "B Detune", 0.2),
    percent(B_BLEND, "B Blend", 0.75),
    percent(B_SPREAD, "B Width", 1.0),
    percent(SUB_LEVEL, "Sub", 0.0),
    percent(NOISE_LEVEL, "Noise", 0.0),
    percent(FM_AMOUNT, "FM B to A", 0.0),
    toggle(FILTER_ON, "Filter On", 1.0),
    choice(FILTER_TYPE, "Type", FILTERS, 1.0),
    cont(CUTOFF, "Cutoff", 20.0, 20000.0, 8000.0, Unit::Hz, true),
    percent(RESONANCE, "Resonance", 0.1),
    percent(DRIVE, "Drive", 0.0),
    percent(KEY_TRACK, "Key Track", 0.0),
    time_ms(E1_ATTACK, "Attack", 1.0, 20000.0, 2.0),
    time_ms(E1_DECAY, "Decay", 1.0, 20000.0, 300.0),
    percent(E1_SUSTAIN, "Sustain", 0.8),
    time_ms(E1_RELEASE, "Release", 1.0, 20000.0, 250.0),
    time_ms(E2_ATTACK, "Attack", 1.0, 20000.0, 5.0),
    time_ms(E2_DECAY, "Decay", 1.0, 20000.0, 400.0),
    percent(E2_SUSTAIN, "Sustain", 0.4),
    time_ms(E2_RELEASE, "Release", 1.0, 20000.0, 300.0),
    time_ms(E3_ATTACK, "Attack", 1.0, 20000.0, 5.0),
    time_ms(E3_DECAY, "Decay", 1.0, 20000.0, 600.0),
    percent(E3_SUSTAIN, "Sustain", 0.0),
    time_ms(E3_RELEASE, "Release", 1.0, 20000.0, 300.0),
    cont(L1_RATE, "Rate", 0.01, 40.0, 2.0, Unit::Hz, true),
    choice(L1_SYNC, "Sync", LFO_SYNCS, 0.0),
    choice(L1_WAVE, "Wave", LFO_WAVES, 1.0),
    toggle(L1_RETRIG, "Key Retrigger", 0.0),
    cont(L2_RATE, "Rate", 0.01, 40.0, 0.5, Unit::Hz, true),
    choice(L2_SYNC, "Sync", LFO_SYNCS, 0.0),
    choice(L2_WAVE, "Wave", LFO_WAVES, 0.0),
    toggle(L2_RETRIG, "Key Retrigger", 0.0),
    time_ms(GLIDE, "Glide", 1.0, 5000.0, 1.0),
    choice(VOICE_MODE, "Mode", VOICE_MODES, 0.0),
    steps(POLYPHONY, "Polyphony", 1.0, 16.0, 8.0),
    cont(BEND_RANGE, "Bend Range", 0.0, 24.0, 2.0, Unit::Semitones, false),
    ten(VOLUME, "Volume", 7.0),
    choice(MOD_BASE, "Slot 1 Source", MOD_SOURCES, 0.0),
    choice(MOD_BASE + 1, "Slot 1 Dest", MOD_DESTS, 0.0),
    cont(MOD_BASE + 2, "Slot 1 Amount", -1.0, 1.0, 0.0, Unit::None, false),
    choice(MOD_BASE + 3, "Slot 2 Source", MOD_SOURCES, 0.0),
    choice(MOD_BASE + 3 + 1, "Slot 2 Dest", MOD_DESTS, 0.0),
    cont(MOD_BASE + 3 + 2, "Slot 2 Amount", -1.0, 1.0, 0.0, Unit::None, false),
    choice(MOD_BASE + 6, "Slot 3 Source", MOD_SOURCES, 0.0),
    choice(MOD_BASE + 6 + 1, "Slot 3 Dest", MOD_DESTS, 0.0),
    cont(MOD_BASE + 6 + 2, "Slot 3 Amount", -1.0, 1.0, 0.0, Unit::None, false),
    choice(MOD_BASE + 9, "Slot 4 Source", MOD_SOURCES, 0.0),
    choice(MOD_BASE + 9 + 1, "Slot 4 Dest", MOD_DESTS, 0.0),
    cont(MOD_BASE + 9 + 2, "Slot 4 Amount", -1.0, 1.0, 0.0, Unit::None, false),
    choice(MOD_BASE + 12, "Slot 5 Source", MOD_SOURCES, 0.0),
    choice(MOD_BASE + 12 + 1, "Slot 5 Dest", MOD_DESTS, 0.0),
    cont(MOD_BASE + 12 + 2, "Slot 5 Amount", -1.0, 1.0, 0.0, Unit::None, false),
    choice(MOD_BASE + 15, "Slot 6 Source", MOD_SOURCES, 0.0),
    choice(MOD_BASE + 15 + 1, "Slot 6 Dest", MOD_DESTS, 0.0),
    cont(MOD_BASE + 15 + 2, "Slot 6 Amount", -1.0, 1.0, 0.0, Unit::None, false),
    choice(MOD_BASE + 18, "Slot 7 Source", MOD_SOURCES, 0.0),
    choice(MOD_BASE + 18 + 1, "Slot 7 Dest", MOD_DESTS, 0.0),
    cont(MOD_BASE + 18 + 2, "Slot 7 Amount", -1.0, 1.0, 0.0, Unit::None, false),
    choice(MOD_BASE + 21, "Slot 8 Source", MOD_SOURCES, 0.0),
    choice(MOD_BASE + 21 + 1, "Slot 8 Dest", MOD_DESTS, 0.0),
    cont(MOD_BASE + 21 + 2, "Slot 8 Amount", -1.0, 1.0, 0.0, Unit::None, false),
];

pub const SECTIONS: &[Section] = &[
    Section { title: "Oscillator A", params: &[A_ON, A_LEVEL, A_TABLE, A_POS, A_OCT, A_SEMI, A_FINE, A_PAN] },
    Section { title: "A Unison", params: &[A_VOICES, A_DETUNE, A_BLEND, A_SPREAD] },
    Section { title: "Oscillator B", params: &[B_ON, B_LEVEL, B_TABLE, B_POS, B_OCT, B_SEMI, B_FINE, B_PAN] },
    Section { title: "B Unison", params: &[B_VOICES, B_DETUNE, B_BLEND, B_SPREAD] },
    Section { title: "Mix", params: &[SUB_LEVEL, NOISE_LEVEL, FM_AMOUNT] },
    Section { title: "Filter", params: &[FILTER_ON, FILTER_TYPE, CUTOFF, RESONANCE, DRIVE, KEY_TRACK] },
    Section { title: "Env 1 (Amp)", params: &[E1_ATTACK, E1_DECAY, E1_SUSTAIN, E1_RELEASE] },
    Section { title: "Env 2", params: &[E2_ATTACK, E2_DECAY, E2_SUSTAIN, E2_RELEASE] },
    Section { title: "Env 3", params: &[E3_ATTACK, E3_DECAY, E3_SUSTAIN, E3_RELEASE] },
    Section { title: "LFO 1", params: &[L1_RATE, L1_SYNC, L1_WAVE, L1_RETRIG] },
    Section { title: "LFO 2", params: &[L2_RATE, L2_SYNC, L2_WAVE, L2_RETRIG] },
    Section { title: "Slot 1", params: &[MOD_BASE, MOD_BASE + 1, MOD_BASE + 2] },
    Section { title: "Slot 2", params: &[MOD_BASE + 3, MOD_BASE + 4, MOD_BASE + 5] },
    Section { title: "Slot 3", params: &[MOD_BASE + 6, MOD_BASE + 7, MOD_BASE + 8] },
    Section { title: "Slot 4", params: &[MOD_BASE + 9, MOD_BASE + 10, MOD_BASE + 11] },
    Section { title: "Slot 5", params: &[MOD_BASE + 12, MOD_BASE + 13, MOD_BASE + 14] },
    Section { title: "Slot 6", params: &[MOD_BASE + 15, MOD_BASE + 16, MOD_BASE + 17] },
    Section { title: "Slot 7", params: &[MOD_BASE + 18, MOD_BASE + 19, MOD_BASE + 20] },
    Section { title: "Slot 8", params: &[MOD_BASE + 21, MOD_BASE + 22, MOD_BASE + 23] },
    Section { title: "Voices", params: &[VOICE_MODE, POLYPHONY, GLIDE, BEND_RANGE, VOLUME] },
];

/// Every parameter's default, by id.
pub fn defaults() -> [f32; COUNT] {
    let mut out = [0.0; COUNT];
    for p in PARAMS {
        out[p.id as usize] = p.default;
    }
    out
}
