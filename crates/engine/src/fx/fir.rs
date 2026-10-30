//! Long FIR filters for the Linear Phase EQ: designing a linear-phase kernel from a magnitude
//! response, and running it by FFT convolution on the audio thread.
//!
//! **Design** ([`linear_phase_taps`]) is frequency sampling: the target magnitude is sampled on
//! an FFT grid four times the kernel's length with zero phase, transformed back (an impulse
//! response symmetric about time zero), shifted so its centre lands on the middle tap and
//! shaped by a Blackman window. The kernel has an odd number of taps, so it is exactly
//! symmetric about a whole sample: a pure delay of `(taps - 1) / 2` frames, whatever the
//! magnitude.
//!
//! **Convolution** ([`Convolver`]) is uniformly partitioned, with no latency of its own: the
//! first [`PART`] taps run directly in the time domain and every later partition in the
//! frequency domain, from the spectra of past input blocks (a frequency-domain delay line). The
//! input spectra don't depend on the kernel, so two kernels can run side by side for a
//! crossfade at the cost of only the multiply-adds.
//!
//! **Kernel changes** never allocate on the audio thread: kernels are built elsewhere (see
//! [`Kernel::new`]) and handed over whole; the convolver fades from the old kernel's output to
//! the new one's over a few blocks and gives the old kernel back for disposal.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// The partition (block) size: the directly computed head, and the hop of the FFT part.
pub const PART: usize = 128;
/// Bins in one partition's spectrum (an FFT of `2 * PART` real values).
const BINS: usize = PART + 1;

/// A symmetric window that falls to zero at both ends.
fn blackman(n: usize, len: usize) -> f64 {
    if len < 2 {
        return 1.0;
    }
    let x = std::f64::consts::TAU * n as f64 / (len - 1) as f64;
    0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos()
}

/// A linear-phase FIR of `len` taps (made odd if it isn't) whose magnitude follows `mag`, a
/// linear gain as a function of frequency in radians per sample (0 to π). Allocates.
pub fn linear_phase_taps(len: usize, mag: impl Fn(f64) -> f64) -> Vec<f32> {
    let len = len.max(1) | 1;
    let grid = (len + 1).next_power_of_two() * 4;
    let inv = RealFftPlanner::<f64>::new().plan_fft_inverse(grid);
    let mut spec = inv.make_input_vec();
    for (k, s) in spec.iter_mut().enumerate() {
        let g = mag(std::f64::consts::TAU * k as f64 / grid as f64);
        *s = Complex::new(if g.is_finite() { g } else { 0.0 }, 0.0);
    }
    let mut h = inv.make_output_vec();
    // The input is real, so the first and last bins are valid: this can't fail.
    let _ = inv.process(&mut spec, &mut h);
    let centre = (len - 1) / 2;
    (0..len)
        .map(|n| {
            let at = (n as i64 - centre as i64).rem_euclid(grid as i64) as usize;
            (h[at] / grid as f64 * blackman(n, len)) as f32
        })
        .collect()
}

/// A kernel ready for the [`Convolver`]: its first partition as time-domain taps and every
/// later one as a spectrum.
pub struct Kernel {
    /// Which parameter change it was designed for (the effect's counter).
    pub gen: u64,
    /// Its length in taps.
    pub taps: usize,
    /// Taps `0..PART`, reversed, for the direct part.
    head: Vec<f32>,
    head_zero: bool,
    /// Spectra of partitions `1..count`, `BINS` each, scaled for the inverse FFT.
    parts: Vec<Complex<f32>>,
    /// Which of those partitions have any non-zero tap (a flat EQ's kernel has one).
    live: Vec<bool>,
}

impl Kernel {
    /// Splits `taps` into partitions and transforms them. Allocates: call it off the audio
    /// thread. `fft` must be a forward FFT of `2 * PART` values.
    pub fn new(taps: &[f32], gen: u64, fft: &dyn RealToComplex<f32>) -> Box<Kernel> {
        let count = taps.len().div_ceil(PART).max(1);
        let mut head = vec![0.0f32; PART];
        for (i, h) in head.iter_mut().enumerate() {
            *h = taps.get(PART - 1 - i).copied().unwrap_or(0.0);
        }
        let head_zero = head.iter().all(|&x| x == 0.0);
        let mut parts = vec![Complex::new(0.0f32, 0.0); (count - 1) * BINS];
        let mut live = vec![false; count - 1];
        let mut buf = vec![0.0f32; 2 * PART];
        let scale = 1.0 / (2 * PART) as f32;
        for j in 1..count {
            buf.fill(0.0);
            let src = &taps[j * PART..((j + 1) * PART).min(taps.len())];
            buf[..src.len()].copy_from_slice(src);
            live[j - 1] = src.iter().any(|&x| x != 0.0);
            let out = &mut parts[(j - 1) * BINS..j * BINS];
            let _ = fft.process(&mut buf, out);
            for c in out.iter_mut() {
                *c *= scale;
            }
        }
        Box::new(Kernel { gen, taps: taps.len(), head, head_zero, parts, live })
    }

