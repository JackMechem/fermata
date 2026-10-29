//! Loudness Meter: EBU R128 / ITU-R BS.1770-4 loudness, loudness range and true peak, after
//! Logic's Loudness Meter. The audio passes through untouched.
//!
//! - **K-weighting:** the two BS.1770 stages (a high shelf and a high-pass), derived from their
//!   analog parameters for the actual sample rate. At 48 kHz they reproduce the standard's
//!   coefficient table.
//! - **Windows:** the K-weighted power is summed in 100 ms sub-blocks. Momentary loudness is
//!   the last 4 (400 ms), short-term the last 30 (3 s); both update every 100 ms.
//! - **Integrated:** gating blocks of 400 ms every 100 ms, with the -70 LUFS absolute gate and the
//!   -10 LU relative gate. The blocks go into a histogram of 0.1 LU bins over -70..+5 LUFS that
//!   keeps each bin's count and summed power, so the gated mean is exact (only blocks in the bin
//!   holding the relative gate are rounded) and nothing grows with the programme length.
//! - **Loudness range:** EBU Tech 3342, from a second histogram of short-term values: the -70
//!   absolute and -20 LU relative gates, then the 10th to 95th percentile spread.
//! - **True peak:** 4x oversampling with a windowed-sinc interpolator, maximum since reset.
//!
//! The integrated and range measurements only run while the transport plays, so the time since
//! reset or play start is what they cover; momentary, short-term and the peaks always run.

use super::{EffectInfo, Effect, FactoryPreset, FxContext, Key, ParamKind, ParamSpec, Unit, MAX_METERS};
use std::f64::consts::PI;

pub const TARGET: u32 = 0;
pub const CUSTOM_TARGET: u32 = 1;
pub const SCALE: u32 = 2;
pub const RESET: u32 = 3;
pub const AUTO_RESET: u32 = 4;

/// The loudness targets offered by the Target choice, in LUFS (the last entry is Custom).
pub const TARGETS: [f32; 4] = [-23.0, -16.0, -14.0, -24.0];

static PARAMS: [ParamSpec; 5] = [
    ParamSpec {
        id: TARGET,
        name: "Target",
        min: 0.0,
        max: 4.0,
        default: 0.0,
        unit: Unit::None,
        kind: ParamKind::Choice(&[
            "−23 LUFS (EBU R128)",
            "−16 LUFS (Apple Music)",
            "−14 LUFS (Spotify, YouTube)",
            "−24 LUFS (ATSC A/85)",
            "Custom",
        ]),
        log: false,
    },
    ParamSpec {
        id: CUSTOM_TARGET,
        name: "Custom Target",
        min: -36.0,
        max: -6.0,
        default: -23.0,
        unit: Unit::Db,
        kind: ParamKind::Continuous,
        log: false,
    },
    ParamSpec {
        id: SCALE,
        name: "Scale",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: Unit::None,
        kind: ParamKind::Choice(&["EBU +9", "EBU +18"]),
        log: false,
    },
    ParamSpec {
        id: RESET,
        name: "Reset",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: Unit::None,
        kind: ParamKind::Toggle,
        log: false,
    },
    ParamSpec {
        id: AUTO_RESET,
        name: "Auto-Reset on Play",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: Unit::None,
        kind: ParamKind::Toggle,
        log: false,
    },
];

/// Meter order: momentary, short-term and integrated loudness (LUFS), loudness range (LU), true
/// peak since reset (dBTP), and the current sample peaks of the left and right channels (dBFS).
/// Loudness and peaks read [`FLOOR`] when there is nothing to measure.
static METERS: [&str; 7] = ["momentary", "short_term", "integrated", "range", "true_peak", "peak_l", "peak_r"];

/// The Target choice index for Custom.
const CUSTOM: f32 = TARGETS.len() as f32;

/// Factory presets, one per delivery target. Each lists only what it changes from the defaults
/// (EBU R128 target, EBU +9 scale, Auto-Reset on).
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "EBU R128 (−23 LUFS)", values: &[(TARGET, 0.0)] },
    FactoryPreset { name: "Spotify / YouTube (−14)", values: &[(TARGET, 2.0), (SCALE, 1.0)] },
    FactoryPreset { name: "Apple Music (−16)", values: &[(TARGET, 1.0), (SCALE, 1.0)] },
    FactoryPreset { name: "ATSC A/85 (−24)", values: &[(TARGET, 3.0)] },
    FactoryPreset { name: "Podcast (−16)", values: &[(TARGET, CUSTOM), (CUSTOM_TARGET, -16.0), (SCALE, 1.0)] },
    FactoryPreset { name: "CD Master (−9)", values: &[(TARGET, CUSTOM), (CUSTOM_TARGET, -9.0), (SCALE, 1.0)] },
];

