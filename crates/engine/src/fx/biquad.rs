//! A small RBJ-cookbook biquad for effects: transposed direct form II in f64, so low cutoffs
//! stay accurate and coefficient changes don't click (the state carries over).

use std::f64::consts::{FRAC_1_SQRT_2, TAU};

#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    s1: f64,
    s2: f64,
}

impl Default for Biquad {
    /// A wire: passes the signal unchanged.
    fn default() -> Self {
        Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, s1: 0.0, s2: 0.0 }
    }
}

/// The Butterworth Q, for a maximally flat 12 dB/oct slope.
pub const BUTTERWORTH_Q: f64 = FRAC_1_SQRT_2;

impl Biquad {
    fn set(&mut self, b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) {
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// Makes this a 12 dB/oct low-pass at `freq`, keeping the filter's state.
    pub fn lowpass(&mut self, sr: f64, freq: f64, q: f64) {
        let w = TAU * (freq / sr).clamp(1e-5, 0.49);
        let (c, alpha) = (w.cos(), w.sin() / (2.0 * q));
        self.set((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }

    /// Makes this a 12 dB/oct high-pass at `freq`, keeping the filter's state.
    pub fn highpass(&mut self, sr: f64, freq: f64, q: f64) {
        let w = TAU * (freq / sr).clamp(1e-5, 0.49);
        let (c, alpha) = (w.cos(), w.sin() / (2.0 * q));
        self.set((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }

    /// Makes this a wire again.
    pub fn bypass(&mut self) {
        self.set(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        // Flush denormals so a filter ringing down to silence stays cheap.
        if self.s1.abs() < 1e-25 {
            self.s1 = 0.0;
        }
        if self.s2.abs() < 1e-25 {
            self.s2 = 0.0;
        }
        y as f32
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}
