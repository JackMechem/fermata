//! Delay: Logic's Stereo Delay and Tape Delay in one effect.
//!
//! Four modes share one pair of delay lines:
//! - **Stereo**: independent left and right times and feedback, with cross-feed.
//! - **Ping-Pong**: the input, summed to mono, bounces left → right → left.
//! - **Tape**: wow and flutter, saturation and a darkening filter in the loop. Time changes glide
//!   like a varispeed transport, and feedback can pass 100 % and self-oscillate, held in check by
//!   the saturation.
//! - **Digital**: one linked time, no colouring, feedback clamped below 100 %.
//!
//! Time changes (including tempo changes while synced) crossfade between two read heads in the
//! clean modes and glide in Tape mode, so neither clicks.
//!
//! This module also holds the small building blocks the reverb shares: [`Line`], [`Biquad`],
//! [`flush`] and the tempo-sync note values.

use super::{Effect, EffectInfo, FactoryPreset, FxContext, Key, ParamKind, ParamSpec, Unit};

// ---- shared building blocks -----------------------------------------------------------------

/// The highest sample rate buffers are sized for.
pub(crate) const MAX_RATE: f32 = 192_000.0;

/// Flushes values below −300 dB to exactly zero, so decaying feedback loops never reach
/// subnormal floats (which are very slow on x86) and can't get stuck in a rounding limit cycle
/// just above zero.
#[inline]
pub(crate) fn flush(x: f32) -> f32 {
    if x.abs() < 1e-15 { 0.0 } else { x }
}

/// A ring-buffer delay line.
///
/// The buffer is allocated once for the largest size at [`MAX_RATE`]; `set_active` picks how much
/// of it is in use for the actual sample rate, so memory pages the line never wraps into are
/// never touched.
pub(crate) struct Line {
    buf: Vec<f32>,
    len: usize,
    w: usize,
}

impl Line {
    pub(crate) fn new(capacity: usize) -> Line {
        let capacity = capacity.max(8);
        Line { buf: vec![0.0; capacity], len: capacity, w: 0 }
    }

    /// Uses the first `len` samples of the buffer, and clears them.
    pub(crate) fn set_active(&mut self, len: usize) {
        self.len = len.clamp(8, self.buf.len());
        self.clear();
    }

    pub(crate) fn clear(&mut self) {
        self.buf[..self.len].fill(0.0);
        self.w = 0;
    }

    /// The longest delay `read_frac` can serve.
    pub(crate) fn max_delay(&self) -> f32 {
        (self.len - 3) as f32
    }

    #[inline]
    pub(crate) fn write(&mut self, x: f32) {
        self.buf[self.w] = x;
        self.w += 1;
        if self.w == self.len {
            self.w = 0;
        }
    }

    /// The sample written `d` writes ago: 1 is the most recent. Read before writing to get a
    /// delay of exactly `d` samples.
    #[inline]
    pub(crate) fn read(&self, d: usize) -> f32 {
        let d = d.min(self.len);
        let i = if d > self.w { self.w + self.len - d } else { self.w - d };
        self.buf[i]
    }

    /// A fractional read with linear interpolation (for taps that only drift slowly).
    #[inline]
    pub(crate) fn read_lin(&self, d: f32) -> f32 {
        let d = d.clamp(1.0, self.max_delay());
        let di = d as usize;
        let f = d - di as f32;
        let a = self.read(di);
        a + (self.read(di + 1) - a) * f
    }

    /// A fractional read with 4-point (Catmull-Rom) Hermite interpolation, for modulated and
    /// gliding delays. Its gain never exceeds 1, so it is safe inside feedback loops, and an
    /// integer delay reads back exactly.
    #[inline]
    pub(crate) fn read_frac(&self, d: f32) -> f32 {
        let d = d.clamp(2.0, self.max_delay());
        let di = d as usize;
        let f = d - di as f32;
        let xm1 = self.read(di - 1);
        let x0 = self.read(di);
        if f == 0.0 {
            return x0;
        }
        let x1 = self.read(di + 1);
        let x2 = self.read(di + 2);
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + x0
    }

    #[cfg(test)]
    pub(crate) fn has_subnormal(&self) -> bool {
        self.buf.iter().any(|x| x.is_subnormal())
    }
}

/// An RBJ-cookbook biquad (transposed direct form II).
#[derive(Clone, Copy)]
pub(crate) struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    s1: f32,
    s2: f32,
}

impl Biquad {
    pub(crate) fn new() -> Biquad {
        Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, s1: 0.0, s2: 0.0 }
    }

    fn set(&mut self, b: [f64; 3], a: [f64; 3]) {
        self.b0 = (b[0] / a[0]) as f32;
        self.b1 = (b[1] / a[0]) as f32;
        self.b2 = (b[2] / a[0]) as f32;
        self.a1 = (a[1] / a[0]) as f32;
        self.a2 = (a[2] / a[0]) as f32;
    }

    /// A 12 dB/oct Butterworth low-pass. The state is kept so sweeps don't click.
    pub(crate) fn set_lowpass(&mut self, freq: f32, sample_rate: f32) {
        let w = std::f64::consts::TAU * (freq.min(sample_rate * 0.45) as f64) / sample_rate as f64;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        self.set([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]);
    }

    /// A 12 dB/oct Butterworth high-pass.
    pub(crate) fn set_highpass(&mut self, freq: f32, sample_rate: f32) {
        let w = std::f64::consts::TAU * (freq.min(sample_rate * 0.45) as f64) / sample_rate as f64;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        self.set([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]);
    }

    #[inline]
    pub(crate) fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.s1;
        self.s1 = flush(self.b1 * x - self.a1 * y + self.s2);
        self.s2 = flush(self.b2 * x - self.a2 * y);
        y
    }

    pub(crate) fn clear(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    #[cfg(test)]
    pub(crate) fn has_subnormal(&self) -> bool {
        self.s1.is_subnormal() || self.s2.is_subnormal()
    }
}

