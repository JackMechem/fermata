//! The session side of Settings and Project Settings: applying app preferences, recording
//! placement rules, project options, autosave and crash recovery.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Serialize;

use super::{default_projects_root, untitled_dir, Session, AUDIO_DIR, AUTOSAVE_FILE, PROJECT_FILE};
use crate::dsp::{ClickParams, Cmd};
use crate::midi_io::{MidiLogEntry, MidiPort};
use crate::model::{Id, Note, Project, Region, RegionContent};
use crate::prefs::{AudioOverlap, BitDepth, ClickSettings, Prefs};

/// An autosave waiting to be written. Taking it (on the main thread, where plug-in states can
/// be read) is cheap; [`AutosaveJob::write`] does the disk work and can run on any thread.
pub struct AutosaveJob {
    path: PathBuf,
    text: String,
    /// Recording journals whose takes this autosave holds: deleted once it is written.
    journals: Vec<PathBuf>,
}

impl AutosaveJob {
    /// Writes the autosave atomically (temporary file, synced, then renamed over the old one),
    /// then deletes the recording journals it supersedes.
    pub fn write(self) -> Result<PathBuf> {
        super::recovery::write_atomic(&self.path, self.text.as_bytes(), true)?;
        for j in &self.journals {
            let _ = std::fs::remove_file(j);
        }
        Ok(self.path)
    }
}

/// A project folder holding unsaved work: an autosave newer than its saved document, or audio
/// from a recording that never finished.
#[derive(Debug, Clone, Serialize)]
pub struct Recovery {
    pub path: String,
    pub name: String,
    /// When the work was autosaved (or, with only a recording to recover, when that started),
    /// in seconds since the Unix epoch.
    pub autosaved: u64,
    /// When project.json was last saved; None if the project was never saved.
    pub saved: Option<u64>,
    /// Whether there is a readable autosave newer than project.json.
    pub has_autosave: bool,
    /// Audio recorded when Fermata quit, recovered along with the project.
    pub recording: Option<super::RecordingRecovery>,
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Whether `dir` has unsaved work to recover: a readable autosave newer than project.json, or
/// audio left by an interrupted recording.
pub fn recovery_info(dir: &Path) -> Option<Recovery> {
    let saved = modified(&dir.join(PROJECT_FILE));
    let auto = modified(&dir.join(AUTOSAVE_FILE)).filter(|a| saved.map(|s| s < *a).unwrap_or(true)).filter(|_| {
        // A damaged autosave can't be recovered; opening the project falls back past it.
        let ok = std::fs::read(dir.join(AUTOSAVE_FILE)).ok().map(|b| serde_json::from_slice::<Project>(&b).is_ok()).unwrap_or(false);
        if !ok {
            log::warn!("the autosave in {} is damaged; not offering it", dir.display());
        }
        ok
    });
    let recording = super::recovery::recording_info(dir);
    if auto.is_none() && recording.is_none() {
        return None;
    }
    Some(Recovery {
        path: dir.to_string_lossy().to_string(),
        name: dir.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        autosaved: auto.map(secs).unwrap_or(0).max(recording.as_ref().map(|r| r.started).unwrap_or(0)),
        saved: saved.map(secs),
        has_autosave: auto.is_some(),
        recording,
    })
}

/// Deletes the autosave in `dir`, if any, and the journals of interrupted recordings (their
/// audio files stay, repaired).
pub fn discard_autosave(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(AUTOSAVE_FILE));
    super::recovery::discard_journals(dir, None);
}

