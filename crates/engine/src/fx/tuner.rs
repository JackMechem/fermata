//! Tuner: a chromatic tuner for guitar, bass and voice, after Logic's Tuner.
//!
//! The audio passes through untouched (or is silenced while Mute is on). The detector runs on a
//! mono, low-passed copy of the input in two stages:
//!
//! 1. **Coarse:** the McLeod Pitch Method (normalized square difference function, NSDF) on a
//!    copy decimated to about 12 kHz, over lags from [`F_MAX`] down to [`F_MIN`]. Picking the
//!    first NSDF peak within 90 % of the highest one avoids octave errors in both directions.
//! 2. **Refine:** the NSDF again at the full rate, only for the few lags around the coarse
//!    period, over a window of a whole number of periods, with parabolic interpolation. At the
//!    full rate the peak spans many samples, so the interpolation is accurate to a small
//!    fraction of a cent even for high notes.
//!
//! Low-passing (and the DC blocker) can't move the period of a periodic signal, but they do
//! strip the high harmonics that would otherwise make the NSDF peak too sharp to interpolate.
//! The detector runs every [`HOP_SECS`], at the end of a block, so its cost is bounded per block.

use super::{EffectInfo, Effect, FactoryPreset, FxContext, Key, ParamKind, ParamSpec, Unit, MAX_METERS};
use std::f64::consts::TAU;

pub const REFERENCE: u32 = 0;
pub const MUTE: u32 = 1;
pub const DISPLAY: u32 = 2;
pub const NOTATION: u32 = 3;
pub const TRANSPOSE: u32 = 4;
pub const THRESHOLD: u32 = 5;

static PARAMS: [ParamSpec; 6] = [
    ParamSpec {
        id: REFERENCE,
        name: "Reference",
        min: 400.0,
        max: 480.0,
        default: 440.0,
        unit: Unit::Hz,
        kind: ParamKind::Continuous,
        log: false,
    },
    ParamSpec {
        id: MUTE,
        name: "Mute",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: Unit::None,
        kind: ParamKind::Toggle,
        log: false,
    },
    ParamSpec {
        id: DISPLAY,
        name: "Display",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: Unit::None,
        kind: ParamKind::Choice(&["Needle", "Strobe"]),
        log: false,
    },
    ParamSpec {
        id: NOTATION,
        name: "Notation",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: Unit::None,
        kind: ParamKind::Choice(&["Sharps", "Flats"]),
        log: false,
    },
    ParamSpec {
        id: TRANSPOSE,
        name: "Transpose",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        unit: Unit::Semitones,
        kind: ParamKind::Continuous,
        log: false,
    },
    ParamSpec {
        id: THRESHOLD,
        name: "Threshold",
        min: -90.0,
        max: -20.0,
        default: -60.0,
        unit: Unit::Db,
        kind: ParamKind::Continuous,
        log: false,
    },
];

/// Meter order: detected frequency in Hz (0 when nothing is detected), the nearest MIDI note
/// after Transpose (-1 when nothing is detected), cents off that note (-50..50), confidence
/// (0..1) and input level in dBFS.
static METERS: [&str; 5] = ["frequency", "note", "cents", "confidence", "level"];

/// Factory presets. Each lists only what it changes from the defaults (A4 = 440 Hz, Needle,
/// sharps, no transpose, gate at -60 dB, output on).
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Guitar (A440)", values: &[(REFERENCE, 440.0)] },
    // A lower gate holds a decaying low string longer.
    FactoryPreset { name: "Bass", values: &[(REFERENCE, 440.0), (THRESHOLD, -70.0)] },
    FactoryPreset { name: "Baroque A415", values: &[(REFERENCE, 415.0), (NOTATION, 1.0)] },
    FactoryPreset { name: "Orchestra A442", values: &[(REFERENCE, 442.0)] },
    FactoryPreset { name: "B♭ Instrument", values: &[(TRANSPOSE, 2.0), (NOTATION, 1.0)] },
    FactoryPreset { name: "E♭ Instrument", values: &[(TRANSPOSE, 9.0), (NOTATION, 1.0)] },
    FactoryPreset { name: "Mute While Tuning", values: &[(MUTE, 1.0)] },
    FactoryPreset { name: "Strobe", values: &[(DISPLAY, 1.0)] },
];

