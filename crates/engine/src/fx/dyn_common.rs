//! Building blocks shared by the Compressor and the Limiter: parameter smoothing, gain-computer
//! curves, the side-chain filter, delay lines, saturators and a 4× true-peak estimator.
//!
//! Everything here is allocation-free once constructed, so it can run on the audio thread.

use super::util::coeff;

/// Values below this are flushed to zero in decaying states, so silence never turns into a
/// stream of denormals (which are very slow on x86).
const TINY: f32 = 1e-15;

/// Flushes a value that has decayed into denormal territory to zero.
#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < TINY {
        0.0
    } else {
        x
    }
}

/// A linear-amplitude value expressed in dB, floored at -120 dB so silence stays finite.
#[inline]
pub fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(1e-6).log10()
}

/// dB to gain. `util::db_to_gain` uses `powf`; `exp` is noticeably cheaper per sample.
#[inline]
pub fn db_to_lin(db: f32) -> f32 {
    (db * (std::f32::consts::LN_10 / 20.0)).exp()
}

/// A parameter on its way to a new value: a one-pole glide of about 20 ms, so automation and
/// knob drags never zipper. When it arrives it holds the target exactly, so a parameter left
/// at 0 dB multiplies by exactly 1.0.
#[derive(Debug, Clone, Copy)]
pub struct Smoother {
    cur: f32,
    target: f32,
    k: f32,
}

impl Smoother {
    pub fn new(value: f32, sample_rate: f32) -> Self {
        Smoother { cur: value, target: value, k: coeff(20.0, sample_rate) }
    }

    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    /// Jumps straight to the target (on reset, when there is nothing to glide from).
    pub fn snap(&mut self) {
        self.cur = self.target;
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        if self.cur != self.target {
            let next = self.cur + self.k * (self.target - self.cur);
            // Land exactly once the remaining step is inaudible, or too small for f32 to take.
            self.cur = if next == self.cur || (next - self.target).abs() <= 1e-5 * (1.0 + self.target.abs()) {
                self.target
            } else {
                next
            };
        }
        self.cur
    }
}

/// Gain reduction in dB of a classic feedforward gain computer for a level `over` dB above the
/// threshold, after Giannoulis, Massberg & Reiss (JAES 2012, eq. 4): no reduction below the
/// knee, a quadratic blend across `knee` dB centred on the threshold, then `slope` dB of
/// reduction per dB above. For a ratio R, `slope` is `1 - 1/R`.
#[inline]
pub fn knee_reduction(over: f32, slope: f32, knee: f32) -> f32 {
    if 2.0 * over <= -knee {
        0.0
    } else if knee > 0.0 && 2.0 * over < knee {
        let t = over + knee * 0.5;
        slope * t * t / (2.0 * knee)
    } else {
        slope * over
    }
}

/// The derivative of [`knee_reduction`] with respect to `over`.
#[inline]
pub fn knee_reduction_slope(over: f32, slope: f32, knee: f32) -> f32 {
    if 2.0 * over <= -knee {
        0.0
    } else if knee > 0.0 && 2.0 * over < knee {
        slope * (over + knee * 0.5) / knee
    } else {
        slope
    }
}

/// Gain reduction of a "program" curve whose ratio keeps rising with level, as optical cells
/// and remote-cutoff tubes behave: `slope · w · ln(1 + e^(over/w))`. Its local slope is
/// `slope · σ(over/w)`, so the local ratio climbs smoothly from 1:1 well below the threshold
/// towards `1/(1 - slope)` far above it, and `w` sets how soft (wide) that climb is.
#[inline]
pub fn program_reduction(over: f32, slope: f32, w: f32) -> f32 {
    let z = over / w;
    // ln(1 + e^z) without overflow for large z.
    let sp = if z > 20.0 { z } else if z < -20.0 { z.exp() } else { z.exp().ln_1p() };
    slope * w * sp
}

/// The detector's side-chain filter shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScMode {
    Off,
    HighPass,
    LowPass,
    BandPass,
}

impl ScMode {
    pub fn from_index(i: f32) -> ScMode {
        match i.round() as i32 {
            1 => ScMode::HighPass,
            2 => ScMode::LowPass,
            3 => ScMode::BandPass,
            _ => ScMode::Off,
        }
    }
}

/// A stereo topology-preserving state-variable filter (Simper/Zavalishin), 12 dB/oct. It keeps
/// its state when the frequency moves, so sweeping the side-chain filter doesn't click.
#[derive(Debug, Clone, Copy, Default)]
pub struct Svf {
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    ic1: [f32; 2],
    ic2: [f32; 2],
}

impl Svf {
    /// Sets the cutoff (or centre) frequency and the resonance (Q).
    pub fn set(&mut self, freq: f32, q: f32, sample_rate: f32) {
        let f = freq.clamp(10.0, sample_rate * 0.49);
        self.g = (std::f32::consts::PI * f / sample_rate).tan();
        self.k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }

    pub fn reset(&mut self) {
        self.ic1 = [0.0; 2];
        self.ic2 = [0.0; 2];
    }

