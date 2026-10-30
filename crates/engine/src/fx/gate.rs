//! Noise Gate, after Logic's: closes (turns the signal down by Reduction) while the level stays
//! under the threshold, and opens again when it rises above it.
//!
//! - The detector listens to the input, or to the side-chain key when one is picked, through
//!   the key's High Cut and Low Cut filters. Monitor plays what the detector hears.
//! - Hysteresis sets how far under the threshold the level must fall before the gate closes,
//!   so a signal hovering around the threshold doesn't make it chatter. Hold keeps it open that
//!   long after the level falls.
//! - Attack and Release are how long opening and closing take (10 % to 90 % of the way). The
//!   gain glides between open and closed, so opening and closing never click.
//! - Lookahead delays the audio (and reports that latency for delay compensation) so the gate
//!   is already open when a transient arrives.

use super::biquad::{Biquad, BUTTERWORTH_Q};
use super::util::{coeff, db_to_gain, gain_to_db};
use super::*;

pub const THRESHOLD: u32 = 0;
pub const REDUCTION: u32 = 1;
pub const ATTACK: u32 = 2;
pub const HOLD: u32 = 3;
pub const RELEASE: u32 = 4;
pub const HYSTERESIS: u32 = 5;
pub const LOOKAHEAD: u32 = 6;
pub const HIGH_CUT: u32 = 7;
pub const LOW_CUT: u32 = 8;
pub const MONITOR: u32 = 9;

/// The longest lookahead (ms); the delay line is sized for it.
const MAX_LOOKAHEAD_MS: f32 = 20.0;
/// High Cut at its top and Low Cut at its bottom leave the key unfiltered.
const HIGH_CUT_OFF: f32 = 20000.0;
const LOW_CUT_OFF: f32 = 20.0;
/// How fast the detector's peak level falls back (ms). Slow enough not to ripple on bass notes,
/// quick next to the gate's own Hold and Release.
const DETECTOR_RELEASE_MS: f32 = 20.0;

const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind: ParamKind::Continuous, log }
}

static PARAMS: [ParamSpec; 10] = [
    spec(THRESHOLD, "Threshold", -80.0, 0.0, -40.0, Unit::Db, false),
    spec(REDUCTION, "Reduction", -100.0, 0.0, -100.0, Unit::Db, false),
    spec(ATTACK, "Attack", 0.01, 200.0, 1.0, Unit::Ms, true),
    spec(HOLD, "Hold", 0.0, 1000.0, 50.0, Unit::Ms, false),
    spec(RELEASE, "Release", 1.0, 5000.0, 150.0, Unit::Ms, true),
    spec(HYSTERESIS, "Hysteresis", -30.0, 0.0, -6.0, Unit::Db, false),
    spec(LOOKAHEAD, "Lookahead", 0.0, MAX_LOOKAHEAD_MS, 0.0, Unit::Ms, false),
    spec(HIGH_CUT, "High Cut", 100.0, HIGH_CUT_OFF, HIGH_CUT_OFF, Unit::Hz, true),
    spec(LOW_CUT, "Low Cut", LOW_CUT_OFF, 10000.0, LOW_CUT_OFF, Unit::Hz, true),
    ParamSpec { id: MONITOR, name: "Monitor", min: 0.0, max: 1.0, default: 0.0, unit: Unit::None, kind: ParamKind::Toggle, log: false },
];

static METERS: [&str; 3] = ["Input", "Key", "Reduction"];

/// Starting points for common gating jobs. The key filters aim the detector at the instrument
/// so bleed from the rest of the kit doesn't open the gate.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Tight Kick",
        values: &[(THRESHOLD, -30.0), (REDUCTION, -60.0), (ATTACK, 0.5), (HOLD, 30.0), (RELEASE, 80.0), (HYSTERESIS, -6.0), (LOOKAHEAD, 2.0), (HIGH_CUT, 2500.0), (LOW_CUT, 30.0)],
    },
    FactoryPreset {
        name: "Snare",
        values: &[(THRESHOLD, -28.0), (REDUCTION, -40.0), (ATTACK, 0.3), (HOLD, 50.0), (RELEASE, 140.0), (HYSTERESIS, -6.0), (LOOKAHEAD, 1.5), (HIGH_CUT, 9000.0), (LOW_CUT, 150.0)],
    },
    FactoryPreset {
        name: "Tom Rumble Killer",
        values: &[(THRESHOLD, -32.0), (REDUCTION, -30.0), (ATTACK, 1.0), (HOLD, 120.0), (RELEASE, 350.0), (HYSTERESIS, -8.0), (LOOKAHEAD, 2.0), (HIGH_CUT, 5000.0), (LOW_CUT, 60.0)],
    },
    FactoryPreset {
        name: "Vocal Breath",
        values: &[(THRESHOLD, -45.0), (REDUCTION, -12.0), (ATTACK, 5.0), (HOLD, 150.0), (RELEASE, 300.0), (HYSTERESIS, -6.0), (LOW_CUT, 120.0)],
    },
    FactoryPreset {
        name: "Guitar Amp Hiss",
        values: &[(THRESHOLD, -52.0), (REDUCTION, -40.0), (ATTACK, 2.0), (HOLD, 80.0), (RELEASE, 250.0), (HYSTERESIS, -8.0), (HIGH_CUT, 6000.0), (LOW_CUT, 80.0)],
    },
    FactoryPreset {
        name: "Gentle Cleanup",
        values: &[(THRESHOLD, -55.0), (REDUCTION, -15.0), (ATTACK, 3.0), (HOLD, 100.0), (RELEASE, 500.0), (HYSTERESIS, -10.0)],
    },
];

