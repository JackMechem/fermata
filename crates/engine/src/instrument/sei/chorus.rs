//! The bucket-brigade stereo chorus that is half of this instrument's sound.
//!
//! One triangle LFO sweeps two delay lines in opposite directions, one per side. The dry signal
//! goes to both sides, so the two sides' pitch moves in opposite directions. Three modes: I and II
//! are slow and deep, I+II fast and shallow. The figures are the widely repeated measurements of
//! the earlier sibling instrument's board (delay 1.66 to 5.35 ms at 0.513 and 0.863 Hz, 3.28 to
//! 3.82 ms at 9.75 Hz); nothing here was measured from a real unit.
//!
//! The bucket-brigade's character is modelled as: an anti-aliasing low-pass going in, gentle
//! saturation at the charge transfer, a reconstruction low-pass coming out, and hiss added at the
//! output. Clock bleed isn't: it lies above the audio band. Neither are the sample-and-hold steps:
//! at these delays the clock runs from 24 to 77 kHz, so they are above the filters' corners.

use crate::synth_dsp::analog::{sub_seed, Rng};
use crate::synth_dsp::osc::Noise;
use crate::synth_dsp::smooth::Smoothed;
use crate::synth_dsp::tanh_fast;

/// Per mode: shortest delay and longest in ms, and the LFO in Hz. "Off" keeps I's numbers, so a
/// chorus fading out doesn't sweep anywhere.
const MODES: [(f32, f32, f32); 4] = [(1.66, 5.35, 0.513), (1.66, 5.35, 0.513), (1.66, 5.35, 0.863), (3.28, 3.82, 9.75)];
/// The filters either side of the bucket brigade, in Hz. A guess in the range such boards use.
const BBD_LP_HZ: f32 = 9000.0;
/// Hiss at Chorus Noise 100 %. A guess: audible in a quiet room, about -70 dB against a loud note,
/// which is how players describe it.
const HISS: f32 = 6e-4;
const MODE_LAG_MS: f32 = 30.0;

#[derive(Clone, Copy)]
struct TwoPole {
    a: f32,
    z: [f32; 2],
}

impl TwoPole {
    fn new(hz: f32, sr: f32) -> TwoPole {
        TwoPole { a: 1.0 - (-std::f32::consts::TAU * hz / sr).exp(), z: [0.0; 2] }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.z[0] += (x - self.z[0]) * self.a;
        self.z[1] += (self.z[0] - self.z[1]) * self.a;
        self.z[1]
    }
}

pub struct Chorus {
    sr: f32,
    seed: u64,
    buf: Vec<f32>,
    mask: usize,
    w: usize,
    phase: f64,
    inc: f64,
    lo: Smoothed,
    hi: Smoothed,
    /// 0 off to 1 on.
    amount: Smoothed,
    /// 0 quiet to 1 hissing, with the voices.
    hiss: Smoothed,
    input_lp: TwoPole,
    output_lp: [TwoPole; 2],
    noise: [Noise; 2],
    noise_lp: [TwoPole; 2],
}

impl Chorus {
    pub fn new(sr: f32, seed: u64) -> Chorus {
        // Twice the longest delay, in a power of two.
        let len = ((sr * 0.012) as usize + 8).next_power_of_two();
        let mut rng = Rng::new(sub_seed(seed, 1));
        Chorus {
            sr,
            seed,
            buf: vec![0.0; len],
            mask: len - 1,
            w: 0,
            phase: rng.unit() as f64,
            inc: MODES[1].2 as f64 / sr as f64,
            lo: Smoothed::new(MODES[1].0, MODE_LAG_MS, sr),
            hi: Smoothed::new(MODES[1].1, MODE_LAG_MS, sr),
            amount: Smoothed::new(0.0, 8.0, sr),
            hiss: Smoothed::new(0.0, 40.0, sr),
            input_lp: TwoPole::new(BBD_LP_HZ, sr),
            output_lp: [TwoPole::new(BBD_LP_HZ, sr); 2],
            noise: [Noise::new(sub_seed(seed, 2)), Noise::new(sub_seed(seed, 3))],
            noise_lp: [TwoPole::new(BBD_LP_HZ, sr); 2],
        }
    }

    pub fn set_mode(&mut self, mode: usize) {
        let (lo, hi, hz) = MODES[mode.min(3)];
        self.lo.target = lo;
        self.hi.target = hi;
        self.inc = hz as f64 / self.sr as f64;
        self.amount.target = if mode == 0 { 0.0 } else { 1.0 };
    }