pub fn info() -> EffectInfo {
    EffectInfo {
        id: "builtin:loudness",
        name: "Loudness Meter",
        category: "Metering",
        params: &PARAMS,
        meters: &METERS,
        sidechain: false,
        new: |sr| Box::new(Loudness::new(sr)),
    }
}

/// What the meters read for silence or no measurement yet.
pub const FLOOR: f32 = -120.0;

const ABS_GATE: f64 = -70.0;
const HIST_MIN: f64 = ABS_GATE;
const HIST_STEP: f64 = 0.1;
/// -70..+5 LUFS in 0.1 LU bins; anything louder lands in the top bin.
const HIST_BINS: usize = 750;
const MOMENTARY_SUBS: usize = 4;
const SHORT_TERM_SUBS: usize = 30;
/// Taps per phase of the true-peak interpolator.
const TP_TAPS: usize = 16;
const TP_PHASES: usize = 4;
/// The sample peak meters fall at this rate (dB per second).
const PEAK_FALL_DB: f32 = 15.0;

/// BS.1770 loudness of a summed channel power.
fn lufs(power: f64) -> f64 {
    if power > 0.0 {
        -0.691 + 10.0 * power.log10()
    } else {
        f64::NEG_INFINITY
    }
}

fn meter(v: f64) -> f32 {
    (v as f32).max(FLOOR)
}

/// A biquad in transposed direct form II, run in f64.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn clear(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// The K-weighting stages for a sample rate: the high shelf that models the head, then the
/// RLB high-pass. The analog parameters are those that reproduce the BS.1770-4 48 kHz table
/// through the bilinear transform (as libebur128 derives them).
fn k_weighting(sr: f64) -> [Biquad; 2] {
    let (f0, gain_db, q) = (1681.974_450_955_533, 3.999_843_853_973_347, 0.707_175_236_955_419_6);
    let k = (PI * f0 / sr).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
        ..Biquad::default()
    };
    let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
    let k = (PI * f0 / sr).tan();
    let a0 = 1.0 + k / q + k * k;
    let highpass = Biquad {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
        ..Biquad::default()
    };
    [shelf, highpass]
}

/// Loudness values binned by 0.1 LU, with each bin's summed power for exact power averages.
struct Histogram {
    count: [u32; HIST_BINS],
    power: [f64; HIST_BINS],
}

impl Histogram {
    fn new() -> Histogram {
        Histogram { count: [0; HIST_BINS], power: [0.0; HIST_BINS] }
    }

    fn clear(&mut self) {
        self.count.fill(0);
        self.power.fill(0.0);
    }

    fn bin(loudness: f64) -> usize {
        (((loudness - HIST_MIN) / HIST_STEP).floor().max(0.0) as usize).min(HIST_BINS - 1)
    }

    /// Adds a block's power, applying the absolute gate.
    fn add(&mut self, power: f64) {
        let l = lufs(power);
        if l > ABS_GATE {
            let i = Histogram::bin(l);
            self.count[i] += 1;
            self.power[i] += power;
        }
    }

    /// The first bin at or above the relative gate `offset` LU below the power average of
    /// everything stored, with the number of values at and above it.
    fn relative_gate(&self, offset: f64) -> Option<(usize, u64)> {
        let n: u64 = self.count.iter().map(|&c| c as u64).sum();
        if n == 0 {
            return None;
        }
        let total: f64 = self.power.iter().sum();
        let start = Histogram::bin(lufs(total / n as f64) - offset);
        let kept: u64 = self.count[start..].iter().map(|&c| c as u64).sum();
        (kept > 0).then_some((start, kept))
    }

    /// BS.1770 integrated loudness of the stored gating blocks.
    fn integrated(&self) -> Option<f64> {
        let (start, n) = self.relative_gate(10.0)?;
        let power: f64 = self.power[start..].iter().sum();
        Some(lufs(power / n as f64))
    }

