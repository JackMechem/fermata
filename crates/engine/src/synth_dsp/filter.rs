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
//! To add a filter: write its `tick` in this file with
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

/// A 4-pole cascade of OTA integrators with one feedback path, the family of the IR3109 and the
/// CEM3320 and (near enough for this level of model) the SSM2040:
///
/// ```text
/// dV1/dt = wc tanh(a (u - V1)) / a,   dV2/dt = wc tanh(a (V1 - V2)) / a,   and so on
/// u = tanh(in (1 + bass k) - k V4)
/// ```
///
/// Where the transistor ladder bends on each stage's level, an OTA bends on the difference across
/// it, so it slews on big fast edges rather than thickening, and stays cleaner at low levels.
/// `resonance` is k: 4 is where it starts to self-oscillate. Two constants tell chips apart, both
/// set per model and both guesses: `slew` (`a` above), how soon a stage runs out of current, and
/// `bass`, how much input the circuit adds back as resonance takes the low end away (0 loses it
/// all as a plain ladder does, 1 keeps the pass band's level).
#[derive(Debug, Clone, Copy)]
pub struct OtaCascade {
    z: [f32; 4],
    g: f32,
    k: f32,
    slew: f32,
    bass: f32,
    last_in: f32,
    stage: [f32; 4],
}

impl OtaCascade {
    pub fn new(slew: f32, bass: f32) -> OtaCascade {
        OtaCascade { z: [0.0; 4], g: 0.1, k: 0.0, slew, bass, last_in: 0.0, stage: [1.0; 4] }
    }

    pub fn set_character(&mut self, slew: f32, bass: f32) {
        self.slew = slew;
        self.bass = bass;
    }

    /// Each stage's gain ratio, near 1: component tolerances.
    pub fn set_tolerance(&mut self, stage: [f32; 4]) {
        self.stage = stage;
    }
}

impl Default for OtaCascade {
    fn default() -> Self {
        OtaCascade::new(1.0, 0.0)
    }
}

impl ZdfFilter for OtaCascade {
    #[inline]
    fn set(&mut self, g: f32, resonance: f32) {
        self.g = g;
        self.k = resonance;
    }

