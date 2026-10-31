//! Zero-delay-feedback filters with real nonlinearity.
//!
//! Each is a topology-preserving (trapezoidal) discretization of its analog circuit: the feedback
//! path is solved within the sample rather than delayed by one, so cutoff and resonance stay
//! where they are set, in tune, up to a good fraction of the rate it runs at. The nonlinearity is
//! Mystran's "cheap" method: each stage's gain tanh(x)/x is taken from the state before the
//! sample, which turns the solve into a linear one per sample with no iteration, and stays stable
//! under audio-rate modulation. Run them oversampled ([`crate::synth_dsp::resample`]): they add
//! harmonics, and the estimate of the nonlinearity improves with the rate.
//!
//! To add a filter (a 2-pole state-variable, a diode ladder): write its `tick` in this file with
//! the same trapezoidal integrators and `tanh_x_dx` stage gains, implement [`ZdfFilter`] and add
//! its tuning and stability tests. A model is generic over the trait, or holds the concrete
//! filter it models.

use super::{pow2, tan_fast, tanh_x_dx};

/// What every filter offers a voice. All run in a voice's inner loop.
pub trait ZdfFilter {
    /// Sets the prewarped cutoff `g` from [`cutoff_g`] and the resonance (0 is none; each filter
    /// says where its self-oscillation starts).
    fn set(&mut self, g: f32, resonance: f32);
    /// One sample in, one out. Must stay finite and bounded for any finite input and settings.
    fn tick(&mut self, input: f32) -> f32;
    /// Clears the state.
    fn reset(&mut self);
}

/// The prewarped integrator gain tan(π·hz/rate), with `hz` kept to 45 % of the rate.
#[inline]
pub fn cutoff_g(hz: f32, rate: f32) -> f32 {
    tan_fast(std::f32::consts::PI * (hz / rate).clamp(1e-6, 0.45))
}

/// Keeps a filter's state from settling into denormals (the audio thread flushes them, a test or
/// another thread might not), and clears it should it ever stop being finite.
#[inline]
fn tidy(z: &mut f32) {
    // NaN goes the same way, as it is neither of these.
    let a = z.abs();
    if a < 1e-18 || a >= 1e6 || a.is_nan() {
        *z = 0.0;
    }
}

/// A 4-pole transistor ladder: four one-pole stages, each a differential pair that saturates, and
/// feedback from the last stage to the first, also through a differential pair. That is the
/// circuit of the classic 24 dB/octave low-pass:
///
/// ```text
/// dV1/dt = wc (tanh(in - k V4) - tanh(V1))
/// dV2/dt = wc (tanh(V1) - tanh(V2))   and so on
/// ```
///
/// `resonance` is the feedback gain k: 0 up to 4 where it starts to self-oscillate, its pitch
/// following the cutoff. Signal levels are in the differential pair's own units: about 1 is where
/// it begins to bend, so the level going in decides how much it overdrives, which is why bass
/// falls away as resonance goes up (a ladder's feedback takes it from the first stage).
#[derive(Debug, Clone, Copy)]
pub struct Ladder {
    z: [f32; 4],
    g: f32,
    k: f32,
    last_in: f32,
    /// Per-stage gain differences, near 1: component tolerances (see `analog`).
    stage: [f32; 4],
    /// The tuning trim at the resonance last set.
    trim_k: f32,
    trim_st: f32,
}

impl Default for Ladder {
    fn default() -> Self {
        Ladder { z: [0.0; 4], g: 0.1, k: 0.0, last_in: 0.0, stage: [1.0; 4], trim_k: 0.0, trim_st: 0.0 }
    }
}

impl Ladder {
    /// Sets each stage's gain ratio. Small differences (a percent or so) leave the pitch of its
    /// self-oscillation near the cutoff and detail the sound slightly.
    pub fn set_tolerance(&mut self, stage: [f32; 4]) {
        self.stage = stage;
    }
}

