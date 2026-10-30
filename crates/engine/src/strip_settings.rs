//! Channel strip settings, as in Logic's channel strip Setting menu: a strip's instrument, its
//! Audio FX inserts (with their settings, a VST3's state included) and its Channel EQ, which
//! can be copied onto other strips or saved and loaded again in any project.
//!
//! - Copying and applying them is in `session/strip_settings.rs`; the clipboard lives in the UI
//!   (`src/stripSettings.ts`), like the plug-in clipboard.
//! - Saved settings are files, `<config dir>/channel-strips/<name>.json`, written like presets
//!   (`crate::presets`): to a temporary file first and renamed into place. They leave out
//!   side-chain sources, which name tracks of the project they were saved in.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::{ChannelEq, PluginSlot};

const VERSION: u32 = 1;

/// A channel strip setting. Instance ids (`uid`) in it mean nothing: applying it makes new
/// instances.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StripSetting {
    /// The strip it was copied from, or the name it was saved under.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: u32,
    /// The instrument, when copied from an instrument track that has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrument: Option<PluginSlot>,
    /// The Audio FX slots, in order; empty slots are None.
    #[serde(default)]
    pub inserts: Vec<Option<PluginSlot>>,
    /// The Channel EQ (None leaves the target's alone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eq: Option<ChannelEq>,
}

impl StripSetting {
    /// The setting without what only makes sense in the project it came from.
    pub fn portable(&self) -> StripSetting {
        let mut s = self.clone();
        for slot in s.instrument.iter_mut().chain(s.inserts.iter_mut().flatten()) {
            slot.sidechain = None;
        }
        // Trailing empty slots say nothing.
        while matches!(s.inserts.last(), Some(None)) {
            s.inserts.pop();
        }
        s
    }
}

/// Where saved channel strip settings live: `channel-strips` in the app's config folder.
pub fn user_root() -> PathBuf {
    crate::plugin_db::config_dir().join("channel-strips")
}

fn clean(name: &str) -> Result<String> {
    crate::presets::clean_name(name).map_err(|_| anyhow!("Type a name for the channel strip setting"))
}

fn file(root: &Path, name: &str) -> PathBuf {
    root.join(format!("{name}.json"))
}

/// The saved settings, by name.
pub fn list(root: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut names: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension().map(|x| x == "json").unwrap_or(false)).then(|| p.file_stem().map(|s| s.to_string_lossy().to_string())).flatten()
        })
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

pub fn read(root: &Path, name: &str) -> Result<StripSetting> {
    let name = clean(name)?;
    let text = std::fs::read_to_string(file(root, &name)).with_context(|| format!("No channel strip setting named \"{name}\""))?;
    let mut s: StripSetting = serde_json::from_str(&text).with_context(|| format!("The channel strip setting \"{name}\" can't be read"))?;
    // The file name is what the menu shows, whatever the file says.
    s.name = name;
    Ok(s)
}

/// Saves a setting under `name` (cleaned). Refuses to replace an existing one unless
/// `overwrite`. Returns the name it was saved under.
pub fn write(root: &Path, setting: &StripSetting, name: &str, overwrite: bool) -> Result<String> {
    let name = clean(name)?;
    let path = file(root, &name);
    if path.exists() && !overwrite {
        return Err(anyhow!("A channel strip setting named \"{name}\" already exists"));
    }
    std::fs::create_dir_all(root)?;
    let doc = StripSetting { name: name.clone(), version: VERSION, ..setting.portable() };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&doc)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(name)
}

pub fn delete(root: &Path, name: &str) -> Result<()> {
    let name = clean(name)?;
    std::fs::remove_file(file(root, &name)).with_context(|| format!("No channel strip setting named \"{name}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(uid: u64, id: &str) -> PluginSlot {
        PluginSlot { uid, plugin_id: id.into(), name: id.into(), bypass: false, state: Some("AAAA:BBBB".into()), params: vec![(1, 2.5)], sidechain: Some(7) }
    }

    #[test]
    fn saved_settings_round_trip() {
        let root = std::env::temp_dir().join(format!("fermata-strips-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut eq = ChannelEq::default();
        eq.bands[3].gain = -4.0;
        let s = StripSetting { name: "Lead".into(), version: 0, instrument: Some(slot(3, "builtin:synth")), inserts: vec![Some(slot(4, "builtin:compressor")), None, Some(slot(5, "/x.vst3|ABC")), None, None], eq: Some(eq) };
        let name = write(&root, &s, " Lead / Vocal? ", false).unwrap();
        assert_eq!(name, "Lead Vocal");
        assert!(write(&root, &s, "Lead Vocal", false).is_err(), "an existing setting isn't replaced silently");
        write(&root, &s, "Lead Vocal", true).unwrap();
        assert_eq!(list(&root), vec!["Lead Vocal".to_string()]);

        let back = read(&root, "Lead Vocal").unwrap();
        assert_eq!((back.name.as_str(), back.version), ("Lead Vocal", VERSION));
        assert_eq!(back.inserts.len(), 3, "trailing empty slots are dropped");
        assert!(back.inserts[1].is_none());
        let vst = back.inserts[2].as_ref().unwrap();
        assert_eq!((vst.plugin_id.as_str(), vst.state.as_deref(), vst.sidechain), ("/x.vst3|ABC", Some("AAAA:BBBB"), None), "state kept, side-chain left out");
        assert_eq!(back.instrument.as_ref().unwrap().params, vec![(1, 2.5)]);
        assert_eq!(back.eq.unwrap().bands[3].gain, -4.0);

        // A file written by hand with only some of the parts reads.
        std::fs::write(root.join("Bare.json"), r#"{"inserts":[null,{"uid":1,"plugin_id":"builtin:limiter","name":"Limiter"}]}"#).unwrap();
        let bare = read(&root, "Bare").unwrap();
        assert!(bare.instrument.is_none() && bare.eq.is_none() && bare.inserts[1].is_some());

        delete(&root, "Lead Vocal").unwrap();
        assert_eq!(list(&root), vec!["Bare".to_string()]);
        assert!(read(&root, "Lead Vocal").is_err());
        assert!(write(&root, &s, "  ", false).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
