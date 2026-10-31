//! Fermata Sampler on the session's side: its zones and files, recording into it, turning regions
//! into sampler tracks, and instruments saved with their samples.
//!
//! - **Zones** live in the instrument slot's `PluginSlot::state`, so saving, undo, copying and
//!   Freeze carry them. Every edit rewrites the state and sends the instance a new
//!   [`SamplerMap`](map::SamplerMap), built here, through `Cmd::InstrumentData`.
//! - **Files** are copied into the project's `Samples` folder when they arrive and referred to
//!   relative to the project, as recordings are, so a project survives being moved or the original
//!   being deleted. Saving somewhere new copies the folder along.
//! - **Loading** is decoding into the [`SamplePool`] off the audio thread, either right here or,
//!   for many files, from a worker first ([`prepare_files`], called without the session's lock),
//!   so the edit itself finds them ready. A map is complete when it's sent, so an offline render
//!   has nothing to wait for: it hears whichever map the last edit sent.
//! - **Freeing:** what a map replaces comes back as `Garbage::InstrumentData` and stays in
//!   `retired` until the session holds the last reference. Notes ringing on it hold clones, and
//!   dropping those on the audio thread is only ever a decrement.

use std::sync::Arc;

use super::*;
use crate::instrument::sampler::map::{SamplerMap, ZoneIssue};
use crate::instrument::sampler::mapping::{self, NewFile};
use crate::instrument::sampler::sample::{wav_info, SampleBuf, SamplePool};
use crate::instrument::sampler::state::{SamplerState, Zone};
use crate::instrument::sampler::{sfz, ID as SAMPLER};
use crate::instrument::InstrumentData;
use crate::presets::{self, Preset};

/// The project's folder for a sampler's files.
pub const SAMPLES_DIR: &str = "Samples";
/// Slices start on this key (C1, where a drum map starts).
const SLICE_FIRST_KEY: u8 = 36;
/// The key an audio region's sample plays at its own pitch on after converting (C3).
const CONVERT_ROOT: u8 = 60;
/// Below this against the take's loudest moment is silence to trim.
const TRIM_BELOW: f32 = 0.01;
/// What's kept before a trimmed take's first sound and after its last, in seconds.
const TRIM_MARGIN: f64 = 0.005;
/// A take quieter than this all the way through caught nothing.
const SILENT: f32 = 1e-4;

#[derive(Default)]
pub(super) struct SamplerHost {
    pub pool: Arc<SamplePool>,
    /// Maps a voice might still read, until the session holds the last reference.
    retired: Vec<InstrumentData>,
    /// Why zones can't play, by slot.
    issues: HashMap<Id, Vec<ZoneIssue>>,
    recorder: Option<Recorder>,
    take: Option<LiveTake>,
}

struct LiveTake {
    track: Id,
    path: PathBuf,
}

/// A zone as the panel shows it: its settings and what's known about its file.
#[derive(Debug, Clone, Serialize)]
pub struct ZoneView {
    #[serde(flatten)]
    pub zone: Zone,
    pub frames: u64,
    pub rate: u32,
    pub channels: u32,
    /// Why it can't play.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SamplerView {
    pub zones: Vec<ZoneView>,
    /// Decoded audio held for the session, and the limit.
    pub memory: u64,
    pub limit: u64,
    pub recording: Option<SamplerRecording>,
}

/// A recording into a sampler in progress.
#[derive(Debug, Clone, Serialize)]
pub struct SamplerRecording {
    pub track: Id,
    pub frames: u64,
    pub secs: f64,
    /// The loudest since the last look (the panel's level meter). 0 if none since.
    pub peak: f32,
}

/// What adding files did.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SamplerAdded {
    pub zones: Vec<u64>,
    /// Per-file problems: the file was left out, the others came in.
    pub errors: Vec<String>,
}

/// What reading an SFZ file did, stated in full.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SfzReport {
    pub zones: usize,
    /// Opcodes it doesn't do, with how many times each came up.
    pub ignored: Vec<(String, usize)>,
    pub left_out: Vec<String>,
    pub missing: Vec<String>,
}

/// Part of a file's waveform: (min, max) pairs across its channels, one per `bucket` frames.
#[derive(Debug, Clone, Serialize)]
pub struct SamplerWave {
    pub frames: u64,
    pub from: u64,
    pub to: u64,
    pub peaks: Vec<(f32, f32)>,
}

