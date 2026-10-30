//! Project alternatives (Logic's File › Project Alternatives): named versions of a project's
//! arrangement in one project folder, sharing its Audio Files.
//!
//! The active alternative is always the project.json at the top of the folder, with its name
//! in `Project::alternative`, so autosave, recovery, backups and older versions of Fermata see
//! it as the project. The others wait in `Alternatives/<name>/project.json`. Switching saves
//! the active one, copies it into its folder, puts the chosen one in its place and removes
//! that one's folder. Every step writes atomically, and the order means a crash between steps
//! leaves at worst an extra copy, never a lost alternative ([`Session::alternatives`] lists a
//! name once).
//!
//! Save As saves only the active alternative into the new folder (the others refer to audio
//! files the copy doesn't take along).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use super::backups::{backup_dir, BACKUPS_DIR};
use super::recovery::write_atomic;
use super::{Session, AUTOSAVE_FILE, PROJECT_FILE};
use crate::model::Project;

pub const ALTERNATIVES_DIR: &str = "Alternatives";

/// Checks a name given to an alternative or a template, which is also a folder or file name.
/// Returns it trimmed.
pub(super) fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("The name can't be empty."));
    }
    if name.starts_with('.') || name.chars().any(|c| c.is_control() || "/\\:*?\"<>|".contains(c)) {
        return Err(anyhow!("A name can't start with a dot or contain / \\ : * ? \" < > |"));
    }
    if name.chars().count() > 100 {
        return Err(anyhow!("The name is too long."));
    }
    Ok(name.to_string())
}

/// The folder holding inactive alternative `name`.
fn alt_dir(dir: &Path, name: &str) -> PathBuf {
    dir.join(ALTERNATIVES_DIR).join(name)
}

/// The inactive alternatives in the project folder `dir`.
fn inactive(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir.join(ALTERNATIVES_DIR))
        .map(|rd| rd.flatten().filter(|e| e.path().join(PROJECT_FILE).is_file()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| !n.starts_with('.')).collect())
        .unwrap_or_default()
}

fn read_doc(path: &Path) -> Result<Project> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|e| anyhow!("{} can't be read: {e}", path.display()))
}

/// Renames a folder, also when only the case of its name changes (on file systems that ignore
/// case, a direct rename would be a no-op or fail).
fn rename_dir(from: &Path, to: &Path) -> Result<()> {
    let tmp = from.with_file_name(format!(".rename-{}", std::process::id()));
    std::fs::rename(from, &tmp)?;
    std::fs::rename(&tmp, to)?;
    Ok(())
}

impl Session {
    /// The active project alternative's name: the project's own name until it has one.
    pub fn alternative_name(&self) -> String {
        if self.project.alternative.is_empty() {
            self.project.name.clone()
        } else {
            self.project.alternative.clone()
        }
    }

    /// Every alternative of the project, the active one included, by name.
    pub fn alternatives(&self) -> Vec<String> {
        let active = self.alternative_name();
        let mut out = if self.saved { inactive(&self.dir) } else { Vec::new() };
        out.retain(|n| *n != active);
        out.push(active);
        out.sort_by_key(|n| n.to_lowercase());
        out
    }

    fn require_saved(&self) -> Result<()> {
        if !self.saved || !self.dir.join(PROJECT_FILE).exists() {
            return Err(anyhow!("Save the project first."));
        }
        Ok(())
    }

    fn check_free(&self, name: &str, except: Option<&str>) -> Result<()> {
        let taken = self.alternatives().iter().any(|n| Some(n.as_str()) != except && n.to_lowercase() == name.to_lowercase());
        if taken {
            return Err(anyhow!("There's already an alternative named “{name}”."));
        }
        Ok(())
    }

    /// Writes `doc` as project.json, as alternative `name`, without keeping a backup of the
    /// file it replaces (which is kept elsewhere, or being deleted).
    fn put_active(&self, mut doc: Project, name: &str) -> Result<()> {
        doc.name = self.project.name.clone();
        doc.alternative = name.to_string();
        write_atomic(&self.dir.join(PROJECT_FILE), serde_json::to_string_pretty(&doc)?.as_bytes(), true)
    }

