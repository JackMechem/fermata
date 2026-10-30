//! Overdrive, after Logic's: a pedal-style soft clipper with three controls.
//!
//! - **Drive** (0 to 40 dB) into an asymmetric tanh curve, which adds a little even-harmonic
//!   warmth. A gentle high-pass before it keeps the low end tight instead of farting out.
//! - **Tone** is a low-pass after the curve; **Output** sets the level. Drive is
//!   level-compensated as in the Distortion.
//! - The curve runs 4× oversampled; the latency is reported, as for the Distortion.

use super::distortion::{DriveCore, DriveSettings};
use super::oversample::LATENCY;
use super::*;

pub const DRIVE: u32 = 0;
pub const TONE: u32 = 1;
pub const OUTPUT: u32 = 2;

/// The curve's offset: how asymmetric it is.
const BIAS: f32 = 0.3;
/// The high-pass before the drive.
const TIGHTEN_HZ: f32 = 120.0;

const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind: ParamKind::Continuous, log }
}

static PARAMS: [ParamSpec; 3] = [spec(DRIVE, "Drive", 0.0, 40.0, 12.0, Unit::Db, false), spec(TONE, "Tone", 500.0, 20000.0, 5000.0, Unit::Hz, true), spec(OUTPUT, "Output", -24.0, 12.0, 0.0, Unit::Db, false)];

pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Edge of Breakup", values: &[(DRIVE, 6.0), (TONE, 7000.0), (OUTPUT, 0.0)] },
    FactoryPreset { name: "Classic Overdrive", values: &[(DRIVE, 14.0), (TONE, 4500.0), (OUTPUT, 0.0)] },
    FactoryPreset { name: "Warm Bass", values: &[(DRIVE, 10.0), (TONE, 2200.0), (OUTPUT, 0.0)] },
    FactoryPreset { name: "Bright Crunch", values: &[(DRIVE, 22.0), (TONE, 9000.0), (OUTPUT, -2.0)] },
    FactoryPreset { name: "Thick Lead", values: &[(DRIVE, 32.0), (TONE, 3500.0), (OUTPUT, -3.0)] },
    FactoryPreset { name: "Vocal Grit", values: &[(DRIVE, 8.0), (TONE, 6000.0), (OUTPUT, 0.0)] },
];

pub fn info() -> EffectInfo {
    EffectInfo { id: "builtin:overdrive", name: "Overdrive", category: "Distortion", params: &PARAMS, meters: &[], sidechain: false, new: |sr| Box::new(Overdrive::new(sr)) }
}

/// The Overdrive's curve: tanh, offset so it clips a little earlier on one side. Zero in gives
/// zero out, and it stays within ±1.3.
pub fn curve(_kind: usize, x: f32) -> f32 {
    (x + BIAS).tanh() - BIAS.tanh()
}

pub struct Overdrive {
    values: [f32; PARAMS.len()],
    core: DriveCore,
}

impl Overdrive {
    pub fn new(sample_rate: f32) -> Overdrive {
        Overdrive { values: std::array::from_fn(|i| PARAMS[i].default), core: DriveCore::new(sample_rate, Some(TIGHTEN_HZ)) }
    }
}

impl Effect for Overdrive {
    fn set_param(&mut self, id: u32, value: f32) {
        if let Some(v) = self.values.get_mut(id as usize) {
            *v = value;
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, ctx: &FxContext) {
        let v = |id: u32| self.values[id as usize];
        let s = DriveSettings { drive_db: v(DRIVE), tone_hz: v(TONE), output_db: v(OUTPUT), mix: 1.0, curve: 0 };
        self.core.process(l, r, &s, ctx, curve);
    }

    fn reset(&mut self) {
        self.core.reset();
    }

    fn latency(&self) -> usize {
        LATENCY
    }

    fn tail_secs(&self) -> f32 {
        0.02
    }
}

#[cfg(test)]
mod tests {
    use super::super::chorus::tests::{check_extremes, check_presets, run, SR};
    use super::super::distortion::{compensation, COMP_REF};
    use super::super::util::db_to_gain;
    use super::*;

    #[test]
    fn the_curve_is_asymmetric_and_passes_zero() {
        assert_eq!(curve(0, 0.0), 0.0);
        assert!(curve(0, 5.0) + curve(0, -5.0) < -0.2);
        assert!(curve(0, 1e9).abs() < 1.3 && curve(0, -1e9).abs() < 1.3);
    }

    #[test]
    fn drive_is_level_compensated() {
        // A 1 kHz sine at −18 dBFS keeps about its level at any drive.
        for db in [0.0, 12.0, 24.0, 40.0] {
            let mut o = Overdrive::new(SR);
            o.set_param(DRIVE, db);
            o.set_param(TONE, 20000.0);
            let mut l: Vec<f32> = (0..24_000).map(|i| COMP_REF * (std::f32::consts::TAU * 1000.0 * i as f32 / SR).sin()).collect();
            let mut r = l.clone();
            run(&mut o, &mut l, &mut r);
            let peak = l[12_000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!((peak / COMP_REF).log10().abs() * 20.0 < 2.5, "{db} dB drive: peak {peak}");
        }
        let c = compensation(|x| curve(0, x), db_to_gain(40.0));
        assert!(c < 0.2 && c > 0.05, "{c}");
    }

    #[test]
    fn extremes_and_presets_are_clean() {
        check_extremes(&info(), 16.0);
        check_presets(&info(), PRESETS);
        assert_eq!(Overdrive::new(SR).latency(), LATENCY);
    }
}