impl ZdfFilter for Ladder {
    /// Past k = 4 a real ladder's self-oscillation falls flat as it saturates harder (by 70 cents
    /// at k = 5, measured on this model), the way a circuit is trimmed to keep it from doing. So do
    /// we: the cutoff rises by a fit of the measured shortfall, which keeps the whistle in tune.
    #[inline]
    fn set(&mut self, g: f32, resonance: f32) {
        self.k = resonance;
        self.g = if resonance > 4.0 {
            if resonance != self.trim_k {
                self.trim_k = resonance;
                self.trim_st = 1.3 * (1.0 - pow2((4.0 - resonance) * 1.163));
            }
            // Less is needed towards the top of the range: another fit.
            let c = self.trim_st * (1.0 - 3.1 * g).max(0.0);
            g * (1.0 + c * (0.057_76 + c * 0.001_67))
        } else {
            g
        };
    }

    #[inline]
    fn tick(&mut self, input: f32) -> f32 {
        let [z0, z1, z2, z3] = self.z;
        let k = self.k;
        // Half way through the sample (the method's estimate of the input).
        let mid = 0.5 * (input + self.last_in);
        self.last_in = input;
        // The stage gains, from the state before this sample.
        let t0 = tanh_x_dx(mid - k * z3);
        let t1 = tanh_x_dx(z0);
        let t2 = tanh_x_dx(z1);
        let t3 = tanh_x_dx(z2);
        let t4 = tanh_x_dx(z3);
        let [f0, f1, f2, f3] = [self.g * self.stage[0], self.g * self.stage[1], self.g * self.stage[2], self.g * self.stage[3]];
        let g0 = 1.0 / (1.0 + f0 * t1);
        let g1 = 1.0 / (1.0 + f1 * t2);
        let g2 = 1.0 / (1.0 + f2 * t3);
        let g3 = 1.0 / (1.0 + f3 * t4);
        // Each stage's dependence on the one before.
        let a0 = g0 * f0 * t0;
        let a1 = g1 * f1 * t1;
        let a2 = g2 * f2 * t2;
        let a3 = g3 * f3 * t3;
        // Solve the feedback within the sample.
        let y3 = (g3 * z3 + a3 * (g2 * z2 + a2 * (g1 * z1 + a1 * (g0 * z0 + a0 * input)))) / (1.0 + k * a0 * a1 * a2 * a3);
        let xx = t0 * (input - k * y3);
        let y0 = g0 * (z0 + f0 * xx);
        let y1 = g1 * (z1 + f1 * t1 * y0);
        let y2 = g2 * (z2 + f2 * t2 * y1);
        self.z[0] = z0 + 2.0 * f0 * (xx - t1 * y0);
        self.z[1] = z1 + 2.0 * f1 * (t1 * y0 - t2 * y1);
        self.z[2] = z2 + 2.0 * f2 * (t2 * y1 - t3 * y2);
        self.z[3] = z3 + 2.0 * f3 * (t3 * y2 - t4 * y3);
        for z in &mut self.z {
            tidy(z);
        }
        y3
    }

    fn reset(&mut self) {
        self.z = [0.0; 4];
        self.last_in = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::*;
    use super::*;

    /// Lets the ladder ring with no input, from a small kick, and returns the settled signal.
    fn self_oscillation(hz: f32, rate: f32, k: f32, seconds: f32) -> Vec<f32> {
        let mut f = Ladder::default();
        f.set(cutoff_g(hz, rate), k);
        let n = (seconds * rate) as usize;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let input = if i < 8 { 0.2 } else { 0.0 };
            out.push(f.tick(input));
        }
        out.split_off(n / 2)
    }

