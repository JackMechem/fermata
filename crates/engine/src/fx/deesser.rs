//! De-Esser, after Logic's DeEsser 2: turns sibilance down when the band above Frequency gets
//! louder than the threshold, by up to Range.
//!
//! - A Linkwitz-Riley crossover (4th order, 24 dB/oct) splits the signal at Frequency. Its two
//!   bands add back up to a flat response, so the de-esser is transparent while it isn't
//!   reducing.
//! - The detector listens to the high band (or to the side-chain key's high band). Its level
//!   rises fast and falls back with Smoothing.
//! - Split reduces only the high band; Wide turns the whole signal down, which keeps the
//!   voice's tone but ducks it more audibly.
//! - Listen plays what the detector hears, for tuning Frequency.
//!
//! Frequency glides and mode and Listen changes crossfade, so moving the controls doesn't
//! click or zipper.

use super::biquad::{Biquad, BUTTERWORTH_Q};
use super::util::{coeff, db_to_gain, gain_to_db};
use super::*;

pub const FREQUENCY: u32 = 0;
pub const THRESHOLD: u32 = 1;
pub const RANGE: u32 = 2;
pub const MODE: u32 = 3;
pub const SMOOTHING: u32 = 4;
pub const LISTEN: u32 = 5;

/// Soft knee width around the threshold (dB).
const KNEE_DB: f32 = 6.0;
/// The detector's rise time (ms): fast enough to catch an "s" as it starts.
const DETECTOR_ATTACK_MS: f32 = 0.5;
/// Frequency changes glide over about this long (ms); coefficients follow every few samples.
const FREQ_GLIDE_MS: f32 = 20.0;
const FREQ_STEP: usize = 16;

static PARAMS: [ParamSpec; 6] = [
    ParamSpec { id: FREQUENCY, name: "Frequency", min: 1000.0, max: 16000.0, default: 6000.0, unit: Unit::Hz, kind: ParamKind::Continuous, log: true },
    ParamSpec { id: THRESHOLD, name: "Threshold", min: -60.0, max: 0.0, default: -30.0, unit: Unit::Db, kind: ParamKind::Continuous, log: false },
    ParamSpec { id: RANGE, name: "Range", min: 0.0, max: 30.0, default: 12.0, unit: Unit::Db, kind: ParamKind::Continuous, log: false },
    ParamSpec { id: MODE, name: "Mode", min: 0.0, max: 1.0, default: 0.0, unit: Unit::None, kind: ParamKind::Choice(&["Split", "Wide"]), log: false },
    ParamSpec { id: SMOOTHING, name: "Smoothing", min: 1.0, max: 200.0, default: 30.0, unit: Unit::Ms, kind: ParamKind::Continuous, log: true },
    ParamSpec { id: LISTEN, name: "Listen", min: 0.0, max: 1.0, default: 0.0, unit: Unit::None, kind: ParamKind::Toggle, log: false },
];

static METERS: [&str; 2] = ["Reduction", "Sibilance"];

/// Starting points: sibilance sits higher in most female voices than in male ones.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Female Vocal", values: &[(FREQUENCY, 7500.0), (THRESHOLD, -28.0), (RANGE, 8.0), (MODE, 0.0), (SMOOTHING, 25.0)] },
    FactoryPreset { name: "Male Vocal", values: &[(FREQUENCY, 5500.0), (THRESHOLD, -28.0), (RANGE, 8.0), (MODE, 0.0), (SMOOTHING, 30.0)] },
    FactoryPreset { name: "Gentle", values: &[(FREQUENCY, 6500.0), (THRESHOLD, -24.0), (RANGE, 4.0), (MODE, 0.0), (SMOOTHING, 45.0)] },
    FactoryPreset { name: "Aggressive", values: &[(FREQUENCY, 6000.0), (THRESHOLD, -38.0), (RANGE, 16.0), (MODE, 0.0), (SMOOTHING, 15.0)] },
    FactoryPreset { name: "Wide and Smooth", values: &[(FREQUENCY, 6000.0), (THRESHOLD, -30.0), (RANGE, 6.0), (MODE, 1.0), (SMOOTHING, 60.0)] },
    FactoryPreset { name: "Hi-Hat Tamer", values: &[(FREQUENCY, 9000.0), (THRESHOLD, -30.0), (RANGE, 10.0), (MODE, 0.0), (SMOOTHING, 20.0)] },
    FactoryPreset { name: "Acoustic Guitar Squeak", values: &[(FREQUENCY, 3500.0), (THRESHOLD, -32.0), (RANGE, 6.0), (MODE, 0.0), (SMOOTHING, 20.0)] },
];