    /// EBU Tech 3342 loudness range of the stored short-term values.
    fn range(&self) -> f64 {
        let Some((start, n)) = self.relative_gate(20.0) else { return 0.0 };
        let percentile = |p: f64| {
            let rank = ((n - 1) as f64 * p).round() as u64;
            let mut seen = 0u64;
            for i in start..HIST_BINS {
                seen += self.count[i] as u64;
                if seen > rank {
                    return HIST_MIN + (i as f64 + 0.5) * HIST_STEP;
                }
            }
            HIST_MIN + HIST_BINS as f64 * HIST_STEP
        };
        percentile(0.95) - percentile(0.10)
    }
}

/// One channel's true-peak detector: the latest samples, doubled so the newest `TP_TAPS` are
/// always one contiguous slice.
struct TruePeak {
    hist: [f32; 2 * TP_TAPS],
    pos: usize,
}

impl TruePeak {
    fn new() -> TruePeak {
        TruePeak { hist: [0.0; 2 * TP_TAPS], pos: 0 }
    }

    /// Takes one sample and returns the largest magnitude among it and the interpolated points
    /// between the samples around the interpolator's centre.
    #[inline]
    fn push(&mut self, x: f32, taps: &[[f32; TP_TAPS]; TP_PHASES]) -> f32 {
        self.hist[self.pos] = x;
        self.hist[self.pos + TP_TAPS] = x;
        self.pos = (self.pos + 1) % TP_TAPS;
        let window = &self.hist[self.pos..self.pos + TP_TAPS];
        let mut peak = x.abs();
        for phase in &taps[1..] {
            let y: f32 = window.iter().zip(phase).map(|(a, b)| a * b).sum();
            peak = peak.max(y.abs());
        }
        peak
    }
}

/// Kaiser-windowed sinc phases for interpolating at quarter-sample offsets. Phase p estimates
/// the signal p/4 of a sample after the window's centre sample; phase 0 is that sample itself.
fn true_peak_taps() -> [[f32; TP_TAPS]; TP_PHASES] {
    fn bessel_i0(x: f64) -> f64 {
        let (mut sum, mut term) = (1.0, 1.0);
        for k in 1..30 {
            term *= (x / (2.0 * k as f64)).powi(2);
            sum += term;
        }
        sum
    }
    let beta = 7.0;
    let half = TP_TAPS as f64 / 2.0;
    let centre = half - 1.0;
    let mut taps = [[0.0f32; TP_TAPS]; TP_PHASES];
    for (p, phase) in taps.iter_mut().enumerate() {
        let t = centre + p as f64 / TP_PHASES as f64;
        let mut h = [0.0f64; TP_TAPS];
        for (j, v) in h.iter_mut().enumerate() {
            let u = t - j as f64;
            let sinc = if u.abs() < 1e-12 { 1.0 } else { (PI * u).sin() / (PI * u) };
            let w = (1.0 - (u / half).powi(2)).max(0.0).sqrt();
            *v = sinc * bessel_i0(beta * w) / bessel_i0(beta);
        }
        let sum: f64 = h.iter().sum();
        for (o, v) in phase.iter_mut().zip(h) {
            *o = (v / sum) as f32;
        }
    }
    taps
}

pub struct Loudness {
    target: f32,
    custom_target: f32,
    scale: f32,
    reset_held: bool,
    auto_reset: bool,

    k: [[Biquad; 2]; 2],
    tp_taps: [[f32; TP_TAPS]; TP_PHASES],
    tp: [TruePeak; 2],
    sub_len: usize,
    sub_n: usize,
    sub_power: f64,
    /// The summed K-weighted power of the latest sub-blocks (a ring, newest at `sub_pos - 1`).
    subs: [f64; SHORT_TERM_SUBS],
    sub_pos: usize,
    sub_count: usize,
    /// Consecutive sub-blocks measured while playing since the last measurement reset.
    run: usize,
    was_playing: bool,
    blocks: Box<Histogram>,
    short_terms: Box<Histogram>,

    momentary: f32,
    short_term: f32,
    integrated: f32,
    range: f32,
    true_peak: f32,
    peak: [f32; 2],
    sr: f32,
}

