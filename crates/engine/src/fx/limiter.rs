//! The Limiter, modelled on Logic Pro's Limiter and Adaptive Limiter: a lookahead brickwall
//! that never lets a sample past the Output Ceiling.
//!
//! How it guarantees the ceiling (the approach is the well-known one described, among others,
//! by Geraint Luff in "Designing a straightforward limiter", Signalsmith Audio, 2022; the code
//! here is Fermata's own):
//!
//! 1. Each sample's peak (the 4× oversampled true peak when True Peak is on) gives the gain
//!    that sample needs: `min(1, ceiling / peak)`.
//! 2. A sliding minimum over the lookahead window holds that gain from the moment the peak
//!    enters the window.
//! 3. The release only ever moves the gain up towards the held value, never below it.
//! 4. A moving average over the same window ramps the gain down smoothly. Every value it
//!    averages is at or below the gain the peak needs, so the ramp arrives in time: with the
//!    audio delayed by the window, each sample is multiplied by at most what it needs.
//!
//! A final clamp at the ceiling catches float rounding, so Precise is sample-accurate.
//!
//! Modes:
//! - **Precise**: the above, as transparent as possible.
//! - **Smooth**: a soft knee that eases in from about 6 dB below the ceiling, and a
//!   program-dependent release that slows down under sustained limiting.
//! - **Adaptive**: in the spirit of Logic's Adaptive Limiter, peaks are first rounded by a soft
//!   saturator that starts 6 dB below the ceiling (for loudness and analogue-style colour),
//!   then the Smooth limiter catches what's left.

use super::dyn_common::{db_to_lin, lin_to_db, Delay, Smoother, TruePeak, TP_DELAY};
use super::util::coeff;
use super::{Effect, EffectInfo, FactoryPreset, FxContext, Key, ParamKind, ParamSpec, Unit, MAX_METERS};

/// The plug-in id.
pub const ID: &str = "builtin:limiter";

/// Parameter ids (stable forever).
pub mod param {
    pub const GAIN: u32 = 0;
    pub const CEILING: u32 = 1;
    pub const RELEASE: u32 = 2;
    pub const LOOKAHEAD: u32 = 3;
    pub const MODE: u32 = 4;
    pub const TRUE_PEAK: u32 = 5;
}

const NPARAMS: usize = 6;

static PARAMS: [ParamSpec; NPARAMS] = [
    ParamSpec { id: param::GAIN, name: "Gain", min: 0.0, max: 24.0, default: 0.0, unit: Unit::Db, kind: ParamKind::Continuous, log: false },
    ParamSpec { id: param::CEILING, name: "Out Ceiling", min: -20.0, max: 0.0, default: -0.1, unit: Unit::Db, kind: ParamKind::Continuous, log: false },
    ParamSpec { id: param::RELEASE, name: "Release", min: 1.0, max: 1000.0, default: 50.0, unit: Unit::Ms, kind: ParamKind::Continuous, log: true },
    ParamSpec { id: param::LOOKAHEAD, name: "Lookahead", min: 0.0, max: 20.0, default: 2.0, unit: Unit::Ms, kind: ParamKind::Continuous, log: false },
    ParamSpec {
        id: param::MODE,
        name: "Mode",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: Unit::None,
        kind: ParamKind::Choice(&["Precise", "Smooth", "Adaptive"]),
        log: false,
    },
    ParamSpec { id: param::TRUE_PEAK, name: "True Peak", min: 0.0, max: 1.0, default: 1.0, unit: Unit::None, kind: ParamKind::Toggle, log: false },
];

