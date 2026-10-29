//! Reverb: an algorithmic studio reverb in the spirit of Logic's ChromaVerb and SilverVerb.
//!
//! Two engines sit behind the Room Type choice:
//! - **Plate** is Jon Dattorro's plate (J. Dattorro, "Effect Design, Part 1: Reverberator and
//!   Other Filters", JAES 45(9), 1997). The topology, delay lengths and output taps follow the
//!   paper's figure and table; the code is written from the paper, not ported from any
//!   implementation. Its single damping filter per tank half is replaced by the two-band decay
//!   filter below, and every element scales with Size.
//! - **Room, Hall, Chamber, Cathedral, Ambience** share a 16-line feedback delay network with a
//!   Hadamard mixing matrix. Each line has a short in-loop allpass (Density), a gently modulated
//!   read (against metallic ringing), and a two-band decay filter whose gains come from the RT60
//!   so every line decays at the same rate per second (Jot's method). Input diffusers smear the
//!   attack before the network, and a tapped delay per room type gives the early reflections.
//!
//! The decay filter is a low shelf and a high shelf around a mid gain. Each first-order shelf's
//! magnitude stays between its two plateau gains, so the loop gain never exceeds the larger of
//! the low and mid gains, which are below 1 for any finite RT60: the network can't run away.
//! Freeze sets an hour-long RT60 and mutes the input.

use super::delay::{flush, note_ms, Biquad, Line, Osc, MAX_RATE, NOTE_LABELS};
use super::{Effect, EffectInfo, FactoryPreset, FxContext, Key, ParamKind, ParamSpec, Unit};

// ---- parameters -----------------------------------------------------------------------------

pub const ROOM: u32 = 0;
pub const PREDELAY: u32 = 1;
pub const SYNC: u32 = 2;
pub const PREDELAY_NOTE: u32 = 3;
pub const SIZE: u32 = 4;
/// RT60 in ms (the contract has no seconds unit).
pub const DECAY: u32 = 5;
pub const DENSITY: u32 = 6;
pub const DIFFUSION: u32 = 7;
/// How much faster the highs decay than the mids.
pub const DAMPING: u32 = 8;
/// The low band's RT60 as a multiple of the mid RT60.
pub const LOW_DECAY: u32 = 9;
/// 0 = early reflections only, 0.5 = both at full level, 1 = late reverb only.
pub const BALANCE: u32 = 10;
pub const MOD_RATE: u32 = 11;
pub const MOD_DEPTH: u32 = 12;
pub const WIDTH: u32 = 13;
pub const LOW_CUT: u32 = 14;
pub const HIGH_CUT: u32 = 15;
pub const FREEZE: u32 = 16;
pub const DRY: u32 = 17;
pub const WET: u32 = 18;

pub const ROOM_TYPES: &[&str] = &["Plate", "Room", "Hall", "Chamber", "Cathedral", "Ambience"];
const PLATE: usize = 0;

/// The longest pre-delay (synced values are clamped to it).
const MAX_PREDELAY_MS: f32 = 1000.0;
/// Size maps to this scale of the room's nominal dimensions: 0.35× .. 1.65×, 1× at 50 %.
const SIZE_MIN: f32 = 0.35;
const SIZE_SPAN: f32 = 1.3;
const SIZE_MAX: f32 = SIZE_MIN + SIZE_SPAN;
/// The largest modulation excursion of the network's lines.
const MOD_MS: f32 = 1.0;
/// Crossover frequencies of the two-band decay filter.
const LOW_XOVER_HZ: f32 = 250.0;
const HIGH_XOVER_HZ: f32 = 3500.0;
/// Freeze's RT60: an hour, infinite in practice, but finite so the loop stays (just) lossy.
/// The modulated lines' interpolation still darkens a frozen sound slowly.
const FREEZE_RT60: f32 = 3600.0;
const MAX_TAIL_SECS: f32 = 30.0;
/// How fast a size change may move a network line (samples per sample) and the size scale (per
/// sample), so sweeping Size bends the pitch by at most about 5 % instead of clicking.
const LINE_SLEW: f32 = 0.05;
const SCALE_SLEW: f32 = 1e-5;
/// The fade when the room type changes, which clears the old tail.
const TYPE_FADE_MS: f32 = 15.0;

const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, kind: ParamKind, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind, log }
}

const C: ParamKind = ParamKind::Continuous;

