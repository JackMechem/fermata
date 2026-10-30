//! Phaser, after Logic's: a chain of first-order all-pass filters whose frequency an LFO sweeps,
//! mixed with the dry signal. Where the chain shifts the phase by 180° the two cancel, so a set of
//! notches (half as many as there are stages) moves up and down the spectrum.
//!
//! - **Frequency** is the centre of the sweep; **Depth** sweeps up to two octaves either side.
//! - **Stages:** 4, 6, 8 or 12 all-pass filters. Changing it crossfades between the two taps of
//!   one chain, so it doesn't click.
//! - **Feedback** (bipolar) sends the chain's output back to its input, deepening the notches
//!   into resonant peaks; its loop gain stops at [`MAX_FEEDBACK`].
//! - **Spread** offsets the right channel's LFO by up to 180°; **Mix** at 50 % (the default)
//!   gives the deepest notches.

use std::f32::consts::PI;

use super::lfo::{flush, guard, unipolar_sine, Lfo, Smoothed};
use super::util::coeff;
use super::*;

pub const RATE: u32 = 0;
pub const DEPTH: u32 = 1;
pub const FREQUENCY: u32 = 2;
pub const STAGES: u32 = 3;
pub const FEEDBACK: u32 = 4;
pub const SPREAD: u32 = 5;
pub const MIX: u32 = 6;

pub const STAGE_LABELS: &[&str] = &["4", "6", "8", "12"];
const STAGE_COUNTS: [usize; 4] = [4, 6, 8, 12];
const MAX_STAGES: usize = 12;
/// How many octaves Depth at 100 % sweeps either side of Frequency.
pub const SWEEP_OCTAVES: f32 = 2.0;
/// The loop gain at Feedback ±100 %.
pub const MAX_FEEDBACK: f32 = 0.9;
/// The crossfade when the number of stages changes.
const STAGE_FADE_MS: f32 = 30.0;

const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind: ParamKind::Continuous, log }
}

static PARAMS: [ParamSpec; 7] = [
    spec(RATE, "Rate", 0.02, 10.0, 0.5, Unit::Hz, true),
    spec(DEPTH, "Depth", 0.0, 1.0, 0.7, Unit::Percent, false),
    spec(FREQUENCY, "Frequency", 100.0, 4000.0, 700.0, Unit::Hz, true),
    ParamSpec { id: STAGES, name: "Stages", min: 0.0, max: 3.0, default: 0.0, unit: Unit::None, kind: ParamKind::Choice(STAGE_LABELS), log: false },
    spec(FEEDBACK, "Feedback", -1.0, 1.0, 0.3, Unit::Percent, false),
    spec(SPREAD, "Spread", 0.0, 1.0, 0.5, Unit::Percent, false),
    spec(MIX, "Mix", 0.0, 1.0, 0.5, Unit::Percent, false),
];

/// Stages: 0 = 4, 1 = 6, 2 = 8, 3 = 12.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Classic Four-Stage", values: &[(RATE, 0.5), (DEPTH, 0.7), (FREQUENCY, 700.0), (STAGES, 0.0), (FEEDBACK, 0.3), (MIX, 0.5)] },
    FactoryPreset { name: "Slow Swirl", values: &[(RATE, 0.12), (DEPTH, 0.9), (FREQUENCY, 900.0), (STAGES, 1.0), (FEEDBACK, 0.5), (SPREAD, 1.0), (MIX, 0.5)] },
    FactoryPreset { name: "Funky Keys", values: &[(RATE, 1.6), (DEPTH, 0.6), (FREQUENCY, 1200.0), (STAGES, 0.0), (FEEDBACK, 0.6), (SPREAD, 0.25), (MIX, 0.5)] },
    FactoryPreset { name: "Deep Twelve", values: &[(RATE, 0.3), (DEPTH, 0.8), (FREQUENCY, 600.0), (STAGES, 3.0), (FEEDBACK, 0.7), (SPREAD, 0.5), (MIX, 0.5)] },
    FactoryPreset { name: "Resonant Sweep", values: &[(RATE, 0.25), (DEPTH, 1.0), (FREQUENCY, 800.0), (STAGES, 2.0), (FEEDBACK, 0.9), (SPREAD, 0.5), (MIX, 0.5)] },
    FactoryPreset { name: "Hollow", values: &[(RATE, 0.4), (DEPTH, 0.5), (FREQUENCY, 1500.0), (STAGES, 1.0), (FEEDBACK, -0.7), (SPREAD, 0.5), (MIX, 0.5)] },
    FactoryPreset { name: "Subtle Motion", values: &[(RATE, 0.2), (DEPTH, 0.4), (FREQUENCY, 1000.0), (STAGES, 0.0), (FEEDBACK, 0.1), (SPREAD, 1.0), (MIX, 0.3)] },
];

