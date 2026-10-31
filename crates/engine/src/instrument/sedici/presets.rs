//! Factory presets of Fermata Sedici: written here, from what the instrument is known for, with no
//! patch data from any instrument. Parameters a preset doesn't list go to their defaults.

use super::params::param::*;
use crate::fx::FactoryPreset;

pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Tine Keys",
        values: &[
            (ALGORITHM, 5.0), (FEEDBACK, 4.0), (O1 + LEVEL, 99.0), (O1 + COARSE, 1.0), (O1 + R1, 96.0), (O1 + R2, 30.0), (O1 + R3, 22.0), (O1 + R4, 62.0),
            (O1 + L2, 84.0), (O1 + L3, 0.0), (O1 + VEL, 2.0), (O1 + RATE_SCALE, 3.0), (O2 + LEVEL, 62.0), (O2 + COARSE, 1.0), (O2 + R1, 95.0), (O2 + R2, 58.0),
            (O2 + R3, 30.0), (O2 + R4, 70.0), (O2 + L2, 50.0), (O2 + L3, 0.0), (O2 + VEL, 6.0), (O2 + RATE_SCALE, 3.0), (O3 + LEVEL, 99.0), (O3 + COARSE, 1.0),
            (O3 + R1, 95.0), (O3 + R2, 26.0), (O3 + R3, 18.0), (O3 + R4, 60.0), (O3 + L2, 82.0), (O3 + L3, 0.0), (O3 + VEL, 2.0), (O3 + RATE_SCALE, 3.0),
            (O3 + DETUNE, -2.0), (O4 + LEVEL, 64.0), (O4 + COARSE, 12.0), (O4 + R1, 96.0), (O4 + R2, 72.0), (O4 + R3, 40.0), (O4 + R4, 78.0), (O4 + L2, 28.0),
            (O4 + L3, 0.0), (O4 + VEL, 7.0), (O4 + RATE_SCALE, 4.0), (O5 + LEVEL, 88.0), (O5 + COARSE, 1.0), (O5 + R1, 95.0), (O5 + R2, 24.0), (O5 + R3, 20.0),
            (O5 + R4, 58.0), (O5 + L2, 80.0), (O5 + L3, 0.0), (O5 + VEL, 1.0), (O5 + RATE_SCALE, 2.0), (O5 + DETUNE, 2.0), (O6 + LEVEL, 48.0), (O6 + COARSE, 1.0),
            (O6 + R1, 92.0), (O6 + R2, 50.0), (O6 + R3, 30.0), (O6 + R4, 72.0), (O6 + L2, 40.0), (O6 + L3, 0.0), (O6 + VEL, 5.0), (O6 + RATE_SCALE, 3.0),
        ],
    },
    FactoryPreset {
        name: "Slap Rubber Bass",
        values: &[
            (ALGORITHM, 16.0), (FEEDBACK, 5.0), (TRANSPOSE, -12.0), (O1 + LEVEL, 99.0), (O1 + COARSE, 0.0), (O1 + R2, 42.0), (O1 + R3, 28.0), (O1 + R4, 70.0),
            (O1 + L2, 60.0), (O1 + L3, 0.0), (O1 + VEL, 2.0), (O1 + RATE_SCALE, 2.0), (O2 + LEVEL, 80.0), (O2 + COARSE, 1.0), (O2 + R2, 60.0), (O2 + R3, 36.0),
            (O2 + R4, 70.0), (O2 + L2, 30.0), (O2 + L3, 0.0), (O2 + VEL, 5.0), (O3 + LEVEL, 70.0), (O3 + COARSE, 2.0), (O3 + R2, 70.0), (O3 + R3, 40.0),
            (O3 + R4, 70.0), (O3 + L2, 20.0), (O3 + L3, 0.0), (O3 + VEL, 6.0), (O4 + LEVEL, 66.0), (O4 + COARSE, 3.0), (O4 + R2, 80.0), (O4 + R3, 30.0),
            (O4 + R4, 70.0), (O4 + L2, 0.0), (O4 + L3, 0.0), (O4 + VEL, 6.0), (O5 + LEVEL, 72.0), (O5 + COARSE, 1.0), (O5 + R2, 50.0), (O5 + R3, 30.0),
            (O5 + R4, 70.0), (O5 + L2, 36.0), (O5 + L3, 0.0), (O5 + VEL, 4.0), (O6 + LEVEL, 52.0), (O6 + COARSE, 5.0), (O6 + R2, 76.0), (O6 + R3, 30.0),
            (O6 + R4, 70.0), (O6 + L2, 0.0), (O6 + L3, 0.0), (O6 + VEL, 6.0),
        ],
    },
    FactoryPreset {
        name: "Rosewood Mallet",
        values: &[
            (ALGORITHM, 7.0), (O1 + LEVEL, 99.0), (O1 + COARSE, 1.0), (O1 + R2, 48.0), (O1 + R3, 26.0), (O1 + R4, 50.0), (O1 + L2, 0.0), (O1 + L3, 0.0),
            (O1 + VEL, 3.0), (O1 + RATE_SCALE, 4.0), (O2 + LEVEL, 70.0), (O2 + COARSE, 4.0), (O2 + R2, 72.0), (O2 + R3, 40.0), (O2 + R4, 60.0), (O2 + L2, 0.0),
            (O2 + L3, 0.0), (O2 + VEL, 5.0), (O2 + RATE_SCALE, 4.0), (O3 + LEVEL, 90.0), (O3 + COARSE, 1.0), (O3 + R2, 44.0), (O3 + R3, 26.0), (O3 + R4, 50.0),
            (O3 + L2, 0.0), (O3 + L3, 0.0), (O3 + VEL, 2.0), (O3 + RATE_SCALE, 4.0), (O4 + LEVEL, 60.0), (O4 + COARSE, 3.0), (O4 + FINE, 50.0), (O4 + R2, 80.0),
            (O4 + R3, 40.0), (O4 + R4, 60.0), (O4 + L2, 0.0), (O4 + L3, 0.0), (O4 + VEL, 6.0), (O4 + RATE_SCALE, 4.0), (O5 + LEVEL, 50.0), (O5 + COARSE, 7.0),
            (O5 + R2, 86.0), (O5 + R3, 40.0), (O5 + R4, 60.0), (O5 + L2, 0.0), (O5 + L3, 0.0), (O5 + VEL, 6.0), (O6 + LEVEL, 0.0), (O6 + COARSE, 1.0),
        ],
    },
    FactoryPreset {
        name: "Long Tubular Chime",
        values: &[
            (ALGORITHM, 5.0), (O1 + LEVEL, 99.0), (O1 + COARSE, 1.0), (O1 + R2, 20.0), (O1 + R3, 18.0), (O1 + R4, 28.0), (O1 + L2, 60.0), (O1 + L3, 0.0),
            (O1 + RATE_SCALE, 2.0), (O1 + VEL, 1.0), (O2 + LEVEL, 74.0), (O2 + COARSE, 3.0), (O2 + FINE, 50.0), (O2 + R2, 24.0), (O2 + R3, 20.0), (O2 + R4, 28.0),
            (O2 + L2, 40.0), (O2 + L3, 0.0), (O2 + RATE_SCALE, 2.0), (O2 + VEL, 3.0), (O3 + LEVEL, 88.0), (O3 + COARSE, 2.0), (O3 + R2, 18.0), (O3 + R3, 16.0),
            (O3 + R4, 26.0), (O3 + L2, 50.0), (O3 + L3, 0.0), (O3 + RATE_SCALE, 2.0), (O3 + VEL, 1.0), (O4 + LEVEL, 66.0), (O4 + COARSE, 5.0), (O4 + FINE, 41.0),
            (O4 + R2, 30.0), (O4 + R3, 20.0), (O4 + R4, 26.0), (O4 + L2, 30.0), (O4 + L3, 0.0), (O4 + RATE_SCALE, 2.0), (O4 + VEL, 3.0), (O5 + LEVEL, 76.0),
            (O5 + COARSE, 4.0), (O5 + R2, 26.0), (O5 + R3, 18.0), (O5 + R4, 26.0), (O5 + L2, 40.0), (O5 + L3, 0.0), (O5 + RATE_SCALE, 2.0), (O6 + LEVEL, 58.0),
            (O6 + COARSE, 9.0), (O6 + FINE, 10.0), (O6 + R2, 34.0), (O6 + R3, 20.0), (O6 + R4, 26.0), (O6 + L2, 20.0), (O6 + L3, 0.0), (O6 + RATE_SCALE, 2.0),
        ],
    },
    FactoryPreset {
        name: "Stiff Section Brass",
        values: &[
            (ALGORITHM, 22.0), (FEEDBACK, 6.0), (LFO_PMD, 4.0), (LFO_SPEED, 36.0), (LFO_DELAY, 50.0), (PR1, 80.0), (PL1, 47.0), (O1 + LEVEL, 92.0),
            (O1 + COARSE, 1.0), (O1 + R1, 60.0), (O1 + R2, 30.0), (O1 + R3, 40.0), (O1 + R4, 60.0), (O1 + L2, 88.0), (O1 + L3, 84.0), (O1 + VEL, 2.0),
            (O2 + LEVEL, 70.0), (O2 + COARSE, 1.0), (O2 + R1, 62.0), (O2 + R2, 32.0), (O2 + R3, 40.0), (O2 + R4, 60.0), (O2 + L1, 90.0), (O2 + L2, 70.0),
            (O2 + L3, 62.0), (O2 + VEL, 4.0), (O3 + LEVEL, 90.0), (O3 + COARSE, 1.0), (O3 + R1, 58.0), (O3 + R2, 30.0), (O3 + R3, 40.0), (O3 + R4, 60.0),
            (O3 + L2, 88.0), (O3 + L3, 84.0), (O3 + DETUNE, 2.0), (O3 + VEL, 2.0), (O4 + LEVEL, 88.0), (O4 + COARSE, 1.0), (O4 + R1, 56.0), (O4 + R2, 30.0),
            (O4 + R3, 40.0), (O4 + R4, 60.0), (O4 + L2, 88.0), (O4 + L3, 84.0), (O4 + DETUNE, -2.0), (O4 + VEL, 2.0), (O5 + LEVEL, 88.0), (O5 + COARSE, 2.0),
            (O5 + R1, 54.0), (O5 + R2, 30.0), (O5 + R3, 40.0), (O5 + R4, 60.0), (O5 + L2, 84.0), (O5 + L3, 80.0), (O5 + DETUNE, 1.0), (O5 + VEL, 2.0),
            (O6 + LEVEL, 68.0), (O6 + COARSE, 1.0), (O6 + R1, 50.0), (O6 + R2, 30.0), (O6 + R3, 40.0), (O6 + R4, 60.0), (O6 + L1, 96.0), (O6 + L2, 76.0),
            (O6 + L3, 66.0), (O6 + VEL, 5.0),
        ],
    },
    FactoryPreset {
        name: "Biting Clav",
        values: &[
            (ALGORITHM, 3.0), (FEEDBACK, 3.0), (O1 + LEVEL, 99.0), (O1 + COARSE, 1.0), (O1 + R2, 50.0), (O1 + R3, 30.0), (O1 + R4, 80.0), (O1 + L2, 0.0),
            (O1 + L3, 0.0), (O1 + VEL, 2.0), (O1 + RATE_SCALE, 3.0), (O2 + LEVEL, 78.0), (O2 + COARSE, 3.0), (O2 + R2, 60.0), (O2 + R3, 30.0), (O2 + R4, 80.0),
            (O2 + L2, 20.0), (O2 + L3, 0.0), (O2 + VEL, 6.0), (O2 + RATE_SCALE, 3.0), (O3 + LEVEL, 70.0), (O3 + COARSE, 5.0), (O3 + R2, 70.0), (O3 + R3, 30.0),
            (O3 + R4, 80.0), (O3 + L2, 0.0), (O3 + L3, 0.0), (O3 + VEL, 6.0), (O3 + RATE_SCALE, 3.0), (O4 + LEVEL, 90.0), (O4 + COARSE, 1.0), (O4 + R2, 46.0),
            (O4 + R3, 30.0), (O4 + R4, 80.0), (O4 + L2, 0.0), (O4 + L3, 0.0), (O4 + VEL, 2.0), (O4 + RATE_SCALE, 3.0), (O4 + DETUNE, 3.0), (O5 + LEVEL, 76.0),
            (O5 + COARSE, 4.0), (O5 + R2, 62.0), (O5 + R3, 30.0), (O5 + R4, 80.0), (O5 + L2, 12.0), (O5 + L3, 0.0), (O5 + VEL, 5.0), (O5 + RATE_SCALE, 3.0),
            (O6 + LEVEL, 70.0), (O6 + COARSE, 7.0), (O6 + R2, 74.0), (O6 + R3, 30.0), (O6 + R4, 80.0), (O6 + L2, 0.0), (O6 + L3, 0.0), (O6 + VEL, 5.0),
        ],
    },
    FactoryPreset {
        name: "Thin Silk Strings",
        values: &[
            (ALGORITHM, 2.0), (FEEDBACK, 2.0), (LFO_PMD, 6.0), (LFO_SPEED, 30.0), (LFO_DELAY, 60.0), (O1 + LEVEL, 90.0), (O1 + COARSE, 1.0), (O1 + R1, 42.0),
            (O1 + R2, 30.0), (O1 + R3, 40.0), (O1 + R4, 44.0), (O1 + L2, 92.0), (O1 + L3, 90.0), (O2 + LEVEL, 60.0), (O2 + COARSE, 1.0), (O2 + R1, 44.0),
            (O2 + R2, 32.0), (O2 + R3, 40.0), (O2 + R4, 44.0), (O2 + L1, 90.0), (O2 + L2, 80.0), (O2 + L3, 76.0), (O3 + LEVEL, 92.0), (O3 + COARSE, 1.0),
            (O3 + R1, 40.0), (O3 + R2, 30.0), (O3 + R3, 40.0), (O3 + R4, 44.0), (O3 + L2, 92.0), (O3 + L3, 90.0), (O3 + DETUNE, 3.0), (O4 + LEVEL, 62.0),
            (O4 + COARSE, 1.0), (O4 + R1, 38.0), (O4 + R2, 30.0), (O4 + R3, 40.0), (O4 + R4, 44.0), (O4 + L1, 92.0), (O4 + L2, 84.0), (O4 + L3, 80.0),
            (O5 + LEVEL, 60.0), (O5 + COARSE, 2.0), (O5 + R1, 36.0), (O5 + R2, 30.0), (O5 + R3, 40.0), (O5 + R4, 44.0), (O5 + L1, 90.0), (O5 + L2, 82.0),
            (O5 + L3, 78.0), (O6 + LEVEL, 58.0), (O6 + COARSE, 3.0), (O6 + R1, 34.0), (O6 + R2, 30.0), (O6 + R3, 40.0), (O6 + R4, 44.0), (O6 + L1, 88.0),
            (O6 + L2, 80.0), (O6 + L3, 72.0),
        ],
    },
    FactoryPreset {
        name: "Glass Harp",
        values: &[
            (ALGORITHM, 5.0), (O1 + LEVEL, 99.0), (O1 + COARSE, 1.0), (O1 + R2, 32.0), (O1 + R3, 24.0), (O1 + R4, 44.0), (O1 + L2, 50.0), (O1 + L3, 0.0),
            (O1 + VEL, 3.0), (O1 + RATE_SCALE, 3.0), (O2 + LEVEL, 64.0), (O2 + COARSE, 2.0), (O2 + R2, 44.0), (O2 + R3, 30.0), (O2 + R4, 44.0), (O2 + L2, 0.0),
            (O2 + L3, 0.0), (O2 + VEL, 6.0), (O2 + RATE_SCALE, 3.0), (O3 + LEVEL, 92.0), (O3 + COARSE, 2.0), (O3 + R2, 36.0), (O3 + R3, 26.0), (O3 + R4, 44.0),
            (O3 + L2, 40.0), (O3 + L3, 0.0), (O3 + VEL, 3.0), (O3 + RATE_SCALE, 3.0), (O4 + LEVEL, 60.0), (O4 + COARSE, 7.0), (O4 + R2, 50.0), (O4 + R3, 30.0),
            (O4 + R4, 44.0), (O4 + L2, 0.0), (O4 + L3, 0.0), (O4 + VEL, 7.0), (O4 + RATE_SCALE, 4.0), (O5 + LEVEL, 80.0), (O5 + COARSE, 4.0), (O5 + R2, 38.0),
            (O5 + R3, 26.0), (O5 + R4, 44.0), (O5 + L2, 30.0), (O5 + L3, 0.0), (O5 + VEL, 4.0), (O5 + RATE_SCALE, 3.0), (O6 + LEVEL, 50.0), (O6 + COARSE, 11.0),
            (O6 + R2, 56.0), (O6 + R3, 30.0), (O6 + R4, 44.0), (O6 + L2, 0.0), (O6 + L3, 0.0), (O6 + VEL, 7.0),
        ],
    },
    FactoryPreset {
        name: "Crystal Motion Pad",
        values: &[
            (ALGORITHM, 31.0), (FEEDBACK, 2.0), (LFO_AMD, 30.0), (LFO_SPEED, 28.0), (LFO_WAVE, 4.0), (O1 + LEVEL, 90.0), (O1 + COARSE, 1.0), (O1 + R1, 30.0),
            (O1 + R2, 20.0), (O1 + R3, 24.0), (O1 + R4, 32.0), (O1 + L2, 90.0), (O1 + L3, 88.0), (O2 + LEVEL, 80.0), (O2 + COARSE, 2.0), (O2 + R1, 28.0),
            (O2 + R2, 18.0), (O2 + R3, 22.0), (O2 + R4, 32.0), (O2 + L2, 84.0), (O2 + L3, 80.0), (O2 + DETUNE, 3.0), (O3 + LEVEL, 72.0), (O3 + COARSE, 4.0),
            (O3 + FINE, 1.0), (O3 + R1, 26.0), (O3 + R2, 22.0), (O3 + R3, 24.0), (O3 + R4, 32.0), (O3 + L2, 80.0), (O3 + L3, 70.0), (O3 + AMS_FIELD, 2.0),
            (O4 + LEVEL, 66.0), (O4 + COARSE, 6.0), (O4 + FINE, 2.0), (O4 + R1, 32.0), (O4 + R2, 20.0), (O4 + R3, 18.0), (O4 + R4, 32.0), (O4 + L2, 76.0),
            (O4 + L3, 60.0), (O4 + AMS_FIELD, 3.0), (O5 + LEVEL, 82.0), (O5 + COARSE, 1.0), (O5 + R1, 24.0), (O5 + R2, 18.0), (O5 + R3, 20.0), (O5 + R4, 32.0),
            (O5 + L2, 86.0), (O5 + L3, 80.0), (O5 + DETUNE, -3.0), (O6 + LEVEL, 54.0), (O6 + COARSE, 8.0), (O6 + R1, 20.0), (O6 + R2, 24.0), (O6 + R3, 20.0),
            (O6 + R4, 32.0), (O6 + L2, 70.0), (O6 + L3, 60.0),
        ],
    },
    FactoryPreset {
        name: "Drawbar Sines",
        values: &[
            (ALGORITHM, 32.0), (FEEDBACK, 1.0), (LFO_PMD, 5.0), (LFO_SPEED, 44.0), (VOLUME, 6.0), (O1 + LEVEL, 84.0), (O1 + COARSE, 1.0), (O2 + LEVEL, 76.0),
            (O2 + COARSE, 0.0), (O3 + LEVEL, 80.0), (O3 + COARSE, 2.0), (O4 + LEVEL, 70.0), (O4 + COARSE, 3.0), (O5 + LEVEL, 62.0), (O5 + COARSE, 4.0), (O6 + LEVEL, 56.0),
            (O6 + COARSE, 6.0),
        ],
    },
    FactoryPreset {
        name: "Breath Flute",
        values: &[
            (ALGORITHM, 18.0), (FEEDBACK, 7.0), (LFO_PMD, 7.0), (LFO_SPEED, 38.0), (LFO_DELAY, 56.0), (O1 + LEVEL, 96.0), (O1 + COARSE, 1.0), (O1 + R1, 58.0),
            (O1 + R2, 40.0), (O1 + R3, 40.0), (O1 + R4, 60.0), (O1 + L2, 92.0), (O1 + L3, 90.0), (O1 + VEL, 2.0), (O2 + LEVEL, 46.0), (O2 + COARSE, 1.0),
            (O2 + R1, 50.0), (O2 + R2, 40.0), (O2 + R3, 40.0), (O2 + R4, 60.0), (O2 + L1, 90.0), (O2 + L2, 60.0), (O2 + L3, 50.0), (O2 + VEL, 2.0),
            (O3 + LEVEL, 52.0), (O3 + COARSE, 3.0), (O3 + R1, 70.0), (O3 + R2, 50.0), (O3 + R3, 40.0), (O3 + R4, 60.0), (O3 + L1, 90.0), (O3 + L2, 40.0),
            (O3 + L3, 30.0), (O3 + VEL, 3.0), (O4 + LEVEL, 46.0), (O4 + COARSE, 2.0), (O4 + R1, 60.0), (O4 + R2, 40.0), (O4 + R3, 40.0), (O4 + R4, 60.0),
            (O4 + L1, 80.0), (O4 + L2, 60.0), (O4 + L3, 50.0), (O5 + LEVEL, 40.0), (O5 + COARSE, 5.0), (O5 + R1, 70.0), (O5 + R2, 50.0), (O5 + R3, 40.0),
            (O5 + R4, 60.0), (O5 + L1, 70.0), (O5 + L2, 40.0), (O5 + L3, 30.0), (O6 + LEVEL, 50.0), (O6 + COARSE, 9.0), (O6 + R1, 80.0), (O6 + R2, 40.0),
            (O6 + R3, 30.0), (O6 + R4, 60.0), (O6 + L1, 90.0), (O6 + L2, 36.0), (O6 + L3, 20.0),
        ],
    },
    FactoryPreset {
        name: "Inharmonic Steel",
        values: &[
            (ALGORITHM, 1.0), (FEEDBACK, 3.0), (O1 + LEVEL, 99.0), (O1 + COARSE, 1.0), (O1 + R2, 30.0), (O1 + R3, 20.0), (O1 + R4, 40.0), (O1 + L2, 40.0),
            (O1 + L3, 0.0), (O1 + VEL, 2.0), (O1 + RATE_SCALE, 2.0), (O2 + LEVEL, 78.0), (O2 + COARSE, 1.0), (O2 + FINE, 41.0), (O2 + R2, 40.0), (O2 + R3, 24.0),
            (O2 + R4, 40.0), (O2 + L2, 20.0), (O2 + L3, 0.0), (O2 + VEL, 5.0), (O2 + RATE_SCALE, 3.0), (O3 + LEVEL, 90.0), (O3 + COARSE, 1.0), (O3 + R2, 32.0),
            (O3 + R3, 22.0), (O3 + R4, 40.0), (O3 + L2, 40.0), (O3 + L3, 0.0), (O3 + DETUNE, 3.0), (O3 + VEL, 2.0), (O4 + LEVEL, 76.0), (O4 + COARSE, 2.0),
            (O4 + FINE, 76.0), (O4 + R2, 40.0), (O4 + R3, 24.0), (O4 + R4, 40.0), (O4 + L2, 24.0), (O4 + L3, 0.0), (O4 + VEL, 5.0), (O5 + LEVEL, 62.0),
            (O5 + COARSE, 6.0), (O5 + FINE, 27.0), (O5 + R2, 50.0), (O5 + R3, 30.0), (O5 + R4, 40.0), (O5 + L2, 0.0), (O5 + L3, 0.0), (O5 + VEL, 6.0),
            (O6 + LEVEL, 50.0), (O6 + COARSE, 4.0), (O6 + FINE, 14.0), (O6 + R2, 60.0), (O6 + R3, 30.0), (O6 + R4, 40.0), (O6 + L2, 0.0), (O6 + L3, 0.0),
        ],
    },
    FactoryPreset {
        name: "Reedy Mono Lead",
        values: &[
            (ALGORITHM, 1.0), (FEEDBACK, 6.0), (POLY, 1.0), (PORTA, 44.0), (LFO_PMD, 10.0), (LFO_SPEED, 40.0), (LFO_DELAY, 56.0), (O1 + LEVEL, 96.0),
            (O1 + COARSE, 1.0), (O1 + R1, 90.0), (O1 + R2, 40.0), (O1 + R3, 40.0), (O1 + R4, 70.0), (O1 + L2, 92.0), (O1 + L3, 90.0), (O1 + VEL, 2.0),
            (O2 + LEVEL, 74.0), (O2 + COARSE, 2.0), (O2 + R1, 88.0), (O2 + R2, 50.0), (O2 + R3, 40.0), (O2 + R4, 70.0), (O2 + L1, 90.0), (O2 + L2, 76.0),
            (O2 + L3, 70.0), (O2 + VEL, 3.0), (O3 + LEVEL, 88.0), (O3 + COARSE, 1.0), (O3 + R1, 90.0), (O3 + R2, 40.0), (O3 + R3, 40.0), (O3 + R4, 70.0),
            (O3 + L2, 92.0), (O3 + L3, 90.0), (O3 + DETUNE, 2.0), (O4 + LEVEL, 70.0), (O4 + COARSE, 3.0), (O4 + R1, 80.0), (O4 + R2, 50.0), (O4 + R3, 40.0),
            (O4 + R4, 70.0), (O4 + L1, 90.0), (O4 + L2, 70.0), (O4 + L3, 60.0), (O5 + LEVEL, 62.0), (O5 + COARSE, 1.0), (O5 + R1, 80.0), (O5 + R2, 50.0),
            (O5 + R3, 40.0), (O5 + R4, 70.0), (O5 + L1, 90.0), (O5 + L2, 70.0), (O5 + L3, 66.0), (O6 + LEVEL, 60.0), (O6 + COARSE, 2.0), (O6 + R1, 80.0),
            (O6 + R2, 50.0), (O6 + R3, 40.0), (O6 + R4, 70.0), (O6 + L1, 84.0), (O6 + L2, 70.0), (O6 + L3, 60.0),
        ],
    },
    FactoryPreset {
        name: "Feedback Sub Bass",
        values: &[
            (ALGORITHM, 32.0), (FEEDBACK, 5.0), (TRANSPOSE, -24.0), (O1 + LEVEL, 0.0), (O1 + COARSE, 1.0), (O2 + LEVEL, 0.0), (O2 + COARSE, 1.0), (O3 + LEVEL, 0.0),
            (O3 + COARSE, 1.0), (O4 + LEVEL, 0.0), (O4 + COARSE, 1.0), (O5 + LEVEL, 0.0), (O5 + COARSE, 1.0), (O6 + LEVEL, 99.0), (O6 + COARSE, 1.0), (O6 + R2, 40.0),
            (O6 + R3, 40.0), (O6 + R4, 70.0), (O6 + L2, 86.0), (O6 + L3, 84.0), (O6 + VEL, 2.0),
        ],
    },
    FactoryPreset {
        name: "Buzzing Sitar Pluck",
        values: &[
            (ALGORITHM, 4.0), (FEEDBACK, 7.0), (O1 + LEVEL, 99.0), (O1 + COARSE, 1.0), (O1 + R2, 30.0), (O1 + R3, 22.0), (O1 + R4, 50.0), (O1 + L2, 50.0),
            (O1 + L3, 0.0), (O1 + RATE_SCALE, 3.0), (O1 + VEL, 2.0), (O2 + LEVEL, 72.0), (O2 + COARSE, 3.0), (O2 + R2, 40.0), (O2 + R3, 22.0), (O2 + R4, 50.0),
            (O2 + L2, 30.0), (O2 + L3, 0.0), (O2 + VEL, 4.0), (O3 + LEVEL, 60.0), (O3 + COARSE, 1.0), (O3 + R2, 40.0), (O3 + R3, 26.0), (O3 + R4, 50.0),
            (O3 + L2, 30.0), (O3 + L3, 0.0), (O3 + VEL, 4.0), (O4 + LEVEL, 88.0), (O4 + COARSE, 1.0), (O4 + R2, 28.0), (O4 + R3, 22.0), (O4 + R4, 50.0),
            (O4 + L2, 50.0), (O4 + L3, 0.0), (O4 + RATE_SCALE, 3.0), (O4 + DETUNE, 2.0), (O4 + VEL, 2.0), (O5 + LEVEL, 78.0), (O5 + COARSE, 2.0), (O5 + R2, 34.0),
            (O5 + R3, 22.0), (O5 + R4, 50.0), (O5 + L2, 40.0), (O5 + L3, 0.0), (O5 + VEL, 4.0), (O6 + LEVEL, 72.0), (O6 + COARSE, 5.0), (O6 + R2, 38.0),
            (O6 + R3, 22.0), (O6 + R4, 50.0), (O6 + L2, 30.0), (O6 + L3, 0.0), (O6 + VEL, 4.0),
        ],
    },
    FactoryPreset {
        name: "Sweep Noise Hit",
        values: &[
            (ALGORITHM, 6.0), (FEEDBACK, 7.0), (PR1, 99.0), (PR2, 46.0), (PL1, 50.0), (PL2, 86.0), (PL3, 30.0), (PL4, 30.0),
            (O1 + LEVEL, 70.0), (O1 + COARSE, 1.0), (O1 + R2, 60.0), (O1 + R3, 30.0), (O1 + R4, 55.0), (O1 + L2, 0.0), (O1 + L3, 0.0), (O2 + LEVEL, 80.0),
            (O2 + COARSE, 6.0), (O2 + R2, 70.0), (O2 + R3, 30.0), (O2 + R4, 55.0), (O2 + L2, 0.0), (O2 + L3, 0.0), (O3 + LEVEL, 70.0), (O3 + COARSE, 1.0),
            (O3 + R2, 60.0), (O3 + R3, 30.0), (O3 + R4, 55.0), (O3 + L2, 0.0), (O3 + L3, 0.0), (O4 + LEVEL, 84.0), (O4 + COARSE, 9.0), (O4 + R2, 70.0),
            (O4 + R3, 30.0), (O4 + R4, 55.0), (O4 + L2, 0.0), (O4 + L3, 0.0), (O5 + LEVEL, 99.0), (O5 + COARSE, 1.0), (O5 + R2, 44.0), (O5 + R3, 30.0),
            (O5 + R4, 55.0), (O5 + L2, 0.0), (O5 + L3, 0.0), (O6 + LEVEL, 99.0), (O6 + COARSE, 1.0), (O6 + FINE, 41.0), (O6 + R2, 40.0), (O6 + R3, 30.0),
            (O6 + R4, 55.0), (O6 + L2, 50.0), (O6 + L3, 0.0),
        ],
    },
];
