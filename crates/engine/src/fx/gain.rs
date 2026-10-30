//! Gain, after Logic's utility plug-in: level, polarity and channel tools, with no latency.
//!
//! In signal order:
//! 1. **Phase Invert L** and **Phase Invert R** flip each side's polarity.
//! 2. **Swap L/R** exchanges the sides.
//! 3. **Mono:** Off, **L+R** (both sides get the average of the two, so a centred sound keeps
//!    its level), **Left** or **Right** (that side on both).
//! 4. **Balance** turns the opposite side down (to silence at the end of its travel) and never
//!    boosts.
//! 5. **Gain**, −96 to +24 dB.
//!
//! All of it comes down to a 2×2 matrix from the input sides to the output sides. Each entry
//! glides for a few milliseconds when anything changes, so flipping a switch never clicks, and
//! at the defaults the matrix is exactly the identity: the signal passes bit for bit.

use super::lfo::Smoothed;
use super::util::{coeff, db_to_gain};
use super::*;

pub const GAIN: u32 = 0;
pub const INVERT_L: u32 = 1;
pub const INVERT_R: u32 = 2;
pub const SWAP: u32 = 3;
pub const MONO: u32 = 4;
pub const BALANCE: u32 = 5;

pub const MONO_MODES: &[&str] = &["Off", "L+R", "Left", "Right"];
const MONO_SUM: usize = 1;
const MONO_LEFT: usize = 2;
const MONO_RIGHT: usize = 3;

/// How long the matrix glides to new settings.
const GLIDE_MS: f32 = 5.0;

const fn toggle(id: u32, name: &'static str) -> ParamSpec {
    ParamSpec { id, name, min: 0.0, max: 1.0, default: 0.0, unit: Unit::None, kind: ParamKind::Toggle, log: false }
}

static PARAMS: [ParamSpec; 6] = [
    ParamSpec { id: GAIN, name: "Gain", min: -96.0, max: 24.0, default: 0.0, unit: Unit::Db, kind: ParamKind::Continuous, log: false },
    toggle(INVERT_L, "Phase Invert L"),
    toggle(INVERT_R, "Phase Invert R"),
    toggle(SWAP, "Swap L/R"),
    ParamSpec { id: MONO, name: "Mono", min: 0.0, max: 3.0, default: 0.0, unit: Unit::None, kind: ParamKind::Choice(MONO_MODES), log: false },
    ParamSpec { id: BALANCE, name: "Balance", min: -1.0, max: 1.0, default: 0.0, unit: Unit::Percent, kind: ParamKind::Continuous, log: false },
];

/// Mono: 0 Off, 1 L+R, 2 Left, 3 Right.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Unity", values: &[] },
    FactoryPreset { name: "Mono Check", values: &[(MONO, 1.0)] },
    FactoryPreset { name: "Flip Polarity", values: &[(INVERT_L, 1.0), (INVERT_R, 1.0)] },
    FactoryPreset { name: "Left Channel Only", values: &[(MONO, 2.0)] },
    FactoryPreset { name: "Right Channel Only", values: &[(MONO, 3.0)] },
    FactoryPreset { name: "Swap Sides", values: &[(SWAP, 1.0)] },
    FactoryPreset { name: "Side Check (L-R)", values: &[(INVERT_R, 1.0), (MONO, 1.0), (GAIN, 6.0)] },
    FactoryPreset { name: "Trim -6 dB", values: &[(GAIN, -6.0)] },
];

pub fn info() -> EffectInfo {
    EffectInfo { id: "builtin:gain", name: "Gain", category: "Utility", params: &PARAMS, meters: &[], sidechain: false, new: |sr| Box::new(Gain::new(sr)) }
}

/// The left and right gains for a balance of −1 (left) to 1 (right): the far side follows a
/// quarter-cosine down to silence; the near side stays at unity.
pub fn balance_gains(b: f32) -> (f32, f32) {
    let b = b.clamp(-1.0, 1.0);
    let fade = |x: f32| if x <= 0.0 { 1.0 } else { (x * std::f32::consts::FRAC_PI_2).cos().max(0.0) };
    (fade(b), fade(-b))
}

/// The matrix `[[ll, rl], [lr, rr]]`: output left = `ll·L + rl·R`, output right = `lr·L + rr·R`.
pub fn matrix(gain_db: f32, invert: [bool; 2], swap: bool, mono: usize, balance: f32) -> [[f32; 2]; 2] {
    // Rows are output sides, columns input sides; build it up in signal order.
    let s = |on: bool| if on { -1.0 } else { 1.0 };
    let mut m = [[s(invert[0]), 0.0], [0.0, s(invert[1])]];
    if swap {
        m.swap(0, 1);
    }
    let mixed = |a: [f32; 2], b: [f32; 2], wa: f32, wb: f32| [wa * a[0] + wb * b[0], wa * a[1] + wb * b[1]];
    m = match mono {
        MONO_SUM => {
            let avg = mixed(m[0], m[1], 0.5, 0.5);
            [avg, avg]
        }
        MONO_LEFT => [m[0], m[0]],
        MONO_RIGHT => [m[1], m[1]],
        _ => m,
    };
    let (bl, br) = balance_gains(balance);
    let g = db_to_gain(gain_db);
    [[m[0][0] * bl * g, m[0][1] * bl * g], [m[1][0] * br * g, m[1][1] * br * g]]
}

pub struct Gain {
    values: [f32; PARAMS.len()],
    sr: f32,
    m: [[Smoothed; 2]; 2],
    primed: bool,
}

