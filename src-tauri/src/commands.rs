//! Tauri commands. Synchronous commands run on the main thread, which is where VST3 wants its
//! controllers and editors to live.

use std::path::PathBuf;

use daw_engine::audio_io::{list_devices, AudioConfig, AudioStatus, DeviceList};
use daw_engine::fades::{Crossfade, DragMode, FadeInType, FadeOutType};
use daw_engine::model::{AutoMode, AutoParam, AutoPoint, ChannelEq, Group, Id, InputSel, Project, Region, TrackKind, TrackSend};
use daw_engine::prefs::{BitDepth, ClickSettings, Prefs};
use daw_engine::session::{BipOptions, BipSource, BounceResult, ClipInfo, MidiContent, PdcReport, RangeEdit, RenderOptions, TrackPatch};
use daw_engine::vst3_host::{ParamInfo, PluginDescriptor};
use daw_engine::tempo::{SigEvent, TempoEvent};
use daw_engine::{plugin_db, transients, Session};
use serde::Serialize;
use tauri::{Emitter, State};

use crate::AppState;

type R<T> = Result<T, String>;

#[derive(Serialize, Clone)]
pub struct Snapshot {
    project: Project,
    selected: Option<Id>,
    path: String,
    saved: bool,
    dirty: bool,
    audio: AudioStatus,
    midi_inputs: Vec<String>,
    open_editors: Vec<Id>,
    midi_target: Option<String>,
    undo: Option<String>,
    redo: Option<String>,
    /// Plug-in latency per strip and the total compensation.
    pdc: PdcReport,
    /// Why the edit was refused and undone (it touched a frozen track).
    refused: Option<String>,
    /// Something the command did went wrong without failing it (the editor of a plug-in just
    /// inserted didn't open); shown as a toast.
    warning: Option<String>,
    /// Where regions crossfade, for drawing.
    crossfades: Vec<Crossfade>,
    /// Instance ids of VST3 slots whose plug-in isn't installed (or didn't load).
    missing: Vec<Id>,
}

pub fn snapshot(s: &Session) -> Snapshot {
    Snapshot {
        project: s.project.for_ui(),
        selected: s.selected,
        path: s.project_path().to_string_lossy().to_string(),
        saved: s.saved,
        dirty: s.dirty,
        audio: s.audio_status.clone(),
        midi_inputs: s.midi_inputs(),
        open_editors: s.open_editors(),
        midi_target: s.midi_target_name(),
        undo: s.undo_label(),
        redo: s.redo_label(),
        pdc: s.pdc_report(),
        refused: None,
        warning: None,
        crossfades: s.crossfades(),
        missing: s.missing_plugins().iter().map(|p| p.uid).collect(),
    }
}

fn with<T>(state: &State<AppState>, f: impl FnOnce(&mut Session) -> T) -> T {
    f(&mut state.session.lock())
}

fn snap(state: &State<AppState>, f: impl FnOnce(&mut Session)) -> Snapshot {
    with(state, |s| {
        f(s);
        snapshot(s)
    })
}

/// Like `snap`, recording an undo step first.
fn edit(state: &State<AppState>, label: &str, key: Option<String>, plugins: bool, f: impl FnOnce(&mut Session)) -> Snapshot {
    with(state, |s| {
        s.checkpoint(label, key, plugins);
        f(s);
        // Frozen tracks' regions and plug-ins can't change; such an edit is put back.
        let refused = s.frozen_guard().err().map(|e| e.to_string());
        Snapshot { refused, ..snapshot(s) }
    })
}

fn try_edit(state: &State<AppState>, label: &str, key: Option<String>, plugins: bool, f: impl FnOnce(&mut Session) -> anyhow::Result<()>) -> R<Snapshot> {
    try_snap(state, |s| {
        let pushed = s.checkpoint(label, key, plugins);
        // A failed edit leaves no undo step behind.
        if let Err(e) = f(s) {
            if pushed {
                s.abandon_checkpoint();
            }
            return Err(e);
        }
        s.frozen_guard()
    })
}

fn try_snap(state: &State<AppState>, f: impl FnOnce(&mut Session) -> anyhow::Result<()>) -> R<Snapshot> {
    with(state, |s| {
        f(s).map_err(|e| e.to_string())?;
        Ok(snapshot(s))
    })
}

/// A snapshot plus the ids of what an edit created (new regions or tracks), so the UI can
/// select them.
#[derive(Serialize)]
pub struct Made {
    snapshot: Snapshot,
    ids: Vec<Id>,
}

fn edit_made(state: &State<AppState>, label: &str, plugins: bool, f: impl FnOnce(&mut Session) -> anyhow::Result<Vec<Id>>) -> R<Made> {
    with(state, |s| {
        s.checkpoint(label, None, plugins);
        let ids = f(s).map_err(|e| {
            s.abandon_checkpoint();
            e.to_string()
        })?;
        s.frozen_guard().map_err(|e| e.to_string())?;
        Ok(Made { snapshot: snapshot(s), ids })
    })
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n > 1 { many.into() } else { one.into() }
}

#[tauri::command]
pub fn get_state(state: State<AppState>) -> Snapshot {
    snap(&state, |_| {})
}

/// The plug-ins offered in the Library and slot menus (the ones not switched off in
/// Settings › Plug-ins).
#[tauri::command]
pub fn list_plugins(state: State<AppState>) -> Vec<PluginDescriptor> {
    enabled_plugins(with(&state, |s| s.plugins.clone()))
}

#[tauri::command(async)]
pub fn rescan_plugins(state: State<AppState>) -> R<Vec<PluginDescriptor>> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let list = plugin_db::scan_with(&exe, &crate::settings::extra_plugin_folders());
    with(&state, |s| s.set_plugins(list.clone()));
    Ok(enabled_plugins(list))
}

#[tauri::command(async)]
pub fn audio_devices(host: Option<String>) -> DeviceList {
    list_devices(host.as_deref())
}

#[tauri::command]
pub fn get_audio_config(state: State<AppState>) -> AudioConfig {
    with(&state, |s| s.audio_config.clone())
}

#[tauri::command]
pub fn set_audio_config(state: State<AppState>, config: AudioConfig) -> Snapshot {
    let mut settings = crate::settings::load();
    settings.audio = config.clone();
    crate::settings::save(&settings);
    snap(&state, |s| s.set_audio_config(config))
}

#[tauri::command]
pub fn play(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.play())
}

#[tauri::command]
pub fn stop(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.stop())
}

#[tauri::command]
pub fn toggle_play(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.toggle_play())
}

#[tauri::command]
pub fn record(state: State<AppState>) -> R<Snapshot> {
    try_snap(&state, |s| {
        if s.is_recording() {
            s.stop();
            Ok(())
        } else {
            s.record()
        }
    })
}

#[tauri::command]
pub fn locate(state: State<AppState>, pos: i64) {
    with(&state, |s| s.locate(pos))
}

#[tauri::command]
pub fn set_cycle(state: State<AppState>, enabled: bool, start: i64, end: i64) -> Snapshot {
    snap(&state, |s| s.set_cycle(enabled, start, end))
}