impl Session {
    /// Applies app-wide settings. Safe to call at any time; only what changed is touched.
    pub fn set_prefs(&mut self, prefs: Prefs) {
        let old = std::mem::replace(&mut self.prefs, prefs);
        let p = &self.prefs;
        if old.midi.disabled_inputs != p.midi.disabled_inputs {
            self.midi.set_disabled(p.midi.disabled_inputs.clone());
        }
        self.midi.set_velocity(p.midi.velocity, p.midi.fixed_velocity);
        let limit = p.general.undo_limit.max(1);
        if self.undo_stack.len() > limit {
            let extra = self.undo_stack.len() - limit;
            self.undo_stack.drain(..extra);
        }
        // An untouched new project follows the new defaults.
        let pristine = !self.saved && !self.dirty && self.project.tracks.is_empty() && self.project.clips.is_empty();
        if pristine {
            self.project.click = self.prefs.click;
            if old.general.project_folder != self.prefs.general.project_folder && !self.dir.exists() {
                self.dir = untitled_dir(&self.projects_root());
            }
        }
        let thru = self.prefs.midi.thru;
        self.send(Cmd::MidiThru(thru));
        self.send(Cmd::Click(ClickParams::from_settings(&self.project.click)));
        // Settings › Audio › Plug-in Delay Compensation and Low Latency Mode.
        self.update_pdc();
    }

    /// Where new projects go.
    pub fn projects_root(&self) -> PathBuf {
        self.prefs.general.project_folder.as_deref().filter(|f| !f.trim().is_empty()).map(PathBuf::from).unwrap_or_else(default_projects_root)
    }

    /// Where new recordings go: the project's Audio Files, or a folder per project inside the
    /// recording folder chosen in Settings › Recording.
    pub(super) fn recording_dir(&self) -> PathBuf {
        match self.prefs.recording.folder.as_deref().filter(|f| !f.trim().is_empty()) {
            Some(f) => PathBuf::from(f).join(&self.project.name),
            None => self.dir.join(AUDIO_DIR),
        }
    }

    /// The anti-click fade for a new audio region of `length` frames (Settings › Editing).
    pub(super) fn default_fade(&self, length: i64) -> i64 {
        let ms = self.prefs.editing.fade_ms.clamp(0.0, 1000.0) as f64;
        ((ms * self.sample_rate() / 1000.0).round() as i64).min(length / 2).max(0)
    }

