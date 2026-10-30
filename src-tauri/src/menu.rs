//! The native menu bar. Items emit a `menu` event carrying their id to the frontend (to the
//! focused detached panel window, if any; see windows.rs).

use tauri::menu::{AboutMetadata, MenuBuilder, MenuItem, PredefinedMenuItem, SubmenuBuilder};
use tauri::{App, Runtime};

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
    let app_menu = SubmenuBuilder::new(app, "Fermata")
        .item(&PredefinedMenuItem::about(app, Some("About Fermata"), Some(about))?)
        .separator()
        .item(&item("settings", "Settings…", Some("CmdOrCtrl+,"))?)
        .item(&item("settings-appearance", "Appearance…", None)?)
        .item(&item("settings-keys", "Key Commands…", None)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some("Quit Fermata"))?)
        .build()?;
    let file = SubmenuBuilder::new(app, "File")
        .item(&item("new", "New…", Some("CmdOrCtrl+N"))?)
        .item(&item("chooser-recent", "Open Recent…", Some("Shift+CmdOrCtrl+O"))?)
        .item(&item("open", "Open…", Some("CmdOrCtrl+O"))?)
        .separator()
        .item(&item("save", "Save", Some("CmdOrCtrl+S"))?)
        .item(&item("save-as", "Save As…", Some("Shift+CmdOrCtrl+S"))?)
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
    let track = SubmenuBuilder::new(app, "Track")
        .item(&item("new-tracks", "New Tracks…", Some("Alt+CmdOrCtrl+N"))?)
        .item(&item("new-aux", "New Aux", None)?)
        .item(&item("duplicate-track", "Duplicate Track", None)?)
        .item(&item("same-settings", "New Track with Same Settings", None)?)
        .separator()
        .item(&item("bounce-in-place", "Bounce in Place…", Some(BOUNCE_IN_PLACE_KEY))?)
        .item(&item("freeze-track", "Freeze Track", None)?)
        .item(&item("unfreeze-track", "Unfreeze Track", None)?)
        .separator()
        .item(&item("folder-stack", "Create Folder Stack", Some("Shift+CmdOrCtrl+D"))?)
        .item(&item("summing-stack", "Create Summing Stack", None)?)
        .item(&item("group", "Group Selected Tracks", Some("CmdOrCtrl+G"))?)
        .separator()
        .item(&item("hide-tracks", "Hide Track", None)?)
        .item(&item("unhide-all", "Unhide All Tracks", None)?)
        .separator()
        .item(&item("delete-track", "Delete Track", Some("CmdOrCtrl+Backspace"))?)
        .item(&item("delete-unused", "Delete Unused Tracks", None)?)
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
        // Toggles: a detached panel's own window, or back into the main window.
        .item(&item("detach:mixer", "Mixer in Separate Window", None)?)
        .item(&item("detach:editor", "Editor in Separate Window", None)?)
        .item(&item("browser", "Browsers", None)?)
        .item(&item("automation", "Show Automation", None)?)
        .item(&item("global-tracks", "Show Global Tracks", None)?)
        .item(&item("overview", "Show Overview", None)?)
        .item(&item("musical-typing", "Musical Typing", Some("CmdOrCtrl+K"))?)
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
    app.on_menu_event(|app, ev| crate::windows::route_menu(app, ev.id().0.clone()));
    Ok(())
}
