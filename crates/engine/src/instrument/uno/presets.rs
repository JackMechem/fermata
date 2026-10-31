//! Factory presets of Fermata Uno: written here, knob positions a player might dial in on an
//! instrument with this few knobs. Parameters a preset doesn't list go to their defaults.

use super::params::param::*;
use crate::fx::FactoryPreset;

const SAW: f32 = 0.0;
const SQUARE: f32 = 1.0;

pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Round Bottom", values: &[(WAVE, SAW), (CUTOFF, 280.0), (RESONANCE, 2.0), (ENV_MOD, 2.5), (DECAY, 700.0), (ACCENT, 4.0), (VOLUME, 8.0)] },
    FactoryPreset { name: "Rubbery Pluck", values: &[(WAVE, SAW), (CUTOFF, 380.0), (RESONANCE, 5.5), (ENV_MOD, 5.5), (DECAY, 260.0), (ACCENT, 6.0)] },
    FactoryPreset { name: "Squelch Line", values: &[(WAVE, SAW), (CUTOFF, 420.0), (RESONANCE, 8.5), (ENV_MOD, 7.5), (DECAY, 320.0), (ACCENT, 8.0), (VOLUME, 6.5)] },
    FactoryPreset { name: "Screaming Peak", values: &[(WAVE, SAW), (CUTOFF, 900.0), (RESONANCE, 10.0), (ENV_MOD, 9.0), (DECAY, 400.0), (ACCENT, 10.0), (VOLUME, 5.5)] },
    FactoryPreset { name: "Long Open Sweep", values: &[(WAVE, SAW), (CUTOFF, 300.0), (RESONANCE, 7.0), (ENV_MOD, 8.0), (DECAY, 2000.0), (ACCENT, 5.0)] },
    FactoryPreset { name: "Hollow Square", values: &[(WAVE, SQUARE), (CUTOFF, 520.0), (RESONANCE, 4.0), (ENV_MOD, 4.0), (DECAY, 450.0), (ACCENT, 5.0), (VOLUME, 8.0)] },
    FactoryPreset { name: "Square Chirp", values: &[(WAVE, SQUARE), (CUTOFF, 340.0), (RESONANCE, 8.8), (ENV_MOD, 8.0), (DECAY, 220.0), (ACCENT, 7.0), (VOLUME, 7.0)] },
    FactoryPreset { name: "Dark Square Thump", values: &[(WAVE, SQUARE), (CUTOFF, 170.0), (RESONANCE, 3.0), (ENV_MOD, 3.0), (DECAY, 250.0), (ACCENT, 9.0), (VOLUME, 9.0)] },
    FactoryPreset { name: "Accent Ladder Climb", values: &[(WAVE, SAW), (CUTOFF, 230.0), (RESONANCE, 9.5), (ENV_MOD, 2.5), (DECAY, 500.0), (ACCENT, 10.0), (VOLUME, 6.5)] },
    FactoryPreset { name: "Quiet Tick", values: &[(WAVE, SAW), (CUTOFF, 1400.0), (RESONANCE, 3.0), (ENV_MOD, 0.0), (DECAY, 200.0), (ACCENT, 3.0), (VOLUME, 5.5)] },
    FactoryPreset { name: "Fast Slide Wire", values: &[(WAVE, SAW), (CUTOFF, 600.0), (RESONANCE, 8.0), (ENV_MOD, 5.0), (DECAY, 350.0), (ACCENT, 6.0), (SLIDE_TIME, 25.0), (VOLUME, 6.5)] },
    FactoryPreset { name: "Lazy Slide Bass", values: &[(WAVE, SAW), (CUTOFF, 320.0), (RESONANCE, 4.0), (ENV_MOD, 3.5), (DECAY, 900.0), (ACCENT, 5.0), (SLIDE_TIME, 180.0), (VOLUME, 8.0)] },
    FactoryPreset { name: "Every Note Accented", values: &[(WAVE, SAW), (CUTOFF, 300.0), (RESONANCE, 7.5), (ENV_MOD, 6.0), (DECAY, 800.0), (ACCENT, 7.5), (ACCENT_VELOCITY, 1.0), (VOLUME, 5.5)] },
    FactoryPreset { name: "Low Drone Tune", values: &[(WAVE, SAW), (TUNE, -12.0), (CUTOFF, 240.0), (RESONANCE, 6.0), (ENV_MOD, 4.5), (DECAY, 1400.0), (ACCENT, 6.0), (VOLUME, 8.5)] },
];
