//! Flanger, after Logic's: a very short delay swept by an LFO and mixed with the dry signal,
//! so a comb of notches sweeps up and down the spectrum (the "jet" sound), with feedback to
//! sharpen it.
//!
//! - **Delay** is the shortest delay (0.1 to 5 ms); **Intensity** sweeps it up by as much as
//!   three octaves, so the notches move evenly across the spectrum.
//! - **Feedback** is bipolar: negative values invert the loop, moving the comb's peaks to where
//!   its notches were (a hollower sound). The loop gain stops at [`MAX_FEEDBACK`], so even at
//!   ±100 % the repeats always die away.
//! - **Spread** and **Mix** work as in the Chorus (see [`super::chorus`]).

use super::chorus::{ModDelay, ModSettings, Sweep, LFO_METER};
use super::*;

pub const RATE: u32 = 0;
pub const INTENSITY: u32 = 1;
pub const DELAY: u32 = 2;
pub const FEEDBACK: u32 = 3;
pub const SPREAD: u32 = 4;
pub const MIX: u32 = 5;

/// The loop gain at Feedback ±100 %.
pub const MAX_FEEDBACK: f32 = 0.95;

const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind: ParamKind::Continuous, log }
}

static PARAMS: [ParamSpec; 6] = [
    spec(RATE, "Rate", 0.02, 10.0, 0.25, Unit::Hz, true),
    spec(INTENSITY, "Intensity", 0.0, 1.0, 0.7, Unit::Percent, false),
    spec(DELAY, "Delay", 0.1, 5.0, 1.0, Unit::Ms, true),
    spec(FEEDBACK, "Feedback", -1.0, 1.0, 0.5, Unit::Percent, false),
    spec(SPREAD, "Spread", 0.0, 1.0, 0.5, Unit::Percent, false),
    spec(MIX, "Mix", 0.0, 1.0, 0.5, Unit::Percent, false),
];

pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Classic Flange", values: &[(RATE, 0.25), (INTENSITY, 0.7), (DELAY, 1.0), (FEEDBACK, 0.5), (SPREAD, 0.5), (MIX, 0.5)] },
    FactoryPreset { name: "Jet Plane", values: &[(RATE, 0.12), (INTENSITY, 1.0), (DELAY, 0.4), (FEEDBACK, 0.85), (SPREAD, 0.25), (MIX, 0.5)] },
    FactoryPreset { name: "Gentle Sweep", values: &[(RATE, 0.2), (INTENSITY, 0.6), (DELAY, 1.5), (FEEDBACK, 0.3), (SPREAD, 0.5), (MIX, 0.4)] },
    FactoryPreset { name: "Hollow Metal", values: &[(RATE, 0.4), (INTENSITY, 0.5), (DELAY, 0.3), (FEEDBACK, -0.85), (SPREAD, 0.5), (MIX, 0.5)] },
    FactoryPreset { name: "Drum Bus Swirl", values: &[(RATE, 0.08), (INTENSITY, 0.8), (DELAY, 2.0), (FEEDBACK, 0.45), (SPREAD, 1.0), (MIX, 0.35)] },
    FactoryPreset { name: "Fast Wobble", values: &[(RATE, 4.0), (INTENSITY, 0.4), (DELAY, 1.0), (FEEDBACK, 0.4), (SPREAD, 0.0), (MIX, 0.5)] },
    FactoryPreset { name: "Wide Flange", values: &[(RATE, 0.25), (INTENSITY, 0.7), (DELAY, 1.2), (FEEDBACK, 0.6), (SPREAD, 1.0), (MIX, 0.5)] },
];

pub fn info() -> EffectInfo {
    EffectInfo { id: "builtin:flanger", name: "Flanger", category: "Modulation", params: &PARAMS, meters: &LFO_METER, sidechain: false, new: |sr| Box::new(Flanger::new(sr)) }
}

pub struct Flanger {
    values: [f32; PARAMS.len()],
    core: ModDelay,
}

impl Flanger {
    pub fn new(sample_rate: f32) -> Flanger {
        Flanger { values: std::array::from_fn(|i| PARAMS[i].default), core: ModDelay::new(Sweep::Octaves, sample_rate) }
    }