    /// How many partitions it spans.
    fn count(&self) -> usize {
        self.live.len() + 1
    }
}

/// A forward FFT of the convolver's block size, for [`Kernel::new`].
pub fn plan_forward() -> Arc<dyn RealToComplex<f32>> {
    RealFftPlanner::<f32>::new().plan_fft_forward(2 * PART)
}

/// The dot product of two equally long slices, in eight lanes so it vectorizes.
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0.0f32; 8];
    let (ca, cb) = (a.chunks_exact(8), b.chunks_exact(8));
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (x, y) in ca.zip(cb) {
        for k in 0..8 {
            acc[k] += x[k] * y[k];
        }
    }
    let mut s: f32 = acc.iter().sum();
    for (x, y) in ra.iter().zip(rb) {
        s += x * y;
    }
    s
}

/// One channel's signal state.
struct Channel {
    /// The previous input block, then the current one as it fills.
    x: Vec<f32>,
    /// Spectra of the last `max_parts` blocks (each with the block before it), a ring.
    fdl: Vec<Complex<f32>>,
    /// The tails (all partitions but the first) of the block being output, for the current
    /// kernel and the one fading in.
    tail: Vec<f32>,
    tail_next: Vec<f32>,
}

/// Stereo FFT convolution with click-free kernel changes. Everything is allocated up front;
/// `process`, `queue`, `replace` and `reset` don't allocate.
pub struct Convolver {
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    scratch: Vec<Complex<f32>>,
    time: Vec<f32>,
    acc: Vec<Complex<f32>>,
    max_parts: usize,
    /// The ring slot holding the newest block's spectrum.
    newest: usize,
    /// Where the next sample goes in the current block.
    pos: usize,
    ch: [Channel; 2],
    cur: Box<Kernel>,
    /// The kernel fading in, and the one waiting to fade in after it.
    next: Option<Box<Kernel>>,
    pending: Option<Box<Kernel>>,
    /// The crossfade's gain curve (0 to 1, a raised cosine) and how far along it is.
    fade: Vec<f32>,
    fade_at: usize,
    /// Kernels taken out of use, for the owner to dispose of off the audio thread.
    retired: Vec<Box<Kernel>>,
}

impl Convolver {
    /// A convolver for kernels up to `max_taps` long, starting with `kernel`, crossfading
    /// kernel changes over `fade_frames` (rounded to whole blocks). Allocates.
    pub fn new(max_taps: usize, kernel: Box<Kernel>, fade_frames: usize) -> Convolver {
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(2 * PART);
        let inv = planner.plan_fft_inverse(2 * PART);
        let scratch = vec![Complex::new(0.0, 0.0); fwd.get_scratch_len().max(inv.get_scratch_len())];
        let max_parts = max_taps.div_ceil(PART).max(kernel.count()).max(1);
        let channel = || Channel { x: vec![0.0; 2 * PART], fdl: vec![Complex::new(0.0, 0.0); max_parts * BINS], tail: vec![0.0; PART], tail_next: vec![0.0; PART] };
        let blocks = fade_frames.div_ceil(PART).max(1);
        let len = blocks * PART;
        let fade = (0..len).map(|i| (0.5 - 0.5 * (std::f64::consts::PI * i as f64 / len as f64).cos()) as f32).collect();
        Convolver {
            fwd,
            inv,
            scratch,
            time: vec![0.0; 2 * PART],
            acc: vec![Complex::new(0.0, 0.0); BINS],
            max_parts,
            newest: 0,
            pos: 0,
            ch: [channel(), channel()],
            cur: kernel,
            next: None,
            pending: None,
            fade,
            fade_at: 0,
            retired: Vec::with_capacity(4),
        }
    }

    /// The newest kernel it was given (running, fading in or waiting).
    pub fn newest_gen(&self) -> u64 {
        self.pending.as_ref().or(self.next.as_ref()).unwrap_or(&self.cur).gen
    }

    /// The kernel in use (after any fade under way).
    pub fn kernel_taps(&self) -> usize {
        self.next.as_ref().unwrap_or(&self.cur).taps
    }

    /// A crossfade is under way or waiting to start.
    pub fn changing(&self) -> bool {
        self.next.is_some() || self.pending.is_some()
    }

    /// Fades `k` in at the next block boundary, once any fade under way has finished. A kernel
    /// still waiting from before is returned: `k` supersedes it.
    pub fn queue(&mut self, k: Box<Kernel>) -> Option<Box<Kernel>> {
        if k.count() > self.max_parts {
            return Some(k);
        }
        self.pending.replace(k)
    }

