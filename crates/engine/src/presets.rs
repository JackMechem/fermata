//! Presets for the Channel EQ, the built-in effects and the built-in instruments, shared by every
//! project.
//!
//! - Factory presets ship with the code: `fx::factory_presets` for the effects,
//!   `InstrumentInfo::presets` for the instruments and [`eq_factory`] for the Channel EQ.
//! - User presets are files, `<config dir>/presets/<effect>/<name>.json` (the effect id with
//!   anything unsafe in a file name replaced, so "builtin:deesser" becomes "builtin_deesser").
//!   Each holds its name, effect id and format version alongside the settings, and is written
//!   to a temporary file first and renamed into place, so a crash never leaves half a preset.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::{BandKind, ChannelEq, EqBand};

/// The preset id of the Channel EQ (it isn't in the effect registry).
pub const CHANNEL_EQ: &str = "builtin:channel-eq";
const VERSION: u32 = 1;
const MAX_NAME: usize = 64;

/// A preset as saved (and as the UI gets it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub effect: String,
    #[serde(default)]
    pub version: u32,
    /// A built-in effect's values: (parameter id, plain value). Unlisted ones are defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<(u32, f32)>,
    /// The Channel EQ's settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eq: Option<ChannelEq>,
}

/// The presets offered for one effect.
#[derive(Debug, Clone, Serialize, Default)]
pub struct PresetList {
    pub factory: Vec<String>,
    pub user: Vec<String>,
}

/// Where user presets live: `presets` in the app's config folder.
pub fn user_root() -> PathBuf {
    crate::plugin_db::config_dir().join("presets")
}

/// A preset name made safe to use as a file name: trimmed, without path separators or other
/// characters file systems refuse, at most 64 characters.
pub fn clean_name(name: &str) -> Result<String> {
    let cleaned: String = name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { ' ' } else { c }).collect();
    let mut out = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    out = out.trim_start_matches('.').trim().to_string();
    if out.chars().count() > MAX_NAME {
        out = out.chars().take(MAX_NAME).collect::<String>().trim_end().to_string();
    }
    if out.is_empty() {
        return Err(anyhow!("Type a name for the preset"));
    }
    Ok(out)
}

fn effect_dir(root: &Path, effect: &str) -> PathBuf {
    let safe: String = effect.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    root.join(safe)
}

fn file(root: &Path, effect: &str, name: &str) -> PathBuf {
    effect_dir(root, effect).join(format!("{name}.json"))
}

/// The user presets saved for an effect, by name.
pub fn list_user(root: &Path, effect: &str) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(effect_dir(root, effect)) else { return Vec::new() };
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

pub fn read_user(root: &Path, effect: &str, name: &str) -> Result<Preset> {
    let name = clean_name(name)?;
    let path = file(root, effect, &name);
    let text = std::fs::read_to_string(&path).with_context(|| format!("No preset named \"{name}\""))?;
    let mut p: Preset = serde_json::from_str(&text).with_context(|| format!("The preset \"{name}\" can't be read"))?;
    // The file name is what the list shows, whatever the file says.
    p.name = name;
    Ok(p)
}

/// Saves a user preset (named by `p.name`, cleaned). Refuses to replace an existing one unless
/// `overwrite`. Returns the name it was saved under.
pub fn write_user(root: &Path, p: &Preset, overwrite: bool) -> Result<String> {
    let name = clean_name(&p.name)?;
    let path = file(root, &p.effect, &name);
    if path.exists() && !overwrite {
        return Err(anyhow!("A preset named \"{name}\" already exists"));
    }
    std::fs::create_dir_all(effect_dir(root, &p.effect))?;
    let doc = Preset { name: name.clone(), version: VERSION, ..p.clone() };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&doc)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(name)
}

pub fn delete_user(root: &Path, effect: &str, name: &str) -> Result<()> {
    let name = clean_name(name)?;
    std::fs::remove_file(file(root, effect, &name)).with_context(|| format!("No preset named \"{name}\""))
}