/// Factory presets. Each lists only what it changes from the defaults.
pub const PRESETS: &[FactoryPreset] = {
    use param::*;
    &[
        FactoryPreset {
            // Streaming services ask for -1 dBTP so their lossy encoders don't clip.
            name: "Master -1 dBTP Streaming",
            values: &[(CEILING, -1.0), (TRUE_PEAK, 1.0), (LOOKAHEAD, 3.0), (RELEASE, 80.0), (MODE, 1.0)],
        },
        FactoryPreset {
            name: "Master -0.1 dB CD",
            values: &[(CEILING, -0.1), (TRUE_PEAK, 0.0), (LOOKAHEAD, 2.0), (RELEASE, 50.0), (MODE, 0.0)],
        },
        FactoryPreset {
            name: "Loud & Smooth (Adaptive)",
            values: &[(MODE, 2.0), (GAIN, 6.0), (CEILING, -1.0), (TRUE_PEAK, 1.0), (LOOKAHEAD, 5.0), (RELEASE, 120.0)],
        },
        FactoryPreset {
            name: "Transparent Safety",
            values: &[(MODE, 0.0), (CEILING, -0.3), (TRUE_PEAK, 1.0), (LOOKAHEAD, 1.5), (RELEASE, 30.0)],
        },
        FactoryPreset {
            name: "Drum Peak Catcher",
            values: &[(MODE, 0.0), (GAIN, 4.0), (CEILING, -3.0), (TRUE_PEAK, 0.0), (LOOKAHEAD, 1.0), (RELEASE, 15.0)],
        },
        FactoryPreset {
            name: "Gentle Bus Leveling (Smooth)",
            values: &[(MODE, 1.0), (GAIN, 3.0), (CEILING, -1.0), (LOOKAHEAD, 8.0), (RELEASE, 250.0)],
        },
    ]
};

/// Meter names: input peak (dBFS, before Gain), output peak (dBFS; true peak when True Peak
/// is on), and gain reduction (positive dB).
pub const METERS: &[&str] = &["Input", "Output", "Gain Reduction"];

/// The registry entry.
pub fn info() -> EffectInfo {
    EffectInfo {
        id: ID,
        name: "Limiter",
        category: "Dynamics",
        params: &PARAMS,
        meters: METERS,
        sidechain: false,
        new: |sr| Box::new(Limiter::new(sr)),
    }
}

/// The longest lookahead, in seconds.
const MAX_LOOKAHEAD: f32 = 0.020;

/// The shortest latency with True Peak on, in samples: the estimator's own lookahead plus a
/// gain ramp of the same length (about 0.65 ms at 48 kHz).
pub const TP_MIN_LATENCY: usize = 2 * TP_DELAY - 1;

/// The fraction of the ceiling True Peak mode aims for (-0.01 dB).
const TP_MARGIN: f32 = 0.99885;

/// Smooth mode's knee: how many dB the soft minimum of input and ceiling is rounded over.
const SMOOTH_KNEE: f32 = 1.5;

/// Adaptive mode's saturator: linear up to this fraction of the ceiling, then rounding off
/// towards `ADAPTIVE_TOP` times the ceiling (the limiter takes care of the rest).
const ADAPTIVE_KNEE: f32 = 0.5;
const ADAPTIVE_TOP: f32 = 1.15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Precise,
    Smooth,
    Adaptive,
}

#[inline]
fn adaptive_shape(u: f32) -> f32 {
    let a = u.abs();
    if a <= ADAPTIVE_KNEE {
        u
    } else {
        let span = ADAPTIVE_TOP - ADAPTIVE_KNEE;
        (ADAPTIVE_KNEE + span * ((a - ADAPTIVE_KNEE) / span).tanh()).copysign(u)
    }
}

/// The Limiter instance.
pub struct Limiter {
    sr: f32,
    values: [f32; NPARAMS],
    mode: Mode,
    true_peak: bool,
    gain: Smoother,
    ceiling: Smoother,
    tp_in: [TruePeak; 2],
    tp_out: [TruePeak; 2],
    /// The previous interval's true peak (both channels), since a sample's peak includes the
    /// intervals on both sides of it.
    prev_interval: f32,
    prev_interval_out: f32,
    /// The lookahead window in samples (sliding minimum and moving average length).
    window: usize,
    latency: usize,
    // Sliding minimum: a monotonic queue in a ring.
    dq_val: Vec<f32>,
    dq_idx: Vec<u64>,
    dq_head: usize,
    dq_len: usize,
    t: u64,
    // Release stages.
    env: f32,
    env_slow: f32,
    // Moving average.
    box_buf: Vec<f32>,
    box_pos: usize,
    box_sum: f64,
    delays: [Delay; 2],
    m_in: f32,
    m_out: f32,
    m_gr: f32,
}

