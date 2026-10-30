//! The native menu bar. Items emit a `menu` event carrying their id to the frontend (to the
//! focused detached panel window, if any; see windows.rs).

use serde::Deserialize;
use tauri::menu::{AboutMetadata, CheckMenuItem, MenuBuilder, MenuItem, PredefinedMenuItem, Submenu, SubmenuBuilder};
use tauri::{App, AppHandle, Manager, Runtime};

/// Logic's ⌃B. Elsewhere Ctrl+B is already Bounce Project (⌘B), so Bounce in Place adds Alt.
const BOUNCE_IN_PLACE_KEY: &str = if cfg!(target_os = "macos") { "Ctrl+B" } else { "Ctrl+Alt+B" };

pub fn install<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    let item = |id: &str, label: &str, accel: Option<&str>| MenuItem::with_id(app, id, label, true, accel);

    let about = AboutMetadata {
        name: Some("Fermata".into()),
        version: Some(app.package_info().version.to_string()),
        comments: Some("A digital audio workstation with VST3 hosting, modelled on Logic Pro.".into()),
        license: Some("MIT".into()),
        website: Some("https://github.com/JackMechem/fermata".into()),
        ..Default::default()
    };
    // Logic Pro › Control Surfaces. The keys live in the keymap.
    let control_surfaces = SubmenuBuilder::new(app, "Control Surfaces")
        .item(&item("learn-assignment", "Learn Assignment for Last Touched Parameter", None)?)
        .item(&item("controller-assignments", "Controller Assignments…", None)?)
        .build()?;
    let app_menu = SubmenuBuilder::new(app, "Fermata")
        .item(&PredefinedMenuItem::about(app, Some("About Fermata"), Some(about))?)
        .separator()
        .item(&item("settings", "Settings…", Some("CmdOrCtrl+,"))?)
        .item(&item("settings-appearance", "Appearance…", None)?)
        .item(&item("settings-keys", "Key Commands…", None)?)
        .item(&control_surfaces)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some("Quit Fermata"))?)
        .build()?;
    // Filled in by set_project_menus once a project is open.
    let alternatives = SubmenuBuilder::new(app, "Project Alternatives").build()?;
    let revert = SubmenuBuilder::new(app, "Revert to").build()?;
    fill_project_menus(app.handle(), &alternatives, &revert, &[], "", &[])?;
    let file = SubmenuBuilder::new(app, "File")
        .item(&item("new", "New…", Some("CmdOrCtrl+N"))?)
        .item(&item("chooser-recent", "Open Recent…", Some("Shift+CmdOrCtrl+O"))?)
        .item(&item("open", "Open…", Some("CmdOrCtrl+O"))?)
        .separator()
        .item(&item("save", "Save", Some("CmdOrCtrl+S"))?)
        .item(&item("save-as", "Save As…", Some("Shift+CmdOrCtrl+S"))?)
        .item(&item("save-template", "Save as Template…", None)?)
        .item(&item("revert-saved", "Revert to Saved", None)?)
        .item(&revert)
        .separator()
        .item(&alternatives)
        .separator()
        .item(&item("project-settings", "Project Settings…", None)?)
        .separator()
        .item(&item("import-audio", "Import Audio File…", Some("Shift+CmdOrCtrl+I"))?)
        .separator()
        .item(&item("bounce", "Bounce Project or Section…", Some("CmdOrCtrl+B"))?)
        .item(&item("export-selected-tracks", "Export Selected Tracks as Audio Files…", Some("CmdOrCtrl+E"))?)
        .item(&item("export-tracks", "Export All Tracks as Audio Files…", Some("Shift+CmdOrCtrl+E"))?)
        .item(&item("import-midi", "Import MIDI File…", None)?)
        .item(&item("export-midi", "Export Selection as MIDI File…", None)?)
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .item(&item("undo", "Undo", Some("CmdOrCtrl+Z"))?)
        .item(&item("redo", "Redo", Some("Shift+CmdOrCtrl+Z"))?)
        .separator()
        // Cut/Copy/Paste carry no accelerators so text fields keep the clipboard keys; the
        // window handles ⌘X/C/V itself.
        .item(&item("cut", "Cut Regions", None)?)
        .item(&item("copy", "Copy Regions", None)?)
        .item(&item("paste", "Paste Regions", None)?)
        .item(&item("duplicate", "Duplicate", Some("CmdOrCtrl+D"))?)
        .item(&item("repeat", "Repeat Regions", None)?)
        .separator()
        .item(&item("split", "Split Regions at Playhead", Some("CmdOrCtrl+T"))?)
        .item(&item("join", "Join Regions", Some("CmdOrCtrl+J"))?)
        .item(&item("loop", "Loop Regions", None)?)
        .item(&item("convert-loops", "Convert Loops to Real Copies", None)?)
        .item(&item("pack-takes", "Pack Take Folder", None)?)
        .item(&item("set-locators", "Set Locators by Regions", None)?)
        .item(&item("select-all", "Select All", Some("CmdOrCtrl+A"))?)
        .separator()
        .item(&item("delete", "Delete", None)?)
        .build()?;
    let track_alternatives = SubmenuBuilder::new(app, "Track Alternatives")
        .item(&item("track-alt-new", "New Alternative", None)?)
        .item(&item("track-alt-duplicate", "Duplicate Alternative", None)?)
        .item(&item("track-alt-rename", "Rename Alternative…", None)?)
        .item(&item("track-alt-delete", "Delete Alternative", None)?)
        .build()?;
    let track = SubmenuBuilder::new(app, "Track")
        .item(&item("new-tracks", "New Tracks…", Some("Alt+CmdOrCtrl+N"))?)
        .item(&item("new-aux", "New Aux", None)?)
        .item(&item("duplicate-track", "Duplicate Track", None)?)
        .item(&item("same-settings", "New Track with Same Settings", None)?)
        .item(&item("new-pattern-region", "New Pattern Region", None)?)
        .separator()
        .item(&item("bounce-in-place", "Bounce in Place…", Some(BOUNCE_IN_PLACE_KEY))?)
        .item(&item("freeze-track", "Freeze Track", None)?)
        .item(&item("unfreeze-track", "Unfreeze Track", None)?)
        .separator()
        .item(&item("folder-stack", "Create Folder Stack", Some("Shift+CmdOrCtrl+D"))?)
        .item(&item("summing-stack", "Create Summing Stack", None)?)
        .item(&item("group", "Group Selected Tracks", Some("CmdOrCtrl+G"))?)
        .separator()
        .item(&item("output-track", "Show Output Track", None)?)
        .item(&item("hide-tracks", "Hide Track", None)?)
        .item(&item("unhide-all", "Unhide All Tracks", None)?)
        .separator()
        .item(&item("delete-track", "Delete Track", Some("CmdOrCtrl+Backspace"))?)
        .item(&item("delete-unused", "Delete Unused Tracks", None)?)
        .separator()
        .item(&track_alternatives)
        .build()?;
    let record = SubmenuBuilder::new(app, "Record")
        .item(&item("play", "Play / Stop", None)?)
        .item(&item("record", "Record", None)?)
        .separator()
        .item(&item("cycle", "Cycle", None)?)
        .item(&item("autopunch", "Autopunch", None)?)
        .item(&item("metronome", "Metronome", None)?)
        .item(&item("count-in", "Count-in", None)?)
        .separator()
        .item(&item("low-latency", "Low Latency Mode", None)?)
        .build()?;
    // Logic's Navigate menu, for the Marker track. The keys live in the keymap.
    let navigate = SubmenuBuilder::new(app, "Navigate")
        .item(&item("create-marker", "Create Marker", None)?)
        .item(&item("delete-marker", "Delete Marker", None)?)
        .separator()
        .item(&item("previous-marker", "Go to Previous Marker", None)?)
        .item(&item("next-marker", "Go to Next Marker", None)?)
        .separator()
        .item(&item("marker-list", "Open Marker List", None)?)
        .build()?;
    let themes = SubmenuBuilder::new(app, "Theme")
        .item(&item("theme:studio", "Studio", None)?)
        .item(&item("theme:flat", "Flat", None)?)
        .item(&item("theme:midnight", "Midnight", None)?)
        .item(&item("theme:light", "Light", None)?)
        .item(&item("theme:contrast", "High Contrast", None)?)
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&item("library", "Library", None)?)
        .item(&item("inspector", "Inspector", None)?)
        .item(&item("mixer", "Mixer", None)?)
        .item(&item("editor", "Editors", None)?)
        .item(&item("list-editors", "List Editors", None)?)
        // Toggles: a detached panel's own window, or back into the main window.
        .item(&item("detach:mixer", "Mixer in Separate Window", None)?)
        .item(&item("detach:editor", "Editor in Separate Window", None)?)
        .item(&item("detach:list", "Event List in Separate Window", None)?)
        .item(&item("browser", "Browsers", None)?)
        .item(&item("automation", "Show Automation", None)?)
        .item(&item("used-automation", "Show Used Automation", None)?)
        .item(&item("global-tracks", "Show Global Tracks", None)?)
        .item(&item("overview", "Show Overview", None)?)
        .item(&item("musical-typing", "Musical Typing", Some("CmdOrCtrl+K"))?)
        .item(&item("step-input", "Step Input Keyboard", None)?)
        .separator()
        .item(&item("show-hidden", "Show Hidden Tracks", None)?)
        .separator()
        .item(&item("zoom-in", "Zoom In", None)?)
        .item(&item("zoom-out", "Zoom Out", None)?)
        .separator()
        .item(&item("ui-larger", "Larger Interface", Some("CmdOrCtrl+Equal"))?)
        .item(&item("ui-smaller", "Smaller Interface", Some("CmdOrCtrl+Minus"))?)
        .item(&item("ui-actual", "Actual Size", Some("CmdOrCtrl+Digit0"))?)
        .separator()
        .item(&themes)
        .build()?;

    let menu = MenuBuilder::new(app).items(&[&app_menu, &file, &edit, &track, &record, &navigate, &view]).build()?;
    app.set_menu(menu)?;
    app.manage(ProjectMenus { alternatives, revert });
    app.on_menu_event(|app, ev| crate::windows::route_menu(app, ev.id().0.clone()));
    Ok(())
}