#[tauri::command]
pub fn set_tempo(state: State<AppState>, bpm: f64) -> Snapshot {
    edit(&state, "Tempo", Some("tempo".into()), false, |s| s.set_tempo(bpm))
}

#[tauri::command]
pub fn set_signature(state: State<AppState>, num: u32, den: u32) -> Snapshot {
    edit(&state, "Time Signature", None, false, |s| s.set_signature(num, den))
}

#[tauri::command]
pub fn set_key(state: State<AppState>, key: String) -> Snapshot {
    edit(&state, "Key", None, false, |s| s.set_key(key))
}

#[tauri::command]
pub fn set_metronome(state: State<AppState>, on: bool) -> Snapshot {
    snap(&state, |s| s.set_metronome(on))
}

#[tauri::command]
pub fn set_count_in(state: State<AppState>, on: bool) -> Snapshot {
    snap(&state, |s| s.set_count_in(on))
}

#[tauri::command]
pub fn set_master_volume(state: State<AppState>, db: f32) -> Snapshot {
    edit(&state, "Stereo Out Volume", Some("master-volume".into()), false, |s| s.set_master_volume(db))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn add_tracks(
    state: State<AppState>,
    kind: TrackKind,
    count: usize,
    input: InputSel,
    ascending: bool,
    instrument: Option<String>,
    armed: bool,
    monitor: bool,
) -> R<Snapshot> {
    try_edit(&state, if count > 1 { "New Tracks" } else { "New Track" }, None, false, |s| {
        let after = s.selected.and_then(|id| s.project.tracks.iter().position(|t| t.id == id)).map(|i| i + 1);
        let mut last = None;
        for n in 0..count.clamp(1, 64) {
            let input = match input {
                InputSel::Mono(c) if ascending => InputSel::Mono(c + n as u16),
                InputSel::Stereo(c) if ascending => InputSel::Stereo(c + 2 * n as u16),
                other => other,
            };
            let idx = after.map(|i| i + n);
            let id = s.add_track(kind, input, instrument.clone(), idx)?;
            if armed || monitor {
                s.update_track(id, TrackPatch { armed: Some(armed), monitor: Some(monitor), ..Default::default() });
            }
            last = Some(id);
        }
        s.select_track(last);
        Ok(())
    })
}

#[tauri::command]
pub fn delete_track(state: State<AppState>, id: Id) -> Snapshot {
    edit(&state, "Delete Track", None, true, |s| s.delete_track(id))
}

#[tauri::command]
pub fn update_track(state: State<AppState>, id: Id, patch: TrackPatch) -> Snapshot {
    // Mute, solo, record-enable and monitoring are performance switches, not edits (as in Logic).
    let label = if patch.volume_db.is_some() {
        Some(("Volume", "volume"))
    } else if patch.pan.is_some() {
        Some(("Pan", "pan"))
    } else if patch.name.is_some() {
        Some(("Rename Track", "name"))
    } else if patch.color.is_some() {
        Some(("Track Color", "color"))
    } else if patch.input.is_some() {
        Some(("Track Input", "input"))
    } else {
        None
    };
    // Moves recorded as automation belong to the pass's undo step.
    let label = label.filter(|_| !with(&state, |s| s.automation_writing(id)));
    match label {
        Some((label, field)) => edit(&state, label, Some(format!("track:{id}:{field}")), false, |s| s.update_track_linked(id, patch)),
        None => snap(&state, |s| s.update_track_linked(id, patch)),
    }
}

#[tauri::command]
pub fn delete_tracks(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, &plural(ids.len(), "Delete Track", "Delete Tracks"), None, true, |s| s.delete_tracks(&ids))
}

#[tauri::command]
pub fn delete_unused_tracks(state: State<AppState>) -> Made {
    with(&state, |s| {
        s.checkpoint("Delete Unused Tracks", None, true);
        let n = s.delete_unused_tracks();
        Made { snapshot: snapshot(s), ids: vec![n as Id] }
    })
}

/// `content` copies the regions too (Duplicate Track); without it, New Track with Same Settings.
#[tauri::command]
pub fn duplicate_tracks(state: State<AppState>, ids: Vec<Id>, content: bool) -> R<Made> {
    let label = if content { plural(ids.len(), "Duplicate Track", "Duplicate Tracks") } else { "New Track with Same Settings".into() };
    edit_made(&state, &label, false, |s| {
        let made = s.duplicate_tracks(&ids, content);
        if let Some(&last) = made.last() {
            s.select_track(Some(last));
        }
        Ok(made)
    })
}

#[tauri::command]
pub fn create_stack(state: State<AppState>, ids: Vec<Id>, summing: bool) -> R<Made> {
    edit_made(&state, "Create Track Stack", false, |s| {
        let id = s.create_stack(&ids, summing).ok_or_else(|| anyhow::anyhow!("select the tracks to put in a stack first"))?;
        s.select_track(Some(id));
        Ok(vec![id])
    })
}

#[tauri::command]
pub fn flatten_stack(state: State<AppState>, id: Id) -> Snapshot {
    edit(&state, "Flatten Stack", None, true, |s| s.flatten_stack(id))
}

/// Reorders tracks; `parents` moves some into a stack (Some) or out of one (None).
#[tauri::command]
pub fn move_tracks(state: State<AppState>, ids: Vec<Id>, parents: Vec<(Id, Option<Id>)>) -> Snapshot {
    edit(&state, "Move Tracks", None, false, |s| s.move_tracks(ids, &parents))
}

#[tauri::command]
pub fn create_group(state: State<AppState>, ids: Vec<Id>) -> R<Made> {
    edit_made(&state, "Create Group", false, |s| Ok(vec![s.create_group(&ids)]))
}

#[tauri::command]
pub fn update_group(state: State<AppState>, group: Group) -> Snapshot {
    let key = format!("group:{}", group.id);
    edit(&state, "Group Settings", Some(key), false, |s| s.update_group(group))
}

#[tauri::command]
pub fn delete_group(state: State<AppState>, id: Id) -> Snapshot {
    edit(&state, "Delete Group", None, false, |s| s.delete_group(id))
}

#[tauri::command]
pub fn set_track_group(state: State<AppState>, ids: Vec<Id>, group: Option<Id>) -> Snapshot {
    edit(&state, if group.is_some() { "Assign Group" } else { "Remove from Group" }, None, false, |s| s.set_track_group(&ids, group))
}

#[tauri::command]
pub fn paste_regions(state: State<AppState>, items: Vec<(Id, Region)>, offset: i64, label: Option<String>) -> R<Made> {
    edit_made(&state, label.as_deref().unwrap_or("Paste"), false, |s| Ok(s.paste_regions(&items, offset)))
}

#[tauri::command]
pub fn repeat_regions(state: State<AppState>, ids: Vec<Id>, times: usize) -> R<Made> {
    edit_made(&state, "Repeat Regions", false, |s| Ok(s.repeat_regions(&ids, times)))
}

#[tauri::command]
pub fn set_region_gain(state: State<AppState>, ids: Vec<Id>, db: Option<f32>, delta: Option<f32>) -> Snapshot {
    let key = format!("gain:{ids:?}");
    edit(&state, "Region Gain", Some(key), false, |s| s.set_region_gain(&ids, db, delta))
}