impl Limiter {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8000.0);
        let mut values = [0.0; NPARAMS];
        for p in &PARAMS {
            values[p.id as usize] = p.default;
        }
        let max_look = (MAX_LOOKAHEAD * sr).ceil() as usize;
        let max_window = max_look + 1;
        let mut lim = Limiter {
            sr,
            values,
            mode: Mode::Precise,
            true_peak: true,
            gain: Smoother::new(values[param::GAIN as usize], sr),
            ceiling: Smoother::new(values[param::CEILING as usize], sr),
            tp_in: [TruePeak::new(), TruePeak::new()],
            tp_out: [TruePeak::new(), TruePeak::new()],
            prev_interval: 0.0,
            prev_interval_out: 0.0,
            window: 1,
            latency: 0,
            dq_val: vec![1.0; max_window + 1],
            dq_idx: vec![0; max_window + 1],
            dq_head: 0,
            dq_len: 0,
            t: 0,
            env: 1.0,
            env_slow: 1.0,
            box_buf: vec![1.0; max_window],
            box_pos: 0,
            box_sum: 0.0,
            delays: [Delay::new(max_look + TP_DELAY), Delay::new(max_look + TP_DELAY)],
            m_in: -120.0,
            m_out: -120.0,
            m_gr: 0.0,
        };
        lim.configure();
        lim
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    /// Works out the window and latency from Lookahead and True Peak, and restarts the gain
    /// pipeline. The true-peak estimate needs `TP_DELAY` samples of its own lookahead, which
    /// comes out of the window, so the latency is the Lookahead setting. With True Peak on it
    /// is at least [`TP_MIN_LATENCY`]: a gain that jumps from one sample to the next makes new
    /// intersample peaks of its own, so true-peak limiting needs a ramp of some length.
    fn configure(&mut self) {
        let look = (self.v(param::LOOKAHEAD).max(0.0) * 0.001 * self.sr).round() as usize;
        let look = look.min((MAX_LOOKAHEAD * self.sr).ceil() as usize);
        let (detect_delay, min_latency) = if self.true_peak { (TP_DELAY, TP_MIN_LATENCY) } else { (0, 0) };
        self.latency = look.max(min_latency);
        self.window = self.latency - detect_delay + 1;
        for d in &mut self.delays {
            d.set_len(self.latency);
        }
        self.reset_pipeline();
    }

    fn reset_pipeline(&mut self) {
        self.dq_head = 0;
        self.dq_len = 0;
        self.t = 0;
        self.env = 1.0;
        self.env_slow = 1.0;
        self.box_buf[..self.window].fill(1.0);
        self.box_pos = 0;
        self.box_sum = self.window as f64;
    }

    /// Pushes the needed gain into the sliding minimum and returns the window's minimum.
    #[inline]
    fn sliding_min(&mut self, v: f32) -> f32 {
        let cap = self.dq_val.len();
        let t = self.t;
        self.t += 1;
        while self.dq_len > 0 {
            let back = (self.dq_head + self.dq_len - 1) % cap;
            if self.dq_val[back] >= v {
                self.dq_len -= 1;
            } else {
                break;
            }
        }
        let pos = (self.dq_head + self.dq_len) % cap;
        self.dq_val[pos] = v;
        self.dq_idx[pos] = t;
        self.dq_len += 1;
        while self.dq_idx[self.dq_head] + self.window as u64 <= t {
            self.dq_head = (self.dq_head + 1) % cap;
            self.dq_len -= 1;
        }
        self.dq_val[self.dq_head]
    }

    /// Pushes a gain into the moving average and returns the average. The running sum is
    /// recomputed from scratch once per window so rounding never drifts (and a window of
    /// unity gains averages to exactly 1.0).
    #[inline]
    fn moving_average(&mut self, v: f32) -> f32 {
        let w = self.window;
        if w == 1 {
            return v;
        }
        self.box_sum += v as f64 - self.box_buf[self.box_pos] as f64;
        self.box_buf[self.box_pos] = v;
        self.box_pos += 1;
        if self.box_pos == w {
            self.box_pos = 0;
            self.box_sum = self.box_buf[..w].iter().map(|&g| g as f64).sum();
        }
        (self.box_sum / w as f64) as f32
    }
}

