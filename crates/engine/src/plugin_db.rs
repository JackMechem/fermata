//! The plug-in database. Bundles are scanned in child processes (`<exe> --scan-vst3 <bundle>`)
//! so a crashing plugin cannot take the application down, and results are cached on disk.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use vst3_host::scan::{default_search_paths, find_bundles};
use serde::{Deserialize, Serialize};
use vst3_host::PluginDescriptor;

pub const SCAN_ARG: &str = "--scan-vst3";

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("fermata")
}

fn cache_path() -> PathBuf {
    config_dir().join("plugins.json")
}

pub fn load_cache() -> Option<Vec<PluginDescriptor>> {
    let text = std::fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_cache(list: &[PluginDescriptor]) {
    let _ = std::fs::create_dir_all(config_dir());
    if let Ok(text) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(cache_path(), text);
    }
}

/// Entry point for the child process: prints the bundle's descriptors as JSON.
pub fn scan_child_main(bundle: &Path) -> i32 {
    match vst3_host::scan::scan_bundle(bundle) {
        Ok(list) => {
            println!("{}", serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()));
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// Scans one bundle in a child process. Errors say why nothing came back.
fn scan_one(exe: &Path, bundle: &Path) -> Result<Vec<PluginDescriptor>, String> {
    let child = Command::new(exe)
        .arg(SCAN_ARG)
        .arg(bundle)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = child.map_err(|e| format!("could not start the scanner: {e}"))?;
    // Both pipes are read while the child runs: a chatty plug-in would otherwise fill one,
    // block, and look like a hang.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_string(&mut text);
            }
            text
        })
    };
    let out_reader = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err_reader = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() > deadline => {
                log::warn!("plugin scan timed out: {}", bundle.display());
                let _ = child.kill();
                // Reap it, which also closes its pipes so the reader threads finish.
                let _ = child.wait();
                return Err("timed out after 20 s".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    };
    let out = out_reader.join().unwrap_or_default();
    // Plugins sometimes print to stdout themselves; the JSON is the last line.
    if let Some(list) = out.lines().rev().find_map(|l| serde_json::from_str::<Vec<PluginDescriptor>>(l).ok()) {
        return if list.is_empty() { Err("no plug-ins in the bundle".into()) } else { Ok(list) };
    }
    let err = err_reader.join().unwrap_or_default();
    let detail = err.lines().rev().find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string());
    Err(match (status.code(), detail) {
        (_, Some(d)) => d,
        (Some(c), None) => format!("scanner exited with code {c}"),
        (None, None) => "the plug-in crashed while loading".into(),
    })
}

/// A bundle that could not be scanned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanFailure {
    pub path: String,
    pub reason: String,
}

/// The outcome of the last full scan, shown in Settings › Plug-ins.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanReport {
    /// Seconds since the Unix epoch.
    pub at: u64,
    pub bundles: usize,
    pub plugins: usize,
    pub failed: Vec<ScanFailure>,
    /// Folders that were searched.
    pub folders: Vec<String>,
}

fn report_path() -> PathBuf {
    config_dir().join("plugin-scan.json")
}

pub fn load_report() -> Option<ScanReport> {
    serde_json::from_str(&std::fs::read_to_string(report_path()).ok()?).ok()
}

/// The standard VST3 folders, followed by the user's own (missing ones are skipped).
pub fn search_paths(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = default_search_paths();
    for p in extra {
        if !paths.contains(p) {
            paths.push(p.clone());
        }
    }
    paths
}

pub fn scan_all(exe: &Path) -> Vec<PluginDescriptor> {
    scan_with(exe, &[])
}

/// Scans the standard folders plus `extra`, caching the result and writing a scan report.
pub fn scan_with(exe: &Path, extra: &[PathBuf]) -> Vec<PluginDescriptor> {
    let folders = search_paths(extra);
    let bundles = find_bundles(&folders);
    let count = bundles.len();
    let results = std::sync::Mutex::new(Vec::new());
    let failed = std::sync::Mutex::new(Vec::new());
    let queue = std::sync::Mutex::new(bundles.into_iter());
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let next = queue.lock().unwrap().next();
                let Some(b) = next else { break };
                match scan_one(exe, &b) {
                    Ok(found) => results.lock().unwrap().extend(found),
                    Err(reason) => failed.lock().unwrap().push(ScanFailure { path: b.to_string_lossy().to_string(), reason }),
                }
            });
        }
    });
    let mut list = results.into_inner().unwrap();
    list.sort_by(|a, b| a.vendor.to_lowercase().cmp(&b.vendor.to_lowercase()).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    save_cache(&list);
    let mut failed = failed.into_inner().unwrap();
    failed.sort_by(|a, b| a.path.cmp(&b.path));
    let report = ScanReport {
        at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        bundles: count,
        plugins: list.len(),
        failed,
        folders: folders.iter().map(|p| p.to_string_lossy().to_string()).collect(),
    };
    if let Ok(text) = serde_json::to_string_pretty(&report) {
        let _ = std::fs::write(report_path(), text);
    }
    list
}