/// Renames a user preset; refuses a name that is already taken.
pub fn rename_user(root: &Path, effect: &str, old: &str, new: &str) -> Result<String> {
    let mut p = read_user(root, effect, old)?;
    let new_name = clean_name(new)?;
    if new_name == p.name {
        return Ok(new_name);
    }
    // Case-only renames are the same file on some systems.
    let same_file = new_name.to_lowercase() == p.name.to_lowercase();
    if !same_file && file(root, effect, &new_name).exists() {
        return Err(anyhow!("A preset named \"{new_name}\" already exists"));
    }
    let old_name = std::mem::replace(&mut p.name, new_name);
    let saved = write_user(root, &p, same_file)?;
    if !same_file {
        std::fs::remove_file(file(root, effect, &old_name))?;
    }
    Ok(saved)
}

/// The factory presets of an effect (or the Channel EQ), complete.
pub fn factory(effect: &str) -> Vec<Preset> {
    if effect == CHANNEL_EQ {
        return eq_factory().into_iter().map(|(name, eq)| Preset { name: name.into(), effect: effect.into(), version: VERSION, values: Vec::new(), eq: Some(eq) }).collect();
    }
    let preset = |name: &str, values: &[(u32, f32)]| Preset { name: name.into(), effect: effect.into(), version: VERSION, values: values.to_vec(), eq: None };
    // Every built-in effect and instrument starts with its defaults, as Logic's "#default".
    if let Some(info) = crate::instrument::find(effect) {
        let own = info.presets.iter().map(|f| preset(f.name, f.values));
        return std::iter::once(preset(DEFAULT, &[])).chain(own).collect();
    }
    let default = crate::fx::find(effect).map(|_| preset(DEFAULT, &[]));
    default.into_iter().chain(crate::fx::factory_presets(effect).iter().map(|f| preset(f.name, f.values))).collect()
}

/// The factory preset that puts a built-in effect back to its defaults ("Flat" for the EQ).
pub const DEFAULT: &str = "Default";

/// Lists an effect's factory and user presets.
pub fn list(root: &Path, effect: &str) -> PresetList {
    PresetList { factory: factory(effect).into_iter().map(|p| p.name).collect(), user: list_user(root, effect) }
}

/// Finds a factory or user preset.
pub fn get(root: &Path, effect: &str, name: &str, factory_preset: bool) -> Result<Preset> {
    if factory_preset {
        factory(effect).into_iter().find(|p| p.name == name).ok_or_else(|| anyhow!("No factory preset named \"{name}\""))
    } else {
        read_user(root, effect, name)
    }
}