    /// Puts a finished audio take on `track`, following Settings › Recording › Overlapping
    /// Recordings. Returns the track it landed on.
    pub(super) fn place_take(&mut self, track: Id, region: Region) -> Option<Id> {
        let (rs, re) = (region.start, region.start + region.length);
        let overlaps = |r: &Region| matches!(r.content, RegionContent::Audio { .. }) && r.start < re && r.start + r.length > rs;
        let any = self.project.track(track)?.regions.iter().any(overlaps);
        let dest = match self.prefs.recording.audio_overlap {
            AudioOverlap::NewTrack if any => {
                // A track with the same settings and plug-ins, right below, not record-enabled.
                *self.duplicate_tracks(&[track], false).first()?
            }
            AudioOverlap::Replace if any => {
                let ids: Vec<Id> = self.project.track(track)?.regions.iter().filter(|r| overlaps(r)).map(|r| r.id).collect();
                for id in ids {
                    let right_id = self.project.alloc_id();
                    let t = self.project.track_mut(track)?;
                    let Some(i) = t.regions.iter().position(|r| r.id == id) else { continue };
                    let (start, end) = (t.regions[i].start, t.regions[i].start + t.regions[i].length);
                    if start >= rs && end <= re {
                        t.regions.remove(i);
                        continue;
                    }
                    let r = &mut t.regions[i];
                    if start < rs && end > re {
                        // The take lands in the middle: keep both ends, the right one where the
                        // original sat so the layering doesn't change.
                        let mut right = r.clone();
                        right.id = right_id;
                        right.start = re;
                        right.length = end - re;
                        if let RegionContent::Audio { offset, fade_in, .. } = &mut right.content {
                            *offset += re - start;
                            *fade_in = 0;
                        }
                        let len = rs - start;
                        r.length = len;
                        if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
                            *fade_out = 0;
                            *fade_in = (*fade_in).min(len);
                        }
                        t.regions.insert(i + 1, right);
                    } else if start < rs {
                        let len = rs - start;
                        r.length = len;
                        if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
                            *fade_out = (*fade_out).min(len);
                            *fade_in = (*fade_in).min(len - *fade_out);
                        }
                    } else {
                        let len = end - re;
                        r.start = re;
                        r.length = len;
                        if let RegionContent::Audio { offset, fade_in, fade_out, .. } = &mut r.content {
                            *offset += re - start;
                            *fade_in = (*fade_in).min(len);
                            *fade_out = (*fade_out).min(len - *fade_in);
                        }
                    }
                }
                track
            }
            _ => track,
        };
        self.project.track_mut(dest)?.regions.push(region);
        Some(dest)
    }

    /// Merges recorded notes (relative to `start`) into the MIDI region they overlap on
    /// `track`, growing it to cover them. False when there is none to merge into.
    pub(super) fn merge_midi_take(&mut self, track: Id, start: i64, end: i64, notes: &[Note]) -> bool {
        let Some(t) = self.project.track_mut(track) else { return false };
        let Some(r) = t.regions.iter_mut().rev().find(|r| matches!(r.content, RegionContent::Midi { .. }) && r.start < end && r.start + r.length > start) else {
            return false;
        };
        let new_start = r.start.min(start);
        let new_end = (r.start + r.length).max(end);
        let RegionContent::Midi { notes: existing, cc } = &mut r.content else { return false };
        let shift_old = r.start - new_start;
        for n in existing.iter_mut() {
            n.start += shift_old;
        }
        for c in cc.iter_mut() {
            c.start += shift_old;
        }
        existing.extend(notes.iter().map(|n| Note { start: n.start + start - new_start, ..*n }));
        existing.sort_by_key(|n| n.start);
        r.start = new_start;
        r.length = new_end - new_start;
        true
    }

    /// Project Settings › Metronome.
    pub fn set_click(&mut self, click: ClickSettings) {
        self.project.click = click;
        self.send(Cmd::Click(ClickParams::from_settings(&click)));
        self.dirty = true;
    }

    /// Project Settings › Recording: None follows Settings › Recording.
    pub fn set_record_bit_depth(&mut self, depth: Option<BitDepth>) {
        self.project.bit_depth = depth;
        self.dirty = true;
    }

    pub fn set_notes(&mut self, notes: String) {
        self.project.notes = notes;
        self.dirty = true;
    }

    /// Moves regions by `delta` samples (the Nudge keys).
    pub fn nudge_regions(&mut self, ids: &[Id], delta: i64) {
        let pairs = self.crossfade_pairs(ids);
        let mut touched = Vec::new();
        for t in &mut self.project.tracks {
            for r in t.regions.iter_mut().filter(|r| ids.contains(&r.id)) {
                r.start = (r.start + delta).max(0);
                if !touched.contains(&t.id) {
                    touched.push(t.id);
                }
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
        self.auto_crossfade(ids, pairs);
    }

    /// MIDI input ports and whether each is in use.
    pub fn midi_ports(&self) -> Vec<MidiPort> {
        self.midi.ports()
    }

    /// The latest messages from hardware MIDI inputs.
    pub fn midi_monitor(&self) -> Vec<MidiLogEntry> {
        self.midi.recent()
    }

    // ---- autosave ---------------------------------------------------------------------------

    /// The next autosave, once Settings › General's interval has passed with unsaved changes,
    /// or right after a recording, bounce or freeze (`request_autosave`). Call it from the
    /// main thread now and then and hand the job to a worker thread.
    pub fn autosave_job(&mut self) -> Option<AutosaveJob> {
        let mins = self.prefs.general.autosave_minutes;
        let due = mins > 0 && self.autosaved_at.elapsed() >= Duration::from_secs(mins as u64 * 60);
        if !self.dirty || self.rec_punch.is_some() || !(due || self.autosave_requested()) {
            return None;
        }
        self.autosave_soon = false;
        self.autosaved_at = Instant::now();
        self.autosave_doc()
    }

    /// Autosaves right away (if anything changed since the last autosave).
    pub fn autosave_now(&mut self) -> Result<Option<PathBuf>> {
        match self.autosave_doc() {
            Some(job) => job.write().map(Some),
            None => Ok(None),
        }
    }

    pub(super) fn autosave_doc(&mut self) -> Option<AutosaveJob> {
        let doc = self.capture_plugin_states();
        let text = serde_json::to_string(&doc).ok()?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&self.dir, &text).hash(&mut h);
        let hash = h.finish();
        if hash == self.autosave_hash {
            // The autosave on disk already holds these takes.
            for j in std::mem::take(&mut self.unsaved_journals) {
                let _ = std::fs::remove_file(j);
            }
            return None;
        }
        self.autosave_hash = hash;
        self.remember_states(&doc);
        Some(AutosaveJob { path: self.dir.join(AUTOSAVE_FILE), text, journals: std::mem::take(&mut self.unsaved_journals) })
    }

    /// Opens a project from its autosave (or, with only an interrupted recording to recover,
    /// from project.json), then puts back any takes recordings left unfinished. It counts as
    /// edited, so saving writes it back.
    pub fn open_recovered(&mut self, dir: &Path) -> Result<()> {
        let dir = if dir.is_file() { dir.parent().unwrap_or(dir).to_path_buf() } else { dir.to_path_buf() };
        let autosave = recovery_info(&dir).map(|r| r.has_autosave).unwrap_or(false);
        if autosave {
            self.open_first(&dir, &[AUTOSAVE_FILE, PROJECT_FILE, super::BACKUP_FILE])?;
        } else if [PROJECT_FILE, AUTOSAVE_FILE, super::BACKUP_FILE].iter().any(|f| dir.join(f).exists()) {
            self.open_checked(&dir)?;
        } else {
            // A recording in a project that was never saved or autosaved.
            self.new_project();
            self.project.name = dir.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
            self.dir = dir.clone();
        }
        self.saved = self.dir.join(PROJECT_FILE).exists();
        self.recover_recordings();
        self.dirty = true;
        Ok(())
    }

    /// Deletes the current project's autosave and recording journals (the user chose not to
    /// keep the changes).
    pub fn discard_autosave(&mut self) {
        let _ = std::fs::remove_file(self.dir.join(AUTOSAVE_FILE));
        for j in std::mem::take(&mut self.unsaved_journals) {
            let _ = std::fs::remove_file(j);
        }
        super::recovery::discard_journals(&self.dir, self.rec_journal.as_deref());
        self.autosave_hash = 0;
        self.autosave_soon = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::model::{InputSel, TrackKind};

    fn session(tag: &str) -> (Session, PathBuf) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let scratch = std::env::temp_dir().join(format!("fermata-options-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        s.dir = scratch.join("Song.fermata");
        (s, scratch)
    }

    #[test]
    fn autosave_writes_and_recovery_is_detected() {
        let (mut s, scratch) = session("autosave");
        s.prefs.general.autosave_minutes = 1;
        let dir = s.dir.clone();
        s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        assert!(s.dirty);
        // Not due yet.
        assert!(s.autosave_job().is_none());
        s.autosaved_at = Instant::now().checked_sub(Duration::from_secs(61)).unwrap();
        let job = s.autosave_job().expect("autosave due");
        let path = std::thread::spawn(move || job.write()).join().unwrap().unwrap();
        assert_eq!(path, dir.join(AUTOSAVE_FILE));
        assert!(path.exists());
        // Never saved: the autosave is recoverable.
        let r = recovery_info(&dir).expect("recoverable");
        assert!(r.saved.is_none());

        // Saving supersedes it.
        s.save(None).unwrap();
        assert!(!dir.join(AUTOSAVE_FILE).exists());
        assert!(recovery_info(&dir).is_none());

        // A later edit autosaves again and is newer than project.json.
        std::thread::sleep(Duration::from_millis(20));
        s.set_tempo(97.0);
        s.dirty = true;
        assert!(s.autosave_now().unwrap().is_some());
        // Nothing new since: nothing written.
        assert!(s.autosave_now().unwrap().is_none());
        let r = recovery_info(&dir).expect("newer autosave");
        assert!(r.saved.is_some());

        // Recovering brings back the unsaved tempo, marked as edited.
        s.open(&dir).unwrap();
        assert_eq!(s.project.tempo, 120.0);
        s.open_recovered(&dir).unwrap();
        assert_eq!(s.project.tempo, 97.0);
        assert!(s.dirty && s.saved);
        s.discard_autosave();
        assert!(recovery_info(&dir).is_none());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn autosave_off_or_clean_does_nothing() {
        let (mut s, scratch) = session("off");
        s.prefs.general.autosave_minutes = 0;
        s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.autosaved_at = Instant::now().checked_sub(Duration::from_secs(3600)).unwrap();
        assert!(s.autosave_job().is_none());
        s.prefs.general.autosave_minutes = 1;
        s.dirty = false;
        assert!(s.autosave_job().is_none());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn undo_limit_is_configurable() {
        let (mut s, _scratch) = session("undo");
        let mut p = Prefs::default();
        p.general.undo_limit = 3;
        s.set_prefs(p);
        for i in 0..6 {
            s.checkpoint("Tempo", None, false);
            s.set_tempo(100.0 + i as f64);
        }
        let mut steps = 0;
        while s.undo().is_some() {
            steps += 1;
        }
        assert_eq!(steps, 3);
    }

    #[test]
    fn replace_trims_overlapped_regions() {
        let (mut s, _scratch) = session("replace");
        s.prefs.recording.audio_overlap = AudioOverlap::Replace;
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let audio = |id, start, length| Region {
            id,
            name: "r".into(),
            start,
            length,
            muted: false,
            color: None,
            content: RegionContent::Audio { clip: 0, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() },
            loop_end: None,
            takes: None,
            params: Default::default(),
        };
        s.project.track_mut(t).unwrap().regions.push(audio(100, 0, 1000));
        s.place_take(t, audio(101, 400, 200)).unwrap();
        let mut spans: Vec<(i64, i64, i64)> = s
            .project
            .track(t)
            .unwrap()
            .regions
            .iter()
            .map(|r| match r.content {
                RegionContent::Audio { offset, .. } => (r.start, r.length, offset),
                _ => unreachable!(),
            })
            .collect();
        spans.sort();
        assert_eq!(spans, vec![(0, 400, 0), (400, 200, 0), (600, 400, 600)]);

        s.prefs.recording.audio_overlap = AudioOverlap::NewTrack;
        let n = s.project.tracks.len();
        let dest = s.place_take(t, audio(102, 500, 100)).unwrap();
        assert_ne!(dest, t);
        assert_eq!(s.project.tracks.len(), n + 1);
        assert!(!s.project.track(dest).unwrap().armed);
    }

    #[test]
    fn midi_merge_grows_the_region() {
        let (mut s, _scratch) = session("merge");
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let note = |start| Note { start, length: 10, pitch: 60, velocity: 100, muted: false };
        s.project.track_mut(t).unwrap().regions.push(Region {
            id: 500,
            name: "m".into(),
            start: 1000,
            length: 1000,
            muted: false,
            color: None,
            content: RegionContent::Midi { notes: vec![note(0)], cc: Vec::new() },
            loop_end: None,
            takes: None,
            params: Default::default(),
        });
        assert!(s.merge_midi_take(t, 500, 1500, &[note(0), note(900)]));
        let r = &s.project.track(t).unwrap().regions[0];
        assert_eq!((r.start, r.length), (500, 1500));
        let RegionContent::Midi { notes, .. } = &r.content else { unreachable!() };
        let starts: Vec<i64> = notes.iter().map(|n| n.start).collect();
        assert_eq!(starts, vec![0, 500, 900]);
        assert!(!s.merge_midi_take(t, 5000, 6000, &[note(0)]));
    }
}
