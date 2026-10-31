//! The sampler's interpolator: windowed-sinc kernels read from precomputed tables.
//!
//! Each bank is a Kaiser-windowed sinc (β = 9, about -90 dB) with its cutoff at `1 / r`, for
//! playing at up to `r` source samples per output sample. Bank 0 (`r` = 1) is the plain sinc, so
//! a sample at its own pitch and rate comes out unchanged. Above that the kernel stretches: its
//! length grows with `r`, keeping the same transition relative to the output's Nyquist, instead
//! of decimated copies of every sample. That costs no memory and leaves no seam at a loop, and a
//! voice's cost rises with how far up it plays. Banks are a quarter octave apart. At 4 octaves up
//! (`r` = 16) the kernel reaches its longest; from there to 6 octaves it stays that long, and its
//! cutoff comes down early to keep the aliases out, so the top of the sound dulls instead. Past 6
//! octaves aliasing rises.
//!
//! Rows of a table are the kernel at [`PHASES`] fractional positions, and reading blends two
//! neighbouring rows. Only `dot` and the blend run on the audio thread. They're written so the
//! compiler vectorizes them without loosening floating point, so a render repeats bit for bit.

use std::sync::OnceLock;

/// Fractional positions in a table. With the blend the table's error is below -110 dB.
pub const PHASES: usize = 256;
/// Banks per octave, and how many octaves of stretching they cover.
const BANKS_PER_OCTAVE: usize = 4;
const OCTAVES: usize = 6;
/// Taps of the kernel per unit of `r`.
const TAPS_PER_R: f64 = 32.0;
/// Kaiser's β: side lobes at about -90 dB.
const BETA: f64 = 9.0;
/// The longest kernel, from `r` = 16 up.
pub const MAX_TAPS: usize = 512;
/// Silence kept before and after a sample's frames (and audio around a loop) so a kernel never
/// reads out of bounds.
pub const GUARD: usize = MAX_TAPS / 2 + 2;

pub struct Bank {
    pub taps: usize,
    /// `(PHASES + 1) * taps` values.
    table: Box<[f32]>,
}

pub struct Kernels {
    banks: Vec<Bank>,
}

/// The modified Bessel function of order 0, by its series.
fn i0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0, 1.0, x * x / 4.0);
    for k in 1..64 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < 1e-17 * sum {
            break;
        }
    }
    sum
}

fn make_bank(r: f64) -> Bank {
    // Even, so a position always sits between the two middle taps.
    let taps = (((TAPS_PER_R * r / 2.0).round() as usize) * 2).clamp(32, MAX_TAPS);
    let half = taps as f64 / 2.0;
    // Past the longest kernel its transition can't shrink with the cutoff, so the cutoff falls
    // further, by up to the transition, to end the stop band where it should. Half the transition
    // is 2.85 / taps at this window, in cycles per source sample.
    let nominal = 0.5 / r;
    let spill = 2.85 / taps as f64 * 2.0 * (1.0 - (MAX_TAPS as f64 / TAPS_PER_R / r).min(1.0));
    let cut = 2.0 * (nominal - spill).max(nominal * 0.3);
    let denom = i0(BETA);
    let mut table = vec![0.0f32; (PHASES + 1) * taps].into_boxed_slice();
    for (p, row) in table.chunks_exact_mut(taps).enumerate() {
        let frac = p as f64 / PHASES as f64;
        let mut sum = 0.0;
        let mut values = vec![0.0f64; taps];
        for (i, v) in values.iter_mut().enumerate() {
            // Tap i reads the frame at (position's floor) - half + 1 + i.
            let t = i as f64 - half + 1.0 - frac;
            let x = t * cut;
            let s = if x.abs() < 1e-12 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
            let w = t / half;
            let window = if w.abs() >= 1.0 { 0.0 } else { i0(BETA * (1.0 - w * w).sqrt()) / denom };
            *v = s * window;
            sum += *v;
        }
        // Flat at DC at every fractional position.
        for (o, v) in row.iter_mut().zip(&values) {
            *o = (*v / sum) as f32;
        }
    }
    Bank { taps, table }
}

