//! AIFF, written by hand: big-endian samples in a `FORM` of `COMM` and `SSND` chunks. Integer
//! samples go in plain AIFF; 32-bit float needs AIFF-C, with the `fl32` compression type (what
//! Logic and Core Audio write).

use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{anyhow, Context, Result};

use super::{frames, BitDepth, Quantizer};

/// AIFF-C version 1 (May 1990), the only one there is.
const AIFC_VERSION_1: u32 = 0xA280_5140;
/// The compression name for `fl32`, as a Pascal string (a length byte, then the text) of even
/// length, so it needs no pad byte.
const FL32_NAME: &[u8] = b"\x1532-bit floating point";
const _: () = assert!(FL32_NAME.len() % 2 == 0 && FL32_NAME[0] as usize == FL32_NAME.len() - 1);

/// `rate` as the 80-bit IEEE extended float AIFF stores sample rates in: a 15-bit biased
/// exponent, then a 64-bit mantissa with an explicit leading 1.
pub fn extended(rate: u32) -> [u8; 10] {
    let mut out = [0u8; 10];
    if rate == 0 {
        return out;
    }
    let log2 = 31 - rate.leading_zeros();
    let exponent = 16383 + log2 as u16;
    let mantissa = (rate as u64) << (63 - log2);
    out[..2].copy_from_slice(&exponent.to_be_bytes());
    out[2..].copy_from_slice(&mantissa.to_be_bytes());
    out
}

pub fn write(path: &Path, rate: u32, audio: &[Vec<f32>], depth: BitDepth) -> Result<()> {
    let channels = audio.len();
    let n = frames(audio);
    let bytes = (depth.bits() / 8) as u64;
    let data = n as u64 * channels as u64 * bytes;
    let float = depth == BitDepth::Float32;
    // Chunk bodies: COMM (plus the compression type and name for AIFF-C), SSND's offset and
    // block size before the samples, and FVER for AIFF-C.
    let comm = if float { 18 + 4 + FL32_NAME.len() as u64 } else { 18 };
    let ssnd = 8 + data;
    let pad = data % 2;
    let form = 4 + (8 + comm) + (8 + ssnd + pad) + if float { 8 + 4 } else { 0 };
    if form > u32::MAX as u64 {
        return Err(anyhow!("too long for an AIFF file (over 4 GB): bounce a shorter range or use FLAC"));
    }

    let file = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut w = BufWriter::new(file);
    w.write_all(b"FORM")?;
    w.write_all(&(form as u32).to_be_bytes())?;
    w.write_all(if float { b"AIFC" } else { b"AIFF" })?;
    if float {
        w.write_all(b"FVER")?;
        w.write_all(&4u32.to_be_bytes())?;
        w.write_all(&AIFC_VERSION_1.to_be_bytes())?;
    }
    w.write_all(b"COMM")?;
    w.write_all(&(comm as u32).to_be_bytes())?;
    w.write_all(&(channels as i16).to_be_bytes())?;
    w.write_all(&(n as u32).to_be_bytes())?;
    w.write_all(&(depth.bits() as i16).to_be_bytes())?;
    w.write_all(&extended(rate))?;
    if float {
        w.write_all(b"fl32")?;
        w.write_all(FL32_NAME)?;
    }
    w.write_all(b"SSND")?;
    w.write_all(&(ssnd as u32).to_be_bytes())?;
    w.write_all(&[0u8; 8])?;
    let mut q = Quantizer::new(depth);
    for i in 0..n {
        for c in audio {
            match depth {
                BitDepth::Int16 => w.write_all(&(q.int(c[i]) as i16).to_be_bytes())?,
                BitDepth::Int24 => w.write_all(&q.int(c[i]).to_be_bytes()[1..])?,
                BitDepth::Float32 => w.write_all(&c[i].to_be_bytes())?,
            }
        }
    }
    // Chunks are word-aligned; the pad byte isn't counted in SSND's size.
    if pad == 1 {
        w.write_all(&[0])?;
    }
    w.flush()?;
    Ok(())
}
