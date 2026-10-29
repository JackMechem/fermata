//! Recently opened and saved projects, newest first, for the Project Chooser.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use daw_engine::plugin_db::config_dir;
use serde::Serialize;

const MAX: usize = 20;

fn file() -> PathBuf {
    config_dir().join("recent.json")
}

fn load() -> Vec<String> {
    std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn store(list: &[String]) {
    let _ = std::fs::create_dir_all(config_dir());
    if let Ok(t) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(file(), t);
    }
}

/// Moves `path` to the top of the list.
pub fn add(path: &Path) {
    let p = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().to_string();
    let mut list = load();
    list.retain(|x| *x != p);
    list.insert(0, p);
    list.truncate(MAX);
    store(&list);
}

/// The recent project folders, newest first.
pub fn paths() -> Vec<String> {
    load()
}

pub fn remove(path: &str) {
    let mut list = load();
    list.retain(|x| x != path);
    store(&list);
}

#[derive(Serialize)]
pub struct RecentProject {
    path: String,
    name: String,
    exists: bool,
    /// Seconds since the Unix epoch when project.json was last written.
    modified: Option<u64>,
    tracks: usize,
    tempo: Option<f64>,
    sample_rate: Option<u32>,
}

fn describe(path: &str) -> RecentProject {
    let p = Path::new(path);
    let doc = p.join("project.json");
    let json: Option<serde_json::Value> = std::fs::read_to_string(&doc).ok().and_then(|t| serde_json::from_str(&t).ok());
    let modified = std::fs::metadata(&doc).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs());
    RecentProject {
        path: path.to_string(),
        name: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string()),
        exists: json.is_some(),
        modified,
        tracks: json.as_ref().and_then(|j| j["tracks"].as_array()).map(|a| a.len()).unwrap_or(0),
        tempo: json.as_ref().and_then(|j| j["tempo"].as_f64()),
        sample_rate: json.as_ref().and_then(|j| j["sample_rate"].as_u64()).map(|v| v as u32),
    }
}

pub fn list() -> Vec<RecentProject> {
    load().iter().map(|p| describe(p)).collect()
}
