//! Factory presets of Fermata Sei: written here, from what the instrument is known for. Parameters
//! a preset doesn't list go to their defaults.

use super::params::param::*;
use crate::fx::FactoryPreset;

const R16: f32 = 0.0;
const R4: f32 = 2.0;
const LFO: f32 = 0.0;
const CH_OFF: f32 = 0.0;
const CH1: f32 = 1.0;
const CH2: f32 = 2.0;
const CH3: f32 = 3.0;
const GATE: f32 = 1.0;
const INVERT: f32 = 1.0;
const POLY2: f32 = 1.0;
const UNISON: f32 = 2.0;
const HP_BASS: f32 = 0.0;
const HP2: f32 = 2.0;
const HP3: f32 = 3.0;

pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Velvet Strings",
        values: &[(SAW_ON, 1.0), (SUB, 2.5), (HPF, HP2), (CUTOFF, 3400.0), (ENV_AMOUNT, 0.6), (KEYBOARD, 6.0), (ATTACK, 380.0), (DECAY, 2000.0), (SUSTAIN, 8.5), (RELEASE, 900.0), (CHORUS, CH2), (LFO_RATE, 5.2), (LFO_DELAY, 5.0), (WHEEL_LFO, 4.0)],
    },
    FactoryPreset {
        name: "Pulse Width Pad",
        values: &[(SAW_ON, 0.0), (PULSE_ON, 1.0), (PWM_MODE, LFO), (PWM, 6.5), (LFO_RATE, 3.8), (CUTOFF, 2100.0), (RESONANCE, 1.5), (ENV_AMOUNT, 1.0), (ATTACK, 650.0), (DECAY, 3000.0), (SUSTAIN, 9.0), (RELEASE, 1400.0), (CHORUS, CH1), (SUB, 2.0)],
    },
    FactoryPreset {
        name: "Round Square Bass",
        values: &[(RANGE, R16), (SAW_ON, 0.0), (PULSE_ON, 1.0), (PWM, 0.0), (SUB, 9.0), (HPF, HP_BASS), (CUTOFF, 340.0), (RESONANCE, 2.0), (ENV_AMOUNT, 3.2), (KEYBOARD, 4.0), (ATTACK, 1.5), (DECAY, 280.0), (SUSTAIN, 2.0), (RELEASE, 60.0), (CHORUS, CH_OFF), (LEVEL, 8.5)],
    },
    FactoryPreset {
        name: "Pumping Saw Bass",
        values: &[(RANGE, R16), (SAW_ON, 1.0), (SUB, 6.0), (HPF, HP_BASS), (CUTOFF, 240.0), (RESONANCE, 3.5), (ENV_AMOUNT, 4.2), (KEYBOARD, 5.0), (DECAY, 200.0), (SUSTAIN, 0.8), (RELEASE, 50.0), (CHORUS, CH_OFF), (LEVEL, 8.5), (ASSIGN, POLY2)],
    },
    FactoryPreset {
        name: "Bright Brass Stab",
        values: &[(SAW_ON, 1.0), (SUB, 3.0), (CUTOFF, 520.0), (RESONANCE, 1.0), (ENV_AMOUNT, 5.0), (KEYBOARD, 7.0), (ATTACK, 28.0), (DECAY, 420.0), (SUSTAIN, 6.0), (RELEASE, 180.0), (CHORUS, CH1), (LFO_RATE, 6.0), (LFO_DELAY, 6.0), (WHEEL_LFO, 3.0)],
    },
    FactoryPreset {
        name: "Glass Pluck",
        values: &[(RANGE, R4), (SAW_ON, 0.0), (PULSE_ON, 1.0), (PWM, 7.5), (HPF, HP2), (CUTOFF, 800.0), (RESONANCE, 5.0), (ENV_AMOUNT, 4.5), (KEYBOARD, 9.0), (DECAY, 380.0), (SUSTAIN, 0.0), (RELEASE, 350.0), (CHORUS, CH2)],
    },
    FactoryPreset {
        name: "Warm Keys",
        values: &[(SAW_ON, 1.0), (PULSE_ON, 1.0), (PWM, 4.0), (CUTOFF, 900.0), (ENV_AMOUNT, 2.6), (KEYBOARD, 8.0), (DECAY, 1100.0), (SUSTAIN, 2.5), (RELEASE, 280.0), (CHORUS, CH1), (VELOCITY, 0.4)],
    },
    FactoryPreset {
        name: "Organ on the Gate",
        values: &[(SAW_ON, 0.0), (PULSE_ON, 1.0), (PWM, 0.0), (SUB, 5.0), (VCA_MODE, GATE), (CUTOFF, 5200.0), (ENV_AMOUNT, 0.0), (KEYBOARD, 10.0), (CHORUS, CH3), (HPF, 1.0), (LEVEL, 6.0)],
    },
    FactoryPreset {
        name: "Fast Chorus Clav",
        values: &[(SAW_ON, 0.0), (PULSE_ON, 1.0), (PWM, 8.5), (HPF, HP3), (CUTOFF, 1000.0), (RESONANCE, 2.5), (ENV_AMOUNT, 3.4), (KEYBOARD, 9.0), (DECAY, 200.0), (SUSTAIN, 0.0), (RELEASE, 90.0), (CHORUS, CH3), (LEVEL, 8.5)],
    },
    FactoryPreset {
        name: "Unison Rave Stack",
        values: &[(ASSIGN, UNISON), (SAW_ON, 1.0), (PULSE_ON, 1.0), (PWM_MODE, LFO), (PWM, 7.0), (LFO_RATE, 4.5), (UNISON_DETUNE, 0.35), (CUTOFF, 1400.0), (RESONANCE, 2.0), (ENV_AMOUNT, 2.5), (DECAY, 600.0), (SUSTAIN, 6.0), (RELEASE, 220.0), (CHORUS, CH2), (LEVEL, 6.0)],
    },
    FactoryPreset {
        name: "Portamento Lead",
        values: &[(ASSIGN, UNISON), (SAW_ON, 1.0), (SUB, 3.0), (UNISON_DETUNE, 0.12), (PORTA_ON, 1.0), (PORTA, 220.0), (CUTOFF, 1800.0), (RESONANCE, 3.0), (ENV_AMOUNT, 1.5), (ATTACK, 8.0), (SUSTAIN, 9.0), (RELEASE, 200.0), (LFO_DELAY, 7.0), (LFO_RATE, 6.2), (DCO_LFO, 0.6), (CHORUS, CH1), (LEVEL, 6.5)],
    },
    FactoryPreset {
        name: "Inverted Sweep",
        values: &[(SAW_ON, 1.0), (ENV_POLARITY, INVERT), (CUTOFF, 6200.0), (RESONANCE, 5.0), (ENV_AMOUNT, 3.0), (ATTACK, 2500.0), (DECAY, 4000.0), (SUSTAIN, 2.5), (RELEASE, 1400.0), (CHORUS, CH2), (HPF, HP2)],
    },
    FactoryPreset {
        name: "Wind Through a Pipe",
        values: &[(SAW_ON, 0.0), (NOISE, 10.0), (CUTOFF, 1100.0), (RESONANCE, 6.0), (LEVEL, 10.0), (VCF_LFO, 2.4), (LFO_RATE, 2.2), (ENV_AMOUNT, 0.0), (KEYBOARD, 10.0), (ATTACK, 1500.0), (SUSTAIN, 10.0), (RELEASE, 2200.0), (CHORUS, CH2)],
    },
    FactoryPreset {
        name: "Whistling Sine",
        values: &[(SAW_ON, 0.0), (RESONANCE, 9.6), (CUTOFF, 261.6), (KEYBOARD, 10.0), (ENV_AMOUNT, 0.0), (ATTACK, 90.0), (SUSTAIN, 10.0), (RELEASE, 350.0), (LFO_DELAY, 6.0), (LFO_RATE, 6.0), (DCO_LFO, 0.0), (VCF_LFO, 0.1), (CHORUS, CH1)],
    },
    FactoryPreset {
        name: "Square Arp",
        values: &[(SAW_ON, 0.0), (PULSE_ON, 1.0), (PWM, 2.0), (HPF, HP2), (CUTOFF, 1100.0), (RESONANCE, 4.0), (ENV_AMOUNT, 3.0), (KEYBOARD, 6.0), (DECAY, 170.0), (SUSTAIN, 0.5), (RELEASE, 120.0), (CHORUS, CH1), (ASSIGN, POLY2)],
    },
    FactoryPreset {
        name: "Slow Choir Haze",
        values: &[(SAW_ON, 1.0), (PULSE_ON, 1.0), (PWM_MODE, LFO), (PWM, 5.0), (LFO_RATE, 2.8), (HPF, HP3), (CUTOFF, 2800.0), (RESONANCE, 2.5), (ENV_AMOUNT, 0.4), (KEYBOARD, 3.0), (ATTACK, 1800.0), (DECAY, 5000.0), (SUSTAIN, 9.5), (RELEASE, 3000.0), (CHORUS, CH2), (LEVEL, 8.0)],
    },
];
