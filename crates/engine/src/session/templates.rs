//! User project templates (Logic's File › Save as Template…): a project's tracks, routing,
//! plug-ins with their settings, and project settings, without regions or audio, saved as
//! `templates/<name>.json` in the config folder. The Project Chooser lists them under My
//! Templates, next to the built-in ones, and starts new projects from them.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{anyhow, Result};
use serde::Serialize;

use super::alternatives::check_name;
use super::recovery::write_atomic;
use super::Session;
use crate::model::Project;

/// Where user templates live: `templates` in the config folder.
pub fn templates_dir() -> PathBuf {
    crate::plugin_db::config_dir().join("templates")
}

/// A user template, as the Project Chooser lists it.
#[derive(Debug, Clone, Serialize)]
pub struct UserTemplate {
    pub name: String,
    pub tracks: usize,
    /// Track kinds in order ("audio", "instrument", …), for the chooser's miniature.
    pub kinds: Vec<crate::model::TrackKind>,
    pub tempo: f64,
    /// When the template was saved, in seconds since the Unix epoch.
    pub modified: u64,
}

fn template_path(folder: &Path, name: &str) -> PathBuf {
    folder.join(format!("{name}.json"))
}

/// The templates in `folder`, by name.
pub fn list_templates(folder: &Path) -> Vec<UserTemplate> {
    let mut out: Vec<UserTemplate> = std::fs::read_dir(folder)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let path = e.path();
                    let name = path.file_name()?.to_string_lossy().strip_suffix(".json")?.to_string();
                    if name.starts_with('.') {
                        return None;
                    }
                    let doc: Project = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
                    let modified = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                    Some(UserTemplate { name, tracks: doc.tracks.len(), kinds: doc.tracks.iter().map(|t| t.kind).collect(), tempo: doc.tempo, modified })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|t| t.name.to_lowercase());
    out
}

/// Deletes template `name` from `folder`.
pub fn delete_template(folder: &Path, name: &str) -> Result<()> {
    std::fs::remove_file(template_path(folder, name)).map_err(|e| anyhow!("Couldn't delete the template “{name}”: {e}"))
}

/// Renames template `name` in `folder`.
pub fn rename_template(folder: &Path, name: &str, new_name: &str) -> Result<()> {
    let new_name = check_name(new_name)?;
    if new_name == name {
        return Ok(());
    }
    let (from, to) = (template_path(folder, name), template_path(folder, &new_name));
    // A change of case only finds the template itself where names ignore case.
    if to.exists() && new_name.to_lowercase() != name.to_lowercase() {
        return Err(anyhow!("There's already a template named “{new_name}”."));
    }
    let mut doc: Project = serde_json::from_slice(&std::fs::read(&from)?)?;
    doc.name = new_name.clone();
    write_atomic(&from, serde_json::to_string_pretty(&doc)?.as_bytes(), true)?;
    std::fs::rename(&from, &to)?;
    Ok(())
}

impl Session {
    /// The project as a template: everything but regions, audio, freeze files and
    /// alternatives, with plug-in settings.
    pub fn template_doc(&self, name: &str) -> Project {
        let mut doc = self.capture_plugin_states();
        doc.name = name.to_string();
        doc.alternative = String::new();
        doc.clips.clear();
        for t in &mut doc.tracks {
            t.regions.clear();
            t.alternatives.clear();
            t.alternative.clear();
            // Its plug-ins come back live; the settings were saved when it was frozen.
            t.frozen = None;
        }
        doc
    }

    /// File › Save as Template: saves the project as template `name` in `folder` (see
    /// [`templates_dir`]). An existing template of that name is only replaced with `replace`.
    pub fn save_template(&self, folder: &Path, name: &str, replace: bool) -> Result<PathBuf> {
        let name = check_name(name)?;
        let path = template_path(folder, &name);
        if path.exists() && !replace {
            return Err(anyhow!("There's already a template named “{name}”."));
        }
        let doc = self.template_doc(&name);
        write_atomic(&path, serde_json::to_string_pretty(&doc)?.as_bytes(), true)?;
        Ok(path)
    }

    /// Starts a new, untitled project from template `name` in `folder`.
    pub fn new_from_template(&mut self, folder: &Path, name: &str) -> Result<()> {
        let path = template_path(folder, name);
        let mut doc: Project = serde_json::from_slice(&std::fs::read(&path).map_err(|e| anyhow!("The template “{name}” can't be read: {e}"))?)
            .map_err(|e| anyhow!("The template “{name}” is damaged: {e}"))?;
        self.new_project();
        doc.name = self.project.name.clone();
        doc.alternative = String::new();
        // No audio to keep at the template's rate: the project runs at the device's.
        doc.sample_rate = self.sample_rate() as u32;
        self.project = doc;
        self.project.prune_routes();
        self.project.normalize_tree();
        self.selected = self.project.tracks.first().map(|t| t.id);
        self.rebuild_graph();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::model::{InputSel, TrackKind};

    fn session() -> Session {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        Session::new(cfg, Vec::new())
    }

    #[test]
    fn template_round_trip() {
        let folder = std::env::temp_dir().join(format!("fermata-templates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut s = session();
        let a = s.add_track(TrackKind::Audio, InputSel::Mono(1), None, None).unwrap();
        let i = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        s.update_track(a, crate::session::TrackPatch { name: Some("Vocals".into()), volume_db: Some(-6.0), ..Default::default() });
        s.set_tempo(87.0);
        s.project.track_mut(a).unwrap().regions.push(crate::model::Region {
            id: 9000,
            name: "Take".into(),
            start: 0,
            length: 48000,
            muted: false,
            color: None,
            content: crate::model::RegionContent::Audio { clip: 1, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() },
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: None,
        });
        s.project.next_id = 9001;
        assert!(!s.project.track(a).unwrap().regions.is_empty());

        s.save_template(&folder, "Band", false).unwrap();
        assert!(s.save_template(&folder, "Band", false).is_err());
        s.save_template(&folder, "Band", true).unwrap();
        let list = list_templates(&folder);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].name.as_str(), list[0].tracks, list[0].tempo), ("Band", 2, 87.0));
        assert_eq!(list[0].kinds, vec![TrackKind::Audio, TrackKind::Instrument]);

        let mut s2 = session();
        s2.new_from_template(&folder, "Band").unwrap();
        assert!(!s2.saved && !s2.dirty);
        assert_eq!(s2.project.name, "Untitled");
        assert_eq!(s2.project.tempo, 87.0);
        let v = s2.project.track(a).expect("same track ids");
        assert_eq!((v.name.as_str(), v.volume_db, v.input), ("Vocals", -6.0, InputSel::Mono(1)));
        assert!(v.regions.is_empty() && s2.project.clips.is_empty());
        assert!(s2.project.track(i).unwrap().instrument.is_some());
        // New ids don't collide with the template's.
        assert!(s2.project.alloc_id() > i);

        rename_template(&folder, "Band", "Trio").unwrap();
        assert_eq!(list_templates(&folder).iter().map(|t| t.name.clone()).collect::<Vec<_>>(), vec!["Trio".to_string()]);
        assert!(rename_template(&folder, "Trio", "a/b").is_err());
        delete_template(&folder, "Trio").unwrap();
        assert!(list_templates(&folder).is_empty());
        assert!(s2.new_from_template(&folder, "Trio").is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
