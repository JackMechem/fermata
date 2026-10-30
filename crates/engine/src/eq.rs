//! Channel EQ DSP: RBJ-cookbook biquads in transposed direct form II, run in f64 so low
//! frequencies stay accurate. Coefficients are computed off the audio thread and copied into
//! the running filters, which keep their state so parameter drags don't click.

use crate::model::{BandKind, ChannelEq};

/// Enough sections for eight bands with both cuts at 48 dB/oct.
pub const MAX_SECTIONS: usize = 16;

#[derive(Debug, Clone, Copy, Default)]
pub struct Section {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct EqCoeffs {
    pub active: bool,
    sections: [Section; MAX_SECTIONS],
    count: usize,
    output: f32,
}

impl Default for EqCoeffs {
    fn default() -> Self {
        EqCoeffs { active: false, sections: [Section::default(); MAX_SECTIONS], count: 0, output: 1.0 }
    }
}

fn normalize(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Section {
    Section { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
}

fn bell(sr: f64, f: f64, q: f64, db: f64) -> Section {
    let a = 10f64.powf(db / 40.0);
    let w = std::f64::consts::TAU * f / sr;
    let alpha = w.sin() / (2.0 * q);
    let c = w.cos();
    normalize(1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a)
}

fn shelf(sr: f64, f: f64, q: f64, db: f64, high: bool) -> Section {
    let a = 10f64.powf(db / 40.0);
    let w = std::f64::consts::TAU * f / sr;
    let c = w.cos();
    let alpha = w.sin() / (2.0 * q);
    let sa = 2.0 * a.sqrt() * alpha;
    if high {
        normalize(
            a * ((a + 1.0) + (a - 1.0) * c + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - sa),
            (a + 1.0) - (a - 1.0) * c + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - sa,
        )
    } else {
        normalize(
            a * ((a + 1.0) - (a - 1.0) * c + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - sa),
            (a + 1.0) + (a - 1.0) * c + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - sa,
        )
    }
}

fn pass(sr: f64, f: f64, q: f64, high_pass: bool) -> Section {
    let w = std::f64::consts::TAU * f / sr;
    let c = w.cos();
    let alpha = w.sin() / (2.0 * q);
    if high_pass {
        normalize((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    } else {
        normalize((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }
}

/// Q of each second-order stage of a Butterworth filter of the given (even) order. The first
/// stage of a 12 dB/oct cut uses the band's own Q so it can resonate, as in Logic.
pub fn butterworth_qs(order: usize) -> Vec<f64> {
    (1..=order / 2).map(|k| 1.0 / (2.0 * (std::f64::consts::PI * (2 * k - 1) as f64 / (2 * order) as f64).cos())).collect()
}

pub fn coefficients(eq: &ChannelEq, sample_rate: f64) -> EqCoeffs {
    let mut out = EqCoeffs::default();
    if eq.is_flat() {
        return out;
    }
    let nyq = sample_rate * 0.49;
    let push = |s: Section, out: &mut EqCoeffs| {
        if out.count < MAX_SECTIONS {
            out.sections[out.count] = s;
            out.count += 1;
        }
    };
    for b in eq.bands.iter().filter(|b| b.on) {
        let f = (b.freq as f64).clamp(10.0, nyq);
        let q = (b.q as f64).clamp(0.1, 18.0);
        match b.kind {
            BandKind::Bell if b.gain.abs() >= 0.01 => push(bell(sample_rate, f, q, b.gain as f64), &mut out),
            BandKind::LowShelf if b.gain.abs() >= 0.01 => push(shelf(sample_rate, f, q, b.gain as f64, false), &mut out),
            BandKind::HighShelf if b.gain.abs() >= 0.01 => push(shelf(sample_rate, f, q, b.gain as f64, true), &mut out),
            BandKind::LowCut | BandKind::HighCut => {
                let order = match b.slope {
                    0..=12 => 2,
                    13..=24 => 4,
                    _ => 8,
                };
                let hp = b.kind == BandKind::LowCut;
                for (i, bq) in butterworth_qs(order).into_iter().enumerate() {
                    let stage_q = if order == 2 && i == 0 { q } else { bq };
                    push(pass(sample_rate, f, stage_q, hp), &mut out);
                }
            }
            _ => {}
        }
    }
    out.output = 10f32.powf(eq.output_db / 20.0);
    out.active = true;
    out
}

impl EqCoeffs {
    /// The EQ's gain (linear, output gain included) at `w` radians per sample: what the
    /// Linear Phase EQ realizes with linear phase. 1 when the EQ is off or flat.
    pub fn magnitude(&self, w: f64) -> f64 {
        if !self.active {
            return 1.0;
        }
        let (c1, c2) = (w.cos(), (2.0 * w).cos());
        let mut power = 1.0;
        for s in &self.sections[..self.count] {
            // |b0 + b1 z^-1 + b2 z^-2|² on the unit circle, and the same for the poles.
            let num = s.b0 * s.b0 + s.b1 * s.b1 + s.b2 * s.b2 + 2.0 * (s.b0 * s.b1 + s.b1 * s.b2) * c1 + 2.0 * s.b0 * s.b2 * c2;
            let den = 1.0 + s.a1 * s.a1 + s.a2 * s.a2 + 2.0 * (s.a1 + s.a1 * s.a2) * c1 + 2.0 * s.a2 * c2;
            power *= num.max(0.0) / den.max(1e-300);
        }
        power.sqrt() * self.output as f64
    }
}

/// Per-channel filter memory for one strip.
pub struct EqState {
    z: [[[f64; 2]; MAX_SECTIONS]; 2],
}

impl Default for EqState {
    fn default() -> Self {
        EqState { z: [[[0.0; 2]; MAX_SECTIONS]; 2] }
    }
}

impl EqState {
    pub fn reset(&mut self) {
        *self = EqState::default();
    }

    pub fn process(&mut self, c: &EqCoeffs, l: &mut [f32], r: &mut [f32]) {
        if !c.active {
            return;
        }
        for (ch, buf) in [l, r].into_iter().enumerate() {
            let z = &mut self.z[ch];
            for x in buf.iter_mut() {
                let mut v = *x as f64;
                for (s, zz) in c.sections[..c.count].iter().zip(z.iter_mut()) {
                    let y = s.b0 * v + zz[0];
                    zz[0] = s.b1 * v - s.a1 * y + zz[1];
                    zz[1] = s.b2 * v - s.a2 * y;
                    v = y;
                }
                *x = v as f32 * c.output;
            }
        }
        // Flush denormals so silence after loud material doesn't cost CPU.
        for ch in self.z.iter_mut() {
            for zz in ch.iter_mut() {
                for v in zz.iter_mut() {
                    if v.abs() < 1e-30 {
                        *v = 0.0;
                    }
                }
            }
        }
    }
}

// ---- spectrum analyzer ----------------------------------------------------------------------

pub const FFT_SIZE: usize = 4096;
/// Log-spaced display bands between 20 Hz and 20 kHz.
pub const SPECTRUM_BANDS: usize = 120;

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f64::consts::TAU / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

/// Magnitude spectrum in dBFS (a full-scale sine reads about 0 dB), as `SPECTRUM_BANDS`
/// log-spaced bands. `samples` must hold `FFT_SIZE` values, oldest first.
pub fn spectrum(samples: &[f32], sample_rate: f64) -> Vec<f32> {
    let n = FFT_SIZE;
    let mut re = vec![0.0f64; n];
    let mut im = vec![0.0f64; n];
    let mut wsum = 0.0;
    for i in 0..n {
        let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (n - 1) as f64).cos();
        wsum += w;
        re[i] = samples.get(i).copied().unwrap_or(0.0) as f64 * w;
    }
    fft(&mut re, &mut im);
    let bin_hz = sample_rate / n as f64;
    let mag = |k: usize| (re[k] * re[k] + im[k] * im[k]).sqrt() * 2.0 / wsum;
    let (lo, hi) = (20f64.ln(), 20000f64.ln());
    (0..SPECTRUM_BANDS)
        .map(|b| {
            let f0 = (lo + (hi - lo) * b as f64 / SPECTRUM_BANDS as f64).exp();
            let f1 = (lo + (hi - lo) * (b + 1) as f64 / SPECTRUM_BANDS as f64).exp();
            let k0 = ((f0 / bin_hz).floor() as usize).clamp(1, n / 2 - 1);
            let k1 = ((f1 / bin_hz).ceil() as usize).clamp(k0 + 1, n / 2);
            let m = (k0..k1).map(mag).fold(0.0, f64::max);
            (20.0 * m.max(1e-9).log10()) as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BandKind, ChannelEq};

    const SR: f64 = 48000.0;

    /// Steady-state gain in dB of the EQ at `freq`, measured by running a sine through it.
    fn gain_at(eq: &ChannelEq, freq: f64) -> f64 {
        let c = coefficients(eq, SR);
        let mut st = EqState::default();
        let n = 48000;
        let mut l: Vec<f32> = (0..n).map(|i| (std::f64::consts::TAU * freq * i as f64 / SR).sin() as f32 * 0.5).collect();
        let mut r = l.clone();
        st.process(&c, &mut l, &mut r);
        let peak = l[n / 2..].iter().fold(0.0f32, |a, &b| a.max(b.abs())) as f64;
        20.0 * (peak / 0.5).log10()
    }

    fn only(kind_idx: usize, f: impl Fn(&mut crate::model::EqBand)) -> ChannelEq {
        let mut eq = ChannelEq::default();
        for b in eq.bands.iter_mut() {
            b.gain = 0.0;
            if matches!(b.kind, BandKind::LowCut | BandKind::HighCut) {
                b.on = false;
            }
        }
        f(&mut eq.bands[kind_idx]);
        eq
    }

    #[test]
    fn default_is_flat_and_skipped() {
        let eq = ChannelEq::default();
        assert!(eq.is_flat());
        assert!(!coefficients(&eq, SR).active);
    }

    #[test]
    fn bell_boost_hits_its_gain() {
        let eq = only(4, |b| {
            b.freq = 1000.0;
            b.gain = 6.0;
        });
        let g = gain_at(&eq, 1000.0);
        assert!((g - 6.0).abs() < 0.2, "bell gain at centre {g}");
        assert!(gain_at(&eq, 100.0).abs() < 0.3);
    }

    #[test]
    fn shelves_and_cuts() {
        let ls = only(1, |b| {
            b.freq = 200.0;
            b.gain = -9.0;
        });
        assert!((gain_at(&ls, 40.0) + 9.0).abs() < 0.5, "low shelf {}", gain_at(&ls, 40.0));
        assert!(gain_at(&ls, 5000.0).abs() < 0.3);

        let lc = only(0, |b| {
            b.on = true;
            b.freq = 200.0;
            b.slope = 24;
        });
        // One octave below a 24 dB/oct Butterworth: about -24 dB; -3 dB at the corner.
        assert!((gain_at(&lc, 200.0) + 3.0).abs() < 0.5, "corner {}", gain_at(&lc, 200.0));
        assert!(gain_at(&lc, 100.0) < -20.0, "octave below {}", gain_at(&lc, 100.0));
        assert!(gain_at(&lc, 2000.0).abs() < 0.2);

        let hc = only(7, |b| {
            b.on = true;
            b.freq = 2000.0;
            b.slope = 48;
        });
        assert!(gain_at(&hc, 4000.0) < -44.0, "48 dB/oct {}", gain_at(&hc, 4000.0));
    }

    #[test]
    fn analyzer_reads_full_scale_sine_near_zero_db() {
        let s: Vec<f32> = (0..FFT_SIZE).map(|i| (std::f64::consts::TAU * 1000.0 * i as f64 / SR).sin() as f32).collect();
        let spec = spectrum(&s, SR);
        let peak = spec.iter().cloned().fold(f32::MIN, f32::max);
        assert!((peak - 0.0).abs() < 1.5, "peak {peak}");
    }
}
