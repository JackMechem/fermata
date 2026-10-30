//! Project backups (Logic's File › Revert to): every save keeps the version of project.json it
//! replaces in `Backups/<alternative>/<time>.json`, up to Settings › General › Backups of them
//! per project alternative (0 = off), and the project can go back to any of them.
//!
//! The backups take the place of project.json.bak: with backups on, a save keeps no .bak, and
//! opening a project whose project.json and autosave can't be read falls back on the newest
//! backup ([`latest_backup`]). With backups off, a save keeps the single .bak as before (see
//! `recovery.rs`).
//!
//! A backup is named after when its version was saved (the replaced file's modification
//! time), in UTC: `2026-10-29T143205.123Z.json` sorts by time and is a valid file name
//! everywhere. The UI shows it in local time.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use serde::Serialize;

use super::alternatives::ALTERNATIVES_DIR;
use super::recovery::{write_atomic, write_project_file};
use super::{Session, BACKUP_FILE, PROJECT_FILE};
use crate::model::Project;

pub const BACKUPS_DIR: &str = "Backups";

/// One kept version of a project alternative.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackupInfo {
    /// Path relative to the project folder.
    pub file: String,
    /// When this version was saved, in milliseconds since the Unix epoch.
    pub time: u64,
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The inverse of [`civil_from_days`].
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `ms` since the epoch as a backup's name: `2026-10-29T143205.123Z`.
pub(super) fn stamp(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{d:02}T{:02}{:02}{:02}.{:03}Z", t / 3600, t / 60 % 60, t % 60, ms % 1000)
}

/// The time a backup's name stands for, if it is one.
pub(super) fn parse_stamp(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 22 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[17] != b'.' || b[21] != b'Z' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r).filter(|x| x.bytes().all(|c| c.is_ascii_digit())).and_then(|x| x.parse::<u64>().ok());
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec, ms) = (num(11..13)?, num(13..15)?, num(15..17)?, num(18..21)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    let days = days_from_civil(y as i64, mo as u32, d as u32);
    Some((days as u64 * 86_400 + h * 3600 + mi * 60 + sec) * 1000 + ms)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn modified_ms(path: &Path) -> Option<u64> {
    let t = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis() as u64)
}

/// The folder holding alternative `alt`'s backups.
pub(super) fn backup_dir(dir: &Path, alt: &str) -> PathBuf {
    dir.join(BACKUPS_DIR).join(alt)
}

/// Alternative `alt`'s backups in the project folder `dir`, newest first.
pub(super) fn list_backups(dir: &Path, alt: &str) -> Vec<BackupInfo> {
    let mut out: Vec<BackupInfo> = std::fs::read_dir(backup_dir(dir, alt))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let time = parse_stamp(name.strip_suffix(".json")?)?;
                    Some(BackupInfo { file: format!("{BACKUPS_DIR}/{alt}/{name}"), time })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.time.cmp(&a.time));
    out
}

/// The name of the alternative a project document belongs to.
fn doc_alternative(bytes: &[u8]) -> Option<String> {
    let doc: Project = serde_json::from_slice(bytes).ok()?;
    Some(if doc.alternative.is_empty() { doc.name } else { doc.alternative })
}

/// Keeps `old`, a project.json about to be replaced that was saved at `time`, as a backup of
/// alternative `alt`, then deletes all but the newest `keep`. A version identical to the
/// newest backup isn't kept twice.
pub(super) fn keep_backup(dir: &Path, alt: &str, old: &[u8], time: u64, keep: usize) -> Result<()> {
    let existing = list_backups(dir, alt);
    let duplicate = existing.first().map(|b| std::fs::read(dir.join(&b.file)).map(|x| x == old).unwrap_or(false)).unwrap_or(false);
    if !duplicate {
        let folder = backup_dir(dir, alt);
        let mut t = time;
        while folder.join(format!("{}.json", stamp(t))).exists() {
            t += 1;
        }
        write_atomic(&folder.join(format!("{}.json", stamp(t))), old, false)?;
    }
    for b in list_backups(dir, alt).iter().skip(keep) {
        let _ = std::fs::remove_file(dir.join(&b.file));
    }
    Ok(())
}