static PARAMS: [ParamSpec; 19] = [
    spec(ROOM, "Room Type", 0.0, 5.0, 2.0, Unit::None, ParamKind::Choice(ROOM_TYPES), false),
    spec(PREDELAY, "Pre-delay", 0.0, 500.0, 20.0, Unit::Ms, C, false),
    spec(SYNC, "Sync", 0.0, 1.0, 0.0, Unit::None, ParamKind::Toggle, false),
    spec(PREDELAY_NOTE, "Pre-delay Note", 0.0, 17.0, 12.0, Unit::Note, ParamKind::Choice(NOTE_LABELS), false),
    spec(SIZE, "Size", 0.0, 1.0, 0.5, Unit::Percent, C, false),
    spec(DECAY, "Decay", 100.0, 20_000.0, 2000.0, Unit::Ms, C, true),
    spec(DENSITY, "Density", 0.0, 1.0, 0.7, Unit::Percent, C, false),
    spec(DIFFUSION, "Diffusion", 0.0, 1.0, 0.7, Unit::Percent, C, false),
    spec(DAMPING, "Damping", 0.0, 1.0, 0.4, Unit::Percent, C, false),
    spec(LOW_DECAY, "Low Decay", 0.5, 2.0, 1.0, Unit::None, C, true),
    spec(BALANCE, "Early/Late", 0.0, 1.0, 0.5, Unit::Percent, C, false),
    spec(MOD_RATE, "Mod Rate", 0.05, 5.0, 0.6, Unit::Hz, C, true),
    spec(MOD_DEPTH, "Mod Depth", 0.0, 1.0, 0.35, Unit::Percent, C, false),
    spec(WIDTH, "Width", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(LOW_CUT, "Low Cut", 20.0, 1000.0, 30.0, Unit::Hz, C, true),
    spec(HIGH_CUT, "High Cut", 1000.0, 20_000.0, 16_000.0, Unit::Hz, C, true),
    spec(FREEZE, "Freeze", 0.0, 1.0, 0.0, Unit::None, ParamKind::Toggle, false),
    spec(DRY, "Dry", 0.0, 1.0, 1.0, Unit::Percent, C, false),
    spec(WET, "Wet", 0.0, 1.0, 0.3, Unit::Percent, C, false),
];

/// Factory presets. Room types: 0 Plate, 1 Room, 2 Hall, 3 Chamber, 4 Cathedral, 5 Ambience.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset {
        name: "Vocal Plate",
        values: &[(ROOM, 0.0), (PREDELAY, 35.0), (DECAY, 1800.0), (DAMPING, 0.45), (LOW_CUT, 150.0), (HIGH_CUT, 11_000.0), (WET, 0.25)],
    },
    FactoryPreset {
        name: "Bright Plate",
        values: &[(ROOM, 0.0), (PREDELAY, 10.0), (DECAY, 2400.0), (DAMPING, 0.1), (DIFFUSION, 0.8), (HIGH_CUT, 18_000.0), (LOW_CUT, 120.0)],
    },
    FactoryPreset {
        name: "Snare Plate",
        values: &[(ROOM, 0.0), (PREDELAY, 5.0), (SIZE, 0.4), (DECAY, 1300.0), (DENSITY, 0.9), (DAMPING, 0.3), (LOW_CUT, 200.0), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Small Room",
        values: &[(ROOM, 1.0), (PREDELAY, 4.0), (SIZE, 0.3), (DECAY, 600.0), (DAMPING, 0.5), (BALANCE, 0.4), (WET, 0.25)],
    },
    FactoryPreset {
        name: "Drum Room",
        values: &[(ROOM, 1.0), (PREDELAY, 8.0), (SIZE, 0.6), (DECAY, 1100.0), (DENSITY, 0.85), (DAMPING, 0.35), (BALANCE, 0.35), (LOW_DECAY, 0.8), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Short Slap Room",
        values: &[(ROOM, 1.0), (PREDELAY, 0.0), (SIZE, 0.15), (DECAY, 350.0), (DIFFUSION, 0.4), (BALANCE, 0.2), (HIGH_CUT, 9000.0), (WET, 0.3)],
    },
    FactoryPreset {
        name: "Warm Hall",
        values: &[(ROOM, 2.0), (PREDELAY, 25.0), (SIZE, 0.6), (DECAY, 2600.0), (DAMPING, 0.65), (LOW_DECAY, 1.3), (HIGH_CUT, 8000.0), (WET, 0.3)],
    },
    FactoryPreset {
        name: "Concert Hall",
        values: &[(ROOM, 2.0), (PREDELAY, 30.0), (SIZE, 0.75), (DECAY, 2200.0), (DAMPING, 0.4), (LOW_DECAY, 1.15), (BALANCE, 0.55), (WET, 0.3)],
    },
    FactoryPreset {
        name: "Chamber",
        values: &[(ROOM, 3.0), (PREDELAY, 15.0), (SIZE, 0.5), (DECAY, 1500.0), (DENSITY, 0.8), (DAMPING, 0.4), (WET, 0.28)],
    },
    FactoryPreset {
        name: "Big Cathedral",
        values: &[(ROOM, 4.0), (PREDELAY, 60.0), (SIZE, 0.85), (DECAY, 7000.0), (DAMPING, 0.5), (LOW_DECAY, 1.25), (MOD_DEPTH, 0.45), (WET, 0.35)],
    },
    FactoryPreset {
        name: "Ambience Glue",
        values: &[(ROOM, 5.0), (PREDELAY, 0.0), (SIZE, 0.4), (DECAY, 450.0), (DENSITY, 0.9), (BALANCE, 0.45), (WIDTH, 0.8), (WET, 0.15)],
    },
    FactoryPreset {
        name: "Long Ambient Pad",
        values: &[(ROOM, 4.0), (PREDELAY, 80.0), (SIZE, 1.0), (DECAY, 14_000.0), (DAMPING, 0.55), (MOD_RATE, 0.3), (MOD_DEPTH, 0.7), (BALANCE, 0.85), (LOW_CUT, 120.0), (WET, 0.45)],
    },
    FactoryPreset {
        name: "Synced Pre-delay Hall",
        values: &[(ROOM, 2.0), (SYNC, 1.0), (PREDELAY_NOTE, 12.0), (DECAY, 2400.0), (WET, 0.3)],
    },
    FactoryPreset {
        name: "Frozen Pad",
        values: &[(ROOM, 2.0), (SIZE, 0.9), (DECAY, 8000.0), (MOD_DEPTH, 0.6), (BALANCE, 1.0), (FREEZE, 1.0), (DRY, 0.0), (WET, 0.7)],
    },
];

pub fn info() -> EffectInfo {
    EffectInfo {
        id: "builtin:reverb",
        name: "Reverb",
        category: "Reverb",
        params: &PARAMS,
        meters: &[],
        sidechain: false,
        new: |sample_rate| Box::new(Reverb::new(sample_rate)),
    }
}

// ---- room definitions -----------------------------------------------------------------------

/// A room type's geometry at 1× size.
struct RoomDef {
    /// The shortest and longest network line, in ms.
    lines: (f32, f32),
    /// The in-loop allpasses' nominal length, in ms.
    allpass_ms: f32,
    /// The input diffusers' length scale.
    diffuser: f32,
    /// Early reflections: tap count, first and last tap in ms, and the pattern's seed.
    /// `src/components/fx/Reverb.tsx` draws the same pattern; keep the two in step.
    er: (usize, f32, f32, u32),
    /// When the late reverb starts after the pre-delay, in ms.
    late_ms: f32,
}

const ROOMS: [RoomDef; 6] = [
    // Plate: the network fields are unused. A real plate has no early reflections; the short
    // dense spray stands in for the onset of the plate's own diffusion.
    RoomDef { lines: (1.0, 1.0), allpass_ms: 1.0, diffuser: 1.0, er: (18, 0.7, 13.0, 0x68e3_1da4), late_ms: 0.0 },
    // Room
    RoomDef { lines: (9.0, 38.0), allpass_ms: 1.7, diffuser: 0.8, er: (14, 2.5, 42.0, 0x02f6_e2b1), late_ms: 8.0 },
    // Hall
    RoomDef { lines: (21.0, 97.0), allpass_ms: 3.1, diffuser: 1.3, er: (12, 8.0, 85.0, 0x9e37_79b9), late_ms: 22.0 },
    // Chamber
    RoomDef { lines: (13.0, 54.0), allpass_ms: 2.3, diffuser: 1.0, er: (16, 3.5, 58.0, 0x7f4a_7c15), late_ms: 12.0 },
    // Cathedral
    RoomDef { lines: (37.0, 158.0), allpass_ms: 4.4, diffuser: 1.6, er: (10, 16.0, 150.0, 0x5bd1_e995), late_ms: 40.0 },
    // Ambience
    RoomDef { lines: (4.3, 17.0), allpass_ms: 0.9, diffuser: 0.55, er: (20, 1.1, 22.0, 0x1b87_3593), late_ms: 3.0 },
];

const N: usize = 16;
const MAX_TAPS: usize = 20;

/// Small irregular offsets so the line lengths aren't a clean geometric series.
const LINE_JITTER: [f32; N] = [0.0, 0.31, -0.22, 0.17, -0.35, 0.26, -0.08, 0.39, -0.29, 0.12, -0.18, 0.33, -0.37, 0.05, 0.24, -0.13];
/// The order lines take their lengths in, so neighbours in the matrix differ a lot.
const LINE_ORDER: [usize; N] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
/// Each in-loop allpass's length relative to the room's nominal one.
const ALLPASS_SPREAD: [f32; N] = [0.62, 1.31, 0.84, 1.12, 0.71, 1.38, 0.93, 1.05, 0.66, 1.24, 0.79, 1.17, 0.88, 1.43, 0.74, 1.0];
/// The input diffusers per channel, in ms at a diffuser scale of 1.
const DIFFUSER_MS: [[f32; 4]; 2] = [[1.37, 2.13, 3.73, 5.51], [1.53, 2.37, 3.31, 5.87]];

/// The Hadamard matrix entry: rows and columns are mutually orthogonal ±1 vectors.
const fn hadamard(row: usize, col: usize) -> f32 {
    if (row & col).count_ones() % 2 == 0 { 1.0 } else { -1.0 }
}

/// The next value of the shared pattern generator (an LCG, the same one the panel uses).
fn lcg(s: &mut u32) -> f32 {
    *s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*s >> 8) as f32 / 16_777_216.0
}

