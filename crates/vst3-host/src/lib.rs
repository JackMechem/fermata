//! Minimal VST3 hosting: module loading, scanning, instantiation, processing and editor windows.

pub mod host;
pub mod module;
pub mod plugin;
pub mod process;
pub mod scan;
pub mod stream;
mod util;

#[cfg(target_os = "linux")]
#[path = "editor_linux.rs"]
pub mod editor;
#[cfg(not(target_os = "linux"))]
#[path = "editor_other.rs"]
pub mod editor;

#[cfg(target_os = "macos")]
mod macos;

pub use module::PluginDescriptor;
pub use host::Gesture;
pub use plugin::{instantiate, MidiEvent, ParamInfo, PluginController, PluginProcessor, PluginState, TransportInfo};