/// The File menu's lists that change with the project: Project Alternatives and Revert to.
struct ProjectMenus<R: Runtime> {
    alternatives: Submenu<R>,
    revert: Submenu<R>,
}

/// A backup in File › Revert to: its file (the item's id is `revert:<file>`) and its label in
/// local time, which the frontend formats.
#[derive(Deserialize)]
pub struct BackupItem {
    file: String,
    label: String,
}

fn fill_project_menus<R: Runtime>(app: &AppHandle<R>, alternatives: &Submenu<R>, revert: &Submenu<R>, names: &[String], active: &str, backups: &[BackupItem]) -> tauri::Result<()> {
    for it in alternatives.items()? {
        alternatives.remove(&it)?;
    }
    for name in names {
        alternatives.append(&CheckMenuItem::with_id(app, format!("alternative:{name}"), name, true, name == active, None::<&str>)?)?;
    }
    if !names.is_empty() {
        alternatives.append(&PredefinedMenuItem::separator(app)?)?;
    }
    alternatives.append(&MenuItem::with_id(app, "alt-new", "New Alternative…", true, None::<&str>)?)?;
    alternatives.append(&MenuItem::with_id(app, "alt-edit", "Edit Alternatives…", true, None::<&str>)?)?;
    for it in revert.items()? {
        revert.remove(&it)?;
    }
    if backups.is_empty() {
        revert.append(&MenuItem::with_id(app, "revert-none", "No Backups", false, None::<&str>)?)?;
    }
    for b in backups {
        revert.append(&MenuItem::with_id(app, format!("revert:{}", b.file), &b.label, true, None::<&str>)?)?;
    }
    Ok(())
}

/// Rebuilds File › Project Alternatives (`alternatives`, `active` ticked) and File › Revert to
/// (`backups`, newest first). The frontend calls it when the project, its alternative or its
/// saved state changes (src/projectFiles.ts).
#[tauri::command]
pub fn set_project_menus<R: Runtime>(app: AppHandle<R>, alternatives: Vec<String>, active: String, backups: Vec<BackupItem>) -> Result<(), String> {
    let Some(menus) = app.try_state::<ProjectMenus<R>>() else { return Ok(()) };
    fill_project_menus(&app, &menus.alternatives, &menus.revert, &alternatives, &active, &backups).map_err(|e| e.to_string())
}
