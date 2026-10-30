//! Crash safety for project files and recordings.
//!
//! - Project files are written atomically (temporary file, sync, rename), and saving keeps the
//!   previous project.json as project.json.bak. Opening falls back from a missing or damaged
//!   project.json to the autosave, then to the backup, and says so.
//! - Recordings: when recording starts, a small journal (`recording-<ms>.journal.json`) goes
//!   into the project folder, saying which files are being recorded for which tracks, from
//!   where, and how the recording is cut into takes. The recorder keeps those files valid as
//!   it goes (`recorder.rs`). After a crash, the journal is what lets the next launch repair
//!   the files and put the takes back where they were recorded. A clean stop hands the journal
//!   to the autosave that follows right away; once the takes are in a file on disk (that
//!   autosave, or a save), the journal is deleted.
//! - The autosave runs as soon as a recording, Bounce in Place, Freeze or import finishes,
//!   rather than waiting for the interval (Settings › General).

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::{Session, AUDIO_DIR, AUTOSAVE_FILE, PROJECT_FILE};
use crate::model::{Id, InputSel, PluginSlot, Project, TrackKind};
use crate::recorder::FinishedTake;
use crate::takes::RecPlan;

/// The previous project.json, kept by every save.
pub const BACKUP_FILE: &str = "project.json.bak";
const JOURNAL_PREFIX: &str = "recording-";
const JOURNAL_SUFFIX: &str = ".journal.json";

/// Writes `bytes` to `path` so that a crash leaves either the old file or the new one, never
/// a mix: into a temporary file next to it, then renamed over it. `durable` also syncs the
/// data (and on Unix the folder) so the new file survives a power cut.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8], durable: bool) -> Result<()> {
    let dir = path.parent().ok_or_else(|| anyhow!("no folder for {}", path.display()))?;
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        if durable {
            f.sync_all()?;
        }
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    #[cfg(unix)]
    if durable {
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

/// Saves project.json in `dir`, first keeping the one there as project.json.bak (when it is
/// readable: a damaged file never replaces a good backup).
pub(super) fn write_project_file(dir: &Path, bytes: &[u8]) -> Result<()> {
    let path = dir.join(PROJECT_FILE);
    if let Ok(old) = std::fs::read(&path) {
        if old != bytes && serde_json::from_slice::<serde::de::IgnoredAny>(&old).is_ok() {
            if let Err(e) = write_atomic(&dir.join(BACKUP_FILE), &old, false) {
                log::warn!("couldn't keep a backup of {}: {e}", path.display());
            }
        }
    }
    write_atomic(&path, bytes, true)
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The crash-recovery journal of one recording: what `finish_recording` would have needed to
/// place its takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecJournal {
    pub version: u32,
    /// When recording started, in seconds since the Unix epoch.
    pub started: u64,
    pub sample_rate: u32,
    /// Where recording started (the punch-in or the left locator).
    pub punch: i64,
    /// Where the transport started (before a count-in or pre-roll).
    pub from: i64,
    /// The cycle, when recording in cycle mode (passes become a take folder).
    pub cycle: Option<(i64, i64)>,
    /// The Autopunch range.
    pub punch_range: Option<(i64, i64)>,
    /// Frames the takes move earlier to line up with what was heard.
    pub latency: i64,
    /// A cycle pass cut short is kept as a take if it got this far.
    pub min_take: i64,
    pub takes: Vec<JournalTake>,
}

/// One file being recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalTake {
    pub track: Id,
    /// To make a track for the take if the recovered project doesn't have this one.
    pub track_name: String,
    pub input: InputSel,
    /// Absolute path of the WAV file.
    pub path: String,
    pub channels: usize,
    /// Engine position of the file's first frame.
    pub start: i64,
    pub bits: u16,
}

impl RecJournal {
    fn plan(&self) -> RecPlan {
        RecPlan { from: self.from, cycle: self.cycle, punch: self.punch_range, latency: self.latency, min_take: self.min_take }
    }
}

