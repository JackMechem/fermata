//! The parameters of Fermata Sedici. The ids are stable forever: projects and presets store them.
//! Six operators, each with the same fields: `O1 + LEVEL` and so on. The values run in the
//! instrument's own 0..99 steps, and the voice rounds them.

use crate::fx::{ParamSpec, Unit};
use crate::instrument::common::{choice, cont, toggle};
use crate::instrument::Section;

pub mod param {
    pub const ALGORITHM: u32 = 0;
    pub const FEEDBACK: u32 = 1;
    pub const KEY_SYNC: u32 = 2;
    pub const PR1: u32 = 3;
    pub const PR2: u32 = 4;
    pub const PR3: u32 = 5;
    pub const PR4: u32 = 6;
    pub const PL1: u32 = 7;
    pub const PL2: u32 = 8;
    pub const PL3: u32 = 9;
    pub const PL4: u32 = 10;
    pub const LFO_SPEED: u32 = 11;
    pub const LFO_DELAY: u32 = 12;
    pub const LFO_PMD: u32 = 13;
    pub const LFO_AMD: u32 = 14;
    pub const LFO_SYNC: u32 = 15;
    pub const LFO_WAVE: u32 = 16;
    pub const LFO_PMS: u32 = 17;
    pub const TRANSPOSE: u32 = 18;
    pub const POLY: u32 = 19;
    pub const PORTA: u32 = 20;
    pub const BEND_RANGE: u32 = 21;
    pub const WHEEL_PITCH: u32 = 22;
    pub const WHEEL_AMP: u32 = 23;
    pub const TOUCH_PITCH: u32 = 24;
    pub const TOUCH_AMP: u32 = 25;
    pub const VOLUME: u32 = 26;
    pub const GRIT: u32 = 27;

    /// Each operator's first id, and how far apart they are.
    pub const OP_BASE: u32 = 100;
    pub const OP_STRIDE: u32 = 30;
    pub const O1: u32 = OP_BASE;
    pub const O2: u32 = OP_BASE + OP_STRIDE;
    pub const O3: u32 = OP_BASE + 2 * OP_STRIDE;
    pub const O4: u32 = OP_BASE + 3 * OP_STRIDE;
    pub const O5: u32 = OP_BASE + 4 * OP_STRIDE;
    pub const O6: u32 = OP_BASE + 5 * OP_STRIDE;

    // An operator's fields, as offsets.
    pub const R1: u32 = 0;
    pub const R2: u32 = 1;
    pub const R3: u32 = 2;
    pub const R4: u32 = 3;
    pub const L1: u32 = 4;
    pub const L2: u32 = 5;
    pub const L3: u32 = 6;
    pub const L4: u32 = 7;
    pub const LEVEL: u32 = 8;
    pub const COARSE: u32 = 9;
    pub const FINE: u32 = 10;
    pub const DETUNE: u32 = 11;
    pub const VEL: u32 = 12;
    pub const AMS_FIELD: u32 = 13;
    pub const RATE_SCALE: u32 = 14;
    pub const BREAK: u32 = 15;
    pub const L_DEPTH: u32 = 16;
    pub const R_DEPTH: u32 = 17;
    pub const L_CURVE: u32 = 18;
    pub const R_CURVE: u32 = 19;
    pub const MODE: u32 = 20;
    pub const ON: u32 = 21;

    /// One past the highest id.
    pub const COUNT: usize = (OP_BASE + 6 * OP_STRIDE) as usize;
}

use param::*;

pub const LFO_WAVES: &[&str] = &["Triangle", "Saw Down", "Saw Up", "Square", "Sine", "Sample & Hold"];
pub const CURVES: &[&str] = &["-Lin", "-Exp", "+Exp", "+Lin"];

const fn steps(id: u32, name: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    cont(id, name, min, max, default, Unit::None, false)
}

