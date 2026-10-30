//! FLAC, through the pure-Rust `flacenc` encoder. The samples are quantized block by block as
//! the encoder asks for them, so no second copy of the whole render is made.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use flacenc::component::BitRepr;
use flacenc::error::{SourceError, Verify};
use flacenc::source::{Fill, Source};

use super::{frames, BitDepth, Quantizer};

/// The render, read out as interleaved integer samples.
struct Render<'a> {
    audio: &'a [Vec<f32>],
    rate: u32,
    depth: BitDepth,
    q: Quantizer,
    pos: usize,
    block: Vec<i32>,
}

impl Source for Render<'_> {
    fn channels(&self) -> usize {
        self.audio.len()
    }

    fn bits_per_sample(&self) -> usize {
        self.depth.bits() as usize
    }

    fn sample_rate(&self) -> usize {
        self.rate as usize
    }

    fn read_samples<F: Fill>(&mut self, block_size: usize, dest: &mut F) -> Result<usize, SourceError> {
        let end = (self.pos + block_size).min(frames(self.audio));
        self.block.clear();
        for i in self.pos..end {
            for c in self.audio {
                self.block.push(self.q.int(c[i]));
            }
        }
        dest.fill_interleaved(&self.block)?;
        let read = end - self.pos;
        self.pos = end;
        Ok(read)
    }

    fn len_hint(&self) -> Option<usize> {
        Some(frames(self.audio))
    }
}

pub fn write(path: &Path, rate: u32, audio: &[Vec<f32>], depth: BitDepth) -> Result<()> {
    let config = flacenc::config::Encoder::default().into_verified().map_err(|(_, e)| anyhow!("FLAC encoder settings: {e}"))?;
    let src = Render { audio, rate, depth, q: Quantizer::new(depth), pos: 0, block: Vec::new() };
    let mut stream = flacenc::encode_with_fixed_block_size(&config, src, config.block_size).map_err(|e| anyhow!("FLAC encoding failed: {e}"))?;
    // flacenc counts the short last block in STREAMINFO's minimum block size, but the format
    // leaves it out: with min != max, decoders (symphonia among them) take the stream for a
    // variable-block-size one and can't find its fixed-size frames.
    stream.stream_info_mut().set_block_sizes(config.block_size, config.block_size).map_err(|e| anyhow!("FLAC encoding failed: {e}"))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| anyhow!("FLAC encoding failed: {e}"))?;
    std::fs::write(path, sink.as_slice()).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
