//! The wavetables, made in code. A table is a set of frames, single cycles that morph into each
//! other as the position moves. Each frame is stored at several mip levels, each with half the
//! harmonics of the one before and so half the samples, so playing a level whose top harmonic stays
//! under the voice's Nyquist leaves nothing to alias. They're worked out once, the first time an
//! instance is made (off the audio thread), and read-only after that.

use std::sync::OnceLock;

use realfft::RealFftPlanner;

/// Frames per table: the position runs over them with a crossfade.
pub const FRAMES: usize = 32;
/// The first level's harmonics, with this many levels, halving: 512 down to 1.
pub const LEVELS: usize = 10;
pub const TOP_HARMONICS: usize = 512;
/// Samples per cycle at a level: 8 per cycle of its top harmonic, and never under 256. Playback's
/// cubic interpolation's images are what's left of the aliasing, and at this ratio they're far down
/// (linear at this ratio left a narrow pulse at -52 dB).
const fn level_len(level: usize) -> usize {
    let h = TOP_HARMONICS >> level;
    let n = h * 8;
    if n < 256 {
        256
    } else {
        n
    }
}

pub const TABLE_NAMES: &[&str] = &["Basic Shapes", "Harmonic Rise", "Vowels", "Pulse Sweep", "Sync Sweep", "FM Sweep", "Stepped Digital", "Drawbars", "Wavefolder", "Comb Sweep"];
pub const TABLES: usize = TABLE_NAMES.len();

/// All the tables. One flat block of samples, indexed by [`Bank::frame`].
pub struct Bank {
    samples: Vec<f32>,
    level_start: [usize; LEVELS],
    frame_len: usize,
}

/// One cycle at one level.
#[derive(Clone, Copy)]
pub struct Cycle<'a> {
    pub samples: &'a [f32],
}

impl Bank {
    #[inline]
    pub fn cycle(&self, table: usize, frame: usize, level: usize) -> Cycle<'_> {
        let at = (table * FRAMES + frame) * self.frame_len + self.level_start[level];
        Cycle { samples: &self.samples[at..at + level_len(level)] }
    }
}

/// The level for a phase increment (cycles per tick): the most harmonics that stay under Nyquist.
#[inline]
pub fn level_for(inc: f32) -> usize {
    // Harmonics allowed: 0.5 / inc. The level halves them: level = ceil(log2(512 * inc * 2)).
    let want = (TOP_HARMONICS as f32 * 2.0 * inc).max(1.0);
    let bits = want.to_bits();
    // ceil(log2(x)) from the float's own exponent, no call to log2 in the inner loop.
    let exp = ((bits >> 23) & 0xFF) as i32 - 127;
    let frac = bits & 0x7F_FFFF;
    let level = exp + if frac != 0 { 1 } else { 0 };
    (level.max(0) as usize).min(LEVELS - 1)
}