/// The kernels, built on first use (a sampler's constructor, off the audio thread).
pub fn kernels() -> &'static Kernels {
    static K: OnceLock<Kernels> = OnceLock::new();
    K.get_or_init(|| Kernels { banks: (0..=BANKS_PER_OCTAVE * OCTAVES).map(|k| make_bank(2f64.powf(k as f64 / BANKS_PER_OCTAVE as f64))).collect() })
}

impl Kernels {
    /// The bank for playing at `ratio` source samples per output sample: the first whose cutoff
    /// is at or below what that ratio needs.
    #[inline]
    pub fn bank_index(&self, ratio: f64) -> usize {
        if ratio <= 1.000_001 {
            return 0;
        }
        (((ratio.log2() * BANKS_PER_OCTAVE as f64).ceil()) as usize).min(self.banks.len() - 1)
    }

    #[inline]
    pub fn bank(&self, index: usize) -> &Bank {
        &self.banks[index]
    }

    /// All the tables' size in bytes, for the docs.
    pub fn bytes(&self) -> usize {
        self.banks.iter().map(|b| b.table.len() * 4).sum()
    }
}

/// a·b. Eight running sums, so the compiler can vectorize it without reordering additions.
#[inline]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0.0f32; 8];
    let (ca, ra) = a.as_chunks::<8>();
    let (cb, rb) = b.as_chunks::<8>();
    for (x, y) in ca.iter().zip(cb) {
        for k in 0..8 {
            acc[k] += x[k] * y[k];
        }
    }
    let mut sum = (acc[0] + acc[4]) + (acc[1] + acc[5]) + (acc[2] + acc[6]) + (acc[3] + acc[7]);
    for (x, y) in ra.iter().zip(rb) {
        sum += x * y;
    }
    sum
}

/// Where a frame's worth of audio is read from: `at` indexes the `data` of each channel (guards
/// included), `frac` is 0..1 past it.
impl Bank {
    /// The kernel at `frac`, into `out`. Returns the part of `out` it filled.
    #[inline]
    pub fn blend<'a>(&self, frac: f32, out: &'a mut [f32; MAX_TAPS]) -> &'a [f32] {
        let ph = frac * PHASES as f32;
        let p = (ph as usize).min(PHASES - 1);
        let t = ph - p as f32;
        let taps = self.taps;
        let a = &self.table[p * taps..][..taps];
        let b = &self.table[(p + 1) * taps..][..taps];
        for ((o, x), y) in out[..taps].iter_mut().zip(a).zip(b) {
            *o = x + (y - x) * t;
        }
        &out[..taps]
    }

    /// Applies a kernel from [`blend`](Bank::blend) around frame index `at`.
    #[inline]
    pub fn apply(&self, kernel: &[f32], data: &[f32], at: usize) -> f32 {
        let from = at + 1 - self.taps / 2;
        dot(kernel, &data[from..from + self.taps])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernels_are_flat_and_exact_at_whole_positions() {
        let k = kernels();
        let mut buf = [0.0; MAX_TAPS];
        for b in &k.banks {
            for frac in [0.0, 0.1, 0.37, 0.5, 0.99] {
                let sum: f32 = b.blend(frac, &mut buf).iter().sum();
                assert!((sum - 1.0).abs() < 1e-5, "{sum}");
            }
        }
        // The plain sinc at a whole position is a single tap.
        let b = k.bank(0);
        let row = b.blend(0.0, &mut buf);
        let mid = b.taps / 2 - 1;
        assert!((row[mid] - 1.0).abs() < 1e-6);
        assert!(row.iter().enumerate().all(|(i, v)| i == mid || v.abs() < 1e-6));
    }

    #[test]
    fn banks_cover_the_ratios() {
        let k = kernels();
        assert_eq!(k.bank_index(1.0), 0);
        assert_eq!(k.bank_index(0.5), 0);
        assert_eq!(k.bank_index(1.1), 1);
        assert_eq!(k.bank_index(2.0), 4);
        assert_eq!(k.bank_index(16.0), 16);
        assert_eq!(k.bank_index(64.0), 24);
        assert_eq!(k.bank_index(1000.0), 24);
        assert_eq!(k.bank(16).taps, MAX_TAPS);
        assert_eq!(k.bank(24).taps, MAX_TAPS);
        assert!(k.bytes() < 8 << 20, "{}", k.bytes());
    }
}