pub fn info() -> EffectInfo {
    EffectInfo {
        id: "builtin:tuner",
        name: "Tuner",
        category: "Metering",
        params: &PARAMS,
        meters: &METERS,
        sidechain: false,
        new: |sr| Box::new(Tuner::new(sr)),
    }
}

/// The lowest pitch detected: below drop-A (27.5 Hz) on a 5-string bass, whose low B is 30.9 Hz.
pub const F_MIN: f64 = 25.0;
/// The highest pitch detected: above C7 (2093 Hz), the top of a soprano's range.
pub const F_MAX: f64 = 2200.0;
/// The detector's low-pass corner.
const LOWPASS_HZ: f64 = 3000.0;
/// The coarse stage's target rate.
const COARSE_RATE: f64 = 12000.0;
/// How often the detector runs.
const HOP_SECS: f64 = 0.02;
/// The shortest refine window; low notes use two periods instead when that is longer.
const REFINE_SECS: f64 = 0.03;
/// NSDF peaks below this are noise, not pitch.
const CLARITY_MIN: f32 = 0.8;
/// Picks the first NSDF peak at least this fraction of the highest one (McLeod's k).
const PEAK_RATIO: f32 = 0.9;
/// A pitch change bigger than this (semitones) is a new note, not drift.
const JUMP: f64 = 0.3;
/// The time constant for smoothing the pitch of a held note.
const SMOOTH_SECS: f64 = 0.08;
/// How long a note stays on screen after the detector loses it.
const HOLD_SECS: f64 = 0.2;
/// How far past the half-way point (semitones) the pitch must move before the note changes.
const NOTE_HYSTERESIS: f64 = 0.08;
const MAX_KEY_PEAKS: usize = 32;
const LEVEL_FLOOR: f32 = -120.0;

