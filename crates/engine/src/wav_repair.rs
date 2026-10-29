//! Repairs WAV files a crash left behind mid-recording.
//!
//! The recorder (hound) writes the RIFF header first with zero sizes and only fills them in on
//! a checkpoint or when the take finishes, so after a crash the sizes are zero or stale while
//! the audio itself is in the file. The data chunk is always the last chunk of the files
//! Fermata records, so its true size is simply what follows its header, down to a whole frame.
//! Don't use this on arbitrary files: one with chunks after the audio would lose them.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{anyhow, Result};

/// What [`inspect`] found, or what [`repair`] left on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits: u16,
    /// Whole frames of audio in the file (not what the header says).
    pub frames: u64,
    /// Whether the header sizes disagree with the file (always false after a repair).
    pub damaged: bool,
}

/// Where the parts of the header that matter are.
struct Layout {
    channels: u16,
    sample_rate: u32,
    bits: u16,
    block_align: u64,
    riff_size: u32,
    /// Offset of the data chunk's size field; the audio starts 4 bytes later.
    data_size_at: u64,
    data_size: u32,
    file_len: u64,
}

impl Layout {
    fn data_start(&self) -> u64 {
        self.data_size_at + 4
    }

    /// The data size the file supports: whole frames, within what a RIFF size can say.
    fn true_data_size(&self) -> u64 {
        let avail = self.file_len.saturating_sub(self.data_start()).min(u32::MAX as u64 - self.data_start());
        avail / self.block_align * self.block_align
    }

    fn info(&self, damaged: bool) -> WavInfo {
        WavInfo { channels: self.channels, sample_rate: self.sample_rate, bits: self.bits, frames: self.true_data_size() / self.block_align, damaged }
    }

    fn damaged(&self) -> bool {
        let data = self.true_data_size();
        self.data_size as u64 != data || self.riff_size as u64 != self.data_start() + data - 8 || self.file_len != self.data_start() + data
    }
}

fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn layout(f: &mut std::fs::File) -> Result<Layout> {
    let file_len = f.metadata()?.len();
    // The header the recorder writes is 44 to 80 bytes; a few KiB covers any sane one.
    let mut head = vec![0u8; file_len.min(4096) as usize];
    f.seek(SeekFrom::Start(0))?;
    f.read_exact(&mut head)?;
    if head.len() < 12 || &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return Err(anyhow!("not a WAV file (or nothing was written before the crash)"));
    }
    let riff_size = le32(&head[4..8]);
    let mut fmt = None;
    let mut at = 12usize;
    while at + 8 <= head.len() {
        let id = &head[at..at + 4];
        let size = le32(&head[at + 4..at + 8]) as usize;
        if id == b"data" {
            let (channels, sample_rate, block_align, bits) = fmt.ok_or_else(|| anyhow!("the WAV header has no format chunk"))?;
            if block_align == 0 {
                return Err(anyhow!("the WAV format chunk is invalid"));
            }
            return Ok(Layout { channels, sample_rate, bits, block_align: block_align as u64, riff_size, data_size_at: at as u64 + 4, data_size: size as u32, file_len });
        }
        if id == b"fmt " {
            let b = head.get(at + 8..at + 24).ok_or_else(|| anyhow!("the WAV format chunk is cut short"))?;
            fmt = Some((le16(&b[2..4]), le32(&b[4..8]), le16(&b[12..14]), le16(&b[14..16])));
        }
        at += 8 + size + (size & 1);
    }
    Err(anyhow!("the WAV header is cut short (no data chunk)"))
}

/// Reads a recording's header and how much audio the file really holds, changing nothing.
pub fn inspect(path: &Path) -> Result<WavInfo> {
    let mut f = std::fs::File::open(path)?;
    let l = layout(&mut f)?;
    Ok(l.info(l.damaged()))
}

