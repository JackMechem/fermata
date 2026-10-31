//! Factory presets of Fermata Tre. Parameters a preset doesn't list go to their defaults, so
//! these read as what each sound changes from the panel's starting point.

use super::params::param::*;
use crate::fx::FactoryPreset;

// Range and wave choices.
const LO: f32 = 0.0;
const R32: f32 = 1.0;
const R16: f32 = 2.0;
const R8: f32 = 3.0;
const R4: f32 = 4.0;
const R2: f32 = 5.0;
const TRI: f32 = 0.0;
const SHARK: f32 = 1.0;
const SAW: f32 = 2.0;
const SQUARE: f32 = 3.0;
const WIDE: f32 = 4.0;
const NARROW: f32 = 5.0;
// Osc 3's list has Rev Saw where the others have Saw.
const W3_TRI: f32 = 0.0;
const W3_REV: f32 = 2.0;
const W3_SAW: f32 = 3.0;
const W3_SQUARE: f32 = 4.0;

pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Round Foundation",
        values: &[(RANGE1, R16), (WAVE1, SAW), (RANGE2, R16), (WAVE2, SQUARE), (TUNE2, 0.05), (LEVEL1, 8.0), (LEVEL2, 6.0), (CUTOFF, 320.0), (EMPHASIS, 2.0), (CONTOUR, 4.0), (F_DECAY, 350.0), (F_SUSTAIN, 2.0), (A_DECAY, 400.0), (A_SUSTAIN, 9.0), (DECAY_ON, 0.0)],
    },
    FactoryPreset {
        name: "Three-Saw Growl",
        values: &[(RANGE1, R32), (WAVE1, SAW), (RANGE2, R16), (WAVE2, SAW), (TUNE2, 0.09), (RANGE3, R16), (WAVE3, W3_SAW), (TUNE3, -0.08), (ON3, 1.0), (LEVEL1, 7.0), (LEVEL2, 6.0), (LEVEL3, 6.0), (CUTOFF, 240.0), (EMPHASIS, 3.5), (CONTOUR, 6.0), (F_DECAY, 500.0), (F_SUSTAIN, 2.5), (DECAY_ON, 0.0), (VOLUME, 8.0)],
    },
    FactoryPreset {
        name: "Funk Pluck",
        values: &[(RANGE1, R16), (WAVE1, SAW), (RANGE2, R8), (WAVE2, SQUARE), (TUNE2, -0.04), (LEVEL1, 8.0), (LEVEL2, 3.0), (CUTOFF, 180.0), (EMPHASIS, 5.0), (CONTOUR, 7.0), (F_ATTACK, 1.0), (F_DECAY, 180.0), (F_SUSTAIN, 0.0), (A_ATTACK, 1.0), (A_DECAY, 350.0), (A_SUSTAIN, 0.0), (DECAY_ON, 0.0), (KBD1, 1.0), (VOLUME, 9.5)],
    },
    FactoryPreset {
        name: "Sub Weight",
        values: &[(RANGE1, R32), (WAVE1, TRI), (RANGE2, R16), (WAVE2, TRI), (TUNE2, 0.04), (LEVEL1, 10.0), (LEVEL2, 4.0), (CUTOFF, 300.0), (EMPHASIS, 0.0), (CONTOUR, 1.0), (F_DECAY, 200.0), (F_SUSTAIN, 0.0), (A_ATTACK, 3.0), (A_DECAY, 600.0), (A_SUSTAIN, 7.0), (DECAY_ON, 0.0), (VOLUME, 8.0)],
    },
    FactoryPreset {
        name: "Squelch Machine",
        values: &[(RANGE1, R16), (WAVE1, SAW), (ON2, 0.0), (LEVEL1, 8.0), (CUTOFF, 200.0), (EMPHASIS, 7.5), (CONTOUR, 8.0), (F_ATTACK, 1.0), (F_DECAY, 240.0), (F_SUSTAIN, 0.0), (A_DECAY, 500.0), (A_SUSTAIN, 6.0), (KBD1, 1.0), (KBD2, 1.0), (DECAY_ON, 0.0), (VOLUME, 9.5)],
    },
    FactoryPreset {
        name: "Overdriven Mixer Bass",
        values: &[(RANGE1, R16), (WAVE1, SAW), (RANGE2, R16), (WAVE2, SAW), (TUNE2, 0.1), (RANGE3, R32), (WAVE3, W3_SQUARE), (ON3, 1.0), (LEVEL1, 10.0), (LEVEL2, 10.0), (LEVEL3, 9.0), (CUTOFF, 420.0), (EMPHASIS, 2.5), (CONTOUR, 5.0), (F_DECAY, 300.0), (F_SUSTAIN, 2.0), (DECAY_ON, 0.0), (VOLUME, 7.5)],
    },
    FactoryPreset {
        name: "Singing Lead",
        values: &[(RANGE1, R8), (WAVE1, SAW), (RANGE2, R8), (WAVE2, SAW), (TUNE2, 0.12), (RANGE3, LO), (WAVE3, W3_TRI), (TUNE3, 2.0), (OSC3_KBD, 0.0), (LEVEL1, 8.0), (LEVEL2, 7.0), (CUTOFF, 900.0), (EMPHASIS, 4.0), (CONTOUR, 4.5), (F_ATTACK, 30.0), (F_DECAY, 700.0), (F_SUSTAIN, 5.0), (A_ATTACK, 8.0), (A_SUSTAIN, 10.0), (A_DECAY, 300.0), (OSC_MOD, 1.0), (GLIDE_ON, 1.0), (GLIDE, 70.0), (KBD1, 1.0), (KBD2, 1.0), (PRIORITY, 1.0), (TOUCH_MOD, 0.5)],
    },
    FactoryPreset {
        name: "Fifth Above",
        values: &[(RANGE1, R8), (WAVE1, SAW), (RANGE2, R8), (WAVE2, SAW), (TUNE2, 7.0), (RANGE3, R4), (WAVE3, W3_SAW), (TUNE3, 0.06), (ON3, 1.0), (LEVEL1, 8.0), (LEVEL2, 6.0), (LEVEL3, 4.0), (CUTOFF, 1400.0), (EMPHASIS, 2.5), (CONTOUR, 3.0), (F_DECAY, 450.0), (F_SUSTAIN, 5.0), (A_SUSTAIN, 10.0), (KBD1, 1.0), (PRIORITY, 2.0)],
    },
    FactoryPreset {
        name: "Hollow Square",
        values: &[(RANGE1, R8), (WAVE1, SQUARE), (RANGE2, R16), (WAVE2, SQUARE), (TUNE2, -0.07), (LEVEL1, 8.0), (LEVEL2, 5.0), (CUTOFF, 1800.0), (EMPHASIS, 1.0), (CONTOUR, 2.0), (F_DECAY, 250.0), (F_SUSTAIN, 6.0), (A_SUSTAIN, 10.0), (KBD1, 1.0), (GLIDE_ON, 1.0), (GLIDE, 40.0), (PRIORITY, 1.0)],
    },
    FactoryPreset {
        name: "Filter Whistle",
        values: &[(ON1, 0.0), (ON2, 0.0), (CUTOFF, 261.6), (EMPHASIS, 10.0), (CONTOUR, 0.0), (KBD1, 1.0), (KBD2, 1.0), (A_ATTACK, 25.0), (A_SUSTAIN, 10.0), (A_DECAY, 500.0), (GLIDE_ON, 1.0), (GLIDE, 60.0), (VOLUME, 10.0), (RANGE3, LO), (WAVE3, W3_TRI), (TUNE3, 2.5), (OSC3_KBD, 0.0), (FILTER_MOD, 1.0), (PRIORITY, 1.0), (DRIFT, 0.1)],
    },
    FactoryPreset {
        name: "Metal Clang",
        values: &[(RANGE1, R8), (WAVE1, TRI), (ON2, 0.0), (RANGE3, R8), (WAVE3, W3_TRI), (TUNE3, 5.3), (OSC_MOD, 1.0), (FILTER_MOD, 1.0), (MOD_AMOUNT, 6.0), (LEVEL1, 8.0), (CUTOFF, 2600.0), (EMPHASIS, 4.0), (CONTOUR, 3.0), (F_DECAY, 900.0), (F_SUSTAIN, 0.0), (A_ATTACK, 1.0), (A_DECAY, 1400.0), (A_SUSTAIN, 0.0), (KBD1, 1.0), (KBD2, 1.0), (VOLUME, 9.5)],
    },
    FactoryPreset {
        name: "Night Wind",
        values: &[(ON1, 0.0), (ON2, 0.0), (NOISE_ON, 1.0), (NOISE, 8.0), (NOISE_COLOR, 1.0), (CUTOFF, 700.0), (EMPHASIS, 7.0), (CONTOUR, 0.0), (FILTER_MOD, 1.0), (RANGE3, LO), (WAVE3, W3_TRI), (TUNE3, -6.0), (OSC3_KBD, 0.0), (MOD_AMOUNT, 7.0), (A_ATTACK, 900.0), (A_DECAY, 2500.0), (A_SUSTAIN, 10.0), (KBD1, 1.0), (VOLUME, 10.0)],
    },
    FactoryPreset {
        name: "Slow Brass Swell",
        values: &[(RANGE1, R8), (WAVE1, SAW), (RANGE2, R8), (WAVE2, SAW), (TUNE2, 0.1), (RANGE3, R16), (WAVE3, W3_SAW), (ON3, 1.0), (TUNE3, -0.1), (LEVEL1, 7.0), (LEVEL2, 7.0), (LEVEL3, 5.0), (CUTOFF, 400.0), (EMPHASIS, 1.5), (CONTOUR, 5.5), (F_ATTACK, 280.0), (F_DECAY, 900.0), (F_SUSTAIN, 5.0), (A_ATTACK, 80.0), (A_DECAY, 500.0), (A_SUSTAIN, 9.0), (KBD1, 1.0), (DRIFT, 0.4), (VEL_FILTER, 0.2)],
    },
    FactoryPreset {
        name: "Laser Drop",
        values: &[(RANGE1, R8), (WAVE1, SAW), (RANGE2, R4), (WAVE2, SAW), (ON3, 0.0), (RANGE3, LO), (WAVE3, W3_REV), (TUNE3, 7.0), (OSC3_KBD, 0.0), (OSC_MOD, 1.0), (MOD_AMOUNT, 8.0), (LEVEL1, 8.0), (LEVEL2, 4.0), (CUTOFF, 1200.0), (EMPHASIS, 6.5), (CONTOUR, 4.0), (F_DECAY, 500.0), (F_SUSTAIN, 0.0), (A_ATTACK, 1.0), (A_DECAY, 700.0), (A_SUSTAIN, 0.0), (VOLUME, 9.0)],
    },
    FactoryPreset {
        name: "Feedback Roar",
        values: &[(RANGE1, R16), (WAVE1, SAW), (RANGE2, R16), (WAVE2, WIDE), (TUNE2, 0.07), (LEVEL1, 9.0), (LEVEL2, 7.0), (FEEDBACK_ON, 1.0), (FEEDBACK, 6.0), (CUTOFF, 600.0), (EMPHASIS, 3.0), (CONTOUR, 5.0), (F_DECAY, 400.0), (F_SUSTAIN, 3.0), (DECAY_ON, 0.0), (VOLUME, 8.0)],
    },
    FactoryPreset {
        name: "Thin Reed",
        values: &[(RANGE1, R8), (WAVE1, NARROW), (RANGE2, R2), (WAVE2, SHARK), (LEVEL1, 8.0), (LEVEL2, 2.0), (CUTOFF, 2200.0), (EMPHASIS, 1.5), (CONTOUR, 1.0), (A_ATTACK, 12.0), (A_SUSTAIN, 10.0), (KBD1, 1.0), (KBD2, 1.0), (GLIDE_ON, 1.0), (GLIDE, 30.0), (PRIORITY, 1.0), (VEL_LOUDNESS, 0.4)],
    },
    FactoryPreset {
        name: "Drifting Keys (poly)",
        values: &[(VOICE_MODE, 2.0), (VOICES, 3.0), (RANGE1, R8), (WAVE1, SAW), (RANGE2, R8), (WAVE2, WIDE), (TUNE2, 0.08), (LEVEL1, 6.0), (LEVEL2, 5.0), (CUTOFF, 1000.0), (EMPHASIS, 1.5), (CONTOUR, 3.0), (F_ATTACK, 15.0), (F_DECAY, 800.0), (F_SUSTAIN, 3.0), (A_ATTACK, 25.0), (A_DECAY, 700.0), (A_SUSTAIN, 7.0), (KBD1, 1.0), (DRIFT, 0.6), (VEL_LOUDNESS, 0.5), (VEL_FILTER, 0.4), (SPREAD, 0.7)],
    },
    FactoryPreset {
        name: "Wall of Saws (unison)",
        values: &[(VOICE_MODE, 1.0), (VOICES, 2.0), (RANGE1, R8), (WAVE1, SAW), (RANGE2, R16), (WAVE2, SAW), (UNISON_DETUNE, 0.35), (SPREAD, 0.8), (LEVEL1, 7.0), (LEVEL2, 6.0), (CUTOFF, 1600.0), (EMPHASIS, 2.0), (CONTOUR, 3.0), (F_DECAY, 500.0), (F_SUSTAIN, 5.0), (A_SUSTAIN, 10.0), (KBD1, 1.0), (PRIORITY, 1.0), (VOLUME, 6.0)],
    },
];
