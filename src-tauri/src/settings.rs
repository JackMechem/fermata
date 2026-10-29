//! Application settings persisted between launches, in `~/.config/fermata/settings.json`.
//!
//! Every section and field falls back to its default, so files written by older versions load.

use std::collections::BTreeMap;

use daw_engine::audio_io::AudioConfig;
use daw_engine::plugin_db::config_dir;
use daw_engine::prefs::Prefs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub audio: AudioConfig,
    /// Behaviour of the engine session (General, Recording, MIDI, Editing).
    #[serde(default)]
    pub prefs: Prefs,
    /// Options only the interface uses.
    #[serde(default)]
    pub ui: UiPrefs,
    #[serde(default)]
    pub plugins: PluginPrefs,
}

/// What Fermata shows at launch when no project was given on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Startup {
    #[default]
    Chooser,
    LastProject,
    EmptyProject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    pub startup: Startup,
    /// Ask before quitting, closing or opening over unsaved changes.
    pub confirm_unsaved: bool,
    /// Snap mode of new windows: "smart", "bar", "beat" or "off".
    pub snap: String,
    /// Nudge step for the Nudge Left/Right key commands: "bar", "beat", "8", "16", "32",
    /// "ms10" or "ms1".
    pub nudge: String,
    /// Key command overrides: command id -> key strings (an empty list removes the default).
    pub keymap: BTreeMap<String, Vec<String>>,
    /// Settings › Appearance: theme id, interface scale and waveform style (None until the
    /// frontend first saves them; it keeps a localStorage copy for the first paint).
    pub theme: Option<String>,
    pub ui_scale: Option<f64>,
    pub wave_style: Option<String>,
    /// Settings › Editing: what a click in the tracks area does to the playhead: "anywhere"
    /// (empty space and regions), "empty" (empty space only) or "off".
    pub click_locates: String,
}

impl Default for UiPrefs {
    fn default() -> Self {
        UiPrefs {
            startup: Startup::Chooser,
            confirm_unsaved: true,
            snap: "smart".into(),
            nudge: "16".into(),
            keymap: BTreeMap::new(),
            theme: None,
            ui_scale: None,
            wave_style: None,
            click_locates: "anywhere".into(),
        }
    }
}

/// Settings › Plug-ins (Logic's Plug-in Manager).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginPrefs {
    /// Descriptor ids hidden from the Library and the slot menus.
    pub disabled: Vec<String>,
    /// Folders scanned in addition to the standard VST3 locations.
    pub folders: Vec<String>,
}

/// The extra VST3 folders from Settings › Plug-ins.
pub fn extra_plugin_folders() -> Vec<std::path::PathBuf> {
    load().plugins.folders.iter().filter(|f| !f.trim().is_empty()).map(std::path::PathBuf::from).collect()
}

pub fn load() -> Settings {
    std::fs::read_to_string(config_dir().join("settings.json"))
        .ok()
        .and_then(|t| parse(&t))
        .unwrap_or_default()
}

/// Reads a settings file, keeping what can be read even if one section is malformed.
fn parse(text: &str) -> Option<Settings> {
    if let Ok(s) = serde_json::from_str(text) {
        return Some(s);
    }
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    fn section<T: serde::de::DeserializeOwned + Default>(v: &serde_json::Value, k: &str) -> T {
        v.get(k).cloned().and_then(|x| serde_json::from_value(x).ok()).unwrap_or_default()
    }
    Some(Settings { audio: section(&v, "audio"), prefs: section(&v, "prefs"), ui: section(&v, "ui"), plugins: section(&v, "plugins") })
}

pub fn save(s: &Settings) {
    let _ = std::fs::create_dir_all(config_dir());
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let path = config_dir().join("settings.json");
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_files_load_with_defaults() {
        // A settings file from before the preferences existed: only the audio section.
        let s = parse(r#"{"audio":{"host":null,"input":null,"output":null,"sample_rate":44100,"buffer_size":128,"input_enabled":true}}"#).unwrap();
        assert_eq!(s.audio.sample_rate, 44100);
        assert_eq!(s.prefs, Prefs::default());
        assert_eq!(s.ui, UiPrefs::default());
        assert!(s.ui.confirm_unsaved);
        assert_eq!(s.plugins, PluginPrefs::default());
    }

    #[test]
    fn a_bad_section_does_not_lose_the_others() {
        let s = parse(r#"{"audio":{"sample_rate":"oops"},"ui":{"startup":"last_project","nudge":"beat"}}"#).unwrap();
        assert_eq!(s.ui.startup, Startup::LastProject);
        assert_eq!(s.ui.nudge, "beat");
        assert_eq!(s.ui.snap, "smart");
        assert_eq!(s.audio.sample_rate, AudioConfig::default().sample_rate);
    }

    #[test]
    fn round_trip() {
        let mut s = Settings::default();
        s.ui.keymap.insert("play".into(), vec!["Shift+Space".into()]);
        s.plugins.disabled.push("/x.vst3|abc".into());
        s.prefs.general.autosave_minutes = 7;
        let back = parse(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.ui, s.ui);
        assert_eq!(back.plugins, s.plugins);
        assert_eq!(back.prefs, s.prefs);
    }
}