/// Makes a recording interrupted by a crash a valid WAV again: sets the RIFF and data sizes
/// from the file's length and cuts off a partly written last frame. A file that's already
/// fine is left alone. Returns what the file now holds.
pub fn repair(path: &Path) -> Result<WavInfo> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;
    let l = layout(&mut f)?;
    if !l.damaged() {
        return Ok(l.info(false));
    }
    let data = l.true_data_size();
    let end = l.data_start() + data;
    if l.file_len > end {
        f.set_len(end)?;
    }
    f.seek(SeekFrom::Start(4))?;
    f.write_all(&((end - 8) as u32).to_le_bytes())?;
    f.seek(SeekFrom::Start(l.data_size_at))?;
    f.write_all(&(data as u32).to_le_bytes())?;
    f.sync_all()?;
    Ok(WavInfo { damaged: false, ..l.info(false) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fermata-wavrepair-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample(i: usize, c: usize) -> f32 {
        ((i * 7 + c * 3) % 200) as f32 / 200.0 - 0.5
    }

    /// Writes `frames` frames the way the recorder does (same formats), checkpointing the
    /// header after `flushed` frames, then "crashes": the writer is leaked, so its buffer and
    /// final header update never happen. Returns where the audio starts in the file.
    fn crashed_take(path: &Path, bits: u16, channels: u16, frames: usize, flushed: usize) -> usize {
        let spec = hound::WavSpec {
            channels,
            sample_rate: 48000,
            bits_per_sample: bits,
            sample_format: if bits == 24 { hound::SampleFormat::Int } else { hound::SampleFormat::Float },
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames {
            if i == flushed {
                w.flush().unwrap();
            }
            for c in 0..channels as usize {
                let s = sample(i, c);
                if bits == 24 {
                    w.write_sample((s * 8_388_607.0).round() as i32).unwrap();
                } else {
                    w.write_sample(s).unwrap();
                }
            }
        }
        std::mem::forget(w);
        let bytes = std::fs::read(path).unwrap();
        bytes.windows(4).position(|w| w == b"data").unwrap() + 8
    }

    #[test]
    fn repairs_crashed_takes_of_every_recorded_format() {
        let dir = scratch("formats");
        for bits in [24u16, 32] {
            for channels in [1u16, 2] {
                for flushed in [0usize, 5000] {
                    let path = dir.join(format!("take-{bits}-{channels}-{flushed}.wav"));
                    let frames = 20_000;
                    let header = crashed_take(&path, bits, channels, frames, flushed);
                    // Cut the file mid-frame, as a crash mid-write could.
                    let len = std::fs::metadata(&path).unwrap().len() as usize - 1;
                    std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(len as u64).unwrap();
                    let want = (len - header) / (channels as usize * bits as usize / 8);
                    assert!(want > 10_000 && want < frames, "{want}");

                    let before = inspect(&path).unwrap();
                    assert!(before.damaged, "{bits}/{channels}/{flushed}");
                    assert_eq!(before.frames as usize, want);
                    let after = repair(&path).unwrap();
                    assert_eq!((after.frames as usize, after.bits, after.channels, after.damaged), (want, bits, channels, false));
                    assert!(!inspect(&path).unwrap().damaged);
                    // A second repair changes nothing.
                    assert_eq!(repair(&path).unwrap(), after);

                    let d = crate::decode::decode(&path).unwrap();
                    assert_eq!(d.channels.len(), channels as usize);
                    assert_eq!(d.channels[0].len(), want);
                    let tol = if bits == 24 { 1e-6 } else { 0.0 };
                    for i in [0, want / 2, want - 1] {
                        for c in 0..channels as usize {
                            assert!((d.channels[c][i] - sample(i, c)).abs() <= tol, "{bits}/{channels} frame {i}");
                        }
                    }
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leaves_finished_files_alone_and_refuses_empty_ones() {
        let dir = scratch("fine");
        let path = dir.join("fine.wav");
        crate::decode::write_wav(&path, 44100, &[vec![0.25; 1000]]).unwrap();
        let before = std::fs::read(&path).unwrap();
        let info = repair(&path).unwrap();
        assert_eq!((info.frames, info.damaged), (1000, false));
        assert_eq!(std::fs::read(&path).unwrap(), before);

        // Nothing written before the crash: the header never left the writer's buffer.
        let empty = dir.join("empty.wav");
        std::fs::write(&empty, b"").unwrap();
        assert!(repair(&empty).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
