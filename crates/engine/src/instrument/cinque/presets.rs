//! Factory presets of Fermata Cinque: written here, from what the instrument is known for.
//! Parameters a preset doesn't list go to their defaults.

use super::params::param::*;
use crate::fx::FactoryPreset;

const OFF: f32 = 0.0;
const ON: f32 = 1.0;
const KBD_HALF: f32 = 1.0;
const EARLY: f32 = 0.0;

pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Sync Sweep Lead",
        values: &[(A_SYNC, ON), (A_FREQ, 12.0), (B_SAW, ON), (MIX_A, 9.0), (MIX_B, 0.0), (PM_FENV, 3.5), (PM_FREQ_A, ON), (CUTOFF, 2600.0), (ENV_AMOUNT, 1.5), (F_DECAY, 900.0), (F_SUSTAIN, 2.0), (A_SUSTAIN, 9.0), (UNISON, ON), (UNISON_DETUNE, 0.1), (GLIDE, 2.0), (VOLUME, 6.0)],
    },
    FactoryPreset {
        name: "Clanging Bell Mod",
        values: &[(A_FREQ, 7.0), (B_FREQ, 24.0), (B_FINE, 0.12), (B_SAW, OFF), (B_TRI, ON), (MIX_B, 0.0), (MIX_A, 9.0), (PM_OSCB, 1.6), (PM_FREQ_A, ON), (CUTOFF, 3500.0), (ENV_AMOUNT, 2.5), (F_DECAY, 1800.0), (F_SUSTAIN, 0.0), (A_DECAY, 2800.0), (A_SUSTAIN, 0.0), (A_RELEASE, 1200.0), (F_RELEASE, 1200.0)],
    },
    FactoryPreset {
        name: "Warm Analog Brass",
        values: &[(A_SAW, ON), (B_SAW, ON), (B_FINE, 0.1), (MIX_A, 8.0), (MIX_B, 8.0), (CUTOFF, 420.0), (RESONANCE, 1.0), (ENV_AMOUNT, 4.5), (F_ATTACK, 90.0), (F_DECAY, 500.0), (F_SUSTAIN, 5.0), (A_ATTACK, 40.0), (A_SUSTAIN, 9.0), (F_RELEASE, 220.0), (A_RELEASE, 220.0), (CHIP, EARLY)],
    },
    FactoryPreset {
        name: "Silk Strings",
        values: &[(A_SAW, ON), (A_PULSE, ON), (A_PW, 0.3), (B_SAW, ON), (B_FINE, 0.14), (WM_PW_A, ON), (WM_FREQ_A, OFF), (WM_FREQ_B, OFF), (MOD_REST, 4.0), (LFO_RATE, 4.5), (MIX_A, 7.0), (MIX_B, 7.0), (CUTOFF, 2800.0), (ENV_AMOUNT, 0.8), (F_ATTACK, 800.0), (A_ATTACK, 500.0), (A_SUSTAIN, 9.0), (A_RELEASE, 1100.0), (F_RELEASE, 1100.0), (KEYBOARD, KBD_HALF)],
    },
    FactoryPreset {
        name: "Poly Mod Pad",
        values: &[(A_PULSE, ON), (A_SAW, OFF), (A_PW, 0.4), (B_TRI, ON), (B_SAW, OFF), (B_LO, ON), (B_FREQ, 5.0), (B_KBD, OFF), (MIX_B, 0.0), (MIX_A, 8.0), (PM_OSCB, 5.0), (PM_PW_A, ON), (PM_FILTER, ON), (PM_FENV, 0.5), (CUTOFF, 1500.0), (RESONANCE, 3.0), (ENV_AMOUNT, 1.0), (F_ATTACK, 1500.0), (A_ATTACK, 900.0), (A_SUSTAIN, 9.0), (A_RELEASE, 1800.0), (F_RELEASE, 1800.0)],
    },
    FactoryPreset {
        name: "Funky Filter Clav",
        values: &[(A_PULSE, ON), (A_SAW, OFF), (A_PW, 0.14), (B_PULSE, ON), (B_SAW, OFF), (B_PW, 0.22), (B_FREQ, 12.0), (MIX_A, 7.0), (MIX_B, 5.0), (CUTOFF, 900.0), (RESONANCE, 3.0), (ENV_AMOUNT, 3.6), (F_DECAY, 170.0), (F_SUSTAIN, 0.0), (A_DECAY, 300.0), (A_SUSTAIN, 0.0), (F_RELEASE, 80.0), (A_RELEASE, 80.0), (VEL_AMP, 0.5)],
    },
    FactoryPreset {
        name: "Fat Unison Bass",
        values: &[(UNISON, ON), (UNISON_DETUNE, 0.25), (A_FREQ, -12.0), (B_FREQ, -12.0), (A_SAW, ON), (B_PULSE, ON), (B_SAW, OFF), (MIX_A, 9.0), (MIX_B, 8.0), (CUTOFF, 200.0), (RESONANCE, 2.0), (ENV_AMOUNT, 3.8), (F_DECAY, 320.0), (F_SUSTAIN, 2.0), (A_SUSTAIN, 9.0), (F_RELEASE, 90.0), (A_RELEASE, 90.0), (CHIP, EARLY), (VOLUME, 7.0)],
    },
    FactoryPreset {
        name: "Sub Pluck Bass",
        values: &[(A_FREQ, -12.0), (B_FREQ, -24.0), (B_SAW, OFF), (B_TRI, ON), (MIX_A, 7.0), (MIX_B, 10.0), (CUTOFF, 190.0), (RESONANCE, 3.0), (ENV_AMOUNT, 3.6), (F_DECAY, 240.0), (F_SUSTAIN, 0.0), (A_DECAY, 420.0), (A_SUSTAIN, 2.0), (F_RELEASE, 70.0), (A_RELEASE, 70.0), (VOLUME, 8.0)],
    },
    FactoryPreset {
        name: "Metal Keys",
        values: &[(A_FREQ, 12.0), (B_FREQ, 31.0), (B_SAW, ON), (MIX_A, 8.0), (MIX_B, 1.5), (PM_OSCB, 0.8), (PM_FENV, 1.0), (PM_FREQ_A, ON), (CUTOFF, 1800.0), (ENV_AMOUNT, 3.0), (F_DECAY, 700.0), (F_SUSTAIN, 0.5), (A_DECAY, 1500.0), (A_SUSTAIN, 0.0), (F_RELEASE, 500.0), (A_RELEASE, 500.0), (VEL_FILTER, 0.5)],
    },
    FactoryPreset {
        name: "Wobbling Formant",
        values: &[(A_PULSE, ON), (A_SAW, ON), (A_PW, 0.25), (B_SAW, OFF), (B_TRI, ON), (B_FREQ, 12.0), (MIX_B, 0.0), (PM_OSCB, 2.0), (PM_FILTER, ON), (CUTOFF, 1100.0), (RESONANCE, 6.5), (ENV_AMOUNT, 1.2), (F_ATTACK, 300.0), (A_ATTACK, 100.0), (A_SUSTAIN, 9.0), (WM_FILTER, ON), (WM_FREQ_A, OFF), (WM_FREQ_B, OFF), (MOD_REST, 3.0), (LFO_RATE, 5.5)],
    },
    FactoryPreset {
        name: "Square Lead with Vibrato",
        values: &[(A_PULSE, ON), (A_SAW, OFF), (A_PW, 0.5), (B_PULSE, ON), (B_SAW, OFF), (B_PW, 0.46), (B_FINE, 0.08), (MIX_A, 7.0), (MIX_B, 6.0), (CUTOFF, 1900.0), (RESONANCE, 2.0), (ENV_AMOUNT, 1.5), (F_DECAY, 700.0), (A_SUSTAIN, 9.5), (GLIDE, 2.0), (UNISON, ON), (UNISON_DETUNE, 0.08), (MOD_REST, 2.2), (LFO_RATE, 6.4), (VOLUME, 6.0)],
    },
    FactoryPreset {
        name: "Airy Noise Whisper",
        values: &[(A_SAW, OFF), (B_SAW, OFF), (MIX_A, 0.0), (MIX_B, 0.0), (MIX_NOISE, 8.0), (CUTOFF, 1200.0), (RESONANCE, 5.0), (ENV_AMOUNT, 1.5), (F_ATTACK, 1200.0), (F_SUSTAIN, 6.0), (A_ATTACK, 900.0), (A_SUSTAIN, 10.0), (A_RELEASE, 1600.0), (F_RELEASE, 1600.0), (VOLUME, 9.5)],
    },
    FactoryPreset {
        name: "Vowel Sync Stab",
        values: &[(A_SYNC, ON), (A_FREQ, 19.0), (A_PULSE, ON), (A_SAW, OFF), (A_PW, 0.35), (B_SAW, ON), (MIX_B, 3.0), (PM_FENV, 2.5), (PM_FREQ_A, ON), (PM_PW_A, ON), (CUTOFF, 1400.0), (RESONANCE, 3.5), (ENV_AMOUNT, 2.5), (F_DECAY, 350.0), (F_SUSTAIN, 1.5), (A_DECAY, 700.0), (A_SUSTAIN, 0.0), (F_RELEASE, 160.0), (A_RELEASE, 160.0)],
    },
    FactoryPreset {
        name: "Soft Electric Piano",
        values: &[(A_SAW, OFF), (A_PULSE, ON), (A_PW, 0.5), (B_SAW, OFF), (B_TRI, ON), (B_FREQ, 12.0), (MIX_A, 3.0), (MIX_B, 8.0), (PM_OSCB, 0.5), (PM_FREQ_A, ON), (CUTOFF, 1300.0), (ENV_AMOUNT, 1.2), (F_DECAY, 800.0), (F_SUSTAIN, 2.0), (A_DECAY, 2200.0), (A_SUSTAIN, 0.8), (F_RELEASE, 280.0), (A_RELEASE, 280.0), (VEL_AMP, 0.5), (VEL_FILTER, 0.3)],
    },
    FactoryPreset {
        name: "Drifting Fifths",
        values: &[(B_FREQ, 7.0), (B_FINE, 0.05), (SLOP, 0.9), (MIX_A, 8.0), (MIX_B, 7.0), (CUTOFF, 900.0), (RESONANCE, 1.5), (ENV_AMOUNT, 2.5), (F_ATTACK, 120.0), (F_DECAY, 1200.0), (F_SUSTAIN, 4.0), (A_ATTACK, 30.0), (A_SUSTAIN, 9.0), (A_RELEASE, 700.0), (F_RELEASE, 700.0)],
    },
    FactoryPreset {
        name: "Siren Lo Mod",
        values: &[(B_LO, ON), (B_KBD, OFF), (B_TRI, ON), (B_SAW, OFF), (B_FREQ, 12.0), (MIX_B, 0.0), (MIX_A, 9.0), (PM_OSCB, 6.0), (PM_FREQ_A, ON), (CUTOFF, 3000.0), (ENV_AMOUNT, 0.0), (A_SUSTAIN, 10.0), (RESONANCE, 2.0)],
    },
];