/// A sine oscillator by rotation: two multiplies per sample, no `sin()` calls.
#[derive(Clone, Copy)]
pub(crate) struct Osc {
    s: f32,
    c: f32,
    ks: f32,
    kc: f32,
}

impl Osc {
    pub(crate) fn new(phase: f32) -> Osc {
        Osc { s: phase.sin(), c: phase.cos(), ks: 0.0, kc: 1.0 }
    }

    /// Restarts at `phase`, keeping the frequency, so a reset effect renders the same every time.
    pub(crate) fn set_phase(&mut self, phase: f32) {
        self.s = phase.sin();
        self.c = phase.cos();
    }

    pub(crate) fn set_freq(&mut self, hz: f32, sample_rate: f32) {
        let w = std::f32::consts::TAU * hz / sample_rate;
        self.ks = w.sin();
        self.kc = w.cos();
    }

    /// Advances one sample and returns the sine.
    #[inline]
    pub(crate) fn tick(&mut self) -> f32 {
        let s = self.s * self.kc + self.c * self.ks;
        self.c = self.c * self.kc - self.s * self.ks;
        self.s = s;
        s
    }

    /// The cosine of the current phase (the quadrature output).
    #[inline]
    pub(crate) fn cos(&self) -> f32 {
        self.c
    }

    /// Pulls the amplitude back to 1; rounding makes a rotation drift slowly. Call once a block.
    pub(crate) fn normalize(&mut self) {
        let g = 1.5 - 0.5 * (self.s * self.s + self.c * self.c);
        self.s *= g;
        self.c *= g;
    }
}

/// Tempo-sync note values, as shown in the choice pop-ups ("." dotted, "T" triplet).
pub(crate) const NOTE_LABELS: &[&str] = &[
    "1/1", "1/1.", "1/1T", "1/2", "1/2.", "1/2T", "1/4", "1/4.", "1/4T", "1/8", "1/8.", "1/8T", "1/16", "1/16.", "1/16T",
    "1/32", "1/32.", "1/32T",
];

/// The length of a note value in quarter-note beats.
pub(crate) fn note_beats(index: usize) -> f64 {
    let base = [4.0, 2.0, 1.0, 0.5, 0.25, 0.125][(index / 3).min(5)];
    match index % 3 {
        0 => base,
        1 => base * 1.5,
        _ => base * 2.0 / 3.0,
    }
}

/// The length of a note value in ms at a tempo.
pub(crate) fn note_ms(index: usize, tempo: f64) -> f32 {
    (note_beats(index) * 60_000.0 / tempo.max(1.0)) as f32
}

// ---- parameters -----------------------------------------------------------------------------

pub const MODE: u32 = 0;
pub const SYNC: u32 = 1;
pub const TIME_L: u32 = 2;
pub const TIME_R: u32 = 3;
pub const NOTE_L: u32 = 4;
pub const NOTE_R: u32 = 5;
/// The right channel's time and feedback follow the left's.
pub const LINK: u32 = 6;
pub const FEEDBACK_L: u32 = 7;
pub const FEEDBACK_R: u32 = 8;
pub const CROSSFEED: u32 = 9;
pub const LOW_CUT: u32 = 10;
pub const HIGH_CUT: u32 = 11;
pub const MOD_RATE: u32 = 12;
pub const MOD_DEPTH: u32 = 13;
pub const WOW: u32 = 14;
pub const FLUTTER: u32 = 15;
pub const SATURATION: u32 = 16;
pub const SMOOTH: u32 = 17;
pub const WIDTH: u32 = 18;
pub const DRY: u32 = 19;
pub const WET: u32 = 20;
pub const FREEZE: u32 = 21;

pub const MODES: &[&str] = &["Stereo", "Ping-Pong", "Tape", "Digital"];
/// The default mode, which the code handles as "not one of the others".
#[cfg_attr(not(test), allow(dead_code))]
const STEREO: usize = 0;
const PING_PONG: usize = 1;
const TAPE: usize = 2;
const DIGITAL: usize = 3;

/// The longest delay time, in ms (synced times are clamped to it).
pub const MAX_TIME_MS: f32 = 4000.0;
/// The chorus modulation's largest excursion.
const MOD_MS: f32 = 3.0;
/// Extra room in the lines for modulation, wow and flutter.
const MARGIN_MS: f32 = 30.0;
/// The crossfade between read heads when the time changes in the clean modes.
const XFADE_MS: f32 = 25.0;
/// Low and high cut at these extremes are off.
const LOW_CUT_OFF: f32 = 20.0;
const HIGH_CUT_OFF: f32 = 20_000.0;
/// Tape mode's loop gain at 100 % feedback: a little over unity, so it can run away into
/// self-oscillation, which the saturation then limits.
const TAPE_OVERDRIVE: f32 = 1.1;
/// Digital mode's feedback ceiling.
const DIGITAL_MAX_FEEDBACK: f32 = 0.98;
const MAX_TAIL_SECS: f32 = 30.0;

const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, kind: ParamKind, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind, log }
}

const C: ParamKind = ParamKind::Continuous;
const T: ParamKind = ParamKind::Toggle;
const NOTE: ParamKind = ParamKind::Choice(NOTE_LABELS);