    /// File › Project Alternatives › New Alternative: saves the active alternative, then goes
    /// on as a copy of it named `name`.
    pub fn new_alternative(&mut self, name: &str) -> Result<()> {
        let name = check_name(name)?;
        self.require_saved()?;
        self.check_free(&name, None)?;
        self.save(None)?;
        let cur = self.alternative_name();
        let root = self.dir.join(PROJECT_FILE);
        write_atomic(&alt_dir(&self.dir, &cur).join(PROJECT_FILE), &std::fs::read(&root)?, true)?;
        self.put_active(read_doc(&root)?, &name)?;
        self.project.alternative = name;
        Ok(())
    }

    /// Switches to alternative `name`, saving the active one first.
    pub fn switch_alternative(&mut self, name: &str) -> Result<()> {
        self.require_saved()?;
        let cur = self.alternative_name();
        if name == cur {
            return Ok(());
        }
        let src = alt_dir(&self.dir, name);
        // Read before anything changes: a damaged alternative leaves the project as it is.
        let doc = read_doc(&src.join(PROJECT_FILE))?;
        self.save(None)?;
        let root = self.dir.join(PROJECT_FILE);
        write_atomic(&alt_dir(&self.dir, &cur).join(PROJECT_FILE), &std::fs::read(&root)?, true)?;
        self.put_active(doc, name)?;
        std::fs::remove_dir_all(&src)?;
        let dir = self.dir.clone();
        self.open_from(&dir, PROJECT_FILE)
    }

    /// Renames alternative `name` (the active one or another), with its backups.
    pub fn rename_alternative(&mut self, name: &str, new_name: &str) -> Result<()> {
        let new_name = check_name(new_name)?;
        self.require_saved()?;
        if !self.alternatives().iter().any(|n| n == name) {
            return Err(anyhow!("There's no alternative named “{name}”."));
        }
        if new_name == name {
            return Ok(());
        }
        self.check_free(&new_name, Some(name))?;
        if name == self.alternative_name() {
            // The saved document and the autosave carry the name, not only the open project:
            // the name changes on disk now, unsaved changes or not.
            let root = self.dir.join(PROJECT_FILE);
            self.put_active(read_doc(&root)?, &new_name)?;
            let auto = self.dir.join(AUTOSAVE_FILE);
            if let Ok(mut doc) = read_doc(&auto) {
                doc.alternative = new_name.clone();
                write_atomic(&auto, serde_json::to_string(&doc)?.as_bytes(), true)?;
            }
            self.project.alternative = new_name.clone();
        } else {
            rename_dir(&alt_dir(&self.dir, name), &alt_dir(&self.dir, &new_name))?;
            let path = alt_dir(&self.dir, &new_name).join(PROJECT_FILE);
            let mut doc = read_doc(&path)?;
            doc.alternative = new_name.clone();
            write_atomic(&path, serde_json::to_string_pretty(&doc)?.as_bytes(), true)?;
        }
        let backups = backup_dir(&self.dir, name);
        if backups.is_dir() {
            rename_dir(&backups, &backup_dir(&self.dir, &new_name))?;
        }
        Ok(())
    }

