//! Distortion, after Logic's: drive into a waveshaper, then a tone filter.
//!
//! - **Type** picks the curve: **Soft** (tanh, smooth and even), **Hard** (clips flat with a
//!   small knee), **Tube** (asymmetric, so it adds even harmonics) or **Fuzz** (very asymmetric,
//!   clipped hard on one side). Changing it crossfades the two curves.
//! - **Drive** (0 to 60 dB) pushes the signal into the curve. It is level-compensated around
//!   −18 dBFS: a signal at that level comes out at about the same level however hard it is
//!   driven, so Drive changes the character more than the loudness.
//! - **Tone** is a 12 dB/oct low-pass after the curve; **Output** sets the level; **Mix** blends
//!   in the dry signal (kept in time with the distorted one).
//! - The curve runs 4× oversampled (see `oversample.rs`), which delays the output by
//!   [`super::oversample::LATENCY`] samples; the effect reports that for delay compensation.
//!
//! This module also holds [`DriveCore`], which the Overdrive shares.

use super::biquad::{Biquad, BUTTERWORTH_Q};
use super::lfo::{flush, Smoothed};
use super::oversample::{Oversampler, LATENCY};
use super::util::{coeff, db_to_gain};
use super::*;

pub const DRIVE: u32 = 0;
pub const TONE: u32 = 1;
pub const TYPE: u32 = 2;
pub const OUTPUT: u32 = 3;
pub const MIX: u32 = 4;

pub const TYPES: &[&str] = &["Soft", "Hard", "Tube", "Fuzz"];
const HARD: usize = 1;
const TUBE: usize = 2;
const FUZZ: usize = 3;

/// The level Drive is compensated around (−18 dBFS).
pub const COMP_REF: f32 = 0.125_9;
/// The crossfade when the curve changes.
const CURVE_FADE_MS: f32 = 20.0;

const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind: ParamKind::Continuous, log }
}

static PARAMS: [ParamSpec; 5] = [
    spec(DRIVE, "Drive", 0.0, 60.0, 18.0, Unit::Db, false),
    spec(TONE, "Tone", 500.0, 20000.0, 6000.0, Unit::Hz, true),
    ParamSpec { id: TYPE, name: "Type", min: 0.0, max: 3.0, default: 0.0, unit: Unit::None, kind: ParamKind::Choice(TYPES), log: false },
    spec(OUTPUT, "Output", -24.0, 24.0, 0.0, Unit::Db, false),
    spec(MIX, "Mix", 0.0, 1.0, 1.0, Unit::Percent, false),
];

/// Types: 0 Soft, 1 Hard, 2 Tube, 3 Fuzz.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Warm Saturation", values: &[(DRIVE, 8.0), (TONE, 12000.0), (TYPE, 2.0), (OUTPUT, 0.0), (MIX, 1.0)] },
    FactoryPreset { name: "Crunch", values: &[(DRIVE, 22.0), (TONE, 5000.0), (TYPE, 0.0), (OUTPUT, -2.0), (MIX, 1.0)] },
    FactoryPreset { name: "Hard Clip", values: &[(DRIVE, 30.0), (TONE, 8000.0), (TYPE, 1.0), (OUTPUT, -3.0), (MIX, 1.0)] },
    FactoryPreset { name: "Tube Grit", values: &[(DRIVE, 28.0), (TONE, 4500.0), (TYPE, 2.0), (OUTPUT, -2.0), (MIX, 1.0)] },
    FactoryPreset { name: "Fuzz Box", values: &[(DRIVE, 45.0), (TONE, 3500.0), (TYPE, 3.0), (OUTPUT, -4.0), (MIX, 1.0)] },
    FactoryPreset { name: "Parallel Drum Smash", values: &[(DRIVE, 36.0), (TONE, 7000.0), (TYPE, 1.0), (OUTPUT, -6.0), (MIX, 0.35)] },
    FactoryPreset { name: "Bass Grit", values: &[(DRIVE, 20.0), (TONE, 2500.0), (TYPE, 2.0), (OUTPUT, 0.0), (MIX, 0.6)] },
    FactoryPreset { name: "Lo-Fi Vocal", values: &[(DRIVE, 26.0), (TONE, 3000.0), (TYPE, 0.0), (OUTPUT, -3.0), (MIX, 0.8)] },
];

