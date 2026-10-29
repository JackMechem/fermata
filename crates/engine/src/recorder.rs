//! Disk writer for audio recordings. The audio thread pushes samples into one ring buffer per
//! armed track; this thread drains them into WAV files and keeps a copy plus waveform peaks.
//!
//! So a crash mid-take leaves a usable file, the writer updates the WAV header and asks the OS
//! to put the data on disk every [`CHECKPOINT`]; `session/recovery.rs` repairs whatever came
//! after the last checkpoint from the file's length.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    /// The first write error (a full disk, say): the file stops there, the take is still kept
    /// in memory. `reported` once [`Recorder::take_errors`] has passed it on.
    error: Option<String>,
    reported: bool,
    /// A second handle on the file, for `sync_data` (hound owns the writer's).
    sync: Option<File>,
    /// When the header was last brought up to date and the data synced.
    checkpoint: Instant,
    bits: u16,
}

/// How often a take in progress is made safe on disk: header sizes updated, data synced.
pub const CHECKPOINT: Duration = Duration::from_secs(2);

impl Take {
    /// Writes what the audio thread has captured. Every [`CHECKPOINT`] it also returns a
    /// handle to sync, for the caller to do once it has let go of the takes' lock (`tick`
    /// waits on that lock, and a sync can take a while on a slow disk).
    fn drain(&mut self) -> Option<File> {
        let ch = self.channels.max(1);
        let n = self.consumer.slots() / ch * ch;
        if n == 0 {
            return None;
        }
        let Ok(chunk) = self.consumer.read_chunk(n) else { return None };
        let (a, b) = chunk.as_slices();
        for (i, &s) in a.iter().chain(b.iter()).enumerate() {
            let c = i % ch;
            self.data[c].push(s);
            if let Some(w) = &mut self.writer {
                let res = if self.int24 { w.write_sample((s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32) } else { w.write_sample(s) };
                if let Err(e) = res {
                    log::error!("writing {}: {e}", self.path.display());
                    self.error = Some(format!("Couldn't write the recording {}: {e}", self.path.display()));
                    // Stop writing: dropping the writer finalizes what's in the file.
                    self.writer = None;
                }
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
        if self.checkpoint.elapsed() >= CHECKPOINT {
            return self.checkpoint();
        }
        None
    }

    /// Updates the header to cover what's written so far and hands back the file to sync, so
    /// a crash loses at most the last [`CHECKPOINT`] (and even that is usually still in the
    /// file).
    fn checkpoint(&mut self) -> Option<File> {
        self.checkpoint = Instant::now();
        let w = self.writer.as_mut()?;
        if let Err(e) = w.flush() {
            log::warn!("updating the header of {}: {e}", self.path.display());
            return None;
        }
        self.sync.as_ref()?.try_clone().ok()
    }
}

fn sync(f: File) {
    if let Err(e) = f.sync_data() {
        log::warn!("syncing a recording: {e}");
    }
}

/// A take being recorded, as the crash-recovery journal describes it.
#[derive(Debug, Clone)]
pub struct ActiveTake {
    pub track: Id,
    pub path: PathBuf,
    pub channels: usize,
    /// Engine position of the first frame.
    pub start: i64,
    pub bits: u16,
}

pub struct FinishedTake {
    pub path: PathBuf,
    pub start: i64,
    pub channels: Vec<Vec<f32>>,
    /// Why the file is incomplete or missing, if it is.
    pub error: Option<String>,
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
                let due: Vec<File> = t.lock().values_mut().filter_map(|take| take.drain()).collect();
                due.into_iter().for_each(sync);
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
        let sync = std::fs::OpenOptions::new().write(true).open(&path).ok();
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
            error: None,
            reported: false,
            sync,
            checkpoint: Instant::now(),
            bits: spec.bits_per_sample,
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

    /// The takes being recorded.
    pub fn active(&self) -> Vec<ActiveTake> {
        self.takes
            .lock()
            .iter()
            .map(|(&track, t)| ActiveTake { track, path: t.path.clone(), channels: t.channels, start: t.start, bits: t.bits })
            .collect()
    }

    /// Write errors not reported yet, for the UI.
    pub fn take_errors(&self) -> Option<String> {
        let mut msgs = Vec::new();
        for t in self.takes.lock().values_mut() {
            if let (Some(e), false) = (&t.error, t.reported) {
                msgs.push(e.clone());
                t.reported = true;
            }
        }
        (!msgs.is_empty()).then(|| msgs.join("\n"))
    }

    pub fn finish_all(&self) -> Vec<(Id, FinishedTake)> {
        let mut out = Vec::new();
        let mut finished = Vec::new();
        for (id, mut t) in self.takes.lock().drain() {
            t.drain();
            if let Some(w) = t.writer.take() {
                if let Err(e) = w.finalize() {
                    log::error!("finalizing {}: {e}", t.path.display());
                    t.error.get_or_insert(format!("Couldn't finish the recording {}: {e}", t.path.display()));
                }
            }
            finished.extend(t.sync.take());
            // Already reported while recording: don't say it twice.
            let error = if t.reported { None } else { t.error };
            out.push((id, FinishedTake { path: t.path, start: t.start, channels: t.data, error }));
        }
        // On disk before the autosave that follows refers to them, without holding up Stop.
        if !finished.is_empty() {
            std::thread::spawn(move || finished.into_iter().for_each(sync));
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