impl Gain {
    pub fn new(sample_rate: f32) -> Gain {
        Gain { values: std::array::from_fn(|i| PARAMS[i].default), sr: sample_rate, m: [[Smoothed::default(); 2]; 2], primed: false }
    }

    fn target(&self) -> [[f32; 2]; 2] {
        let v = |id: u32| self.values[id as usize];
        matrix(v(GAIN), [v(INVERT_L) >= 0.5, v(INVERT_R) >= 0.5], v(SWAP) >= 0.5, v(MONO).round() as usize, v(BALANCE))
    }
}

impl Effect for Gain {
    fn set_param(&mut self, id: u32, value: f32) {
        if let Some(v) = self.values.get_mut(id as usize) {
            *v = value;
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, ctx: &FxContext) {
        if ctx.sample_rate > 0.0 {
            self.sr = ctx.sample_rate;
        }
        let t = self.target();
        if !self.primed {
            self.primed = true;
            for (row, trow) in self.m.iter_mut().zip(t) {
                for (s, v) in row.iter_mut().zip(trow) {
                    s.snap(v);
                }
            }
        }
        let k = coeff(GLIDE_MS, self.sr);
        let settled = self.m.iter().flatten().zip(t.iter().flatten()).all(|(s, &v)| s.get() == v);
        if settled && t == [[1.0, 0.0], [0.0, 1.0]] {
            return;
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let ll = self.m[0][0].next(t[0][0], k);
            let rl = self.m[0][1].next(t[0][1], k);
            let lr = self.m[1][0].next(t[1][0], k);
            let rr = self.m[1][1].next(t[1][1], k);
            let (x, y) = (*a, *b);
            *a = ll * x + rl * y;
            *b = lr * x + rr * y;
        }
    }

    fn reset(&mut self) {
        self.primed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::super::chorus::tests::{check_extremes, check_presets, run, tones, SR};
    use super::*;

    fn gain_with(values: &[(u32, f32)]) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
        let mut g = Gain::new(SR);
        for &(id, v) in values {
            g.set_param(id, v);
        }
        let (l0, r0) = tones(4800);
        let (mut l, mut r) = (l0.clone(), r0.clone());
        run(&mut g, &mut l, &mut r);
        (l, r, l0, r0)
    }

    #[test]
    fn defaults_pass_bit_for_bit() {
        let (l, r, l0, r0) = gain_with(&[]);
        assert_eq!((l, r), (l0, r0));
    }

    #[test]
    fn polarity_swap_and_mono() {
        let (l, r, l0, r0) = gain_with(&[(INVERT_L, 1.0)]);
        assert!(l.iter().zip(&l0).all(|(a, b)| *a == -*b) && r == r0);
        let (l, r, l0, r0) = gain_with(&[(SWAP, 1.0)]);
        assert_eq!((l, r), (r0, l0));
        let (l, r, l0, r0) = gain_with(&[(MONO, 1.0)]);
        assert!(l.iter().zip(&r).all(|(a, b)| a == b));
        assert!(l.iter().zip(l0.iter().zip(&r0)).all(|(y, (a, b))| (y - 0.5 * (a + b)).abs() < 1e-7));
        let (l, r, _, r0) = gain_with(&[(MONO, 3.0)]);
        assert!(l == r0 && r == r0);
        let (l, r, l0, _) = gain_with(&[(MONO, 2.0)]);
        assert!(l == l0 && r == l0);
        // Swap first, then pick a side: Left is what was the right.
        let (l, _, _, r0) = gain_with(&[(SWAP, 1.0), (MONO, 2.0)]);
        assert_eq!(l, r0);
    }

    #[test]
    fn inverting_one_side_then_summing_nulls_a_mono_source() {
        let mut g = Gain::new(SR);
        g.set_param(INVERT_R, 1.0);
        g.set_param(MONO, 1.0);
        let (mut l, _) = tones(4800);
        let mut r = l.clone();
        run(&mut g, &mut l, &mut r);
        assert!(l.iter().chain(&r).all(|x| x.abs() < 1e-7));
    }

    #[test]
    fn gain_and_balance() {
        let (l, _, l0, _) = gain_with(&[(GAIN, -6.0)]);
        let ratio = l[1000] / l0[1000];
        assert!((ratio - db_to_gain(-6.0)).abs() < 1e-6);
        assert_eq!(balance_gains(0.0), (1.0, 1.0));
        let (bl, br) = balance_gains(0.5);
        assert!((bl - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6 && br == 1.0);
        assert!(balance_gains(-1.0).1 < 1e-6 && balance_gains(-1.0).0 == 1.0);
        let (l, r, _, r0) = gain_with(&[(BALANCE, 1.0)]);
        assert!(l.iter().all(|x| x.abs() < 1e-6) && r == r0);
    }

    #[test]
    fn switching_glides() {
        // Flipping the polarity mid-stream ramps through zero instead of jumping.
        let mut g = Gain::new(SR);
        let mut l = vec![0.5; 4096];
        let mut r = l.clone();
        for (i, (bl, br)) in l.chunks_mut(512).zip(r.chunks_mut(512)).enumerate() {
            if i == 2 {
                g.set_param(INVERT_L, 1.0);
            }
            g.process(bl, br, None, &super::super::chorus::tests::CTX);
        }
        let step = l.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        assert!(step < 0.01, "{step}");
        assert!((l[4095] + 0.5).abs() < 1e-4);
    }

    #[test]
    fn extremes_and_presets_are_clean() {
        check_extremes(&info(), 16.0);
        check_presets(&info(), PRESETS);
    }
}