pub fn info() -> EffectInfo {
    EffectInfo { id: "builtin:distortion", name: "Distortion", category: "Distortion", params: &PARAMS, meters: &[], sidechain: false, new: |sr| Box::new(Distortion::new(sr)) }
}

/// The waveshaper for a [`TYPES`] index. Every curve has a slope of 1 at zero (2 for Fuzz) and
/// stays within ±1.
pub fn curve(kind: usize, x: f32) -> f32 {
    match kind {
        HARD => {
            // Straight to ±0.8, then a short knee into ±1.
            let a = x.abs();
            if a <= 0.8 {
                x
            } else {
                x.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh())
            }
        }
        TUBE => {
            // The positive side saturates towards 1, the negative earlier, towards −2/3.
            if x >= 0.0 {
                1.0 - (-x).exp()
            } else {
                ((1.5 * x).exp() - 1.0) / 1.5
            }
        }
        FUZZ => {
            // A squared term lifts the positive half and flattens the negative, then the
            // result clips: strongly asymmetric, rich in even harmonics.
            let s = (2.0 * x).tanh();
            (s + 0.4 * s * s).clamp(-1.0, 1.0)
        }
        _ => x.tanh(),
    }
}

/// The output gain that brings a signal at [`COMP_REF`], driven by `drive` (linear), back to
/// about that level through `shape`.
pub fn compensation(shape: impl Fn(f32) -> f32, drive: f32) -> f32 {
    let x = COMP_REF * drive;
    let out = 0.5 * (shape(x) - shape(-x));
    if out > 1e-9 {
        COMP_REF / out
    } else {
        1.0
    }
}

/// One block's settings for a [`DriveCore`].
pub(crate) struct DriveSettings {
    pub drive_db: f32,
    pub tone_hz: f32,
    pub output_db: f32,
    pub mix: f32,
    /// The curve, an index for the shape function the effect passes in.
    pub curve: usize,
}

/// Drive, a 4× oversampled waveshaper, a DC blocker, a tone low-pass, output gain and a
/// latency-matched dry/wet mix: the parts the Distortion and the Overdrive share.
pub(crate) struct DriveCore {
    sr: f32,
    os: [Oversampler; 2],
    /// An optional first-order high-pass before the drive (Overdrive tightens its low end), and
    /// its state per channel.
    pre_hp_hz: Option<f32>,
    pre: [(f32, f32); 2],
    /// The DC blocker after the curve (the asymmetric curves add DC), as (last in, last out).
    dc: [(f32, f32); 2],
    tone: [Biquad; 2],
    tone_set: f32,
    /// The dry signal, delayed to line up with the oversampled path.
    dry: [[f32; LATENCY]; 2],
    dry_at: usize,
    drive: Smoothed,
    out: Smoothed,
    mix: Smoothed,
    tone_oct: Smoothed,
    /// The curve heard, the one fading out, and the fade's progress (1 when done).
    curve: usize,
    old_curve: usize,
    fade: f32,
    primed: bool,
}

impl DriveCore {
    pub(crate) fn new(sample_rate: f32, pre_hp_hz: Option<f32>) -> DriveCore {
        DriveCore {
            sr: sample_rate,
            os: [Oversampler::new(), Oversampler::new()],
            pre_hp_hz,
            pre: [(0.0, 0.0); 2],
            dc: [(0.0, 0.0); 2],
            tone: [Biquad::default(); 2],
            tone_set: 0.0,
            dry: [[0.0; LATENCY]; 2],
            dry_at: 0,
            drive: Smoothed::default(),
            out: Smoothed::default(),
            mix: Smoothed::default(),
            tone_oct: Smoothed::default(),
            curve: 0,
            old_curve: 0,
            fade: 1.0,
            primed: false,
        }
    }