    /// Back to as new, keeping the mode.
    pub fn reset(&mut self) {
        let (mode_inc, lo, hi, amount) = (self.inc, self.lo.target, self.hi.target, self.amount.target);
        self.clear();
        self.w = 0;
        self.phase = Rng::new(sub_seed(self.seed, 1)).unit() as f64;
        self.noise = [Noise::new(sub_seed(self.seed, 2)), Noise::new(sub_seed(self.seed, 3))];
        self.inc = mode_inc;
        self.lo.target = lo;
        self.hi.target = hi;
        self.amount.target = amount;
        self.lo.snap();
        self.hi.snap();
        self.amount.snap();
    }

    /// Everything played is forgotten; the LFO and noise generators run on.
    pub fn clear(&mut self) {
        self.buf.fill(0.0);
        self.input_lp.z = [0.0; 2];
        for f in self.output_lp.iter_mut().chain(self.noise_lp.iter_mut()) {
            f.z = [0.0; 2];
        }
        self.hiss.target = 0.0;
        self.hiss.snap();
    }

    pub fn snap_settings(&mut self) {
        self.lo.snap();
        self.hi.snap();
        self.amount.snap();
    }

    /// The samples of rest went by: only the LFO cares.
    pub fn skip(&mut self, samples: u64) {
        self.phase = (self.phase + samples as f64 * self.inc).fract();
    }

    /// The LFO, -1..1.
    #[inline]
    fn tri(&self) -> f32 {
        1.0 - 4.0 * (self.phase as f32 - 0.5).abs()
    }

    /// The current delays in ms, left and right.
    pub fn delays_ms(&self) -> (f32, f32) {
        let (c, d) = (0.5 * (self.lo.value + self.hi.value), 0.5 * (self.hi.value - self.lo.value));
        let t = self.tri();
        (c + d * t, c - d * t)
    }

    #[inline]
    fn read(&self, delay_samples: f32) -> f32 {
        // Four-point Hermite: a moving delay read linearly would dull and warble the highs.
        let at = self.w as f32 + self.buf.len() as f32 - delay_samples;
        let i = at as usize;
        let f = at - i as f32;
        let m = self.mask;
        let (a, b, c, d) = (self.buf[(i + m) & m], self.buf[i & m], self.buf[(i + 1) & m], self.buf[(i + 2) & m]);
        let c1 = 0.5 * (c - a);
        let c2 = a - 2.5 * b + 2.0 * c - 0.5 * d;
        let c3 = 0.5 * (d - a) + 1.5 * (b - c);
        b + f * (c1 + f * (c2 + f * c3))
    }