impl Loudness {
    pub fn new(sample_rate: f32) -> Loudness {
        let k = k_weighting(sample_rate as f64);
        Loudness {
            target: 0.0,
            custom_target: -23.0,
            scale: 0.0,
            reset_held: false,
            auto_reset: true,
            k: [k, k],
            tp_taps: true_peak_taps(),
            tp: [TruePeak::new(), TruePeak::new()],
            sub_len: (sample_rate as f64 * 0.1).round().max(1.0) as usize,
            sub_n: 0,
            sub_power: 0.0,
            subs: [0.0; SHORT_TERM_SUBS],
            sub_pos: 0,
            sub_count: 0,
            run: 0,
            was_playing: false,
            blocks: Box::new(Histogram::new()),
            short_terms: Box::new(Histogram::new()),
            momentary: FLOOR,
            short_term: FLOOR,
            integrated: FLOOR,
            range: 0.0,
            true_peak: 0.0,
            peak: [0.0; 2],
            sr: sample_rate,
        }
    }

    /// Starts a new integrated / range / true-peak measurement.
    fn clear_measurement(&mut self) {
        self.blocks.clear();
        self.short_terms.clear();
        self.integrated = FLOOR;
        self.range = 0.0;
        self.true_peak = 0.0;
        self.run = 0;
        // The first gating block starts here, not part-way through a sub-block.
        self.sub_n = 0;
        self.sub_power = 0.0;
    }

    /// The mean power of the latest `n` sub-blocks (fewer right after a reset).
    fn window(&self, n: usize) -> f64 {
        let n = n.min(self.sub_count);
        if n == 0 {
            return 0.0;
        }
        let sum: f64 = (1..=n).map(|i| self.subs[(self.sub_pos + SHORT_TERM_SUBS - i) % SHORT_TERM_SUBS]).sum();
        sum / (n * self.sub_len) as f64
    }

    fn end_sub_block(&mut self, playing: bool) {
        self.subs[self.sub_pos] = self.sub_power;
        self.sub_pos = (self.sub_pos + 1) % SHORT_TERM_SUBS;
        self.sub_count = (self.sub_count + 1).min(SHORT_TERM_SUBS);
        self.sub_power = 0.0;
        self.sub_n = 0;
        let (m, s) = (self.window(MOMENTARY_SUBS), self.window(SHORT_TERM_SUBS));
        self.momentary = meter(lufs(m));
        self.short_term = meter(lufs(s));
        if !playing {
            self.run = 0;
            return;
        }
        self.run += 1;
        if self.run >= MOMENTARY_SUBS {
            self.blocks.add(m);
            self.integrated = self.blocks.integrated().map_or(FLOOR, meter);
        }
        if self.run >= SHORT_TERM_SUBS {
            self.short_terms.add(s);
            self.range = self.short_terms.range() as f32;
        }
    }

    /// The target in LUFS.
    pub fn target_lufs(&self) -> f32 {
        TARGETS.get(self.target as usize).copied().unwrap_or(self.custom_target)
    }
}

impl Effect for Loudness {
    fn set_param(&mut self, id: u32, value: f32) {
        match id {
            TARGET => self.target = value.round(),
            CUSTOM_TARGET => self.custom_target = value,
            SCALE => self.scale = value.round(),
            RESET => {
                // The panel pulses this; only the rising edge resets.
                let on = value >= 0.5;
                if on && !self.reset_held {
                    self.clear_measurement();
                }
                self.reset_held = on;
            }
            AUTO_RESET => self.auto_reset = value >= 0.5,
            _ => {}
        }
    }