/// The Channel EQ's factory presets: the default bands moved into place.
pub fn eq_factory() -> Vec<(&'static str, ChannelEq)> {
    // Band order: low cut, low shelf, four bells, high shelf, high cut.
    let with = |f: &dyn Fn(&mut [EqBand])| {
        let mut eq = ChannelEq::default();
        f(&mut eq.bands);
        eq
    };
    let set = |b: &mut EqBand, on: bool, freq: f32, gain: f32, q: f32| {
        b.on = on;
        b.freq = freq;
        b.gain = gain;
        b.q = q;
    };
    debug_assert!(matches!(ChannelEq::default().bands[0].kind, BandKind::LowCut));
    vec![
        ("Flat", ChannelEq::default()),
        (
            "Low Cut 80 Hz",
            with(&|b| {
                set(&mut b[0], true, 80.0, 0.0, 0.71);
                b[0].slope = 24;
            }),
        ),
        (
            "Vocal Presence",
            with(&|b| {
                set(&mut b[0], true, 90.0, 0.0, 0.71);
                set(&mut b[3], true, 350.0, -2.0, 1.2);
                set(&mut b[5], true, 3500.0, 3.0, 0.9);
            }),
        ),
        (
            "Vocal Air",
            with(&|b| {
                set(&mut b[0], true, 100.0, 0.0, 0.71);
                set(&mut b[5], true, 5000.0, 1.5, 1.0);
                set(&mut b[6], true, 12000.0, 4.0, 0.71);
            }),
        ),
        (
            "Kick Punch",
            with(&|b| {
                set(&mut b[0], true, 30.0, 0.0, 0.71);
                set(&mut b[2], true, 60.0, 4.0, 1.2);
                set(&mut b[3], true, 380.0, -5.0, 1.4);
                set(&mut b[5], true, 4000.0, 4.0, 1.1);
            }),
        ),
        (
            "Snare Crack",
            with(&|b| {
                set(&mut b[0], true, 80.0, 0.0, 0.71);
                set(&mut b[2], true, 200.0, 3.0, 1.2);
                set(&mut b[3], true, 800.0, -3.0, 1.5);
                set(&mut b[5], true, 5000.0, 4.0, 1.0);
            }),
        ),
        (
            "Bass Clarity",
            with(&|b| {
                set(&mut b[0], true, 35.0, 0.0, 0.71);
                set(&mut b[3], true, 250.0, -3.0, 1.2);
                set(&mut b[4], true, 800.0, 2.5, 1.2);
                set(&mut b[7], true, 8000.0, 0.0, 0.71);
            }),
        ),
        (
            "Acoustic Guitar",
            with(&|b| {
                set(&mut b[0], true, 80.0, 0.0, 0.71);
                set(&mut b[3], true, 220.0, -3.0, 1.0);
                set(&mut b[5], true, 3000.0, 2.0, 1.0);
                set(&mut b[6], true, 10000.0, 2.5, 0.71);
            }),
        ),
        (
            "Telephone",
            with(&|b| {
                set(&mut b[0], true, 400.0, 0.0, 0.71);
                b[0].slope = 48;
                set(&mut b[4], true, 1500.0, 4.0, 1.0);
                set(&mut b[7], true, 3200.0, 0.0, 0.71);
                b[7].slope = 48;
            }),
        ),
        (
            "Master Polish",
            with(&|b| {
                set(&mut b[0], true, 25.0, 0.0, 0.71);
                set(&mut b[1], true, 80.0, 1.0, 0.71);
                set(&mut b[3], true, 300.0, -1.0, 0.8);
                set(&mut b[6], true, 12000.0, 1.5, 0.71);
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fermata-presets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn user_presets_round_trip() {
        let root = temp_root("rt");
        let p = Preset { name: "My: Kick/Gate?".into(), effect: "builtin:noise-gate".into(), version: 0, values: vec![(0, -31.5), (4, 120.0)], eq: None };
        let name = write_user(&root, &p, false).unwrap();
        assert_eq!(name, "My Kick Gate");
        assert!(root.join("builtin_noise-gate").join("My Kick Gate.json").exists());
        assert!(write_user(&root, &p, false).is_err(), "an existing preset isn't replaced silently");
        write_user(&root, &Preset { values: vec![(0, -20.0)], ..p.clone() }, true).unwrap();
        assert_eq!(list(&root, "builtin:noise-gate").user, vec!["My Kick Gate".to_string()]);
        let back = get(&root, "builtin:noise-gate", "My Kick Gate", false).unwrap();
        assert_eq!((back.values.clone(), back.version), (vec![(0, -20.0)], VERSION));
        rename_user(&root, "builtin:noise-gate", "My Kick Gate", "Kick").unwrap();
        assert_eq!(list_user(&root, "builtin:noise-gate"), vec!["Kick".to_string()]);
        delete_user(&root, "builtin:noise-gate", "Kick").unwrap();
        assert!(list_user(&root, "builtin:noise-gate").is_empty());
        let eq = Preset { name: "Bright".into(), effect: CHANNEL_EQ.into(), version: 0, values: Vec::new(), eq: Some(eq_factory()[3].1.clone()) };
        write_user(&root, &eq, false).unwrap();
        assert_eq!(get(&root, CHANNEL_EQ, "Bright", false).unwrap().eq.unwrap().bands[6].gain, 4.0);
        assert!(clean_name(" ..  ").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn factory_presets_are_valid() {
        assert!(factory(CHANNEL_EQ).len() >= 8);
        assert!(factory("builtin:noise-gate").len() >= 6 && factory("builtin:deesser").len() >= 6);
        assert_eq!(factory("builtin:deesser")[0].name, DEFAULT);
        for e in crate::fx::registry() {
            for p in factory(e.id) {
                for (id, v) in &p.values {
                    let spec = e.param(*id).unwrap_or_else(|| panic!("{}: {} sets unknown {id}", e.id, p.name));
                    assert!(*v >= spec.min && *v <= spec.max, "{}: {} {} out of range", e.id, p.name, spec.name);
                }
            }
        }
    }
}