    /// Deletes alternative `name` and its backups. Deleting the active one switches to the
    /// first of the others, without saving. A project keeps at least one alternative.
    pub fn delete_alternative(&mut self, name: &str) -> Result<()> {
        self.require_saved()?;
        let all = self.alternatives();
        if !all.iter().any(|n| n == name) {
            return Err(anyhow!("There's no alternative named “{name}”."));
        }
        let Some(next) = all.iter().find(|n| *n != name).cloned() else {
            return Err(anyhow!("A project keeps at least one alternative."));
        };
        if name == self.alternative_name() {
            let src = alt_dir(&self.dir, &next);
            self.put_active(read_doc(&src.join(PROJECT_FILE))?, &next)?;
            std::fs::remove_dir_all(&src)?;
            // Its unsaved changes go with it.
            self.discard_autosave();
            let dir = self.dir.clone();
            self.open_from(&dir, PROJECT_FILE)?;
        } else {
            std::fs::remove_dir_all(alt_dir(&self.dir, name))?;
        }
        let _ = std::fs::remove_dir_all(backup_dir(&self.dir, name));
        // Leave no empty Backups folder behind.
        let _ = std::fs::remove_dir(self.dir.join(BACKUPS_DIR));
        let _ = std::fs::remove_dir(self.dir.join(ALTERNATIVES_DIR));
        Ok(())
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
        let scratch = std::env::temp_dir().join(format!("fermata-alternatives-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        s.dir = scratch.join("Song.fermata");
        (s, scratch)
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(check_name("  Mix 2 ").unwrap(), "Mix 2");
        for bad in ["", "  ", ".hidden", "a/b", "a\\b", "a:b", "x?", "tab\there"] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
        assert!(check_name(&"x".repeat(101)).is_err());
    }

    #[test]
    fn new_switch_rename_delete() {
        let (mut s, scratch) = session("cycle");
        let dir = s.dir.clone();
        assert!(s.new_alternative("B").is_err(), "needs a saved project");
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.save(None).unwrap();
        assert_eq!(s.alternatives(), vec!["Song".to_string()]);

        // A copy, under a new name.
        s.new_alternative("Rough Mix").unwrap();
        assert_eq!(s.alternative_name(), "Rough Mix");
        assert_eq!(s.alternatives(), vec!["Rough Mix".to_string(), "Song".to_string()]);
        assert!(s.project.track(t).is_some());
        assert!(s.new_alternative("song").is_err(), "names are unique, ignoring case");

        // Edits stay with their alternative; switching saves first.
        s.set_tempo(90.0);
        s.dirty = true;
        s.switch_alternative("Song").unwrap();
        assert_eq!((s.alternative_name().as_str(), s.project.tempo), ("Song", 120.0));
        assert!(!s.dirty);
        assert!(!alt_dir(&dir, "Song").exists());
        s.switch_alternative("Rough Mix").unwrap();
        assert_eq!(s.project.tempo, 90.0);

        // Reopening the folder opens the active alternative.
        let (mut s2, _) = session("cycle2");
        s2.open(&dir).unwrap();
        assert_eq!((s2.alternative_name().as_str(), s2.project.tempo), ("Rough Mix", 90.0));
        assert_eq!(s2.alternatives().len(), 2);

        // Rename either one, backups included.
        s.set_tempo(95.0);
        s.save(None).unwrap();
        assert!(backup_dir(&dir, "Rough Mix").is_dir());
        s.rename_alternative("Rough Mix", "Final").unwrap();
        assert_eq!(s.alternative_name(), "Final");
        assert!(backup_dir(&dir, "Final").is_dir() && !backup_dir(&dir, "Rough Mix").exists());
        assert_eq!(read_doc(&dir.join(PROJECT_FILE)).unwrap().alternative, "Final");
        s.rename_alternative("Song", "First").unwrap();
        assert_eq!(s.alternatives(), vec!["Final".to_string(), "First".to_string()]);
        assert!(s.rename_alternative("First", "final").is_err());
        // Undo doesn't bring an old name back.
        s.checkpoint("Tempo", None, false);
        s.set_tempo(99.0);
        s.undo();
        assert_eq!(s.alternative_name(), "Final");

        // Delete the active one: the other takes over. The last one stays.
        s.delete_alternative("Final").unwrap();
        assert_eq!((s.alternative_name().as_str(), s.project.tempo), ("First", 120.0));
        assert_eq!(s.alternatives(), vec!["First".to_string()]);
        assert!(!backup_dir(&dir, "Final").exists());
        assert!(s.delete_alternative("First").is_err());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn deleting_an_inactive_alternative() {
        let (mut s, scratch) = session("inactive");
        s.save(None).unwrap();
        s.new_alternative("B").unwrap();
        s.new_alternative("C").unwrap();
        s.delete_alternative("B").unwrap();
        assert_eq!(s.alternatives(), vec!["C".to_string(), "Song".to_string()]);
        assert_eq!(s.alternative_name(), "C");
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