/// One channel's early reflection pattern at 1× size: tap times (ms) and gains.
fn er_pattern(def: &RoomDef, channel: u32) -> ([f32; MAX_TAPS], [f32; MAX_TAPS]) {
    let (n, t0, t1, seed) = def.er;
    let mut times = [0.0; MAX_TAPS];
    let mut gains = [0.0; MAX_TAPS];
    let mut s = seed.wrapping_add(channel * 7919);
    let mut energy = 0.0;
    for k in 0..n.min(MAX_TAPS) {
        let pos = (k as f32 + 0.2 + 0.6 * lcg(&mut s)) / n as f32;
        times[k] = t0 + (t1 - t0) * pos;
        let amp = 0.2f32.powf(pos) * (0.55 + 0.45 * lcg(&mut s));
        gains[k] = if lcg(&mut s) < 0.5 { -amp } else { amp };
        energy += amp * amp;
    }
    if energy > 0.0 {
        let norm = 0.8 / energy.sqrt();
        gains.iter_mut().for_each(|g| *g *= norm);
    }
    (times, gains)
}

fn is_prime(n: usize) -> bool {
    if n < 4 {
        return n > 1;
    }
    if n % 2 == 0 {
        return false;
    }
    let mut d = 3;
    while d * d <= n {
        if n % d == 0 {
            return false;
        }
        d += 2;
    }
    true
}

/// Line lengths are rounded up to a prime so no two share a factor (fewer coinciding echoes).
fn next_prime(n: usize) -> usize {
    let mut n = n.max(2);
    while !is_prime(n) {
        n += 1;
    }
    n
}

fn ms_to_samples(ms: f32, sample_rate: f32) -> f32 {
    ms * 0.001 * sample_rate
}

/// A buffer long enough for `ms` at a sample rate, with room for interpolation.
fn buf_len(ms: f32, sample_rate: f32) -> usize {
    ms_to_samples(ms, sample_rate) as usize + 8
}

// ---- building blocks ------------------------------------------------------------------------

/// A Schroeder allpass with a whole-sample delay.
#[inline]
fn allpass(line: &mut Line, x: f32, d: usize, g: f32) -> f32 {
    let v = line.read(d);
    let w = flush(x + g * v);
    line.write(w);
    v - g * w
}

/// A Schroeder allpass with a fractional (modulated or gliding) delay.
#[inline]
fn allpass_frac(line: &mut Line, x: f32, d: f32, g: f32) -> f32 {
    let v = line.read_frac(d);
    let w = flush(x + g * v);
    line.write(w);
    v - g * w
}

/// The per-line decay: a mid gain with first-order low and high shelves around it.
#[derive(Clone, Copy)]
struct DecayFilter {
    g: f32,
    /// Low and high gains relative to the mid gain.
    low: f32,
    high: f32,
    a_low: f32,
    a_high: f32,
    s_low: f32,
    s_high: f32,
}

impl DecayFilter {
    fn new() -> DecayFilter {
        DecayFilter { g: 0.0, low: 1.0, high: 1.0, a_low: 0.0, a_high: 0.0, s_low: 0.0, s_high: 0.0 }
    }

    /// Sets the gains for a loop `delay` samples long so each band decays by 60 dB in its RT60.
    fn set(&mut self, delay: f32, rt_mid: f32, rt_low: f32, rt_high: f32, sample_rate: f32) {
        let gain = |rt: f32| 10f32.powf(-3.0 * delay / (rt * sample_rate));
        self.g = gain(rt_mid);
        self.low = gain(rt_low) / self.g;
        self.high = gain(rt_high) / self.g;
        self.a_low = 1.0 - (-std::f32::consts::TAU * LOW_XOVER_HZ / sample_rate).exp();
        self.a_high = 1.0 - (-std::f32::consts::TAU * HIGH_XOVER_HZ / sample_rate).exp();
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        self.s_low = flush(self.s_low + self.a_low * (x - self.s_low));
        let x = x + (self.low - 1.0) * self.s_low;
        self.s_high = flush(self.s_high + self.a_high * (x - self.s_high));
        let hp = x - self.s_high;
        self.g * (self.s_high + self.high * hp)
    }

    fn clear(&mut self) {
        self.s_low = 0.0;
        self.s_high = 0.0;
    }
}

/// Decay settings shared by both engines.
#[derive(Clone, Copy)]
struct Rt {
    mid: f32,
    low: f32,
    high: f32,
}

// ---- the feedback delay network -------------------------------------------------------------

struct Fdn {
    lines: [Line; N],
    allpasses: [Line; N],
    filters: [DecayFilter; N],
    /// The current (gliding) and target line lengths in samples.
    cur: [f32; N],
    target: [f32; N],
    allpass_len: [usize; N],
    osc: [Osc; N],
    diffusers: [[Line; 4]; 2],
    diffuser_len: [[usize; 4]; 2],
}

impl Fdn {
    /// Buffer sizes in samples at a sample rate: lines, in-loop allpasses, input diffusers.
    fn sizes(sample_rate: f32) -> [usize; 3] {
        let max = |f: &dyn Fn(&RoomDef) -> f32| ROOMS.iter().map(f).fold(0.0, f32::max);
        let line_ms = max(&|r| r.lines.1) * 1.06 * SIZE_MAX + MOD_MS * 2.0 + 1.0;
        let ap_ms = max(&|r| r.allpass_ms) * 1.5;
        let diff_ms = max(&|r| r.diffuser) * 6.0;
        [line_ms, ap_ms, diff_ms].map(|ms| buf_len(ms, sample_rate))
    }

    fn new(alloc_rate: f32) -> Fdn {
        let [line, ap, diff] = Fdn::sizes(alloc_rate);
        Fdn {
            lines: std::array::from_fn(|_| Line::new(line)),
            allpasses: std::array::from_fn(|_| Line::new(ap)),
            filters: [DecayFilter::new(); N],
            cur: [100.0; N],
            target: [100.0; N],
            allpass_len: [8; N],
            osc: std::array::from_fn(|i| Osc::new(i as f32 * 2.399)),
            diffusers: std::array::from_fn(|_| std::array::from_fn(|_| Line::new(diff))),
            diffuser_len: [[8; 4]; 2],
        }
    }

    /// Sizes the buffers in use for a sample rate (the room with the longest lines at full size).
    fn set_rate(&mut self, sample_rate: f32) {
        let [line, ap, diff] = Fdn::sizes(sample_rate);
        self.lines.iter_mut().for_each(|l| l.set_active(line));
        self.allpasses.iter_mut().for_each(|l| l.set_active(ap));
        self.diffusers.iter_mut().flatten().for_each(|l| l.set_active(diff));
    }