impl Effect for Limiter {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(slot) = self.values.get_mut(id as usize) else { return };
        *slot = value;
        match id {
            param::GAIN => self.gain.set(value),
            param::CEILING => self.ceiling.set(value.min(0.0)),
            param::MODE => {
                self.mode = match value.round() as i32 {
                    1 => Mode::Smooth,
                    2 => Mode::Adaptive,
                    _ => Mode::Precise,
                }
            }
            param::TRUE_PEAK => {
                let on = value >= 0.5;
                if on != self.true_peak {
                    self.true_peak = on;
                    self.configure();
                }
            }
            param::LOOKAHEAD => self.configure(),
            _ => {}
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, _ctx: &FxContext) {
        let n = l.len().min(r.len());
        let release_ms = self.v(param::RELEASE).max(1.0);
        let a_rel = 1.0 - coeff(release_ms, self.sr);
        // Smooth/Adaptive: a second stage that sinks only under sustained limiting (80 ms)
        // and recovers five times slower than the Release setting.
        let a_slow_att = 1.0 - coeff(80.0, self.sr);
        let a_slow_rel = 1.0 - coeff(release_ms * 5.0, self.sr);
        let mode = self.mode;
        let tp = self.true_peak;

        let mut m_in = 0.0f32;
        let mut m_out = 0.0f32;
        let mut min_gain = 1.0f32;
        for i in 0..n {
            let drive = db_to_lin(self.gain.next());
            let ceil_db = self.ceiling.next();
            let ceil = db_to_lin(ceil_db);
            m_in = m_in.max(l[i].abs()).max(r[i].abs());
            let mut xl = l[i] * drive;
            let mut xr = r[i] * drive;
            if mode == Mode::Adaptive {
                xl = ceil * adaptive_shape(xl / ceil);
                xr = ceil * adaptive_shape(xr / ceil);
            }

            // The peak of the sample now leaving the detector's own lookahead.
            let peak = if tp {
                let inter = self.tp_in[0].push(xl).max(self.tp_in[1].push(xr));
                let sample = self.tp_in[0].center().abs().max(self.tp_in[1].center().abs());
                let p = sample.max(inter).max(self.prev_interval);
                self.prev_interval = inter;
                p
            } else {
                xl.abs().max(xr.abs())
            };

            // With True Peak on, aim a hair (0.01 dB) under the ceiling: the 4× estimate can
            // still read a few thousandths of a dB low on dense top-end material.
            let target = if tp { ceil * TP_MARGIN } else { ceil };
            let hard = if peak > target { target / peak } else { 1.0 };
            let need = match mode {
                Mode::Precise => hard,
                Mode::Smooth | Mode::Adaptive => {
                    let over = lin_to_db(peak) - ceil_db;
                    if over < -12.0 {
                        1.0
                    } else {
                        // A soft minimum of the level and the ceiling: always below both.
                        let g = db_to_lin(-SMOOTH_KNEE * (over / SMOOTH_KNEE).exp().ln_1p());
                        g.min(hard)
                    }
                }
            };

            let held = self.sliding_min(need);
            // Release: rise back towards the held gain, never past it; drops are instant.
            self.env = if held < self.env { held } else { held + a_rel * (self.env - held) };
            let mut g = self.env;
            if mode != Mode::Precise {
                let a = if held < self.env_slow { a_slow_att } else { a_slow_rel };
                self.env_slow = held + a * (self.env_slow - held);
                g = g.min(self.env_slow);
            }
            let g = self.moving_average(g).min(1.0);
            min_gain = min_gain.min(g);

            let yl = (self.delays[0].process(xl) * g).clamp(-ceil, ceil);
            let yr = (self.delays[1].process(xr) * g).clamp(-ceil, ceil);
            l[i] = yl;
            r[i] = yr;
            if tp {
                let inter = self.tp_out[0].push(yl).max(self.tp_out[1].push(yr));
                m_out = m_out.max(inter).max(self.prev_interval_out).max(yl.abs()).max(yr.abs());
                self.prev_interval_out = inter;
            } else {
                m_out = m_out.max(yl.abs()).max(yr.abs());
            }
        }
        self.m_in = lin_to_db(m_in);
        self.m_out = lin_to_db(m_out);
        self.m_gr = -lin_to_db(min_gain);
    }

