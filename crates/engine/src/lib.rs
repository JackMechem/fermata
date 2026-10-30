//! Fermata's audio engine: project model, real-time graph, device I/O, recording and MIDI.

pub mod audio_file;
pub mod audio_io;
pub mod auto_move;
pub mod automation;
pub mod clip;
pub mod decode;
pub mod denormals;
pub mod drag_modes;
pub mod dsp;
pub mod eq;
pub mod fades;
pub mod fx;
pub mod midi;
pub mod midi_io;
pub mod model;
pub mod plugin_db;
pub mod prefs;
pub mod presets;
pub mod recorder;
pub mod region_params;
pub mod session;
pub mod smf;
pub mod strip_settings;
pub mod synth;
#[cfg(target_os = "linux")]
pub mod sysvol;
pub mod takes;
pub mod tempo;
pub mod transients;
pub mod wav_repair;

pub use session::Session;
pub use vst3_host;
