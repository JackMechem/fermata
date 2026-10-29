//! Fermata's audio engine: project model, real-time graph, device I/O, recording and MIDI.

pub mod audio_io;
pub mod automation;
pub mod clip;
pub mod decode;
pub mod dsp;
pub mod eq;
pub mod midi;
pub mod midi_io;
pub mod model;
pub mod plugin_db;
pub mod prefs;
pub mod recorder;
pub mod session;
pub mod smf;
pub mod synth;
#[cfg(target_os = "linux")]
pub mod sysvol;
pub mod takes;
pub mod transients;

pub use session::Session;
pub use vst3_host;
