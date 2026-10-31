//! Band-limited oscillators, and noise.
//!
//! The waveform is the plain ("naive") one, corrected around each discontinuity by a 4-point
//! B-spline residual (polyBLEP for a jump, polyBLAMP for a corner). Because the corrections reach
//! 2 samples before the event, the oscillator delays its output by a constant 2 samples at the rate
//! it ticks at, which makes every event alike: a free-running edge, a pulse-width edge moved by
//! PWM, and a hard-sync reset the oscillator couldn't have seen coming. Run a voice at twice the
//! output rate and the rest of the aliasing falls above the audio band too: see the measured
//! figures in the tests and in docs/ARCHITECTURE.md.
//!
//! Phase is f64 so pitch stays exact in the bottom octaves (a cent is 6e-4 of the increment, and
//! at 1 Hz an f32 phase couldn't hold that).

use super::analog::Rng;

/// A waveform. All run from -1 to 1, except a pulse's DC offset (a model's DC blocker removes it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wave {
    Sine,
    /// A triangle whose peak sits at `width` of the cycle: 0.5 is symmetric, near 1 a soft
    /// ramp that falls fast.
    Triangle { width: f32 },
    /// Rises through the cycle and jumps down.
    Saw,
    /// Falls through the cycle and jumps up.
    RevSaw,
    /// High for `width` of the cycle (0.5 is a square); clamped to 0.01..0.99.
    Pulse { width: f32 },
}

/// A jump's residual at the 4 sample positions around it. `d` is how far past the jump (in
/// samples, 0..1) the sample after it lies. These are the cubic B-spline's integral less a step.
#[inline]
fn blep(d: f32) -> [f32; 4] {
    let (d2, u) = (d * d, 1.0 - d);
    let u2 = u * u;
    const K: f32 = 1.0 / 24.0;
    [
        K * d2 * d2,
        K * (1.0 + d * (4.0 + d * (6.0 + d * (4.0 - 3.0 * d)))),
        -K * (1.0 + u * (4.0 + u * (6.0 + u * (4.0 - 3.0 * u)))),
        -K * u2 * u2,
    ]
}

/// A corner's residual at the same 4 positions, for a slope change of 1 per sample: the integral
/// of [`blep`].
#[inline]
fn blamp(d: f32) -> [f32; 4] {
    let u = 1.0 - d;
    const K5: f32 = 1.0 / 120.0;
    let inner = |v: f32| K5 + v * (1.0 + v * (2.0 + v * (2.0 + v * (1.0 - 0.6 * v)))) / 24.0;
    let fifth = |v: f32| {
        let v2 = v * v;
        K5 * v2 * v2 * v
    };
    [fifth(d), inner(d), inner(u), fifth(u)]
}

/// A band-limited oscillator.
#[derive(Debug, Clone, Copy)]
pub struct Osc {
    wave: Wave,
    phase: f64,
    /// Samples t-2, t-1, t, t+1 around the current one: the output is taken from the front once
    /// no later event can reach it.
    w: [f32; 4],
}

/// How far past a master's wrap (in samples, 0..1) the current sample lies; see [`Osc::tick`].
pub type SyncAt = Option<f32>;

impl Osc {
    pub fn new(wave: Wave) -> Osc {
        Osc { wave, phase: 0.0, w: [0.0; 4] }
    }

    pub fn set_wave(&mut self, wave: Wave) {
        self.wave = wave;
    }

    pub fn wave(&self) -> Wave {
        self.wave
    }

    /// Sets the phase (0..1) and forgets what is in the delay.
    pub fn reset(&mut self, phase: f64) {
        self.phase = phase.rem_euclid(1.0);
        self.w = [0.0; 4];
    }

    pub fn phase(&self) -> f64 {
        self.phase
    }

