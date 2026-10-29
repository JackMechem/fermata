//! Finding VST3 bundles on disk.

use std::path::{Path, PathBuf};

use crate::module::{load_module, PluginDescriptor};

pub fn default_search_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(v) = std::env::var_os("VST3_PATH") {
        out.extend(std::env::split_paths(&v));
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(h) = &home {
            out.push(h.join(".vst3"));
            out.push(h.join(".nix-profile/lib/vst3"));
        }
        out.push("/usr/lib/vst3".into());
        out.push("/usr/local/lib/vst3".into());
        out.push("/run/current-system/sw/lib/vst3".into());
        if let Ok(user) = std::env::var("USER") {
            out.push(format!("/etc/profiles/per-user/{user}/lib/vst3").into());
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(h) = &home {
            out.push(h.join("Library/Audio/Plug-Ins/VST3"));
        }
        out.push("/Library/Audio/Plug-Ins/VST3".into());
    }
    #[cfg(target_os = "windows")]
    {
        let common = std::env::var_os("CommonProgramFiles").map(PathBuf::from).unwrap_or_else(|| "C:\\Program Files\\Common Files".into());
        out.push(common.join("VST3"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            out.push(PathBuf::from(l).join("Programs/Common/VST3"));
        }
    }
    let _ = home;
    out.dedup();
    out
}

/// Recursively finds `*.vst3` bundles (directories or single files) without descending into them.
pub fn find_bundles(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        walk(p, &mut out, 0);
    }
    // The same bundle can be reachable via several profile symlinks on NixOS.
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| {
        let key = p.file_name().map(|n| n.to_os_string());
        seen.insert(key)
    });
    out.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().map(|x| x.eq_ignore_ascii_case("vst3")).unwrap_or(false) {
            out.push(p);
        } else if p.is_dir() {
            walk(&p, out, depth + 1);
        }
    }
}

/// Loads a bundle in-process and lists its classes. Prefer running this in a child process so a
/// misbehaving plugin cannot take the host down during a scan.
pub fn scan_bundle(path: &Path) -> Result<Vec<PluginDescriptor>, String> {
    Ok(load_module(path)?.descriptors())
}