    /// One sample. `gate` is whether there's something playing for the hiss to go with, and
    /// `noise` the Chorus Noise knob.
    #[inline]
    pub fn tick(&mut self, x: f32, gate: bool, noise: f32) -> (f32, f32) {
        let amount = self.amount.tick();
        self.hiss.target = if gate { 1.0 } else { 0.0 };
        let hiss = self.hiss.tick() * noise * HISS * amount;
        let (lo, hi) = (self.lo.tick(), self.hi.tick());
        let t = self.tri();
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        // Into the buckets.
        let v = self.input_lp.tick(x);
        // A bucket brigade runs out of headroom softly, on one side a little sooner.
        let v = tanh_fast(v * 0.9) / 0.9;
        let v = v + 0.015 * v * v;
        self.buf[self.w] = v;
        let (c, d) = (0.5 * (lo + hi), 0.5 * (hi - lo));
        let ms = 0.001 * self.sr;
        let w_l = self.output_lp[0].tick(self.read((c + d * t) * ms));
        let w_r = self.output_lp[1].tick(self.read((c - d * t) * ms));
        self.w = (self.w + 1) & self.mask;
        let n_l = self.noise_lp[0].tick(self.noise[0].white());
        let n_r = self.noise_lp[1].tick(self.noise[1].white());
        // It gets a little louder with the chorus on: most of that kept, the peaks taken in.
        let g = 1.0 - 0.15 * amount;
        (g * (x + amount * w_l) + hiss * n_l, g * (x + amount * w_r) + hiss * n_r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn chorus(mode: usize) -> Chorus {
        let mut c = Chorus::new(SR, 3);
        c.set_mode(mode);
        c.snap_settings();
        c
    }

    #[test]
    fn delay_ranges_and_rates_are_each_modes() {
        for (mode, lo, hi, hz) in [(1usize, 1.66f32, 5.35f32, 0.513f32), (2, 1.66, 5.35, 0.863), (3, 3.28, 3.82, 9.75)] {
            let mut c = chorus(mode);
            let n = (SR * 8.0) as usize;
            let (mut min, mut max) = (f32::MAX, f32::MIN);
            let mut last = 0.0f32;
            let mut peaks = 0;
            let mut rising = false;
            for _ in 0..n {
                let (l, r) = c.delays_ms();
                // Always opposite each other about the middle.
                assert!(((l + r) * 0.5 - (lo + hi) * 0.5).abs() < 1e-3);
                min = min.min(l);
                max = max.max(l);
                if l < last && rising {
                    peaks += 1;
                }
                rising = l > last;
                last = l;
                c.tick(0.0, false, 0.0);
            }
            assert!((min - lo).abs() < 0.02 && (max - hi).abs() < 0.02, "mode {mode}: {min}..{max}");
            let got = peaks as f32 / 8.0;
            assert!((got - hz).abs() < 0.2, "mode {mode}: {got} Hz");
        }
    }

    /// Where an impulse comes out, in samples, on each side.
    fn impulse_delay(c: &mut Chorus) -> (usize, usize) {
        let (mut bl, mut br) = ((0usize, 0.0f32), (0usize, 0.0f32));
        for i in 0..1024 {
            let (l, r) = c.tick(if i == 0 { 1.0 } else { 0.0 }, false, 0.0);
            // Skip the dry impulse itself.
            if i > 8 {
                if l.abs() > bl.1 {
                    bl = (i, l.abs());
                }
                if r.abs() > br.1 {
                    br = (i, r.abs());
                }
            }
        }
        (bl.0, br.0)
    }

    #[test]
    fn an_impulse_comes_out_where_the_lfo_has_the_delay() {
        let mut c = chorus(1);
        // A quarter of the way round from a known phase: left long, right short.
        c.phase = 0.5;
        let (l, r) = impulse_delay(&mut c);
        let (lm, rm) = (l as f32 / SR * 1000.0, r as f32 / SR * 1000.0);
        println!("impulse with the LFO at its top: left {lm:.2} ms, right {rm:.2} ms");
        // The filters add about a sample or two.
        assert!((lm - 5.35).abs() < 0.2, "{lm}");
        assert!((rm - 1.66).abs() < 0.2, "{rm}");
    }

    #[test]
    fn off_passes_dry_and_both_sides_alike() {
        let mut c = chorus(0);
        for i in 0..4800 {
            let x = (i as f32 * 0.05).sin();
            let (l, r) = c.tick(x, true, 1.0);
            assert_eq!(l, x);
            assert_eq!(r, x);
        }
    }

    #[test]
    fn it_widens_and_hisses_only_when_on_and_gated() {
        let mut c = chorus(1);
        let mut diff = 0.0f32;
        for i in 0..48000 {
            let (l, r) = c.tick(0.5 * (i as f32 * 0.05).sin(), true, 0.0);
            diff = diff.max((l - r).abs());
        }
        assert!(diff > 0.1, "{diff}");
        // Noise: a level around what's asked for, gone without the gate.
        let mut c = chorus(1);
        let level = |c: &mut Chorus, gate: bool| {
            let mut p = 0.0f32;
            for _ in 0..24000 {
                p = p.max(c.tick(0.0, gate, 1.0).0.abs());
            }
            p
        };
        let on = level(&mut c, true);
        assert!(on > 1e-4 && on < 2e-3, "{on}");
        let off = level(&mut c, false);
        let off = off.min(level(&mut c, false));
        assert!(off < 1e-6, "{off}");
    }

    #[test]
    fn delay_sweep_shifts_the_two_sides_opposite_ways() {
        // A steady sine through the wet path only: with the delay growing, pitch falls.
        let mut c = chorus(3);
        // The window below runs up one side of the triangle.
        c.phase = 0.82;
        let hz = 4000.0;
        let n = 2200; // Under half a cycle of the 9.75 Hz LFO.
        let (mut wl, mut wr) = (Vec::new(), Vec::new());
        for i in 0..n + 1000 {
            let x = (std::f32::consts::TAU * hz * i as f32 / SR).sin();
            let (l, r) = c.tick(x, false, 0.0);
            let g = 0.85;
            if i >= 1000 {
                wl.push(l / g - x);
                wr.push(r / g - x);
            }
        }
        let rate = |w: &[f32]| w.windows(2).filter(|p| p[0] < 0.0 && p[1] >= 0.0).count();
        assert_ne!(rate(&wl), rate(&wr), "{} {}", rate(&wl), rate(&wr));
    }
}