static PARAMS: [ParamSpec; 22] = [
    spec(MODE, "Mode", 0.0, 3.0, 0.0, Unit::None, ParamKind::Choice(MODES), false),
    spec(SYNC, "Sync", 0.0, 1.0, 1.0, Unit::None, T, false),
    spec(TIME_L, "Time L", 1.0, MAX_TIME_MS, 500.0, Unit::Ms, C, true),
    spec(TIME_R, "Time R", 1.0, MAX_TIME_MS, 375.0, Unit::Ms, C, true),
    spec(NOTE_L, "Note L", 0.0, 17.0, 6.0, Unit::Note, NOTE, false),
    spec(NOTE_R, "Note R", 0.0, 17.0, 10.0, Unit::Note, NOTE, false),
    spec(LINK, "Link", 0.0, 1.0, 0.0, Unit::None, T, false),
    spec(FEEDBACK_L, "Feedback L", 0.0, 1.0, 0.35, Unit::Percent, C, false),
    spec(FEEDBACK_R, "Feedback R", 0.0, 1.0, 0.35, Unit::Percent, C, false),
    spec(CROSSFEED, "Cross-feed", 0.0, 1.0, 0.0, Unit::Percent, C, false),
    spec(LOW_CUT, "Low Cut", LOW_CUT_OFF, 2000.0, LOW_CUT_OFF, Unit::Hz, C, true),
    spec(HIGH_CUT, "High Cut", 500.0, HIGH_CUT_OFF, HIGH_CUT_OFF, Unit::Hz, C, true),
    spec(MOD_RATE, "Mod Rate", 0.05, 10.0, 0.8, Unit::Hz, C, true),
    spec(MOD_DEPTH, "Mod Depth", 0.0, 1.0, 0.0, Unit::Percent, C, false),
    spec(WOW, "Wow", 0.0, 1.0, 0.3, Unit::Percent, C, false),
    spec(FLUTTER, "Flutter", 0.0, 1.0, 0.2, Unit::Percent, C, false),
    spec(SATURATION, "Saturation", 0.0, 1.0, 0.3, Unit::Percent, C, false),
    spec(SMOOTH, "Smooth", 0.0, 1.0, 0.5, Unit::Percent, C, false),
    spec(WIDTH, "Width", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(DRY, "Dry", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(WET, "Wet", 0.0, 1.0, 0.35, Unit::Percent, C, false),
    spec(FREEZE, "Freeze", 0.0, 1.0, 0.0, Unit::None, T, false),
];

/// Factory presets. Modes: 0 Stereo, 1 Ping-Pong, 2 Tape, 3 Digital. Note indices follow
/// [`NOTE_LABELS`] (6 = 1/4, 9 = 1/8, 10 = 1/8 dotted, 11 = 1/8 triplet, 14 = 1/16 triplet).
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Quarter Note",
        values: &[(MODE, 3.0), (SYNC, 1.0), (NOTE_L, 6.0), (FEEDBACK_L, 0.35), (WET, 0.3)],
    },
    FactoryPreset {
        name: "Dotted Eighth (U2)",
        values: &[(MODE, 3.0), (SYNC, 1.0), (NOTE_L, 10.0), (FEEDBACK_L, 0.45), (HIGH_CUT, 7000.0), (LOW_CUT, 150.0), (WET, 0.4)],
    },
    FactoryPreset {
        name: "Slapback",
        values: &[(MODE, 2.0), (SYNC, 0.0), (TIME_L, 95.0), (LINK, 1.0), (FEEDBACK_L, 0.08), (WOW, 0.15), (FLUTTER, 0.1), (SATURATION, 0.4), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Ping-Pong Eighths",
        values: &[(MODE, 1.0), (SYNC, 1.0), (NOTE_L, 9.0), (NOTE_R, 9.0), (FEEDBACK_L, 0.5), (HIGH_CUT, 9000.0), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Tape Echo",
        values: &[(MODE, 2.0), (SYNC, 1.0), (NOTE_L, 10.0), (LINK, 1.0), (FEEDBACK_L, 0.45), (WOW, 0.35), (FLUTTER, 0.25), (SATURATION, 0.45), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Dub Delay",
        values: &[
            (MODE, 2.0),
            (SYNC, 1.0),
            (NOTE_L, 10.0),
            (NOTE_R, 6.0),
            (FEEDBACK_L, 0.85),
            (FEEDBACK_R, 0.85),
            (CROSSFEED, 0.35),
            (LOW_CUT, 250.0),
            (HIGH_CUT, 3500.0),
            (SATURATION, 0.6),
            (WOW, 0.4),
            (WET, 0.45),
        ],
    },
    FactoryPreset {
        name: "Wide Doubler",
        values: &[(MODE, 0.0), (SYNC, 0.0), (TIME_L, 13.0), (TIME_R, 21.0), (FEEDBACK_L, 0.0), (FEEDBACK_R, 0.0), (MOD_RATE, 0.6), (MOD_DEPTH, 0.35), (WET, 0.5)],
    },
    FactoryPreset {
        name: "Vocal Throw",
        values: &[(MODE, 0.0), (SYNC, 1.0), (NOTE_L, 6.0), (NOTE_R, 10.0), (FEEDBACK_L, 0.4), (FEEDBACK_R, 0.4), (LOW_CUT, 300.0), (HIGH_CUT, 6000.0), (DRY, 1.0), (WET, 0.5)],
    },
    FactoryPreset {
        name: "Lo-Fi Tape",
        values: &[(MODE, 2.0), (SYNC, 1.0), (NOTE_L, 9.0), (LINK, 1.0), (FEEDBACK_L, 0.55), (WOW, 0.8), (FLUTTER, 0.7), (SATURATION, 0.85), (HIGH_CUT, 4000.0), (LOW_CUT, 200.0), (SMOOTH, 0.2), (WET, 0.4)],
    },
    FactoryPreset {
        name: "Triplet Bounce",
        values: &[(MODE, 1.0), (SYNC, 1.0), (NOTE_L, 11.0), (NOTE_R, 11.0), (FEEDBACK_L, 0.45), (WIDTH, 0.9), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Sixteenth Shimmer",
        values: &[(MODE, 0.0), (SYNC, 1.0), (NOTE_L, 12.0), (NOTE_R, 14.0), (FEEDBACK_L, 0.55), (FEEDBACK_R, 0.5), (CROSSFEED, 0.5), (MOD_DEPTH, 0.25), (LOW_CUT, 400.0), (WET, 0.3)],
    },
];

pub fn info() -> EffectInfo {
    EffectInfo {
        id: "builtin:delay",
        name: "Delay",
        category: "Delay",
        params: &PARAMS,
        meters: &[],
        sidechain: false,
        new: |sample_rate| Box::new(Delay::new(sample_rate)),
    }
}

// ---- the effect -----------------------------------------------------------------------------

/// One channel's read position. In the clean modes a time change crossfades from `cur` to
/// `next`; in Tape mode `cur` glides.
#[derive(Clone, Copy)]
struct Head {
    cur: f32,
    next: f32,
    /// Crossfade progress, 0 when not fading.
    x: f32,
}

impl Head {
    #[inline]
    fn tick_fade(&mut self, target: f32, step: f32) {
        if self.x == 0.0 {
            if (target - self.cur).abs() >= 0.5 {
                self.next = target;
                self.x = step;
            }
        } else {
            self.x += step;
            if self.x >= 1.0 {
                self.cur = self.next;
                self.x = 0.0;
            }
        }
    }

    #[inline]
    fn tick_glide(&mut self, target: f32, k: f32, fade_step: f32) {
        if self.x > 0.0 {
            // A crossfade left over from a clean mode finishes first.
            self.tick_fade(self.next, fade_step);
            return;
        }
        // The slew limit keeps the varispeed between 0.1× and 1.9×.
        self.cur += ((target - self.cur) * k).clamp(-0.9, 0.9);
    }

    #[inline]
    fn read(&self, line: &Line, m: f32) -> f32 {
        let a = line.read_frac(self.cur + m);
        if self.x > 0.0 {
            let b = line.read_frac(self.next + m);
            a + (b - a) * self.x
        } else {
            a
        }
    }
}

/// `tanh` by a Padé approximant, exact at ±3 where it reaches ±1.
#[inline]
fn tanh_approx(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// Tape saturation with unity small-signal gain, so quiet repeats keep the set feedback.
#[inline]
fn saturate(x: f32, drive: f32) -> f32 {
    tanh_approx(drive * x) / drive
}

/// A transparent safety limiter for the clean modes: it only acts above 1.5 (+3.5 dBFS).
#[inline]
fn guard(x: f32) -> f32 {
    let a = x.abs();
    if a <= 1.5 {
        x
    } else {
        x.signum() * (1.5 + tanh_approx(a - 1.5))
    }
}

/// A one-pole smoother step.
#[inline]
fn approach(v: &mut f32, target: f32, k: f32) {
    *v += (target - *v) * k;
}

pub struct Delay {
    values: [f32; PARAMS.len()],
    sr: f32,
    lines: [Line; 2],
    heads: [Head; 2],
    low_cut: [Biquad; 2],
    high_cut: [Biquad; 2],
    filters_dirty: bool,
    /// Tape mode's darkening low-pass and its DC-blocking high-pass, one pole each.
    dark: [f32; 2],
    dc: [f32; 2],
    chorus: Osc,
    wow: Osc,
    flutter: [Osc; 2],
    rng: u32,
    wow_drift: f32,
    flutter_jitter: f32,
    // Smoothed per-sample values.
    dry: f32,
    wet: f32,
    width: f32,
    gate: f32,
    fb: [f32; 2],
    cross: f32,
    /// The tempo of the last block, for `tail_secs`.
    tempo: f64,
    /// False until the first block after a reset, which snaps every smoother to its target.
    primed: bool,
}

impl Delay {
    pub fn new(sample_rate: f32) -> Delay {
        let alloc = sample_rate.max(MAX_RATE);
        let cap = ((MAX_TIME_MS + MARGIN_MS) * 0.001 * alloc) as usize + 8;
        let mut d = Delay {
            values: std::array::from_fn(|i| PARAMS[i].default),
            sr: sample_rate,
            lines: [Line::new(cap), Line::new(cap)],
            heads: [Head { cur: 2.0, next: 2.0, x: 0.0 }; 2],
            low_cut: [Biquad::new(); 2],
            high_cut: [Biquad::new(); 2],
            filters_dirty: true,
            dark: [0.0; 2],
            dc: [0.0; 2],
            chorus: Osc::new(0.0),
            wow: Osc::new(0.0),
            flutter: [Osc::new(0.0), Osc::new(1.3)],
            rng: 0x2545_f491,
            wow_drift: 0.0,
            flutter_jitter: 0.0,
            dry: 1.0,
            wet: 0.0,
            width: 1.0,
            gate: 1.0,
            fb: [0.0; 2],
            cross: 0.0,
            tempo: 120.0,
            primed: false,
        };
        d.set_rate(sample_rate);
        d
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    fn on(&self, id: u32) -> bool {
        self.v(id) >= 0.5
    }

    fn mode(&self) -> usize {
        (self.v(MODE).round() as usize).min(DIGITAL)
    }

    fn set_rate(&mut self, sample_rate: f32) {
        self.sr = sample_rate;
        let len = ((MAX_TIME_MS + MARGIN_MS) * 0.001 * sample_rate) as usize + 8;
        for line in &mut self.lines {
            line.set_active(len);
        }
        self.wow.set_freq(0.55, sample_rate);
        self.flutter[0].set_freq(6.3, sample_rate);
        self.flutter[1].set_freq(10.9, sample_rate);
        self.filters_dirty = true;
        self.reset();
    }

    /// The left and right delay times in ms for the current mode, sync and link settings.
    fn times_ms(&self, tempo: f64) -> [f32; 2] {
        let time = |ms: u32, note: u32| {
            let t = if self.on(SYNC) { note_ms(self.v(note).round() as usize, tempo) } else { self.v(ms) };
            t.clamp(1.0, MAX_TIME_MS)
        };
        let l = time(TIME_L, NOTE_L);
        let linked = self.mode() == DIGITAL || self.on(LINK);
        [l, if linked { l } else { time(TIME_R, NOTE_R) }]
    }

    /// The loop gains per channel (each repeat is scaled by this).
    fn feedback(&self) -> [f32; 2] {
        if self.on(FREEZE) {
            return [1.0; 2];
        }
        let mode = self.mode();
        let l = self.v(FEEDBACK_L);
        let r = if mode == DIGITAL || mode == PING_PONG || self.on(LINK) { l } else { self.v(FEEDBACK_R) };
        match mode {
            DIGITAL => [l.min(DIGITAL_MAX_FEEDBACK), r.min(DIGITAL_MAX_FEEDBACK)],
            TAPE => [l * TAPE_OVERDRIVE, r * TAPE_OVERDRIVE],
            _ => [l, r],
        }
    }

    fn noise(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    #[cfg(test)]
    fn has_subnormal(&self) -> bool {
        self.lines.iter().any(|l| l.has_subnormal())
            || self.low_cut.iter().chain(&self.high_cut).any(|b| b.has_subnormal())
            || self.dark.iter().chain(&self.dc).any(|x| x.is_subnormal())
    }
}

impl Effect for Delay {
    fn set_param(&mut self, id: u32, value: f32) {
        if let Some(v) = self.values.get_mut(id as usize) {
            *v = value;
            if id == LOW_CUT || id == HIGH_CUT {
                self.filters_dirty = true;
            }
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, ctx: &FxContext) {
        if ctx.sample_rate > 0.0 && (ctx.sample_rate - self.sr).abs() > 0.5 && ctx.sample_rate <= MAX_RATE {
            self.set_rate(ctx.sample_rate);
        }
        self.tempo = ctx.tempo;
        let sr = self.sr;
        let mode = self.mode();
        let freeze = self.on(FREEZE);
        let clean = mode != TAPE;

        if self.filters_dirty {
            self.filters_dirty = false;
            let (lc, hc) = (self.v(LOW_CUT), self.v(HIGH_CUT));
            for b in &mut self.low_cut {
                b.set_highpass(lc, sr);
            }
            for b in &mut self.high_cut {
                b.set_lowpass(hc, sr);
            }
        }
        let low_cut = !freeze && self.v(LOW_CUT) > LOW_CUT_OFF + 0.5;
        let high_cut = !freeze && self.v(HIGH_CUT) < HIGH_CUT_OFF - 1.0;
        let tape = mode == TAPE && !freeze;

        // Targets for this block.
        let times = self.times_ms(ctx.tempo);
        let max_d = self.lines[0].max_delay() - MARGIN_MS * 0.001 * sr;
        let target = times.map(|ms| {
            let d = (ms * 0.001 * sr).clamp(2.0, max_d);
            // Clean modes read whole samples, so unmodulated repeats lose no treble.
            if clean { d.round() } else { d }
        });
        let fb_t = self.feedback();
        let cross_t = if mode == PING_PONG { 0.0 } else { self.v(CROSSFEED) };
        let dry_t = self.v(DRY);
        let wet_t = self.v(WET);
        let width_t = self.v(WIDTH);
        let gate_t = if freeze { 0.0 } else { 1.0 };

        if freeze {
            // Straight to unity so the loop holds exactly what it had.
            self.fb = fb_t;
        }
        if !self.primed {
            self.primed = true;
            for (h, t) in self.heads.iter_mut().zip(target) {
                *h = Head { cur: t, next: t, x: 0.0 };
            }
            self.fb = fb_t;
            self.cross = cross_t;
            self.dry = dry_t;
            self.wet = wet_t;
            self.width = width_t;
            self.gate = gate_t;
        }

        let ks = super::util::coeff(20.0, sr);
        let smooth = self.v(SMOOTH);
        let glide_k = super::util::coeff(40.0 + 1460.0 * smooth, sr);
        let fade_step = 1.0 / (XFADE_MS * 0.001 * sr);
        let ms = 0.001 * sr;
        self.chorus.set_freq(self.v(MOD_RATE), sr);
        let chorus = self.v(MOD_DEPTH) * MOD_MS * ms;
        // Wow scales with the delay time (a slower transport stretches a longer stretch of
        // tape); flutter is a small fixed jitter.
        let wow = if tape { self.v(WOW) } else { 0.0 };
        let wow_amt = times.map(|t| wow * (0.5 + 0.003 * t) * ms);
        let flutter = if tape { self.v(FLUTTER) * 0.12 * ms } else { 0.0 };
        // Smooth tames the random parts of the modulation.
        let drift_k = super::util::coeff(400.0 + 2000.0 * smooth, sr);
        let jitter_k = super::util::coeff(4.0 + 40.0 * smooth, sr);
        let drive = 0.4 + 2.6 * self.v(SATURATION);
        let dark_k = super::util::coeff(1000.0 / (std::f32::consts::TAU * 5500.0), sr);
        let dc_k = super::util::coeff(1000.0 / (std::f32::consts::TAU * 50.0), sr);

        for n in 0..l.len() {
            approach(&mut self.dry, dry_t, ks);
            approach(&mut self.wet, wet_t, ks);
            approach(&mut self.width, width_t, ks);
            approach(&mut self.gate, gate_t, ks);
            approach(&mut self.fb[0], fb_t[0], ks);
            approach(&mut self.fb[1], fb_t[1], ks);
            approach(&mut self.cross, cross_t, ks);

            // Modulation: a quadrature chorus LFO, plus wow and flutter shared by both channels
            // (one tape transport).
            let ch = self.chorus.tick() * chorus;
            let cc = self.chorus.cos() * chorus;
            let mut tape_mod = [0.0; 2];
            if tape {
                let nz = self.noise();
                approach(&mut self.wow_drift, nz, drift_k);
                let nz = self.noise();
                approach(&mut self.flutter_jitter, nz, jitter_k);
                let w = 0.7 * self.wow.tick() + 6.0 * self.wow_drift;
                let f = 0.6 * self.flutter[0].tick() + 0.25 * self.flutter[1].tick() + 1.5 * self.flutter_jitter;
                // Offset so the modulation only ever lengthens the delay: the set time stays the
                // earliest an echo can arrive.
                let (w, f) = ((1.0 + w).max(0.0), (1.0 + f).max(0.0));
                tape_mod = [w * wow_amt[0] + f * flutter, w * wow_amt[1] + f * flutter];
            }

            for c in 0..2 {
                if clean {
                    self.heads[c].tick_fade(target[c], fade_step);
                } else {
                    self.heads[c].tick_glide(target[c], glide_k, fade_step);
                }
            }
            let yl = self.heads[0].read(&self.lines[0], chorus + ch + tape_mod[0]);
            let yr = self.heads[1].read(&self.lines[1], chorus + cc + tape_mod[1]);

            let (il, ir) = (l[n], r[n]);
            let (mut xl, mut xr) = if mode == PING_PONG {
                (0.5 * (il + ir) * self.gate + self.fb[0] * yr, self.fb[1] * yl)
            } else {
                let c = self.cross;
                (
                    il * self.gate + self.fb[0] * ((1.0 - c) * yl + c * yr),
                    ir * self.gate + self.fb[1] * ((1.0 - c) * yr + c * yl),
                )
            };
            if low_cut {
                xl = self.low_cut[0].process(xl);
                xr = self.low_cut[1].process(xr);
            }
            if high_cut {
                xl = self.high_cut[0].process(xl);
                xr = self.high_cut[1].process(xr);
            }
            let mut x = [xl, xr];
            for c in 0..2 {
                x[c] = if tape {
                    let s = saturate(x[c], drive);
                    self.dark[c] = flush(self.dark[c] + (s - self.dark[c]) * dark_k);
                    self.dc[c] = flush(self.dc[c] + (self.dark[c] - self.dc[c]) * dc_k);
                    self.dark[c] - self.dc[c]
                } else {
                    guard(x[c])
                };
                self.lines[c].write(flush(x[c]));
            }

            let mid = 0.5 * (yl + yr);
            let side = 0.5 * (yl - yr) * self.width;
            l[n] = il * self.dry + (mid + side) * self.wet;
            r[n] = ir * self.dry + (mid - side) * self.wet;
        }
        self.chorus.normalize();
        self.wow.normalize();
        self.flutter[0].normalize();
        self.flutter[1].normalize();
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        for b in self.low_cut.iter_mut().chain(&mut self.high_cut) {
            b.clear();
        }
        self.dark = [0.0; 2];
        self.dc = [0.0; 2];
        self.wow_drift = 0.0;
        self.flutter_jitter = 0.0;
        self.chorus.set_phase(0.0);
        self.wow.set_phase(0.0);
        self.flutter[0].set_phase(0.0);
        self.flutter[1].set_phase(1.3);
        self.rng = 0x2545_f491;
        self.primed = false;
    }

    fn tail_secs(&self) -> f32 {
        if self.on(FREEZE) {
            return MAX_TAIL_SECS;
        }
        let times = self.times_ms(self.tempo);
        // A repeat takes the longer time; ping-pong hops alternate between the two.
        let hop = if self.mode() == PING_PONG { 0.5 * (times[0] + times[1]) } else { times[0].max(times[1]) } * 0.001;
        let g = self.feedback().iter().fold(0.0f32, |a, &b| a.max(b));
        if g < 1e-4 {
            return hop;
        }
        if g >= 0.999 {
            return MAX_TAIL_SECS;
        }
        // Repeats until −60 dB.
        let repeats = (1e-3f32).ln() / g.ln();
        ((repeats + 1.0) * hop).min(MAX_TAIL_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn ctx(tempo: f64) -> FxContext {
        FxContext { sample_rate: SR, tempo, playing: true, position: 0.0 }
    }

    /// A delay with clean, wet-only settings and sync off.
    fn clean(mode: usize) -> Delay {
        let mut d = Delay::new(SR);
        d.set_param(MODE, mode as f32);
        d.set_param(SYNC, 0.0);
        d.set_param(FEEDBACK_L, 0.0);
        d.set_param(FEEDBACK_R, 0.0);
        d.set_param(DRY, 0.0);
        d.set_param(WET, 1.0);
        d.set_param(MOD_DEPTH, 0.0);
        d
    }

    fn run(d: &mut Delay, l: &mut [f32], r: &mut [f32], tempo: f64) {
        for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            d.process(bl, br, None, &ctx(tempo));
        }
    }

    fn impulse(len: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; len];
        let mut r = vec![0.0; len];
        l[0] = 1.0;
        r[0] = 1.0;
        (l, r)
    }

    fn peak_index(x: &[f32]) -> usize {
        (0..x.len()).max_by(|&a, &b| x[a].abs().total_cmp(&x[b].abs())).unwrap()
    }

    #[test]
    fn note_values() {
        assert_eq!(note_ms(6, 120.0), 500.0); // 1/4
        assert_eq!(note_ms(10, 120.0), 375.0); // 1/8 dotted
        assert!((note_ms(11, 120.0) - 166.666_67).abs() < 1e-3); // 1/8 triplet
        assert_eq!(note_ms(0, 120.0), 2000.0); // 1/1
        assert_eq!(NOTE_LABELS.len(), 18);
    }

    #[test]
    fn first_echo_at_set_time_ms() {
        for mode in [STEREO, TAPE, DIGITAL] {
            let mut d = clean(mode);
            d.set_param(TIME_L, 250.0);
            d.set_param(TIME_R, 250.0);
            d.set_param(WOW, 0.0);
            d.set_param(FLUTTER, 0.0);
            let (mut l, mut r) = impulse(20_000);
            run(&mut d, &mut l, &mut r, 120.0);
            assert_eq!(peak_index(&l), 12_000, "mode {mode}");
            assert!(l[..12_000].iter().all(|x| x.abs() < 1e-6), "nothing before the echo, mode {mode}");
            if mode != TAPE {
                assert!((l[12_000] - 1.0).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn first_echo_synced_at_120_bpm() {
        let mut d = clean(STEREO);
        d.set_param(SYNC, 1.0);
        d.set_param(NOTE_L, 6.0); // 1/4 = 500 ms
        d.set_param(NOTE_R, 10.0); // 1/8 dotted = 375 ms
        let (mut l, mut r) = impulse(30_000);
        run(&mut d, &mut l, &mut r, 120.0);
        assert_eq!(peak_index(&l), 24_000);
        assert_eq!(peak_index(&r), 18_000);
        assert!((l[24_000] - 1.0).abs() < 1e-6 && (r[18_000] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn feedback_decays_by_the_set_amount_per_repeat() {
        let mut d = clean(DIGITAL);
        d.set_param(TIME_L, 100.0);
        d.set_param(FEEDBACK_L, 0.5);
        let (mut l, mut r) = impulse(48_000);
        run(&mut d, &mut l, &mut r, 120.0);
        for k in 1..8 {
            let expect = 0.5f32.powi(k as i32 - 1);
            assert!((l[4800 * k] - expect).abs() < 1e-5, "repeat {k}: {} vs {expect}", l[4800 * k]);
            assert!((r[4800 * k] - expect).abs() < 1e-5);
        }
        // Stereo mode keeps independent feedback per side.
        let mut d = clean(STEREO);
        d.set_param(TIME_L, 100.0);
        d.set_param(TIME_R, 100.0);
        d.set_param(FEEDBACK_L, 0.5);
        d.set_param(FEEDBACK_R, 0.25);
        let (mut l, mut r) = impulse(48_000);
        run(&mut d, &mut l, &mut r, 120.0);
        assert!((l[14_400] - 0.25).abs() < 1e-5);
        assert!((r[14_400] - 0.0625).abs() < 1e-5);
    }

    #[test]
    fn cross_feed_moves_repeats_across() {
        let mut d = clean(STEREO);
        d.set_param(TIME_L, 100.0);
        d.set_param(TIME_R, 100.0);
        d.set_param(FEEDBACK_L, 0.8);
        d.set_param(FEEDBACK_R, 0.8);
        d.set_param(CROSSFEED, 1.0);
        let mut l = vec![0.0; 20_000];
        let mut r = vec![0.0; 20_000];
        l[0] = 1.0;
        run(&mut d, &mut l, &mut r, 120.0);
        assert!((l[4800] - 1.0).abs() < 1e-6 && r[4800].abs() < 1e-6);
        assert!((r[9600] - 0.8).abs() < 1e-5 && l[9600].abs() < 1e-6);
    }

    #[test]
    fn ping_pong_alternates_channels() {
        let mut d = clean(PING_PONG);
        d.set_param(TIME_L, 100.0);
        d.set_param(TIME_R, 100.0);
        d.set_param(FEEDBACK_L, 0.6);
        let (mut l, mut r) = impulse(30_000);
        run(&mut d, &mut l, &mut r, 120.0);
        let expect = [(1.0, 0.0), (0.0, 0.6), (0.36, 0.0), (0.0, 0.216)];
        for (k, (el, er)) in expect.iter().enumerate() {
            let i = 4800 * (k + 1);
            assert!((l[i] - el).abs() < 1e-5 && (r[i] - er).abs() < 1e-5, "hop {k}: {} {}", l[i], r[i]);
        }
    }

    /// The largest sample-to-sample step of a 220 Hz sine through the delay while its time
    /// changes mid-stream.
    fn max_step(mode: usize, change: impl Fn(&mut Delay, usize) -> f64) -> f32 {
        let mut d = clean(mode);
        d.set_param(TIME_L, 300.0);
        d.set_param(TIME_R, 300.0);
        d.set_param(WOW, 0.0);
        d.set_param(FLUTTER, 0.0);
        d.set_param(SATURATION, 0.0);
        let mut worst = 0.0f32;
        let mut last = 0.0f32;
        let mut t = 0usize;
        for block in 0..300 {
            let tempo = change(&mut d, block);
            let mut l: Vec<f32> = (0..512).map(|i| 0.5 * (std::f32::consts::TAU * 220.0 * (t + i) as f32 / SR).sin()).collect();
            let mut r = l.clone();
            d.process(&mut l, &mut r, None, &ctx(tempo));
            for &y in &l {
                if t > 20_000 {
                    worst = worst.max((y - last).abs());
                }
                last = y;
                t += 1;
            }
        }
        worst
    }

    #[test]
    fn time_changes_do_not_click() {
        let slope = std::f32::consts::TAU * 220.0 * 0.5 / SR;
        let jump = |d: &mut Delay, block: usize| {
            if block == 100 {
                d.set_param(TIME_L, 120.0);
                d.set_param(TIME_R, 120.0);
            }
            if block == 200 {
                d.set_param(TIME_L, 700.0);
                d.set_param(TIME_R, 700.0);
            }
            120.0
        };
        // An abrupt jump between two phases of the sine could step by up to 1.0.
        let digital = max_step(DIGITAL, jump);
        assert!(digital < 1.3 * slope, "digital step {digital} vs slope {slope}");
        let tape = max_step(TAPE, jump);
        assert!(tape < 2.5 * slope, "tape step {tape} vs slope {slope}");
        // A tempo change while synced.
        let tempo = |d: &mut Delay, block: usize| {
            d.set_param(SYNC, 1.0);
            d.set_param(NOTE_L, 9.0);
            if block < 150 { 120.0 } else { 97.0 }
        };
        let synced = max_step(STEREO, tempo);
        assert!(synced < 1.3 * slope, "synced step {synced}");
    }

    #[test]
    fn tape_is_bounded_at_full_feedback() {
        let mut d = clean(TAPE);
        d.set_param(TIME_L, 80.0);
        d.set_param(TIME_R, 110.0);
        d.set_param(FEEDBACK_L, 1.0);
        d.set_param(FEEDBACK_R, 1.0);
        d.set_param(CROSSFEED, 0.3);
        d.set_param(SATURATION, 0.0); // the gentlest drive is the loosest bound
        d.set_param(WOW, 1.0);
        d.set_param(FLUTTER, 1.0);
        let mut peak = 0.0f32;
        let mut late = 0.0f32;
        for block in 0..(20 * 48_000 / 512) {
            let mut l: Vec<f32> = (0..512).map(|_| if block < 200 { d.noise() } else { 0.0 }).collect();
            let mut r = l.clone();
            d.process(&mut l, &mut r, None, &ctx(120.0));
            for (&a, &b) in l.iter().zip(&r) {
                assert!(a.is_finite() && b.is_finite());
                peak = peak.max(a.abs()).max(b.abs());
                if block > 1800 {
                    late = late.max(a.abs());
                }
            }
        }
        assert!(peak < 4.0, "peak {peak}");
        // Over 100 % it keeps sounding on its own: tape self-oscillation.
        assert!(late > 0.05, "late {late}");
    }

    #[test]
    fn clean_modes_are_bounded_at_full_feedback() {
        for mode in [STEREO, PING_PONG, DIGITAL] {
            let mut d = clean(mode);
            d.set_param(TIME_L, 50.0);
            d.set_param(TIME_R, 50.0);
            d.set_param(FEEDBACK_L, 1.0);
            d.set_param(FEEDBACK_R, 1.0);
            let mut peak = 0.0f32;
            for _ in 0..(10 * 48_000 / 512) {
                let mut l: Vec<f32> = (0..512).map(|_| d.noise()).collect();
                let mut r = l.clone();
                d.process(&mut l, &mut r, None, &ctx(120.0));
                peak = l.iter().chain(&r).fold(peak, |p, x| p.max(x.abs()));
            }
            assert!(peak.is_finite() && peak < 4.5, "mode {mode}: peak {peak}");
        }
    }

    #[test]
    fn freeze_holds_the_loop_and_ignores_input() {
        let mut d = clean(DIGITAL);
        d.set_param(TIME_L, 100.0);
        d.set_param(FEEDBACK_L, 0.3);
        let (mut l, mut r) = impulse(2400);
        run(&mut d, &mut l, &mut r, 120.0);
        d.set_param(FREEZE, 1.0);
        // New input is ignored and the first echo comes round at full level for good.
        let mut l = vec![0.0; 48_000 * 5];
        let mut r = l.clone();
        l[24_000] = 1.0;
        run(&mut d, &mut l, &mut r, 120.0);
        let last = &l[l.len() - 4800..];
        let p = last.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!((p - 1.0).abs() < 1e-3, "held level {p}");
        assert!(last.iter().filter(|x| x.abs() > 1e-3).count() == 1, "only the frozen echo");
        assert_eq!(d.tail_secs(), MAX_TAIL_SECS);
    }

    #[test]
    fn silence_stays_clean_without_denormals() {
        let mut d = clean(TAPE);
        d.set_param(TIME_L, 30.0);
        d.set_param(TIME_R, 40.0);
        d.set_param(FEEDBACK_L, 0.5);
        d.set_param(FEEDBACK_R, 0.5);
        d.set_param(LOW_CUT, 200.0);
        d.set_param(HIGH_CUT, 3000.0);
        let (mut l, mut r) = impulse(48_000 * 20);
        run(&mut d, &mut l, &mut r, 120.0);
        assert!(!d.has_subnormal());
        assert!(l[l.len() - 48_000..].iter().all(|&x| x == 0.0), "decays to exact silence");
    }

    #[test]
    fn presets_are_in_range() {
        assert!((10..=12).contains(&PRESETS.len()));
        for p in PRESETS {
            for &(id, v) in p.values {
                let spec = PARAMS.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("{}: no param {id}", p.name));
                assert!(v >= spec.min && v <= spec.max, "{}: {} = {v}", p.name, spec.name);
            }
            let mut d = Delay::new(SR);
            for &(id, v) in p.values {
                d.set_param(id, v);
            }
            let (mut l, mut r) = impulse(48_000);
            run(&mut d, &mut l, &mut r, 120.0);
            assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 4.0), "{}", p.name);
        }
    }

    #[test]
    fn tail_follows_feedback_and_time() {
        let mut d = clean(DIGITAL);
        d.set_param(TIME_L, 500.0);
        assert!((d.tail_secs() - 0.5).abs() < 1e-6);
        d.set_param(FEEDBACK_L, 0.5);
        // 0.5^n = −60 dB after about 10 repeats.
        let t = d.tail_secs();
        assert!(t > 5.0 && t < 6.0, "{t}");
        d.set_param(FEEDBACK_L, 1.0);
        let t = d.tail_secs();
        assert!(t > 30.0 * 0.9, "{t}");
    }
}