/// The recording journals left in a project folder, oldest first.
pub fn recording_journals(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().map(|n| n.to_string_lossy()).map(|n| n.starts_with(JOURNAL_PREFIX) && n.ends_with(JOURNAL_SUFFIX)).unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

pub fn read_journal(path: &Path) -> Result<RecJournal> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

/// Where a journal's take is now: where it was recorded, or in the Audio Files of `dir` when
/// the project folder has moved since.
fn take_path(dir: &Path, t: &JournalTake) -> PathBuf {
    let p = PathBuf::from(&t.path);
    if p.exists() {
        return p;
    }
    match p.file_name() {
        Some(name) if dir.join(AUDIO_DIR).join(name).exists() => dir.join(AUDIO_DIR).join(name),
        _ => p,
    }
}

/// Audio left behind by a recording that never finished, as the recovery sheet shows it.
#[derive(Debug, Clone, Serialize)]
pub struct RecordingRecovery {
    /// When the (latest) interrupted recording started, in seconds since the Unix epoch.
    pub started: u64,
    /// Files with audio in them.
    pub takes: usize,
    /// The longest file, in seconds.
    pub seconds: f64,
    pub tracks: Vec<String>,
}

/// What interrupted recordings in `dir` left to recover, if anything.
pub fn recording_info(dir: &Path) -> Option<RecordingRecovery> {
    let mut info = RecordingRecovery { started: 0, takes: 0, seconds: 0.0, tracks: Vec::new() };
    for j in recording_journals(dir).iter().filter_map(|p| read_journal(p).ok()) {
        for t in &j.takes {
            let Ok(w) = crate::wav_repair::inspect(&take_path(dir, t)) else { continue };
            if w.frames == 0 {
                continue;
            }
            info.started = info.started.max(j.started);
            info.takes += 1;
            info.seconds = info.seconds.max(w.frames as f64 / w.sample_rate.max(1) as f64);
            if !info.tracks.contains(&t.track_name) {
                info.tracks.push(t.track_name.clone());
            }
        }
    }
    (info.takes > 0).then_some(info)
}

/// Deletes the recording journals in `dir` (but `keep`), first repairing the files they
/// list, so the audio stays usable by hand (File › Import) even though no region refers to it.
pub fn discard_journals(dir: &Path, keep: Option<&Path>) {
    for p in recording_journals(dir) {
        if Some(p.as_path()) == keep {
            continue;
        }
        if let Ok(j) = read_journal(&p) {
            for t in &j.takes {
                let _ = crate::wav_repair::repair(&take_path(dir, t));
            }
        }
        let _ = std::fs::remove_file(&p);
    }
}

/// Every plug-in slot in a project.
fn slots_mut(p: &mut Project) -> impl Iterator<Item = &mut PluginSlot> {
    p.tracks
        .iter_mut()
        .flat_map(|t| t.instrument.iter_mut().chain(t.inserts.iter_mut().flatten()))
        .chain(p.master.inserts.iter_mut().flatten())
}

/// Why a project document couldn't be opened, in words for the user.
fn describe(file: &str, e: &anyhow::Error) -> String {
    if let Some(j) = e.downcast_ref::<serde_json::Error>() {
        return match j.classify() {
            serde_json::error::Category::Eof => format!("{file} is cut short"),
            _ => format!("{file} is damaged ({j})"),
        };
    }
    match e.root_cause().downcast_ref::<std::io::Error>() {
        Some(io) if io.kind() == std::io::ErrorKind::NotFound => format!("{file} is missing"),
        _ => format!("{file} can't be read ({e:#})"),
    }
}

impl Session {
    /// Asks for an autosave at the main loop's next turn, if Settings › General says so: after
    /// work that would be costly to redo.
    pub(super) fn request_autosave(&mut self) {
        if self.prefs.general.autosave_after_recording {
            self.autosave_soon = true;
        }
    }

    /// Whether an autosave was asked for (by [`Session::request_autosave`]) and not written yet.
    pub(super) fn autosave_requested(&self) -> bool {
        self.autosave_soon && self.prefs.general.autosave_after_recording
    }

    /// Writes the journal of the recording that just started (call once the recorder has its
    /// takes and `rec_punch`/`rec_plan` are set). With unsaved changes, the project is
    /// autosaved too, so the tracks the takes belong to can be recovered with them.
    pub(super) fn start_journal(&mut self) {
        let (Some(punch), Some(plan)) = (self.rec_punch, self.rec_plan) else { return };
        if self.prefs.general.autosave_after_recording && (self.autosave_soon || self.dirty) {
            if let Some(job) = self.autosave_doc() {
                self.autosave_soon = false;
                std::thread::spawn(move || {
                    if let Err(e) = job.write() {
                        log::warn!("autosave failed: {e}");
                    }
                });
            }
        }
        let takes: Vec<JournalTake> = self
            .recorder
            .active()
            .into_iter()
            .map(|a| {
                let t = self.project.track(a.track);
                JournalTake {
                    track: a.track,
                    track_name: t.map(|t| t.name.clone()).unwrap_or_default(),
                    input: t.map(|t| t.input).unwrap_or(InputSel::Mono(0)),
                    path: a.path.to_string_lossy().to_string(),
                    channels: a.channels,
                    start: a.start,
                    bits: a.bits,
                }
            })
            .collect();
        if takes.is_empty() {
            return;
        }
        let millis = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let path = self.dir.join(format!("{JOURNAL_PREFIX}{millis}{JOURNAL_SUFFIX}"));
        let journal = RecJournal {
            version: 1,
            started: now_secs(),
            sample_rate: self.sample_rate() as u32,
            punch,
            from: plan.from,
            cycle: plan.cycle,
            punch_range: plan.punch,
            latency: plan.latency,
            min_take: plan.min_take,
            takes,
        };
        // Small, and it must be on disk before there's audio worth recovering.
        match serde_json::to_vec_pretty(&journal).map_err(anyhow::Error::from).and_then(|b| write_atomic(&path, &b, true)) {
            Ok(()) => self.rec_journal = Some(path),
            Err(e) => log::warn!("couldn't write the recording journal {}: {e}", path.display()),
        }
    }

    /// After a clean stop: the takes are in the project now, so the journal waits for them to
    /// reach a file on disk, which the autosave asked for here does right away.
    pub(super) fn recording_finished(&mut self) {
        if let Some(j) = self.rec_journal.take() {
            self.unsaved_journals.push(j);
        }
        self.request_autosave();
    }

    /// Puts takes from recordings that never finished (the journals in the project folder)
    /// back on their tracks, as stopping would have, as one undo step. Files already in the
    /// project are skipped, so this is safe to repeat. Returns how many takes it placed.
    pub(super) fn recover_recordings(&mut self) -> usize {
        let mut placed = 0;
        let mut checkpointed = false;
        for jpath in recording_journals(&self.dir) {
            let j = match read_journal(&jpath) {
                Ok(j) => j,
                Err(e) => {
                    log::warn!("unreadable recording journal {}: {e}", jpath.display());
                    continue;
                }
            };
            let used: HashSet<PathBuf> = self.project.clips.iter().map(|c| if Path::new(&c.path).is_absolute() { PathBuf::from(&c.path) } else { self.dir.join(&c.path) }).collect();
            let mut takes = Vec::new();
            for t in &j.takes {
                let path = take_path(&self.dir, t);
                if used.contains(&path) {
                    continue;
                }
                let info = match crate::wav_repair::repair(&path) {
                    Ok(i) if i.frames > 0 => i,
                    Ok(_) => continue,
                    Err(e) => {
                        log::warn!("can't recover {}: {e}", path.display());
                        continue;
                    }
                };
                let decoded = match crate::decode::decode(&path) {
                    Ok(d) => d,
                    Err(e) => {
                        log::warn!("can't read the recovered {}: {e}", path.display());
                        continue;
                    }
                };
                if info.sample_rate as f64 != self.sample_rate() {
                    log::warn!("{} was recorded at {} Hz; the project now runs at {}", path.display(), info.sample_rate, self.sample_rate());
                }
                if !checkpointed {
                    self.checkpoint("Recover Recording", None, false);
                    checkpointed = true;
                }
                let track = match self.project.track(t.track) {
                    Some(tr) if tr.kind == TrackKind::Audio => t.track,
                    // The project was last written before this track existed.
                    _ => match self.add_track(TrackKind::Audio, t.input, None, None) {
                        Ok(id) => {
                            if let Some(tr) = self.project.track_mut(id) {
                                tr.name = if t.track_name.is_empty() { "Recovered".into() } else { t.track_name.clone() };
                            }
                            id
                        }
                        Err(e) => {
                            log::warn!("can't make a track for {}: {e}", path.display());
                            continue;
                        }
                    },
                };
                takes.push((track, FinishedTake { path, start: t.start, channels: decoded.channels, error: None }));
            }
            if !takes.is_empty() {
                placed += takes.len();
                self.rec_punch = Some(j.punch);
                self.rec_plan = Some(j.plan());
                self.rec_midi_track = None;
                self.place_recording(takes, j.latency);
            }
            // Gone once the recovered project reaches the disk.
            self.unsaved_journals.push(jpath);
        }
        if placed > 0 {
            self.sync_tracks();
            self.dirty = true;
            self.request_autosave();
        }
        placed
    }

    /// Opens the project in `dir` from project.json, or when that is missing or damaged, from
    /// the autosave or the backup, saying which in the next tick's warning.
    pub(super) fn open_checked(&mut self, dir: &Path) -> Result<()> {
        let folder = if dir.is_file() { dir.parent().unwrap_or(dir) } else { dir };
        // With backups on there's no .bak, but the newest of the backups (backups.rs).
        let latest = super::backups::latest_backup(folder);
        let mut files = vec![PROJECT_FILE, AUTOSAVE_FILE, BACKUP_FILE];
        files.extend(latest.as_deref());
        self.open_first(dir, &files)
    }

    /// Opens the first of `files` in `dir` that can be read. Falling back past the first one
    /// is reported as a warning and leaves the project marked as edited, so saving repairs it.
    pub(super) fn open_first(&mut self, dir: &Path, files: &[&str]) -> Result<()> {
        let dir = if dir.is_file() { dir.parent().unwrap_or(dir).to_path_buf() } else { dir.to_path_buf() };
        let name = dir.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut problems: Vec<String> = Vec::new();
        for &file in files {
            let path = dir.join(file);
            if !problems.is_empty() && !path.exists() {
                continue;
            }
            match self.open_from(&dir, file) {
                Ok(()) => {
                    if !problems.is_empty() {
                        let what = match file {
                            AUTOSAVE_FILE => "its most recent autosave",
                            BACKUP_FILE => "the backup kept by the save before last (recent changes may be missing)",
                            f if f.starts_with(super::backups::BACKUPS_DIR) => "its most recent backup (recent changes may be missing)",
                            _ => file,
                        };
                        let msg = format!("“{name}”: {}, so Fermata opened {what} instead. Save the project to repair it.", problems.join("; "));
                        log::warn!("{msg}");
                        self.warning = Some(msg);
                        self.saved = dir.join(PROJECT_FILE).exists();
                        self.dirty = true;
                    }
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("opening {}: {e:#}", path.display());
                    problems.push(describe(file, &e));
                }
            }
        }
        if problems.len() == 1 && !dir.join(PROJECT_FILE).exists() {
            return Err(anyhow!("“{name}” is not a Fermata project (there's no {PROJECT_FILE} in it)"));
        }
        Err(anyhow!("“{name}” can't be opened: {}, and there's no autosave or backup to fall back on.", problems.join("; ")))
    }

    /// After a save wrote `doc`: the autosave and journals it supersedes go.
    pub(super) fn saved_doc(&mut self, doc: &Project) {
        self.remember_states(doc);
        for j in std::mem::take(&mut self.unsaved_journals) {
            let _ = std::fs::remove_file(j);
        }
        self.autosave_soon = false;
    }

    /// Keeps the plug-in states of a document just captured, for [`Session::emergency_autosave`].
    pub(super) fn remember_states(&mut self, doc: &Project) {
        let mut doc = doc.clone();
        self.captured_states = slots_mut(&mut doc).filter_map(|s| Some((s.uid, s.state.take()?))).collect();
    }

    /// A new project or another one was opened: what was pending for the old one no longer
    /// applies. (Its journals stay on disk, so its takes are offered for recovery next time,
    /// unless the user chose not to keep them, which deletes them; see `discard_autosave`.)
    pub(super) fn forget_unsaved(&mut self) {
        self.unsaved_journals.clear();
        self.captured_states.clear();
        self.autosave_soon = false;
    }

    /// Writes the autosave from the project as it stands, with plug-in states from the last
    /// autosave or save: for a panic hook, where calling into plug-ins isn't safe. Nothing is
    /// written without unsaved changes.
    pub fn emergency_autosave(&self) -> Result<Option<PathBuf>> {
        if !self.dirty {
            return Ok(None);
        }
        let mut doc = self.project.clone();
        for s in slots_mut(&mut doc) {
            if let Some(st) = self.captured_states.get(&s.uid) {
                s.state = Some(st.clone());
            }
        }
        let path = self.dir.join(AUTOSAVE_FILE);
        write_atomic(&path, &serde_json::to_vec(&doc)?, true)?;
        Ok(Some(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::model::RegionContent;

    fn session(tag: &str) -> (Session, PathBuf) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let scratch = std::env::temp_dir().join(format!("fermata-recovery-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        s.dir = scratch.join("Song.fermata");
        (s, scratch)
    }

    /// A take as a crash leaves it: 24-bit, sizes never written, `frames` frames of audio.
    fn crashed_wav(path: &Path, frames: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames {
            w.write_sample((i % 100) as i32 * 1000).unwrap();
        }
        w.finalize().unwrap();
        let mut b = std::fs::read(path).unwrap();
        let data = b.windows(4).position(|w| w == b"data").unwrap();
        b[4..8].copy_from_slice(&[0; 4]);
        b[data + 4..data + 8].copy_from_slice(&[0; 4]);
        std::fs::write(path, b).unwrap();
    }

    fn journal(dir: &Path, track: Id, path: &Path, start: i64) -> RecJournal {
        RecJournal {
            version: 1,
            started: 1_700_000_000,
            sample_rate: 48000,
            punch: start,
            from: start,
            cycle: None,
            punch_range: None,
            latency: 0,
            min_take: 24000,
            takes: vec![JournalTake { track, track_name: "Vocal".into(), input: InputSel::Mono(0), path: path.to_string_lossy().to_string(), channels: 1, start, bits: 24 }],
        }
        .write_to(dir)
    }

    impl RecJournal {
        fn write_to(self, dir: &Path) -> RecJournal {
            let p = dir.join(format!("{JOURNAL_PREFIX}{}{JOURNAL_SUFFIX}", self.started));
            write_atomic(&p, &serde_json::to_vec(&self).unwrap(), false).unwrap();
            self
        }
    }

    #[test]
    fn journal_round_trips() {
        let dir = std::env::temp_dir().join(format!("fermata-journal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut j = journal(&dir, 7, &dir.join("a.wav"), 96000);
        j.cycle = Some((0, 192000));
        j.punch_range = Some((48000, 144000));
        let j = j.write_to(&dir);
        let found = recording_journals(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(read_journal(&found[0]).unwrap(), j);
        assert_eq!(j.plan(), RecPlan { from: 96000, cycle: Some((0, 192000)), punch: Some((48000, 144000)), latency: 0, min_take: 24000 });
        // Files that merely look similar aren't journals.
        std::fs::write(dir.join("recording-notes.txt"), "x").unwrap();
        assert_eq!(recording_journals(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interrupted_recording_is_detected_and_recovered() {
        let (mut s, scratch) = session("recover");
        let dir = s.dir.clone();
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.save(None).unwrap();
        assert!(super::super::recovery_info(&dir).is_none());

        let wav = dir.join(AUDIO_DIR).join("Vocal#01.wav");
        crashed_wav(&wav, 30000);
        journal(&dir, t, &wav, 96000);
        let info = super::super::recovery_info(&dir).expect("recording to recover");
        assert!(!info.has_autosave);
        let rec = info.recording.unwrap();
        assert_eq!((rec.takes, rec.tracks.clone()), (1, vec!["Vocal".to_string()]));
        assert!((rec.seconds - 30000.0 / 48000.0).abs() < 1e-9);

        // A fresh launch recovers it onto the track, where it was recorded.
        let (mut s2, _) = session("recover2");
        s2.open_recovered(&dir).unwrap();
        let regions = &s2.project.track(t).unwrap().regions;
        assert_eq!(regions.len(), 1);
        assert_eq!((regions[0].start, regions[0].length), (96000, 30000));
        assert!(matches!(regions[0].content, RegionContent::Audio { offset: 0, .. }));
        assert!(s2.dirty && s2.saved);
        assert!(!crate::wav_repair::inspect(&wav).unwrap().damaged);
        // The recovery can be undone, and an autosave is due now.
        assert!(s2.autosave_requested());
        assert_eq!(s2.undo_label().as_deref(), Some("Recover Recording"));
        // Doing it again finds the file already in the project.
        assert_eq!(s2.recover_recordings(), 0);
        // The autosave that follows takes the journal's place.
        let job = s2.autosave_job().expect("autosave right after recovery");
        job.write().unwrap();
        assert!(recording_journals(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn recovery_makes_a_track_the_project_never_saved() {
        let (mut s, scratch) = session("newtrack");
        let dir = s.dir.clone();
        let wav = dir.join(AUDIO_DIR).join("Vocal#01.wav");
        crashed_wav(&wav, 4800);
        journal(&dir, 4242, &wav, 0);
        // Never saved, never autosaved: only the journal is there.
        let info = super::super::recovery_info(&dir).expect("recoverable");
        assert!(info.saved.is_none() && !info.has_autosave);
        s.open_recovered(&dir).unwrap();
        assert_eq!(s.dir, dir);
        let t = s.project.tracks.iter().find(|t| t.name == "Vocal").expect("a track for the take");
        assert_eq!(t.regions.len(), 1);
        assert_eq!(t.regions[0].length, 4800);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn discarding_keeps_the_audio_usable() {
        let (s, scratch) = session("discard");
        let dir = s.dir.clone();
        let wav = dir.join(AUDIO_DIR).join("Vocal#01.wav");
        crashed_wav(&wav, 4800);
        journal(&dir, 1, &wav, 0);
        super::super::discard_autosave(&dir);
        assert!(recording_journals(&dir).is_empty());
        assert!(super::super::recovery_info(&dir).is_none());
        assert_eq!(crate::decode::decode(&wav).unwrap().channels[0].len(), 4800);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn saves_are_atomic_with_a_backup_and_damage_falls_back() {
        let (mut s, scratch) = session("damaged");
        let dir = s.dir.clone();
        // The single .bak is what a save keeps with backups off (backups.rs tests the rest).
        s.prefs.general.backups = 0;
        s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.set_tempo(100.0);
        s.save(None).unwrap();
        assert!(!dir.join(BACKUP_FILE).exists());
        s.set_tempo(110.0);
        s.save(None).unwrap();
        // The previous save is the backup; no temporary files are left.
        let bak: Project = serde_json::from_slice(&std::fs::read(dir.join(BACKUP_FILE)).unwrap()).unwrap();
        assert_eq!(bak.tempo, 100.0);
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(leftovers.is_empty());

        // Truncated project.json, newer autosave: the autosave opens, with a warning.
        s.set_tempo(120.0);
        s.dirty = true;
        s.autosave_now().unwrap();
        let full = std::fs::read(dir.join(PROJECT_FILE)).unwrap();
        std::fs::write(dir.join(PROJECT_FILE), &full[..full.len() / 2]).unwrap();
        let (mut s2, _) = session("damaged2");
        s2.open(&dir).unwrap();
        assert_eq!(s2.project.tempo, 120.0);
        assert!(s2.dirty);
        let w = s2.warning.take().unwrap();
        assert!(w.contains("project.json is cut short") && w.contains("autosave"), "{w}");

        // No autosave either: the backup.
        std::fs::remove_file(dir.join(AUTOSAVE_FILE)).unwrap();
        s2.open(&dir).unwrap();
        assert_eq!(s2.project.tempo, 100.0);
        assert!(s2.warning.take().unwrap().contains("backup"));

        // Nothing to fall back on: a clear error, and the open project stays.
        std::fs::write(dir.join(BACKUP_FILE), b"{ not json").unwrap();
        let e = s2.open(&dir).unwrap_err().to_string();
        assert!(e.contains("project.json is cut short") && e.contains("no autosave or backup"), "{e}");
        assert_eq!(s2.project.tempo, 100.0);

        // A damaged project.json never replaces a good backup.
        std::fs::write(dir.join(BACKUP_FILE), &full).unwrap();
        s2.set_tempo(130.0);
        s2.save(None).unwrap();
        assert_eq!(std::fs::read(dir.join(BACKUP_FILE)).unwrap(), full);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_damaged_autosave_is_not_offered() {
        let (mut s, scratch) = session("badauto");
        let dir = s.dir.clone();
        s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.save(None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join(AUTOSAVE_FILE), b"{\"name\": \"So").unwrap();
        assert!(super::super::recovery_info(&dir).is_none());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn emergency_autosave_keeps_captured_plugin_states() {
        let (mut s, scratch) = session("emergency");
        assert!(s.emergency_autosave().unwrap().is_none());
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.set_tempo(90.0);
        s.dirty = true;
        let path = s.emergency_autosave().unwrap().unwrap();
        let doc: Project = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(doc.tempo, 90.0);
        assert!(doc.track(t).is_some());
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