    fn reset(&mut self) {
        self.gain.snap();
        self.ceiling.snap();
        for tp in self.tp_in.iter_mut().chain(&mut self.tp_out) {
            tp.reset();
        }
        self.prev_interval = 0.0;
        self.prev_interval_out = 0.0;
        for d in &mut self.delays {
            d.reset();
        }
        self.reset_pipeline();
        self.m_in = -120.0;
        self.m_out = -120.0;
        self.m_gr = 0.0;
    }

    fn latency(&self) -> usize {
        self.latency
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        out[0] = self.m_in;
        out[1] = self.m_out;
        out[2] = self.m_gr;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;
    const CTX: FxContext = FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: 0.0 };

    fn limiter(settings: &[(u32, f32)]) -> Limiter {
        let mut lim = Limiter::new(SR);
        for &(id, v) in settings {
            lim.set_param(id, v);
        }
        lim.reset();
        lim
    }

    fn run(lim: &mut Limiter, l: &mut [f32], r: &mut [f32]) {
        for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            lim.process(bl, br, None, &CTX);
        }
    }

    /// A hard test signal at +12 dBFS: tones that put peaks between samples (fs/4 at 45°,
    /// and others near the top of the band), a kick-like thump and a burst of dense,
    /// noise-like program (random tones up to 18 kHz, like real material above which there is
    /// little energy).
    fn hot_signal(len: usize, seed: u32) -> (Vec<f32>, Vec<f32>) {
        let mut state = seed;
        let mut rand = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32
        };
        let tau = std::f32::consts::TAU;
        let mut cluster = || -> Vec<(f32, f32)> { (0..24).map(|_| (200.0 + 17800.0 * rand(), tau * rand())).collect() };
        let (burst, side) = (cluster(), cluster());
        let dense = |set: &[(f32, f32)], t: f32| set.iter().map(|&(f, ph)| (tau * f * t + ph).sin()).sum::<f32>() / 5.0;
        let mut l = Vec::with_capacity(len);
        let mut r = Vec::with_capacity(len);
        for n in 0..len {
            let t = n as f32 / SR;
            let mut x = 0.45 * (tau * 12000.0 * t + std::f32::consts::FRAC_PI_4).sin()
                + 0.3 * (tau * 997.0 * t).sin()
                + 0.25 * (tau * 15013.0 * t).sin();
            let sec = n % 24000;
            if sec < 2400 {
                // A decaying 60 Hz thump every half second.
                x += 1.2 * (tau * 60.0 * t).sin() * (-(sec as f32) / 600.0).exp();
            }
            if (12000..14000).contains(&sec) {
                let w = 0.5 - 0.5 * (tau * (sec - 12000) as f32 / 2000.0).cos();
                x += w * dense(&burst, t);
            }
            l.push(4.0 * x / 1.6);
            r.push(4.0 * (0.8 * x + 0.2 * dense(&side, t)) / 1.6);
        }
        (l, r)
    }

    /// A reference true-peak measurement: 16× windowed-sinc reconstruction over ±64 samples,
    /// far more accurate than the limiter's own 4× estimator.
    fn reference_true_peak(x: &[f32]) -> f32 {
        let half = 64i64;
        let mut peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64;
        for n in half as usize..x.len() - half as usize - 1 {
            if x[n].abs().max(x[n + 1].abs()) < 0.3 {
                continue;
            }
            for k in 1..16 {
                let t = n as f64 + k as f64 / 16.0;
                let mut acc = 0.0f64;
                for m in (n as i64 - half + 1)..=(n as i64 + half) {
                    let d = t - m as f64;
                    let sinc = (std::f64::consts::PI * d).sin() / (std::f64::consts::PI * d);
                    let w = 0.5 + 0.5 * (std::f64::consts::PI * d / (half as f64 + 1.0)).cos();
                    acc += x[m as usize] as f64 * sinc * w;
                }
                peak = peak.max(acc.abs());
            }
        }
        peak as f32
    }

    #[test]
    fn precise_never_exceeds_the_ceiling_even_between_samples() {
        for (seed, ceil_db, look) in [(1, -0.1, 2.0), (5, -1.0, 5.0), (9, -0.3, 0.0)] {
            let mut lim = limiter(&[(param::CEILING, ceil_db), (param::LOOKAHEAD, look)]);
            let (mut l, mut r) = hot_signal(48000, seed);
            run(&mut lim, &mut l, &mut r);
            let ceil = db_to_lin(ceil_db);
            let sample_peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(sample_peak <= ceil, "sample peak {sample_peak}");
            // It is actually limiting (about 12 dB), not just turning down.
            assert!(sample_peak > 0.95 * ceil);
            for ch in [&l, &r] {
                let tp = lin_to_db(reference_true_peak(ch));
                assert!(tp <= ceil_db, "seed {seed}, lookahead {look}: true peak {tp} dBTP over {ceil_db}");
            }
        }
    }

    #[test]
    fn every_mode_holds_the_ceiling() {
        for (mode, tp, look) in [(0.0, 0.0, 0.0), (0.0, 0.0, 5.0), (1.0, 1.0, 2.0), (2.0, 1.0, 2.0), (1.0, 0.0, 0.0), (2.0, 0.0, 10.0)] {
            let mut lim = limiter(&[(param::MODE, mode), (param::TRUE_PEAK, tp), (param::LOOKAHEAD, look), (param::CEILING, -1.0), (param::GAIN, 6.0)]);
            let (mut l, mut r) = hot_signal(30000, 7);
            run(&mut lim, &mut l, &mut r);
            let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak <= db_to_lin(-1.0), "mode {mode} tp {tp} lookahead {look}: {peak}");
            let mut m = [0.0; MAX_METERS];
            lim.meters(&mut m);
            // (Adaptive's saturator does much of the work before the limiter sees it.)
            if mode < 2.0 {
                assert!(m[2] > 6.0, "gain reduction {}", m[2]);
            }
        }
    }

    #[test]
    fn latency_equals_lookahead() {
        for (look, tp, want) in [(2.0, 1.0, 96), (5.0, 0.0, 240), (0.0, 0.0, 0), (0.0, 1.0, TP_MIN_LATENCY), (20.0, 1.0, 960)] {
            let lim = limiter(&[(param::LOOKAHEAD, look), (param::TRUE_PEAK, tp)]);
            assert_eq!(lim.latency(), want, "lookahead {look} ms, true peak {tp}");
        }
    }

    #[test]
    fn transparent_below_the_ceiling() {
        for (tp, look) in [(1.0, 2.0), (0.0, 5.0), (0.0, 0.0)] {
            let mut lim = limiter(&[(param::TRUE_PEAK, tp), (param::LOOKAHEAD, look)]);
            let lat = lim.latency();
            let src: Vec<f32> = (0..20000).map(|n| 0.5 * (n as f32 * 0.031).sin() + 0.3 * (n as f32 * 0.0021).sin()).collect();
            let mut l = src.clone();
            let mut r: Vec<f32> = src.iter().map(|v| -v).collect();
            run(&mut lim, &mut l, &mut r);
            for i in 0..src.len() - lat {
                assert_eq!(l[i + lat], src[i], "sample {i} (tp {tp}, lookahead {look})");
                assert_eq!(r[i + lat], -src[i]);
            }
        }
    }

    #[test]
    fn presets_are_valid() {
        assert!((5..=8).contains(&PRESETS.len()));
        super::super::dyn_common::check_presets(&PARAMS, PRESETS);
    }

    #[test]
    fn silence_stays_clean() {
        for mode in [0.0, 1.0, 2.0] {
            let mut lim = limiter(&[(param::MODE, mode)]);
            let (mut l, mut r) = hot_signal(4800, 3);
            run(&mut lim, &mut l, &mut r);
            let mut l = vec![0.0f32; 96000];
            let mut r = vec![0.0f32; 96000];
            run(&mut lim, &mut l, &mut r);
            // Past the lookahead delay still holding the burst, silence in is silence out.
            assert!(l[1000..].iter().chain(&r[1000..]).all(|v| *v == 0.0), "mode {mode}");
            assert!(lim.env.is_finite() && lim.env_slow.is_finite());
        }
    }
}