/// A biquad in transposed direct form II, run in f64.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// The RBJ-cookbook low-pass.
    fn lowpass(sr: f64, f: f64, q: f64) -> Biquad {
        let w = TAU * f / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        Biquad {
            b0: (1.0 - c) / 2.0 / a0,
            b1: (1.0 - c) / a0,
            b2: (1.0 - c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
            ..Biquad::default()
        }
    }

    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn clear(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// A power-of-two ring of the most recent samples.
struct Ring {
    buf: Vec<f32>,
    /// The next write index.
    pos: usize,
    /// How many samples have been written since the last clear, saturating at the length.
    filled: usize,
}

impl Ring {
    fn new(min_len: usize) -> Ring {
        Ring { buf: vec![0.0; min_len.next_power_of_two()], pos: 0, filled: 0 }
    }

    #[inline]
    fn push(&mut self, x: f32) {
        let mask = self.buf.len() - 1;
        self.buf[self.pos] = x;
        self.pos = (self.pos + 1) & mask;
        self.filled = (self.filled + 1).min(self.buf.len());
    }

    /// Copies the latest `n` samples, oldest first, into `out[..n]`.
    fn latest(&self, n: usize, out: &mut [f32]) {
        let mask = self.buf.len() - 1;
        let start = self.pos.wrapping_sub(n) & mask;
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = self.buf[(start + i) & mask];
        }
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
        self.filled = 0;
    }
}

pub struct Tuner {
    sr: f64,
    reference: f32,
    mute: bool,
    display: f32,
    notation: f32,
    transpose: f32,
    threshold: f32,

    // Detector front end: DC blocker, 4th-order Butterworth low-pass, decimator.
    dc_coeff: f64,
    dc_x1: f64,
    dc_y1: f64,
    lowpass: [Biquad; 2],
    decim: usize,
    decim_phase: usize,
    full: Ring,
    coarse: Ring,
    /// Scratch for one analysis: the latest samples, unrolled from a ring.
    scratch: Vec<f32>,
    /// The coarse NSDF, indexed by lag.
    nsdf: Vec<f32>,
    /// The refine NSDF, indexed by lag minus the lowest refine lag.
    refine: Vec<f64>,
    tau_lo: usize,
    tau_hi: usize,
    coarse_window: usize,
    hop: usize,
    since_analysis: usize,
    level_ms: f64,
    level_coeff: f64,

    // Tracker.
    active: bool,
    /// The smoothed pitch as a fractional MIDI note against A4 = 440 Hz.
    pitch: f64,
    /// A new pitch waiting for a second analysis to confirm it.
    candidate: Option<f64>,
    missing_for: usize,
    confidence: f32,
    /// The displayed note against the reference (before Transpose).
    note: Option<i32>,
    cents: f32,
}

impl Tuner {
    pub fn new(sample_rate: f32) -> Tuner {
        let sr = sample_rate as f64;
        let decim = (sr / COARSE_RATE).round().max(1.0) as usize;
        let fd = sr / decim as f64;
        let tau_lo = ((fd / F_MAX).floor() as usize).saturating_sub(1).max(2);
        let tau_hi = (fd / F_MIN).ceil() as usize + 2;
        // One longest period of window, plus the lags.
        let coarse_window = tau_hi;
        let coarse_len = coarse_window + tau_hi + 1;
        // The refine window is at most two longest periods or REFINE_SECS plus one, and the lags
        // reach one longest period (plus the search span) past it.
        let p_max = decim * (tau_hi + 1);
        let span = decim + 2;
        let refine_win_max = (2 * p_max).max((REFINE_SECS * sr).ceil() as usize + p_max);
        let full_len = refine_win_max + p_max + span + 2;
        let hop = (HOP_SECS * sr).round().max(1.0) as usize;
        let butterworth_q = [0.541_196_100_146_197, 1.306_562_964_876_376_5];
        Tuner {
            sr,
            reference: 440.0,
            mute: false,
            display: 0.0,
            notation: 0.0,
            transpose: 0.0,
            threshold: -60.0,
            dc_coeff: (-TAU * 10.0 / sr).exp(),
            dc_x1: 0.0,
            dc_y1: 0.0,
            lowpass: butterworth_q.map(|q| Biquad::lowpass(sr, LOWPASS_HZ.min(sr * 0.45), q)),
            decim,
            decim_phase: 0,
            full: Ring::new(full_len),
            coarse: Ring::new(coarse_len),
            scratch: vec![0.0; full_len.max(coarse_len)],
            nsdf: vec![0.0; tau_hi + 1],
            refine: vec![0.0; 2 * span + 1],
            tau_lo,
            tau_hi,
            coarse_window,
            hop,
            since_analysis: 0,
            level_ms: 0.0,
            level_coeff: 1.0 - (-1.0 / (0.1 * sr)).exp(),
            active: false,
            pitch: 0.0,
            candidate: None,
            missing_for: 0,
            confidence: 0.0,
            note: None,
            cents: 0.0,
        }
    }

    /// Finds the pitch in the latest audio: the frequency in Hz and the NSDF peak (0..1).
    fn detect(&mut self) -> Option<(f64, f32)> {
        let (w, tau_hi) = (self.coarse_window, self.tau_hi);
        let n = w + tau_hi + 1;
        if self.coarse.filled < n {
            return None;
        }
        self.coarse.latest(n, &mut self.scratch);
        let x = &self.scratch[..n];

        // The NSDF for every lag up to the longest period. m(τ) is the energy of both windows;
        // the lagged one slides, so update it instead of summing it again.
        let e0: f64 = x[..w].iter().map(|&v| (v as f64) * (v as f64)).sum();
        let mut e1 = e0;
        self.nsdf[0] = 1.0;
        for tau in 1..=tau_hi {
            let (a, b) = (x[tau - 1] as f64, x[tau - 1 + w] as f64);
            e1 += b * b - a * a;
            let r: f32 = x[..w].iter().zip(&x[tau..tau + w]).map(|(a, b)| a * b).sum();
            let m = e0 + e1.max(0.0);
            let v = if m > 1e-12 { (2.0 * r as f64 / m) as f32 } else { 0.0 };
            // A sum can overflow to NaN on a runaway input; the peak search below must never
            // see one, or it stops advancing.
            self.nsdf[tau] = if v.is_finite() { v } else { 0.0 };
        }

        // Key maxima: the highest point of each positive lobe after the zero-lag lobe.
        let nsdf = &self.nsdf;
        let mut peaks = [0usize; MAX_KEY_PEAKS];
        let mut count = 0;
        let mut tau = 1;
        while tau <= tau_hi && nsdf[tau] > 0.0 {
            tau += 1;
        }
        while tau <= tau_hi && count < MAX_KEY_PEAKS {
            while tau <= tau_hi && nsdf[tau] <= 0.0 {
                tau += 1;
            }
            let mut best = tau;
            while tau <= tau_hi && nsdf[tau] > 0.0 {
                if nsdf[tau] > nsdf[best] {
                    best = tau;
                }
                tau += 1;
            }
            // A lobe cut off by the end of the lag range has no interior peak to interpolate.
            if best < tau_hi && best >= self.tau_lo {
                peaks[count] = best;
                count += 1;
            }
        }
        let highest = peaks[..count].iter().map(|&t| nsdf[t]).fold(0.0f32, f32::max);
        let pick = *peaks[..count].iter().find(|&&t| nsdf[t] >= PEAK_RATIO * highest)?;
        let clarity = nsdf[pick];
        if clarity < CLARITY_MIN {
            return None;
        }
        let tau_c = pick as f64 + parabola(nsdf[pick - 1] as f64, clarity as f64, nsdf[pick + 1] as f64);
        let coarse_freq = self.sr / (tau_c * self.decim as f64);
        Some(self.refine(tau_c * self.decim as f64).unwrap_or((coarse_freq, clarity)))
    }

    /// Refines a period estimate (in full-rate samples) on the full-rate signal.
    fn refine(&mut self, period: f64) -> Option<(f64, f32)> {
        let span = self.decim + 2;
        let centre = period.round() as usize;
        let lo = centre.checked_sub(span)?.max(2);
        let hi = centre + span;
        // A whole number of periods keeps the window's edge effects from tilting the peak.
        let periods = ((REFINE_SECS * self.sr / period).ceil()).max(2.0);
        let w = (periods * period).round() as usize;
        let n = w + hi + 1;
        if n > self.full.filled || n > self.scratch.len() {
            return None;
        }
        self.full.latest(n, &mut self.scratch);
        let x = &self.scratch[..n];
        let energy = |s: &[f32]| s.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>();
        let e0 = energy(&x[..w]);
        for tau in lo..=hi {
            // f64 sums: near a low note's peak neighbouring lags differ by about 1e-5.
            let r: f64 = x[..w].iter().zip(&x[tau..tau + w]).map(|(&a, &b)| a as f64 * b as f64).sum();
            let m = e0 + energy(&x[tau..tau + w]);
            self.refine[tau - lo] = if m > 1e-12 { 2.0 * r / m } else { 0.0 };
        }
        let vals = &self.refine[..=hi - lo];
        let (i, &peak) = vals.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
        if i == 0 || i == vals.len() - 1 {
            return None;
        }
        let tau = (lo + i) as f64 + parabola(vals[i - 1], peak, vals[i + 1]);
        Some((self.sr / tau, peak.clamp(0.0, 1.0) as f32))
    }

    /// Folds one analysis into the displayed pitch: smooths drift, follows real note changes
    /// after one confirming analysis, and holds through short dropouts.
    fn track(&mut self, found: Option<(f64, f32)>, elapsed: usize) {
        let dt = elapsed as f64 / self.sr;
        match found {
            Some((freq, clarity)) => {
                let p = 69.0 + 12.0 * (freq / 440.0).log2();
                self.missing_for = 0;
                if !self.active {
                    self.active = true;
                    self.pitch = p;
                    self.confidence = clarity;
                    self.candidate = None;
                } else if (p - self.pitch).abs() > JUMP {
                    match self.candidate {
                        Some(c) if (p - c).abs() <= JUMP => {
                            self.pitch = p;
                            self.candidate = None;
                        }
                        _ => self.candidate = Some(p),
                    }
                } else {
                    self.candidate = None;
                    let a = 1.0 - (-dt / SMOOTH_SECS).exp();
                    self.pitch += a * (p - self.pitch);
                    self.confidence += a as f32 * (clarity - self.confidence);
                }
            }
            None => {
                self.missing_for += elapsed;
                if self.missing_for as f64 > HOLD_SECS * self.sr {
                    self.active = false;
                    self.candidate = None;
                }
            }
        }
        self.update_note();
    }

    /// Works out the note and cents against the reference, with hysteresis on the note.
    fn update_note(&mut self) {
        if !self.active {
            self.note = None;
            self.cents = 0.0;
            return;
        }
        let rel = self.pitch - 12.0 * (self.reference as f64 / 440.0).log2();
        let note = match self.note {
            Some(n) if (rel - n as f64).abs() <= 0.5 + NOTE_HYSTERESIS => n,
            _ => rel.round() as i32,
        };
        self.note = Some(note);
        self.cents = (((rel - note as f64) * 100.0) as f32).clamp(-50.0, 50.0);
    }

    fn frequency(&self) -> f64 {
        if self.active {
            440.0 * 2f64.powf((self.pitch - 69.0) / 12.0)
        } else {
            0.0
        }
    }
}

/// The offset (-0.5..0.5) of the vertex of the parabola through three equally spaced points.
fn parabola(a: f64, b: f64, c: f64) -> f64 {
    let d = a - 2.0 * b + c;
    if d.abs() < 1e-18 {
        0.0
    } else {
        (0.5 * (a - c) / d).clamp(-0.5, 0.5)
    }
}

impl Effect for Tuner {
    fn set_param(&mut self, id: u32, value: f32) {
        match id {
            REFERENCE => {
                self.reference = value;
                self.update_note();
            }
            MUTE => self.mute = value >= 0.5,
            DISPLAY => self.display = value,
            NOTATION => self.notation = value,
            TRANSPOSE => self.transpose = value.round(),
            THRESHOLD => self.threshold = value,
            _ => {}
        }
    }

    fn param(&self, id: u32) -> f32 {
        match id {
            REFERENCE => self.reference,
            MUTE => self.mute as u8 as f32,
            DISPLAY => self.display,
            NOTATION => self.notation,
            TRANSPOSE => self.transpose,
            THRESHOLD => self.threshold,
            _ => 0.0,
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, _ctx: &FxContext) {
        for (&a, &b) in l.iter().zip(r.iter()) {
            let x = 0.5 * (a as f64 + b as f64);
            // One-pole DC blocker at 10 Hz, then the low-pass.
            let y = x - self.dc_x1 + self.dc_coeff * self.dc_y1;
            self.dc_x1 = x;
            self.dc_y1 = y;
            // The level is measured after the DC blocker: an interface's DC offset alone
            // mustn't open the gate, or the detector runs on the blocker's decaying output.
            self.level_ms += self.level_coeff * (y * y - self.level_ms);
            let [lp1, lp2] = &mut self.lowpass;
            let y = lp2.run(lp1.run(y));
            // Far below anything audible (-300 dB); tinier values would square into f32
            // denormals in the NSDF sums, which are very slow where flush-to-zero is off.
            let y = if y.abs() < 1e-15 { 0.0 } else { y };
            self.full.push(y as f32);
            self.decim_phase += 1;
            if self.decim_phase == self.decim {
                self.decim_phase = 0;
                self.coarse.push(y as f32);
            }
        }
        // Keep denormals out of the filter tails after the input goes silent.
        if self.level_ms < 1e-30 {
            self.level_ms = 0.0;
        }

        self.since_analysis += l.len();
        if self.since_analysis >= self.hop {
            let elapsed = self.since_analysis;
            self.since_analysis = 0;
            let found = if self.level_db() >= self.threshold { self.detect() } else { None };
            self.track(found, elapsed);
        }

        if self.mute {
            l.fill(0.0);
            r.fill(0.0);
        }
    }

    fn reset(&mut self) {
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
        for f in &mut self.lowpass {
            f.clear();
        }
        self.decim_phase = 0;
        self.full.clear();
        self.coarse.clear();
        self.since_analysis = 0;
        self.level_ms = 0.0;
        self.active = false;
        self.candidate = None;
        self.missing_for = 0;
        self.confidence = 0.0;
        self.update_note();
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        out[0] = self.frequency() as f32;
        out[1] = match self.note {
            Some(n) => (n + self.transpose as i32) as f32,
            None => -1.0,
        };
        out[2] = self.cents;
        out[3] = if self.active { self.confidence } else { 0.0 };
        out[4] = self.level_db();
    }
}

impl Tuner {
    fn level_db(&self) -> f32 {
        if self.level_ms > 0.0 {
            ((10.0 * self.level_ms.log10()) as f32).max(LEVEL_FLOOR)
        } else {
            LEVEL_FLOOR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn presets_use_real_params_in_range() {
        let info = info();
        for p in PRESETS {
            for &(id, v) in p.values {
                let spec = info.param(id).unwrap_or_else(|| panic!("{}: no parameter {id}", p.name));
                assert!(v >= spec.min && v <= spec.max, "{}: {} = {v} is out of range", p.name, spec.name);
            }
        }
    }

    const SR: f32 = 48000.0;

    fn ctx(sr: f32) -> FxContext {
        FxContext { sample_rate: sr, tempo: 120.0, playing: true, position: 0.0 }
    }

    fn midi_hz(m: f64) -> f64 {
        440.0 * 2f64.powf((m - 69.0) / 12.0)
    }

    /// Feeds `secs` of a generated mono signal through the tuner and returns the meters after
    /// every block.
    fn run(t: &mut Tuner, sr: f32, secs: f64, mut gen: impl FnMut(usize) -> f32) -> Vec<[f32; MAX_METERS]> {
        let total = (secs * sr as f64) as usize;
        let mut out = Vec::new();
        let mut n = 0;
        let block = 512;
        while n < total {
            let len = block.min(total - n);
            let mut l: Vec<f32> = (n..n + len).map(&mut gen).collect();
            let mut r = l.clone();
            t.process(&mut l, &mut r, None, &ctx(sr));
            let mut m = [0.0; MAX_METERS];
            t.meters(&mut m);
            out.push(m);
            n += len;
        }
        out
    }

    fn sine(f: f64, sr: f32, amp: f32) -> impl FnMut(usize) -> f32 {
        move |n| amp * (TAU * f * n as f64 / sr as f64).sin() as f32
    }

    /// A band-limited sawtooth, summed from its harmonics up to Nyquist.
    fn saw(f: f64, sr: f32, amp: f32) -> impl FnMut(usize) -> f32 {
        let harmonics = ((sr as f64 * 0.5) / f) as usize;
        move |n| {
            let ph = TAU * f * n as f64 / sr as f64;
            let s: f64 = (1..=harmonics).map(|k| (k as f64 * ph).sin() / k as f64).sum();
            amp * (s * 2.0 / PI) as f32
        }
    }

    fn cents_between(a: f64, b: f64) -> f64 {
        1200.0 * (a / b).log2()
    }

    /// Checks frequency, note and cents over the sustained part (after 0.5 s).
    fn assert_steady(meters: &[[f32; MAX_METERS]], sr: f32, f: f64, note: i32, label: &str) {
        let start = (0.5 * sr as f64 / 512.0) as usize;
        for m in &meters[start..] {
            let err = cents_between(m[0] as f64, f);
            assert!(err.abs() <= 1.0, "{label}: {} Hz is {err:.2} cents off {f:.3} Hz", m[0]);
            assert_eq!(m[1] as i32, note, "{label}: wrong note at {} Hz", m[0]);
            assert!(m[2].abs() <= 1.0, "{label}: cents meter {}", m[2]);
            assert!(m[3] > 0.9, "{label}: confidence {}", m[3]);
        }
    }

    // E1, A1, B0 (5-string low B), E2, A4, C6, C7.
    const NOTES: [i32; 7] = [28, 33, 23, 40, 69, 84, 96];

    #[test]
    fn sines_are_within_a_cent() {
        for sr in [44100.0, SR, 96000.0] {
            for &note in &NOTES {
                let f = midi_hz(note as f64);
                let mut t = Tuner::new(sr);
                let m = run(&mut t, sr, 1.5, sine(f, sr, 0.5));
                assert_steady(&m, sr, f, note, &format!("sine {f:.2} Hz at {sr}"));
            }
        }
    }

    /// An input with a DC offset and nothing else once kept the detector busy on the DC
    /// blocker's decaying, denormal output, slowly enough to take the audio thread down.
    #[test]
    fn a_dc_offset_is_silence() {
        let mut t = Tuner::new(SR);
        t.set_param(THRESHOLD, -90.0);
        let m = run(&mut t, SR, 2.0, |_| 0.3);
        assert!(m.iter().all(|m| m[1] == -1.0), "no note from DC");
        // The level smooths over 0.1 s, so the blocker's settling takes a while to leave it.
        assert!(m.last().unwrap()[4] < -90.0, "no level from DC: {:?}", m.last());
    }

    /// A plug-in blowing up before the tuner once overflowed the NSDF sums into NaN, and the
    /// peak search stopped advancing: the audio thread hung until the kernel killed it.
    #[test]
    fn a_runaway_input_neither_hangs_nor_sticks() {
        let f = midi_hz(45.0);
        let mut t = Tuner::new(SR);
        let mut tone = sine(f, SR, 0.5);
        let half = (0.5 * SR) as usize;
        run(&mut t, SR, 1.0, |n| if n < half { tone(n) } else { 1e30 * tone(n) });
        t.reset();
        let m = run(&mut t, SR, 1.5, sine(f, SR, 0.5));
        assert_steady(&m, SR, f, 45, "after a runaway input");
    }

    #[test]
    fn sawtooths_are_within_a_cent_and_octave_correct() {
        for &note in &NOTES {
            let f = midi_hz(note as f64);
            let mut t = Tuner::new(SR);
            let m = run(&mut t, SR, 1.5, saw(f, SR, 0.5));
            assert_steady(&m, SR, f, note, &format!("saw {f:.2} Hz"));
        }
    }

    #[test]
    fn strong_second_harmonic_keeps_the_octave() {
        // A plucked bass string: the fundamental weaker than the octave above it.
        for &note in &[23, 28, 33, 40] {
            let f = midi_hz(note as f64);
            let amps = [0.4, 1.0, 0.5, 0.3, 0.2, 0.1];
            let gen = move |n: usize| {
                let ph = TAU * f * n as f64 / SR as f64;
                let s: f64 = amps.iter().enumerate().map(|(k, a)| a * ((k + 1) as f64 * ph + k as f64).sin()).sum();
                (0.25 * s) as f32
            };
            let mut t = Tuner::new(SR);
            let m = run(&mut t, SR, 1.5, gen);
            assert_steady(&m, SR, f, note, &format!("bass {f:.2} Hz"));
        }
    }

    #[test]
    fn detuned_tone_reads_its_cents() {
        let f = 440.0 * 2f64.powf(13.0 / 1200.0);
        let mut t = Tuner::new(SR);
        let m = run(&mut t, SR, 1.0, sine(f, SR, 0.3));
        let last = m.last().unwrap();
        assert_eq!(last[1] as i32, 69);
        assert!((last[2] - 13.0).abs() < 1.0, "cents {}", last[2]);
    }

    #[test]
    fn silence_and_noise_read_nothing() {
        let mut t = Tuner::new(SR);
        for m in run(&mut t, SR, 1.0, |_| 0.0) {
            assert_eq!(m[0], 0.0);
            assert_eq!(m[1], -1.0);
        }
        // White noise from a fixed LCG, loud enough to pass the level gate.
        let mut seed = 0x1234_5678u32;
        let noise = move |_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        let mut t = Tuner::new(SR);
        for m in run(&mut t, SR, 2.0, noise) {
            assert_eq!(m[0], 0.0, "noise detected as {} Hz", m[0]);
        }
        // A quiet tone below the threshold is gated too.
        let mut t = Tuner::new(SR);
        let m = run(&mut t, SR, 1.0, sine(440.0, SR, 0.0005));
        assert_eq!(m.last().unwrap()[0], 0.0);
    }

    #[test]
    fn note_drops_after_the_tone_stops() {
        let mut t = Tuner::new(SR);
        let f = 110.0;
        let m = run(&mut t, SR, 2.0, |n| if n < 48000 { (0.5 * (TAU * f * n as f64 / 48000.0).sin()) as f32 } else { 0.0 });
        assert!((m[80][0] as f64 - f).abs() < 0.1);
        assert_eq!(m.last().unwrap()[0], 0.0);
    }

    #[test]
    fn reference_changes_the_cents() {
        let mut t = Tuner::new(SR);
        t.set_param(REFERENCE, 442.0);
        let m = run(&mut t, SR, 1.0, sine(440.0, SR, 0.5));
        let last = m.last().unwrap();
        let expect = cents_between(440.0, 442.0) as f32;
        assert!((last[0] - 440.0).abs() < 0.05, "frequency doesn't depend on the reference");
        assert_eq!(last[1] as i32, 69);
        assert!((last[2] - expect).abs() < 0.5, "cents {} vs {expect}", last[2]);
        // Moving the reference updates the reading straight away.
        t.set_param(REFERENCE, 430.0);
        let mut out = [0.0; MAX_METERS];
        t.meters(&mut out);
        assert!((out[2] - cents_between(440.0, 430.0) as f32).abs() < 0.5, "cents {}", out[2]);
    }

    #[test]
    fn transpose_shifts_the_note_name() {
        let mut t = Tuner::new(SR);
        t.set_param(TRANSPOSE, 2.0);
        let m = run(&mut t, SR, 1.0, sine(midi_hz(58.0), SR, 0.5));
        assert_eq!(m.last().unwrap()[1] as i32, 60, "a B-flat instrument's written C");
    }

    #[test]
    fn note_has_hysteresis() {
        let mut t = Tuner::new(SR);
        // A4 + 45 cents, then + 55 cents: still A4 (with the needle pinned), then + 65: B-flat.
        run(&mut t, SR, 0.6, sine(midi_hz(69.45), SR, 0.5));
        let m = run(&mut t, SR, 0.6, sine(midi_hz(69.55), SR, 0.5));
        assert_eq!(m.last().unwrap()[1] as i32, 69);
        let m = run(&mut t, SR, 0.6, sine(midi_hz(69.65), SR, 0.5));
        let last = m.last().unwrap();
        assert_eq!(last[1] as i32, 70);
        assert!((last[2] + 35.0).abs() < 1.0, "cents {}", last[2]);
    }

    #[test]
    fn passes_audio_bit_exact_and_mutes() {
        let mut t = Tuner::new(SR);
        let mut seed = 7u32;
        let input: Vec<f32> = (0..4096)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f32::from_bits((seed >> 9) | 0x3f00_0000) - 0.75
            })
            .collect();
        let (mut l, mut r) = (input.clone(), input.iter().map(|x| -x).collect::<Vec<_>>());
        for (bl, br) in l.chunks_mut(1024).zip(r.chunks_mut(1024)) {
            t.process(bl, br, None, &ctx(SR));
        }
        assert!(l.iter().zip(&input).all(|(a, b)| a.to_bits() == b.to_bits()));
        assert!(r.iter().zip(&input).all(|(a, b)| a.to_bits() == (-b).to_bits()));

        t.set_param(MUTE, 1.0);
        let (mut l, mut r) = (input[..1024].to_vec(), input[..1024].to_vec());
        t.process(&mut l, &mut r, None, &ctx(SR));
        assert!(l.iter().chain(&r).all(|&x| x == 0.0));
        let mut m = [0.0; MAX_METERS];
        t.meters(&mut m);
        assert!(m[4] > -40.0, "the level meter still reads the input while muted");
    }
}
