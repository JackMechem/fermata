//! 4× oversampling for the distortion effects, so the harmonics a waveshaper adds above the
//! Nyquist frequency are filtered away instead of folding back into the audible band as
//! inharmonic aliasing.
//!
//! Two linear-phase half-band FIR stages run in cascade (1× ↔ 2× ↔ 4×). A half-band filter has
//! every other tap at zero and its centre tap at exactly ½, so each stage costs about half its
//! length per sample. The filters are symmetric, so the round trip only delays the signal: by
//! [`LATENCY`] samples at the base rate, which the effects report for delay compensation.

/// The first stage's centre tap (its length is twice this plus one). Its band edges sit
/// symmetrically around a quarter of the 2× rate: flat to about 20 kHz at 48 kHz, more than 80
/// dB down from 28 kHz.
const C1: usize = 31;
const BETA1: f64 = 8.0;
/// The second stage only has to reject images of what the first stage passes (0 to 28 kHz at
/// 48 kHz), which lie well above it, so it can be short. Its centre must be even for the latency
/// to come out in whole base-rate samples.
const C2: usize = 14;
const BETA2: f64 = 8.0;

/// The round trip's delay at the base rate: C1 at the 2× rate twice over, plus C2 at the 4× rate
/// twice over.
pub(crate) const LATENCY: usize = C1 + C2 / 2;

/// The modified Bessel function I0, for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let q = x * x / 4.0;
    for k in 1..64 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// A Kaiser-windowed half-band low-pass of length `2c + 1`: `h[c]` is exactly ½ and the taps an
/// even distance from it are zero. The others are scaled to sum to ½, so the DC gain is exactly 1
/// both when decimating and in each polyphase branch when interpolating.
fn design(c: usize, beta: f64) -> Vec<f64> {
    let n = 2 * c + 1;
    let mut h = vec![0.0; n];
    for (k, t) in h.iter_mut().enumerate() {
        let o = k as f64 - c as f64;
        if k == c || (k as isize - c as isize) % 2 != 0 {
            let x = std::f64::consts::PI * o / 2.0;
            let sinc = if o == 0.0 { 1.0 } else { x.sin() / x };
            let r = o / c as f64;
            let w = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta);
            *t = 0.5 * sinc * w;
        }
    }
    h[c] = 0.5;
    let side: f64 = h.iter().enumerate().filter(|(k, _)| *k != c).map(|(_, t)| t).sum();
    for (k, t) in h.iter_mut().enumerate() {
        if k != c {
            *t *= 0.5 / side;
        }
    }
    h
}

/// A history of the last `len` samples, readable newest-first as one slice: every sample is
/// written twice, `len` apart, so the window never wraps.
struct History {
    buf: Vec<f32>,
    len: usize,
    at: usize,
}

impl History {
    fn new(len: usize) -> History {
        History { buf: vec![0.0; 2 * len], len, at: 0 }
    }

    #[inline]
    fn push(&mut self, x: f32) {
        self.at = if self.at == 0 { self.len - 1 } else { self.at - 1 };
        self.buf[self.at] = x;
        self.buf[self.at + self.len] = x;
    }

