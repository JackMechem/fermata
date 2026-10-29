//! Opens one plugin's editor with a tiny run loop of its own (no GTK or Tauri), for debugging
//! editor embedding: `cargo run -p vst3-host --example editor -- <name substring> [seconds]`.

// Windows has no editor windows yet (see vst3-host's editor_other.rs).
#[cfg(target_os = "linux")]
#[path = "editor/linux.rs"]
mod linux;
#[cfg(target_os = "macos")]
#[path = "editor/macos.rs"]
mod macos;

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(target_os = "macos")]
fn main() {
    macos::main();
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("plug-in editor windows are only implemented on Linux and macOS so far");
}
