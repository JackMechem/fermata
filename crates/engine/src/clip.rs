//! Audio files held in memory, plus the waveform overview used by the UI.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Frames per peak bucket in the live recording preview.
pub const PEAK_BUCKET: usize = 256;

/// Frames per bucket in a clip's waveform overview. Fine enough that the arrange area's
/// closest zoom still reads a separate value for every pixel.
pub const WAVE_BUCKET: usize = 32;

pub struct Clip {
    pub path: PathBuf,
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

impl Clip {
    pub fn frames(&self) -> usize {
        self.channels.first().map(|c| c.len()).unwrap_or(0)
    }

    pub fn from_channels(path: PathBuf, sample_rate: u32, channels: Vec<Vec<f32>>) -> Clip {
        Clip { path, sample_rate, channels }
    }

    /// Loads any supported audio file. Used for files already in the project, which are WAVs at
    /// the project rate; imports go through `Session::import_audio`, which converts first.
    pub fn load(path: &Path) -> Result<Clip> {
        let d = crate::decode::decode(path)?;
        Ok(Clip::from_channels(path.to_path_buf(), d.sample_rate, d.channels))
    }

    /// The waveform overview, packed for the UI: frame count and bucket size as little-endian
    /// u32s, then a signed (min, max) pair of i16s per `WAVE_BUCKET` frames, taken across
    /// channels.
    pub fn waveform(&self) -> Vec<u8> {
        let frames = self.frames();
        let buckets = frames.div_ceil(WAVE_BUCKET);
        let mut out = Vec::with_capacity(8 + buckets * 4);
        out.extend_from_slice(&(frames as u32).to_le_bytes());
        out.extend_from_slice(&(WAVE_BUCKET as u32).to_le_bytes());
        let q = |v: f32| (v.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        for b in 0..buckets {
            let (i, end) = (b * WAVE_BUCKET, ((b + 1) * WAVE_BUCKET).min(frames));
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for c in &self.channels {
                for &s in &c[i..end] {
                    lo = lo.min(s);
                    hi = hi.max(s);
                }
            }
            out.extend_from_slice(&q(lo).to_le_bytes());
            out.extend_from_slice(&q(hi).to_le_bytes());
        }
        out
    }
}

/// Below this level (about −80 dBFS) every channel counts as silent, so an edit there is as
/// clean as one at a crossing.
const ZERO_LEVEL: f32 = 1e-4;

/// The frame nearest `at`, at most `window` frames away, where an edit makes no click (Logic's
/// Snap Edits to Zero Crossings): where the channels' sum changes sign (the side of the
/// crossing nearer zero), or where every channel is close to silent. Earlier frames win ties.
/// None when there is no such frame in the window.
pub fn nearest_zero_crossing(channels: &[Vec<f32>], at: usize, window: usize) -> Option<usize> {
    let frames = channels.first().map(|c| c.len()).unwrap_or(0);
    if frames == 0 || at >= frames {
        return None;
    }
    let sum = |i: usize| channels.iter().map(|c| c[i]).sum::<f32>();
    let quiet = |i: usize| channels.iter().all(|c| c[i].abs() <= ZERO_LEVEL);
    // `i` sits on a crossing with its neighbour `j` and is the one of the pair nearer zero.
    let crosses = |i: usize, j: usize| {
        let (a, b) = (sum(i), sum(j));
        (a == 0.0 || (a > 0.0) != (b > 0.0)) && a.abs() <= b.abs()
    };
    let good = |i: usize| quiet(i) || (i > 0 && crosses(i, i - 1)) || (i + 1 < frames && crosses(i, i + 1));
    for d in 0..=window {
        if d <= at && good(at - d) {
            return Some(at - d);
        }
        if d > 0 && at + d < frames && good(at + d) {
            return Some(at + d);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_packs_signed_extremes() {
        // A bucket of positive samples then one of negative, across two channels.
        let mut a = vec![0.25f32; WAVE_BUCKET];
        a.extend(vec![-0.5f32; WAVE_BUCKET / 2]);
        let mut b = vec![0.5f32; WAVE_BUCKET];
        b.extend(vec![-1.0f32; WAVE_BUCKET / 2]);
        let c = Clip::from_channels(PathBuf::new(), 48000, vec![a, b]);
        let w = c.waveform();
        let u32_at = |i: usize| u32::from_le_bytes(w[i..i + 4].try_into().unwrap());
        let i16_at = |i: usize| i16::from_le_bytes(w[i..i + 2].try_into().unwrap());
        assert_eq!(u32_at(0), (WAVE_BUCKET * 3 / 2) as u32);
        assert_eq!(u32_at(4), WAVE_BUCKET as u32);
        assert_eq!(w.len(), 8 + 2 * 4);
        assert_eq!((i16_at(8), i16_at(10)), (8192, 16384));
        assert_eq!((i16_at(12), i16_at(14)), (-32767, -16384));
    }

    #[test]
    fn zero_crossing_finds_the_nearest_sign_change() {
        // A 100-frame-period sine: crossings at 0, 50, 100, ...
        let sine: Vec<f32> = (0..1000).map(|i| (i as f32 * std::f32::consts::TAU / 100.0).sin() * 0.5).collect();
        let ch = vec![sine.clone()];
        let z = nearest_zero_crossing(&ch, 140, 20).unwrap();
        assert!((z as i64 - 150).abs() <= 1, "{z}");
        assert!(sine[z].abs() < 0.02);
        let z = nearest_zero_crossing(&ch, 112, 20).unwrap();
        assert!((z as i64 - 100).abs() <= 1, "{z}");
        // Nothing within reach of the peak at 125.
        assert_eq!(nearest_zero_crossing(&ch, 125, 10), None);
        // Out of range.
        assert_eq!(nearest_zero_crossing(&ch, 5000, 10), None);
    }

    #[test]
    fn zero_crossing_uses_the_channels_sum_or_silence() {
        // Left and right in opposite phase: the sum is silent everywhere, so any frame will do.
        let l: Vec<f32> = (0..200).map(|i| if i % 2 == 0 { 0.3 } else { -0.3 }).collect();
        let r: Vec<f32> = l.iter().map(|v| -v).collect();
        assert_eq!(nearest_zero_crossing(&[l.clone(), r], 77, 5), Some(77));
        // A DC offset never crosses, but a stretch of near-silence counts.
        let mut dc = vec![0.4f32; 300];
        for v in &mut dc[200..210] {
            *v = 0.00005;
        }
        assert_eq!(nearest_zero_crossing(&[dc.clone()], 190, 15), Some(200));
        assert_eq!(nearest_zero_crossing(&[dc], 100, 15), None);
        // The side of a crossing nearer zero, and the earlier frame on a tie.
        let step = vec![0.5f32, 0.4, 0.1, -0.3, -0.5, 0.2, -0.2];
        assert_eq!(nearest_zero_crossing(&[step.clone()], 3, 2), Some(2));
        assert_eq!(nearest_zero_crossing(&[step], 6, 0), Some(6));
    }
}