/// A file ready to become a zone: copied into the project and decoded into the pool.
pub struct PreparedFile {
    pub source: PathBuf,
    pub rel: String,
}

/// Copies `path` into the project's `sub` under its Samples folder, unless it's in the project
/// already. A file of the same name and the same bytes is the same file; a different one gets a
/// number. Returns its path relative to the project.
fn adopt_file(dir: &Path, sub: &Path, path: &Path) -> Result<String> {
    if let Ok(rel) = path.strip_prefix(dir) {
        return Ok(rel.to_string_lossy().to_string());
    }
    if !path.is_file() {
        return Err(anyhow!("{} isn't there", path.display()));
    }
    let folder = dir.join(SAMPLES_DIR).join(sub);
    std::fs::create_dir_all(&folder)?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Sample".into());
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for i in 1.. {
        let name = if i == 1 { format!("{stem}{ext}") } else { format!("{stem} {i}{ext}") };
        let dest = folder.join(&name);
        if !dest.exists() {
            std::fs::copy(path, &dest).with_context(|| format!("copying {}", path.display()))?;
            return Ok(dest.strip_prefix(dir).unwrap_or(&dest).to_string_lossy().to_string());
        }
        if same_bytes(path, &dest) {
            return Ok(dest.strip_prefix(dir).unwrap_or(&dest).to_string_lossy().to_string());
        }
    }
    unreachable!()
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    use std::io::Read;
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else { return false };
    if ma.len() != mb.len() {
        return false;
    }
    let (Ok(mut fa), Ok(mut fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else { return false };
    let (mut ba, mut bb) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let Ok(n) = fa.read(&mut ba) else { return false };
        if n == 0 {
            return true;
        }
        if fb.read_exact(&mut bb[..n]).is_err() || ba[..n] != bb[..n] {
            return false;
        }
    }
}

/// Copies files into the project and decodes them into the pool. Takes no lock on the session, so
/// a command can run it on a worker for a large library, and the edit after it finds them ready.
/// Returns each file's result in order.
pub fn prepare_files(pool: &SamplePool, dir: &Path, sub: &Path, paths: &[PathBuf]) -> Vec<Result<PreparedFile, String>> {
    let one = |p: &PathBuf| -> Result<PreparedFile, String> {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let rel = adopt_file(dir, sub, p).map_err(|e| format!("{name}: {e}"))?;
        pool.get(&dir.join(&rel)).map_err(|e| format!("{name}: {}", e.text()))?;
        Ok(PreparedFile { source: p.clone(), rel })
    };
    paths.iter().map(one).collect()
}

/// (min, max) over `[from, to)` of a sample, in `points` steps.
fn wave_of(buf: &SampleBuf, from: u64, to: u64, points: usize) -> SamplerWave {
    let frames = buf.frames as u64;
    let to = if to == 0 { frames } else { to.min(frames) };
    let from = from.min(to);
    let points = points.clamp(1, 8192);
    let span = (to - from) as f64 / points as f64;
    let mut peaks = Vec::with_capacity(points);
    for k in 0..points {
        let a = from + (k as f64 * span) as u64;
        let b = (from + ((k + 1) as f64 * span).ceil() as u64).clamp(a + 1, to.max(a + 1)).min(frames);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for c in 0..buf.ch.len() {
            for &s in &buf.frames_of(c)[(a as usize).min(buf.frames)..b as usize] {
                lo = lo.min(s);
                hi = hi.max(s);
            }
        }
        peaks.push(if lo <= hi { (lo, hi) } else { (0.0, 0.0) });
    }
    SamplerWave { frames, from, to, peaks }
}

/// Where a take's sound starts and ends: its first and last frames above the silence, with a
/// moment around them. Non-destructive: the zone's start and end move, the file stays whole.
fn trim_points(channels: &[Vec<f32>], rate: u32) -> (u64, u64) {
    let frames = channels.first().map(|c| c.len()).unwrap_or(0);
    let level = |i: usize| channels.iter().fold(0.0f32, |m, c| m.max(c[i].abs()));
    let peak = (0..frames).fold(0.0f32, |m, i| m.max(level(i)));
    let floor = (peak * TRIM_BELOW).max(SILENT);
    let Some(first) = (0..frames).find(|&i| level(i) >= floor) else { return (0, frames as u64) };
    let last = (0..frames).rev().find(|&i| level(i) >= floor).unwrap_or(first);
    let margin = (TRIM_MARGIN * rate as f64) as usize;
    (first.saturating_sub(margin) as u64, (last + margin + 1).min(frames) as u64)
}

impl Session {
    /// The state of a track's sampler, by its slot in the document.
    fn sampler_slot(&self, track: Id) -> Result<&PluginSlot> {
        self.project.track(track).and_then(|t| t.instrument.as_ref()).filter(|s| s.plugin_id == SAMPLER).ok_or_else(|| anyhow!("That track has no Fermata Sampler"))
    }

    /// The pool, for a command that fills it from a worker.
    pub fn sampler_pool(&self) -> Arc<SamplePool> {
        self.sampler.pool.clone()
    }

    /// The project's folder as it stands.
    pub fn sampler_dir(&self) -> PathBuf {
        self.dir.clone()
    }

    /// Builds what a slot's instance plays from its state, noting anything that can't load.
    pub(super) fn sampler_data(&mut self, slot: &PluginSlot) -> InstrumentData {
        let state = SamplerState::parse(slot.state.as_deref());
        let (map, issues) = SamplerMap::build(&state, &self.dir, self.sample_rate() as f32, &self.sampler.pool);
        if issues.is_empty() {
            self.sampler.issues.remove(&slot.uid);
        } else {
            self.sampler.issues.insert(slot.uid, issues);
        }
        Arc::new(map)
    }

    /// Something the engine sent back. A map might still be sounding, so it waits.
    pub(super) fn sampler_garbage(&mut self, g: Garbage) {
        match g {
            Garbage::InstrumentData(d) => self.sampler.retired.push(d),
            other => drop(other),
        }
    }

    /// Frees maps no voice reads any more, here on the session's thread.
    pub(super) fn sampler_idle(&mut self) {
        self.sampler.retired.retain(|d| Arc::strong_count(d) > 1);
    }

    /// How many replaced maps are still waiting to be freed, for checks.
    pub fn sampler_retired(&self) -> usize {
        self.sampler.retired.len()
    }

    /// Writes a slot's zones back and sends its instance the result.
    fn sampler_store(&mut self, track: Id, state: &SamplerState) -> Result<()> {
        self.check_unfrozen(&[track])?;
        let t = self.project.track_mut(track).ok_or_else(|| anyhow!("no such track"))?;
        let s = t.instrument.as_mut().filter(|s| s.plugin_id == SAMPLER).ok_or_else(|| anyhow!("That track has no Fermata Sampler"))?;
        s.state = Some(state.to_json());
        let slot = s.clone();
        let data = self.sampler_data(&slot);
        self.send(Cmd::InstrumentData { track, uid: slot.uid, data });
        self.dirty = true;
        Ok(())
    }

    fn sampler_state(&self, track: Id) -> Result<SamplerState> {
        Ok(SamplerState::parse(self.sampler_slot(track)?.state.as_deref()))
    }

    /// The zones, with what's known about their files, for the panel.
    pub fn sampler_view(&self, track: Id) -> Result<SamplerView> {
        let slot = self.sampler_slot(track)?;
        let state = SamplerState::parse(slot.state.as_deref());
        let issues = self.sampler.issues.get(&slot.uid);
        let pool = &self.sampler.pool;
        let zones = state
            .zones
            .into_iter()
            .map(|zone| {
                let mut error = issues.and_then(|i| i.iter().find(|x| x.zone == zone.id)).map(|x| x.error.text());
                // Gone since it loaded: it plays on from memory, but won't next time.
                let buf = if error.is_some() { None } else { pool.get(&self.dir.join(&zone.file)).map_err(|e| error = Some(e.text())).ok() };
                ZoneView { frames: buf.as_ref().map_or(0, |b| b.frames as u64), rate: buf.as_ref().map_or(0, |b| b.rate), channels: buf.as_ref().map_or(0, |b| b.ch.len() as u32), error, zone }
            })
            .collect();
        Ok(SamplerView { zones, memory: pool.bytes() as u64, limit: pool.limit() as u64, recording: self.sampler_recording() })
    }

    /// Part of a zone's file, for the waveform view.
    pub fn sampler_waveform(&self, track: Id, zone: u64, from: u64, to: u64, points: usize) -> Result<SamplerWave> {
        let state = self.sampler_state(track)?;
        let z = state.zones.iter().find(|z| z.id == zone).ok_or_else(|| anyhow!("no such zone"))?;
        let buf = self.sampler.pool.get(&self.dir.join(&z.file)).map_err(|e| anyhow!("{}: {}", z.name, e.text()))?;
        Ok(wave_of(&buf, from, to, points))
    }

    /// Replaces the zones with these ids by these versions of them. New ones can't come this way.
    pub fn sampler_update_zones(&mut self, track: Id, zones: Vec<Zone>) -> Result<()> {
        let mut state = self.sampler_state(track)?;
        for mut z in zones {
            z.tidy();
            if let Some(at) = state.zones.iter_mut().find(|x| x.id == z.id) {
                *at = z;
            }
        }
        self.sampler_store(track, &state)
    }

    pub fn sampler_delete_zones(&mut self, track: Id, ids: &[u64]) -> Result<()> {
        let mut state = self.sampler_state(track)?;
        state.zones.retain(|z| !ids.contains(&z.id));
        self.sampler_store(track, &state)
    }

    /// Removes every zone.
    pub fn sampler_clear(&mut self, track: Id) -> Result<()> {
        let mut state = self.sampler_state(track)?;
        state.zones.clear();
        self.sampler_store(track, &state)
    }

    /// Adds files: copies them into the project, decodes them and maps them to keys (see
    /// [`mapping::map_files`]). A file that won't load is reported and the rest come in.
    pub fn sampler_add_files(&mut self, track: Id, paths: &[PathBuf], at_key: Option<u8>) -> Result<SamplerAdded> {
        self.sampler_slot(track)?;
        self.check_unfrozen(&[track])?;
        let prepared = prepare_files(&self.sampler.pool, &self.dir, Path::new(""), paths);
        self.sampler_add_prepared(track, prepared, at_key)
    }

    /// Finishes [`Session::sampler_add_files`] for files [`prepare_files`] has done.
    pub fn sampler_add_prepared(&mut self, track: Id, prepared: Vec<Result<PreparedFile, String>>, at_key: Option<u8>) -> Result<SamplerAdded> {
        let mut state = self.sampler_state(track)?;
        let mut added = SamplerAdded::default();
        let mut files = Vec::new();
        for p in prepared {
            match p {
                Ok(p) => {
                    let name = p.source.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Sample".into());
                    let mut zone = Zone::new(state.alloc_id(), name.clone(), p.rel.clone(), 60);
                    let info = wav_info(&self.dir.join(&p.rel));
                    zone.fine = info.fine_cents;
                    if let Some((a, b)) = info.loop_points {
                        zone.loop_on = true;
                        zone.loop_start = a;
                        zone.loop_end = b;
                        zone.xfade = 64;
                    }
                    files.push(NewFile { zone, info_root: info.root.filter(|r| *r != 60 || info.loop_points.is_some()), name: mapping::parse_name(&name) });
                }
                Err(e) => added.errors.push(e),
            }
        }
        let mut next_group = state.zones.iter().map(|z| z.rr_group).max().unwrap_or(0);
        let zones = mapping::map_files(files, at_key, &mut next_group);
        if zones.is_empty() {
            if added.errors.is_empty() {
                return Err(anyhow!("No audio files"));
            }
            return Err(anyhow!(added.errors.join("\n")));
        }
        added.zones = zones.iter().map(|z| z.id).collect();
        state.zones.extend(zones);
        self.sampler_store(track, &state)?;
        Ok(added)
    }

    /// Reads an SFZ file, replacing the zones, with its samples copied into the project's own
    /// folder for it. What it couldn't do is listed in the report.
    pub fn sampler_load_sfz(&mut self, track: Id, path: &Path) -> Result<SfzReport> {
        self.sampler_slot(track)?;
        self.check_unfrozen(&[track])?;
        let file = sfz::read(path)?;
        let sub = path.file_stem().map(PathBuf::from).unwrap_or_default();
        let paths: Vec<PathBuf> = file.regions.iter().map(|r| r.path.clone()).collect();
        let prepared = prepare_files(&self.sampler.pool, &self.dir, &sub, &paths);
        self.sampler_finish_sfz(track, file, prepared)
    }

    /// Finishes [`Session::sampler_load_sfz`] for a file a worker has read and prepared.
    pub fn sampler_finish_sfz(&mut self, track: Id, file: sfz::SfzFile, prepared: Vec<Result<PreparedFile, String>>) -> Result<SfzReport> {
        let mut state = SamplerState::parse(None);
        let mut report = SfzReport { ignored: file.ignored.into_iter().collect(), left_out: file.left_out, missing: file.missing, zones: 0 };
        for (region, p) in file.regions.into_iter().zip(prepared) {
            match p {
                Ok(p) => {
                    let mut z = region.zone;
                    z.id = state.alloc_id();
                    z.file = p.rel;
                    z.tidy();
                    state.zones.push(z);
                }
                Err(e) => report.left_out.push(e),
            }
        }
        report.zones = state.zones.len();
        if state.zones.is_empty() {
            return Err(anyhow!("Nothing in that SFZ file could be loaded"));
        }
        self.sampler_store(track, &state)?;
        Ok(report)
    }

    /// Cuts a zone at its transients, one slice per key from C1 up. Returns how many there are.
    /// `sensitivity` 0..1: higher finds softer hits.
    pub fn sampler_slice(&mut self, track: Id, zone: u64, sensitivity: f32) -> Result<usize> {
        let mut state = self.sampler_state(track)?;
        let at = state.zones.iter().position(|z| z.id == zone).ok_or_else(|| anyhow!("no such zone"))?;
        let z = state.zones[at].clone();
        let buf = self.sampler.pool.get(&self.dir.join(&z.file)).map_err(|e| anyhow!("{}: {}", z.name, e.text()))?;
        let new = self.slice_zones(&mut state, &z, &buf, sensitivity);
        let n = new.len();
        state.zones.splice(at..=at, new);
        self.sampler_store(track, &state)?;
        Ok(n)
    }

    fn slice_zones(&self, state: &mut SamplerState, z: &Zone, buf: &SampleBuf, sensitivity: f32) -> Vec<Zone> {
        let end = if z.end == 0 { buf.frames as u64 } else { z.end.min(buf.frames as u64) };
        let start = z.start.min(end);
        let part: Vec<Vec<f32>> = (0..buf.ch.len()).map(|c| buf.frames_of(c)[start as usize..end as usize].to_vec()).collect();
        let onsets = crate::transients::detect(&part, buf.rate, sensitivity);
        let min_gap = (buf.rate as f64 * 0.02) as u64;
        // Silence before the first hit isn't a slice of its own.
        let lead_in = onsets.first().map_or(0, |&o| o.max(0) as usize);
        let quiet = part.iter().all(|c| c[..lead_in].iter().all(|v| v.abs() < 0.003));
        let (first, onsets) = if quiet { (start + lead_in as u64, onsets.iter().map(|o| o - lead_in as i64).collect()) } else { (start, onsets) };
        let points = mapping::slices(&onsets, first, end, min_gap);
        let room = 128 - SLICE_FIRST_KEY as usize;
        points
            .into_iter()
            .take(room)
            .enumerate()
            .map(|(i, (a, b))| {
                let key = SLICE_FIRST_KEY + i as u8;
                let mut s = z.clone();
                s.id = state.alloc_id();
                s.name = format!("{} {}", z.name, i + 1);
                s.key_lo = key;
                s.key_hi = key;
                s.root = key;
                s.start = a;
                s.end = b;
                s.loop_on = false;
                s.one_shot = true;
                s
            })
            .collect()
    }

    // ---- recording ---------------------------------------------------------------------------

    /// Starts recording `input` into the sampler on `track`, through the recorder and the input
    /// every audio track uses, whatever the transport is doing.
    pub fn sampler_record_start(&mut self, track: Id, input: InputSel) -> Result<()> {
        self.sampler_slot(track)?;
        self.check_unfrozen(&[track])?;
        if self.sampler.take.is_some() {
            return Err(anyhow!("A sampler is already recording"));
        }
        if self.rec_punch.is_some() {
            return Err(anyhow!("Stop the song's recording first"));
        }
        if input.channels() == 0 {
            return Err(anyhow!("Choose an input to record from"));
        }
        let sr = self.sample_rate() as u32;
        let depth = self.project.bit_depth.unwrap_or(self.prefs.recording.bit_depth);
        let dir = self.dir.join(SAMPLES_DIR);
        let path = (1..).map(|i| dir.join(format!("Recording {i}.wav"))).find(|p| !p.exists()).unwrap_or_else(|| dir.join("Recording.wav"));
        let rec = self.sampler.recorder.get_or_insert_with(Recorder::new).clone();
        let sink = rec.begin(track, path.clone(), input.channels(), sr, 0, depth)?;
        self.send(Cmd::InputTap { input, sink: Some(sink) });
        self.sampler.take = Some(LiveTake { track, path });
        Ok(())
    }

    /// How the recording's going, for the panel.
    pub fn sampler_recording(&self) -> Option<SamplerRecording> {
        let take = self.sampler.take.as_ref()?;
        let rec = self.sampler.recorder.as_ref()?;
        let p = rec.previews().into_iter().find(|t| t.track == take.track)?;
        let peak = p.peaks.iter().fold(0.0f32, |m, v| m.max(*v));
        Some(SamplerRecording { track: take.track, frames: p.frames as u64, secs: p.frames as f64 / self.sample_rate(), peak })
    }

    /// Whether a sampler recording is running, for the caller's undo step.
    pub fn sampler_is_recording(&self) -> bool {
        self.sampler.take.is_some()
    }

    fn sampler_stop_take(&mut self) -> Option<(LiveTake, Option<FinishedTake>)> {
        let take = self.sampler.take.take()?;
        self.send(Cmd::InputTap { input: InputSel::None, sink: None });
        self.wait_for_engine();
        let finished = self.sampler.recorder.as_ref().map(|r| r.finish_all()).and_then(|mut f| f.pop()).map(|f| f.1);
        Some((take, finished))
    }

    /// Stops and throws the take away.
    pub fn sampler_record_cancel(&mut self) {
        if let Some((take, _)) = self.sampler_stop_take() {
            let _ = std::fs::remove_file(&take.path);
        }
    }

    /// Stops and makes the take playable: a zone over the whole keyboard (or just `at_key`) with
    /// its pitch at C3. `trim` moves its start and end in past the silence. Returns the zone.
    pub fn sampler_record_stop(&mut self, trim: bool, at_key: Option<u8>) -> Result<u64> {
        let (take, finished) = self.sampler_stop_take().ok_or_else(|| anyhow!("Nothing is recording"))?;
        let finished = finished.ok_or_else(|| anyhow!("The recording was lost"))?;
        if let Some(e) = finished.error {
            self.warning = Some(e);
        }
        let channels = finished.channels;
        let frames = channels.first().map_or(0, |c| c.len());
        let peak = channels.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()));
        if frames == 0 || peak < SILENT {
            let _ = std::fs::remove_file(&take.path);
            return Err(anyhow!(if frames == 0 { "Nothing came in from the input. Check Settings › Audio." } else { "The input was silent, so nothing was kept. Check the input and its level." }));
        }
        let rate = self.sample_rate() as u32;
        self.sampler.pool.insert(&take.path, rate, &channels).map_err(|e| anyhow!("The recording is {}", e.text()))?;
        let mut state = self.sampler_state(take.track)?;
        let rel = take.path.strip_prefix(&self.dir).unwrap_or(&take.path).to_string_lossy().to_string();
        let name = take.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Recording".into());
        let mut z = Zone::new(state.alloc_id(), name, rel, CONVERT_ROOT);
        if let Some(k) = at_key {
            z.key_lo = k;
            z.key_hi = k;
            z.root = k;
        }
        if trim {
            let (a, b) = trim_points(&channels, rate);
            z.start = a;
            z.end = b;
        }
        let id = z.id;
        state.zones.push(z);
        self.sampler_store(take.track, &state)?;
        Ok(id)
    }

    // ---- converting regions ------------------------------------------------------------------

    /// Logic's Convert to Sampler Track: makes a new instrument track below with a sampler playing
    /// this audio region, and a MIDI region in its place, and mutes the original. With
    /// `slices` (a sensitivity, 0..1) the region is cut at its transients, one key each, and the
    /// MIDI plays them back in time; otherwise one note plays the whole region at its own pitch.
    pub fn convert_region_to_sampler(&mut self, region: Id, slices: Option<f32>) -> Result<Id> {
        let (track, idx) = self.find_region(region).ok_or_else(|| anyhow!("no such region"))?;
        self.check_unfrozen(&[track])?;
        let r = self.project.track(track).unwrap().regions[idx].clone();
        let RegionContent::Audio { clip, offset, gain_db, .. } = r.content else { return Err(anyhow!("Only audio regions convert to a sampler")) };
        let c = self.clips.get(&clip).cloned().ok_or_else(|| anyhow!("{}'s audio file is missing", r.name))?;
        // Normally a copy of its file; one that's gone comes from what's in memory.
        let rel = if c.path.is_file() {
            adopt_file(&self.dir, Path::new(""), &c.path)?
        } else {
            let dest = self.dir.join(SAMPLES_DIR).join(format!("{}.wav", r.name.replace(['/', '\\'], " ")));
            crate::decode::write_wav(&dest, c.sample_rate, &c.channels)?;
            dest.strip_prefix(&self.dir).unwrap_or(&dest).to_string_lossy().to_string()
        };
        let buf = self.sampler.pool.get(&self.dir.join(&rel)).map_err(|e| anyhow!("{}: {}", r.name, e.text()))?;
        let at = self.project.tracks.iter().position(|t| t.id == track).map_or(self.project.tracks.len(), |i| i + 1);
        let new = self.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.to_string()), Some(at))?;
        if let Some(t) = self.project.track_mut(new) {
            t.name = format!("{} Sampler", r.name);
        }
        let mut state = SamplerState::parse(None);
        let end = (offset + r.length).max(offset + 1) as u64;
        let mut z = Zone::new(state.alloc_id(), r.name.clone(), rel, CONVERT_ROOT);
        z.start = offset.max(0) as u64;
        z.end = end;
        z.gain_db = gain_db;
        let velocity = 127;
        let mut notes = Vec::new();
        if let Some(sensitivity) = slices {
            let zones = self.slice_zones(&mut state, &z, &buf, sensitivity);
            for s in &zones {
                let note_start = s.start as i64 - offset;
                notes.push(Note { start: note_start, length: (s.end - s.start) as i64, pitch: s.root, velocity, muted: false });
            }
            state.zones = zones;
        } else {
            notes.push(Note { start: 0, length: r.length, pitch: CONVERT_ROOT, velocity, muted: false });
            state.zones.push(z);
        }
        self.sampler_store(new, &state)?;
        let midi = self.create_midi_region(new, r.start, r.length)?;
        self.set_midi_content(vec![midi_content::MidiContent { region: midi, notes, cc: Vec::new(), start: None, length: None }]);
        if let Some(t) = self.project.track_mut(track) {
            t.regions[idx].muted = true;
        }
        self.push_regions(track);
        self.dirty = true;
        Ok(new)
    }

    // ---- files that move ---------------------------------------------------------------------

    /// Saving somewhere new: the Samples folder goes along whole, not just what's in use, because
    /// undo steps might refer to the rest.
    pub(super) fn sampler_copy_samples(&self, target: &Path) -> Result<()> {
        fn copy_dir(from: &Path, to: &Path) -> Result<()> {
            let Ok(rd) = std::fs::read_dir(from) else { return Ok(()) };
            std::fs::create_dir_all(to)?;
            for e in rd.flatten() {
                let (src, dest) = (e.path(), to.join(e.file_name()));
                if src.is_dir() {
                    copy_dir(&src, &dest)?;
                } else if !dest.exists() {
                    std::fs::copy(&src, &dest).with_context(|| format!("copying {}", src.display()))?;
                }
            }
            Ok(())
        }
        copy_dir(&self.dir.join(SAMPLES_DIR), &target.join(SAMPLES_DIR))
    }

    /// After opening: says plainly which samples aren't there. The rest play.
    pub(super) fn sampler_warn_missing(&mut self) {
        let mut names: Vec<String> = Vec::new();
        for t in &self.project.tracks {
            let Some(slot) = t.instrument.as_ref().filter(|s| s.plugin_id == SAMPLER) else { continue };
            for i in self.sampler.issues.get(&slot.uid).into_iter().flatten() {
                names.push(format!("{} on {} ({})", i.file, t.name, i.error.text()));
            }
        }
        if !names.is_empty() {
            let n = names.len();
            names.truncate(5);
            let more = if n > 5 { format!(" and {} more", n - 5) } else { String::new() };
            let text = format!("Samples that can't play: {}{more}. Everything else plays. Replace them in the sampler's panel.", names.join(", "));
            self.warning = Some(match self.warning.take() {
                Some(w) => format!("{w}\n{text}"),
                None => text,
            });
        }
    }

    /// Every zone that can't play and why, across the project.
    pub fn sampler_issues(&self, track: Id) -> Vec<(u64, String, String)> {
        let Ok(slot) = self.sampler_slot(track) else { return Vec::new() };
        self.sampler.issues.get(&slot.uid).into_iter().flatten().map(|i| (i.zone, i.file.clone(), i.error.text())).collect()
    }

    /// Points a zone at another file, copied into the project: mending a missing sample.
    pub fn sampler_replace_file(&mut self, track: Id, zone: u64, path: &Path) -> Result<()> {
        let mut state = self.sampler_state(track)?;
        let rel = adopt_file(&self.dir, Path::new(""), path)?;
        self.sampler.pool.get(&self.dir.join(&rel)).map_err(|e| anyhow!("{}", e.text()))?;
        let z = state.zones.iter_mut().find(|z| z.id == zone).ok_or_else(|| anyhow!("no such zone"))?;
        z.file = rel;
        // Its positions were for the other file.
        z.start = 0;
        z.end = 0;
        z.loop_on = false;
        z.loop_start = 0;
        z.loop_end = 0;
        self.sampler_store(track, &state)
    }

    // ---- instruments saved with their samples ------------------------------------------------

    /// Saves the sampler on `track` as a folder under the user's Music folder (`presets::bundle_root`,
    /// created on first save), with a copy of every sample it plays, so any project can load it.
    /// It shows in the instrument's preset menu. Returns the name it was saved under.
    pub(super) fn sampler_save_instrument(&self, track: Id, name: &str, overwrite: bool) -> Result<String> {
        let name = presets::clean_name(name)?;
        let slot = self.sampler_slot(track)?;
        let dest = presets::bundle_dir(&name);
        if dest.exists() && !overwrite {
            return Err(anyhow!("A preset named \"{name}\" already exists"));
        }
        let mut state = SamplerState::parse(slot.state.as_deref());
        // Built beside it, so a failure halfway leaves the old one as it was.
        let root = presets::bundle_root();
        let tmp = root.join(format!(".saving {name}"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(SAMPLES_DIR))?;
        let mut done: HashMap<String, String> = HashMap::new();
        for z in &mut state.zones {
            if let Some(rel) = done.get(&z.file) {
                z.file = rel.clone();
                continue;
            }
            let src = self.dir.join(&z.file);
            let was = std::mem::take(&mut z.file);
            // A missing sample stays listed in the instrument, and shows as missing when loaded.
            let rel = if src.is_file() { adopt_file(&tmp, Path::new(""), &src)? } else { was.clone() };
            done.insert(was, rel.clone());
            z.file = rel;
        }
        let p = Preset { name: name.clone(), effect: SAMPLER.into(), version: 1, values: self.builtin_params(slot.uid), eq: None, state: Some(state.to_json()), base: None };
        std::fs::write(tmp.join(presets::BUNDLE_DOC), serde_json::to_string_pretty(&p)?)?;
        let old = root.join(format!(".replaced {name}"));
        let _ = std::fs::remove_dir_all(&old);
        if dest.exists() {
            std::fs::rename(&dest, &old)?;
        }
        std::fs::rename(&tmp, &dest)?;
        let _ = std::fs::remove_dir_all(&old);
        Ok(name)
    }

    /// Takes a preset's zones, with their files copied into this project.
    pub(super) fn sampler_apply_preset(&mut self, track: Id, state: &str, base: Option<&Path>) -> Result<()> {
        self.sampler_slot(track)?;
        let mut state = SamplerState::parse(Some(state));
        if let Some(base) = base {
            let sub = base.file_name().map(PathBuf::from).unwrap_or_default();
            let mut done: HashMap<String, String> = HashMap::new();
            for z in &mut state.zones {
                if let Some(rel) = done.get(&z.file) {
                    z.file = rel.clone();
                    continue;
                }
                let src = base.join(&z.file);
                let was = std::mem::take(&mut z.file);
                let rel = if src.is_file() { adopt_file(&self.dir, &sub, &src)? } else { was.clone() };
                done.insert(was, rel.clone());
                z.file = rel;
            }
        }
        self.sampler_store(track, &state)
    }

    /// Lets a check push input through the engine, as an audio device would, with `channels`
    /// interleaved. The null device has no input of its own.
    pub fn feed_input_for_checks(&self, input: &[f32], channels: usize) {
        let frames = input.len() / channels.max(1);
        let mut out = vec![0.0f32; frames * 2];
        self.engine.lock().process(input, channels, &mut out, 2);
    }
}