    fn settings(&self) -> ModSettings {
        let v = |id: u32| self.values[id as usize];
        ModSettings { rate_hz: v(RATE), depth: v(INTENSITY), delay_ms: v(DELAY), feedback: v(FEEDBACK) * MAX_FEEDBACK, spread: v(SPREAD), mix: v(MIX) }
    }
}

impl Effect for Flanger {
    fn set_param(&mut self, id: u32, value: f32) {
        if let Some(v) = self.values.get_mut(id as usize) {
            *v = value;
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, ctx: &FxContext) {
        let s = self.settings();
        self.core.process(l, r, &s, ctx);
    }

    fn reset(&mut self) {
        self.core.reset();
    }

    fn tail_secs(&self) -> f32 {
        self.core.tail_secs(&self.settings())
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        self.core.meters(out);
    }
}

#[cfg(test)]
mod tests {
    use super::super::chorus::tests::{check_extremes, check_presets, run, tones, CTX, SR};
    use super::*;

    #[test]
    fn mix_at_zero_is_the_dry_signal() {
        let mut f = Flanger::new(SR);
        f.set_param(MIX, 0.0);
        f.set_param(FEEDBACK, 1.0);
        let (l0, r0) = tones(20_000);
        let (mut l, mut r) = (l0.clone(), r0.clone());
        run(&mut f, &mut l, &mut r);
        assert_eq!((l, r), (l0, r0));
    }

    #[test]
    fn feedback_repeats_decay_by_the_loop_gain() {
        let mut f = Flanger::new(SR);
        f.set_param(INTENSITY, 0.0);
        f.set_param(DELAY, 1.0);
        f.set_param(FEEDBACK, -0.5);
        f.set_param(MIX, 1.0);
        let mut l = vec![0.0; 4800];
        let mut r = l.clone();
        l[0] = 1.0;
        run(&mut f, &mut l, &mut r);
        let g = -0.5 * MAX_FEEDBACK;
        for k in 1..6 {
            assert!((l[48 * k] - g.powi(k as i32 - 1)).abs() < 1e-5, "repeat {k}: {}", l[48 * k]);
        }
    }

    #[test]
    fn full_feedback_stays_bounded_and_dies_away() {
        for fb in [1.0, -1.0] {
            let mut f = Flanger::new(SR);
            f.set_param(FEEDBACK, fb);
            f.set_param(DELAY, 0.1);
            f.set_param(INTENSITY, 1.0);
            f.set_param(RATE, 10.0);
            f.set_param(MIX, 1.0);
            // Ten seconds of full-scale square wave at the comb's own resonance, then silence.
            let mut peak = 0.0f32;
            let mut late = 0.0f32;
            for block in 0..(12 * 48_000 / 512) {
                let mut l: Vec<f32> = (0..512).map(|i| if block < 940 { if (block * 512 + i) / 5 % 2 == 0 { 1.0 } else { -1.0 } } else { 0.0 }).collect();
                let mut r = l.clone();
                f.process(&mut l, &mut r, None, &CTX);
                for x in l.iter().chain(&r) {
                    assert!(x.is_finite());
                    peak = peak.max(x.abs());
                    if block > 1100 {
                        late = late.max(x.abs());
                    }
                }
            }
            assert!(peak < 3.0, "feedback {fb}: peak {peak}");
            assert!(late < 1e-6, "feedback {fb}: still ringing at {late}");
        }
    }

    #[test]
    fn the_sweep_spans_octaves() {
        use super::super::chorus::swept_ms;
        assert_eq!(swept_ms(Sweep::Octaves, 1.0, 1.0, 1.0), 8.0);
        assert_eq!(swept_ms(Sweep::Octaves, 1.0, 0.0, 1.0), 1.0);
        assert!((swept_ms(Sweep::Octaves, 2.0, 1.0 / 3.0, 1.0) - 4.0).abs() < 1e-5);
    }

    #[test]
    fn extremes_and_presets_are_clean() {
        check_extremes(&info(), 3.0);
        check_presets(&info(), PRESETS);
        let f = Flanger::new(SR);
        // 4.3 ms at the top of the default sweep; a 0.475 loop gain falls 60 dB in 9.3 repeats.
        let t = f.tail_secs();
        assert!(t > 0.04 && t < 0.05, "{t}");
    }
}
