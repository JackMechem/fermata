//! Small pieces the modulation and distortion effects share: an LFO, a parameter smoother and
//! the soft safety limit their feedback loops run through.

use std::f64::consts::TAU;

/// A low-frequency oscillator: a phase that runs from 0 to 1 once per cycle. The rate is given
/// each sample, so rate changes keep the phase continuous and never click.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Lfo {
    phase: f64,
}

impl Lfo {
    pub(crate) fn phase(&self) -> f64 {
        self.phase
    }

    pub(crate) fn set_phase(&mut self, phase: f64) {
        self.phase = phase.rem_euclid(1.0);
    }

    /// Moves on by `cycles` (the rate in Hz over the sample rate).
    #[inline]
    pub(crate) fn advance(&mut self, cycles: f64) {
        self.phase += cycles;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
    }
}

/// A sine LFO's value at `phase` (cycles), scaled to 0..1 and starting at its top, so a
/// modulation at phase 0 is at its peak (the way a synced LFO lands on the beat).
#[inline]
pub(crate) fn unipolar_sine(phase: f64) -> f32 {
    (0.5 + 0.5 * (TAU * phase).cos()) as f32
}

/// A one-pole smoother that lands exactly on its target, so a control at rest leaves the signal
/// bit-exact (mix at 0, depth at 0).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Smoothed {
    v: f32,
}

impl Smoothed {
    pub(crate) fn snap(&mut self, target: f32) {
        self.v = target;
    }

    pub(crate) fn get(&self) -> f32 {
        self.v
    }

    /// One step towards `target` with coefficient `k` (see [`super::util::coeff`]).
    #[inline]
    pub(crate) fn next(&mut self, target: f32, k: f32) -> f32 {
        let d = target - self.v;
        self.v = if d.abs() < 1e-6 { target } else { self.v + d * k };
        self.v
    }
}

/// A transparent safety limit for feedback loops: unchanged up to ±1.5 (+3.5 dBFS), then
/// rounded off towards ±2.5, so a loop pushed to its limit can't run away.
#[inline]
pub(crate) fn guard(x: f32) -> f32 {
    let a = x.abs();
    if a <= 1.5 {
        x
    } else {
        x.signum() * (1.5 + (a - 1.5).tanh())
    }
}

/// Flushes values below −300 dB to zero, so a decaying loop never reaches subnormal floats.
#[inline]
pub(crate) fn flush(x: f32) -> f32 {
    if x.abs() < 1e-15 {
        0.0
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfo_wraps_and_starts_at_the_top() {
        let mut l = Lfo::default();
        for _ in 0..1000 {
            l.advance(0.0137);
        }
        assert!((l.phase() - (13.7f64).fract()).abs() < 1e-9);
        l.advance(2.5);
        assert!(l.phase() >= 0.0 && l.phase() < 1.0);
        assert_eq!(unipolar_sine(0.0), 1.0);
        assert!(unipolar_sine(0.5).abs() < 1e-7);
        l.set_phase(-0.25);
        assert!((l.phase() - 0.75).abs() < 1e-12);
    }

    #[test]
    fn smoother_lands_exactly() {
        let mut s = Smoothed::default();
        s.snap(0.3);
        for _ in 0..10_000 {
            s.next(0.0, 0.01);
        }
        assert_eq!(s.get(), 0.0);
    }

    #[test]
    fn guard_is_transparent_then_bounded() {
        assert_eq!(guard(1.2), 1.2);
        assert_eq!(guard(-1.5), -1.5);
        assert!(guard(1e9) <= 2.5 && guard(-1e9) >= -2.5);
        assert!(guard(3.0) > guard(2.0));
    }
}
