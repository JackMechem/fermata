//! The 2:1 decimator at the end of an oversampled voice bus.
//!
//! A polyphase half-band of allpasses (Valenzuela and Constantinides, in Laurent de Soras' form):
//! two chains, each 4 first-order allpasses at the low rate, so 8 multiplies per output sample.
//! It isn't linear-phase, but it is flat where it passes and its delay is a few output samples at
//! the bottom of the band, like any analog circuit's phase shift and far below anything a player
//! feels, with no latency for delay compensation to chase (`fx/oversample.rs`' FIRs have a fixed
//! one).
//!
//! Designed for a transition from 0.225 to 0.275 of the high rate: at 44.1 kHz input it passes to
//! 19.8 kHz and rejects from 24.3 kHz, which folds down to 19.8 kHz. Measured at about -86 dB in
//! the tests.

/// The allpass coefficients, smallest first: odd ones to one chain, even to the other.
/// `hiir`'s design with 8 coefficients and a transition bandwidth of 0.025 at the high rate.
const COEFS: [f32; 8] = [0.05176965, 0.18746027, 0.36265614, 0.535_496_8, 0.6825356, 0.7984106, 0.8887162, 0.9640071];

#[derive(Debug, Clone, Copy, Default)]
struct Chain {
    x: [f32; 4],
    y: [f32; 4],
}

impl Chain {
    #[inline]
    fn tick(&mut self, input: f32, offset: usize) -> f32 {
        let mut v = input;
        for k in 0..4 {
            let c = COEFS[2 * k + offset];
            let out = c * (v - self.y[k]) + self.x[k];
            self.x[k] = v;
            // A tail below any audio lands on exactly zero, so `at_rest` can come true.
            self.y[k] = if out.abs() < 1e-15 { 0.0 } else { out };
            v = out;
        }
        v
    }
}

/// Two samples at the high rate in, one out.
#[derive(Debug, Clone, Copy, Default)]
pub struct Decimator2x {
    a: Chain,
    b: Chain,
}

impl Decimator2x {
    pub fn new() -> Decimator2x {
        Decimator2x::default()
    }

    /// `first` is the older of the two samples.
    #[inline]
    pub fn process(&mut self, first: f32, second: f32) -> f32 {
        0.5 * (self.a.tick(second, 0) + self.b.tick(first, 1))
    }

    pub fn reset(&mut self) {
        *self = Decimator2x::default();
    }

    /// Whether nothing is left inside: running it on zeros would only give zeros.
    pub fn at_rest(&self) -> bool {
        let quiet = |c: &Chain| c.x.iter().chain(&c.y).all(|v| *v == 0.0);
        quiet(&self.a) && quiet(&self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::db;
    use super::*;

    /// The gain at `f` (of the high rate).
    fn gain(f: f64) -> f64 {
        let mut d = Decimator2x::new();
        let n = 40_000;
        let w = std::f64::consts::TAU * f;
        let mut power = 0.0f64;
        for i in 0..n {
            let a = ((2 * i) as f64 * w).sin() as f32;
            let b = ((2 * i + 1) as f64 * w).sin() as f32;
            let y = d.process(a, b) as f64;
            if i >= n / 2 {
                power += y * y;
            }
        }
        // Against a unit sine's power.
        (2.0 * power / (n / 2) as f64).sqrt()
    }

    #[test]
    fn passes_the_audio_band_and_rejects_what_would_fold_into_it() {
        for i in 2..45 {
            let f = 0.225 * i as f64 / 45.0;
            let g = db(gain(f));
            assert!(g.abs() < 0.05, "{f}: {g} dB");
        }
        let mut worst = -200.0f64;
        for i in 0..90 {
            let f = 0.275 + 0.2249 * i as f64 / 90.0;
            worst = worst.max(db(gain(f)));
        }
        println!("half-band stop band: {worst:.1} dB");
        assert!(worst < -80.0, "{worst}");
    }

    #[test]
    fn settles_and_resets() {
        let mut d = Decimator2x::new();
        d.process(1.0, -1.0);
        assert!(!d.at_rest());
        for _ in 0..5000 {
            d.process(0.0, 0.0);
        }
        assert!(d.at_rest());
        d.process(1.0, 1.0);
        d.reset();
        assert!(d.at_rest());
    }
}