    #[inline]
    fn tick(&mut self, input: f32) -> f32 {
        let [z0, z1, z2, z3] = self.z;
        let (k, a) = (self.k, self.slew);
        let driven = input * (1.0 + self.bass * k);
        let mid = 0.5 * (driven + self.last_in);
        self.last_in = driven;
        // The feedback's own differential pair limits what goes in.
        let tf = tanh_x_dx(mid - k * z3);
        // Each stage's gain, from the difference across it before this sample. Half the feedback
        // pair's output is a fair estimate of what stage 1 sees.
        let t0 = tanh_x_dx(a * (tf * (mid - k * z3) - z0));
        let t1 = tanh_x_dx(a * (z0 - z1));
        let t2 = tanh_x_dx(a * (z1 - z2));
        let t3 = tanh_x_dx(a * (z2 - z3));
        let f = |i: usize, t: f32| self.g * self.stage[i] * t;
        let (f0, f1, f2, f3) = (f(0, t0), f(1, t1), f(2, t2), f(3, t3));
        let (d0, d1, d2, d3) = (1.0 / (1.0 + f0), 1.0 / (1.0 + f1), 1.0 / (1.0 + f2), 1.0 / (1.0 + f3));
        let (a0, a1, a2, a3) = (f0 * d0, f1 * d1, f2 * d2, f3 * d3);
        let (s0, s1, s2, s3) = (z0 * d0, z1 * d1, z2 * d2, z3 * d3);
        // y3 = A u + B, and u = tf (input - k y3): solved within the sample.
        let aa = a0 * a1 * a2 * a3;
        let bb = s3 + a3 * (s2 + a2 * (s1 + a1 * s0));
        let y3 = (aa * tf * driven + bb) / (1.0 + k * tf * aa);
        let u = tf * (driven - k * y3);
        let y0 = a0 * u + s0;
        let y1 = a1 * y0 + s1;
        let y2 = a2 * y1 + s2;
        self.z = [2.0 * y0 - z0, 2.0 * y1 - z1, 2.0 * y2 - z2, 2.0 * y3 - z3];
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

/// Where a [`DiodeLadder`] with the first capacitor at [`DIODE_C1`] peaks, as a fraction of its
/// `1 / RC` (worked out from the circuit's polynomial: see docs/ARCHITECTURE.md).
const DIODE_W0: f32 = 1.421_304_3;
/// The feedback gain at which that polynomial goes unstable.
const DIODE_K_CRIT: f32 = 21.562_647;
/// The first capacitor against the other three: 18 nF against 33 nF in published schematics'
/// reading of the 303 (a figure from memory of the analyses, not measured here).
const DIODE_C1: f32 = 0.545;

/// A diode ladder, the filter of the small silver bass machine. Unlike the transistor ladder, each
/// rung loads the ones around it, so its poles spread out: one near the cutoff, the others far
/// above it, and the first capacitor being smaller spreads them further. The slope around the
/// cutoff is nearer 18 dB per octave than 24:
///
/// ```text
/// C1 dV1/dt = i(u - V1) - i(V1 - V2)
///    dV2/dt = i(V1 - V2) - i(V2 - V3)
///    dV3/dt = i(V2 - V3) - i(V3 - V4)
///    dV4/dt = i(V3 - V4),        i(x) = tanh(x)
/// ```
///
/// `resonance` is 0..1, with 1 where self-oscillation starts. The feedback goes through a
/// high-pass ([`DiodeLadder::set_feedback_highpass`]), so low notes keep their weight as resonance
/// rises and the resonance fades out at the very bottom. The cutoff is the frequency the resonance
/// peaks at.
#[derive(Debug, Clone, Copy)]
pub struct DiodeLadder {
    z: [f32; 4],
    g: f32,
    k: f32,
    last_in: f32,
    /// The feedback high-pass: its gain and its state.
    hp_g: f32,
    hp_z: f32,
}

impl Default for DiodeLadder {
    fn default() -> Self {
        DiodeLadder { z: [0.0; 4], g: 0.1, k: 0.0, last_in: 0.0, hp_g: 0.0, hp_z: 0.0 }
    }
}

impl DiodeLadder {
    /// The feedback path's corner, as [`cutoff_g`] gives it; 0 for none.
    pub fn set_feedback_highpass(&mut self, g: f32) {
        self.hp_g = g;
    }
}

impl ZdfFilter for DiodeLadder {
    #[inline]
    fn set(&mut self, g: f32, resonance: f32) {
        self.g = g / DIODE_W0;
        self.k = resonance * DIODE_K_CRIT;
    }

    #[inline]
    fn tick(&mut self, input: f32) -> f32 {
        let [z0, z1, z2, z3] = self.z;
        let (g, k) = (self.g, self.k);
        let mid = 0.5 * (input + self.last_in);
        self.last_in = input;
        // The feedback high-pass solved as y4 c - d.
        let hp_a = 1.0 / (1.0 + self.hp_g);
        let (c, d) = (hp_a, hp_a * self.hp_z);
        let fb = k * (c * z3 - d);
        // Each rung's gain from the difference across it before this sample. The input pair is
        // where the circuit overloads, and it keeps the loop's gain bounded.
        let t0 = tanh_x_dx(mid - fb - z0);
        let t1 = tanh_x_dx(z0 - z1);
        let t2 = tanh_x_dx(z1 - z2);
        let t3 = tanh_x_dx(z2 - z3);
        let a0 = g / DIODE_C1;
        // Back-substitute the tridiagonal system from the last node: y[n] = al[n] y[n-1] + be[n].
        let e3 = 1.0 / (1.0 + g * t3);
        let (al3, be3) = (g * t3 * e3, z3 * e3);
        let e2 = 1.0 / (1.0 + g * (t2 + t3) - g * t3 * al3);
        let (al2, be2) = (g * t2 * e2, (z2 + g * t3 * be3) * e2);
        let e1 = 1.0 / (1.0 + g * (t1 + t2) - g * t2 * al2);
        let (al1, be1) = (g * t1 * e1, (z1 + g * t2 * be2) * e1);
        let e0 = 1.0 / (1.0 + a0 * (t0 + t1) - a0 * t1 * al1);
        let (al0, be0) = (a0 * t0 * e0, (z0 + a0 * t1 * be1) * e0);
        // y3 = A u + B, u = input + k d - k c y3.
        let aa = al0 * al1 * al2 * al3;
        let bb = be3 + al3 * (be2 + al2 * (be1 + al1 * be0));
        let y3 = (aa * (input + k * d) + bb) / (1.0 + aa * k * c);
        let u = input + k * d - k * c * y3;
        let y0 = al0 * u + be0;
        let y1 = al1 * y0 + be1;
        let y2 = al2 * y1 + be2;
        self.z = [2.0 * y0 - z0, 2.0 * y1 - z1, 2.0 * y2 - z2, 2.0 * y3 - z3];
        // The high-pass's own integrator: its low-pass is y4 - h.
        let h = c * y3 - d;
        self.hp_z += 2.0 * self.hp_g * h;
        tidy(&mut self.hp_z);
        for z in &mut self.z {
            tidy(z);
        }
        y3
    }

    fn reset(&mut self) {
        self.z = [0.0; 4];
        self.last_in = 0.0;
        self.hp_z = 0.0;
    }
}

/// A 2-pole state-variable filter (Simper's trapezoidal form) with all its outputs. Its damping
/// grows with the level in the band-pass, so full resonance rings and sings without running away.
/// `resonance` is 0..1, and 1 just about self-oscillates.
#[derive(Debug, Clone, Copy, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    g: f32,
    k: f32,
    pub lp: f32,
    pub bp: f32,
    pub hp: f32,
}

impl ZdfFilter for Svf {
    #[inline]
    fn set(&mut self, g: f32, resonance: f32) {
        self.g = g;
        self.k = 2.0 * (1.0 - resonance.clamp(0.0, 1.0)) + 0.002;
    }