pub fn info() -> EffectInfo {
    EffectInfo { id: "builtin:deesser", name: "De-Esser", category: "Dynamics", params: &PARAMS, meters: &METERS, sidechain: true, new: |sr| Box::new(DeEsser::new(sr)) }
}

/// One channel of the crossover: two cascaded Butterworth sections per band.
#[derive(Default, Clone, Copy)]
struct Crossover {
    lp: [Biquad; 2],
    hp: [Biquad; 2],
}

impl Crossover {
    fn tune(&mut self, sr: f64, f: f64) {
        for b in &mut self.lp {
            b.lowpass(sr, f, BUTTERWORTH_Q);
        }
        for b in &mut self.hp {
            b.highpass(sr, f, BUTTERWORTH_Q);
        }
    }

    /// Splits one sample into (low, high).
    #[inline]
    fn split(&mut self, x: f32) -> (f32, f32) {
        let low = self.lp[0].run(x);
        let low = self.lp[1].run(low);
        let high = self.hp[0].run(x);
        let high = self.hp[1].run(high);
        (low, high)
    }

    fn reset(&mut self) {
        self.lp.iter_mut().chain(self.hp.iter_mut()).for_each(|b| b.reset());
    }
}

/// The gain reduction (dB, positive) for a level `over` dB above the threshold, with a soft
/// knee, never more than `range`.
fn reduction(over: f32, range: f32) -> f32 {
    let half = KNEE_DB / 2.0;
    let gr = if over <= -half {
        0.0
    } else if over < half {
        (over + half) * (over + half) / (2.0 * KNEE_DB)
    } else {
        over
    };
    gr.min(range)
}

pub struct DeEsser {
    sr: f32,
    values: [f32; PARAMS.len()],
    /// Audio crossovers (L, R) and the key's.
    xo: [Crossover; 2],
    key_xo: [Crossover; 2],
    freq: f32,
    tuned: f32,
    freq_glide: f32,
    since_tune: usize,
    env: f32,
    rise: f32,
    fall: f32,
    /// Smoothed gain on the reduced band (linear), and the crossfades for Mode and Listen.
    gain: f32,
    gain_step: f32,
    wide: f32,
    listen: f32,
    fade_step: f32,
    peak_gr: f32,
}