/// Makes one frame as time-domain samples of a cycle, `pos` running 0..1 over the table.
fn shape(table: usize, pos: f32, n: usize) -> Vec<f32> {
    use std::f32::consts::{PI, TAU};
    let phase = |i: usize| i as f32 / n as f32;
    let partials = |amps: &dyn Fn(usize) -> f32, count: usize| -> Vec<f32> {
        (0..n)
            .map(|i| {
                let p = phase(i);
                let mut s = 0.0f32;
                for h in 1..=count {
                    s += amps(h) * (TAU * h as f32 * p).sin();
                }
                s
            })
            .collect()
    };
    match table {
        // Sine, triangle, saw, square, narrow pulse, crossfading from each to the next.
        0 => {
            let seg = pos * 4.0;
            let k = seg.floor().min(3.0);
            let f = seg - k;
            let w = |kind: usize, p: f32| -> f32 {
                match kind {
                    0 => (TAU * p).sin(),
                    1 => 1.0 - 4.0 * ((p + 0.25).fract() - 0.5).abs(),
                    2 => 1.0 - 2.0 * ((p + 0.5).fract()),
                    3 => {
                        if p < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    _ => {
                        if p < 0.12 {
                            1.0
                        } else {
                            -0.14
                        }
                    }
                }
            };
            (0..n).map(|i| w(k as usize, phase(i)) * (1.0 - f) + w(k as usize + 1, phase(i)) * f).collect()
        }
        // One sine growing to many, partial by partial.
        1 => {
            let count = 1.0 + pos * 47.0;
            let amps = move |h: usize| ((count - h as f32 + 1.0).clamp(0.0, 1.0)) / (h as f32).sqrt();
            partials(&amps, 48)
        }
        // A saw's spectrum through moving formants: a, e, i, o, u.
        2 => {
            const F: [[f32; 3]; 5] = [[800.0, 1150.0, 2900.0], [400.0, 1600.0, 2700.0], [270.0, 2300.0, 3000.0], [450.0, 800.0, 2830.0], [325.0, 700.0, 2530.0]];
            let seg = pos * 4.0;
            let k = (seg.floor() as usize).min(3);
            let f = seg - k as f32;
            let formant = |i: usize| F[k][i] + (F[k + 1][i] - F[k][i]) * f;
            let (f1, f2, f3) = (formant(0), formant(1), formant(2));
            // Laid out at a 110 Hz fundamental.
            let amps = move |h: usize| {
                let hz = h as f32 * 110.0;
                let peak = |c: f32, bw: f32, g: f32| g / (1.0 + ((hz - c) / bw) * ((hz - c) / bw));
                (peak(f1, 90.0, 1.0) + peak(f2, 120.0, 0.6) + peak(f3, 160.0, 0.3) + 0.02) / (h as f32).sqrt()
            };
            partials(&amps, 64)
        }
        // A pulse from square to a sliver.
        3 => {
            let w = 0.5 - 0.47 * pos;
            (0..n).map(|i| if phase(i) < w { 1.0 } else { -1.0 }).map(|v| v - (2.0 * w - 1.0)).collect()
        }
        // A saw reset by a slower master, the sync sweep.
        4 => {
            let ratio = 1.0 + pos * 7.0;
            (0..n).map(|i| 2.0 * (phase(i) * ratio).fract() - 1.0).collect()
        }
        // A sine modulating a sine, by a growing index.
        5 => {
            let index = pos * 9.0;
            (0..n).map(|i| (TAU * phase(i) + index * (TAU * 2.0 * phase(i)).sin()).sin()).collect()
        }
        // A saw on a coarse grid of steps, finer to coarser: the old digital machines' grain.
        6 => {
            let steps = 64.0 * (1.0 - pos) * (1.0 - pos) + 4.0;
            (0..n).map(|i| 2.0 * ((phase(i) * steps).floor() / steps) - 1.0).collect()
        }
        // Registrations of an organ's drawbars.
        7 => {
            let bars: [(usize, f32); 8] = [(1, 1.0), (2, pos), (3, (pos * 2.0).min(1.0) * 0.8), (4, (pos * 3.0 - 0.3).clamp(0.0, 1.0) * 0.7), (5, (pos * 3.0 - 1.0).clamp(0.0, 1.0) * 0.6), (6, (pos * 3.0 - 1.5).clamp(0.0, 1.0) * 0.5), (8, (pos * 2.0 - 1.0).clamp(0.0, 1.0) * 0.5), (16, (pos * 2.0 - 1.2).clamp(0.0, 1.0) * 0.3)];
            (0..n)
                .map(|i| {
                    let p = phase(i);
                    bars.iter().map(|(h, a)| a * (TAU * *h as f32 * p).sin()).sum()
                })
                .collect()
        }
        // A sine through a wavefolder, harder and harder.
        8 => {
            let gain = 1.0 + pos * 7.0;
            (0..n).map(|i| (PI * 0.5 * gain * (TAU * phase(i)).sin()).sin()).collect()
        }
        // A saw with a cycle's echo sliding across it: a comb of notches moves.
        _ => {
            let d = 0.002 + pos * 0.25;
            let saw = |p: f32| 2.0 * (p + 1.0).fract() - 1.0;
            (0..n).map(|i| saw(phase(i)) + saw(phase(i) - d) * 0.9).collect()
        }
    }
}

fn build() -> Bank {
    let mut starts = [0usize; LEVELS];
    let mut frame_len = 0;
    for (l, s) in starts.iter_mut().enumerate() {
        *s = frame_len;
        frame_len += level_len(l);
    }
    let total = TABLES * FRAMES * frame_len;
    let mut samples = vec![0.0f32; total];
    let mut planner = RealFftPlanner::<f32>::new();
    let n = level_len(0);
    let forward = planner.plan_fft_forward(n);
    let mut spec = forward.make_output_vec();
    for table in 0..TABLES {
        for frame in 0..FRAMES {
            let pos = frame as f32 / (FRAMES - 1) as f32;
            let mut x = shape(table, pos, n);
            forward.process(&mut x, &mut spec).unwrap();
            // No DC, and the same peak for every frame after band-limiting at the top level.
            spec[0] = realfft::num_complex::Complex::new(0.0, 0.0);
            let mut norm = 0.0f32;
            for (level, start) in starts.iter().enumerate() {
                let len = level_len(level);
                let harmonics = TOP_HARMONICS >> level;
                let inverse = planner.plan_fft_inverse(len);
                let mut s = inverse.make_input_vec();
                for k in 1..s.len() {
                    if k <= harmonics {
                        // Keeps a fade at the top so that the edge doesn't ring into the cycle.
                        let fade = if harmonics >= 8 && k > harmonics * 7 / 8 { 1.0 - (k - harmonics * 7 / 8) as f32 / (harmonics / 8 + 1) as f32 } else { 1.0 };
                        s[k] = spec[k] * (fade / n as f32);
                    }
                }
                let last = s.len() - 1;
                s[last].im = 0.0;
                let mut out = inverse.make_output_vec();
                inverse.process(&mut s, &mut out).unwrap();
                if level == 0 {
                    norm = out.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
                }
                let at = (table * FRAMES + frame) * frame_len + start;
                for (d, v) in samples[at..at + len].iter_mut().zip(&out) {
                    *d = v / norm;
                }
            }
        }
    }
    Bank { samples, level_start: starts, frame_len }
}

/// The bank, built the first time it's asked for.
pub fn bank() -> &'static Bank {
    static BANK: OnceLock<Bank> = OnceLock::new();
    BANK.get_or_init(build)
}