    /// Returns the low-pass; [`Svf::bp`], [`Svf::hp`] and `lp + hp` (a notch) are there too.
    #[inline]
    fn tick(&mut self, input: f32) -> f32 {
        let (g, ic1, ic2) = (self.g, self.ic1, self.ic2);
        let k = self.k + 0.15 * (ic1 * ic1).min(16.0);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = input - ic2;
        let v1 = a1 * ic1 + a2 * v3;
        let v2 = ic2 + a2 * ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - ic1;
        self.ic2 = 2.0 * v2 - ic2;
        tidy(&mut self.ic1);
        tidy(&mut self.ic2);
        self.lp = v2;
        self.bp = v1;
        self.hp = input - k * v1 - v2;
        v2
    }

    fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
        self.lp = 0.0;
        self.bp = 0.0;
        self.hp = 0.0;
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

    /// A filter's level at `hz` over `level` at the input, once settled.
    fn gain_of(f: &mut impl ZdfFilter, hz: f32, rate: f32, level: f32) -> f32 {
        let n = 1 << 16;
        let x: Vec<f32> = (0..n).map(|i| f.tick(level * (std::f64::consts::TAU * hz as f64 * i as f64 / rate as f64).sin() as f32)).collect();
        x[n / 2..].iter().fold(0.0f32, |m, v| m.max(v.abs())) / level
    }

    fn ring(f: &mut impl ZdfFilter, seconds: f32, rate: f32) -> Vec<f32> {
        let n = (seconds * rate) as usize;
        let mut out: Vec<f32> = (0..n).map(|i| f.tick(if i < 8 { 0.2 } else { 0.0 })).collect();
        out.split_off(n / 2)
    }

    #[test]
    fn ota_cascade_rolls_off_24_db_and_self_oscillates_in_tune() {
        let rate = 96000.0;
        let g = cutoff_g(1000.0, rate);
        let mut f = OtaCascade::new(1.0, 0.0);
        f.set(g, 0.0);
        let (a, b) = (gain_of(&mut f, 4000.0, rate, 0.01), gain_of(&mut f, 8000.0, rate, 0.01));
        let slope = db((b / a) as f64);
        assert!((-25.5..-22.0).contains(&slope), "{slope} dB per octave");
        assert!(db(gain_of(&mut f, 50.0, rate, 0.01) as f64).abs() < 0.2);
        for &sr in &[44100.0f32, 48000.0, 96000.0] {
            let rate = 2.0 * sr;
            let mut worst = 0.0f32;
            for note in (36..=96).step_by(6) {
                let hz = crate::synth_dsp::note_hz(note as f32);
                let mut f = OtaCascade::new(1.0, 0.0);
                f.set(cutoff_g(hz, rate), 4.3);
                let x = ring(&mut f, 2.5, rate);
                assert!(x.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.1, "{hz}: no self-oscillation");
                let c = cents(pitch_hz(&x, rate), hz);
                worst = worst.max(c.abs());
            }
            println!("OTA cascade self-oscillation at {sr} x2, notes 36..96: worst {worst:.1} cents");
            assert!(worst < 30.0, "{worst}");
        }
        // Below 4 it dies away.
        let mut f = OtaCascade::new(1.0, 0.0);
        f.set(cutoff_g(1000.0, rate), 3.6);
        assert!(ring(&mut f, 1.5, rate).iter().all(|v| v.abs() < 1e-3));
    }