pub fn info() -> EffectInfo {
    EffectInfo { id: "builtin:noise-gate", name: "Noise Gate", category: "Dynamics", params: &PARAMS, meters: &METERS, sidechain: true, new: |sr| Box::new(Gate::new(sr)) }
}

/// A time (ms) for 10 % to 90 % of a change, as a one-pole coefficient.
fn ramp(ms: f32, sr: f32) -> f32 {
    coeff(ms / 2.2, sr)
}

pub struct Gate {
    sr: f32,
    values: [f32; PARAMS.len()],
    // Derived from the values.
    open_db: f32,
    close_db: f32,
    floor: f32,
    attack: f32,
    release: f32,
    hold: usize,
    lookahead: usize,
    /// Key filters, per channel.
    hp: [Biquad; 2],
    lp: [Biquad; 2],
    detector_fall: f32,
    /// Monitor's crossfade between the gated signal (0) and the key (1).
    monitor: f32,
    monitor_step: f32,
    // State.
    env: f32,
    open: bool,
    hold_left: usize,
    gain: f32,
    /// Lookahead delay lines for the audio and for the key (Monitor plays it in time).
    line: [Vec<f32>; 4],
    wpos: usize,
    // Meters (linear peak, with a fall-back).
    in_peak: f32,
    meter_fall: f32,
}