pub fn info() -> EffectInfo {
    EffectInfo { id: "builtin:phaser", name: "Phaser", category: "Modulation", params: &PARAMS, meters: &super::chorus::LFO_METER, sidechain: false, new: |sr| Box::new(Phaser::new(sr)) }
}

/// The swept all-pass frequency at LFO position `u` (0..1).
pub fn swept_hz(center: f32, depth: f32, u: f32) -> f32 {
    center * (SWEEP_OCTAVES * depth * (2.0 * u - 1.0)).exp2()
}

pub struct Phaser {
    values: [f32; PARAMS.len()],
    sr: f32,
    lfo: Lfo,
    /// Each channel's all-pass states, and its last output (fed back).
    state: [[f32; MAX_STAGES]; 2],
    last: [f32; 2],
    /// The number of stages heard (an index into STAGE_COUNTS), and the one fading out.
    tap: usize,
    old_tap: usize,
    /// Progress of the fade from `old_tap` to `tap`; 1 when done.
    fade: f32,
    center_oct: Smoothed,
    depth: Smoothed,
    feedback: Smoothed,
    spread: Smoothed,
    mix: Smoothed,
    primed: bool,
}

impl Phaser {
    pub fn new(sample_rate: f32) -> Phaser {
        Phaser {
            values: std::array::from_fn(|i| PARAMS[i].default),
            sr: sample_rate,
            lfo: Lfo::default(),
            state: [[0.0; MAX_STAGES]; 2],
            last: [0.0; 2],
            tap: 0,
            old_tap: 0,
            fade: 1.0,
            center_oct: Smoothed::default(),
            depth: Smoothed::default(),
            feedback: Smoothed::default(),
            spread: Smoothed::default(),
            mix: Smoothed::default(),
            primed: false,
        }
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    fn stages(&self) -> usize {
        (self.v(STAGES).round() as usize).min(STAGE_COUNTS.len() - 1)
    }

    #[cfg(test)]
    fn has_subnormal(&self) -> bool {
        self.state.iter().flatten().chain(&self.last).any(|x| x.is_subnormal())
    }
}

impl Effect for Phaser {
    fn set_param(&mut self, id: u32, value: f32) {
        if let Some(v) = self.values.get_mut(id as usize) {
            *v = value;
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, ctx: &FxContext) {
        if ctx.sample_rate > 0.0 && (ctx.sample_rate - self.sr).abs() > 0.5 {
            self.sr = ctx.sample_rate;
            self.reset();
        }
        let sr = self.sr;
        let center_t = self.v(FREQUENCY).max(1.0).log2();
        let fb_t = self.v(FEEDBACK) * MAX_FEEDBACK;
        let (depth_t, spread_t, mix_t) = (self.v(DEPTH), self.v(SPREAD), self.v(MIX));
        let want = self.stages();
        if !self.primed {
            self.primed = true;
            self.center_oct.snap(center_t);
            self.depth.snap(depth_t);
            self.feedback.snap(fb_t);
            self.spread.snap(spread_t);
            self.mix.snap(mix_t);
            self.tap = want;
            self.old_tap = want;
            self.fade = 1.0;
        } else if want != self.tap && self.fade >= 1.0 {
            self.old_tap = self.tap;
            self.tap = want;
            self.fade = 0.0;
        }
        let k = coeff(20.0, sr);
        let fade_step = 1.0 / (STAGE_FADE_MS * 0.001 * sr);
        let step = (self.v(RATE) / sr) as f64;
        // Keep the swept frequency below Nyquist, where tan() would blow up.
        let top = 0.45 * sr;
        for n in 0..l.len() {
            let center = self.center_oct.next(center_t, k).exp2();
            let depth = self.depth.next(depth_t, k);
            let fb = self.feedback.next(fb_t, k);
            let spread = self.spread.next(spread_t, k);
            let mix = self.mix.next(mix_t, k);
            if self.fade < 1.0 {
                self.fade = (self.fade + fade_step).min(1.0);
            }
            let (n_new, n_old) = (STAGE_COUNTS[self.tap], STAGE_COUNTS[self.old_tap]);
            let ph = self.lfo.phase();
            let u = [unipolar_sine(ph), unipolar_sine(ph + 0.5 * spread as f64)];
            let x = [l[n], r[n]];
            for c in 0..2 {
                let f = swept_hz(center, depth, u[c]).clamp(10.0, top);
                let t = (PI * f / sr).tan();
                let a = (t - 1.0) / (t + 1.0);
                let mut s = x[c] + fb * self.last[c];
                let (mut out_new, mut out_old) = (0.0, 0.0);
                for (i, z) in self.state[c].iter_mut().enumerate().take(n_new.max(n_old)) {
                    let y = a * s + *z;
                    *z = flush(s - a * y);
                    s = y;
                    if i + 1 == n_new {
                        out_new = s;
                    }
                    if i + 1 == n_old {
                        out_old = s;
                    }
                }
                let wet = out_old + (out_new - out_old) * self.fade;
                self.last[c] = flush(guard(wet));
                let y = x[c] + (wet - x[c]) * mix;
                if c == 0 {
                    l[n] = y;
                } else {
                    r[n] = y;
                }
            }
            self.lfo.advance(step);
        }
        // Stages past the ones in use go stale; clear them so switching to more starts clean.
        if self.fade >= 1.0 {
            let used = STAGE_COUNTS[self.tap];
            for st in &mut self.state {
                st[used..].fill(0.0);
            }
        }
    }

    fn reset(&mut self) {
        self.state = [[0.0; MAX_STAGES]; 2];
        self.last = [0.0; 2];
        self.lfo.set_phase(0.0);
        self.primed = false;
    }

    fn tail_secs(&self) -> f32 {
        // The all-passes ring briefly; feedback lengthens it.
        let g = (self.v(FEEDBACK) * MAX_FEEDBACK).abs();
        0.05 / (1.0 - g)
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        out[0] = self.lfo.phase() as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::super::chorus::tests::{check_extremes, check_presets, run, tones, CTX, SR};
    use super::*;

    /// The level (RMS) of a steady sine at `hz` through a static phaser.
    fn gain_at(stages: f32, hz: f32, center: f32) -> f32 {
        let mut p = Phaser::new(SR);
        p.set_param(DEPTH, 0.0);
        p.set_param(FREQUENCY, center);
        p.set_param(STAGES, stages);
        p.set_param(FEEDBACK, 0.0);
        let len = 24_000;
        let mut l: Vec<f32> = (0..len).map(|i| (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect();
        let mut r = l.clone();
        run(&mut p, &mut l, &mut r);
        let tail = &l[len / 2..];
        (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32 * 2.0).sqrt()
    }

    #[test]
    fn four_stages_notch_at_the_centre_frequency() {
        // Four first-order all-passes reach −180° where each gives −45°... two notches: one
        // below and one above the centre. In between, at the centre itself (−360°), the wet
        // and dry add up.
        assert!(gain_at(0.0, 700.0, 700.0) > 0.99, "{}", gain_at(0.0, 700.0, 700.0));
        // tan(22.5°) and tan(67.5°) times the centre: the notches.
        let lo = 700.0 * (std::f32::consts::PI / 8.0).tan();
        let hi = 700.0 / (std::f32::consts::PI / 8.0).tan();
        assert!(gain_at(0.0, lo, 700.0) < 0.02, "low notch {}", gain_at(0.0, lo, 700.0));
        assert!(gain_at(0.0, hi, 700.0) < 0.02, "high notch {}", gain_at(0.0, hi, 700.0));
        // Far from the notches the level is about unchanged.
        assert!(gain_at(0.0, 30.0, 700.0) > 0.95);
    }

    #[test]
    fn mix_at_zero_is_the_dry_signal() {
        let mut p = Phaser::new(SR);
        p.set_param(MIX, 0.0);
        p.set_param(FEEDBACK, -1.0);
        let (l0, r0) = tones(20_000);
        let (mut l, mut r) = (l0.clone(), r0.clone());
        run(&mut p, &mut l, &mut r);
        assert_eq!((l, r), (l0, r0));
    }

    #[test]
    fn changing_stages_does_not_click() {
        let mut p = Phaser::new(SR);
        p.set_param(DEPTH, 0.0);
        let (mut l, mut r) = tones(48_000);
        let slope = std::f32::consts::TAU * 220.0 * 0.5 / SR;
        let mut last = 0.0;
        let mut worst = 0.0f32;
        for (i, (bl, br)) in l.chunks_mut(512).zip(r.chunks_mut(512)).enumerate() {
            match i {
                20 => p.set_param(STAGES, 3.0),
                50 => p.set_param(STAGES, 1.0),
                _ => {}
            }
            p.process(bl, br, None, &CTX);
            for &y in bl.iter() {
                if i > 2 {
                    worst = worst.max((y - last).abs());
                }
                last = y;
            }
        }
        assert!(worst < 1.5 * slope, "step {worst} vs slope {slope}");
    }

    #[test]
    fn extremes_and_presets_are_clean() {
        // Full feedback resonates: full-scale noise can come out a few dB hotter, never more.
        check_extremes(&info(), 6.0);
        check_presets(&info(), PRESETS);
        let mut p = Phaser::new(SR);
        p.set_param(FEEDBACK, 1.0);
        let mut l = vec![0.0; 96_000];
        let mut r = l.clone();
        l[0] = 1.0;
        run(&mut p, &mut l, &mut r);
        assert!(!p.has_subnormal());
        assert!(l[90_000..].iter().all(|x| x.abs() < 1e-9));
    }
}