    #[test]
    fn ota_cascade_bass_keeps_the_pass_band_as_resonance_rises() {
        let rate = 96000.0;
        let g = cutoff_g(2000.0, rate);
        let level = |bass: f32, k: f32| {
            let mut f = OtaCascade::new(1.0, bass);
            f.set(g, k);
            gain_of(&mut f, 80.0, rate, 0.02)
        };
        assert!(level(0.0, 3.5) < 0.3, "{}", level(0.0, 3.5));
        assert!((level(1.0, 3.5) - 1.0).abs() < 0.1, "{}", level(1.0, 3.5));
    }

    #[test]
    fn ota_slew_bends_big_edges_earlier() {
        let rate = 96000.0;
        let g = cutoff_g(1000.0, rate);
        let at = |slew: f32, level: f32| {
            let mut f = OtaCascade::new(slew, 0.0);
            f.set(g, 0.0);
            gain_of(&mut f, 2000.0, rate, level)
        };
        assert!(at(2.0, 2.0) < at(0.5, 2.0), "{} {}", at(2.0, 2.0), at(0.5, 2.0));
        // Quietly, they agree.
        assert!((at(2.0, 0.005) - at(0.5, 0.005)).abs() < 0.01);
    }

    #[test]
    fn diode_ladder_slope_is_shallower_than_24_db_near_the_cutoff() {
        let rate = 192000.0;
        let g = cutoff_g(1000.0, rate);
        let level = |hz: f32| {
            let mut f = DiodeLadder::default();
            f.set(g, 0.0);
            gain_of(&mut f, hz, rate, 0.01)
        };
        let near = db((level(2000.0) / level(1000.0)) as f64);
        let above = db((level(4000.0) / level(2000.0)) as f64);
        let far = db((level(16000.0) / level(8000.0)) as f64);
        println!("diode ladder: an octave up from the cutoff {near:.1} dB, the next {above:.1} dB, far above {far:.1} dB");
        assert!((-17.0..-10.0).contains(&near), "{near}");
        assert!((-21.0..-15.0).contains(&above), "{above}");
        assert!((-26.0..-20.0).contains(&far), "{far}");
        // Its lowest pole sits about a decade below the cutoff: the pass band sags long before it.
        assert!(db(level(10.0) as f64).abs() < 0.3);
        assert!((-7.0..-1.0).contains(&db(level(150.0) as f64)), "{}", db(level(150.0) as f64));
    }

    #[test]
    fn diode_ladder_peaks_at_its_cutoff_and_starts_ringing_at_1() {
        for &(hz, rate) in &[(200.0f32, 96000.0f32), (800.0, 96000.0), (3000.0, 96000.0)] {
            let mut f = DiodeLadder::default();
            f.set(cutoff_g(hz, rate), 1.05);
            let x = ring(&mut f, 2.5, rate);
            assert!(x.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.05, "{hz}: nothing");
            let c = cents(pitch_hz(&x, rate), hz);
            println!("diode ladder self-oscillation at {hz} Hz: {c:.1} cents");
            // No trim as the transistor ladder has: the instrument it models doesn't track a keyboard.
            assert!(c.abs() < 120.0, "{hz}: {c}");
            let mut f = DiodeLadder::default();
            f.set(cutoff_g(hz, rate), 0.92);
            assert!(ring(&mut f, 2.0, rate).iter().all(|v| v.abs() < 1e-2), "{hz} rings below 1");
        }
    }

    #[test]
    fn diode_feedback_highpass_keeps_the_low_end() {
        let rate = 96000.0;
        let g = cutoff_g(1000.0, rate);
        let level = |hp: f32| {
            let mut f = DiodeLadder::default();
            f.set(g, 0.85);
            f.set_feedback_highpass(hp);
            gain_of(&mut f, 40.0, rate, 0.02)
        };
        let (off, on) = (level(0.0), level(cutoff_g(150.0, rate)));
        println!("diode ladder at 40 Hz, resonance 0.85: {off:.3} without the high-pass, {on:.3} with");
        assert!(on > 2.0 * off, "{on} {off}");
    }