    /// `[x[n], x[n-1], ...]`.
    #[inline]
    fn window(&self) -> &[f32] {
        &self.buf[self.at..self.at + self.len]
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

/// Doubles the rate: zero-stuffs and filters, as two polyphase branches. One branch is only the
/// centre tap (a plain delay); the other carries the non-zero side taps.
struct Up {
    /// Twice the side taps of the branch that has them, newest input first.
    taps: Vec<f32>,
    /// Which of the two outputs is the delayed input (the centre tap's branch).
    center_first: bool,
    center_delay: usize,
    hist: History,
}

impl Up {
    fn new(h: &[f64]) -> Up {
        let c = h.len() / 2;
        let par = if c % 2 == 0 { 1 } else { 0 };
        let taps: Vec<f32> = (0..).map(|q| 2 * q + par).take_while(|&k| k < h.len()).map(|k| (2.0 * h[k]) as f32).collect();
        Up { center_first: c % 2 == 0, center_delay: c / 2, hist: History::new(taps.len().max(c / 2 + 1)), taps }
    }

    /// The two 2× samples for one input sample, in time order.
    #[inline]
    fn push(&mut self, x: f32) -> (f32, f32) {
        self.hist.push(x);
        let w = self.hist.window();
        let filtered: f32 = self.taps.iter().zip(w).map(|(t, x)| t * x).sum();
        let delayed = w[self.center_delay];
        if self.center_first {
            (delayed, filtered)
        } else {
            (filtered, delayed)
        }
    }
}

/// Halves the rate: filters and keeps every other sample.
struct Down {
    /// The side taps `h[par], h[par + 2], ...`.
    taps: Vec<f32>,
    par: usize,
    c: usize,
    hist: History,
}

impl Down {
    fn new(h: &[f64]) -> Down {
        let c = h.len() / 2;
        let par = if c % 2 == 0 { 1 } else { 0 };
        let taps = (0..).map(|q| 2 * q + par).take_while(|&k| k < h.len()).map(|k| h[k] as f32).collect();
        Down { taps, par, c, hist: History::new(h.len()) }
    }

    /// One output sample from two input samples (in time order).
    #[inline]
    fn push(&mut self, a: f32, b: f32) -> f32 {
        self.hist.push(a);
        let w = self.hist.window();
        let mut y = 0.5 * w[self.c];
        for (i, t) in self.taps.iter().enumerate() {
            y += t * w[self.par + 2 * i];
        }
        self.hist.push(b);
        y
    }
}

/// One channel's 4× oversampler.
pub(crate) struct Oversampler {
    up1: Up,
    up2: Up,
    down2: Down,
    down1: Down,
}

impl Oversampler {
    /// Designs the filters and allocates the histories: call off the audio thread.
    pub(crate) fn new() -> Oversampler {
        let h1 = design(C1, BETA1);
        let h2 = design(C2, BETA2);
        Oversampler { up1: Up::new(&h1), up2: Up::new(&h2), down2: Down::new(&h2), down1: Down::new(&h1) }
    }

    /// Runs one base-rate sample through `shape` at 4× the rate. The result comes out
    /// [`LATENCY`] samples later.
    #[inline]
    pub(crate) fn process(&mut self, x: f32, mut shape: impl FnMut(f32) -> f32) -> f32 {
        let (a, b) = self.up1.push(x);
        let mut mid = [0.0; 2];
        for (m, s) in mid.iter_mut().zip([a, b]) {
            let (p, q) = self.up2.push(s);
            *m = self.down2.push(shape(p), shape(q));
        }
        self.down1.push(mid[0], mid[1])
    }

    pub(crate) fn reset(&mut self) {
        self.up1.hist.clear();
        self.up2.hist.clear();
        self.down2.hist.clear();
        self.down1.hist.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    /// The magnitude of a filter's response at `f` (cycles per sample), in dB.
    fn response_db(h: &[f64], f: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (k, t) in h.iter().enumerate() {
            re += t * (TAU * f * k as f64).cos();
            im -= t * (TAU * f * k as f64).sin();
        }
        20.0 * (re * re + im * im).sqrt().log10()
    }

    /// The level of frequency `f` (Hz) in `x`, in dB relative to a full-scale sine.
    fn level_db(x: &[f32], f: f64, sr: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        let n = x.len() as f64;
        for (i, &s) in x.iter().enumerate() {
            // A Hann window keeps the strong fundamental from leaking into the bins we measure.
            let w = 0.5 - 0.5 * (TAU * i as f64 / n).cos();
            re += s as f64 * w * (TAU * f * i as f64 / sr).cos();
            im += s as f64 * w * (TAU * f * i as f64 / sr).sin();
        }
        20.0 * ((re * re + im * im).sqrt() * 4.0 / n).max(1e-12).log10()
    }

    #[test]
    fn half_band_filters_are_flat_then_deep() {
        let h1 = design(C1, BETA1);
        // At the 2× rate of 48 kHz: flat to 20 kHz, more than 80 dB down from 28 kHz.
        for f in [0.0, 0.05, 0.1, 0.15, 0.2, 20.0 / 96.0] {
            assert!(response_db(&h1, f).abs() < 0.01, "passband {f}: {}", response_db(&h1, f));
        }
        for f in [28.0 / 96.0, 0.32, 0.4, 0.5] {
            assert!(response_db(&h1, f) < -80.0, "stopband {f}: {}", response_db(&h1, f));
        }
        // The second stage at 192 kHz: flat to 28 kHz, images of 0..28 kHz (68..96 kHz) gone.
        let h2 = design(C2, BETA2);
        assert!(response_db(&h2, 28.0 / 192.0).abs() < 0.01, "{}", response_db(&h2, 28.0 / 192.0));
        for f in [68.0 / 192.0, 0.4, 0.5] {
            assert!(response_db(&h2, f) < -80.0, "second stage at {f}: {}", response_db(&h2, f));
        }
        assert_eq!(h1[C1], 0.5);
        assert!(h1[C1 + 2].abs() < 1e-15 && h1[C1 + 1] > 0.3);
    }

    #[test]
    fn the_round_trip_is_a_pure_delay() {
        let mut os = Oversampler::new();
        let sr = 48_000.0;
        let x: Vec<f32> = (0..4800).map(|i| (0.5 * (TAU * 1000.0 * i as f64 / sr).sin() + 0.3 * (TAU * 9000.0 * i as f64 / sr).sin()) as f32).collect();
        let y: Vec<f32> = x.iter().map(|&s| os.process(s, |v| v)).collect();
        let err = (LATENCY + 200..x.len()).map(|i| (y[i] - x[i - LATENCY]).abs()).fold(0.0f32, f32::max);
        assert!(err < 1e-3, "error {err}");
        // A DC step settles to exactly the same level.
        let mut os = Oversampler::new();
        let last = (0..400).map(|_| os.process(0.25, |v| v)).last().unwrap();
        assert!((last - 0.25).abs() < 1e-5, "{last}");
    }

    #[test]
    fn clipping_aliases_far_less_than_without_oversampling() {
        // A 7 kHz sine clipped hard: its 5th and 7th harmonics (35 and 49 kHz) fold back to 13
        // and 1 kHz at 48 kHz unless they are filtered first.
        let sr = 48_000.0;
        let clip = |v: f32| (3.0 * v).clamp(-1.0, 1.0);
        let x: Vec<f32> = (0..16384).map(|i| (0.9 * (TAU * 7000.0 * i as f64 / sr).sin()) as f32).collect();
        let naive: Vec<f32> = x.iter().map(|&v| clip(v)).collect();
        let mut os = Oversampler::new();
        let over: Vec<f32> = x.iter().map(|&v| os.process(v, clip)).skip(LATENCY).collect();
        let fund = level_db(&over, 7000.0, sr);
        for alias in [13_000.0, 1000.0] {
            let a_naive = level_db(&naive, alias, sr) - level_db(&naive, 7000.0, sr);
            let a_over = level_db(&over, alias, sr) - fund;
            assert!(a_naive > -40.0, "the naive clipper aliases at {alias} ({a_naive:.1} dB)");
            assert!(a_over < -55.0, "oversampled alias at {alias}: {a_over:.1} dB");
        }
    }
}