impl Gate {
    pub fn new(sample_rate: f32) -> Gate {
        let sr = sample_rate.max(1.0);
        let len = (MAX_LOOKAHEAD_MS * 0.001 * sr).ceil() as usize + 2;
        let mut g = Gate {
            sr,
            values: [0.0; PARAMS.len()],
            open_db: 0.0,
            close_db: 0.0,
            floor: 0.0,
            attack: 1.0,
            release: 1.0,
            hold: 0,
            lookahead: 0,
            hp: [Biquad::default(); 2],
            lp: [Biquad::default(); 2],
            detector_fall: 1.0 - coeff(DETECTOR_RELEASE_MS, sr),
            monitor: 0.0,
            monitor_step: coeff(5.0, sr),
            env: 0.0,
            open: false,
            hold_left: 0,
            gain: 0.0,
            line: [vec![0.0; len], vec![0.0; len], vec![0.0; len], vec![0.0; len]],
            wpos: 0,
            in_peak: 0.0,
            meter_fall: (-1.0 / (0.3 * sr)).exp(),
        };
        for p in &PARAMS {
            g.values[p.id as usize] = p.default;
        }
        g.update();
        g.gain = g.floor;
        g
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    /// Works the per-sample constants out of the parameter values.
    fn update(&mut self) {
        let sr = self.sr;
        self.open_db = self.v(THRESHOLD);
        self.close_db = self.v(THRESHOLD) + self.v(HYSTERESIS);
        self.floor = db_to_gain(self.v(REDUCTION)).min(1.0);
        if self.v(REDUCTION) <= -100.0 {
            self.floor = 0.0;
        }
        self.attack = ramp(self.v(ATTACK), sr);
        self.release = ramp(self.v(RELEASE), sr);
        self.hold = (self.v(HOLD) * 0.001 * sr) as usize;
        self.lookahead = ((self.v(LOOKAHEAD) * 0.001 * sr).round() as usize).min(self.line[0].len() - 2);
        let sr = sr as f64;
        for f in &mut self.lp {
            if self.values[HIGH_CUT as usize] >= HIGH_CUT_OFF {
                f.bypass();
            } else {
                f.lowpass(sr, self.values[HIGH_CUT as usize] as f64, BUTTERWORTH_Q);
            }
        }
        for f in &mut self.hp {
            if self.values[LOW_CUT as usize] <= LOW_CUT_OFF {
                f.bypass();
            } else {
                f.highpass(sr, self.values[LOW_CUT as usize] as f64, BUTTERWORTH_Q);
            }
        }
    }
}

impl Effect for Gate {
    fn set_param(&mut self, id: u32, value: f32) {
        if let Some(v) = self.values.get_mut(id as usize) {
            *v = value;
            self.update();
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key, _ctx: &FxContext) {
        let n = l.len().min(r.len());
        let len = self.line[0].len();
        let monitor_target = if self.v(MONITOR) >= 0.5 { 1.0 } else { 0.0 };
        let mut peak = 0.0f32;
        for i in 0..n {
            let (xl, xr) = (l[i], r[i]);
            peak = peak.max(xl.abs()).max(xr.abs());
            let (kl, kr) = match key {
                Some((a, b)) if i < a.len() && i < b.len() => (a[i], b[i]),
                _ => (xl, xr),
            };
            let kl = self.lp[0].run(self.hp[0].run(kl));
            let kr = self.lp[1].run(self.hp[1].run(kr));
            let k = kl.abs().max(kr.abs());
            self.env = if k > self.env { k } else { self.env * self.detector_fall };

            let db = gain_to_db(self.env);
            if db >= self.open_db {
                self.open = true;
                self.hold_left = self.hold;
            } else if self.open {
                if db >= self.close_db {
                    self.hold_left = self.hold;
                } else if self.hold_left > 0 {
                    self.hold_left -= 1;
                } else {
                    self.open = false;
                }
            }
            let target = if self.open { 1.0 } else { self.floor };
            let c = if target > self.gain { self.attack } else { self.release };
            self.gain += (target - self.gain) * c;

            // The audio (and key) come out `lookahead` samples late.
            let w = self.wpos;
            self.line[0][w] = xl;
            self.line[1][w] = xr;
            self.line[2][w] = kl;
            self.line[3][w] = kr;
            let rd = (w + len - self.lookahead) % len;
            self.wpos = (w + 1) % len;
            self.monitor += (monitor_target - self.monitor) * self.monitor_step;
            let m = self.monitor;
            l[i] = self.line[0][rd] * self.gain * (1.0 - m) + self.line[2][rd] * m;
            r[i] = self.line[1][rd] * self.gain * (1.0 - m) + self.line[3][rd] * m;
        }
        self.in_peak = peak.max(self.in_peak * self.meter_fall.powi(n as i32));
    }

    fn reset(&mut self) {
        self.hp.iter_mut().chain(self.lp.iter_mut()).for_each(|f| f.reset());
        self.line.iter_mut().for_each(|b| b.fill(0.0));
        self.env = 0.0;
        self.open = false;
        self.hold_left = 0;
        self.gain = self.floor;
        self.in_peak = 0.0;
        self.monitor = if self.v(MONITOR) >= 0.5 { 1.0 } else { 0.0 };
    }

    fn latency(&self) -> usize {
        self.lookahead
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        out[0] = gain_to_db(self.in_peak).max(-100.0);
        out[1] = gain_to_db(self.env).max(-100.0);
        out[2] = (-gain_to_db(self.gain)).clamp(0.0, 100.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;
    const CTX: FxContext = FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: 0.0 };

    fn sine(db: f32, freq: f32, n: usize) -> Vec<f32> {
        let a = db_to_gain(db);
        (0..n).map(|i| a * (std::f32::consts::TAU * freq * i as f32 / SR).sin()).collect()
    }

    /// Runs `input` through the gate in engine-sized blocks; returns the left output.
    fn run(g: &mut Gate, input: &[f32], key: Option<&[f32]>) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len());
        for (k, chunk) in input.chunks(512).enumerate() {
            let mut l = chunk.to_vec();
            let mut r = chunk.to_vec();
            match key {
                Some(kk) => {
                    let s = &kk[k * 512..k * 512 + chunk.len()];
                    g.process(&mut l, &mut r, Some((s, s)), &CTX);
                }
                None => g.process(&mut l, &mut r, None, &CTX),
            }
            out.extend_from_slice(&l);
        }
        out
    }

    fn peak_db(x: &[f32]) -> f32 {
        gain_to_db(x.iter().fold(0.0f32, |m, s| m.max(s.abs())))
    }

