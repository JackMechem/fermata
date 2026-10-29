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
}