impl DeEsser {
    pub fn new(sample_rate: f32) -> DeEsser {
        let sr = sample_rate.max(1.0);
        let mut d = DeEsser {
            sr,
            values: [0.0; PARAMS.len()],
            xo: [Crossover::default(); 2],
            key_xo: [Crossover::default(); 2],
            freq: 0.0,
            tuned: 0.0,
            freq_glide: 1.0 - (-(FREQ_STEP as f32) / (FREQ_GLIDE_MS * 0.001 * sr)).exp(),
            since_tune: 0,
            env: 0.0,
            rise: coeff(DETECTOR_ATTACK_MS, sr),
            fall: 1.0,
            gain: 1.0,
            gain_step: coeff(0.5, sr),
            wide: 0.0,
            listen: 0.0,
            fade_step: coeff(5.0, sr),
            peak_gr: 0.0,
        };
        for p in &PARAMS {
            d.values[p.id as usize] = p.default;
        }
        d.set_param(SMOOTHING, PARAMS[SMOOTHING as usize].default);
        d.freq = d.values[FREQUENCY as usize];
        d.tune(d.freq);
        d
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    fn tune(&mut self, f: f32) {
        let f = f.min(0.45 * self.sr) as f64;
        let sr = self.sr as f64;
        for x in self.xo.iter_mut().chain(self.key_xo.iter_mut()) {
            x.tune(sr, f);
        }
        self.tuned = f as f32;
    }
}

impl Effect for DeEsser {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(v) = self.values.get_mut(id as usize) else { return };
        *v = value;
        if id == SMOOTHING {
            self.fall = coeff(value.max(1.0), self.sr);
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key, _ctx: &FxContext) {
        let n = l.len().min(r.len());
        let target_freq = self.v(FREQUENCY);
        let threshold = self.v(THRESHOLD);
        let range = self.v(RANGE).max(0.0);
        let wide_target = if self.v(MODE) >= 0.5 { 1.0 } else { 0.0 };
        let listen_target = if self.v(LISTEN) >= 0.5 { 1.0 } else { 0.0 };
        let mut peak_gr = 0.0f32;
        for i in 0..n {
            // Glide the crossover to a new frequency (in octaves), retuning every few samples.
            if self.since_tune == 0 && self.freq != target_freq {
                self.freq *= (target_freq / self.freq).powf(self.freq_glide);
                if (self.freq / target_freq - 1.0).abs() < 1e-3 {
                    self.freq = target_freq;
                }
                if self.freq != self.tuned {
                    self.tune(self.freq);
                }
            }
            self.since_tune = (self.since_tune + 1) % FREQ_STEP;

            let (ll, lh) = self.xo[0].split(l[i]);
            let (rl, rh) = self.xo[1].split(r[i]);
            let (kl, kr) = match key {
                Some((a, b)) if i < a.len() && i < b.len() => (self.key_xo[0].split(a[i]).1, self.key_xo[1].split(b[i]).1),
                _ => (lh, rh),
            };
            let k = kl.abs().max(kr.abs());
            self.env += (k - self.env) * if k > self.env { self.rise } else { self.fall };

            let gr = reduction(gain_to_db(self.env) - threshold, range);
            peak_gr = peak_gr.max(gr);
            self.gain += (db_to_gain(-gr) - self.gain) * self.gain_step;
            self.wide += (wide_target - self.wide) * self.fade_step;
            self.listen += (listen_target - self.listen) * self.fade_step;

            let g = self.gain;
            let (w, m) = (self.wide, self.listen);
            let split_l = ll + lh * g;
            let split_r = rl + rh * g;
            let wide_l = (ll + lh) * g;
            let wide_r = (rl + rh) * g;
            let out_l = split_l * (1.0 - w) + wide_l * w;
            let out_r = split_r * (1.0 - w) + wide_r * w;
            l[i] = out_l * (1.0 - m) + kl * m;
            r[i] = out_r * (1.0 - m) + kr * m;
        }
        self.peak_gr = peak_gr;
    }