    pub(crate) fn process(&mut self, l: &mut [f32], r: &mut [f32], s: &DriveSettings, ctx: &FxContext, shape: fn(usize, f32) -> f32) {
        if ctx.sample_rate > 0.0 && (ctx.sample_rate - self.sr).abs() > 0.5 {
            self.sr = ctx.sample_rate;
            self.reset();
        }
        let sr = self.sr;
        let drive_t = db_to_gain(s.drive_db);
        let out_t = db_to_gain(s.output_db) * compensation(|x| shape(s.curve, x), drive_t);
        let tone_t = s.tone_hz.max(20.0).log2();
        if !self.primed {
            self.primed = true;
            self.drive.snap(drive_t);
            self.out.snap(out_t);
            self.mix.snap(s.mix);
            self.tone_oct.snap(tone_t);
            self.curve = s.curve;
            self.old_curve = s.curve;
            self.fade = 1.0;
            self.tone_set = 0.0;
        } else if s.curve != self.curve && self.fade >= 1.0 {
            self.old_curve = self.curve;
            self.curve = s.curve;
            self.fade = 0.0;
        }
        let k = coeff(20.0, sr);
        // Each base-rate sample runs the curve four times; the fade steps once per base sample.
        let fade_step = 1.0 / (CURVE_FADE_MS * 0.001 * sr);
        let pre_k = self.pre_hp_hz.map(|hz| (-std::f32::consts::TAU * hz / sr).exp());
        let dc_r = 1.0 - std::f32::consts::TAU * 8.0 / sr;
        let (cur, old) = (self.curve, self.old_curve);
        for n in 0..l.len() {
            let drive = self.drive.next(drive_t, k);
            let out = self.out.next(out_t, k);
            let mix = self.mix.next(s.mix, k);
            // The tone filter follows its smoothed frequency in 1/16-octave steps.
            let tone = self.tone_oct.next(tone_t, k);
            if (tone - self.tone_set).abs() > 1.0 / 16.0 || (n == 0 && tone != self.tone_set) {
                self.tone_set = tone;
                for b in &mut self.tone {
                    b.lowpass(sr as f64, tone.exp2() as f64, BUTTERWORTH_Q);
                }
            }
            if self.fade < 1.0 {
                self.fade = (self.fade + fade_step).min(1.0);
            }
            let fade = self.fade;
            let x = [l[n], r[n]];
            let mut y = [0.0; 2];
            for c in 0..2 {
                let mut v = x[c];
                if let Some(a) = pre_k {
                    // y[n] = a·(y[n−1] + x[n] − x[n−1]): a one-pole high-pass.
                    let (px, py) = self.pre[c];
                    let hp = flush(a * (py + v - px));
                    self.pre[c] = (v, hp);
                    v = hp;
                }
                v *= drive;
                let shaped = self.os[c].process(v, |u| {
                    let a = shape(cur, u);
                    if fade < 1.0 {
                        let b = shape(old, u);
                        b + (a - b) * fade
                    } else {
                        a
                    }
                });
                let (lx, ly) = self.dc[c];
                let blocked = flush(shaped - lx + dc_r * ly);
                self.dc[c] = (shaped, blocked);
                let wet = self.tone[c].run(blocked) * out;
                let dry = self.dry[c][self.dry_at];
                self.dry[c][self.dry_at] = x[c];
                y[c] = dry + (wet - dry) * mix;
            }
            self.dry_at = (self.dry_at + 1) % LATENCY;
            l[n] = y[0];
            r[n] = y[1];
        }
    }

    pub(crate) fn reset(&mut self) {
        for os in &mut self.os {
            os.reset();
        }
        for b in &mut self.tone {
            b.reset();
        }
        self.pre = [(0.0, 0.0); 2];
        self.dc = [(0.0, 0.0); 2];
        self.dry = [[0.0; LATENCY]; 2];
        self.dry_at = 0;
        self.primed = false;
    }
}

pub struct Distortion {
    values: [f32; PARAMS.len()],
    core: DriveCore,
}

impl Distortion {
    pub fn new(sample_rate: f32) -> Distortion {
        Distortion { values: std::array::from_fn(|i| PARAMS[i].default), core: DriveCore::new(sample_rate, None) }
    }
}