    /// Uses `k` from now on, without a fade: for a freshly reset convolver, which has no
    /// output to fade from. Returns the kernel it replaced.
    pub fn replace(&mut self, k: Box<Kernel>) -> Box<Kernel> {
        if k.count() > self.max_parts {
            return k;
        }
        let old = std::mem::replace(&mut self.cur, k);
        for c in 0..2 {
            Self::tail_of(&self.cur, &mut self.ch[c], self.newest, self.max_parts, &self.inv, &mut self.acc, &mut self.time, &mut self.scratch, false);
        }
        old
    }

    /// Kernels no longer in use, to be disposed of off the audio thread.
    pub fn retired(&mut self) -> std::vec::Drain<'_, Box<Kernel>> {
        self.retired.drain(..)
    }

    fn retire(&mut self, k: Box<Kernel>) {
        if self.retired.len() < self.retired.capacity() {
            self.retired.push(k);
        } else {
            // Never free on the audio thread; the owner collects after every block, so this
            // doesn't happen in practice.
            std::mem::forget(k);
        }
    }

    /// Forgets the signal it holds. A kernel change under way completes at once.
    pub fn reset(&mut self) {
        for c in &mut self.ch {
            c.x.fill(0.0);
            c.fdl.fill(Complex::new(0.0, 0.0));
            c.tail.fill(0.0);
            c.tail_next.fill(0.0);
        }
        self.pos = 0;
        self.fade_at = 0;
        for k in [self.next.take(), self.pending.take()].into_iter().flatten() {
            let old = std::mem::replace(&mut self.cur, k);
            self.retire(old);
        }
    }

    /// One kernel's tail for the coming block of one channel: the inverse FFT of every
    /// partition but the first times the matching past input spectrum.
    #[allow(clippy::too_many_arguments)]
    fn tail_of(k: &Kernel, ch: &mut Channel, newest: usize, max_parts: usize, inv: &Arc<dyn ComplexToReal<f32>>, acc: &mut [Complex<f32>], time: &mut [f32], scratch: &mut [Complex<f32>], next: bool) {
        let out = if next { &mut ch.tail_next } else { &mut ch.tail };
        if !k.live.iter().any(|&l| l) {
            out.fill(0.0);
            return;
        }
        acc.fill(Complex::new(0.0, 0.0));
        for j in 1..k.count() {
            if !k.live[j - 1] {
                continue;
            }
            // Partition j meets the input block j - 1 before the newest.
            let slot = (newest + max_parts - (j - 1)) % max_parts;
            let h = &k.parts[(j - 1) * BINS..j * BINS];
            let x = &ch.fdl[slot * BINS..(slot + 1) * BINS];
            for ((a, h), x) in acc.iter_mut().zip(h).zip(x) {
                *a += h * x;
            }
        }
        acc[0].im = 0.0;
        acc[BINS - 1].im = 0.0;
        let _ = inv.process_with_scratch(acc, time, scratch);
        out.copy_from_slice(&time[PART..]);
    }

    /// A block has filled: take its spectrum, then work out the next block's tails.
    fn end_block(&mut self) {
        self.newest = (self.newest + 1) % self.max_parts;
        for c in 0..2 {
            let ch = &mut self.ch[c];
            self.time.copy_from_slice(&ch.x);
            let slot = &mut ch.fdl[self.newest * BINS..(self.newest + 1) * BINS];
            let _ = self.fwd.process_with_scratch(&mut self.time, slot, &mut self.scratch);
            ch.x.copy_within(PART.., 0);
        }
        self.pos = 0;
        // A finished fade hands over; a waiting kernel starts its fade.
        if self.next.is_some() && self.fade_at >= self.fade.len() {
            let next = self.next.take().unwrap();
            let old = std::mem::replace(&mut self.cur, next);
            self.retire(old);
        }
        if self.next.is_none() {
            if let Some(p) = self.pending.take() {
                self.next = Some(p);
                self.fade_at = 0;
            }
        }
        for c in 0..2 {
            Self::tail_of(&self.cur, &mut self.ch[c], self.newest, self.max_parts, &self.inv, &mut self.acc, &mut self.time, &mut self.scratch, false);
            if let Some(n) = &self.next {
                Self::tail_of(n, &mut self.ch[c], self.newest, self.max_parts, &self.inv, &mut self.acc, &mut self.time, &mut self.scratch, true);
            }
        }
    }

    /// Filters one block in place (`l` and `r` the same length, any length).
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        // A kernel that arrived while idle starts at the next boundary; at one already (a
        // block just ended, or a reset), start it now.
        if self.pos == 0 && self.next.is_none() && self.pending.is_some() {
            self.next = self.pending.take();
            self.fade_at = 0;
            for c in 0..2 {
                Self::tail_of(self.next.as_ref().unwrap(), &mut self.ch[c], self.newest, self.max_parts, &self.inv, &mut self.acc, &mut self.time, &mut self.scratch, true);
            }
        }
        let mut i = 0;
        while i < n {
            let take = (n - i).min(PART - self.pos);
            let pos = self.pos;
            for (c, buf) in [&mut l[i..i + take], &mut r[i..i + take]].into_iter().enumerate() {
                let ch = &mut self.ch[c];
                ch.x[PART + pos..PART + pos + take].copy_from_slice(buf);
                let cur = &self.cur;
                for (s, y) in buf.iter_mut().enumerate() {
                    let at = pos + s;
                    let window = &ch.x[at + 1..at + 1 + PART];
                    let mut v = ch.tail[at];
                    if !cur.head_zero {
                        v += dot(&cur.head, window);
                    }
                    if let Some(nk) = &self.next {
                        let mut w = ch.tail_next[at];
                        if !nk.head_zero {
                            w += dot(&nk.head, window);
                        }
                        let g = self.fade[(self.fade_at + s).min(self.fade.len() - 1)];
                        v += (w - v) * g;
                    }
                    *y = v;
                }
            }
            if self.next.is_some() {
                self.fade_at += take;
            }
            self.pos += take;
            i += take;
            if self.pos == PART {
                self.end_block();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(len: usize, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..len)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                s as f32 / u32::MAX as f32 * 2.0 - 1.0
            })
            .collect()
    }

    /// Direct convolution, for reference.
    fn direct(x: &[f32], h: &[f32]) -> Vec<f32> {
        (0..x.len()).map(|n| (0..h.len().min(n + 1)).map(|k| h[k] as f64 * x[n - k] as f64).sum::<f64>() as f32).collect()
    }

    #[test]
    fn matches_direct_convolution_in_odd_blocks() {
        let fft = plan_forward();
        let h = noise(1000, 7).iter().map(|v| v * 0.05).collect::<Vec<_>>();
        let x = noise(5000, 3);
        let mut c = Convolver::new(2048, Kernel::new(&h, 0, fft.as_ref()), 512);
        let (mut l, mut r) = (x.clone(), x.clone());
        // Blocks that don't line up with the partitions.
        let mut at = 0;
        for size in [1usize, 77, 128, 300, 1024, 5, 999].iter().cycle() {
            if at >= x.len() {
                break;
            }
            let end = (at + size).min(x.len());
            let (bl, br) = (&mut l[at..end], &mut r[at..end]);
            c.process(bl, br);
            at = end;
        }
        let want = direct(&x, &h);
        let err = l.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(err < 1e-4, "error {err}");
        assert_eq!(l, r);
    }

    #[test]
    fn linear_phase_design_is_symmetric_and_follows_the_magnitude() {
        let taps = linear_phase_taps(1001, |w| if w < 0.5 { 1.0 } else { 0.1 });
        assert_eq!(taps.len(), 1001);
        for k in 0..500 {
            assert!((taps[500 - k] - taps[500 + k]).abs() < 1e-7, "tap {k}");
        }
        // Gain at DC: the sum of the taps.
        let dc: f32 = taps.iter().sum();
        assert!((dc - 1.0).abs() < 1e-3, "{dc}");
        // A flat response is a delta at the centre.
        let flat = linear_phase_taps(255, |_| 1.0);
        assert!((flat[127] - 1.0).abs() < 1e-6 && flat.iter().enumerate().all(|(i, &t)| i == 127 || t.abs() < 1e-6));
    }

    #[test]
    fn a_kernel_change_fades_without_a_step() {
        let fft = plan_forward();
        let mut a = vec![0.0f32; 301];
        a[150] = 0.25;
        let mut b = vec![0.0f32; 301];
        b[150] = 1.0;
        let mut c = Convolver::new(512, Kernel::new(&a, 0, fft.as_ref()), 1024);
        let sine: Vec<f32> = (0..20_000).map(|i| (std::f32::consts::TAU * 500.0 * i as f32 / 48_000.0).sin()).collect();
        let (mut l, mut r) = (sine.clone(), sine.clone());
        for (k, (bl, br)) in l.chunks_mut(256).zip(r.chunks_mut(256)).enumerate() {
            if k == 30 {
                assert!(c.queue(Kernel::new(&b, 1, fft.as_ref())).is_none());
            }
            c.process(bl, br);
        }
        assert_eq!(c.retired().count(), 1, "the old kernel is handed back");
        let step = l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        // The sine's own largest step at full level is 2π·500/48000 ≈ 0.065.
        assert!(step < 0.07, "step {step}");
        // After the fade: exactly the new kernel.
        for i in 19_000..20_000 {
            assert!((l[i] - sine[i - 150]).abs() < 1e-5);
        }
    }
}
