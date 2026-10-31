//! Building blocks for synthesizer models: plain DSP with no knowledge of the engine, parameters
//! or the project. A model (see `crate::instrument`) wires them into voices.
//!
//! - [`osc`]: band-limited oscillators (saw, pulse with PWM, triangle, sine; hard sync) and noise.
//! - [`filter`]: zero-delay-feedback filters with real nonlinearity behind the [`filter::ZdfFilter`]
//!   trait; a 4-pole transistor ladder today.
//! - [`env`]: envelopes with RC-circuit curves.
//! - [`lfo`]: LFOs, free or locked to the transport.
//! - [`alloc`]: the voice allocator: poly with stealing, mono with note priority and legato, unison.
//! - [`analog`]: deterministic "analog character": per-voice drift and component differences.
//! - [`resample`]: the half-band decimator that brings an oversampled voice bus back to the output
//!   rate, without latency to report.
//! - [`smooth`]: parameter smoothing and glide.
//!
//! **Real-time rules:** every part here is plain data with its state inline. Constructors and
//! `tick`s don't allocate, lock or block, and nothing touches a global or the clock: the same
//! seed and the same calls give the same bits, on any thread (parallel output is bit-identical
//! to serial because of this).
//!
//! **Rates:** parts take their rate-dependent values as already worked-out coefficients, or the
//! sample rate they tick at, which for a voice is usually the oversampled rate.

pub mod alloc;
pub mod analog;
pub mod env;
pub mod filter;
pub mod lfo;
pub mod osc;
pub mod resample;
pub mod smooth;

/// 2^x, fast and accurate to about 1e-6 relative (under 0.002 cents), for pitch going to frequency at
/// the audio rate. `x` must be finite and within the range of an f32's exponent.
#[inline]
pub fn pow2(x: f32) -> f32 {
    let fl = x.floor();
    let t = x - fl;
    // The Taylor series, its last term adjusted to land exactly on 2 at the end of the octave.
    let m = 1.0 + t * (std::f32::consts::LN_2 + t * (0.240_226_5 + t * (0.055_504_11 + t * (0.009_618_13 + t * (0.001_333_36 + t * 0.000_170_71)))));
    let e = (fl as i32 + 127).clamp(1, 254) as u32;
    m * f32::from_bits(e << 23)
}

/// The frequency of a MIDI note number (fractional, A4 = 69 = 440 Hz), through [`pow2`].
#[inline]
pub fn note_hz(note: f32) -> f32 {
    440.0 * pow2((note - 69.0) / 12.0)
}

/// tan(x) on 0..about 1.45 (a continued fraction, within about 1e-5 at the top and far better
/// below), for a filter's prewarped cutoff at the audio rate.
#[inline]
pub fn tan_fast(x: f32) -> f32 {
    let a = x * x;
    x * (945.0 + a * (-105.0 + a)) / (945.0 + a * (-420.0 + 15.0 * a))
}

/// tanh(x)/x (a Padé approximant), which a ladder uses as a stage's nonlinear gain. Stays between
/// 0 and 1 for every finite x.
#[inline]
pub fn tanh_x_dx(x: f32) -> f32 {
    let a = x * x;
    // Past here the approximant turns and climbs through 1; tanh is 1 by then anyway.
    if a > 12.25 {
        return 1.0 / x.abs();
    }
    ((a + 105.0) * a + 945.0) / ((15.0 * a + 420.0) * a + 945.0)
}

/// tanh(x), by [`tanh_x_dx`], a soft saturator.
#[inline]
pub fn tanh_fast(x: f32) -> f32 {
    x * tanh_x_dx(x)
}

#[cfg(test)]
pub(crate) mod test_util {
    use realfft::RealFftPlanner;

    /// The magnitude spectrum of a real signal (rectangular window), normalized so a full-scale
    /// sine on a bin reads 1.
    pub fn spectrum(x: &[f32]) -> Vec<f64> {
        let mut input: Vec<f64> = x.iter().map(|&v| v as f64).collect();
        let r2c = RealFftPlanner::<f64>::new().plan_fft_forward(input.len());
        let mut out = r2c.make_output_vec();
        r2c.process(&mut input, &mut out).unwrap();
        let k = 2.0 / x.len() as f64;
        out.iter().map(|c| c.norm() * k).collect()
    }

    pub fn db(x: f64) -> f64 {
        20.0 * x.max(1e-12).log10()
    }

    /// The frequency of a signal in Hz by the phase advance of its largest spectral peak, across
    /// two windowed FFT frames: good to a small fraction of a cent on a steady tone.
    pub fn pitch_hz(x: &[f32], sr: f32) -> f32 {
        let n = (x.len() / 2).next_power_of_two() / 2;
        let hop = n / 8;
        let window = |s: &[f32]| -> Vec<f64> {
            s.iter().enumerate().map(|(i, &v)| v as f64 * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / s.len() as f64).cos())).collect()
        };
        let r2c = RealFftPlanner::<f64>::new().plan_fft_forward(n);
        let start = x.len() - n - hop;
        let (mut a, mut b) = (window(&x[start..start + n]), window(&x[start + hop..start + hop + n]));
        let (mut fa, mut fb) = (r2c.make_output_vec(), r2c.make_output_vec());
        r2c.process(&mut a, &mut fa).unwrap();
        r2c.process(&mut b, &mut fb).unwrap();
        let (k, _) = fa.iter().enumerate().skip(1).fold((0, 0.0), |m, (i, c)| if c.norm() > m.1 { (i, c.norm()) } else { m });
        let tau = std::f64::consts::TAU;
        let expected = tau * k as f64 * hop as f64 / n as f64;
        let dphi = fb[k].arg() - fa[k].arg() - expected;
        let wrapped = dphi - tau * (dphi / tau).round();
        ((k as f64 + wrapped * n as f64 / (tau * hop as f64)) * sr as f64 / n as f64) as f32
    }

    pub fn cents(got: f32, want: f32) -> f32 {
        1200.0 * (got / want).log2()
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::*;
    use super::*;

    /// The tuning tests are worth what their measure is: it must see a small error.
    #[test]
    fn pitch_measure_sees_a_fraction_of_a_cent() {
        let want = 440.0f64 * 2f64.powf(0.25 / 1200.0);
        let x: Vec<f32> = (0..1 << 16).map(|i| (std::f64::consts::TAU * want * i as f64 / 48000.0).sin() as f32).collect();
        let c = cents(pitch_hz(&x, 48000.0), 440.0);
        assert!((c - 0.25).abs() < 0.02, "{c}");
    }

    #[test]
    fn fast_math_is_close_enough() {
        for i in -4000..4000 {
            let x = i as f32 / 200.0;
            let err = (pow2(x) as f64 / 2f64.powf(x as f64) - 1.0).abs();
            assert!(err < 1.2e-6, "2^{x}: {err}");
        }
        for i in 1..1450 {
            let x = i as f32 / 1000.0;
            let err = (tan_fast(x) as f64 / (x as f64).tan() - 1.0).abs();
            assert!(err < 3e-3, "tan {x}: {err}");
            if x < 1.0 {
                assert!(err < 1e-5, "tan {x}: {err}");
            }
        }
        for i in -3000..3000 {
            let x = i as f32 / 100.0;
            let g = tanh_x_dx(x);
            assert!((0.0..=1.0).contains(&g), "tanh gain {x}: {g}");
            assert!((tanh_fast(x) - x.tanh()).abs() < 3e-3, "tanh {x}");
        }
        assert!((note_hz(69.0) - 440.0).abs() < 1e-3);
        assert!((note_hz(60.0) - 261.625_58).abs() < 1e-2);
    }
}
