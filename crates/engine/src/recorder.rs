//! Disk writer for audio recordings. The audio thread pushes samples into one ring buffer per
//! armed track; this thread drains them into WAV files and keeps a copy plus waveform peaks.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use crate::clip::PEAK_BUCKET;
use crate::model::Id;
use crate::prefs::BitDepth;

struct Take {
    consumer: rtrb::Consumer<f32>,
    writer: Option<hound::WavWriter<BufWriter<File>>>,
    path: PathBuf,
    channels: usize,
    /// Integer files get samples scaled to 24 bits (and clipped at full scale).
    int24: bool,
    data: Vec<Vec<f32>>,
    peaks: Vec<f32>,
    bucket_peak: f32,
    bucket_fill: usize,
    /// Peaks not yet handed to the UI.
    fresh_from: usize,
    start: i64,
}

impl Take {
    fn drain(&mut self) {
        let ch = self.channels.max(1);
        let n = self.consumer.slots() / ch * ch;
        if n == 0 {
            return;
        }
        let Ok(chunk) = self.consumer.read_chunk(n) else { return };
        let (a, b) = chunk.as_slices();
        for (i, &s) in a.iter().chain(b.iter()).enumerate() {
            let c = i % ch;
            self.data[c].push(s);
            if let Some(w) = &mut self.writer {
                let _ = if self.int24 { w.write_sample((s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32) } else { w.write_sample(s) };
            }
            self.bucket_peak = self.bucket_peak.max(s.abs());
            if c == ch - 1 {
                self.bucket_fill += 1;
                if self.bucket_fill == PEAK_BUCKET {
                    self.peaks.push(self.bucket_peak);
                    self.bucket_peak = 0.0;
                    self.bucket_fill = 0;
                }
            }
        }
        chunk.commit_all();
    }
}

pub struct FinishedTake {
    pub path: PathBuf,
    pub start: i64,
    pub channels: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TakePreview {
    pub track: Id,
    pub start: i64,
    pub frames: usize,
    /// Index of the first peak in `peaks` within the take.
    pub from: usize,
    pub peaks: Vec<f32>,
}

#[derive(Clone)]
pub struct Recorder {
    takes: Arc<Mutex<HashMap<Id, Take>>>,
}

impl Recorder {
    pub fn new() -> Recorder {
        let takes: Arc<Mutex<HashMap<Id, Take>>> = Arc::default();
        let weak = Arc::downgrade(&takes);
        std::thread::Builder::new()
            .name("recorder".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(10));
                let Some(t) = weak.upgrade() else { break };
                for take in t.lock().values_mut() {
                    take.drain();
                }
            })
            .expect("spawn recorder");
        Recorder { takes }
    }

    pub fn begin(&self, track: Id, path: PathBuf, channels: usize, sample_rate: u32, start: i64, depth: BitDepth) -> anyhow::Result<rtrb::Producer<f32>> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let int24 = depth == BitDepth::Int24;
        let spec = hound::WavSpec {
            channels: channels as u16,
            sample_rate,
            bits_per_sample: if int24 { 24 } else { 32 },
            sample_format: if int24 { hound::SampleFormat::Int } else { hound::SampleFormat::Float },
        };
        let writer = hound::WavWriter::create(&path, spec)?;
        let (tx, rx) = rtrb::RingBuffer::new(sample_rate as usize * channels * 4);
        let take = Take {
            consumer: rx,
            writer: Some(writer),
            path,
            channels,
            int24,
            data: vec![Vec::with_capacity(sample_rate as usize * 60); channels],
            peaks: Vec::new(),
            bucket_peak: 0.0,
            bucket_fill: 0,
            fresh_from: 0,
            start,
        };
        self.takes.lock().insert(track, take);
        Ok(tx)
    }

    /// Waveform peaks recorded since the previous call, for drawing regions while recording.
    pub fn previews(&self) -> Vec<TakePreview> {
        self.takes
            .lock()
            .iter_mut()
            .map(|(&track, t)| {
                let from = t.fresh_from;
                t.fresh_from = t.peaks.len();
                TakePreview { track, start: t.start, frames: t.data[0].len(), from, peaks: t.peaks[from..].to_vec() }
            })
            .collect()
    }

    pub fn finish_all(&self) -> Vec<(Id, FinishedTake)> {
        let mut out = Vec::new();
        for (id, mut t) in self.takes.lock().drain() {
            t.drain();
            if let Some(w) = t.writer.take() {
                if let Err(e) = w.finalize() {
                    log::error!("finalizing {}: {e}", t.path.display());
                }
            }
            out.push((id, FinishedTake { path: t.path, start: t.start, channels: t.data }));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_chosen_bit_depth() {
        let dir = std::env::temp_dir().join(format!("fermata-rec-{}", std::process::id()));
        let rec = Recorder::new();
        for (depth, bits) in [(BitDepth::Int24, 24), (BitDepth::Float32, 32)] {
            let path = dir.join(format!("take{bits}.wav"));
            let mut tx = rec.begin(1, path.clone(), 1, 48000, 0, depth).unwrap();
            for i in 0..480 {
                tx.push((i as f32 / 480.0) * 0.5).unwrap();
            }
            let takes = rec.finish_all();
            assert_eq!(takes[0].1.channels[0].len(), 480);
            let spec = hound::WavReader::open(&path).unwrap().spec();
            assert_eq!(spec.bits_per_sample, bits);
            let d = crate::decode::decode(&path).unwrap();
            assert!((d.channels[0][240] - 0.25).abs() < 1e-5);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
