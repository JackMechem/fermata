//! Detached panel windows: a bottom-section panel (the Mixer, the editor, a Channel EQ…) shown
//! in its own window, labelled `panel-<key>`. The frontend decides what is detached and keeps
//! the windows' UI state in step (src/detach.ts); this module opens, focuses and closes the
//! windows, sends a fresh snapshot to the other windows after one of them edits the project,
//! routes native menu items to the focused window, and closes the panels with the main window.

use serde::Deserialize;
use tauri::window::Color;
use tauri::{AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Runtime, State, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

use crate::{commands, AppState};

/// Labels of panel windows start with this; the capability in `capabilities/default.json`
/// grants `panel-*` the same access as the main window.
pub const PANEL_PREFIX: &str = "panel-";

/// The event carrying a snapshot to the windows that didn't make the edit (src/detach.ts).
const SYNC_EVENT: &str = "project-sync";

/// Whether `label` names a panel window (and only characters Tauri accepts in a label).
pub fn is_panel_label(label: &str) -> bool {
    label.len() > PANEL_PREFIX.len()
        && label.starts_with(PANEL_PREFIX)
        && label.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '/'))
}

/// A window's outer position and inner size in physical pixels, already fitted to the monitors
/// that are connected now (src/detachModel.ts `fitToMonitors`). In logical pixels on macOS,
/// where the screens share one coordinate space in points but a physical position would be
/// converted with the scale of the screen the window is on before it moves.
#[derive(Deserialize)]
pub struct Geometry {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    #[serde(default)]
    logical: bool,
}

/// Opens a panel window, or brings it to the front if it is already open. `width`/`height` are
/// the logical size for a first opening; `geometry` restores where it was last time.
///
/// Async because creating a window from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_panel_window(app: AppHandle, label: String, title: String, width: f64, height: f64, geometry: Option<Geometry>) -> Result<(), String> {
    if !is_panel_label(&label) {
        return Err(format!("not a panel window: {label}"));
    }
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    let w = WebviewWindowBuilder::new(&app, &label, WebviewUrl::default())
        .title(title)
        .inner_size(width, height)
        .min_inner_size(360.0, 180.0)
        .background_color(Color(0x1c, 0x1c, 0x1e, 0xff))
        // Placed before it is shown, so it doesn't appear in one spot and jump to another.
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    if let Some(g) = geometry {
        // Ignored on native Wayland, where the compositor places windows.
        if g.logical {
            let _ = w.set_size(LogicalSize::new(g.w.max(1), g.h.max(1)));
            let _ = w.set_position(LogicalPosition::new(g.x, g.y));
        } else {
            let _ = w.set_size(PhysicalSize::new(g.w.max(1), g.h.max(1)));
            let _ = w.set_position(PhysicalPosition::new(g.x, g.y));
        }
    }
    let _ = w.show();
    let _ = w.set_focus();
    Ok(())
}

/// Closes a panel window without asking it (attaching the panel, or its content went away).
#[tauri::command]
pub fn close_panel_window(app: AppHandle, label: String) {
    if !is_panel_label(&label) {
        return;
    }
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.destroy();
    }
}

/// Brings a panel window to the front (its toggle was used while it is detached).
#[tauri::command]
pub fn focus_panel_window(app: AppHandle, label: String) {
    if let Some(w) = app.get_webview_window(&label).filter(|_| is_panel_label(&label)) {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Sends the current snapshot to every window but the caller, which already has it from the
/// command it ran. Serialized once, however many windows there are.
#[tauri::command]
pub fn broadcast_snapshot(webview: tauri::Webview, state: State<AppState>) -> Result<(), String> {
    let from = webview.label().to_string();
    let snap = commands::snapshot(&state.session.lock());
    let json = serde_json::to_string(&snap).map_err(|e| e.to_string())?;
    // The windows listen for it on their own label (`getCurrentWebviewWindow().listen`), so a
    // filter on the target reaches exactly the other windows.
    webview
        .app_handle()
        .emit_str_filter(SYNC_EVENT, json, |t| matches!(t, EventTarget::WebviewWindow { label } if *label != from))
        .map_err(|e| e.to_string())
}

/// Sends a native menu item to the focused panel window, which handles what belongs to its
/// panel (Select All in a detached Piano Roll) and passes the rest on to the main window;
/// otherwise to the main window, as before panels could be detached.
pub fn route_menu<R: Runtime>(app: &AppHandle<R>, id: String) {
    let panel = app.webview_windows().into_iter().find(|(label, w)| is_panel_label(label) && w.is_focused().unwrap_or(false)).map(|(label, _)| label);
    let _ = app.emit_to(EventTarget::webview_window(panel.as_deref().unwrap_or("main")), "menu", id);
}

/// Panel windows close with the main window, so quitting doesn't leave them behind (and the
/// app exits when the last one goes, as it did with one window). Their detached state is kept
/// for the next launch: being destroyed, they don't ask to be attached.
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if window.label() == "main" && matches!(event, WindowEvent::Destroyed) {
        for (label, w) in window.app_handle().webview_windows() {
            if is_panel_label(&label) {
                let _ = w.destroy();
            }
        }
    }
}

/// The event carrying a key that a plug-in editor window didn't use to the main window, which
/// handles it as if pressed there (src/pluginKeys.ts).
const PLUGIN_KEY_EVENT: &str = "plugin-key";

/// Plug-in editor windows pass the keys their plug-in doesn't use to the main window, as
/// Logic's do (macOS; elsewhere editors don't report keys).
pub fn forward_plugin_keys<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    vst3_host::keys::set_key_forwarder(Box::new(move |k| {
        let _ = app.emit_to(EventTarget::webview_window("main"), PLUGIN_KEY_EVENT, k);
    }));
}

#[cfg(test)]
mod tests {
    use super::is_panel_label;

    #[test]
    fn panel_labels() {
        assert!(is_panel_label("panel-mixer"));
        assert!(is_panel_label("panel-eq:12"));
        assert!(!is_panel_label("panel-"));
        assert!(!is_panel_label("main"));
        assert!(!is_panel_label("panel-a b"));
    }
}