    /// The plain waveform at a phase.
    #[inline]
    fn naive(&self, p: f64) -> f32 {
        let p = p as f32;
        match self.wave {
            Wave::Sine => (p * std::f32::consts::TAU).sin(),
            Wave::Saw => 2.0 * p - 1.0,
            Wave::RevSaw => 1.0 - 2.0 * p,
            Wave::Pulse { width } => {
                if p < width.clamp(0.01, 0.99) {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Triangle { width } => {
                let w = width.clamp(0.01, 0.99);
                if p < w {
                    2.0 * p / w - 1.0
                } else {
                    1.0 - 2.0 * (p - w) / (1.0 - w)
                }
            }
        }
    }

    #[inline]
    fn add(&mut self, residual: [f32; 4], h: f32) {
        for (w, r) in self.w.iter_mut().zip(residual) {
            *w += h * r;
        }
    }

    /// Moves the phase on by `frac` of a sample, correcting for the events on the way. `past` is
    /// how many samples (0..1) of this sample remain after this stretch, and `inc` is in cycles per
    /// sample. Returns whether the phase wrapped, and how far past the wrap the stretch ended (in
    /// samples).
    #[inline]
    fn advance(&mut self, inc: f64, frac: f64, past: f32) -> Option<f32> {
        let from = self.phase;
        let mut p = from + inc * frac;
        let wrapped = p >= 1.0;
        if wrapped {
            p -= 1.0;
        }
        self.phase = p;
        let per = 1.0 / inc;
        // Samples since the point `at` (0..1 of a cycle) that the phase just went by.
        let since = |at: f64| ((if wrapped && at > p { p + 1.0 - at } else { p - at }) * per) as f32;
        let wrap_d = if wrapped { Some(((p * per) as f32).min(1.0)) } else { None };
        // A point in the phase passed in this stretch: it lies in (from, p], allowing for a wrap.
        let passed = |at: f64| if wrapped { at > from || at <= p } else { at > from && at <= p };
        match self.wave {
            Wave::Sine => {}
            Wave::Saw => {
                if let Some(d) = wrap_d {
                    self.add(blep((d + past).min(1.0)), -2.0);
                }
            }
            Wave::RevSaw => {
                if let Some(d) = wrap_d {
                    self.add(blep((d + past).min(1.0)), 2.0);
                }
            }
            Wave::Pulse { width } => {
                let w = width.clamp(0.01, 0.99) as f64;
                if let Some(d) = wrap_d {
                    self.add(blep((d + past).min(1.0)), 2.0);
                }
                if passed(w) {
                    self.add(blep((since(w) + past).clamp(0.0, 1.0)), -2.0);
                }
            }
            Wave::Triangle { width } => {
                let w = width.clamp(0.01, 0.99) as f64;
                let (up, down) = (2.0 * inc as f32 / w as f32, -2.0 * inc as f32 / (1.0 - w) as f32);
                if let Some(d) = wrap_d {
                    self.add(blamp((d + past).min(1.0)), up - down);
                }
                if passed(w) {
                    self.add(blamp((since(w) + past).clamp(0.0, 1.0)), down - up);
                }
            }
        }
        wrap_d
    }

    /// One sample at `inc` cycles per sample (0 < inc < 0.5). With `sync`, the phase resets to
    /// zero that many samples ago (0..1): pass the master's return value from the same tick for
    /// hard sync. Returns the output and, if the phase wrapped, how long ago.
    #[inline]
    pub fn tick(&mut self, inc: f64, sync: SyncAt) -> (f32, SyncAt) {
        let inc = inc.clamp(1e-7, 0.49);
        self.w = [self.w[1], self.w[2], self.w[3], 0.0];
        let wrapped = match sync {
            None => self.advance(inc, 1.0, 0.0),
            Some(d) => {
                let d = d.clamp(0.0, 1.0);
                self.advance(inc, 1.0 - d as f64, d);
                // The jump (and the corner) of landing back on zero.
                let before = self.naive(self.phase);
                let after = self.naive(0.0);
                self.add(blep(d), after - before);
                self.phase = 0.0;
                let a = self.advance(inc, d as f64, 0.0);
                Some(a.unwrap_or(d))
            }
        };
        let v = self.naive(self.phase);
        self.w[2] += v;
        (self.w[0], wrapped)
    }

    /// The samples' delay at the oscillator's own rate. Constant, so sync and mixing stay in step.
    pub const LATENCY: usize = 2;
}

/// White or pink noise from a deterministic generator.
#[derive(Debug, Clone, Copy)]
pub struct Noise {
    rng: Rng,
    b: [f32; 3],
}

impl Noise {
    pub fn new(seed: u64) -> Noise {
        Noise { rng: Rng::new(seed), b: [0.0; 3] }
    }

    /// Uniform white noise, -1..1.
    #[inline]
    pub fn white(&mut self) -> f32 {
        self.rng.bipolar()
    }

    /// Pink (-3 dB per octave) noise by Paul Kellet's three-pole filter; about ±1 at its peaks.
    #[inline]
    pub fn pink(&mut self) -> f32 {
        let w = self.rng.bipolar();
        let b = &mut self.b;
        b[0] = 0.99765 * b[0] + w * 0.0990460;
        b[1] = 0.96300 * b[1] + w * 0.2965164;
        b[2] = 0.57000 * b[2] + w * 1.0526913;
        (b[0] + b[1] + b[2] + w * 0.1848) * 0.2
    }
}

#[cfg(test)]
mod tests {
    use super::super::resample::Decimator2x;
    use super::super::test_util::*;
    use super::*;

    /// A block of an oscillator at exactly `bin` cycles over `n` output samples, after settling,
    /// run at `os` times the output rate.
    fn render(wave: Wave, bin: usize, n: usize, os: usize) -> Vec<f32> {
        let inc = bin as f64 / n as f64 / os as f64;
        let mut o = Osc::new(wave);
        let mut d = Decimator2x::new();
        let mut out = Vec::with_capacity(n);
        for k in 0..2 * n {
            let v = if os == 2 {
                let a = o.tick(inc, None).0;
                let b = o.tick(inc, None).0;
                d.process(a, b)
            } else {
                o.tick(inc, None).0
            };
            if k >= n {
                out.push(v);
            }
        }
        out
    }

    /// The largest thing in the band up to `band` of the output rate that isn't a harmonic, in dB
    /// against the fundamental.
    fn alias_db(x: &[f32], bin: usize, band: f64) -> f64 {
        let s = spectrum(x);
        let top = (band * x.len() as f64) as usize;
        let worst = s[1..top].iter().enumerate().filter(|(i, _)| (i + 1) % bin != 0).map(|(_, v)| *v).fold(0.0, f64::max);
        db(worst / s[bin])
    }

    /// `bin` over 2^16 samples nearest a frequency, odd so its harmonics don't sit on one another's
    /// aliases.
    fn bin_of(hz: f64, sr: f64) -> usize {
        ((hz / sr * 65536.0) as usize) | 1
    }

    #[test]
    fn aliasing_is_measured_low() {
        const N: usize = 1 << 16;
        // At 2x, in the audio band, across the top of the keyboard's range.
        for &(hz, limit) in &[(1046.5, -80.0), (2093.0, -80.0), (4186.0, -75.0), (8372.0, -60.0)] {
            let bin = bin_of(hz, 48000.0);
            let saw = alias_db(&render(Wave::Saw, bin, N, 2), bin, 20000.0 / 48000.0);
            let pulse = alias_db(&render(Wave::Pulse { width: 0.3 }, bin, N, 2), bin, 20000.0 / 48000.0);
            let tri = alias_db(&render(Wave::Triangle { width: 0.5 }, bin, N, 2), bin, 20000.0 / 48000.0);
            println!("{hz} Hz at 48 kHz, 2x: saw {saw:.1} dB, pulse {pulse:.1} dB, triangle {tri:.1} dB");
            assert!(saw < limit, "saw at {hz}: {saw}");
            assert!(pulse < limit, "pulse at {hz}: {pulse}");
            assert!(tri < limit, "triangle at {hz}: {tri}");
        }
    }

    #[test]
    fn band_limiting_beats_a_naive_saw_by_a_lot() {
        const N: usize = 1 << 16;
        let bin = bin_of(4186.0, 48000.0);
        let inc = bin as f64 / N as f64;
        let naive: Vec<f32> = (0..N).map(|k| 2.0 * ((k as f64 * inc).fract() as f32) - 1.0).collect();
        let plain = alias_db(&naive, bin, 20000.0 / 48000.0);
        let one_x = alias_db(&render(Wave::Saw, bin, N, 1), bin, 20000.0 / 48000.0);
        let two_x = alias_db(&render(Wave::Saw, bin, N, 2), bin, 20000.0 / 48000.0);
        println!("4186 Hz saw at 48 kHz: naive {plain:.1} dB, polyBLEP {one_x:.1} dB, polyBLEP at 2x {two_x:.1} dB");
        assert!(plain > -32.0, "the naive saw should alias: {plain}");
        assert!(one_x < plain - 15.0);
        assert!(two_x < one_x - 20.0);
    }

    #[test]
    fn tuned_at_every_rate() {
        for &sr in &[44100.0f32, 48000.0, 96000.0] {
            for note in [21.0f32, 33.0, 45.0, 60.0, 69.0, 84.0, 96.0, 108.0] {
                let hz = super::super::note_hz(note);
                let inc = (hz / sr) as f64;
                let mut o = Osc::new(Wave::Saw);
                let x: Vec<f32> = (0..1 << 17).map(|_| o.tick(inc, None).0).collect();
                let got = pitch_hz(&x, sr);
                // The saw's strongest partial is the fundamental.
                assert!(cents(got, hz).abs() < 0.5, "note {note} at {sr}: {got} vs {hz}");
            }
        }
    }

    #[test]
    fn hard_sync_stays_clean_and_at_the_masters_pitch() {
        const N: usize = 1 << 16;
        let bin = bin_of(1000.0, 48000.0);
        let inc = bin as f64 / N as f64 / 2.0;
        let (mut m, mut s) = (Osc::new(Wave::Saw), Osc::new(Wave::Saw));
        let mut d = Decimator2x::new();
        let mut out = Vec::new();
        let tick = |m: &mut Osc, s: &mut Osc| {
            let (_, wrap) = m.tick(inc, None);
            s.tick(inc * 2.37, wrap).0
        };
        for k in 0..2 * N {
            let a = tick(&mut m, &mut s);
            let b = tick(&mut m, &mut s);
            let v = d.process(a, b);
            if k >= N {
                out.push(v);
            }
        }
        // Everything is a harmonic of the master, even though the slave runs 2.37 times as fast.
        let a = alias_db(&out, bin, 20000.0 / 48000.0);
        println!("hard sync 1 kHz, ratio 2.37, 2x: {a:.1} dB");
        assert!(a < -60.0, "{a}");
    }

    #[test]
    fn stays_in_range_on_fast_and_odd_settings() {
        for wave in [Wave::Saw, Wave::RevSaw, Wave::Sine, Wave::Pulse { width: 0.005 }, Wave::Pulse { width: 0.995 }, Wave::Triangle { width: 0.999 }] {
            let mut o = Osc::new(wave);
            for k in 0..200_000 {
                let inc = 1e-6 + 0.6 * (k as f64 * 3e-4).sin().abs();
                let (v, _) = o.tick(inc, None);
                assert!(v.is_finite() && v.abs() < 2.5, "{wave:?}: {v}");
            }
        }
    }

    #[test]
    fn noise_repeats_with_its_seed() {
        let run = |seed| {
            let mut n = Noise::new(seed);
            (0..1000).map(|_| n.pink() + n.white()).sum::<f32>()
        };
        assert_eq!(run(5), run(5));
        assert_ne!(run(5), run(6));
    }
}
