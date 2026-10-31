//! Smoothing a parameter so a move during a note doesn't zipper, and glide between notes.

/// A value that follows its target through a one-pole lag, updated once per sample. Automation
/// and knob moves arrive a block apart; this turns the steps into a curve.
#[derive(Debug, Clone, Copy)]
pub struct Smoothed {
    pub value: f32,
    pub target: f32,
    coef: f32,
}

impl Smoothed {
    /// `ms` is the lag's time constant at `rate`.
    pub fn new(value: f32, ms: f32, rate: f32) -> Smoothed {
        Smoothed { value, target: value, coef: crate::fx::util::coeff(ms, rate) }
    }

    /// Lands on the target at once, for a voice not yet sounding or a render about to start.
    #[inline]
    pub fn snap(&mut self) {
        self.value = self.target;
    }

    /// Lands exactly on the target once within a hair of it, so a settled value is exact.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        let d = self.target - self.value;
        if d.abs() < 2e-5 {
            self.value = self.target;
        } else {
            self.value += d * self.coef;
        }
        self.value
    }
}

/// Portamento: a pitch (or anything) that lags its target like a capacitor charging through the
/// glide resistor. The time is how long it takes to get 95 % of the way, whatever the interval, as
/// in a synth's glide circuit.
#[derive(Debug, Clone, Copy, Default)]
pub struct Glide {
    pub value: f32,
    pub target: f32,
    coef: f32,
}

impl Glide {
    /// Sets the time, 0 for none.
    pub fn set_time(&mut self, secs: f32, rate: f32) {
        self.coef = if secs <= 0.0 { 1.0 } else { 1.0 - (-3.0 / (secs * rate)).exp() };
    }

    /// Moves on one sample.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.coef;
        self.value
    }

    /// Lands on a value at once.
    pub fn jump(&mut self, v: f32) {
        self.value = v;
        self.target = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_takes_its_time_constant() {
        let mut s = Smoothed::new(0.0, 10.0, 48000.0);
        s.target = 1.0;
        for _ in 0..480 {
            s.tick();
        }
        assert!((s.value - 0.632).abs() < 0.01, "{}", s.value);
        s.snap();
        assert_eq!(s.value, 1.0);
    }

    #[test]
    fn glide_takes_the_same_time_for_any_interval() {
        for interval in [1.0f32, 12.0, 36.0] {
            let mut g = Glide::default();
            g.set_time(0.25, 48000.0);
            g.jump(60.0);
            g.target = 60.0 + interval;
            for _ in 0..12000 {
                g.tick();
            }
            let left = (g.target - g.value) / interval;
            assert!((left - 0.05).abs() < 0.002, "{interval}: {left}");
        }
        let mut g = Glide::default();
        g.set_time(0.0, 48000.0);
        g.target = 5.0;
        assert_eq!(g.tick(), 5.0);
    }
}