pub const PARAMS: &[ParamSpec] = &[
    steps(ALGORITHM, "Algorithm", 1.0, 32.0, 1.0),
    steps(FEEDBACK, "Feedback", 0.0, 7.0, 0.0),
    toggle(KEY_SYNC, "Osc Key Sync", 1.0),
    steps(PR1, "Rate 1", 0.0, 99.0, 99.0),
    steps(PR2, "Rate 2", 0.0, 99.0, 99.0),
    steps(PR3, "Rate 3", 0.0, 99.0, 99.0),
    steps(PR4, "Rate 4", 0.0, 99.0, 99.0),
    steps(PL1, "Level 1", 0.0, 99.0, 50.0),
    steps(PL2, "Level 2", 0.0, 99.0, 50.0),
    steps(PL3, "Level 3", 0.0, 99.0, 50.0),
    steps(PL4, "Level 4", 0.0, 99.0, 50.0),
    steps(LFO_SPEED, "Speed", 0.0, 99.0, 35.0),
    steps(LFO_DELAY, "Delay", 0.0, 99.0, 0.0),
    steps(LFO_PMD, "Pitch Depth", 0.0, 99.0, 0.0),
    steps(LFO_AMD, "Amp Depth", 0.0, 99.0, 0.0),
    toggle(LFO_SYNC, "Key Sync", 1.0),
    choice(LFO_WAVE, "Wave", LFO_WAVES, 0.0),
    steps(LFO_PMS, "Pitch Sens", 0.0, 7.0, 3.0),
    cont(TRANSPOSE, "Transpose", -24.0, 24.0, 0.0, Unit::Semitones, false),
    choice(POLY, "Mode", &["Poly", "Mono"], 0.0),
    steps(PORTA, "Portamento", 0.0, 99.0, 0.0),
    cont(BEND_RANGE, "Bend Range", 0.0, 12.0, 2.0, Unit::Semitones, false),
    steps(WHEEL_PITCH, "Wheel to Pitch", 0.0, 99.0, 50.0),
    steps(WHEEL_AMP, "Wheel to Amp", 0.0, 99.0, 0.0),
    steps(TOUCH_PITCH, "Touch to Pitch", 0.0, 99.0, 0.0),
    steps(TOUCH_AMP, "Touch to Amp", 0.0, 99.0, 0.0),
    cont(VOLUME, "Volume", 0.0, 10.0, 7.0, Unit::None, false),
    toggle(GRIT, "Converter Grit", 1.0),
    steps(O1 + R1, "Op 1 R1", 0.0, 99.0, 99.0),
    steps(O1 + R2, "Op 1 R2", 0.0, 99.0, 99.0),
    steps(O1 + R3, "Op 1 R3", 0.0, 99.0, 99.0),
    steps(O1 + R4, "Op 1 R4", 0.0, 99.0, 99.0),
    steps(O1 + L1, "Op 1 L1", 0.0, 99.0, 99.0),
    steps(O1 + L2, "Op 1 L2", 0.0, 99.0, 99.0),
    steps(O1 + L3, "Op 1 L3", 0.0, 99.0, 99.0),
    steps(O1 + L4, "Op 1 L4", 0.0, 99.0, 0.0),
    steps(O1 + LEVEL, "Op 1 Level", 0.0, 99.0, 99.0),
    steps(O1 + COARSE, "Op 1 Coarse", 0.0, 31.0, 1.0),
    steps(O1 + FINE, "Op 1 Fine", 0.0, 99.0, 0.0),
    steps(O1 + DETUNE, "Op 1 Detune", -7.0, 7.0, 0.0),
    steps(O1 + VEL, "Op 1 Velocity", 0.0, 7.0, 0.0),
    steps(O1 + AMS_FIELD, "Op 1 Amp Mod", 0.0, 3.0, 0.0),
    steps(O1 + RATE_SCALE, "Op 1 Rate Scale", 0.0, 7.0, 0.0),
    steps(O1 + BREAK, "Op 1 Break Point", 0.0, 99.0, 39.0),
    steps(O1 + L_DEPTH, "Op 1 Left Depth", 0.0, 99.0, 0.0),
    steps(O1 + R_DEPTH, "Op 1 Right Depth", 0.0, 99.0, 0.0),
    choice(O1 + L_CURVE, "Op 1 Left Curve", CURVES, 0.0),
    choice(O1 + R_CURVE, "Op 1 Right Curve", CURVES, 0.0),
    choice(O1 + MODE, "Op 1 Mode", &["Ratio", "Fixed"], 0.0),
    toggle(O1 + ON, "Op 1 On", 1.0),
    steps(O2 + R1, "Op 2 R1", 0.0, 99.0, 99.0),
    steps(O2 + R2, "Op 2 R2", 0.0, 99.0, 99.0),
    steps(O2 + R3, "Op 2 R3", 0.0, 99.0, 99.0),
    steps(O2 + R4, "Op 2 R4", 0.0, 99.0, 99.0),
    steps(O2 + L1, "Op 2 L1", 0.0, 99.0, 99.0),
    steps(O2 + L2, "Op 2 L2", 0.0, 99.0, 99.0),
    steps(O2 + L3, "Op 2 L3", 0.0, 99.0, 99.0),
    steps(O2 + L4, "Op 2 L4", 0.0, 99.0, 0.0),
    steps(O2 + LEVEL, "Op 2 Level", 0.0, 99.0, 0.0),
    steps(O2 + COARSE, "Op 2 Coarse", 0.0, 31.0, 1.0),
    steps(O2 + FINE, "Op 2 Fine", 0.0, 99.0, 0.0),
    steps(O2 + DETUNE, "Op 2 Detune", -7.0, 7.0, 0.0),
    steps(O2 + VEL, "Op 2 Velocity", 0.0, 7.0, 0.0),
    steps(O2 + AMS_FIELD, "Op 2 Amp Mod", 0.0, 3.0, 0.0),
    steps(O2 + RATE_SCALE, "Op 2 Rate Scale", 0.0, 7.0, 0.0),
    steps(O2 + BREAK, "Op 2 Break Point", 0.0, 99.0, 39.0),
    steps(O2 + L_DEPTH, "Op 2 Left Depth", 0.0, 99.0, 0.0),
    steps(O2 + R_DEPTH, "Op 2 Right Depth", 0.0, 99.0, 0.0),
    choice(O2 + L_CURVE, "Op 2 Left Curve", CURVES, 0.0),
    choice(O2 + R_CURVE, "Op 2 Right Curve", CURVES, 0.0),
    choice(O2 + MODE, "Op 2 Mode", &["Ratio", "Fixed"], 0.0),
    toggle(O2 + ON, "Op 2 On", 1.0),
    steps(O3 + R1, "Op 3 R1", 0.0, 99.0, 99.0),
    steps(O3 + R2, "Op 3 R2", 0.0, 99.0, 99.0),
    steps(O3 + R3, "Op 3 R3", 0.0, 99.0, 99.0),
    steps(O3 + R4, "Op 3 R4", 0.0, 99.0, 99.0),
    steps(O3 + L1, "Op 3 L1", 0.0, 99.0, 99.0),
    steps(O3 + L2, "Op 3 L2", 0.0, 99.0, 99.0),
    steps(O3 + L3, "Op 3 L3", 0.0, 99.0, 99.0),
    steps(O3 + L4, "Op 3 L4", 0.0, 99.0, 0.0),
    steps(O3 + LEVEL, "Op 3 Level", 0.0, 99.0, 0.0),
    steps(O3 + COARSE, "Op 3 Coarse", 0.0, 31.0, 1.0),
    steps(O3 + FINE, "Op 3 Fine", 0.0, 99.0, 0.0),
    steps(O3 + DETUNE, "Op 3 Detune", -7.0, 7.0, 0.0),
    steps(O3 + VEL, "Op 3 Velocity", 0.0, 7.0, 0.0),
    steps(O3 + AMS_FIELD, "Op 3 Amp Mod", 0.0, 3.0, 0.0),
    steps(O3 + RATE_SCALE, "Op 3 Rate Scale", 0.0, 7.0, 0.0),
    steps(O3 + BREAK, "Op 3 Break Point", 0.0, 99.0, 39.0),
    steps(O3 + L_DEPTH, "Op 3 Left Depth", 0.0, 99.0, 0.0),
    steps(O3 + R_DEPTH, "Op 3 Right Depth", 0.0, 99.0, 0.0),
    choice(O3 + L_CURVE, "Op 3 Left Curve", CURVES, 0.0),
    choice(O3 + R_CURVE, "Op 3 Right Curve", CURVES, 0.0),
    choice(O3 + MODE, "Op 3 Mode", &["Ratio", "Fixed"], 0.0),
    toggle(O3 + ON, "Op 3 On", 1.0),
    steps(O4 + R1, "Op 4 R1", 0.0, 99.0, 99.0),
    steps(O4 + R2, "Op 4 R2", 0.0, 99.0, 99.0),
    steps(O4 + R3, "Op 4 R3", 0.0, 99.0, 99.0),
    steps(O4 + R4, "Op 4 R4", 0.0, 99.0, 99.0),
    steps(O4 + L1, "Op 4 L1", 0.0, 99.0, 99.0),
    steps(O4 + L2, "Op 4 L2", 0.0, 99.0, 99.0),
    steps(O4 + L3, "Op 4 L3", 0.0, 99.0, 99.0),
    steps(O4 + L4, "Op 4 L4", 0.0, 99.0, 0.0),
    steps(O4 + LEVEL, "Op 4 Level", 0.0, 99.0, 0.0),
    steps(O4 + COARSE, "Op 4 Coarse", 0.0, 31.0, 1.0),
    steps(O4 + FINE, "Op 4 Fine", 0.0, 99.0, 0.0),
    steps(O4 + DETUNE, "Op 4 Detune", -7.0, 7.0, 0.0),
    steps(O4 + VEL, "Op 4 Velocity", 0.0, 7.0, 0.0),
    steps(O4 + AMS_FIELD, "Op 4 Amp Mod", 0.0, 3.0, 0.0),
    steps(O4 + RATE_SCALE, "Op 4 Rate Scale", 0.0, 7.0, 0.0),
    steps(O4 + BREAK, "Op 4 Break Point", 0.0, 99.0, 39.0),
    steps(O4 + L_DEPTH, "Op 4 Left Depth", 0.0, 99.0, 0.0),
    steps(O4 + R_DEPTH, "Op 4 Right Depth", 0.0, 99.0, 0.0),
    choice(O4 + L_CURVE, "Op 4 Left Curve", CURVES, 0.0),
    choice(O4 + R_CURVE, "Op 4 Right Curve", CURVES, 0.0),
    choice(O4 + MODE, "Op 4 Mode", &["Ratio", "Fixed"], 0.0),
    toggle(O4 + ON, "Op 4 On", 1.0),
    steps(O5 + R1, "Op 5 R1", 0.0, 99.0, 99.0),
    steps(O5 + R2, "Op 5 R2", 0.0, 99.0, 99.0),
    steps(O5 + R3, "Op 5 R3", 0.0, 99.0, 99.0),
    steps(O5 + R4, "Op 5 R4", 0.0, 99.0, 99.0),
    steps(O5 + L1, "Op 5 L1", 0.0, 99.0, 99.0),
    steps(O5 + L2, "Op 5 L2", 0.0, 99.0, 99.0),
    steps(O5 + L3, "Op 5 L3", 0.0, 99.0, 99.0),
    steps(O5 + L4, "Op 5 L4", 0.0, 99.0, 0.0),
    steps(O5 + LEVEL, "Op 5 Level", 0.0, 99.0, 0.0),
    steps(O5 + COARSE, "Op 5 Coarse", 0.0, 31.0, 1.0),
    steps(O5 + FINE, "Op 5 Fine", 0.0, 99.0, 0.0),
    steps(O5 + DETUNE, "Op 5 Detune", -7.0, 7.0, 0.0),
    steps(O5 + VEL, "Op 5 Velocity", 0.0, 7.0, 0.0),
    steps(O5 + AMS_FIELD, "Op 5 Amp Mod", 0.0, 3.0, 0.0),
    steps(O5 + RATE_SCALE, "Op 5 Rate Scale", 0.0, 7.0, 0.0),
    steps(O5 + BREAK, "Op 5 Break Point", 0.0, 99.0, 39.0),
    steps(O5 + L_DEPTH, "Op 5 Left Depth", 0.0, 99.0, 0.0),
    steps(O5 + R_DEPTH, "Op 5 Right Depth", 0.0, 99.0, 0.0),
    choice(O5 + L_CURVE, "Op 5 Left Curve", CURVES, 0.0),
    choice(O5 + R_CURVE, "Op 5 Right Curve", CURVES, 0.0),
    choice(O5 + MODE, "Op 5 Mode", &["Ratio", "Fixed"], 0.0),
    toggle(O5 + ON, "Op 5 On", 1.0),
    steps(O6 + R1, "Op 6 R1", 0.0, 99.0, 99.0),
    steps(O6 + R2, "Op 6 R2", 0.0, 99.0, 99.0),
    steps(O6 + R3, "Op 6 R3", 0.0, 99.0, 99.0),
    steps(O6 + R4, "Op 6 R4", 0.0, 99.0, 99.0),
    steps(O6 + L1, "Op 6 L1", 0.0, 99.0, 99.0),
    steps(O6 + L2, "Op 6 L2", 0.0, 99.0, 99.0),
    steps(O6 + L3, "Op 6 L3", 0.0, 99.0, 99.0),
    steps(O6 + L4, "Op 6 L4", 0.0, 99.0, 0.0),
    steps(O6 + LEVEL, "Op 6 Level", 0.0, 99.0, 0.0),
    steps(O6 + COARSE, "Op 6 Coarse", 0.0, 31.0, 1.0),
    steps(O6 + FINE, "Op 6 Fine", 0.0, 99.0, 0.0),
    steps(O6 + DETUNE, "Op 6 Detune", -7.0, 7.0, 0.0),
    steps(O6 + VEL, "Op 6 Velocity", 0.0, 7.0, 0.0),
    steps(O6 + AMS_FIELD, "Op 6 Amp Mod", 0.0, 3.0, 0.0),
    steps(O6 + RATE_SCALE, "Op 6 Rate Scale", 0.0, 7.0, 0.0),
    steps(O6 + BREAK, "Op 6 Break Point", 0.0, 99.0, 39.0),
    steps(O6 + L_DEPTH, "Op 6 Left Depth", 0.0, 99.0, 0.0),
    steps(O6 + R_DEPTH, "Op 6 Right Depth", 0.0, 99.0, 0.0),
    choice(O6 + L_CURVE, "Op 6 Left Curve", CURVES, 0.0),
    choice(O6 + R_CURVE, "Op 6 Right Curve", CURVES, 0.0),
    choice(O6 + MODE, "Op 6 Mode", &["Ratio", "Fixed"], 0.0),
    toggle(O6 + ON, "Op 6 On", 1.0),
];

