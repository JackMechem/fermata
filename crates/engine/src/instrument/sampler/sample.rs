//! Audio held in memory for the sampler. Everything here runs off the audio thread: the voices
//! only read what it built, through `super::map::SamplerMap`.
//!
//! **Memory:** samples are decoded whole, 4 bytes per frame and channel at the file's own rate
//! (no disk streaming). [`MEMORY_LIMIT`] caps the pool. Files nothing uses go first, and past that
//! a file refuses to load and its zone reports why, while the rest keep playing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::SystemTime;

use parking_lot::Mutex;

use super::kernel::GUARD;

/// How much decoded audio a session holds: 4 GiB, about 6 hours of 48 kHz stereo.
pub const MEMORY_LIMIT: usize = 4 << 30;

/// One file's audio, at its own rate, with silence around it for the kernels.
pub struct SampleBuf {
    pub rate: u32,
    pub frames: usize,
    /// One or two channels, each `GUARD + frames + GUARD` long.
    pub ch: Vec<Box<[f32]>>,
}

fn guarded(c: &[f32]) -> Box<[f32]> {
    let mut v = vec![0.0f32; GUARD * 2 + c.len()];
    v[GUARD..GUARD + c.len()].copy_from_slice(c);
    // A float file can hold anything. Past +60 dB it's damage, not audio.
    for s in &mut v[GUARD..GUARD + c.len()] {
        if s.is_nan() || s.abs() > 1000.0 {
            *s = 0.0;
        }
    }
    v.into_boxed_slice()
}

impl SampleBuf {
    pub fn new(rate: u32, channels: &[Vec<f32>]) -> SampleBuf {
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        SampleBuf { rate, frames, ch: channels.iter().take(2).map(|c| guarded(&c[..frames])).collect() }
    }

    /// The same audio backwards.
    pub fn reversed(&self) -> SampleBuf {
        SampleBuf {
            rate: self.rate,
            frames: self.frames,
            ch: self
                .ch
                .iter()
                .map(|c| {
                    let mut v = c.to_vec();
                    v.reverse();
                    v.into_boxed_slice()
                })
                .collect(),
        }
    }

    /// A channel's frames, without the guards.
    pub fn frames_of(&self, c: usize) -> &[f32] {
        &self.ch[c][GUARD..GUARD + self.frames]
    }

    pub fn bytes(&self) -> usize {
        self.ch.iter().map(|c| c.len() * 4).sum()
    }
}

/// Why a zone has nothing to play.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadError {
    Missing,
    Unreadable(String),
    OverMemoryLimit,
}

impl LoadError {
    pub fn text(&self) -> String {
        match self {
            LoadError::Missing => "the file is missing".into(),
            LoadError::Unreadable(e) => format!("the file can't be read: {e}"),
            LoadError::OverMemoryLimit => format!("over the sampler's memory limit of {} GB", MEMORY_LIMIT >> 30),
        }
    }
}

struct Entry {
    buf: Arc<SampleBuf>,
    reversed: Option<Arc<SampleBuf>>,
    stamp: Option<(SystemTime, u64)>,
}

/// What a baked loop depends on.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct LoopKey {
    pub path: PathBuf,
    pub reverse: bool,
    pub start: u64,
    pub end: u64,
    pub xfade: u64,
    pub equal_power: bool,
}

#[derive(Default)]
struct Inner {
    files: HashMap<PathBuf, Entry>,
    loops: HashMap<LoopKey, Weak<super::map::LoopBuf>>,
}

/// Decoded files, shared by every sampler in a session. Thread-safe, so a command can fill it
/// from a worker before the edit that uses it runs (`Session::sampler_prepare_files`).
pub struct SamplePool {
    inner: Mutex<Inner>,
    limit: usize,
}