    /// Sets the line lengths for a room and size. With `snap` they jump there; otherwise they
    /// glide (a size change while playing).
    fn configure(&mut self, def: &RoomDef, scale: f32, sample_rate: f32, snap: bool) {
        let (lo, hi) = def.lines;
        for i in 0..N {
            let pos = (LINE_ORDER[i] as f32 + LINE_JITTER[i]).clamp(0.0, 15.0) / 15.0;
            let ms = lo * (hi / lo).powf(pos) * scale;
            let samples = ms_to_samples(ms, sample_rate) as usize;
            let max = (self.lines[i].max_delay() - ms_to_samples(MOD_MS * 2.0, sample_rate)) as usize;
            self.target[i] = next_prime(samples.max(8)).min(max) as f32;
            if snap {
                self.cur[i] = self.target[i];
            }
            let ap = ms_to_samples(def.allpass_ms * ALLPASS_SPREAD[i], sample_rate) as usize;
            self.allpass_len[i] = next_prime(ap.max(2)).min(self.allpasses[i].max_delay() as usize);
        }
        for ch in 0..2 {
            for k in 0..4 {
                let d = ms_to_samples(DIFFUSER_MS[ch][k] * def.diffuser, sample_rate) as usize;
                self.diffuser_len[ch][k] = next_prime(d.max(2)).min(self.diffusers[ch][k].max_delay() as usize);
            }
        }
    }

    fn set_decay(&mut self, rt: Rt, sample_rate: f32) {
        for i in 0..N {
            // The in-loop allpass adds its length to the loop on average.
            let delay = self.target[i] + self.allpass_len[i] as f32;
            self.filters[i].set(delay, rt.mid, rt.low, rt.high, sample_rate);
        }
    }

    fn set_mod_rate(&mut self, hz: f32, sample_rate: f32) {
        for (i, osc) in self.osc.iter_mut().enumerate() {
            // Spread the rates so the lines never beat together.
            osc.set_freq(hz * (0.62 + 0.76 * LINE_ORDER[i] as f32 / 15.0), sample_rate);
        }
    }

    /// The mean loop length, for level normalisation.
    fn mean_len(&self) -> f32 {
        self.target.iter().sum::<f32>() / N as f32
    }

    fn clear(&mut self) {
        for line in self.lines.iter_mut().chain(&mut self.allpasses) {
            line.clear();
        }
        for ch in &mut self.diffusers {
            for line in ch {
                line.clear();
            }
        }
        for f in &mut self.filters {
            f.clear();
        }
        for (i, o) in self.osc.iter_mut().enumerate() {
            o.set_phase(i as f32 * 2.399);
        }
    }

    /// One sample. `x` is the stereo input already scaled; returns the late reverb.
    #[inline]
    fn tick(&mut self, x: [f32; 2], diffusion: f32, density: f32, excursion: f32, glide: f32) -> [f32; 2] {
        // Input diffusion: four allpasses per side smear transients before the network.
        let mut d = x;
        for ch in 0..2 {
            for k in 0..4 {
                let g = if k % 2 == 0 { diffusion } else { -diffusion };
                d[ch] = allpass(&mut self.diffusers[ch][k], d[ch], self.diffuser_len[ch][k], g);
            }
        }

        let mut y = [0.0f32; N];
        for i in 0..N {
            self.cur[i] += ((self.target[i] - self.cur[i]) * glide).clamp(-LINE_SLEW, LINE_SLEW);
            // The excursion is centred one depth past the set length, so it never reads too close.
            let m = excursion * (1.0 + self.osc[i].tick());
            let v = self.lines[i].read_frac(self.cur[i] + m);
            y[i] = allpass(&mut self.allpasses[i], v, self.allpass_len[i], density);
        }

        // Outputs: two orthogonal Hadamard rows, so left and right are decorrelated.
        let (mut out_l, mut out_r) = (0.0, 0.0);
        for i in 0..N {
            out_l += hadamard(4, i) * y[i];
            out_r += hadamard(8, i) * y[i];
        }

        // Decay, then mix through the Hadamard matrix (a fast Walsh-Hadamard transform).
        let mut m = [0.0f32; N];
        for i in 0..N {
            m[i] = self.filters[i].process(y[i]);
        }
        let mut h = 1;
        while h < N {
            for i in (0..N).step_by(2 * h) {
                for j in i..i + h {
                    let (a, b) = (m[j], m[j + h]);
                    m[j] = a + b;
                    m[j + h] = a - b;
                }
            }
            h *= 2;
        }
        for i in 0..N {
            // The input reaches every line, with orthogonal sign patterns per side.
            let input = hadamard(1, i) * d[0] + hadamard(2, i) * d[1];
            self.lines[i].write(flush(0.25 * m[i] + input));
        }
        [out_l * 0.25, out_r * 0.25]
    }

    fn renormalize(&mut self) {
        for osc in &mut self.osc {
            osc.normalize();
        }
    }

    #[cfg(test)]
    fn has_subnormal(&self) -> bool {
        self.lines.iter().chain(&self.allpasses).any(|l| l.has_subnormal())
            || self.diffusers.iter().flatten().any(|l| l.has_subnormal())
            || self.filters.iter().any(|f| f.s_low.is_subnormal() || f.s_high.is_subnormal())
    }
}

// ---- Dattorro's plate -----------------------------------------------------------------------

/// The paper's sample rate; its lengths are in samples at this rate.
const DATTORRO_RATE: f32 = 29_761.0;
const PLATE_INPUT: [usize; 4] = [142, 107, 379, 277];
const PLATE_INPUT_GAIN: [f32; 4] = [0.75, 0.75, 0.625, 0.625];
/// Each tank half: modulated allpass, delay, allpass, delay.
const PLATE_TANK: [[f32; 4]; 2] = [[672.0, 4453.0, 1800.0, 3720.0], [908.0, 4217.0, 2656.0, 3163.0]];
/// The paper's modulation excursion, in samples at its rate.
const PLATE_EXCURSION: f32 = 16.0;
/// Output taps: (half, element: 1 = first delay, 2 = second allpass, 3 = second delay, offset,
/// sign), from the paper's table.
type Tap = (usize, usize, f32, f32);
const PLATE_TAPS: [[Tap; 7]; 2] = [
    [(1, 1, 266.0, 1.0), (1, 1, 2974.0, 1.0), (1, 2, 1913.0, -1.0), (1, 3, 1996.0, 1.0), (0, 1, 1990.0, -1.0), (0, 2, 187.0, -1.0), (0, 3, 1066.0, -1.0)],
    [(0, 1, 353.0, 1.0), (0, 1, 3627.0, 1.0), (0, 2, 1228.0, -1.0), (0, 3, 2673.0, 1.0), (1, 1, 2111.0, -1.0), (1, 2, 335.0, -1.0), (1, 3, 121.0, -1.0)],
];

struct Plate {
    input: [Line; 4],
    input_len: [usize; 4],
    /// Per half: the four tank elements' buffers.
    tank: [[Line; 4]; 2],
    filters: [DecayFilter; 2],
    osc: Osc,
    /// Samples per paper sample (sample rate / 29761), times the gliding size scale.
    k: f32,
    scale: f32,
    scale_target: f32,
    feed: [f32; 2],
}