pub const SECTIONS: &[Section] = &[
    Section { title: "Voice", params: &[ALGORITHM, FEEDBACK, KEY_SYNC, POLY, PORTA, TRANSPOSE, VOLUME, GRIT] },
    Section { title: "Pitch Envelope", params: &[PR1, PL1, PR2, PL2, PR3, PL3, PR4, PL4] },
    Section { title: "LFO", params: &[LFO_WAVE, LFO_SPEED, LFO_DELAY, LFO_SYNC, LFO_PMD, LFO_AMD, LFO_PMS] },
    Section { title: "Controllers", params: &[BEND_RANGE, WHEEL_PITCH, WHEEL_AMP, TOUCH_PITCH, TOUCH_AMP] },
    Section { title: "Operator 1", params: &[O1 + ON, O1 + LEVEL, O1 + MODE, O1 + COARSE, O1 + FINE, O1 + DETUNE, O1 + VEL, O1 + AMS_FIELD] },
    Section { title: "Operator 1 Envelope", params: &[O1 + R1, O1 + L1, O1 + R2, O1 + L2, O1 + R3, O1 + L3, O1 + R4, O1 + L4] },
    Section { title: "Operator 1 Scaling", params: &[O1 + RATE_SCALE, O1 + BREAK, O1 + L_DEPTH, O1 + R_DEPTH, O1 + L_CURVE, O1 + R_CURVE] },
    Section { title: "Operator 2", params: &[O2 + ON, O2 + LEVEL, O2 + MODE, O2 + COARSE, O2 + FINE, O2 + DETUNE, O2 + VEL, O2 + AMS_FIELD] },
    Section { title: "Operator 2 Envelope", params: &[O2 + R1, O2 + L1, O2 + R2, O2 + L2, O2 + R3, O2 + L3, O2 + R4, O2 + L4] },
    Section { title: "Operator 2 Scaling", params: &[O2 + RATE_SCALE, O2 + BREAK, O2 + L_DEPTH, O2 + R_DEPTH, O2 + L_CURVE, O2 + R_CURVE] },
    Section { title: "Operator 3", params: &[O3 + ON, O3 + LEVEL, O3 + MODE, O3 + COARSE, O3 + FINE, O3 + DETUNE, O3 + VEL, O3 + AMS_FIELD] },
    Section { title: "Operator 3 Envelope", params: &[O3 + R1, O3 + L1, O3 + R2, O3 + L2, O3 + R3, O3 + L3, O3 + R4, O3 + L4] },
    Section { title: "Operator 3 Scaling", params: &[O3 + RATE_SCALE, O3 + BREAK, O3 + L_DEPTH, O3 + R_DEPTH, O3 + L_CURVE, O3 + R_CURVE] },
    Section { title: "Operator 4", params: &[O4 + ON, O4 + LEVEL, O4 + MODE, O4 + COARSE, O4 + FINE, O4 + DETUNE, O4 + VEL, O4 + AMS_FIELD] },
    Section { title: "Operator 4 Envelope", params: &[O4 + R1, O4 + L1, O4 + R2, O4 + L2, O4 + R3, O4 + L3, O4 + R4, O4 + L4] },
    Section { title: "Operator 4 Scaling", params: &[O4 + RATE_SCALE, O4 + BREAK, O4 + L_DEPTH, O4 + R_DEPTH, O4 + L_CURVE, O4 + R_CURVE] },
    Section { title: "Operator 5", params: &[O5 + ON, O5 + LEVEL, O5 + MODE, O5 + COARSE, O5 + FINE, O5 + DETUNE, O5 + VEL, O5 + AMS_FIELD] },
    Section { title: "Operator 5 Envelope", params: &[O5 + R1, O5 + L1, O5 + R2, O5 + L2, O5 + R3, O5 + L3, O5 + R4, O5 + L4] },
    Section { title: "Operator 5 Scaling", params: &[O5 + RATE_SCALE, O5 + BREAK, O5 + L_DEPTH, O5 + R_DEPTH, O5 + L_CURVE, O5 + R_CURVE] },
    Section { title: "Operator 6", params: &[O6 + ON, O6 + LEVEL, O6 + MODE, O6 + COARSE, O6 + FINE, O6 + DETUNE, O6 + VEL, O6 + AMS_FIELD] },
    Section { title: "Operator 6 Envelope", params: &[O6 + R1, O6 + L1, O6 + R2, O6 + L2, O6 + R3, O6 + L3, O6 + R4, O6 + L4] },
    Section { title: "Operator 6 Scaling", params: &[O6 + RATE_SCALE, O6 + BREAK, O6 + L_DEPTH, O6 + R_DEPTH, O6 + L_CURVE, O6 + R_CURVE] },
];

/// Every parameter's default, by id.
pub fn defaults() -> [f32; COUNT] {
    let mut out = [0.0; COUNT];
    for p in PARAMS {
        out[p.id as usize] = p.default;
    }
    out
}