/// The newest backup of the active alternative, as a path relative to `dir`: what opening
/// falls back on when project.json and the autosave can't be read. Backups of the other
/// (inactive) alternatives are passed over, since project.json is the active one.
pub(super) fn latest_backup(dir: &Path) -> Option<String> {
    let rd = std::fs::read_dir(dir.join(BACKUPS_DIR)).ok()?;
    rd.flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|alt| !dir.join(ALTERNATIVES_DIR).join(alt).join(PROJECT_FILE).exists())
        .flat_map(|alt| list_backups(dir, &alt))
        .max_by_key(|b| b.time)
        .map(|b| b.file)
}

impl Session {
    /// Writes project.json into the project folder `dir`, keeping the version it replaces: in
    /// Backups with backups on, else as project.json.bak. A damaged old file is never kept.
    pub(super) fn write_project(&self, dir: &Path, bytes: &[u8]) -> Result<()> {
        let keep = self.prefs.general.backups;
        let path = dir.join(PROJECT_FILE);
        let old = match std::fs::read(&path) {
            Ok(old) if keep > 0 && old != bytes => old,
            _ => return write_project_file(dir, bytes),
        };
        match doc_alternative(&old) {
            Some(alt) => match keep_backup(dir, &alt, &old, modified_ms(&path).unwrap_or_else(now_ms), keep) {
                // Superseded by the backup just kept.
                Ok(()) => {
                    let _ = std::fs::remove_file(dir.join(BACKUP_FILE));
                    write_atomic(&path, bytes, true)
                }
                Err(e) => {
                    log::warn!("couldn't keep a backup of {}: {e}", path.display());
                    write_project_file(dir, bytes)
                }
            },
            None => write_project_file(dir, bytes),
        }
    }

    /// The active alternative's backups, newest first (File › Revert to).
    pub fn backups(&self) -> Vec<BackupInfo> {
        if !self.saved {
            return Vec::new();
        }
        list_backups(&self.dir, &self.alternative_name())
    }

    /// Goes back to a backup (`file`, as [`Session::backups`] lists it): the project as it
    /// stands is saved, then becomes a backup itself when the chosen version takes its place
    /// in project.json, so reverting again undoes this.
    pub fn revert_to_backup(&mut self, file: &str) -> Result<()> {
        if !self.backups().iter().any(|b| b.file == file) {
            return Err(anyhow!("That backup no longer exists."));
        }
        let dir = self.dir.clone();
        let mut doc: Project = serde_json::from_slice(&std::fs::read(dir.join(file))?).map_err(|e| anyhow!("The backup can't be read: {e}"))?;
        // Kept even with backups off: the revert must be undoable.
        let keep = self.prefs.general.backups.max(1);
        let backups = std::mem::replace(&mut self.prefs.general.backups, keep);
        let result = (|| {
            self.save(None)?;
            doc.name = self.project.name.clone();
            doc.alternative = self.alternative_name();
            self.write_project(&dir, serde_json::to_string_pretty(&doc)?.as_bytes())
        })();
        self.prefs.general.backups = backups;
        result?;
        self.open_from(&dir, PROJECT_FILE)
    }