    fn reset(&mut self) {
        self.xo.iter_mut().chain(self.key_xo.iter_mut()).for_each(|x| x.reset());
        self.env = 0.0;
        self.gain = 1.0;
        self.peak_gr = 0.0;
        // Glides and crossfades land where the controls are.
        self.wide = if self.v(MODE) >= 0.5 { 1.0 } else { 0.0 };
        self.listen = if self.v(LISTEN) >= 0.5 { 1.0 } else { 0.0 };
        if self.freq != self.v(FREQUENCY) {
            self.freq = self.v(FREQUENCY);
            self.tune(self.freq);
        }
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        out[0] = self.peak_gr;
        out[1] = gain_to_db(self.env).max(-100.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;
    const CTX: FxContext = FxContext { sample_rate: SR, tempo: 120.0, playing: true };

    fn tone(db: f32, freq: f32, n: usize) -> Vec<f32> {
        let a = db_to_gain(db);
        (0..n).map(|i| a * (std::f32::consts::TAU * freq * i as f32 / SR).sin()).collect()
    }

    fn run(d: &mut DeEsser, x: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        for c in x.chunks(256) {
            let mut l = c.to_vec();
            let mut r = c.to_vec();
            d.process(&mut l, &mut r, None, &CTX);
            out.extend(l);
        }
        out
    }

    /// The level of `freq` in `x` (dB), by correlating with a sine and cosine.
    fn level_at(x: &[f32], freq: f32) -> f32 {
        let (mut s, mut c) = (0.0f64, 0.0f64);
        for (i, v) in x.iter().enumerate() {
            let ph = std::f64::consts::TAU * freq as f64 * i as f64 / SR as f64;
            s += *v as f64 * ph.sin();
            c += *v as f64 * ph.cos();
        }
        let amp = 2.0 * (s * s + c * c).sqrt() / x.len() as f64;
        gain_to_db(amp as f32)
    }

    #[test]
    fn sibilance_is_reduced_and_lower_tones_are_not() {
        let mut d = DeEsser::new(SR);
        d.set_param(FREQUENCY, 5000.0);
        d.set_param(THRESHOLD, -30.0);
        d.set_param(RANGE, 12.0);
        let ess = run(&mut d, &tone(-10.0, 8000.0, 24000));
        let got = level_at(&ess[12000..], 8000.0);
        // The crossover's bands are in phase: at 8 kHz the high band carries r⁴ / (1 + r⁴) of
        // the tone (r = 8 / 5), and only it is turned down by the 12 dB range.
        let r4 = (8.0f32 / 5.0).powi(4);
        let want = -10.0 + gain_to_db(1.0 / (1.0 + r4) + db_to_gain(-12.0) * r4 / (1.0 + r4));
        assert!((got - want).abs() < 0.5, "sibilant band {got} dB, want {want}");
        let m = &mut [0.0; MAX_METERS];
        d.meters(m);
        assert!((m[0] - 12.0).abs() < 0.1, "meter {}", m[0]);

        let mut d = DeEsser::new(SR);
        d.set_param(FREQUENCY, 5000.0);
        d.set_param(THRESHOLD, -30.0);
        let low = run(&mut d, &tone(-10.0, 500.0, 24000));
        let got = level_at(&low[12000..], 500.0);
        assert!((got + 10.0).abs() < 0.1, "a tone below the band stays {got} dB");
    }

    #[test]
    fn split_leaves_the_voice_wide_ducks_it() {
        let mix: Vec<f32> = tone(-10.0, 8000.0, 24000).iter().zip(tone(-10.0, 300.0, 24000)).map(|(a, b)| a + b).collect();
        let mut d = DeEsser::new(SR);
        d.set_param(THRESHOLD, -30.0);
        let split = run(&mut d, &mix);
        assert!((level_at(&split[12000..], 300.0) + 10.0).abs() < 0.2);
        let mut d = DeEsser::new(SR);
        d.set_param(THRESHOLD, -30.0);
        d.set_param(MODE, 1.0);
        let wide = run(&mut d, &mix);
        assert!((level_at(&wide[12000..], 300.0) + 22.0).abs() < 0.5);
    }

    #[test]
    fn unreduced_bands_add_back_up_flat() {
        // Nothing crosses the threshold: every frequency keeps its level.
        for f in [200.0, 3000.0, 6000.0, 9000.0, 15000.0] {
            let mut d = DeEsser::new(SR);
            d.set_param(THRESHOLD, 0.0);
            let out = run(&mut d, &tone(-30.0, f, 24000));
            let got = level_at(&out[12000..], f);
            assert!((got + 30.0).abs() < 0.05, "{f} Hz: {got} dB");
        }
    }

    #[test]
    fn listen_plays_the_detector_band() {
        let mut d = DeEsser::new(SR);
        d.set_param(LISTEN, 1.0);
        let low = run(&mut d, &tone(-10.0, 300.0, 24000));
        assert!(level_at(&low[12000..], 300.0) < -60.0);
        assert_eq!(d.latency(), 0);
    }
}