    /// Filters one sample of channel `ch` in the given mode.
    #[inline]
    pub fn process(&mut self, ch: usize, x: f32, mode: ScMode) -> f32 {
        if mode == ScMode::Off {
            return x;
        }
        let v3 = x - self.ic2[ch];
        let v1 = self.a1 * self.ic1[ch] + self.a2 * v3;
        let v2 = self.ic2[ch] + self.a2 * self.ic1[ch] + self.a3 * v3;
        self.ic1[ch] = flush(2.0 * v1 - self.ic1[ch]);
        self.ic2[ch] = flush(2.0 * v2 - self.ic2[ch]);
        match mode {
            ScMode::LowPass => v2,
            ScMode::BandPass => v1 * self.k,
            ScMode::HighPass => x - self.k * v1 - v2,
            ScMode::Off => x,
        }
    }
}

/// A one-channel delay line with a fixed maximum length, for lookahead.
#[derive(Debug, Clone)]
pub struct Delay {
    buf: Vec<f32>,
    pos: usize,
    len: usize,
}

impl Delay {
    pub fn new(max: usize) -> Self {
        Delay { buf: vec![0.0; max + 1], pos: 0, len: 0 }
    }

    /// Sets the delay in samples (clamped to the maximum).
    pub fn set_len(&mut self, len: usize) {
        self.len = len.min(self.buf.len() - 1);
    }

    pub fn reset(&mut self) {
        self.buf.iter_mut().for_each(|v| *v = 0.0);
    }

    /// Pushes a sample and returns the one from `len` samples ago. A zero delay returns the
    /// input itself, bit for bit.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        if self.len == 0 {
            return x;
        }
        let n = self.buf.len();
        self.buf[self.pos] = x;
        let read = (self.pos + n - self.len) % n;
        self.pos = (self.pos + 1) % n;
        self.buf[read]
    }
}

/// A first-order DC blocker (about 8 Hz), used after even-order saturation.
#[derive(Debug, Clone, Copy, Default)]
pub struct DcBlock {
    x1: f32,
    y1: f32,
    r: f32,
}

impl DcBlock {
    pub fn new(sample_rate: f32) -> Self {
        DcBlock { x1: 0.0, y1: 0.0, r: (-std::f32::consts::TAU * 8.0 / sample_rate).exp() }
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = flush(y);
        y
    }
}

/// Logic's Output Distortion choices, applied after makeup gain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distortion {
    Off,
    /// Gentle tape-like saturation that starts well below full scale.
    Soft,
    /// A cubic curve with a firmer knee that reaches full scale at 0 dBFS.
    Hard,
    /// A hard clip at 0 dBFS.
    Clip,
}

impl Distortion {
    pub fn from_index(i: f32) -> Distortion {
        match i.round() as i32 {
            1 => Distortion::Soft,
            2 => Distortion::Hard,
            3 => Distortion::Clip,
            _ => Distortion::Off,
        }
    }

    #[inline]
    pub fn apply(self, x: f32) -> f32 {
        match self {
            Distortion::Off => x,
            Distortion::Soft => x.tanh(),
            Distortion::Hard => {
                let c = x.clamp(-1.0, 1.0);
                1.5 * c - 0.5 * c * c * c
            }
            Distortion::Clip => x.clamp(-1.0, 1.0),
        }
    }
}

/// Taps per phase of the true-peak interpolator.
const TP_TAPS: usize = 32;
/// Oversampling factor of the true-peak estimator.
const TP_OS: usize = 4;

/// Samples of lookahead the true-peak estimate needs: the interpolator is centred on the
/// interval it reconstructs, and each sample's peak includes the intervals on both sides.
pub const TP_DELAY: usize = TP_TAPS / 2;

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let q = x * x / 4.0;
    for k in 1..30 {
        term *= q / (k * k) as f64;
        sum += term;
    }
    sum
}

/// The peak of a parabola through three equally spaced points around a local extreme `b`
/// (or `|b|` if `b` isn't one). Sampling a peak at 4× can still miss its top by up to
/// 1 - cos(π·f / 4fs), about 0.4 dB at 18 kHz; the parabola recovers nearly all of that.
#[inline]
fn vertex(a: f32, b: f32, c: f32) -> f32 {
    let s = if b < 0.0 { -1.0 } else { 1.0 };
    let (a, b, c) = (a * s, b * s, c * s);
    let den = a - 2.0 * b + c;
    if b >= a && b >= c && den < 0.0 {
        b - (a - c) * (a - c) / (8.0 * den)
    } else {
        b
    }
}

/// Estimates true (intersample) peaks the way ITU-R BS.1770 meters do, by 4× oversampling,
/// with a 128-tap Kaiser-windowed sinc split into 32-tap phases, plus a parabolic refinement
/// of each local maximum. Feed it samples one at a time: it reports the peak of the waveform
/// from just before [`center`](Self::center) (the sample `TP_DELAY` back) up to the next
/// sample, the center sample itself included.
#[derive(Debug, Clone)]
pub struct TruePeak {
    phases: [[f32; TP_TAPS]; TP_OS - 1],
    hist: [f32; TP_TAPS],
    /// The previous interval's last reconstructed point, the left neighbour of `center`.
    prev_last: f32,
}

