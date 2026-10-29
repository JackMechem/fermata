//! Opens one plugin's editor with a tiny poll()-based run loop (no GTK), for debugging editor
//! embedding: `cargo run -p vst3-host --example editor -- <name substring> [seconds]`.

// Editor windows are only embedded on Linux so far (see vst3-host's editor_other.rs).
#[cfg(target_os = "linux")]
#[path = "editor/linux.rs"]
mod linux;

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("plug-in editor windows are only implemented on Linux so far");
}