#[tauri::command]
pub fn normalize_regions(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Normalize", None, false, |s| s.normalize_regions(&ids))
}

#[tauri::command]
pub fn set_region_fades(state: State<AppState>, ids: Vec<Id>, fade_in: Option<i64>, fade_out: Option<i64>) -> Snapshot {
    edit(&state, "Fade", None, false, |s| s.set_region_fades(&ids, fade_in, fade_out))
}

#[tauri::command]
pub fn set_region_color(state: State<AppState>, ids: Vec<Id>, color: Option<String>) -> Snapshot {
    edit(&state, "Region Color", None, false, |s| s.set_region_color(&ids, color))
}

#[tauri::command]
pub fn transpose_regions(state: State<AppState>, ids: Vec<Id>, semitones: i32) -> Snapshot {
    let key = format!("transpose:{ids:?}");
    edit(&state, "Transpose", Some(key), false, |s| s.transpose_regions(&ids, semitones))
}

#[tauri::command]
pub fn quantize_regions(state: State<AppState>, ids: Vec<Id>, grid: i64) -> Snapshot {
    edit(&state, "Quantize", None, false, |s| s.quantize_regions(&ids, grid))
}

/// `key` lets the edit that fills the new region (the Piano Roll's first note) join its undo
/// step, as `set_midi_content` with the same key.
#[tauri::command]
pub fn create_midi_region(state: State<AppState>, track: Id, start: i64, length: i64, key: Option<String>) -> R<Made> {
    with(&state, |s| {
        let pushed = s.checkpoint("Create MIDI Region", key, false);
        let ids = vec![s.create_midi_region(track, start, length).map_err(|e| {
            if pushed {
                s.abandon_checkpoint();
            }
            e.to_string()
        })?];
        s.frozen_guard().map_err(|e| e.to_string())?;
        Ok(Made { snapshot: snapshot(s), ids })
    })
}

#[tauri::command]
pub fn regions_to_new_track(state: State<AppState>, ids: Vec<Id>) -> R<Made> {
    edit_made(&state, "Move to New Track", false, |s| {
        let made = s.regions_to_new_track(&ids);
        if let Some(&t) = made.last() {
            s.select_track(Some(t));
        }
        Ok(ids.clone())
    })
}

#[tauri::command]
pub fn reverse_regions(state: State<AppState>, ids: Vec<Id>) -> R<Snapshot> {
    try_edit(&state, "Reverse", None, false, |s| s.reverse_regions(&ids))
}

#[tauri::command]
pub fn join_regions(state: State<AppState>, ids: Vec<Id>) -> R<Made> {
    edit_made(&state, "Join Regions", false, |s| s.join_regions(&ids))
}

#[tauri::command]
pub fn select_track(state: State<AppState>, id: Option<Id>) -> Snapshot {
    snap(&state, |s| s.select_track(id))
}

#[tauri::command]
pub fn reorder_tracks(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Reorder Tracks", None, false, |s| s.reorder_tracks(ids))
}

#[tauri::command]
pub fn move_region(state: State<AppState>, id: Id, start: i64, track: Option<Id>) -> Snapshot {
    edit(&state, "Move Region", None, false, |s| s.move_region(id, start, track))
}

#[tauri::command]
pub fn resize_region(state: State<AppState>, id: Id, start: i64, length: i64) -> Snapshot {
    edit(&state, "Resize Region", None, false, |s| s.resize_region(id, start, length))
}

#[tauri::command]
pub fn delete_regions(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, if ids.len() > 1 { "Delete Regions" } else { "Delete Region" }, None, false, |s| s.delete_regions(&ids))
}

#[tauri::command]
pub fn set_region_muted(state: State<AppState>, id: Id, muted: bool) -> Snapshot {
    edit(&state, if muted { "Mute Region" } else { "Unmute Region" }, None, false, |s| s.set_region_muted(id, muted))
}

#[tauri::command]
pub fn rename_region(state: State<AppState>, id: Id, name: String) -> Snapshot {
    edit(&state, "Rename Region", None, false, |s| s.rename_region(id, name))
}

#[tauri::command]
pub fn split_regions(state: State<AppState>, pos: i64, ids: Vec<Id>) -> Snapshot {
    edit(&state, "Split Regions", None, false, |s| s.split_at(pos, &ids))
}

#[tauri::command]
pub fn import_audio(state: State<AppState>, path: String, track: Id, start: i64) -> R<Snapshot> {
    try_edit(&state, "Import Audio", None, false, |s| s.import_audio(&PathBuf::from(path), track, start).map(|_| ()))
}

/// The clip's waveform overview as raw bytes (an ArrayBuffer in JS); empty if it isn't loaded.
#[tauri::command(async)]
pub fn clip_waveform(state: State<AppState>, id: Id) -> tauri::ipc::Response {
    tauri::ipc::Response::new(with(&state, |s| s.clip_waveform(id)).unwrap_or_default())
}

/// `slot` of -1 addresses the instrument slot.
fn slot_index(slot: i64) -> usize {
    if slot < 0 {
        usize::MAX
    } else {
        slot as usize
    }
}

#[tauri::command]
pub fn set_plugin(state: State<AppState>, track: Id, slot: i64, plugin_id: String, open: bool) -> R<Snapshot> {
    let mut warning = None;
    let snap = try_edit(&state, "Insert Plug-in", None, true, |s| {
        let uid = s.set_plugin(track, slot_index(slot), &plugin_id)?;
        // Built-in effects open their panel in the UI instead.
        if open && !plugin_id.starts_with("builtin:") {
            let title = s.project.track(track).map(|t| t.name.clone()).unwrap_or_else(|| "Stereo Out".into());
            // The plug-in stays inserted; the user hears why its window didn't open.
            if let Err(e) = s.open_editor(uid, &title) {
                log::warn!("{e}");
                warning = Some(format!("Couldn't open the plug-in's window: {e}"));
            }
        }
        Ok(())
    })?;
    Ok(Snapshot { warning, ..snap })
}

#[tauri::command]
pub fn remove_plugin(state: State<AppState>, track: Id, slot: i64) -> Snapshot {
    edit(&state, "Remove Plug-in", None, true, |s| s.remove_plugin(track, slot_index(slot)))
}

#[tauri::command]
pub fn set_bypass(state: State<AppState>, track: Id, slot: i64, bypass: bool) -> Snapshot {
    edit(&state, if bypass { "Bypass Plug-in" } else { "Enable Plug-in" }, None, false, |s| s.set_bypass(track, slot_index(slot), bypass))
}

#[tauri::command]
pub fn open_editor(state: State<AppState>, uid: Id, title: String) -> R<Snapshot> {
    try_snap(&state, |s| s.open_editor(uid, &title))
}

#[tauri::command]
pub fn new_project(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.new_project())
}

#[tauri::command]
pub fn save_project(state: State<AppState>, path: Option<String>) -> R<Snapshot> {
    try_snap(&state, |s| {
        let saved = s.save(path.map(PathBuf::from))?;
        crate::recent::add(&saved);
        Ok(())
    })
}