impl Default for SamplePool {
    fn default() -> Self {
        SamplePool::with_limit(MEMORY_LIMIT)
    }
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

impl SamplePool {
    pub fn with_limit(limit: usize) -> SamplePool {
        SamplePool { inner: Mutex::default(), limit }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// How much decoded audio is held now, in bytes.
    pub fn bytes(&self) -> usize {
        self.inner.lock().files.values().map(|e| e.buf.bytes() + e.reversed.as_ref().map_or(0, |r| r.bytes())).sum()
    }

    /// Lets go of files nothing plays, so opening another project gives their memory back.
    pub fn forget_unused(&self) {
        let mut g = self.inner.lock();
        g.files.retain(|_, e| Arc::strong_count(&e.buf) > 1 || e.reversed.as_ref().is_some_and(|r| Arc::strong_count(r) > 1));
        g.loops.retain(|_, w| w.strong_count() > 0);
    }

    /// Makes room for `want` more bytes, dropping unused files; false if it can't.
    fn room_for(&self, want: usize) -> bool {
        if self.bytes() + want <= self.limit {
            return true;
        }
        self.forget_unused();
        self.bytes() + want <= self.limit
    }

    /// A file's audio, decoded now unless it is already held.
    pub fn get(&self, path: &Path) -> Result<Arc<SampleBuf>, LoadError> {
        let now = stamp(path);
        if let Some(e) = self.inner.lock().files.get(path) {
            if e.stamp == now || now.is_none() {
                if now.is_none() {
                    return Err(LoadError::Missing);
                }
                return Ok(e.buf.clone());
            }
        }
        if now.is_none() {
            return Err(LoadError::Missing);
        }
        // Don't start decoding what plainly can't fit: PCM decodes to at least its file's size,
        // as far as 16-bit audio is concerned twice that.
        if now.is_some_and(|(_, len)| len as usize > self.limit) {
            return Err(LoadError::OverMemoryLimit);
        }
        let d = crate::decode::decode(path).map_err(|e| LoadError::Unreadable(e.to_string()))?;
        Self::check_decoded(&d, path)?;
        let buf = Arc::new(SampleBuf::new(d.sample_rate, &d.channels));
        drop(d);
        if !self.room_for(buf.bytes()) {
            return Err(LoadError::OverMemoryLimit);
        }
        self.inner.lock().files.insert(path.to_path_buf(), Entry { buf: buf.clone(), reversed: None, stamp: now });
        Ok(buf)
    }

    fn check_decoded(d: &crate::decode::Decoded, path: &Path) -> Result<(), LoadError> {
        if d.sample_rate == 0 || d.channels.is_empty() || d.channels[0].is_empty() {
            return Err(LoadError::Unreadable(format!("{} has no audio", path.display())));
        }
        Ok(())
    }

    /// Puts audio that is already in memory (a recording just made) in the pool.
    pub fn insert(&self, path: &Path, rate: u32, channels: &[Vec<f32>]) -> Result<Arc<SampleBuf>, LoadError> {
        let buf = Arc::new(SampleBuf::new(rate, channels));
        if !self.room_for(buf.bytes()) {
            return Err(LoadError::OverMemoryLimit);
        }
        self.inner.lock().files.insert(path.to_path_buf(), Entry { buf: buf.clone(), reversed: None, stamp: stamp(path) });
        Ok(buf)
    }

    /// The file's audio backwards, made the first time a zone asks for it.
    pub fn get_reversed(&self, path: &Path) -> Result<Arc<SampleBuf>, LoadError> {
        let buf = self.get(path)?;
        if let Some(r) = self.inner.lock().files.get(path).and_then(|e| e.reversed.clone()) {
            return Ok(r);
        }
        let r = Arc::new(buf.reversed());
        if !self.room_for(r.bytes()) {
            return Err(LoadError::OverMemoryLimit);
        }
        if let Some(e) = self.inner.lock().files.get_mut(path) {
            e.reversed = Some(r.clone());
        }
        Ok(r)
    }

    /// A baked loop from the cache, or made with `make` and remembered, so moving one zone
    /// doesn't bake every other zone's loop again.
    pub fn baked_loop(&self, key: LoopKey, make: impl FnOnce() -> super::map::LoopBuf) -> Arc<super::map::LoopBuf> {
        if let Some(l) = self.inner.lock().loops.get(&key).and_then(Weak::upgrade) {
            return l;
        }
        let l = Arc::new(make());
        self.inner.lock().loops.insert(key, Arc::downgrade(&l));
        l
    }
}

/// What a WAV file's `smpl` chunk says: the note it was recorded at and its first loop (start,
/// one past the end).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WavInfo {
    pub root: Option<u8>,
    pub fine_cents: f32,
    pub loop_points: Option<(u64, u64)>,
}

/// Reads a WAV's `smpl` chunk, if it has one. Any other file, or a damaged one, gives none.
pub fn wav_info(path: &Path) -> WavInfo {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return WavInfo::default() };
    let mut head = [0u8; 12];
    if f.read_exact(&mut head).is_err() || &head[..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return WavInfo::default();
    }
    let mut chunk = [0u8; 8];
    while f.read_exact(&mut chunk).is_ok() {
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as u64;
        if &chunk[..4] == b"smpl" && size >= 36 {
            let mut body = vec![0u8; size.min(4096) as usize];
            if f.read_exact(&mut body).is_err() {
                return WavInfo::default();
            }
            let at = |i: usize| body.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0);
            let unity = at(12);
            let fraction = at(16);
            let loops = at(28);
            // The first loop's record: id, type, start, end (inclusive), fraction, count.
            let loop_points = (loops > 0 && body.len() >= 60).then(|| (at(44) as u64, at(48) as u64 + 1));
            return WavInfo { root: (unity <= 127).then_some(unity as u8), fine_cents: (fraction as f64 / 4294967296.0 * 100.0) as f32, loop_points };
        }
        // Chunks are word-aligned.
        if f.seek(SeekFrom::Current((size + (size & 1)) as i64)).is_err() {
            break;
        }
    }
    WavInfo::default()
}