    #[test]
    fn self_oscillates_in_tune_across_the_range() {
        // A ladder at 2x, as a voice runs it.
        for &sr in &[44100.0f32, 48000.0, 96000.0] {
            let rate = 2.0 * sr;
            let mut worst = 0.0f32;
            for note in (24..=96).step_by(6) {
                let hz = super::super::note_hz(note as f32);
                let x = self_oscillation(hz, rate, 5.0, 2.5);
                let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(peak > 0.2, "{hz} Hz at {sr}: no self-oscillation (peak {peak})");
                let got = pitch_hz(&x, rate);
                let c = cents(got, hz);
                worst = worst.max(c.abs());
                assert!(c.abs() < 5.0, "{hz} Hz at {sr}: {got} Hz, {c} cents");
            }
            println!("ladder self-oscillation at {sr} x2, 24..96: worst {worst:.2} cents");
        }
    }

    #[test]
    fn doesnt_self_oscillate_below_k_4() {
        let x = self_oscillation(1000.0, 96000.0, 3.7, 1.5);
        assert!(x.iter().all(|v| v.abs() < 1e-3), "{}", x.iter().fold(0.0f32, |m, v| m.max(v.abs())));
    }

    #[test]
    fn rolls_off_24_db_per_octave_and_passes_dc() {
        let rate = 96000.0;
        let level = |hz: f32| {
            let mut f = Ladder::default();
            f.set(cutoff_g(1000.0, rate), 0.0);
            let n = 1 << 16;
            let x: Vec<f32> = (0..n).map(|i| f.tick(0.01 * (std::f64::consts::TAU * hz as f64 * i as f64 / rate as f64).sin() as f32)).collect();
            x[n / 2..].iter().fold(0.0f32, |m, v| m.max(v.abs())) / 0.01
        };
        let (a, b) = (level(4000.0), level(8000.0));
        let slope = db((b / a) as f64);
        assert!((-25.5..-22.0).contains(&slope), "{slope} dB per octave");
        assert!(db(level(50.0) as f64).abs() < 0.2, "passband {}", level(50.0));
        // Four poles at cutoff: each -3 dB, so about -12 dB.
        let at = db(level(1000.0) as f64);
        assert!((-13.0..-11.0).contains(&at), "{at} dB at the cutoff");
    }

    #[test]
    fn no_nan_or_runaway_on_extreme_settings() {
        let rate = 96000.0;
        let cases = [(5.0f32, 5.0f32, 1.0f32), (40000.0, 5.0, 1.0), (40000.0, 20.0, 100.0), (20.0, 20.0, 100.0), (1000.0, 0.0, 1000.0), (1000.0, 5.0, 1e-30)];
        for (hz, k, level) in cases {
            let mut f = Ladder::default();
            f.set(cutoff_g(hz, rate), k);
            let mut peak = 0.0f32;
            for i in 0..200_000 {
                // An edgy input, and the cutoff jumping around at the audio rate too.
                let x = if (i / 37) % 2 == 0 { level } else { -level };
                let g = hz * (1.0 + 0.9 * ((i as f32) * 0.01).sin());
                f.set(cutoff_g(g, rate), k);
                let y = f.tick(x);
                assert!(y.is_finite(), "{hz} {k} {level}: {y}");
                peak = peak.max(y.abs());
            }
            assert!(peak < 1000.0 * level.max(1.0), "{hz} {k} {level}: {peak}");
        }
        // A NaN that gets in anyway doesn't stay.
        let mut f = Ladder::default();
        f.set(0.3, 3.0);
        f.tick(f32::NAN);
        f.tick(f32::NAN);
        for _ in 0..16 {
            f.tick(0.0);
        }
        assert!(f.tick(0.0).is_finite());
    }

    #[test]
    fn state_doesnt_go_denormal() {
        let mut f = Ladder::default();
        f.set(cutoff_g(1000.0, 96000.0), 2.0);
        f.tick(1.0);
        for _ in 0..4_000_000 {
            f.tick(0.0);
        }
        assert!(f.z.iter().all(|z| *z == 0.0 || z.abs() >= f32::MIN_POSITIVE), "{:?}", f.z);
    }
}