#[tauri::command]
pub fn open_project(state: State<AppState>, path: String) -> R<Snapshot> {
    try_snap(&state, |s| {
        s.open(&PathBuf::from(&path))?;
        crate::recent::add(&s.project_path());
        Ok(())
    })
}

#[tauri::command(async)]
pub fn recent_projects() -> Vec<crate::recent::RecentProject> {
    crate::recent::list()
}

#[tauri::command]
pub fn forget_recent(path: String) {
    crate::recent::remove(&path)
}

#[tauri::command]
pub fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

/// Where new projects go unless the user picks somewhere else (Settings › General).
#[tauri::command]
pub fn default_project_dir(state: State<AppState>) -> String {
    with(&state, |s| s.projects_root()).to_string_lossy().to_string()
}

#[derive(Serialize)]
pub struct DirEntry {
    name: String,
    path: String,
    is_dir: bool,
}

#[derive(Serialize)]
pub struct DirListing {
    path: String,
    parent: Option<String>,
    entries: Vec<DirEntry>,
}

/// Lists folders and audio files for the All Files browser.
#[tauri::command(async)]
pub fn list_dir(path: Option<String>) -> R<DirListing> {
    let dir = match path {
        Some(p) => PathBuf::from(p),
        None => {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
            let music = home.join("Music");
            if music.is_dir() { music } else { home }
        }
    };
    let mut entries: Vec<DirEntry> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None;
            }
            let p = e.path();
            let is_dir = p.is_dir();
            let audio = daw_engine::decode::is_audio_file(&p);
            (is_dir || audio).then(|| DirEntry { name, path: p.to_string_lossy().to_string(), is_dir })
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(DirListing {
        parent: dir.parent().map(|p| p.to_string_lossy().to_string()),
        path: dir.to_string_lossy().to_string(),
        entries,
    })
}

/// The system mixer volume on the current recording device (Linux sound servers only).
#[tauri::command(async)]
pub fn input_level(state: State<AppState>) -> Option<serde_json::Value> {
    #[cfg(target_os = "linux")]
    {
        let (host, input, enabled) = with(&state, |s| (s.audio_status.host.clone(), s.audio_config.input.clone(), s.audio_config.input_enabled));
        if !enabled || host == "JACK" {
            return None;
        }
        let level = daw_engine::sysvol::input_level(input.as_deref())?;
        let attenuating = level.attenuating();
        let mut v = serde_json::to_value(level).ok()?;
        v["attenuating"] = serde_json::Value::Bool(attenuating);
        Some(v)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = state;
        None
    }
}

#[tauri::command(async)]
pub fn reset_input_level(state: State<AppState>) -> R<Option<serde_json::Value>> {
    #[cfg(target_os = "linux")]
    {
        let input = with(&state, |s| s.audio_config.input.clone());
        daw_engine::sysvol::reset_input_level(input.as_deref())?;
    }
    Ok(input_level(state))
}

