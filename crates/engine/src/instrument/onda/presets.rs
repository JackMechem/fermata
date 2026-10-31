//! Factory presets of Fermata Onda: written here. Parameters a preset doesn't list go to their
//! defaults. A slot is three values: source, destination, amount.

use super::params::param::*;
use crate::fx::FactoryPreset;

pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Seven Saw Anthem",
        values: &[
            (A_TABLE, 0.0), (A_POS, 0.5), (A_VOICES, 6.0), (A_DETUNE, 0.28), (A_BLEND, 0.8), (B_ON, 1.0), (B_TABLE, 0.0), (B_POS, 0.5),
            (B_OCT, 1.0), (B_VOICES, 4.0), (B_DETUNE, 0.2), (B_LEVEL, 0.45), (FILTER_TYPE, 1.0), (CUTOFF, 1800.0), (RESONANCE, 0.2), (E1_ATTACK, 8.0),
            (E1_SUSTAIN, 0.9), (E1_RELEASE, 420.0), (E2_DECAY, 700.0), (E2_SUSTAIN, 0.25), (MOD_BASE, 1.0), (MOD_BASE + 1, 7.0), (MOD_BASE + 2, 0.35),
        ],
    },
    FactoryPreset {
        name: "Wide Reese Bass",
        values: &[
            (A_TABLE, 0.0), (A_POS, 0.5), (A_OCT, -2.0), (A_VOICES, 3.0), (A_DETUNE, 0.12), (A_SPREAD, 0.6), (B_ON, 1.0), (B_TABLE, 4.0),
            (B_POS, 0.25), (B_OCT, -2.0), (B_VOICES, 3.0), (B_DETUNE, 0.14), (B_LEVEL, 0.6), (SUB_LEVEL, 0.6), (FILTER_TYPE, 5.0), (CUTOFF, 520.0),
            (RESONANCE, 0.35), (DRIVE, 0.35), (E1_SUSTAIN, 1.0), (E1_RELEASE, 120.0), (L1_RATE, 0.25), (L1_WAVE, 0.0), (VOICE_MODE, 2.0), (GLIDE, 40.0),
            (MOD_BASE, 3.0), (MOD_BASE + 1, 1.0), (MOD_BASE + 2, 0.2), (MOD_BASE + 3, 3.0), (MOD_BASE + 4, 7.0), (MOD_BASE + 5, 0.08),
        ],
    },
    FactoryPreset {
        name: "Tempo Wobble Bass",
        values: &[
            (A_TABLE, 5.0), (A_POS, 0.2), (A_OCT, -2.0), (SUB_LEVEL, 0.7), (FILTER_TYPE, 1.0), (CUTOFF, 300.0), (RESONANCE, 0.45), (DRIVE, 0.25),
            (E1_SUSTAIN, 1.0), (E1_RELEASE, 90.0), (L1_SYNC, 6.0), (L1_WAVE, 1.0), (VOICE_MODE, 2.0), (VOLUME, 6.5), (MOD_BASE, 3.0), (MOD_BASE + 1, 7.0),
            (MOD_BASE + 2, 0.35), (MOD_BASE + 3, 3.0), (MOD_BASE + 4, 1.0), (MOD_BASE + 5, 0.3),
        ],
    },
    FactoryPreset {
        name: "Vowel Morph Lead",
        values: &[
            (A_TABLE, 2.0), (A_POS, 0.0), (A_VOICES, 2.0), (A_DETUNE, 0.08), (B_ON, 1.0), (B_TABLE, 0.0), (B_POS, 0.25), (B_LEVEL, 0.25),
            (FILTER_ON, 0.0), (E1_ATTACK, 10.0), (E1_SUSTAIN, 0.9), (E1_RELEASE, 200.0), (L1_RATE, 1.2), (L1_WAVE, 0.0), (VOICE_MODE, 1.0), (GLIDE, 70.0),
            (VOLUME, 6.0), (MOD_BASE, 3.0), (MOD_BASE + 1, 1.0), (MOD_BASE + 2, 0.5), (MOD_BASE + 3, 7.0), (MOD_BASE + 4, 3.0), (MOD_BASE + 5, 0.01),
        ],
    },
    FactoryPreset {
        name: "Harmonic Bloom Pad",
        values: &[
            (A_TABLE, 1.0), (A_POS, 0.05), (A_VOICES, 4.0), (A_DETUNE, 0.18), (B_ON, 1.0), (B_TABLE, 7.0), (B_POS, 0.3), (B_VOICES, 3.0),
            (B_DETUNE, 0.14), (B_OCT, 1.0), (B_LEVEL, 0.4), (FILTER_TYPE, 0.0), (CUTOFF, 4500.0), (E1_ATTACK, 900.0), (E1_SUSTAIN, 1.0), (E1_RELEASE, 1800.0),
            (E2_ATTACK, 4000.0), (E2_DECAY, 5000.0), (E2_SUSTAIN, 0.7), (L2_RATE, 0.12), (MOD_BASE, 1.0), (MOD_BASE + 1, 1.0), (MOD_BASE + 2, 0.8), (MOD_BASE + 3, 4.0),
            (MOD_BASE + 4, 2.0), (MOD_BASE + 5, 0.3),
        ],
    },
    FactoryPreset {
        name: "Glass Sync Pluck",
        values: &[
            (A_TABLE, 4.0), (A_POS, 0.5), (A_VOICES, 2.0), (A_DETUNE, 0.06), (FILTER_TYPE, 1.0), (CUTOFF, 1200.0), (RESONANCE, 0.25), (KEY_TRACK, 0.6),
            (E1_DECAY, 420.0), (E1_SUSTAIN, 0.0), (E1_RELEASE, 260.0), (E2_DECAY, 260.0), (E2_SUSTAIN, 0.0), (MOD_BASE, 1.0), (MOD_BASE + 1, 1.0), (MOD_BASE + 2, -0.45),
            (MOD_BASE + 3, 1.0), (MOD_BASE + 4, 7.0), (MOD_BASE + 5, 0.45), (MOD_BASE + 6, 5.0), (MOD_BASE + 7, 7.0), (MOD_BASE + 8, 0.15),
        ],
    },
    FactoryPreset {
        name: "FM Metal Keys",
        values: &[
            (A_TABLE, 5.0), (A_POS, 0.1), (B_ON, 1.0), (B_TABLE, 5.0), (B_POS, 0.25), (B_OCT, 2.0), (B_SEMI, 7.0), (B_LEVEL, 0.0),
            (FM_AMOUNT, 0.3), (FILTER_ON, 0.0), (E1_DECAY, 1200.0), (E1_SUSTAIN, 0.1), (E1_RELEASE, 500.0), (E2_DECAY, 400.0), (E2_SUSTAIN, 0.0), (MOD_BASE, 1.0),
            (MOD_BASE + 1, 10.0), (MOD_BASE + 2, 0.5), (MOD_BASE + 3, 5.0), (MOD_BASE + 4, 1.0), (MOD_BASE + 5, 0.3),
        ],
    },
    FactoryPreset {
        name: "Folded Growl",
        values: &[
            (A_TABLE, 8.0), (A_POS, 0.15), (A_OCT, -1.0), (A_VOICES, 2.0), (A_DETUNE, 0.05), (SUB_LEVEL, 0.5), (FILTER_TYPE, 6.0), (CUTOFF, 900.0),
            (RESONANCE, 0.75), (DRIVE, 0.4), (E1_SUSTAIN, 0.9), (E1_RELEASE, 140.0), (E2_DECAY, 350.0), (E2_SUSTAIN, 0.1), (L1_RATE, 4.0), (L1_WAVE, 2.0),
            (VOICE_MODE, 1.0), (GLIDE, 30.0), (VOLUME, 6.5), (MOD_BASE, 1.0), (MOD_BASE + 1, 1.0), (MOD_BASE + 2, 0.7), (MOD_BASE + 3, 1.0), (MOD_BASE + 4, 7.0),
            (MOD_BASE + 5, 0.3), (MOD_BASE + 6, 7.0), (MOD_BASE + 7, 1.0), (MOD_BASE + 8, 0.4),
        ],
    },
    FactoryPreset {
        name: "Stepped Digital Bell",
        values: &[
            (A_TABLE, 6.0), (A_POS, 0.3), (B_ON, 1.0), (B_TABLE, 9.0), (B_POS, 0.5), (B_OCT, 1.0), (B_LEVEL, 0.4), (FILTER_TYPE, 0.0),
            (CUTOFF, 7000.0), (E1_DECAY, 1800.0), (E1_SUSTAIN, 0.0), (E1_RELEASE, 800.0), (E2_DECAY, 900.0), (E2_SUSTAIN, 0.0), (L2_RATE, 0.3), (MOD_BASE, 1.0),
            (MOD_BASE + 1, 1.0), (MOD_BASE + 2, 0.5), (MOD_BASE + 3, 4.0), (MOD_BASE + 4, 2.0), (MOD_BASE + 5, 0.4),
        ],
    },
    FactoryPreset {
        name: "Comb Sweep Pad",
        values: &[
            (A_TABLE, 9.0), (A_POS, 0.1), (A_VOICES, 3.0), (A_DETUNE, 0.14), (B_ON, 1.0), (B_TABLE, 9.0), (B_POS, 0.6), (B_VOICES, 3.0),
            (B_DETUNE, 0.16), (FILTER_TYPE, 4.0), (CUTOFF, 1800.0), (RESONANCE, 0.4), (E1_ATTACK, 400.0), (E1_SUSTAIN, 1.0), (E1_RELEASE, 1500.0), (L1_RATE, 0.09),
            (L1_WAVE, 0.0), (L2_RATE, 0.05), (MOD_BASE, 3.0), (MOD_BASE + 1, 1.0), (MOD_BASE + 2, 0.45), (MOD_BASE + 3, 4.0), (MOD_BASE + 4, 2.0), (MOD_BASE + 5, 0.4),
            (MOD_BASE + 6, 3.0), (MOD_BASE + 7, 7.0), (MOD_BASE + 8, 0.3),
        ],
    },
    FactoryPreset {
        name: "Band Pass Vocal",
        values: &[
            (A_TABLE, 0.0), (A_POS, 0.5), (A_VOICES, 3.0), (A_DETUNE, 0.1), (FILTER_TYPE, 2.0), (CUTOFF, 900.0), (RESONANCE, 0.72), (KEY_TRACK, 0.4),
            (E1_ATTACK, 60.0), (E1_SUSTAIN, 0.9), (E1_RELEASE, 320.0), (L1_RATE, 0.5), (L1_WAVE, 1.0), (VOLUME, 8.5), (MOD_BASE, 3.0), (MOD_BASE + 1, 7.0),
            (MOD_BASE + 2, 0.15), (MOD_BASE + 3, 8.0), (MOD_BASE + 4, 8.0), (MOD_BASE + 5, 0.2),
        ],
    },
    FactoryPreset {
        name: "High Pass Air",
        values: &[
            (A_TABLE, 0.0), (A_POS, 0.5), (NOISE_LEVEL, 0.4), (A_VOICES, 4.0), (A_DETUNE, 0.3), (FILTER_TYPE, 3.0), (CUTOFF, 2600.0), (RESONANCE, 0.3),
            (E1_ATTACK, 600.0), (E1_SUSTAIN, 0.9), (E1_RELEASE, 900.0), (L1_RATE, 0.2), (L1_WAVE, 0.0), (VOLUME, 8.5), (MOD_BASE, 3.0), (MOD_BASE + 1, 7.0),
            (MOD_BASE + 2, 0.12), (MOD_BASE + 3, 3.0), (MOD_BASE + 4, 11.0), (MOD_BASE + 5, 0.4),
        ],
    },
    FactoryPreset {
        name: "Pulse Width Arp",
        values: &[
            (A_TABLE, 3.0), (A_POS, 0.2), (B_ON, 1.0), (B_TABLE, 3.0), (B_POS, 0.6), (B_FINE, 0.08), (FILTER_TYPE, 1.0), (CUTOFF, 1500.0),
            (RESONANCE, 0.3), (E1_DECAY, 190.0), (E1_SUSTAIN, 0.0), (E1_RELEASE, 140.0), (E2_DECAY, 160.0), (E2_SUSTAIN, 0.0), (L1_SYNC, 7.0), (L1_WAVE, 3.0),
            (MOD_BASE, 1.0), (MOD_BASE + 1, 7.0), (MOD_BASE + 2, 0.3), (MOD_BASE + 3, 3.0), (MOD_BASE + 4, 1.0), (MOD_BASE + 5, 0.1),
        ],
    },
    FactoryPreset {
        name: "Organ Drawbar Scan",
        values: &[
            (A_TABLE, 7.0), (A_POS, 0.4), (FILTER_ON, 0.0), (E1_ATTACK, 3.0), (E1_SUSTAIN, 1.0), (E1_RELEASE, 60.0), (L1_RATE, 5.8), (L1_WAVE, 0.0),
            (MOD_BASE, 7.0), (MOD_BASE + 1, 1.0), (MOD_BASE + 2, 0.6), (MOD_BASE + 3, 3.0), (MOD_BASE + 4, 3.0), (MOD_BASE + 5, 0.003),
        ],
    },
    FactoryPreset {
        name: "Dirty Acid Line",
        values: &[
            (A_TABLE, 0.0), (A_POS, 0.5), (FILTER_TYPE, 6.0), (CUTOFF, 420.0), (RESONANCE, 0.82), (DRIVE, 0.3), (E1_DECAY, 800.0), (E1_SUSTAIN, 0.4),
            (E1_RELEASE, 40.0), (E2_DECAY, 240.0), (E2_SUSTAIN, 0.0), (VOICE_MODE, 2.0), (GLIDE, 60.0), (VOLUME, 9.0), (MOD_BASE, 1.0), (MOD_BASE + 1, 7.0),
            (MOD_BASE + 2, 0.45), (MOD_BASE + 3, 5.0), (MOD_BASE + 4, 7.0), (MOD_BASE + 5, 0.12),
        ],
    },
    FactoryPreset {
        name: "Random Scan Texture",
        values: &[
            (A_TABLE, 8.0), (A_POS, 0.4), (A_VOICES, 2.0), (A_DETUNE, 0.1), (B_ON, 1.0), (B_TABLE, 5.0), (B_POS, 0.4), (B_LEVEL, 0.5),
            (FILTER_TYPE, 1.0), (CUTOFF, 2200.0), (RESONANCE, 0.35), (E1_ATTACK, 200.0), (E1_SUSTAIN, 0.9), (E1_RELEASE, 1100.0), (L1_RATE, 7.0), (L1_WAVE, 4.0),
            (L2_RATE, 0.4), (L1_RETRIG, 1.0), (MOD_BASE, 3.0), (MOD_BASE + 1, 1.0), (MOD_BASE + 2, 0.25), (MOD_BASE + 3, 3.0), (MOD_BASE + 4, 2.0), (MOD_BASE + 5, -0.2),
            (MOD_BASE + 6, 4.0), (MOD_BASE + 7, 7.0), (MOD_BASE + 8, 0.15),
        ],
    },
];