    fn param(&self, id: u32) -> f32 {
        match id {
            TARGET => self.target,
            CUSTOM_TARGET => self.custom_target,
            SCALE => self.scale,
            RESET => self.reset_held as u8 as f32,
            AUTO_RESET => self.auto_reset as u8 as f32,
            _ => 0.0,
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, ctx: &FxContext) {
        if ctx.playing && !self.was_playing && self.auto_reset {
            self.clear_measurement();
        }
        self.was_playing = ctx.playing;

        let mut block_peak = [0.0f32; 2];
        let mut block_tp = 0.0f32;
        for (&a, &b) in l.iter().zip(r.iter()) {
            let [[shelf_l, hp_l], [shelf_r, hp_r]] = &mut self.k;
            let yl = hp_l.run(shelf_l.run(a as f64));
            let yr = hp_r.run(shelf_r.run(b as f64));
            self.sub_power += yl * yl + yr * yr;
            block_peak[0] = block_peak[0].max(a.abs());
            block_peak[1] = block_peak[1].max(b.abs());
            block_tp = block_tp.max(self.tp[0].push(a, &self.tp_taps)).max(self.tp[1].push(b, &self.tp_taps));
            self.sub_n += 1;
            if self.sub_n == self.sub_len {
                self.end_sub_block(ctx.playing);
            }
        }
        self.true_peak = self.true_peak.max(block_tp);
        let fall = 10f32.powf(-PEAK_FALL_DB * l.len() as f32 / self.sr / 20.0);
        for (p, b) in self.peak.iter_mut().zip(block_peak) {
            *p = (*p * fall).max(b);
        }
        // Keep denormals out of the filters after the input goes silent.
        for ch in &mut self.k {
            for f in ch {
                if f.z1.abs() < 1e-30 && f.z2.abs() < 1e-30 {
                    f.clear();
                }
            }
        }
    }

    /// Clears the filters and the sliding windows. The integrated, range and true-peak
    /// measurement survives (the Reset command or play start clears it), so a host reset on
    /// stop or locate doesn't throw away a whole pass's reading.
    fn reset(&mut self) {
        for ch in &mut self.k {
            for f in ch {
                f.clear();
            }
        }
        self.tp = [TruePeak::new(), TruePeak::new()];
        self.sub_n = 0;
        self.sub_power = 0.0;
        self.subs = [0.0; SHORT_TERM_SUBS];
        self.sub_count = 0;
        self.run = 0;
        self.momentary = FLOOR;
        self.short_term = FLOOR;
        self.peak = [0.0; 2];
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        let db = |g: f32| if g > 0.0 { (20.0 * g.log10()).max(FLOOR) } else { FLOOR };
        out[0] = self.momentary;
        out[1] = self.short_term;
        out[2] = self.integrated;
        out[3] = self.range;
        out[4] = db(self.true_peak);
        out[5] = db(self.peak[0]);
        out[6] = db(self.peak[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const M: usize = 0;
    const S: usize = 1;
    const I: usize = 2;
    const LRA: usize = 3;
    const TP: usize = 4;
    const PEAK_L: usize = 5;

    /// Plays stereo sine segments of (dBFS peak level, seconds) through a fresh meter, with
    /// continuous phase, and returns the final meters.
    fn play(sr: f32, freq: f64, segments: &[(f64, f64)]) -> [f32; MAX_METERS] {
        let mut fx = Loudness::new(sr);
        play_on(&mut fx, sr, freq, segments)
    }

    fn play_on(fx: &mut Loudness, sr: f32, freq: f64, segments: &[(f64, f64)]) -> [f32; MAX_METERS] {
        let ctx = FxContext { sample_rate: sr, tempo: 120.0, playing: true };
        let mut n = 0usize;
        let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
        for &(db, secs) in segments {
            let amp = 10f64.powf(db / 20.0);
            let total = (secs * sr as f64).round() as usize;
            let mut done = 0;
            while done < total {
                let len = 1024.min(total - done);
                for i in 0..len {
                    let v = (amp * (TAU * freq * (n + i) as f64 / sr as f64).sin()) as f32;
                    l[i] = v;
                    r[i] = v;
                }
                fx.process(&mut l[..len], &mut r[..len], None, &ctx);
                n += len;
                done += len;
            }
        }
        let mut m = [0.0; MAX_METERS];
        fx.meters(&mut m);
        m
    }

    #[test]
    fn presets_use_real_params_in_range() {
        let info = info();
        for p in PRESETS {
            for &(id, v) in p.values {
                let spec = info.param(id).unwrap_or_else(|| panic!("{}: no parameter {id}", p.name));
                assert!(v >= spec.min && v <= spec.max, "{}: {} = {v} is out of range", p.name, spec.name);
            }
        }
        // Each preset's target is the one its name promises.
        for (name, want) in [("EBU", -23.0), ("Spotify", -14.0), ("Apple", -16.0), ("ATSC", -24.0), ("Podcast", -16.0), ("CD", -9.0)] {
            let p = PRESETS.iter().find(|p| p.name.starts_with(name)).unwrap();
            let mut fx = Loudness::new(48000.0);
            for &(id, v) in p.values {
                fx.set_param(id, v);
            }
            assert_eq!(fx.target_lufs(), want, "{}", p.name);
        }
    }

    fn near(v: f32, want: f64, tol: f64, what: &str) {
        assert!((v as f64 - want).abs() <= tol, "{what}: {v} (want {want} ±{tol})");
    }

    #[test]
    fn k_weighting_matches_the_bs1770_table_at_48k() {
        let [shelf, hp] = k_weighting(48000.0);
        let want = [
            (shelf.b0, 1.535_124_859_586_97),
            (shelf.b1, -2.691_696_189_406_38),
            (shelf.b2, 1.198_392_810_852_85),
            (shelf.a1, -1.690_659_293_182_41),
            (shelf.a2, 0.732_480_774_215_85),
            (hp.b0, 1.0),
            (hp.b1, -2.0),
            (hp.b2, 1.0),
            (hp.a1, -1.990_047_454_833_98),
            (hp.a2, 0.990_072_250_366_21),
        ];
        for (i, (got, want)) in want.iter().enumerate() {
            assert!((got - want).abs() < 1e-8, "coefficient {i}: {got} vs {want}");
        }
    }

    #[test]
    fn sine_at_minus_18_dbfs_reads_minus_18_lufs() {
        let m = play(48000.0, 1000.0, &[(-18.0, 5.0)]);
        near(m[M], -18.0, 0.1, "momentary");
        near(m[S], -18.0, 0.1, "short-term");
        near(m[I], -18.0, 0.1, "integrated");
        near(m[LRA], 0.0, 0.1, "range");
        near(m[PEAK_L], -18.0, 0.01, "sample peak");
    }

    #[test]
    fn tech3341_cases_1_and_2() {
        for level in [-23.0, -33.0] {
            let m = play(48000.0, 1000.0, &[(level, 20.0)]);
            near(m[M], level, 0.1, "momentary");
            near(m[S], level, 0.1, "short-term");
            near(m[I], level, 0.1, "integrated");
        }
    }

    #[test]
    fn tech3341_case_3_relative_gate() {
        let m = play(48000.0, 1000.0, &[(-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0)]);
        near(m[I], -23.0, 0.1, "integrated");
    }

    #[test]
    fn tech3341_case_4_absolute_and_relative_gates() {
        let m = play(48000.0, 1000.0, &[(-72.0, 10.0), (-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0), (-72.0, 10.0)]);
        near(m[I], -23.0, 0.1, "integrated");
    }

    #[test]
    fn tech3341_case_5_power_average() {
        let m = play(48000.0, 1000.0, &[(-26.0, 20.0), (-20.0, 20.1), (-26.0, 20.0)]);
        near(m[I], -23.0, 0.1, "integrated");
    }

    #[test]
    fn tech3342_loudness_range() {
        let cases: [(&[(f64, f64)], f64); 4] = [
            (&[(-20.0, 20.0), (-30.0, 20.0)], 10.0),
            (&[(-20.0, 20.0), (-15.0, 20.0)], 5.0),
            (&[(-40.0, 20.0), (-20.0, 20.0)], 20.0),
            (&[(-50.0, 20.0), (-35.0, 20.0), (-20.0, 20.0), (-35.0, 20.0), (-50.0, 20.0)], 15.0),
        ];
        for (i, (segments, want)) in cases.iter().enumerate() {
            let m = play(48000.0, 1000.0, segments);
            near(m[LRA], *want, 1.0, &format!("Tech 3342 case {}", i + 1));
        }
    }

    #[test]
    fn true_peak_finds_the_intersample_peak() {
        // A sine at fs/4 sampled 45° off its peaks: every sample is at 0.707 of the true peak.
        let sr = 48000.0;
        let mut fx = Loudness::new(sr);
        let ctx = FxContext { sample_rate: sr, tempo: 120.0, playing: true };
        let amp = 0.5f64;
        for b in 0..20 {
            // A sine switched on abruptly really does overshoot between samples, so measure
            // after the onset.
            if b == 2 {
                fx.set_param(RESET, 1.0);
                fx.set_param(RESET, 0.0);
            }
            let mut l: Vec<f32> =
                (0..1024).map(|i| (amp * (PI / 2.0 * (b * 1024 + i) as f64 + PI / 4.0).sin()) as f32).collect();
            let mut r = l.clone();
            fx.process(&mut l, &mut r, None, &ctx);
        }
        let mut m = [0.0; MAX_METERS];
        fx.meters(&mut m);
        near(m[PEAK_L], 20.0 * (amp * 0.5f64.sqrt()).log10(), 0.01, "sample peak");
        near(m[TP], 20.0 * amp.log10(), 0.1, "true peak");
    }

    #[test]
    fn true_peak_of_full_scale_sines() {
        // Off the fs/4 special case, across the band: the true peak is the sine's amplitude.
        for f in [1000.0, 5000.0, 12000.0, 17000.0] {
            let mut fx = Loudness::new(48000.0);
            // Skip the onset's overshoot (see above). 0.1 s is a whole number of cycles, so the
            // second call carries on in phase.
            play_on(&mut fx, 48000.0, f, &[(-6.0, 0.1)]);
            fx.set_param(RESET, 1.0);
            let m = play_on(&mut fx, 48000.0, f, &[(-6.0, 0.5)]);
            near(m[TP], -6.0, 0.1, &format!("true peak at {f} Hz"));
            near(m[PEAK_L], -6.0, 0.4, &format!("sample peak at {f} Hz"));
        }
    }

    #[test]
    fn loudness_is_the_same_at_any_sample_rate() {
        for f in [100.0, 1000.0, 3000.0] {
            let at48 = play(48000.0, f, &[(-20.0, 4.0)])[I] as f64;
            for sr in [44100.0, 96000.0] {
                let other = play(sr, f, &[(-20.0, 4.0)]);
                near(other[I], at48, 0.05, &format!("{f} Hz at {sr} vs 48k"));
                near(other[M], at48, 0.05, &format!("{f} Hz momentary at {sr} vs 48k"));
            }
        }
    }

    #[test]
    fn reset_and_auto_reset_start_a_new_measurement() {
        let mut fx = Loudness::new(48000.0);
        play_on(&mut fx, 48000.0, 1000.0, &[(-20.0, 5.0)]);
        // The panel pulses Reset; the measurement restarts and settles on the new level.
        fx.set_param(RESET, 1.0);
        fx.set_param(RESET, 0.0);
        let mut m = [0.0; MAX_METERS];
        fx.meters(&mut m);
        assert_eq!(m[I], FLOOR);
        assert_eq!(m[TP], FLOOR);
        let mut m = play_on(&mut fx, 48000.0, 1000.0, &[(-30.0, 5.0)]);
        near(m[I], -30.0, 0.1, "integrated after reset");

        // Stopping keeps the reading; the next play start clears it.
        let stopped = FxContext { sample_rate: 48000.0, tempo: 120.0, playing: false };
        let (mut l, mut r) = (vec![0.0f32; 1024], vec![0.0f32; 1024]);
        fx.process(&mut l, &mut r, None, &stopped);
        fx.reset();
        fx.meters(&mut m);
        near(m[I], -30.0, 0.1, "integrated after stop");
        let m = play_on(&mut fx, 48000.0, 1000.0, &[(-14.0, 5.0)]);
        near(m[I], -14.0, 0.1, "integrated after auto-reset");
        near(m[LRA], 0.0, 0.1, "range after auto-reset");
    }

    #[test]
    fn silence_reads_the_floor() {
        let m = play(48000.0, 1000.0, &[(-200.0, 4.0)]);
        assert_eq!(m[M], FLOOR);
        assert_eq!(m[I], FLOOR);
        assert_eq!(m[LRA], 0.0);
    }

    #[test]
    fn passes_audio_bit_exact() {
        let mut fx = Loudness::new(44100.0);
        let ctx = FxContext { sample_rate: 44100.0, tempo: 120.0, playing: true };
        let mut seed = 99u32;
        let input: Vec<f32> = (0..8192)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f32::from_bits((seed >> 9) | 0x3f80_0000) - 1.5
            })
            .collect();
        let (mut l, mut r) = (input.clone(), input.iter().rev().copied().collect::<Vec<_>>());
        for (bl, br) in l.chunks_mut(1000).zip(r.chunks_mut(1000)) {
            fx.process(bl, br, None, &ctx);
        }
        assert!(l.iter().zip(&input).all(|(a, b)| a.to_bits() == b.to_bits()));
        assert!(r.iter().zip(input.iter().rev()).all(|(a, b)| a.to_bits() == b.to_bits()));
    }
}
