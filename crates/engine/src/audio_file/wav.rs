//! WAV, through hound.

use std::path::Path;

use anyhow::{Context, Result};

use super::{frames, BitDepth, Quantizer};

pub fn write(path: &Path, rate: u32, audio: &[Vec<f32>], depth: BitDepth) -> Result<()> {
    let format = if depth == BitDepth::Float32 { hound::SampleFormat::Float } else { hound::SampleFormat::Int };
    let spec = hound::WavSpec { channels: audio.len() as u16, sample_rate: rate, bits_per_sample: depth.bits(), sample_format: format };
    let mut w = hound::WavWriter::create(path, spec).with_context(|| format!("creating {}", path.display()))?;
    let mut q = Quantizer::new(depth);
    for i in 0..frames(audio) {
        for c in audio {
            match depth {
                BitDepth::Int16 => w.write_sample(q.int(c[i]) as i16)?,
                BitDepth::Int24 => w.write_sample(q.int(c[i]))?,
                BitDepth::Float32 => w.write_sample(c[i])?,
            }
        }
    }
    w.finalize()?;
    Ok(())
}