    /// File › Revert to Saved: back to project.json, dropping the changes since (and their
    /// autosave).
    pub fn revert_to_saved(&mut self) -> Result<()> {
        if !self.saved || !self.dir.join(PROJECT_FILE).exists() {
            return Err(anyhow!("The project hasn't been saved yet."));
        }
        let dir = self.dir.clone();
        self.discard_autosave();
        self.open(&dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::model::{InputSel, TrackKind};
    use crate::session::AUTOSAVE_FILE;

    fn session(tag: &str) -> (Session, PathBuf) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let scratch = std::env::temp_dir().join(format!("fermata-backups-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        s.dir = scratch.join("Song.fermata");
        (s, scratch)
    }

    #[test]
    fn stamps_round_trip_and_sort() {
        // 2026-10-29 14:32:05.123 UTC
        let ms = 1_793_284_325_123;
        assert_eq!(stamp(ms), "2026-10-29T143205.123Z");
        assert_eq!(parse_stamp("2026-10-29T143205.123Z"), Some(ms));
        assert_eq!(stamp(0), "1970-01-01T000000.000Z");
        // A leap day, and the turn of a century.
        for ms in [951_782_400_000u64, 4_107_542_399_999, 1_709_164_800_001] {
            assert_eq!(parse_stamp(&stamp(ms)), Some(ms));
        }
        assert!(stamp(ms) < stamp(ms + 1) && stamp(ms + 999) < stamp(ms + 1000));
        for bad in ["", "2026-10-29T143205.123", "2026-13-29T143205.123Z", "2026-10-29T253205.123Z", "2026-10-29X143205.123Z", "abcd-10-29T143205.123Z"] {
            assert_eq!(parse_stamp(bad), None, "{bad}");
        }
    }

    #[test]
    fn saves_keep_backups_pruned_to_the_limit() {
        let (mut s, scratch) = session("prune");
        let dir = s.dir.clone();
        s.prefs.general.backups = 3;
        s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.save(None).unwrap();
        assert!(s.backups().is_empty());
        for i in 0..5 {
            s.set_tempo(100.0 + i as f64);
            s.save(None).unwrap();
        }
        // Saving an unchanged project keeps nothing new.
        s.save(None).unwrap();
        let b = s.backups();
        assert_eq!(b.len(), 3);
        assert!(b.windows(2).all(|w| w[0].time > w[1].time));
        // Newest first: the version before the last save.
        let newest: Project = serde_json::from_slice(&std::fs::read(dir.join(&b[0].file)).unwrap()).unwrap();
        assert_eq!(newest.tempo, 103.0);
        assert!(b[0].file.starts_with("Backups/Song/"));
        // Backups replace project.json.bak.
        assert!(!dir.join(BACKUP_FILE).exists());

        // Off: nothing more is kept in Backups, and the .bak is back.
        s.prefs.general.backups = 0;
        s.set_tempo(90.0);
        s.save(None).unwrap();
        assert_eq!(s.backups().len(), 3);
        assert!(dir.join(BACKUP_FILE).exists());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn reverting_is_itself_undoable() {
        let (mut s, scratch) = session("revert");
        s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.set_tempo(100.0);
        s.save(None).unwrap();
        s.set_tempo(110.0);
        s.save(None).unwrap();
        // Unsaved changes are kept too, as a backup.
        s.set_tempo(120.0);
        s.dirty = true;
        let old = s.backups().into_iter().next().unwrap();
        s.revert_to_backup(&old.file).unwrap();
        assert_eq!(s.project.tempo, 100.0);
        assert!(!s.dirty && s.saved);
        let tempos: Vec<f64> = s
            .backups()
            .iter()
            .map(|b| serde_json::from_slice::<Project>(&std::fs::read(s.dir.join(&b.file)).unwrap()).unwrap().tempo)
            .collect();
        assert!(tempos.contains(&120.0) && tempos.contains(&110.0), "{tempos:?}");
        // Back to the state before the revert.
        let undo = s.backups().into_iter().find(|b| serde_json::from_slice::<Project>(&std::fs::read(s.dir.join(&b.file)).unwrap()).unwrap().tempo == 120.0).unwrap();
        s.revert_to_backup(&undo.file).unwrap();
        assert_eq!(s.project.tempo, 120.0);
        // Only listed backups can be reverted to.
        assert!(s.revert_to_backup("project.json").is_err());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn revert_to_saved_drops_changes() {
        let (mut s, scratch) = session("saved");
        s.set_tempo(100.0);
        assert!(s.revert_to_saved().is_err());
        s.save(None).unwrap();
        s.set_tempo(130.0);
        s.dirty = true;
        s.autosave_now().unwrap();
        s.revert_to_saved().unwrap();
        assert_eq!(s.project.tempo, 100.0);
        assert!(!s.dirty);
        assert!(!s.dir.join(AUTOSAVE_FILE).exists());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_damaged_project_falls_back_on_the_newest_backup() {
        let (mut s, scratch) = session("fallback");
        let dir = s.dir.clone();
        s.set_tempo(100.0);
        s.save(None).unwrap();
        s.set_tempo(110.0);
        s.save(None).unwrap();
        std::fs::write(dir.join(PROJECT_FILE), b"{ \"name\": ").unwrap();
        assert_eq!(latest_backup(&dir), s.backups().first().map(|b| b.file.clone()));
        let (mut s2, _) = session("fallback2");
        s2.open(&dir).unwrap();
        assert_eq!(s2.project.tempo, 100.0);
        assert!(s2.dirty);
        assert!(s2.warning.take().unwrap().contains("backup"));
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