/// Updates a strip's channel EQ. Returns nothing: the UI updates optimistically while dragging.
#[tauri::command]
pub fn set_eq(state: State<AppState>, track: Id, eq: ChannelEq) -> R<()> {
    with(&state, |s| {
        s.checkpoint("Channel EQ", Some(format!("eq:{track}")), false);
        s.set_eq(track, eq);
        s.frozen_guard().map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn set_analyzer(state: State<AppState>, track: Option<Id>) {
    with(&state, |s| s.set_analyzer(track))
}

#[derive(Serialize)]
pub struct ImportResult {
    snapshot: Snapshot,
    errors: Vec<String>,
}

/// Imports dropped audio files: onto `track` at `start` (extra files on new tracks below), or
/// onto new tracks inserted at `index`.
#[tauri::command]
pub fn import_audio_files(state: State<AppState>, paths: Vec<String>, track: Option<Id>, start: i64, index: Option<usize>) -> ImportResult {
    with(&state, |s| {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        s.checkpoint(if paths.len() > 1 { "Import Audio Files" } else { "Import Audio" }, None, false);
        let errors = s.import_audio_files(&paths, track, start, index);
        ImportResult { snapshot: snapshot(s), errors }
    })
}

/// A MIDI message from the on-screen keyboard / Musical Typing.
#[tauri::command(async)]
pub fn midi_event(state: State<AppState>, data: [u8; 3]) {
    state.session.lock().send_midi(data);
}

#[derive(Serialize)]
pub struct UndoResult {
    snapshot: Snapshot,
    /// What was undone or redone, if anything.
    label: Option<String>,
}

#[tauri::command]
pub fn undo(state: State<AppState>) -> UndoResult {
    with(&state, |s| {
        let label = s.undo();
        UndoResult { snapshot: snapshot(s), label }
    })
}

#[tauri::command]
pub fn redo(state: State<AppState>) -> UndoResult {
    with(&state, |s| {
        let label = s.redo();
        UndoResult { snapshot: snapshot(s), label }
    })
}

/// Drags a region's loop to `loop_end` (None, or the region's own end, turns it off).
#[tauri::command]
pub fn set_region_loop(state: State<AppState>, id: Id, loop_end: Option<i64>) -> Snapshot {
    edit(&state, "Loop Region", None, false, |s| s.set_region_loop(id, loop_end))
}

/// The Loop switch (L): on loops up to the next region on the track, off removes the loop.
#[tauri::command]
pub fn set_regions_looped(state: State<AppState>, ids: Vec<Id>, on: bool) -> Snapshot {
    let label = if on { plural(ids.len(), "Loop Region", "Loop Regions") } else { "Turn Off Loop".into() };
    edit(&state, &label, None, false, |s| s.set_regions_looped(&ids, on))
}

/// Convert Loops to Real Copies. Returns the regions now standing for the loops (originals
/// and copies), for the UI to select.
#[tauri::command]
pub fn convert_loops(state: State<AppState>, ids: Vec<Id>) -> R<Made> {
    edit_made(&state, "Convert Loops to Real Copies", false, |s| {
        let mut all = ids.clone();
        all.extend(s.convert_loops(&ids));
        Ok(all)
    })
}

#[tauri::command]
pub fn set_tracks_hidden(state: State<AppState>, ids: Vec<Id>, hidden: bool) -> Snapshot {
    edit(&state, if hidden { "Hide Tracks" } else { "Show Tracks" }, None, false, |s| s.set_tracks_hidden(&ids, hidden))
}

/// A new aux at the end of the track list. `send_from` (track, slot) fills that send slot
/// with a 0 dB post-fader send to it, and `output_from` routes that track's output to it, both
/// in the same undo step.
#[tauri::command]
pub fn create_aux(state: State<AppState>, name: Option<String>, send_from: Option<(Id, usize)>, output_from: Option<Id>) -> R<Made> {
    edit_made(&state, "New Aux", false, |s| {
        let id = s.create_aux(name)?;
        if let Some((track, slot)) = send_from {
            s.set_send(track, slot, Some(TrackSend { bus: id, level_db: 0.0, pre_fader: false, muted: false }))?;
        }
        if let Some(track) = output_from {
            s.set_output(track, Some(id))?;
        }
        Ok(vec![id])
    })
}

/// Fills (`send`) or clears (null) a send slot; refuses feedback loops.
#[tauri::command]
pub fn set_send(state: State<AppState>, track: Id, slot: usize, send: Option<TrackSend>) -> R<Snapshot> {
    let label = if send.is_some() { "Send" } else { "Remove Send" };
    try_edit(&state, label, None, false, |s| s.set_send(track, slot, send))
}

/// A send knob drag; consecutive changes to one send make one undo step.
#[tauri::command]
pub fn set_send_level(state: State<AppState>, track: Id, slot: usize, level_db: f32) -> Snapshot {
    // Moves recorded as automation belong to the pass's undo step.
    if with(&state, |s| s.automation_writing(track)) {
        return snap(&state, |s| s.set_send_level_linked(track, slot, level_db));
    }
    edit(&state, "Send Level", Some(format!("send:{track}:{slot}")), false, |s| s.set_send_level_linked(track, slot, level_db))
}

/// Routes a track to an aux, to Stereo Out (0) or back to its default (null).
#[tauri::command]
pub fn set_output(state: State<AppState>, track: Id, output: Option<Id>) -> R<Snapshot> {
    try_edit(&state, "Track Output", None, false, |s| s.set_output(track, output))
}

// ---- bounce and export ----------------------------------------------------------------------
//
// Renders run as async commands, off the main thread, so the window stays responsive. They hold
// the session for the whole render; the UI keeps a busy sheet up meanwhile.

/// Reports render progress (0..1) to the UI as `bounce-progress` events.
fn progress(app: &tauri::AppHandle) -> impl FnMut(f64) {
    let app = app.clone();
    let mut last = -1.0;
    move |f| {
        if f - last >= 0.01 || f >= 1.0 {
            last = f;
            let _ = app.emit("bounce-progress", f);
        }
    }
}

/// The tracks Export All Tracks would write, for the dialog to list.
#[tauri::command]
pub fn export_targets(state: State<AppState>) -> Vec<Id> {
    with(&state, |s| s.export_targets())
}

#[tauri::command(async)]
pub fn bounce_project(app: tauri::AppHandle, state: State<AppState>, path: String, options: RenderOptions) -> R<BounceResult> {
    let mut p = progress(&app);
    with(&state, |s| s.bounce_project(&PathBuf::from(path), &options, &mut p)).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn export_tracks(app: tauri::AppHandle, state: State<AppState>, dir: String, options: RenderOptions) -> R<BounceResult> {
    let mut p = progress(&app);
    with(&state, |s| s.export_tracks(&PathBuf::from(dir), &options, &mut p)).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn export_region(app: tauri::AppHandle, state: State<AppState>, id: Id, path: String, options: RenderOptions) -> R<BounceResult> {
    let mut p = progress(&app);
    with(&state, |s| s.export_region(id, &PathBuf::from(path), &options, &mut p)).map_err(|e| e.to_string())
}

/// Bounce in Place for `ids` (tracks), or for `regions` when that isn't empty. The render runs
/// here; the track edits (which may delete plug-ins) then run on the main thread as one undo
/// step.
#[tauri::command(async)]
pub fn bounce_in_place(app: tauri::AppHandle, state: State<AppState>, ids: Vec<Id>, regions: Vec<Id>, options: BipOptions) -> R<Made> {
    let mut p = progress(&app);
    let renders = with(&state, |s| s.render_in_place(&ids, &regions, &options, &mut p)).map_err(|e| e.to_string())?;
    let session = state.session.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let mut s = session.lock();
        s.checkpoint("Bounce in Place", None, options.source == BipSource::Delete);
        let res = s.place_bounces(renders, &options).map(|made| {
            if let Some(&last) = made.last() {
                s.select_track(Some(last));
            }
            Made { snapshot: snapshot(&s), ids: made }
        });
        if res.is_err() {
            s.abandon_checkpoint();
        }
        let _ = tx.send(res.map_err(|e| e.to_string()));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

// ---- Audio Track editor ----------------------------------------------------------------------

/// Splits the regions at every position given, as one undo step. Returns all the pieces.
#[tauri::command]
pub fn split_regions_at(state: State<AppState>, ids: Vec<Id>, positions: Vec<i64>) -> R<Made> {
    edit_made(&state, "Split Regions", false, |s| Ok(s.split_regions_at(&ids, &positions)))
}

/// Applies a range action (`action`: "split", "trim", "silence", "delete", "normalize",
/// "reverse", "fade_in" or "fade_out") to the parts of `ids` between `start` and `end`.
/// Returns the regions that now hold the range.
#[tauri::command]
pub fn edit_range(state: State<AppState>, ids: Vec<Id>, start: i64, end: i64, action: RangeEdit) -> R<Made> {
    edit_made(&state, action.label(), false, |s| s.edit_range(&ids, start, end, action))
}

/// Onset positions in the clip (frames from its start). Runs off the main thread, without
/// holding the session while it analyses.
#[tauri::command(async)]
pub fn clip_transients(state: State<AppState>, clip: Id, sensitivity: f32) -> Vec<i64> {
    let audio = with(&state, |s| s.clip_audio(clip));
    audio.map(|c| transients::detect(&c.channels, c.sample_rate, sensitivity)).unwrap_or_default()
}

#[tauri::command]
pub fn clip_info(state: State<AppState>, id: Id) -> Option<ClipInfo> {
    with(&state, |s| s.clip_info(id))
}

/// Replaces an automation lane's points. Drags of one lane share an undo step.
#[tauri::command]
pub fn set_automation_lane(state: State<AppState>, track: Id, param: AutoParam, points: Vec<AutoPoint>) -> Snapshot {
    let key = format!("automation:{track}:{}", serde_json::to_string(&param).unwrap_or_default());
    edit(&state, "Edit Automation", Some(key), false, |s| s.set_automation_lane(track, param, points))
}

#[tauri::command]
pub fn set_auto_mode(state: State<AppState>, track: Id, mode: AutoMode) -> Snapshot {
    edit(&state, "Automation Mode", None, false, |s| s.set_auto_mode(track, mode))
}

/// Deletes one lane, or all of the track's automation without `param`.
#[tauri::command]
pub fn clear_automation(state: State<AppState>, track: Id, param: Option<AutoParam>) -> Snapshot {
    edit(&state, "Delete Automation", None, false, |s| s.clear_automation(track, param))
}

/// The UI grabbed or let go of a fader, knob or slider. Writing in Touch and Latch follows it.
#[tauri::command]
pub fn automation_touch(state: State<AppState>, track: Id, param: AutoParam, touching: bool) -> Snapshot {
    snap(&state, |s| s.automation_touch(track, param, touching))
}

#[tauri::command]
pub fn list_plugin_params(state: State<AppState>, uid: Id) -> Vec<ParamInfo> {
    with(&state, |s| s.plugin_params(uid))
}

// ---- Settings and Project Settings ----------------------------------------------------------

fn enabled_plugins(list: Vec<PluginDescriptor>) -> Vec<PluginDescriptor> {
    let off = crate::settings::load().plugins.disabled;
    list.into_iter().filter(|p| !off.contains(&p.id)).collect()
}

/// Everything in Settings except the audio devices (which have their own commands).
#[derive(Serialize, serde::Deserialize)]
pub struct SettingsView {
    prefs: Prefs,
    ui: crate::settings::UiPrefs,
    plugins: crate::settings::PluginPrefs,
}

#[tauri::command]
pub fn get_settings() -> SettingsView {
    let s = crate::settings::load();
    SettingsView { prefs: s.prefs, ui: s.ui, plugins: s.plugins }
}

/// Stores the settings and applies them at once.
#[tauri::command]
pub fn set_settings(state: State<AppState>, settings: SettingsView) -> Snapshot {
    let mut all = crate::settings::load();
    all.prefs = settings.prefs.clone();
    all.ui = settings.ui;
    all.plugins = settings.plugins;
    crate::settings::save(&all);
    snap(&state, |s| s.set_prefs(settings.prefs))
}

#[derive(Serialize)]
pub struct ManagedPlugin {
    #[serde(flatten)]
    desc: PluginDescriptor,
    enabled: bool,
}

#[derive(Serialize)]
pub struct PluginManager {
    plugins: Vec<ManagedPlugin>,
    report: Option<plugin_db::ScanReport>,
    /// The folders a scan searches: the standard ones, then the user's.
    folders: Vec<String>,
}

/// Settings › Plug-ins: every scanned plug-in, the last scan's report and the search folders.
#[tauri::command]
pub fn plugin_manager(state: State<AppState>) -> PluginManager {
    let off = crate::settings::load().plugins.disabled;
    let plugins = with(&state, |s| s.plugins.clone()).into_iter().map(|d| ManagedPlugin { enabled: !off.contains(&d.id), desc: d }).collect();
    let folders = plugin_db::search_paths(&crate::settings::extra_plugin_folders()).iter().map(|p| p.to_string_lossy().to_string()).collect();
    PluginManager { plugins, report: plugin_db::load_report(), folders }
}

/// Settings › Audio › Reset: closes and reopens the audio devices.
#[tauri::command]
pub fn restart_audio(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.restart_audio())
}

#[tauri::command]
pub fn midi_ports(state: State<AppState>) -> Vec<daw_engine::midi_io::MidiPort> {
    with(&state, |s| s.midi_ports())
}

#[tauri::command]
pub fn midi_monitor(state: State<AppState>) -> Vec<daw_engine::midi_io::MidiLogEntry> {
    with(&state, |s| s.midi_monitor())
}

#[tauri::command]
pub fn set_click(state: State<AppState>, click: ClickSettings) -> Snapshot {
    edit(&state, "Metronome Settings", Some("click".into()), false, |s| s.set_click(click))
}

#[tauri::command]
pub fn set_record_bit_depth(state: State<AppState>, depth: Option<BitDepth>) -> Snapshot {
    edit(&state, "Recording Format", None, false, |s| s.set_record_bit_depth(depth))
}

#[tauri::command]
pub fn set_project_notes(state: State<AppState>, notes: String) -> Snapshot {
    edit(&state, "Project Notes", Some("notes".into()), false, |s| s.set_notes(notes))
}

/// Moves regions by `delta` samples (Nudge Left / Right).
#[tauri::command]
pub fn nudge_regions(state: State<AppState>, ids: Vec<Id>, delta: i64) -> Snapshot {
    let key = format!("nudge:{ids:?}");
    edit(&state, "Nudge", Some(key), false, |s| s.nudge_regions(&ids, delta))
}

/// Whether the project at `path` has an autosave newer than its last save.
#[tauri::command]
pub fn recovery_info(path: String) -> Option<daw_engine::session::Recovery> {
    daw_engine::session::recovery_info(std::path::Path::new(&path))
}

/// Projects with unsaved work left behind (after a crash): recent projects, plus project
/// folders in the projects folder (where untitled projects live).
#[tauri::command(async)]
pub fn recoverable_projects(state: State<AppState>) -> Vec<daw_engine::session::Recovery> {
    let (root, current) = with(&state, |s| (s.projects_root(), s.project_path()));
    let mut dirs: Vec<PathBuf> = crate::recent::paths().into_iter().map(PathBuf::from).collect();
    for r in [root, daw_engine::session::default_projects_root()] {
        if let Ok(rd) = std::fs::read_dir(&r) {
            dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "fermata").unwrap_or(false)));
        }
    }
    let mut seen = Vec::new();
    let mut out: Vec<_> = dirs
        .into_iter()
        .filter(|d| *d != current)
        .filter(|d| {
            let k = std::fs::canonicalize(d).unwrap_or_else(|_| d.clone());
            !seen.contains(&k) && {
                seen.push(k);
                true
            }
        })
        .filter_map(|d| daw_engine::session::recovery_info(&d))
        .collect();
    out.sort_by(|a, b| b.autosaved.cmp(&a.autosaved));
    out
}

/// Opens a project from its autosave.
#[tauri::command]
pub fn open_recovered(state: State<AppState>, path: String) -> R<Snapshot> {
    try_snap(&state, |s| {
        s.open_recovered(&PathBuf::from(&path))?;
        if s.saved {
            crate::recent::add(&s.project_path());
        }
        Ok(())
    })
}

/// Deletes an autosave: the given project's, or the current one's.
#[tauri::command]
pub fn discard_autosave(state: State<AppState>, path: Option<String>) {
    match path {
        Some(p) => daw_engine::session::discard_autosave(std::path::Path::new(&p)),
        None => with(&state, |s| s.discard_autosave()),
    }
    }

/// Replaces the notes and controllers of MIDI regions (the Piano Roll's edits). `key`
/// merges a burst of edits (nudges, a drag sent in parts) into one undo step.
#[tauri::command]
pub fn set_midi_content(state: State<AppState>, edits: Vec<MidiContent>, label: String, key: Option<String>) -> Snapshot {
    edit(&state, &label, key, false, |s| s.set_midi_content(edits))
}

/// Imports a Standard MIDI File onto `track` (an instrument track) or new tracks at `index`.
#[tauri::command]
pub fn import_midi(state: State<AppState>, path: String, track: Option<Id>, start: i64, index: Option<usize>) -> R<Made> {
    edit_made(&state, "Import MIDI File", false, |s| s.import_midi(&PathBuf::from(path), track, start, index))
}

/// Writes MIDI regions to a Standard MIDI File.
#[tauri::command]
pub fn export_midi(state: State<AppState>, ids: Vec<Id>, path: String) -> R<()> {
    with(&state, |s| s.export_midi(&ids, &PathBuf::from(path))).map_err(|e| e.to_string())
}

// ---- track freeze -------------------------------------------------------------------------

/// Freezes tracks: renders each through its instrument and plug-ins (off the main thread, like
/// a bounce, reporting `bounce-progress`), then plays the renders and unloads the plug-ins on
/// the main thread, as one undo step.
#[tauri::command(async)]
pub fn freeze_tracks(app: tauri::AppHandle, state: State<AppState>, ids: Vec<Id>) -> R<Snapshot> {
    let mut p = progress(&app);
    let renders = with(&state, |s| s.render_freeze(&ids, &mut p)).map_err(|e| e.to_string())?;
    let session = state.session.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let mut s = session.lock();
        s.checkpoint(if ids.len() > 1 { "Freeze Tracks" } else { "Freeze Track" }, None, true);
        let res = s.place_freeze(renders).map(|_| snapshot(&s));
        if res.is_err() {
            s.abandon_checkpoint();
        }
        let _ = tx.send(res.map_err(|e| e.to_string()));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

/// Unfreezes tracks: their plug-ins load again and the freeze files are deleted.
#[tauri::command]
pub fn unfreeze_tracks(state: State<AppState>, ids: Vec<Id>) -> R<Snapshot> {
    try_edit(&state, if ids.len() > 1 { "Unfreeze Tracks" } else { "Unfreeze Track" }, None, true, |s| ids.iter().try_for_each(|&id| s.unfreeze_track(id)))
}

// ---- markers, Autopunch and take folders ------------------------------------------------------

/// Create Marker at `pos` (the playhead, snapped by the UI). Returns the marker.
#[tauri::command]
pub fn add_marker(state: State<AppState>, pos: i64, name: Option<String>) -> R<Made> {
    edit_made(&state, "Create Marker", false, |s| Ok(vec![s.add_marker(pos, name)]))
}

#[tauri::command]
pub fn delete_markers(state: State<AppState>, ids: Vec<Id>) -> Snapshot {
    edit(&state, &plural(ids.len(), "Delete Marker", "Delete Markers"), None, false, |s| s.delete_markers(&ids))
}

#[tauri::command]
pub fn rename_marker(state: State<AppState>, id: Id, name: String) -> Snapshot {
    edit(&state, "Rename Marker", None, false, |s| s.rename_marker(id, name))
}

#[tauri::command]
pub fn move_marker(state: State<AppState>, id: Id, pos: i64) -> Snapshot {
    edit(&state, "Move Marker", None, false, |s| s.move_marker(id, pos))
}

#[tauri::command]
pub fn set_marker_color(state: State<AppState>, id: Id, color: Option<String>) -> Snapshot {
    edit(&state, "Marker Color", None, false, |s| s.set_marker_color(id, color))
}

/// The Autopunch range and switch (not an undo step, like the cycle).
#[tauri::command]
pub fn set_punch(state: State<AppState>, enabled: bool, start: i64, end: i64) -> Snapshot {
    snap(&state, |s| s.set_punch(enabled, start, end))
}

/// Quick Swipe Comping: `take` is heard over timeline `start..end` of the take folder
/// `region` (None silences that part).
#[tauri::command]
pub fn set_comp_range(state: State<AppState>, region: Id, take: Option<Id>, start: i64, end: i64) -> Snapshot {
    edit(&state, "Edit Comp", None, false, |s| s.set_comp_range(region, take, start, end))
}

#[tauri::command]
pub fn select_take(state: State<AppState>, region: Id, take: Id) -> Snapshot {
    edit(&state, "Select Take", None, false, |s| s.select_take(region, take))
}

#[tauri::command]
pub fn rename_take(state: State<AppState>, region: Id, take: Id, name: String) -> Snapshot {
    edit(&state, "Rename Take", None, false, |s| s.rename_take(region, take, name))
}

#[tauri::command]
pub fn delete_take(state: State<AppState>, region: Id, take: Id) -> Snapshot {
    edit(&state, "Delete Take", None, false, |s| s.delete_take(region, take))
}

#[tauri::command]
pub fn delete_unused_takes(state: State<AppState>, region: Id) -> Snapshot {
    edit(&state, "Delete Unused Takes", None, false, |s| s.delete_unused_takes(region))
}

/// Flatten: the comp as plain regions. Returns them.
#[tauri::command]
pub fn flatten_take_folder(state: State<AppState>, region: Id) -> R<Made> {
    edit_made(&state, "Flatten Take Folder", false, |s| Ok(s.flatten_take_folder(region)))
}

/// Flatten and Merge: the comp rendered to one new file. Returns the region.
#[tauri::command]
pub fn merge_take_folder(state: State<AppState>, region: Id) -> R<Made> {
    edit_made(&state, "Flatten and Merge", false, |s| s.merge_take_folder(region))
}

/// Unpack to New Tracks. Returns the new tracks.
#[tauri::command]
pub fn unpack_take_folder(state: State<AppState>, region: Id) -> R<Made> {
    edit_made(&state, "Unpack Take Folder", false, |s| Ok(s.unpack_take_folder(region)))
}

/// Pack Take Folder from audio regions on one track. Returns the folder.
#[tauri::command]
pub fn pack_take_folder(state: State<AppState>, ids: Vec<Id>) -> R<Made> {
    edit_made(&state, "Pack Take Folder", false, |s| s.pack_take_folder(&ids).map(|id| vec![id]))
}

/// Moves several regions as one undo step (a multi-region drag).
#[tauri::command]
pub fn move_regions(state: State<AppState>, moves: Vec<daw_engine::session::RegionMove>) -> Snapshot {
    edit(&state, if moves.len() == 1 { "Move Region" } else { "Move Regions" }, None, false, |s| s.move_regions(&moves))
}

/// Drops the undo history (after the Project Chooser has set up a template).
#[tauri::command]
pub fn clear_undo_history(state: State<AppState>) -> Snapshot {
    snap(&state, |s| s.clear_history())
}

/// The Tempo track. With `moveMidi`, MIDI regions keep their bars and beats.
#[tauri::command]
pub fn set_tempo_map(state: State<AppState>, events: Vec<TempoEvent>, move_midi: Option<bool>) -> Snapshot {
    edit(&state, "Tempo Change", None, false, |s| s.set_tempo_map(events, move_midi.unwrap_or(false)))
}

/// The Signature track.
#[tauri::command]
pub fn set_signatures(state: State<AppState>, events: Vec<SigEvent>) -> Snapshot {
    edit(&state, "Signature Change", None, false, |s| s.set_signatures(events))
}

/// The LCD's tempo: the tempo event in effect at `pos`.
#[tauri::command]
pub fn set_tempo_at(state: State<AppState>, pos: i64, bpm: f64, move_midi: Option<bool>) -> Snapshot {
    edit(&state, "Tempo", Some("tempo".into()), false, |s| s.set_tempo_at(pos, bpm, move_midi.unwrap_or(false)))
}

/// The LCD's signature: the change in effect at `bar`.
#[tauri::command]
pub fn set_signature_at(state: State<AppState>, bar: u32, num: u32, den: u32) -> Snapshot {
    edit(&state, "Time Signature", None, false, |s| s.set_signature_at(bar, num, den))
}

/// Moves MIDI regions to keep their bars and beats from one Tempo track to another.
#[tauri::command]
pub fn move_midi_with_tempo(state: State<AppState>, old_map: Vec<TempoEvent>, new_map: Vec<TempoEvent>) -> Snapshot {
    edit(&state, "Move MIDI Regions with Tempo", None, false, |s| s.move_midi_between(&old_map, &new_map))
}

// ---- built-in effects -----------------------------------------------------------------------

/// The built-in effects, as src/fx.ts `FxInfo`.
#[tauri::command]
pub fn list_builtin_effects() -> Vec<daw_engine::session::BuiltinInfo> {
    daw_engine::session::builtin_effects()
}

/// Sets a built-in effect's parameter (plain units). A drag of one parameter makes one undo
/// step; while automation is being written the move belongs to the pass instead.
#[tauri::command]
pub fn set_builtin_param(state: State<AppState>, track: Id, slot: i64, id: u32, value: f32) -> R<Snapshot> {
    let slot = slot_index(slot);
    if with(&state, |s| s.automation_writing(track)) {
        return try_snap(&state, |s| s.set_builtin_param(track, slot, id, value));
    }
    let uid = with(&state, |s| {
        let inserts = if track == daw_engine::model::MASTER_ID { Some(&s.project.master.inserts) } else { s.project.track(track).map(|t| &t.inserts) };
        inserts.and_then(|i| i.get(slot)).and_then(|p| p.as_ref()).map(|p| p.uid)
    });
    let key = uid.map(|uid| format!("fxparam:{uid}:{id}"));
    try_edit(&state, "Plug-in Parameter", key, false, |s| s.set_builtin_param(track, slot, id, value))
}

/// Every parameter value of a built-in effect instance: (id, plain value).
#[tauri::command]
pub fn builtin_params(state: State<AppState>, uid: Id) -> Vec<(u32, f32)> {
    with(&state, |s| s.builtin_params(uid))
}

/// Keys a built-in effect's side-chain from a track (null: its own input); refuses loops.
#[tauri::command]
pub fn set_sidechain(state: State<AppState>, track: Id, slot: i64, source: Option<Id>) -> R<Snapshot> {
    try_edit(&state, "Side Chain", None, false, |s| s.set_sidechain(track, slot_index(slot), source))
}

/// A built-in effect's panel opened or closed: its meters and live values come with each tick
/// while it is open.
#[tauri::command]
pub fn watch_builtin(state: State<AppState>, uid: Id, on: bool) {
    with(&state, |s| s.watch_builtin(uid, on))
}

// ---- copy and paste, presets ----------------------------------------------------------------

/// Preset and paste commands address a strip's Channel EQ as slot -2, the instrument as -1.
fn preset_slot(slot: i64) -> usize {
    if slot == -2 {
        daw_engine::session::EQ_SLOT
    } else {
        slot_index(slot)
    }
}

/// A slot's complete settings (a VST3's state included), for the UI's clipboard.
#[tauri::command]
pub fn copy_plugin(state: State<AppState>, track: Id, slot: i64) -> R<daw_engine::model::PluginSlot> {
    with(&state, |s| s.copy_plugin(track, slot_index(slot)).map_err(|e| e.to_string()))
}

/// Pastes a copied slot into `slot` of each of `tracks`, as one undo step.
#[tauri::command]
pub fn paste_plugin(state: State<AppState>, tracks: Vec<Id>, slot: i64, plugin: daw_engine::model::PluginSlot) -> R<Snapshot> {
    try_edit(&state, "Paste Plug-in", None, true, |s| s.paste_plugin(&tracks, slot_index(slot), &plugin))
}

/// Pastes Channel EQ settings onto each strip, as one undo step.
#[tauri::command]
pub fn paste_eq(state: State<AppState>, tracks: Vec<Id>, eq: ChannelEq) -> R<Snapshot> {
    try_edit(&state, "Paste Channel EQ", None, false, |s| s.paste_eq(&tracks, &eq))
}

/// An effect's factory and user presets (`builtin:channel-eq` for the Channel EQ).
#[tauri::command]
pub fn list_presets(effect: String) -> daw_engine::presets::PresetList {
    daw_engine::presets::list(&daw_engine::presets::user_root(), &effect)
}

/// One preset's settings.
#[tauri::command]
pub fn get_preset(effect: String, name: String, factory: bool) -> R<daw_engine::presets::Preset> {
    daw_engine::presets::get(&daw_engine::presets::user_root(), &effect, &name, factory).map_err(|e| e.to_string())
}

/// Applies a preset to a built-in effect's slot or (slot -2) the strip's Channel EQ.
#[tauri::command]
pub fn load_preset(state: State<AppState>, track: Id, slot: i64, effect: String, name: String, factory: bool) -> R<Snapshot> {
    try_edit(&state, "Load Preset", None, false, |s| s.load_preset(track, preset_slot(slot), &effect, &name, factory))
}

/// Saves a slot's (or, slot -2, the Channel EQ's) settings as a user preset. Refuses to replace
/// an existing one unless `overwrite`. Returns the name it was saved under.
#[tauri::command]
pub fn save_preset(state: State<AppState>, track: Id, slot: i64, name: String, overwrite: Option<bool>) -> R<String> {
    with(&state, |s| s.save_preset(track, preset_slot(slot), &name, overwrite.unwrap_or(false)).map_err(|e| e.to_string()))
}

#[tauri::command]
pub fn delete_preset(effect: String, name: String) -> R<()> {
    daw_engine::presets::delete_user(&daw_engine::presets::user_root(), &effect, &name).map_err(|e| e.to_string())
}

/// Renames a user preset; returns the new name.
#[tauri::command]
pub fn rename_preset(effect: String, old: String, new: String) -> R<String> {
    daw_engine::presets::rename_user(&daw_engine::presets::user_root(), &effect, &old, &new).map_err(|e| e.to_string())
}

/// Drags a plug-in to another Audio FX slot: on the same strip it moves (the plug-ins between
/// shift over); onto another strip it moves there, or is copied when `copy` (Alt-drag).
#[tauri::command]
pub fn move_plugin(state: State<AppState>, from: Id, from_slot: i64, to: Id, to_slot: i64, copy: bool) -> R<Snapshot> {
    let label = if copy { "Copy Plug-in" } else { "Move Plug-in" };
    try_edit(&state, label, None, true, |s| s.move_plugin(from, slot_index(from_slot), to, slot_index(to_slot), copy))
}

/// Sets fade curves (-1..1) and types for the Region inspector; omitted settings stay. Curve
/// drags of the same regions share an undo step.
#[tauri::command]
pub fn set_region_fade_shape(state: State<AppState>, ids: Vec<Id>, in_curve: Option<f32>, out_curve: Option<f32>, in_type: Option<FadeInType>, out_type: Option<FadeOutType>) -> Snapshot {
    let curve = in_curve.is_some() || out_curve.is_some();
    let key = curve.then(|| format!("fade-curve:{ids:?}"));
    edit(&state, if curve { "Fade Curve" } else { "Fade Type" }, key, false, |s| s.set_region_fade_shape(&ids, in_curve, out_curve, in_type, out_type))
}

/// Crossfades each region with the one that follows it over `length` frames (0 removes it).
#[tauri::command]
pub fn crossfade_regions(state: State<AppState>, ids: Vec<Id>, length: i64, kind: Option<FadeOutType>) -> Snapshot {
    edit(&state, if length > 0 { "Crossfade" } else { "Remove Crossfade" }, None, false, |s| s.crossfade_regions(&ids, length, kind))
}

/// The Tracks area's drag mode (not an undo step).
#[tauri::command]
pub fn set_drag_mode(state: State<AppState>, mode: DragMode) -> Snapshot {
    snap(&state, |s| s.set_drag_mode(mode))
}