impl Default for TruePeak {
    fn default() -> Self {
        Self::new()
    }
}

impl TruePeak {
    pub fn new() -> Self {
        let mut phases = [[0.0f32; TP_TAPS]; TP_OS - 1];
        let half = TP_TAPS as f64 / 2.0;
        let beta = 8.0;
        for (p, phase) in phases.iter_mut().enumerate() {
            // The point sits at index (half - 1) + frac within the history.
            let t = half - 1.0 + (p + 1) as f64 / TP_OS as f64;
            let mut sum = 0.0;
            let mut c = [0.0f64; TP_TAPS];
            for (j, cj) in c.iter_mut().enumerate() {
                let d = t - j as f64;
                let sinc = if d.abs() < 1e-12 { 1.0 } else { (std::f64::consts::PI * d).sin() / (std::f64::consts::PI * d) };
                let r = d / half;
                let w = if r.abs() >= 1.0 { 0.0 } else { bessel_i0(beta * (1.0 - r * r).sqrt()) / bessel_i0(beta) };
                *cj = sinc * w;
                sum += *cj;
            }
            for (o, cj) in phase.iter_mut().zip(c) {
                *o = (cj / sum) as f32;
            }
        }
        TruePeak { phases, hist: [0.0; TP_TAPS], prev_last: 0.0 }
    }

    pub fn reset(&mut self) {
        self.hist = [0.0; TP_TAPS];
        self.prev_last = 0.0;
    }

    /// The sample `TP_DELAY` back, where the interval [`push`](Self::push) reports starts.
    pub fn center(&self) -> f32 {
        self.hist[TP_TAPS - 1 - TP_DELAY]
    }

    /// Pushes a sample; returns the peak of the waveform around [`center`](Self::center) and
    /// on to the following sample.
    #[inline]
    pub fn push(&mut self, x: f32) -> f32 {
        self.hist.copy_within(1.., 0);
        self.hist[TP_TAPS - 1] = x;
        // The interval's points: the center sample, the reconstructed ones, the next sample.
        let mut q = [0.0f32; TP_OS + 1];
        q[0] = self.center();
        q[TP_OS] = self.hist[TP_TAPS - TP_DELAY];
        for (phase, qk) in self.phases.iter().zip(&mut q[1..TP_OS]) {
            let mut acc = 0.0f32;
            for (c, h) in phase.iter().zip(&self.hist) {
                acc += c * h;
            }
            *qk = acc;
        }
        let mut peak = vertex(self.prev_last, q[0], q[1]);
        for k in 1..TP_OS {
            peak = peak.max(vertex(q[k - 1], q[k], q[k + 1]));
        }
        self.prev_last = q[TP_OS - 1];
        peak
    }
}

/// Checks a factory preset list against its effect's parameters: every id exists, every value
/// is in range, choices and toggles are whole numbers, and no preset sets a parameter twice.
#[cfg(test)]
pub fn check_presets(params: &[super::ParamSpec], presets: &[super::FactoryPreset]) {
    use super::ParamKind;
    let mut names = std::collections::HashSet::new();
    for p in presets {
        assert!(names.insert(p.name), "duplicate preset name {}", p.name);
        let mut seen = std::collections::HashSet::new();
        for &(id, v) in p.values {
            let spec = params.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("{}: no parameter {id}", p.name));
            assert!(seen.insert(id), "{}: {} set twice", p.name, spec.name);
            assert!(v >= spec.min && v <= spec.max, "{}: {} = {v} is outside {}..{}", p.name, spec.name, spec.min, spec.max);
            if !matches!(spec.kind, ParamKind::Continuous) {
                assert_eq!(v, v.round(), "{}: {} must be a whole number", p.name, spec.name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knee_matches_hard_knee_outside() {
        assert_eq!(knee_reduction(-10.0, 0.75, 6.0), 0.0);
        assert!((knee_reduction(10.0, 0.75, 6.0) - 7.5).abs() < 1e-6);
        // Continuous at the knee edges.
        assert!((knee_reduction(3.0, 0.75, 6.0) - 2.25).abs() < 1e-5);
        assert!(knee_reduction(-3.0, 0.75, 6.0).abs() < 1e-6);
    }

    #[test]
    fn smoother_lands_exactly() {
        let mut s = Smoother::new(0.0, 48000.0);
        s.set(1.0);
        for _ in 0..48000 {
            s.next();
        }
        assert_eq!(s.next(), 1.0);
    }

    #[test]
    fn true_peak_finds_intersample_peak() {
        // fs/4 at 45°: every sample is at 0.707 but the waveform peaks at 1.0.
        let mut tp = TruePeak::new();
        let mut peak = 0.0f32;
        for n in 0..400 {
            let x = (std::f32::consts::FRAC_PI_2 * n as f32 + std::f32::consts::FRAC_PI_4).sin();
            let p = tp.push(x);
            if n > 50 {
                peak = peak.max(p);
            }
        }
        assert!(peak > 0.98 && peak < 1.02, "peak {peak}");
    }
}
