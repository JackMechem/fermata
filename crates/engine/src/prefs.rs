//! Options chosen in Settings and Project Settings.
//!
//! [`Prefs`] are app-wide: the Tauri side keeps them in `~/.config/fermata/settings.json` and
//! hands them to the session with [`crate::Session::set_prefs`]. [`ClickSettings`] and
//! [`BitDepth`] also appear in the project document, so they are saved with each project, as
//! Logic does with its metronome and recording settings.
//!
//! Every struct uses `#[serde(default)]`, so settings files and projects written by older
//! versions (or with fields a newer version dropped) still load.

use serde::{Deserialize, Serialize};

/// Sample format for new audio recordings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitDepth {
    /// 24-bit integer PCM: the usual studio interchange format, three quarters of the size.
    Int24,
    /// 32-bit float: never clips on disk, even when the input runs hot.
    #[default]
    Float32,
}

/// The metronome's built-in sounds, synthesized in the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClickSound {
    /// A short sine tick (Fermata's original click).
    #[default]
    Classic,
    /// A bright, very short transient that cuts through a dense mix.
    Click,
    /// Two damped partials, like a wood block.
    Woodblock,
    /// A longer electronic beep.
    Beep,
}

/// The metronome settings, saved with the project.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClickSettings {
    pub sound: ClickSound,
    /// Overall click level in dB (0 = the default level, about -6 dBFS on the downbeat).
    pub volume_db: f32,
    /// The first beat of each bar is louder and higher.
    pub accent: bool,
    /// Pitch offsets in semitones for the bar (accented) and the other beats.
    pub bar_pitch: i32,
    pub beat_pitch: i32,
    /// Clicks per beat: 1 = beats only, 2 = eighths, 3 = triplets, 4 = sixteenths. The extra
    /// clicks are quieter.
    pub division: u32,
    /// Click while playing (not recording).
    pub while_playing: bool,
    /// Click while recording.
    pub while_recording: bool,
    /// While recording, click only during the count-in.
    pub only_count_in: bool,
}

impl Default for ClickSettings {
    fn default() -> Self {
        ClickSettings {
            sound: ClickSound::Classic,
            volume_db: 0.0,
            accent: true,
            bar_pitch: 0,
            beat_pitch: 0,
            division: 1,
            while_playing: true,
            while_recording: true,
            only_count_in: false,
        }
    }
}

/// What happens when a new audio recording overlaps regions already on its track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioOverlap {
    /// The take goes on top; older audio it covers stays but is not heard.
    #[default]
    OnTop,
    /// The take goes on a new track below (Logic's "Create tracks").
    NewTrack,
    /// The parts of older regions under the take are removed (files are kept).
    Replace,
}

/// What happens when a new MIDI recording overlaps a MIDI region on its track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MidiOverlap {
    /// The recording becomes a region of its own.
    #[default]
    NewRegion,
    /// The notes are merged into the region they overlap.
    Merge,
    /// The notes and controllers in the recorded range (punch-in to where recording stopped)
    /// are removed from the regions underneath, and the recording is merged in their place.
    Replace,
}

/// How new recordings are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TakeNames {
    /// "Vocals#03"
    #[default]
    TrackNumber,
    /// "Vocals" (files are still numbered so they stay unique)
    Track,
    /// "Song - Vocals#03"
    ProjectTrack,
}

/// How hardware MIDI note velocities are shaped before they reach the instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VelocityCurve {
    #[default]
    Linear,
    /// Light playing comes out louder (for stiff keyboards).
    Soft,
    /// It takes a firmer touch to reach high velocities.
    Hard,
    /// Every note plays at `MidiPrefs::fixed_velocity`.
    Fixed,
}

