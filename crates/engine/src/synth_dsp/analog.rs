//! "Analog character": the small differences that keep a voice from being exact.
//!
//! Everything here comes from an explicit seed. There is no global generator and no clock, so a
//! render repeats exactly, and parallel output stays bit-identical to serial. An instance's seed
//! comes from its slot's id (`InstrumentInfo::new`), which is saved in the project: reopening a
//! project gives the same drift, and a duplicated track gets a new id, so its own differences.

/// A small deterministic generator (xorshift64*), good enough for audio noise and slow drift.
#[derive(Debug, Clone, Copy)]
pub struct Rng(u64);

impl Rng {
    /// Any seed works, zero included; close seeds give unrelated streams.
    pub fn new(seed: u64) -> Rng {
        // SplitMix64's finalizer spreads seeds that differ by one.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Rng((z ^ (z >> 31)) | 1)
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 0..1.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        // The top 24 bits fit an f32's mantissa exactly.
        (self.next_u64() >> 40) as f32 * (1.0 / 16_777_216.0)
    }

    /// -1..1.
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        2.0 * self.unit() - 1.0
    }
}

/// A seed for the `index`th thing in an instance (a voice, say).
pub fn sub_seed(seed: u64, index: u64) -> u64 {
    seed.wrapping_mul(0x100_0000_01B3).wrapping_add(index.wrapping_mul(0x9E37_79B9).wrapping_add(1))
}

/// The slow wander of an oscillator or a filter: a random target every so often, approached
/// through a lag. Tick it at a control rate. The output is about -1..1.
#[derive(Debug, Clone, Copy)]
pub struct Drift {
    rng: Rng,
    value: f32,
    target: f32,
    /// Ticks until the next target.
    left: u32,
    lag: f32,
    every: f32,
}

impl Drift {
    /// `control_rate` is how often [`tick`](Self::tick) is called (Hz), and `seconds` the average
    /// time between new targets.
    pub fn new(seed: u64, control_rate: f32, seconds: f32) -> Drift {
        let mut rng = Rng::new(seed);
        let every = (control_rate * seconds).max(1.0);
        let value = rng.bipolar();
        let target = rng.bipolar();
        let left = (rng.unit() * every) as u32;
        Drift { rng, value, target, left, lag: 1.0 - (-1.0 / every).exp(), every }
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        if self.left == 0 {
            self.target = self.rng.bipolar();
            // Between half and one and a half of the average, so it never pulses.
            self.left = ((0.5 + self.rng.unit()) * self.every) as u32;
        } else {
            self.left -= 1;
        }
        self.value += (self.target - self.value) * self.lag;
        self.value
    }
}

/// One voice's fixed differences from an ideal one, as a real instrument's voices differ. Scale
/// each with how much character the patch wants; at zero the voice is exact.
#[derive(Debug, Clone, Copy)]
pub struct Tolerance {
    /// Per oscillator, -1..1: a static offset, in units a model scales to cents.
    pub osc: [f32; 3],
    /// -1..1: the filter's cutoff offset.
    pub cutoff: f32,
    /// -1..1: per filter stage.
    pub stage: [f32; 4],
    /// -1..1: the envelope timing, per envelope.
    pub env: [f32; 2],
    /// -1..1: the voice's level.
    pub level: f32,
}

impl Tolerance {
    pub fn new(seed: u64) -> Tolerance {
        let mut r = Rng::new(seed);
        let mut next = || r.bipolar();
        Tolerance { osc: [next(), next(), next()], cutoff: next(), stage: [next(), next(), next(), next()], env: [next(), next()], level: next() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_stream_and_close_seeds_dont() {
        let run = |seed| {
            let mut r = Rng::new(seed);
            (0..64).map(|_| r.next_u64()).collect::<Vec<_>>()
        };
        assert_eq!(run(1), run(1));
        assert_ne!(run(1), run(2));
        assert_ne!(run(0), run(1));
        let mut r = Rng::new(0);
        let mean: f32 = (0..100_000).map(|_| r.bipolar()).sum::<f32>() / 100_000.0;
        assert!(mean.abs() < 0.02, "{mean}");
        assert_ne!(sub_seed(3, 0), sub_seed(3, 1));
    }

    #[test]
    fn drift_wanders_slowly_within_range_and_repeats() {
        let run = |seed| {
            let mut d = Drift::new(seed, 3000.0, 0.5);
            (0..300_000).map(|_| d.tick()).collect::<Vec<_>>()
        };
        let a = run(7);
        assert_eq!(a, run(7));
        assert_ne!(a, run(8));
        assert!(a.iter().all(|v| v.abs() <= 1.0));
        let (min, max) = a.iter().fold((1.0f32, -1.0f32), |m, v| (m.0.min(*v), m.1.max(*v)));
        assert!(max - min > 0.5, "it should move: {min}..{max}");
        let biggest = a.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        assert!(biggest < 2.0 / 1500.0 * 1.5, "slow: {biggest}");
    }

    #[test]
    fn tolerances_differ_between_voices() {
        let (a, b) = (Tolerance::new(sub_seed(1, 0)), Tolerance::new(sub_seed(1, 1)));
        assert_ne!(a.osc, b.osc);
        assert_eq!(a.osc, Tolerance::new(sub_seed(1, 0)).osc);
    }
}