impl Plate {
    /// A buffer for `n` paper samples at the largest size, plus the modulation.
    fn size(n: f32, sample_rate: f32) -> usize {
        let k = sample_rate / DATTORRO_RATE;
        (n * k * SIZE_MAX + PLATE_EXCURSION * k * 3.0) as usize + 8
    }

    fn new(alloc_rate: f32) -> Plate {
        let k = alloc_rate / DATTORRO_RATE;
        Plate {
            input: std::array::from_fn(|i| Line::new(Plate::size(PLATE_INPUT[i] as f32, alloc_rate))),
            input_len: PLATE_INPUT,
            tank: std::array::from_fn(|h| std::array::from_fn(|e| Line::new(Plate::size(PLATE_TANK[h][e], alloc_rate)))),
            filters: [DecayFilter::new(); 2],
            osc: Osc::new(0.0),
            k,
            scale: 1.0,
            scale_target: 1.0,
            feed: [0.0; 2],
        }
    }

    fn set_rate(&mut self, sample_rate: f32) {
        for (i, line) in self.input.iter_mut().enumerate() {
            line.set_active(Plate::size(PLATE_INPUT[i] as f32, sample_rate));
        }
        for (h, half) in self.tank.iter_mut().enumerate() {
            for (e, line) in half.iter_mut().enumerate() {
                line.set_active(Plate::size(PLATE_TANK[h][e], sample_rate));
            }
        }
        self.k = sample_rate / DATTORRO_RATE;
        // The input diffusers are fixed: scaled by the sample rate only.
        for i in 0..4 {
            self.input_len[i] = ((PLATE_INPUT[i] as f32 * self.k).round() as usize).max(1);
        }
    }

    fn set_decay(&mut self, rt: Rt, sample_rate: f32) {
        for h in 0..2 {
            let half: f32 = PLATE_TANK[h].iter().sum::<f32>() * self.k * self.scale_target;
            self.filters[h].set(half, rt.mid, rt.low, rt.high, sample_rate);
        }
    }

    fn clear(&mut self) {
        for line in self.input.iter_mut().chain(self.tank.iter_mut().flatten()) {
            line.clear();
        }
        for f in &mut self.filters {
            f.clear();
        }
        self.feed = [0.0; 2];
        self.osc.set_phase(0.0);
    }

    /// One sample of mono input; returns the stereo tank output.
    #[inline]
    fn tick(&mut self, x: f32, diffusion: f32, density: f32, excursion: f32, glide: f32) -> [f32; 2] {
        self.scale += ((self.scale_target - self.scale) * glide).clamp(-SCALE_SLEW, SCALE_SLEW);
        let mut d = x;
        for i in 0..4 {
            d = allpass(&mut self.input[i], d, self.input_len[i], PLATE_INPUT_GAIN[i] * diffusion);
        }
        // Decay diffusion 1 (negative in the paper) and 2.
        let dd1 = -(0.35 + 0.4 * density);
        let dd2 = 0.25 + 0.25 * density;
        let k = self.k * self.scale;
        let lfo = [self.osc.tick(), self.osc.cos()];
        let mut feed = [0.0; 2];
        for h in 0..2 {
            let len = PLATE_TANK[h];
            let input = d + self.feed[1 - h];
            let [ap1, d1, ap2, d2] = &mut self.tank[h];
            let a = allpass_frac(ap1, input, len[0] * k + excursion * (1.0 + lfo[h]), dd1);
            let b = d1.read_frac(len[1] * k);
            d1.write(flush(a));
            let c = self.filters[h].process(b);
            let e = allpass_frac(ap2, c, len[2] * k, dd2);
            feed[h] = d2.read_frac(len[3] * k);
            d2.write(flush(e));
        }
        self.feed = feed;
        let mut out = [0.0; 2];
        for (o, taps) in out.iter_mut().zip(&PLATE_TAPS) {
            for &(h, e, at, sign) in taps {
                *o += sign * self.tank[h][e].read_lin(at * k);
            }
        }
        [out[0] * 0.6, out[1] * 0.6]
    }

    #[cfg(test)]
    fn has_subnormal(&self) -> bool {
        self.input.iter().chain(self.tank.iter().flatten()).any(|l| l.has_subnormal())
            || self.filters.iter().any(|f| f.s_low.is_subnormal() || f.s_high.is_subnormal())
    }
}

// ---- the effect -----------------------------------------------------------------------------

/// The early buffer's length: the latest tap or late feed at the largest size.
fn early_ms() -> f32 {
    ROOMS.iter().map(|r| r.er.2.max(r.late_ms)).fold(0.0, f32::max) * SIZE_MAX + 2.0
}

/// The longest early-reflection smearing allpass.
const SMEAR_MS: f32 = 12.0;

#[inline]
fn approach(v: &mut f32, target: f32, k: f32) {
    *v += (target - *v) * k;
}