    #[test]
    fn passes_above_and_attenuates_below_the_threshold() {
        let tone = sine(-20.0, 440.0, 48000);
        let mut g = Gate::new(SR);
        g.set_param(REDUCTION, -40.0);
        g.set_param(THRESHOLD, -40.0);
        let out = run(&mut g, &tone, None);
        assert!((peak_db(&out[24000..]) + 20.0).abs() < 0.2, "open: {}", peak_db(&out[24000..]));
        let mut g = Gate::new(SR);
        g.set_param(REDUCTION, -40.0);
        g.set_param(THRESHOLD, -10.0);
        let out = run(&mut g, &tone, None);
        assert!((peak_db(&out[24000..]) + 60.0).abs() < 0.5, "closed: {}", peak_db(&out[24000..]));
        let m = &mut [0.0; MAX_METERS];
        g.meters(m);
        assert!((m[2] - 40.0).abs() < 0.5 && (m[0] + 20.0).abs() < 0.5, "{m:?}");
    }

    #[test]
    fn attack_hold_and_release_take_their_time() {
        // 0.25 s of silence, 0.5 s of tone, then silence.
        let mut input = vec![0.0; 12000];
        input.extend(sine(-12.0, 1000.0, 24000));
        input.extend(vec![0.0; 48000]);
        let mut g = Gate::new(SR);
        g.set_param(ATTACK, 10.0);
        g.set_param(HOLD, 100.0);
        g.set_param(RELEASE, 200.0);
        // Watch the gain itself through a constant signal at the key's level.
        let ones = vec![0.5f32; input.len()];
        let mut gains = Vec::new();
        for (k, chunk) in ones.chunks(480).enumerate() {
            let mut l = chunk.to_vec();
            let mut r = chunk.to_vec();
            let s = &input[k * 480..k * 480 + chunk.len()];
            g.process(&mut l, &mut r, Some((s, s)), &CTX);
            gains.extend(l.iter().map(|v| v / 0.5));
        }
        let t = |from: usize, level: f32, rising: bool| (from..gains.len()).find(|&i| if rising { gains[i] >= level } else { gains[i] <= level }).unwrap() - from;
        // 10 % to 90 % of the opening takes the attack time.
        let open = t(12000, 0.1, true);
        let rise = t(12000 + open, 0.9, true);
        assert!((rise as f32 - 480.0).abs() < 60.0, "attack {rise} samples");
        // Closing waits for Hold, then falls 10 % → 90 % (gain 0.9 → 0.1) in the release time.
        let start = t(36000, 0.9, false);
        assert!(start as f32 > 0.1 * SR * 0.95, "held {start}");
        let fall = t(36000 + start, 0.1, false);
        assert!((fall as f32 - 0.2 * SR).abs() < 600.0, "release {fall}");
    }

    #[test]
    fn hysteresis_keeps_a_hovering_signal_open() {
        // Opens at -20 dB; a tone at -23 dB then keeps it open with -6 dB of hysteresis.
        let mut input = sine(-15.0, 500.0, 4800);
        input.extend(sine(-23.0, 500.0, 48000));
        let mut g = Gate::new(SR);
        g.set_param(THRESHOLD, -20.0);
        g.set_param(HOLD, 0.0);
        let out = run(&mut g, &input, None);
        assert!((peak_db(&out[40000..]) + 23.0).abs() < 0.3);
        let mut g = Gate::new(SR);
        g.set_param(THRESHOLD, -20.0);
        g.set_param(HYSTERESIS, 0.0);
        g.set_param(HOLD, 0.0);
        let out = run(&mut g, &input, None);
        assert!(peak_db(&out[40000..]) < -80.0);
    }

    #[test]
    fn lookahead_is_the_latency() {
        let mut g = Gate::new(SR);
        g.set_param(THRESHOLD, -80.0);
        g.set_param(LOOKAHEAD, 5.0);
        assert_eq!(g.latency(), 240);
        let mut x = vec![0.0f32; 1024];
        x[100] = 1.0;
        let out = run(&mut g, &x, None);
        let at = out.iter().position(|v| v.abs() > 0.5).unwrap();
        assert_eq!(at, 340);
        g.set_param(LOOKAHEAD, 0.0);
        assert_eq!(g.latency(), 0);
    }

    #[test]
    fn the_key_opens_the_gate() {
        let tone = sine(-12.0, 440.0, 24000);
        let silence = vec![0.0; 24000];
        let mut g = Gate::new(SR);
        let shut = run(&mut g, &tone, Some(&silence));
        assert!(peak_db(&shut[12000..]) < -80.0);
        let mut g = Gate::new(SR);
        let key = sine(-12.0, 100.0, 24000);
        let open = run(&mut g, &tone, Some(&key));
        assert!(peak_db(&open[12000..]) > -12.5);
        // Low Cut above the key's pitch keeps it shut again.
        let mut g = Gate::new(SR);
        g.set_param(LOW_CUT, 2000.0);
        let out = run(&mut g, &tone, Some(&key));
        assert!(peak_db(&out[12000..]) < -80.0);
    }
}