    #[test]
    fn svf_outputs_and_resonance() {
        let rate = 96000.0;
        let g = cutoff_g(1000.0, rate);
        let sig = |hz: f32, r: f32, pick: fn(&Svf) -> f32| {
            let mut f = Svf::default();
            f.set(g, r);
            let n = 1 << 16;
            let mut peak = 0.0f32;
            for i in 0..n {
                f.tick(0.01 * (std::f64::consts::TAU * hz as f64 * i as f64 / rate as f64).sin() as f32);
                if i > n / 2 {
                    peak = peak.max(pick(&f).abs());
                }
            }
            peak / 0.01
        };
        let lp = |f: &Svf| f.lp;
        let hp = |f: &Svf| f.hp;
        let bp = |f: &Svf| f.bp;
        let notch = |f: &Svf| f.lp + f.hp;
        let slope = db((sig(8000.0, 0.0, lp) / sig(4000.0, 0.0, lp)) as f64);
        assert!((-13.5..-10.5).contains(&slope), "{slope}");
        assert!(db(sig(100.0, 0.0, lp) as f64).abs() < 0.2);
        assert!(db(sig(10000.0, 0.0, hp) as f64).abs() < 0.3);
        assert!(sig(100.0, 0.5, hp) < 0.05);
        assert!(sig(100.0, 0.5, bp) < 0.2 && sig(10000.0, 0.5, bp) < 0.2 && sig(1000.0, 0.5, bp) > 0.9);
        assert!(sig(1000.0, 0.2, notch) < 0.02, "{}", sig(1000.0, 0.2, notch));
        assert!(sig(1000.0, 0.95, lp) > 5.0, "{}", sig(1000.0, 0.95, lp));
    }

    #[test]
    fn new_filters_stay_finite_and_bounded() {
        let rate = 96000.0;
        fn run(mut f: impl ZdfFilter, res: f32, rate: f32) {
            for &(hz, level) in &[(5.0f32, 1.0f32), (40000.0, 1.0), (40000.0, 100.0), (20.0, 100.0), (1000.0, 1000.0), (1000.0, 1e-30)] {
                let mut peak = 0.0f32;
                for i in 0..200_000 {
                    let x = if (i / 37) % 2 == 0 { level } else { -level };
                    let c = hz * (1.0 + 0.9 * ((i as f32) * 0.01).sin());
                    f.set(cutoff_g(c, rate), res);
                    let y = f.tick(x);
                    assert!(y.is_finite(), "{hz} {level}");
                    peak = peak.max(y.abs());
                }
                assert!(peak < 2000.0 * level.max(1.0), "{hz} {level}: {peak}");
            }
            f.tick(f32::NAN);
            f.tick(f32::NAN);
            for _ in 0..16 {
                f.tick(0.0);
            }
            f.reset();
            assert!(f.tick(0.0).is_finite());
        }
        run(OtaCascade::new(1.6, 0.6), 5.0, rate);
        run(OtaCascade::new(0.5, 0.0), 8.0, rate);
        run(DiodeLadder::default(), 1.5, rate);
        run(Svf::default(), 1.0, rate);
        let mut d = DiodeLadder::default();
        d.set_feedback_highpass(cutoff_g(150.0, rate));
        run(d, 1.2, rate);
    }

    #[test]
    fn new_filters_dont_go_denormal() {
        let g = cutoff_g(1000.0, 96000.0);
        let mut f = OtaCascade::new(1.0, 0.0);
        f.set(g, 2.0);
        f.tick(1.0);
        let mut d = DiodeLadder::default();
        d.set(g, 0.5);
        d.set_feedback_highpass(cutoff_g(100.0, 96000.0));
        d.tick(1.0);
        let mut s = Svf::default();
        s.set(g, 0.5);
        s.tick(1.0);
        for _ in 0..4_000_000 {
            f.tick(0.0);
            d.tick(0.0);
            s.tick(0.0);
        }
        let ok = |z: f32| z == 0.0 || z.abs() >= f32::MIN_POSITIVE;
        assert!(f.z.iter().all(|z| ok(*z)), "{:?}", f.z);
        assert!(d.z.iter().all(|z| ok(*z)) && ok(d.hp_z), "{:?}", d.z);
        assert!(ok(s.ic1) && ok(s.ic2));
    }
}