pub struct Reverb {
    values: [f32; PARAMS.len()],
    sr: f32,
    /// The room type running now; a different requested one fades over.
    room: usize,
    fade: f32,
    fdn: Fdn,
    plate: Plate,
    pre: [Line; 2],
    predelay: f32,
    /// The pre-delayed input, which the early reflection taps and the late feed read.
    early_buf: [Line; 2],
    early_smear: [Line; 2],
    early_smear_len: [usize; 2],
    er_times: [[f32; MAX_TAPS]; 2],
    er_gains: [[f32; MAX_TAPS]; 2],
    er_count: usize,
    /// The gliding size scale for the early taps.
    scale: f32,
    scale_target: f32,
    low_cut: [Biquad; 2],
    high_cut: [Biquad; 2],
    // Smoothed per-sample gains.
    dry: f32,
    wet: f32,
    width: f32,
    early: f32,
    late: f32,
    input: f32,
    tempo: f64,
    /// Decay, filter or modulation settings changed; recompute at the next block.
    dirty: bool,
    /// The geometry (room or size) changed.
    reshape: bool,
    primed: bool,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Reverb {
        let alloc = sample_rate.max(MAX_RATE);
        let cap = |ms: f32| buf_len(ms, alloc);
        let mut r = Reverb {
            values: std::array::from_fn(|i| PARAMS[i].default),
            sr: sample_rate,
            room: 2,
            fade: 1.0,
            fdn: Fdn::new(alloc),
            plate: Plate::new(alloc),
            pre: [Line::new(cap(MAX_PREDELAY_MS)), Line::new(cap(MAX_PREDELAY_MS))],
            predelay: 0.0,
            early_buf: [Line::new(cap(early_ms())), Line::new(cap(early_ms()))],
            early_smear: [Line::new(cap(SMEAR_MS)), Line::new(cap(SMEAR_MS))],
            early_smear_len: [8; 2],
            er_times: [[0.0; MAX_TAPS]; 2],
            er_gains: [[0.0; MAX_TAPS]; 2],
            er_count: 0,
            scale: 1.0,
            scale_target: 1.0,
            low_cut: [Biquad::new(); 2],
            high_cut: [Biquad::new(); 2],
            dry: 1.0,
            wet: 0.0,
            width: 1.0,
            early: 0.0,
            late: 0.0,
            input: 0.0,
            tempo: 120.0,
            dirty: true,
            reshape: true,
            primed: false,
        };
        r.room = r.requested_room();
        r.set_rate(sample_rate);
        r
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    fn on(&self, id: u32) -> bool {
        self.v(id) >= 0.5
    }

    fn requested_room(&self) -> usize {
        (self.v(ROOM).round() as usize).min(ROOM_TYPES.len() - 1)
    }

    fn set_rate(&mut self, sample_rate: f32) {
        self.sr = sample_rate;
        for line in &mut self.pre {
            line.set_active(buf_len(MAX_PREDELAY_MS, sample_rate));
        }
        for line in &mut self.early_buf {
            line.set_active(buf_len(early_ms(), sample_rate));
        }
        for line in &mut self.early_smear {
            line.set_active(buf_len(SMEAR_MS, sample_rate));
        }
        self.fdn.set_rate(sample_rate);
        self.plate.set_rate(sample_rate);
        self.dirty = true;
        self.reshape = true;
        self.reset();
    }

    fn size_scale(&self) -> f32 {
        SIZE_MIN + SIZE_SPAN * self.v(SIZE)
    }

    /// Applies the room and size to the engines and the early pattern.
    fn configure(&mut self, snap: bool) {
        let def = &ROOMS[self.room];
        let scale = self.size_scale();
        self.scale_target = scale;
        self.plate.scale_target = scale;
        if snap {
            self.scale = scale;
            self.plate.scale = scale;
        }
        if self.room != PLATE {
            self.fdn.configure(def, scale, self.sr, snap);
        }
        self.er_count = def.er.0.min(MAX_TAPS);
        for ch in 0..2 {
            let (t, g) = er_pattern(def, ch as u32);
            self.er_times[ch] = t;
            self.er_gains[ch] = g;
            let smear = ms_to_samples(def.er.1.max(1.0) * 0.6 + 1.3 * ch as f32, self.sr) as usize;
            self.early_smear_len[ch] = next_prime(smear.max(2)).min(self.early_smear[ch].max_delay() as usize);
        }
        self.dirty = true;
    }

    fn rt(&self) -> Rt {
        if self.on(FREEZE) {
            return Rt { mid: FREEZE_RT60, low: FREEZE_RT60, high: FREEZE_RT60 };
        }
        let mid = self.v(DECAY) * 0.001;
        Rt { mid, low: mid * self.v(LOW_DECAY), high: mid * (1.0 - 0.85 * self.v(DAMPING)) }
    }

    fn predelay_ms(&self, tempo: f64) -> f32 {
        let ms = if self.on(SYNC) { note_ms(self.v(PREDELAY_NOTE).round() as usize, tempo) } else { self.v(PREDELAY) };
        ms.clamp(0.0, MAX_PREDELAY_MS)
    }

    /// Input gain so a room's loudness doesn't depend much on its size: a network's energy
    /// grows with the decay time per sample of loop, so longer lines need more input.
    fn input_gain(&self) -> f32 {
        if self.room == PLATE {
            return 0.55;
        }
        let reference = ms_to_samples(40.0, self.sr);
        0.32 * (self.fdn.mean_len() / reference).sqrt()
    }

    fn clear_engines(&mut self) {
        self.fdn.clear();
        self.plate.clear();
        for line in &mut self.early_smear {
            line.clear();
        }
    }

    #[cfg(test)]
    fn has_subnormal(&self) -> bool {
        self.fdn.has_subnormal()
            || self.plate.has_subnormal()
            || self.pre.iter().chain(&self.early_buf).chain(&self.early_smear).any(|l| l.has_subnormal())
            || self.low_cut.iter().chain(&self.high_cut).any(|b| b.has_subnormal())
    }
}

impl Effect for Reverb {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(v) = self.values.get_mut(id as usize) else { return };
        *v = value;
        match id {
            SIZE => self.reshape = true,
            DECAY | DAMPING | LOW_DECAY | FREEZE | MOD_RATE | LOW_CUT | HIGH_CUT => self.dirty = true,
            _ => {}
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

        // A room change fades the wet signal out, swaps the engine with its state cleared, and
        // fades back in. Before the first block it just switches.
        let want = self.requested_room();
        let mut fade_target = 1.0;
        if want != self.room {
            if !self.primed || self.fade <= 0.0 {
                self.room = want;
                self.clear_engines();
                self.configure(true);
                self.reshape = false;
            } else {
                fade_target = 0.0;
            }
        }
        if self.reshape {
            self.reshape = false;
            self.configure(!self.primed);
        }
        if self.dirty {
            self.dirty = false;
            let rt = self.rt();
            self.fdn.set_decay(rt, sr);
            self.plate.set_decay(rt, sr);
            let rate = self.v(MOD_RATE);
            self.fdn.set_mod_rate(rate, sr);
            self.plate.osc.set_freq(rate, sr);
            for b in &mut self.low_cut {
                b.set_highpass(self.values[LOW_CUT as usize], sr);
            }
            for b in &mut self.high_cut {
                b.set_lowpass(self.values[HIGH_CUT as usize], sr);
            }
        }

        let plate = self.room == PLATE;
        let freeze = self.on(FREEZE);
        let balance = self.v(BALANCE);
        let early_t = (2.0 * (1.0 - balance)).min(1.0);
        let late_t = (2.0 * balance).min(1.0);
        let input_t = if freeze { 0.0 } else { self.input_gain() };
        let (dry_t, wet_t, width_t) = (self.v(DRY), self.v(WET), self.v(WIDTH));
        // Two samples is the least a Hermite read can delay; the extra one is constant.
        let predelay_t = ms_to_samples(self.predelay_ms(ctx.tempo), sr).round() + 2.0;
        if !self.primed {
            self.primed = true;
            self.dry = dry_t;
            self.wet = wet_t;
            self.width = width_t;
            self.early = early_t;
            self.late = late_t;
            self.input = input_t;
            self.predelay = predelay_t;
        }

        let ks = super::util::coeff(20.0, sr);
        let fade_k = super::util::coeff(TYPE_FADE_MS / 4.0, sr);
        let glide = super::util::coeff(150.0, sr);
        let pd_glide = super::util::coeff(60.0, sr);
        let diffusion = 0.72 * self.v(DIFFUSION);
        let density = 0.62 * self.v(DENSITY);
        let depth = self.v(MOD_DEPTH);
        let excursion = depth * ms_to_samples(MOD_MS, sr) * 0.5;
        let plate_excursion = depth / 0.35 * PLATE_EXCURSION * self.plate.k * 0.5;
        let smear_g = 0.5 * self.v(DIFFUSION);
        // The paper's input diffusion is the default 70 %.
        let plate_diffusion = (self.v(DIFFUSION) / 0.7).min(1.1);
        let late_ms = ROOMS[self.room].late_ms;

        for n in 0..l.len() {
            approach(&mut self.dry, dry_t, ks);
            approach(&mut self.wet, wet_t, ks);
            approach(&mut self.width, width_t, ks);
            approach(&mut self.early, early_t, ks);
            approach(&mut self.late, late_t, ks);
            approach(&mut self.input, input_t, ks);
            // Pre-delay glides, slew-limited, so automating it doesn't click.
            self.predelay += ((predelay_t - self.predelay) * pd_glide).clamp(-0.5, 0.5);
            self.scale += ((self.scale_target - self.scale) * glide).clamp(-SCALE_SLEW, SCALE_SLEW);

            let (il, ir) = (l[n], r[n]);
            let mut x = [0.0; 2];
            for (ch, input) in [il, ir].into_iter().enumerate() {
                self.pre[ch].write(flush(input));
                x[ch] = self.pre[ch].read_frac(self.predelay) * self.input;
                self.early_buf[ch].write(flush(x[ch]));
            }

            let mut early = [0.0f32; 2];
            // Early reflections: each side's taps read mostly its own side.
            let ms = 0.001 * sr * self.scale;
            for ch in 0..2 {
                let (own, other) = (&self.early_buf[ch], &self.early_buf[1 - ch]);
                let mut acc = 0.0;
                for k in 0..self.er_count {
                    let d = 1.0 + self.er_times[ch][k] * ms;
                    acc += self.er_gains[ch][k] * (0.8 * own.read_lin(d) + 0.2 * other.read_lin(d));
                }
                early[ch] = allpass(&mut self.early_smear[ch], acc, self.early_smear_len[ch], smear_g);
            }
            let d = 1.0 + late_ms * ms;
            let feed = [self.early_buf[0].read_lin(d), self.early_buf[1].read_lin(d)];
            let late = if plate {
                self.plate.tick(0.5 * (feed[0] + feed[1]), plate_diffusion, self.v(DENSITY), plate_excursion, glide)
            } else {
                self.fdn.tick(feed, diffusion, density, excursion, glide)
            };

            let mut wet = [0.0; 2];
            for ch in 0..2 {
                let w = early[ch] * self.early + late[ch] * self.late;
                wet[ch] = self.high_cut[ch].process(self.low_cut[ch].process(w));
            }
            approach(&mut self.fade, fade_target, fade_k);
            let mid = 0.5 * (wet[0] + wet[1]);
            let side = 0.5 * (wet[0] - wet[1]) * self.width;
            let g = self.wet * self.fade;
            l[n] = il * self.dry + (mid + side) * g;
            r[n] = ir * self.dry + (mid - side) * g;
        }
        if fade_target == 0.0 && self.fade < 1e-3 {
            self.fade = 0.0;
        }
        self.fdn.renormalize();
        self.plate.osc.normalize();
    }