impl VelocityCurve {
    /// Maps a note-on velocity (1..=127). Allocation-free; called from the MIDI input thread.
    pub fn apply(self, v: u8, fixed: u8) -> u8 {
        let x = v.clamp(1, 127) as f32 / 127.0;
        let y = match self {
            VelocityCurve::Linear => return v.clamp(1, 127),
            VelocityCurve::Fixed => return fixed.clamp(1, 127),
            VelocityCurve::Soft => x.powf(0.6),
            VelocityCurve::Hard => x.powf(1.7),
        };
        (y * 127.0).round().clamp(1.0, 127.0) as u8
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> VelocityCurve {
        match v {
            1 => VelocityCurve::Soft,
            2 => VelocityCurve::Hard,
            3 => VelocityCurve::Fixed,
            _ => VelocityCurve::Linear,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralPrefs {
    /// Where new projects go; None = ~/Music/Fermata.
    pub project_folder: Option<String>,
    /// Minutes between autosaves of a project with unsaved changes; 0 = off.
    pub autosave_minutes: u32,
    /// How many edits Undo can step back through.
    pub undo_limit: usize,
    /// Autosave as soon as a recording, Bounce in Place, Freeze or import finishes, instead of
    /// waiting for the interval.
    pub autosave_after_recording: bool,
}

impl Default for GeneralPrefs {
    fn default() -> Self {
        GeneralPrefs { project_folder: None, autosave_minutes: 2, undo_limit: 100, autosave_after_recording: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingPrefs {
    /// Format of new recordings, unless the project overrides it.
    pub bit_depth: BitDepth,
    /// Bars of count-in when Count-in is on (1..=4).
    pub count_in_bars: u32,
    /// Seconds played before the record position when Count-in is off.
    pub pre_roll_secs: f32,
    pub audio_overlap: AudioOverlap,
    pub midi_overlap: MidiOverlap,
    pub take_names: TakeNames,
    /// Folder for new recordings; None = the project's Audio Files. A project subfolder is
    /// made inside it.
    pub folder: Option<String>,
    /// The control bar's Replace button: every recording replaces what it overlaps, audio and
    /// MIDI alike, whatever `audio_overlap` and `midi_overlap` say.
    pub replace: bool,
}

impl RecordingPrefs {
    /// What an overlapping audio recording does, with the Replace button applied.
    pub fn audio_mode(&self) -> AudioOverlap {
        if self.replace { AudioOverlap::Replace } else { self.audio_overlap }
    }

    /// What an overlapping MIDI recording does, with the Replace button applied.
    pub fn midi_mode(&self) -> MidiOverlap {
        if self.replace { MidiOverlap::Replace } else { self.midi_overlap }
    }
}

impl Default for RecordingPrefs {
    fn default() -> Self {
        RecordingPrefs {
            bit_depth: BitDepth::Float32,
            count_in_bars: 1,
            pre_roll_secs: 0.0,
            audio_overlap: AudioOverlap::OnTop,
            midi_overlap: MidiOverlap::NewRegion,
            take_names: TakeNames::TrackNumber,
            folder: None,
            replace: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MidiPrefs {
    /// Names of MIDI input ports that are not connected.
    pub disabled_inputs: Vec<String>,
    /// Live MIDI plays the selected instrument track. Off, it is only recorded.
    pub thru: bool,
    pub velocity: VelocityCurve,
    pub fixed_velocity: u8,
    /// Chase: Notes. Starting playback (or jumping, or wrapping a cycle) inside a long note
    /// starts it, as Logic does.
    pub chase_notes: bool,
    /// Chase: Controllers. The latest value of each controller (sustain, modulation, pitch
    /// bend...) before the playhead is sent first.
    pub chase_controllers: bool,
}

impl Default for MidiPrefs {
    fn default() -> Self {
        MidiPrefs { disabled_inputs: Vec::new(), thru: true, velocity: VelocityCurve::Linear, fixed_velocity: 100, chase_notes: true, chase_controllers: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditingPrefs {
    /// Fade-in and fade-out given to new audio regions (recordings and imports), in
    /// milliseconds, to avoid clicks at their edges; 0 = none.
    pub fade_ms: f32,
    /// Whether a region's automation moves with it (Logic's Move Automation with Regions).
    pub move_automation: MoveAutomation,
}

impl Default for EditingPrefs {
    fn default() -> Self {
        EditingPrefs { fade_ms: 0.0, move_automation: MoveAutomation::Never }
    }
}

/// Settings › Editing › Move Automation with Regions: whether moving a region along its track
/// takes the track's automation under it along (see `auto_move.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveAutomation {
    #[default]
    Never,
    Always,
    /// The Tracks area asks on each move that would take automation along.
    Ask,
}

/// Plug-in Delay Compensation, as in Logic's Settings › Audio › General.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdcMode {
    /// Every path (tracks, stacks, auxes and sends) is delayed to line up at Stereo Out.
    #[default]
    All,
    /// No compensation: latent plug-ins put their tracks late.
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LatencyPrefs {
    pub compensation: PdcMode,
    /// Low Latency Mode: on record-enabled tracks (and audio tracks monitoring their input),
    /// plug-ins with more latency than the limit are switched off and the track skips its
    /// compensation delay, so playing live feels immediate.
    pub low_latency: bool,
    pub low_latency_limit_ms: f32,
}

impl Default for LatencyPrefs {
    fn default() -> Self {
        LatencyPrefs { compensation: PdcMode::All, low_latency: false, low_latency_limit_ms: 5.0 }
    }
}

/// App-wide behaviour of the session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub general: GeneralPrefs,
    pub recording: RecordingPrefs,
    pub midi: MidiPrefs,
    pub editing: EditingPrefs,
    /// Metronome settings given to new projects.
    pub click: ClickSettings,
    /// Settings › Audio: plug-in delay compensation and Low Latency Mode.
    pub latency: LatencyPrefs,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_json_gives_defaults() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p, Prefs::default());
        assert_eq!(p.general.undo_limit, 100);
        assert_eq!(p.recording.bit_depth, BitDepth::Float32);
        assert!(p.midi.thru);
        assert!(p.click.while_playing && p.click.accent);
    }

    #[test]
    fn partial_json_keeps_other_defaults() {
        let p: Prefs = serde_json::from_str(
            r#"{"recording":{"bit_depth":"int24","count_in_bars":2},"click":{"sound":"woodblock"},"unknown":1}"#,
        )
        .unwrap();
        assert_eq!(p.recording.bit_depth, BitDepth::Int24);
        assert_eq!(p.recording.count_in_bars, 2);
        assert_eq!(p.recording.audio_overlap, AudioOverlap::OnTop);
        assert_eq!(p.click.sound, ClickSound::Woodblock);
        assert_eq!(p.click.division, 1);
        assert_eq!(p.general, GeneralPrefs::default());
    }

    #[test]
    fn round_trip() {
        let mut p = Prefs::default();
        p.general.project_folder = Some("/tmp/x".into());
        p.midi.disabled_inputs = vec!["Port A".into()];
        p.midi.velocity = VelocityCurve::Fixed;
        p.recording.audio_overlap = AudioOverlap::Replace;
        p.click.bar_pitch = -5;
        let text = serde_json::to_string(&p).unwrap();
        let back: Prefs = serde_json::from_str(&text).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn replace_button_overrides_both_overlap_settings() {
        let mut r = RecordingPrefs { midi_overlap: MidiOverlap::Merge, ..Default::default() };
        assert_eq!((r.audio_mode(), r.midi_mode()), (AudioOverlap::OnTop, MidiOverlap::Merge));
        r.replace = true;
        assert_eq!((r.audio_mode(), r.midi_mode()), (AudioOverlap::Replace, MidiOverlap::Replace));
        // Older settings files have neither the button nor the chase switches.
        let p: Prefs = serde_json::from_str(r#"{"recording":{"midi_overlap":"replace"},"midi":{"thru":false}}"#).unwrap();
        assert!(!p.recording.replace && p.recording.midi_mode() == MidiOverlap::Replace);
        assert!(p.midi.chase_notes && p.midi.chase_controllers);
    }

    #[test]
    fn velocity_curves() {
        assert_eq!(VelocityCurve::Linear.apply(64, 100), 64);
        assert_eq!(VelocityCurve::Fixed.apply(20, 100), 100);
        assert!(VelocityCurve::Soft.apply(40, 0) > 40);
        assert!(VelocityCurve::Hard.apply(40, 0) < 40);
        assert_eq!(VelocityCurve::Hard.apply(127, 0), 127);
        assert!(VelocityCurve::Hard.apply(1, 0) >= 1);
        for c in [VelocityCurve::Linear, VelocityCurve::Soft, VelocityCurve::Hard, VelocityCurve::Fixed] {
            assert_eq!(VelocityCurve::from_u8(c.to_u8()), c);
        }
    }
}