impl Effect for Distortion {
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
        let s = DriveSettings { drive_db: v(DRIVE), tone_hz: v(TONE), output_db: v(OUTPUT), mix: v(MIX), curve: (v(TYPE).round() as usize).min(FUZZ) };
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
pub(crate) mod tests {
    use super::super::chorus::tests::{check_extremes, check_presets, run, tones, SR};
    use super::*;

    #[test]
    fn curves_are_bounded_and_unity_at_small_levels() {
        for kind in 0..TYPES.len() {
            let slope = (curve(kind, 1e-4) - curve(kind, -1e-4)) / 2e-4;
            let want = if kind == FUZZ { 2.0 } else { 1.0 };
            assert!((slope - want).abs() < 1e-2, "{}: slope {slope}", TYPES[kind]);
            for i in -1000..=1000 {
                let y = curve(kind, i as f32 * 0.1);
                assert!(y.is_finite() && y.abs() <= 1.0, "{}: {y}", TYPES[kind]);
            }
            assert_eq!(curve(kind, 0.0), 0.0);
        }
        // Tube and Fuzz are asymmetric, Soft and Hard aren't.
        assert!((curve(TUBE, 2.0) + curve(TUBE, -2.0)).abs() > 0.1);
        assert!((curve(FUZZ, 0.5) + curve(FUZZ, -0.5)).abs() > 0.1);
        assert_eq!(curve(0, 0.7), -curve(0, -0.7));
        assert_eq!(curve(HARD, 0.5), 0.5);
    }

    #[test]
    fn compensation_holds_the_reference_level() {
        for kind in 0..TYPES.len() {
            for db in [0.0, 12.0, 30.0, 60.0] {
                let g = db_to_gain(db);
                let c = compensation(|x| curve(kind, x), g);
                let level = 0.5 * (curve(kind, COMP_REF * g) - curve(kind, -COMP_REF * g)) * c;
                assert!((level - COMP_REF).abs() < 1e-4, "{} at {db} dB: {level}", TYPES[kind]);
            }
        }
    }

    #[test]
    fn mix_at_zero_is_the_dry_signal_in_time() {
        let mut d = Distortion::new(SR);
        d.set_param(MIX, 0.0);
        d.set_param(DRIVE, 60.0);
        let (l0, r0) = tones(20_000);
        let (mut l, mut r) = (l0.clone(), r0.clone());
        run(&mut d, &mut l, &mut r);
        assert_eq!(d.latency(), LATENCY);
        assert!(l[..LATENCY].iter().all(|&x| x == 0.0));
        assert_eq!(&l[LATENCY..], &l0[..l0.len() - LATENCY]);
        assert_eq!(&r[LATENCY..], &r0[..r0.len() - LATENCY]);
    }

    #[test]
    fn wet_lines_up_with_dry() {
        // With no drive and the tone open, the wet path is a near-copy of the input, delayed by
        // the reported latency (the tone filter and DC blocker add well under a sample).
        let mut d = Distortion::new(SR);
        d.set_param(DRIVE, 0.0);
        d.set_param(TONE, 20000.0);
        d.set_param(TYPE, HARD as f32);
        // Tones high enough that the DC blocker's phase lead doesn't count.
        let tone = |f: f32| (0..20_000).map(|i| 0.4 * (std::f32::consts::TAU * f * i as f32 / SR).sin()).collect::<Vec<f32>>();
        let (l0, r0) = (tone(2000.0), tone(3100.0));
        let (mut l, mut r) = (l0.clone(), r0.clone());
        run(&mut d, &mut l, &mut r);
        let err = |lag: usize| (4800..l.len()).map(|i| (l[i] - l0[i - lag]).abs().max((r[i] - r0[i - lag]).abs())).fold(0.0f32, f32::max);
        let best = (LATENCY - 4..LATENCY + 4).min_by(|&a, &b| err(a).total_cmp(&err(b))).unwrap();
        assert!(best == LATENCY || best == LATENCY + 1, "best lag {best}");
        assert!(err(LATENCY) < 0.05, "{}", err(LATENCY));
    }

    #[test]
    fn extremes_and_presets_are_clean() {
        check_extremes(&info(), 64.0);
        check_presets(&info(), PRESETS);
    }
}