    fn reset(&mut self) {
        for line in self.pre.iter_mut().chain(&mut self.early_buf) {
            line.clear();
        }
        self.clear_engines();
        for b in self.low_cut.iter_mut().chain(&mut self.high_cut) {
            b.clear();
        }
        self.fade = 1.0;
        self.primed = false;
        self.reshape = true;
    }

    fn tail_secs(&self) -> f32 {
        if self.on(FREEZE) {
            return MAX_TAIL_SECS;
        }
        let rt = self.rt();
        let longest = rt.mid.max(rt.low);
        let early = ROOMS[self.room].er.2.max(ROOMS[self.room].late_ms) * SIZE_MAX * 0.001;
        (1.5 * longest + self.predelay_ms(self.tempo) * 0.001 + early).min(MAX_TAIL_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn ctx() -> FxContext {
        FxContext { sample_rate: SR, tempo: 120.0, playing: true }
    }

    fn wet_only(room: usize) -> Reverb {
        let mut r = Reverb::new(SR);
        r.set_param(ROOM, room as f32);
        r.set_param(DRY, 0.0);
        r.set_param(WET, 1.0);
        r
    }

    fn run(fx: &mut Reverb, l: &mut [f32], r: &mut [f32]) {
        for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            fx.process(bl, br, None, &ctx());
        }
    }

    fn impulse_response(fx: &mut Reverb, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (secs * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        l[0] = 1.0;
        r[0] = 1.0;
        run(fx, &mut l, &mut r);
        (l, r)
    }

    /// RT60 from the Schroeder energy decay curve: the −5 to −35 dB slope (T30), doubled.
    fn rt60(l: &[f32], r: &[f32]) -> f32 {
        let mut edc: Vec<f64> = l.iter().zip(r).map(|(a, b)| (a * a + b * b) as f64).collect();
        for i in (0..edc.len() - 1).rev() {
            edc[i] += edc[i + 1];
        }
        let total = edc[0];
        let db = |i: usize| 10.0 * (edc[i] / total).log10();
        let t5 = (0..edc.len()).find(|&i| db(i) <= -5.0).unwrap();
        let t35 = (0..edc.len()).find(|&i| db(i) <= -35.0).unwrap();
        2.0 * (t35 - t5) as f32 / SR
    }

    fn noise(seed: &mut u32) -> f32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        (*seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    #[test]
    fn decay_matches_rt60_for_every_room() {
        for room in 0..ROOM_TYPES.len() {
            for rt in [1.0f32, 3.0] {
                let mut fx = wet_only(room);
                fx.set_param(DECAY, rt * 1000.0);
                fx.set_param(DAMPING, 0.0);
                fx.set_param(BALANCE, 1.0);
                let (l, r) = impulse_response(&mut fx, rt * 1.6 + 0.5);
                let measured = rt60(&l, &r);
                let err = (measured - rt).abs() / rt;
                assert!(err < 0.2, "{}: RT60 {rt} s measured {measured:.2} s", ROOM_TYPES[room]);
            }
        }
    }

    #[test]
    fn low_decay_and_damping_shape_the_decay() {
        // Longer lows, faster highs: the decay at 4 kHz is shorter than at 100 Hz.
        let band = |fx: &mut Reverb, hz: f32| {
            let n = (4.0 * SR) as usize;
            let mut l: Vec<f32> = (0..n).map(|i| if i < 4800 { (std::f32::consts::TAU * hz * i as f32 / SR).sin() } else { 0.0 }).collect();
            let mut r = l.clone();
            run(fx, &mut l, &mut r);
            // Level 1.5 s after the burst relative to just after it.
            let rms = |a: usize| (l[a..a + 4800].iter().map(|x| x * x).sum::<f32>() / 4800.0).sqrt();
            rms(4800 + 72_000) / rms(4800 + 4800)
        };
        let mut fx = wet_only(2);
        fx.set_param(DAMPING, 0.8);
        fx.set_param(LOW_DECAY, 2.0);
        fx.set_param(LOW_CUT, 20.0);
        let low = band(&mut fx, 100.0);
        fx.reset();
        let high = band(&mut fx, 6000.0);
        assert!(low > high * 10.0, "low {low} high {high}");
    }

    #[test]
    fn stable_at_max_decay_and_freeze() {
        for room in 0..ROOM_TYPES.len() {
            let mut fx = wet_only(room);
            fx.set_param(DECAY, 20_000.0);
            fx.set_param(LOW_DECAY, 2.0);
            fx.set_param(DAMPING, 0.0);
            fx.set_param(SIZE, 1.0);
            fx.set_param(DENSITY, 1.0);
            fx.set_param(DIFFUSION, 1.0);
            fx.set_param(MOD_DEPTH, 1.0);
            fx.set_param(MOD_RATE, 5.0);
            let mut seed = 1u32;
            let mut peak = 0.0f32;
            let mut held = [0.0f32; 2];
            let blocks = (30.0 * SR / 512.0) as usize;
            for b in 0..blocks {
                if b == blocks / 3 {
                    fx.set_param(FREEZE, 1.0);
                }
                let mut l: Vec<f32> = (0..512).map(|_| noise(&mut seed) * 0.5).collect();
                let mut r: Vec<f32> = (0..512).map(|_| noise(&mut seed) * 0.5).collect();
                fx.process(&mut l, &mut r, None, &ctx());
                let e: f32 = l.iter().chain(&r).map(|x| x * x).sum();
                assert!(e.is_finite(), "{}", ROOM_TYPES[room]);
                peak = l.iter().chain(&r).fold(peak, |p, x| p.max(x.abs()));
                // Frozen energy over half a second, a second in and at the end.
                if (blocks / 3 + 100..blocks / 3 + 150).contains(&b) {
                    held[0] += e;
                }
                if b >= blocks - 50 {
                    held[1] += e;
                }
            }
            assert!(peak < 30.0, "{}: peak {peak}", ROOM_TYPES[room]);
            let change = 10.0 * (held[1] / held[0]).log10();
            assert!(change.abs() < 4.0, "{}: freeze drifted {change:.1} dB", ROOM_TYPES[room]);
        }
    }

    #[test]
    fn wet_is_decorrelated() {
        for room in 0..ROOM_TYPES.len() {
            let mut fx = wet_only(room);
            let mut seed = 7u32;
            let n = (3.0 * SR) as usize;
            let mut l: Vec<f32> = (0..n).map(|_| noise(&mut seed)).collect();
            let mut r = l.clone();
            run(&mut fx, &mut l, &mut r);
            let (a, b) = (&l[24_000..], &r[24_000..]);
            let dot: f64 = a.iter().zip(b).map(|(x, y)| (*x as f64) * (*y as f64)).sum();
            let ea: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum();
            let eb: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum();
            let rho = dot / (ea * eb).sqrt();
            assert!(rho.abs() < 0.3, "{}: correlation {rho:.2}", ROOM_TYPES[room]);
            // Mono-compatible: the sum keeps most of the energy.
            let mono: f64 = a.iter().zip(b).map(|(x, y)| ((*x + *y) as f64).powi(2)).sum();
            assert!(mono / (ea + eb) > 0.6, "{}: mono loss", ROOM_TYPES[room]);
        }
    }

    #[test]
    fn silence_after_the_tail_without_denormals() {
        for room in 0..ROOM_TYPES.len() {
            let mut fx = wet_only(room);
            fx.set_param(DECAY, 1500.0);
            let tail = fx.tail_secs();
            let (l, r) = impulse_response(&mut fx, tail + 1.0);
            let after = (tail * SR) as usize;
            let peak = l[after..].iter().chain(&r[after..]).fold(0.0f32, |p, x| p.max(x.abs()));
            assert!(peak < 1e-3, "{}: {peak} after the tail", ROOM_TYPES[room]);
            // Keep going: it reaches exact zero and never leaves subnormals behind.
            let mut l = vec![0.0; (25.0 * SR) as usize];
            let mut r = l.clone();
            run(&mut fx, &mut l, &mut r);
            assert!(!fx.has_subnormal(), "{}", ROOM_TYPES[room]);
            let end = l.len() - 4800;
            assert!(l[end..].iter().chain(&r[end..]).all(|&x| x == 0.0), "{}: not silent", ROOM_TYPES[room]);
        }
    }

    fn onset(fx: &mut Reverb) -> usize {
        let (l, r) = impulse_response(fx, 0.8);
        let peak = l.iter().chain(&r).fold(0.0f32, |p, x| p.max(x.abs()));
        (0..l.len()).find(|&i| l[i].abs().max(r[i].abs()) > peak * 1e-3).unwrap()
    }

    #[test]
    fn predelay_is_accurate() {
        for room in [0, 2, 5] {
            let mut base = wet_only(room);
            base.set_param(PREDELAY, 0.0);
            let zero = onset(&mut base);
            let mut fx = wet_only(room);
            fx.set_param(PREDELAY, 37.0);
            assert_eq!(onset(&mut fx) - zero, 1776, "{}", ROOM_TYPES[room]);
            // Synced: a 1/16 at 120 BPM is 125 ms.
            let mut fx = wet_only(room);
            fx.set_param(SYNC, 1.0);
            fx.set_param(PREDELAY_NOTE, 12.0);
            assert_eq!(onset(&mut fx) - zero, 6000, "{}", ROOM_TYPES[room]);
        }
    }

    #[test]
    fn room_change_and_size_sweep_stay_clean() {
        let mut fx = wet_only(2);
        let mut seed = 3u32;
        let mut last = 0.0f32;
        let mut worst = 0.0f32;
        for b in 0..400 {
            if b % 80 == 40 {
                fx.set_param(ROOM, ((b / 80) % ROOM_TYPES.len()) as f32);
            }
            fx.set_param(SIZE, 0.5 + 0.5 * (b as f32 * 0.05).sin());
            let mut l: Vec<f32> = (0..512).map(|_| noise(&mut seed) * 0.1).collect();
            let mut r = l.clone();
            fx.process(&mut l, &mut r, None, &ctx());
            for &y in &l {
                assert!(y.is_finite());
                worst = worst.max((y - last).abs());
                last = y;
            }
        }
        assert!(worst < 0.5, "step {worst}");
    }

    #[test]
    fn rooms_have_similar_loudness() {
        let mut levels = Vec::new();
        for room in 0..ROOM_TYPES.len() {
            let mut fx = wet_only(room);
            let (l, r) = impulse_response(&mut fx, 3.0);
            let e: f32 = l.iter().chain(&r).map(|x| x * x).sum();
            levels.push(10.0 * e.log10());
        }
        let max = levels.iter().cloned().fold(f32::MIN, f32::max);
        let min = levels.iter().cloned().fold(f32::MAX, f32::min);
        assert!(max - min < 8.0, "levels {levels:?}");
    }

    #[test]
    fn presets_are_in_range() {
        assert!((10..=14).contains(&PRESETS.len()));
        for p in PRESETS {
            for &(id, v) in p.values {
                let spec = PARAMS.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("{}: no param {id}", p.name));
                assert!(v >= spec.min && v <= spec.max, "{}: {} = {v}", p.name, spec.name);
            }
        }
        // Each one runs cleanly.
        for p in PRESETS {
            let mut fx = Reverb::new(SR);
            for &(id, v) in p.values {
                fx.set_param(id, v);
            }
            let (l, r) = impulse_response(&mut fx, 0.5);
            assert!(l.iter().chain(&r).all(|x| x.is_finite()), "{}", p.name);
        }
    }

    #[test]
    fn tail_follows_decay() {
        let mut fx = wet_only(2);
        fx.set_param(DECAY, 2000.0);
        fx.set_param(PREDELAY, 0.0);
        let t = fx.tail_secs();
        assert!(t > 3.0 && t < 3.5, "{t}");
        fx.set_param(FREEZE, 1.0);
        assert_eq!(fx.tail_secs(), MAX_TAIL_SECS);
    }
}
