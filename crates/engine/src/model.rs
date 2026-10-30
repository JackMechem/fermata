//! The project document: everything that gets saved and mirrored to the UI.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::fades::{DragMode, FadeShape};
use crate::prefs::{BitDepth, ClickSettings};
use crate::pattern::Pattern;
use crate::region_params::RegionParams;
use crate::tempo::{SigEvent, TempoEvent};

pub type Id = u64;

/// Track id reserved for the Stereo Out channel strip.
pub const MASTER_ID: Id = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Audio,
    Instrument,
    /// Folder stack main track: groups its subtracks; its fader, mute and solo apply to them.
    Folder,
    /// Summing stack main track: its subtracks play through it like a bus, with its own EQ,
    /// plug-ins and fader.
    Summing,
    /// Aux channel strip, as in Logic: no regions of its own. It plays what sends and routed
    /// outputs deliver to its bus, through its own EQ, plug-ins and fader.
    Aux,
}

impl TrackKind {
    pub fn is_stack(self) -> bool {
        matches!(self, TrackKind::Folder | TrackKind::Summing)
    }

    /// Tracks that can be record-enabled.
    pub fn can_record(self) -> bool {
        matches!(self, TrackKind::Audio | TrackKind::Instrument)
    }
}

/// Which hardware input channels feed an audio track (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "ch", rename_all = "snake_case")]
pub enum InputSel {
    None,
    Mono(u16),
    Stereo(u16),
}

impl InputSel {
    pub fn channels(&self) -> usize {
        match self {
            InputSel::None => 0,
            InputSel::Mono(_) => 1,
            InputSel::Stereo(_) => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSlot {
    /// Instance id, unique within the project.
    pub uid: Id,
    /// Plugin descriptor id, or `builtin:<name>`.
    pub plugin_id: String,
    pub name: String,
    #[serde(default)]
    pub bypass: bool,
    /// Base64 plugin state. Only populated in saved files, never sent to the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// A built-in effect's parameter values: (parameter id, value in its plain units). Ids
    /// missing here play at their defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<(u32, f32)>,
    /// The track whose post-fader output keys a built-in effect's side-chain (None: the
    /// effect's detector listens to its own input).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidechain: Option<Id>,
    /// The side-chain key is taken before the source's fader (still silenced by its mute)
    /// instead of after it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sidechain_pre: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Note {
    /// Offset from the region start, in samples.
    pub start: i64,
    pub length: i64,
    pub pitch: u8,
    pub velocity: u8,
    /// Muted notes stay in the region but don't play.
    #[serde(default)]
    pub muted: bool,
}

/// What a controller event drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "cc", rename_all = "snake_case")]
pub enum CcKind {
    /// A control change (CC 0–127), value 0–127.
    Controller(u8),
    /// Pitch bend, value 0–16383 with 8192 centred.
    PitchBend,
    /// Channel pressure (aftertouch), value 0–127.
    Aftertouch,
}

impl CcKind {
    /// A dense index (controllers 0–127, then pitch bend and aftertouch) for per-kind tables.
    pub fn index(self) -> usize {
        match self {
            CcKind::Controller(n) => (n & 127) as usize,
            CcKind::PitchBend => 128,
            CcKind::Aftertouch => 129,
        }
    }

    /// The inverse of `index`.
    pub fn from_index(i: usize) -> CcKind {
        match i {
            128 => CcKind::PitchBend,
            129 => CcKind::Aftertouch,
            n => CcKind::Controller((n & 127) as u8),
        }
    }

    /// The MIDI message that sets this controller to `value`.
    pub fn message(self, value: u16) -> [u8; 3] {
        match self {
            CcKind::Controller(n) => [0xB0, n & 127, value.min(127) as u8],
            CcKind::PitchBend => {
                let v = value.min(16383);
                [0xE0, (v & 127) as u8, (v >> 7) as u8]
            }
            CcKind::Aftertouch => [0xD0, value.min(127) as u8, 0],
        }
    }

    /// Reads a channel message back into a controller event, if it is one.
    pub fn from_message(d: [u8; 3]) -> Option<(CcKind, u16)> {
        match d[0] & 0xF0 {
            0xB0 => Some((CcKind::Controller(d[1] & 127), (d[2] & 127) as u16)),
            0xE0 => Some((CcKind::PitchBend, ((d[2] as u16 & 127) << 7) | (d[1] as u16 & 127))),
            0xD0 => Some((CcKind::Aftertouch, (d[1] & 127) as u16)),
            _ => None,
        }
    }
}

/// Number of distinct `CcKind::index` values.
pub const CC_KINDS: usize = 130;

/// A controller, pitch bend or aftertouch event in a MIDI region.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CcEvent {
    /// Offset from the region start, in samples.
    pub start: i64,
    pub kind: CcKind,
    /// 0–127, or 0–16383 for pitch bend.
    pub value: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RegionContent {
    Audio {
        clip: Id,
        /// Offset into the clip, in frames.
        offset: i64,
        #[serde(default)]
        gain_db: f32,
        /// Fade lengths in frames (0 = none).
        #[serde(default)]
        fade_in: i64,
        #[serde(default)]
        fade_out: i64,
        /// Fade curves and types (see `fades.rs`).
        #[serde(default, skip_serializing_if = "FadeShape::is_default")]
        fade_shape: FadeShape,
    },
    Midi {
        notes: Vec<Note>,
        /// Controller events, sorted by start.
        #[serde(default)]
        cc: Vec<CcEvent>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Region {
    pub id: Id,
    pub name: String,
    /// Timeline position in samples.
    pub start: i64,
    pub length: i64,
    #[serde(default)]
    pub muted: bool,
    /// Overrides the track colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub content: RegionContent,
    /// Timeline sample where a looping region stops repeating (None = not looping). The
    /// region's own `start..start + length` is what repeats, as with Logic's Loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_end: Option<i64>,
    /// Makes the region a take folder (cycle recording, or Pack Take Folder): it plays its
    /// comp instead of `content`'s clip. See [`TakeFolder`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takes: Option<TakeFolder>,
    /// Playback parameters (Logic's Region inspector: quantize, transpose, velocity, delay
    /// and so on), applied as the region plays. See [`crate::region_params`].
    #[serde(default, skip_serializing_if = "RegionParams::is_default")]
    pub params: RegionParams,
    /// Makes a MIDI region a pattern region (Logic's Step Sequencer): it plays the notes its
    /// pattern makes instead of `content`'s. See [`crate::pattern`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Pattern>,
}

/// A loop can't repeat its region more often than this.
pub const MAX_LOOP_PASSES: i64 = 4096;

impl Region {
    /// Where the region stops sounding on the timeline: its loop end, or its own end.
    pub fn end(&self) -> i64 {
        let own = self.start + self.length;
        match self.loop_end {
            Some(e) if self.length > 0 => e.clamp(own, self.start + self.length * MAX_LOOP_PASSES),
            _ => own,
        }
    }

    pub fn is_looped(&self) -> bool {
        self.end() > self.start + self.length
    }

    /// Timeline `(start, length)` of every pass through the region: the region itself, then
    /// each loop repetition, the last one cut short at the loop end.
    pub fn passes(&self) -> Vec<(i64, i64)> {
        let end = self.end();
        let mut out = vec![(self.start, self.length)];
        let mut at = self.start + self.length;
        while at < end {
            out.push((at, self.length.min(end - at)));
            at += self.length;
        }
        out
    }

    /// Moves the region to `start`, taking its loop end along.
    pub fn move_to(&mut self, start: i64) {
        let delta = start - self.start;
        self.start = start;
        self.loop_end = self.loop_end.map(|e| e + delta);
    }

    /// The loop as plain regions, one per pass (Logic's Convert Loops to Real Copies). Every
    /// copy keeps the original's id; the caller hands out new ones. Copies play exactly as
    /// the loop did: the fade-in stays on the first, the fade-out moves to the last, and the
    /// repetitions drop the notes that start past their end.
    pub fn real_copies(&self) -> Vec<Region> {
        let passes = self.passes();
        let last = passes.len() - 1;
        passes
            .into_iter()
            .enumerate()
            .map(|(k, (start, length))| {
                let mut r = Region { start, length, loop_end: None, ..self.clone() };
                match &mut r.content {
                    RegionContent::Audio { fade_in, fade_out, .. } if last > 0 => {
                        let fo = (*fade_out).min(length);
                        *fade_in = if k == 0 { *fade_in } else { 0 };
                        *fade_out = if k == last { fo } else { 0 };
                    }
                    RegionContent::Midi { notes, cc } if k > 0 => {
                        notes.retain(|n| n.start < length);
                        cc.retain(|c| c.start < length);
                    }
                    _ => {}
                }
                r
            })
            .collect()
    }
}

/// A take folder, Logic's container for the passes of a cycle recording.
///
/// Takes and the comp are laid out in *take time*: frames from the start of the recorded
/// passes. The folder region's `content` is still `RegionContent::Audio`, but its `offset` is
/// the take time at the region's start (so moving, trimming and splitting the folder work like
/// any audio region), its `clip` is the newest take's file, and its gain and fades apply to the
/// whole comp. At take time `t`, a take plays frame `take.offset + t` of its own clip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeFolder {
    /// Oldest first, so the newest take is last.
    pub takes: Vec<Take>,
    /// Which take is heard where: sorted, non-overlapping take-time ranges. Gaps are silent.
    #[serde(default)]
    pub comp: Vec<CompSegment>,
}

/// One pass of a cycle recording (or a region packed into a folder).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Take {
    pub id: Id,
    pub name: String,
    pub clip: Id,
    /// Clip frame at take time 0 (negative when the take's file starts later).
    pub offset: i64,
    /// The take-time range the take has audio for.
    pub start: i64,
    pub end: i64,
    #[serde(default)]
    pub gain_db: f32,
}

/// Part of a comp: take `take` is heard over take time `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompSegment {
    pub start: i64,
    pub end: i64,
    pub take: Id,
}

/// A marker on the Marker track (Logic's global marker lane).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub id: Id,
    /// Timeline position in samples.
    pub pos: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// A marker can span a section; None is a point marker that reaches the next one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<i64>,
}

/// A section on the Arrangement track (Logic's arrangement marker: Intro, Verse, Chorus…).
/// Sections tile the timeline: each starts where the one before it ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrangementMarker {
    pub id: Id,
    pub name: String,
    /// Timeline position in samples, like a marker's.
    pub start: i64,
    pub length: i64,
    /// None colours the section by its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl ArrangementMarker {
    pub fn end(&self) -> i64 {
        self.start + self.length
    }
}

/// The Autopunch range: while it is on, recording only captures between `start` and `end`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Punch {
    pub enabled: bool,
    pub start: i64,
    pub end: i64,
}

impl Punch {
    /// The punch range, when Autopunch is on and the range isn't empty.
    pub fn range(&self) -> Option<(i64, i64)> {
        (self.enabled && self.end > self.start).then_some((self.start, self.end))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BandKind {
    LowCut,
    LowShelf,
    Bell,
    HighShelf,
    HighCut,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EqBand {
    pub kind: BandKind,
    pub on: bool,
    /// Hz
    pub freq: f32,
    /// dB (ignored by the cut filters)
    pub gain: f32,
    pub q: f32,
    /// dB/octave for the cut filters: 12, 24 or 48.
    #[serde(default = "default_slope")]
    pub slope: u8,
}

fn default_slope() -> u8 {
    24
}

/// The built-in eight-band channel EQ every strip has, modelled on Logic's Channel EQ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEq {
    pub on: bool,
    pub bands: Vec<EqBand>,
    #[serde(default)]
    pub output_db: f32,
}

impl Default for ChannelEq {
    fn default() -> Self {
        let b = |kind, on, freq, q| EqBand { kind, on, freq, gain: 0.0, q, slope: 24 };
        ChannelEq {
            on: true,
            bands: vec![
                b(BandKind::LowCut, false, 30.0, 0.71),
                b(BandKind::LowShelf, true, 80.0, 0.71),
                b(BandKind::Bell, true, 160.0, 1.1),
                b(BandKind::Bell, true, 500.0, 1.1),
                b(BandKind::Bell, true, 1500.0, 1.1),
                b(BandKind::Bell, true, 5000.0, 1.1),
                b(BandKind::HighShelf, true, 10000.0, 0.71),
                b(BandKind::HighCut, false, 18000.0, 0.71),
            ],
            output_db: 0.0,
        }
    }
}

impl ChannelEq {
    /// True when the EQ would leave the signal untouched, so it can be skipped entirely.
    pub fn is_flat(&self) -> bool {
        !self.on
            || (self.output_db.abs() < 0.01
                && self.bands.iter().all(|b| {
                    !b.on || (matches!(b.kind, BandKind::Bell | BandKind::LowShelf | BandKind::HighShelf) && b.gain.abs() < 0.01)
                }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub kind: TrackKind,
    pub name: String,
    pub color: String,
    pub volume_db: f32,
    /// -1 (left) ..= 1 (right)
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    pub monitor: bool,
    pub input: InputSel,
    #[serde(default)]
    pub instrument: Option<PluginSlot>,
    #[serde(default)]
    pub inserts: Vec<Option<PluginSlot>>,
    #[serde(default)]
    pub regions: Vec<Region>,
    #[serde(default)]
    pub eq: ChannelEq,
    /// The track stack this track belongs to.
    #[serde(default)]
    pub parent: Option<Id>,
    /// For stack main tracks: subtracks hidden.
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub group: Option<Id>,
    /// Hidden from the tracks area and mixer (Logic's Hide Track). Hidden tracks still play.
    #[serde(default)]
    pub hidden: bool,
    /// Send slots (`SEND_SLOTS` of them; older projects have none). A send is addressed by
    /// track id + slot index.
    #[serde(default)]
    pub sends: Vec<Option<TrackSend>>,
    /// Explicit output: an aux, or `MASTER_ID` for Stereo Out. None is the default route: the
    /// summing stack the track sits in, else Stereo Out. An explicit output overrides the stack.
    #[serde(default)]
    pub output: Option<Id>,
    /// For auxes: the bus number feeding them ("Bus 3"). 0 for other tracks.
    #[serde(default)]
    pub bus: u32,
    /// Automation lanes, one per parameter.
    #[serde(default)]
    pub automation: Vec<AutoLane>,
    #[serde(default)]
    pub auto_mode: AutoMode,
    /// Set while the track is frozen: it plays this render of its instrument and plug-ins
    /// (pre-fader), and its plug-ins are unloaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen: Option<FreezeInfo>,
    /// The VCA whose level offsets this track's fader (see `vca.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vca: Option<Id>,
    /// Logic's Track Alternatives: the track's other sets of regions, kept while `regions` plays
    /// the active one (see `session/track_alternatives.rs`). Empty: the track has just one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<TrackAlternative>,
    /// The name of the active track alternative ("A" when empty).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alternative: String,
    /// Extra automation lanes shown below the track's main lane (Logic's sub-lanes), by the
    /// parameter each shows. The main lane's parameter is a view setting of the UI.
    #[serde(default)]
    pub auto_lanes: Vec<AutoParam>,
}

/// An inactive track alternative: a named set of regions that replaces the track's regions
/// when it is switched to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackAlternative {
    pub name: String,
    #[serde(default)]
    pub regions: Vec<Region>,
}

/// A frozen track's freeze file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreezeInfo {
    /// The clip holding the render (in the project's Freeze Files folder).
    pub clip: Id,
    /// Where on the timeline the render starts.
    pub start: i64,
}

/// A send: a copy of a strip's signal, at its own level, into an aux's bus. (Not called `Send`,
/// which would shadow the marker trait wherever the model is glob-imported.)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackSend {
    /// The aux track the send feeds.
    pub bus: Id,
    pub level_db: f32,
    /// Taps the signal after the plug-ins but before the fader and pan.
    #[serde(default)]
    pub pre_fader: bool,
    #[serde(default)]
    pub muted: bool,
}

/// What an automation lane drives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum AutoParam {
    Volume,
    Pan,
    Mute,
    /// A send level, by send slot.
    Send(u8),
    /// A plug-in parameter: instance id, VST3 parameter id, and its name for display.
    Plugin { uid: Id, param: u32, name: String },
}

impl AutoParam {
    /// Same parameter (a plug-in parameter's name is only a label).
    pub fn same(&self, other: &AutoParam) -> bool {
        match (self, other) {
            (AutoParam::Volume, AutoParam::Volume) | (AutoParam::Pan, AutoParam::Pan) | (AutoParam::Mute, AutoParam::Mute) => true,
            (AutoParam::Send(a), AutoParam::Send(b)) => a == b,
            (AutoParam::Plugin { uid: a, param: p, .. }, AutoParam::Plugin { uid: b, param: q, .. }) => a == b && p == q,
            _ => false,
        }
    }

    /// The range values are clamped to, in the parameter's native units.
    pub fn range(&self) -> (f32, f32) {
        match self {
            AutoParam::Volume => (-96.0, 6.0),
            AutoParam::Pan => (-1.0, 1.0),
            AutoParam::Send(_) => (-96.0, 6.0),
            AutoParam::Mute | AutoParam::Plugin { .. } => (0.0, 1.0),
        }
    }
}

/// One automation breakpoint.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoPoint {
    /// Timeline position in samples.
    pub pos: i64,
    /// In the parameter's native units: dB for volume and sends, -1..1 for pan, 0/1 for mute,
    /// normalized 0..1 for plug-in parameters.
    pub value: f32,
    /// The shape of the segment from this point to the next: 0 is a straight line, towards 1
    /// the change comes early, towards -1 late (see `automation::shape`).
    #[serde(default, skip_serializing_if = "is_linear")]
    pub curve: f32,
}

fn is_linear(c: &f32) -> bool {
    *c == 0.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoLane {
    pub param: AutoParam,
    /// Sorted by position. Values are joined by lines (curved by each point's `curve`); before
    /// the first point its value holds, and after the last point the last value does.
    #[serde(default)]
    pub points: Vec<AutoPoint>,
}

/// Logic's automation modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoMode {
    /// Lanes are ignored.
    Off,
    /// Lanes play back.
    #[default]
    Read,
    /// Writes while a control is held, then returns to the existing curve.
    Touch,
    /// Writes from the first touch until playback stops, holding the last value.
    Latch,
    /// Overwrites every lane from where playback started.
    Write,
}

impl AutoMode {
    /// Whether moving a control while playing records automation.
    pub fn writes(self) -> bool {
        matches!(self, AutoMode::Touch | AutoMode::Latch | AutoMode::Write)
    }
}

/// A track group: what its members share, as in Logic's Group Settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: Id,
    pub name: String,
    pub color: String,
    /// Off works like Logic's group clutch: members act alone.
    pub enabled: bool,
    pub volume: bool,
    pub pan: bool,
    pub mute: bool,
    pub solo: bool,
    pub record: bool,
    /// Selecting one member's track selects all of them.
    pub select: bool,
    /// Selecting a region selects the regions at the same place on the other members.
    pub editing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Master {
    pub volume_db: f32,
    #[serde(default)]
    pub inserts: Vec<Option<PluginSlot>>,
    #[serde(default)]
    pub eq: ChannelEq,
    /// Stereo Out's automation: volume and its plug-ins' parameters, as on a track.
    #[serde(default)]
    pub automation: Vec<AutoLane>,
    #[serde(default)]
    pub auto_mode: AutoMode,
    /// Extra lanes shown below the Output track's main lane.
    #[serde(default)]
    pub auto_lanes: Vec<AutoParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cycle {
    pub enabled: bool,
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipRef {
    pub id: Id,
    /// Path relative to the project folder.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub sample_rate: u32,
    pub tempo: f64,
    pub sig_num: u32,
    pub sig_den: u32,
    pub key: String,
    pub tracks: Vec<Track>,
    pub master: Master,
    pub cycle: Cycle,
    pub metronome: bool,
    pub count_in: bool,
    #[serde(default)]
    pub clips: Vec<ClipRef>,
    #[serde(default)]
    pub groups: Vec<Group>,
    pub next_id: Id,
    /// Metronome settings (Project Settings › Metronome).
    #[serde(default)]
    pub click: ClickSettings,
    /// Overrides the app-wide recording format for this project.
    #[serde(default)]
    pub bit_depth: Option<BitDepth>,
    /// Free-form project notes.
    #[serde(default)]
    pub notes: String,
    /// The Marker track, sorted by position.
    #[serde(default)]
    pub markers: Vec<Marker>,
    /// The Autopunch range.
    #[serde(default)]
    pub punch: Punch,
    /// The Tempo track (see `tempo.rs`). Empty: `tempo` throughout. Otherwise the first
    /// event is at 0 and `tempo` mirrors it.
    #[serde(default)]
    pub tempo_map: Vec<TempoEvent>,
    /// The Signature track. Empty: `sig_num/sig_den` throughout. Otherwise the first change
    /// is at bar 1 and `sig_num/sig_den` mirror it.
    #[serde(default)]
    pub signatures: Vec<SigEvent>,
    /// The Tracks area's drag mode (Logic's Drag pop-up): what edits do to the regions they meet.
    #[serde(default)]
    pub drag_mode: DragMode,
    /// The VCA faders, in the Mixer's order.
    #[serde(default)]
    pub vcas: Vec<Vca>,
    /// The name of this project alternative (File › Project Alternatives). Empty in projects
    /// from before alternatives, where it is the project's name. See `session/alternatives.rs`.
    #[serde(default)]
    pub alternative: String,
    /// The Arrangement track: named sections tiling the timeline, sorted by start (see
    /// `crate::arrangement`).
    #[serde(default)]
    pub arrangement: Vec<ArrangementMarker>,
    /// Edit › Snap Edits to Zero Crossings: splits and trims of audio regions move to the
    /// nearest zero crossing. See `session/zero_crossings.rs`.
    #[serde(default)]
    pub snap_zero_crossings: bool,
}

/// A VCA fader (Logic's VCA strip). It passes no audio: its level offsets the faders of the
/// strips assigned to it without moving them, and its mute mutes them. A VCA can itself be
/// assigned to another (`parent`), whose level and mute then reach its strips too.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vca {
    pub id: Id,
    pub name: String,
    pub color: String,
    /// -96 (silence) ..= +6 dB.
    pub level_db: f32,
    pub mute: bool,
    #[serde(default)]
    pub parent: Option<Id>,
}

pub const INSERT_SLOTS: usize = 8;
/// Send slots per strip, as in Logic.
pub const SEND_SLOTS: usize = 8;

/// Logic's default track colours: blue for audio, green for software instruments.
pub const AUDIO_COLOR: &str = "#3b8fd9";
pub const INSTRUMENT_COLOR: &str = "#4fae4c";
pub const STACK_COLOR: &str = "#8f86c9";
pub const AUX_COLOR: &str = "#5fb3a6";

impl Project {
    pub fn new(name: &str, sample_rate: u32) -> Project {
        Project {
            name: name.to_string(),
            sample_rate,
            tempo: 120.0,
            sig_num: 4,
            sig_den: 4,
            key: "C major".into(),
            tracks: Vec::new(),
            master: Master { volume_db: 0.0, inserts: vec![None; INSERT_SLOTS], eq: ChannelEq::default(), automation: Vec::new(), auto_mode: AutoMode::Read, auto_lanes: Vec::new() },
            cycle: Cycle { enabled: false, start: 0, end: 0 },
            metronome: false,
            count_in: false,
            clips: Vec::new(),
            groups: Vec::new(),
            next_id: 1,
            click: ClickSettings::default(),
            bit_depth: None,
            notes: String::new(),
            markers: Vec::new(),
            punch: Punch::default(),
            tempo_map: Vec::new(),
            signatures: Vec::new(),
            drag_mode: DragMode::Overlap,
            vcas: Vec::new(),
            alternative: String::new(),
            arrangement: Vec::new(),
            snap_zero_crossings: false,
        }
    }

    pub fn alloc_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn track(&self, id: Id) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: Id) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn group(&self, id: Id) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Ids of `id`'s subtracks, nested ones included, in track order.
    pub fn descendants(&self, id: Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut frontier = vec![id];
        while let Some(p) = frontier.pop() {
            for t in self.tracks.iter().filter(|t| t.parent == Some(p)) {
                out.push(t.id);
                frontier.push(t.id);
            }
        }
        out.sort_by_key(|id| self.tracks.iter().position(|t| t.id == *id));
        out
    }

    /// The stacks `id` sits in, innermost first.
    pub fn ancestors(&self, id: Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut cur = self.track(id).and_then(|t| t.parent);
        while let Some(p) = cur {
            if out.contains(&p) {
                break;
            }
            out.push(p);
            cur = self.track(p).and_then(|t| t.parent);
        }
        out
    }

    /// Puts every stack's subtracks directly below it (keeping their relative order) and drops
    /// parent links to tracks that aren't stacks.
    pub fn normalize_tree(&mut self) {
        let stacks: Vec<Id> = self.tracks.iter().filter(|t| t.kind.is_stack()).map(|t| t.id).collect();
        let ids: Vec<Id> = self.tracks.iter().map(|t| t.id).collect();
        for t in &mut self.tracks {
            if t.parent.map(|p| !stacks.contains(&p) || p == t.id).unwrap_or(false) {
                t.parent = None;
            }
        }
        // Break cycles.
        for id in &ids {
            let anc = self.ancestors(*id);
            if anc.contains(id) {
                self.track_mut(*id).unwrap().parent = None;
            }
        }
        let mut order = Vec::with_capacity(ids.len());
        fn visit(p: &Project, id: Id, order: &mut Vec<Id>) {
            order.push(id);
            for t in p.tracks.iter().filter(|t| t.parent == Some(id)) {
                visit(p, t.id, order);
            }
        }
        for t in self.tracks.iter().filter(|t| t.parent.is_none()) {
            visit(self, t.id, &mut order);
        }
        self.tracks.sort_by_key(|t| order.iter().position(|&i| i == t.id).unwrap_or(usize::MAX));
    }

    /// Track ids in processing order: every track before whatever it plays into (its summing
    /// stack, its output aux and the auxes it sends to). Ties keep the track-tree order, with
    /// subtracks before their stack. Cycles are refused when routing is edited; if one is
    /// loaded anyway, the edge that closes it is skipped here and ignored by the engine.
    pub fn process_order(&self) -> Vec<Id> {
        let mut base = Vec::with_capacity(self.tracks.len());
        fn tree(p: &Project, id: Id, out: &mut Vec<Id>) {
            for t in p.tracks.iter().filter(|t| t.parent == Some(id)) {
                tree(p, t.id, out);
            }
            out.push(id);
        }
        for t in self.tracks.iter().filter(|t| t.parent.is_none()) {
            tree(self, t.id, &mut base);
        }
        // Who plays into each track, in tree order.
        let mut inputs: HashMap<Id, Vec<Id>> = HashMap::new();
        for &id in &base {
            let parent = self.track(id).and_then(|t| t.parent);
            for dst in parent.into_iter().chain(self.feeds(id)).chain(self.keyed_by(id)) {
                inputs.entry(dst).or_default().push(id);
            }
        }
        let mut out = Vec::with_capacity(base.len());
        let mut seen: HashSet<Id> = HashSet::new();
        fn visit(id: Id, inputs: &HashMap<Id, Vec<Id>>, seen: &mut HashSet<Id>, out: &mut Vec<Id>) {
            if !seen.insert(id) {
                return;
            }
            for &src in inputs.get(&id).into_iter().flatten() {
                visit(src, inputs, seen, out);
            }
            out.push(id);
        }
        for &id in &base {
            visit(id, &inputs, &mut seen, &mut out);
        }
        out
    }

    pub fn is_aux(&self, id: Id) -> bool {
        self.track(id).map(|t| t.kind == TrackKind::Aux).unwrap_or(false)
    }

    /// Where a track's own signal goes (None = Stereo Out): its explicit output, else the
    /// nearest summing stack it sits in. A folder passes its subtracks on to wherever it plays.
    pub fn output_of(&self, id: Id) -> Option<Id> {
        match self.track(id)?.output {
            Some(MASTER_ID) => None,
            Some(o) if o != id && self.is_aux(o) => Some(o),
            _ => self.ancestors(id).into_iter().find(|a| self.track(*a).map(|t| t.kind == TrackKind::Summing).unwrap_or(false)),
        }
    }

    /// Every track `id` plays into directly: its output and the auxes its sends feed.
    pub fn feeds(&self, id: Id) -> Vec<Id> {
        let mut out: Vec<Id> = self.output_of(id).into_iter().collect();
        if let Some(t) = self.track(id) {
            for s in t.sends.iter().flatten() {
                if s.bus != id && self.is_aux(s.bus) && !out.contains(&s.bus) {
                    out.push(s.bus);
                }
            }
        }
        out
    }

    /// The tracks with an insert keyed (side-chained) by `id`.
    pub fn keyed_by(&self, id: Id) -> Vec<Id> {
        self.tracks.iter().filter(|t| t.id != id && t.inserts.iter().flatten().any(|s| s.sidechain == Some(id))).map(|t| t.id).collect()
    }

    /// Whether `to` must play after `from`: `from`'s audio reaches it, or keys a side-chain on
    /// it or on anything it reaches. Edits that would make this true both ways are refused.
    pub fn depends(&self, from: Id, to: Id) -> bool {
        let mut stack = vec![from];
        let mut seen: HashSet<Id> = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let parent = self.track(id).and_then(|t| t.parent);
            for d in parent.into_iter().chain(self.feeds(id)).chain(self.keyed_by(id)) {
                if d == to {
                    return true;
                }
                stack.push(d);
            }
        }
        false
    }

    /// Whether `from`'s audio reaches `to`, through outputs, stacks and sends.
    pub fn reaches(&self, from: Id, to: Id) -> bool {
        let mut stack = vec![from];
        let mut seen: HashSet<Id> = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            for d in self.feeds(id) {
                if d == to {
                    return true;
                }
                stack.push(d);
            }
        }
        false
    }

    /// Drops routing that no longer makes sense: sends and outputs to tracks that are gone or
    /// aren't auxes, routing on folders (which carry no audio), auxes inside stacks, and any
    /// edge that closes a feedback loop. Pads every track to `SEND_SLOTS` send slots.
    pub fn prune_routes(&mut self) {
        let auxes: HashSet<Id> = self.tracks.iter().filter(|t| t.kind == TrackKind::Aux).map(|t| t.id).collect();
        for t in &mut self.tracks {
            if t.sends.len() < SEND_SLOTS {
                t.sends.resize(SEND_SLOTS, None);
            }
            let id = t.id;
            let folder = t.kind == TrackKind::Folder;
            for s in &mut t.sends {
                if s.map(|s| folder || s.bus == id || !auxes.contains(&s.bus)).unwrap_or(false) {
                    *s = None;
                }
            }
            if t.output.map(|o| folder || (o != MASTER_ID && (o == id || !auxes.contains(&o)))).unwrap_or(false) {
                t.output = None;
            }
            if t.kind == TrackKind::Aux {
                t.parent = None;
            }
        }
        // Break loops, cutting the edge that closes each one.
        for i in 0..self.tracks.len() {
            let id = self.tracks[i].id;
            if let Some(o) = self.tracks[i].output.filter(|&o| o != MASTER_ID) {
                if self.reaches(o, id) {
                    self.tracks[i].output = None;
                }
            }
            for k in 0..self.tracks[i].sends.len() {
                if let Some(s) = self.tracks[i].sends[k] {
                    if self.reaches(s.bus, id) {
                        self.tracks[i].sends[k] = None;
                    }
                }
            }
        }
        // Side-chains from tracks that are gone or carry no audio, and ones closing a loop. A
        // frozen track's plug-ins can't change (and aren't loaded), so they are left alone.
        let sources: HashSet<Id> = self.tracks.iter().filter(|t| t.kind != TrackKind::Folder).map(|t| t.id).collect();
        for i in 0..self.tracks.len() {
            if self.tracks[i].frozen.is_some() {
                continue;
            }
            let id = self.tracks[i].id;
            for k in 0..self.tracks[i].inserts.len() {
                let Some(src) = self.tracks[i].inserts[k].as_ref().and_then(|s| s.sidechain) else { continue };
                if src == id || !sources.contains(&src) || self.depends(id, src) {
                    if let Some(s) = &mut self.tracks[i].inserts[k] {
                        s.sidechain = None;
                    }
                }
            }
        }
        for s in self.master.inserts.iter_mut().flatten() {
            if s.sidechain.map(|src| !sources.contains(&src)).unwrap_or(false) {
                s.sidechain = None;
            }
        }
    }

    /// Samples per beat at the tempo and signature at position 0 (see `tempo::TempoMap` for
    /// positions elsewhere).
    pub fn samples_per_beat(&self) -> f64 {
        self.sample_rate as f64 * 60.0 / self.tempo * (4.0 / self.sig_den as f64)
    }

    /// Samples per bar at position 0.
    pub fn samples_per_bar(&self) -> f64 {
        self.samples_per_beat() * self.sig_num as f64
    }

    /// A copy safe to hand to the UI (plugin state blobs stripped).
    pub fn for_ui(&self) -> Project {
        let mut p = self.clone();
        let strip = |s: &mut Option<PluginSlot>| {
            if let Some(s) = s {
                s.state = None;
            }
        };
        for t in &mut p.tracks {
            strip(&mut t.instrument);
            t.inserts.iter_mut().for_each(strip);
            // The UI shows only the names of inactive track alternatives.
            t.alternatives.iter_mut().for_each(|a| a.regions.clear());
        }
        p.master.inserts.iter_mut().for_each(strip);
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(start: i64, length: i64, loop_end: Option<i64>) -> Region {
        Region {
            id: 1,
            name: "A".into(),
            start,
            length,
            muted: false,
            color: None,
            content: RegionContent::Audio { clip: 9, offset: 100, gain_db: 0.0, fade_in: 10, fade_out: 30, fade_shape: FadeShape::default() },
            loop_end,
            takes: None,
            params: Default::default(),
            pattern: None,
        }
    }

    #[test]
    fn loop_passes_cut_the_last_one_short() {
        let r = audio(1000, 100, Some(1350));
        assert_eq!(r.end(), 1350);
        assert!(r.is_looped());
        assert_eq!(r.passes(), vec![(1000, 100), (1100, 100), (1200, 100), (1300, 50)]);
        // A loop end at or before the region's end is no loop.
        for e in [None, Some(1100), Some(500)] {
            let r = audio(1000, 100, e);
            assert_eq!((r.end(), r.passes().len(), r.is_looped()), (1100, 1, false));
        }
    }

    #[test]
    fn moving_takes_the_loop_along() {
        let mut r = audio(1000, 100, Some(1350));
        r.move_to(400);
        assert_eq!((r.start, r.loop_end), (400, Some(750)));
    }

    #[test]
    fn real_copies_match_the_loop() {
        let r = audio(1000, 100, Some(1350));
        let c = r.real_copies();
        let starts: Vec<(i64, i64)> = c.iter().map(|x| (x.start, x.length)).collect();
        assert_eq!(starts, r.passes());
        let fades: Vec<(i64, i64, i64)> = c
            .iter()
            .map(|x| match x.content {
                RegionContent::Audio { offset, fade_in, fade_out, .. } => (offset, fade_in, fade_out),
                _ => unreachable!(),
            })
            .collect();
        // Fade-in on the first pass, the fade-out (shortened to fit) on the last.
        assert_eq!(fades, vec![(100, 10, 0), (100, 0, 0), (100, 0, 0), (100, 0, 30)]);
        assert!(c.iter().all(|x| x.loop_end.is_none()));

        let notes = vec![Note { start: 0, length: 10, pitch: 60, velocity: 100, muted: false }, Note { start: 60, length: 10, pitch: 62, velocity: 100, muted: false }];
        let m = Region { content: RegionContent::Midi { notes, cc: Vec::new() }, ..audio(0, 100, Some(250)) };
        let counts: Vec<usize> = m
            .real_copies()
            .iter()
            .map(|x| match &x.content {
                RegionContent::Midi { notes, .. } => notes.len(),
                _ => 0,
            })
            .collect();
        // The 50-sample last pass ends before the second note.
        assert_eq!(counts, vec![2, 2, 1]);
    }

    #[test]
    fn old_projects_load_without_loops_or_hidden_tracks() {
        let r: Region = serde_json::from_str(r#"{"id":1,"name":"x","start":0,"length":10,"content":{"type":"midi","notes":[]}}"#).unwrap();
        assert_eq!(r.loop_end, None);
        let t: Track = serde_json::from_str(
            r##"{"id":2,"kind":"audio","name":"a","color":"#fff","volume_db":0,"pan":0,"mute":false,"solo":false,"armed":false,"monitor":false,"input":{"type":"none"}}"##,
        )
        .unwrap();
        assert!(!t.hidden);
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;

    fn track(p: &mut Project, kind: TrackKind) -> Id {
        let id = p.alloc_id();
        p.tracks.push(Track {
            id,
            kind,
            name: format!("{id}"),
            color: String::new(),
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            armed: false,
            monitor: false,
            input: InputSel::None,
            instrument: None,
            inserts: Vec::new(),
            regions: Vec::new(),
            eq: ChannelEq::default(),
            parent: None,
            collapsed: false,
            group: None,
            sends: vec![None; SEND_SLOTS],
            output: None,
            bus: 0,
            hidden: false,
            automation: Vec::new(),
            auto_mode: AutoMode::Read,
            frozen: None,
            vca: None,
            alternatives: Vec::new(),
            alternative: String::new(),
            auto_lanes: Vec::new(),
        });
        id
    }

    fn send(bus: Id) -> Option<TrackSend> {
        Some(TrackSend { bus, level_db: 0.0, pre_fader: false, muted: false })
    }

    #[test]
    fn receivers_follow_senders() {
        let mut p = Project::new("t", 48000);
        let aux2 = track(&mut p, TrackKind::Aux);
        let aux1 = track(&mut p, TrackKind::Aux);
        let sum = track(&mut p, TrackKind::Summing);
        let sub = track(&mut p, TrackKind::Audio);
        p.track_mut(sub).unwrap().parent = Some(sum);
        p.track_mut(sub).unwrap().sends[0] = send(aux1);
        p.track_mut(aux1).unwrap().output = Some(aux2);
        let order = p.process_order();
        let pos = |id| order.iter().position(|&i| i == id).unwrap();
        assert!(pos(sub) < pos(sum) && pos(sub) < pos(aux1) && pos(aux1) < pos(aux2));
        assert_eq!(order.len(), 4);
        assert!(p.reaches(sub, aux2) && !p.reaches(aux2, sub));
        assert_eq!(p.output_of(sub), Some(sum));
        p.track_mut(sub).unwrap().output = Some(MASTER_ID);
        assert_eq!(p.output_of(sub), None);
    }

    #[test]
    fn prune_breaks_loops_and_dangling_routes() {
        let mut p = Project::new("t", 48000);
        let a = track(&mut p, TrackKind::Aux);
        let b = track(&mut p, TrackKind::Aux);
        let audio = track(&mut p, TrackKind::Audio);
        p.track_mut(a).unwrap().sends[0] = send(b);
        p.track_mut(b).unwrap().output = Some(a);
        p.track_mut(audio).unwrap().sends[1] = send(audio);
        p.track_mut(audio).unwrap().output = Some(999);
        p.track_mut(audio).unwrap().sends.truncate(2);
        p.prune_routes();
        assert!(!(p.reaches(a, b) && p.reaches(b, a)));
        let t = p.track(audio).unwrap();
        assert!(t.sends.iter().all(|s| s.is_none()) && t.output.is_none() && t.sends.len() == SEND_SLOTS);
        // Loaded loops never hang the ordering.
        assert_eq!(p.process_order().len(), 3);
    }

    fn keyed(p: &mut Project, track: Id, source: Id) {
        let slot = PluginSlot { uid: p.alloc_id(), plugin_id: "builtin:noise-gate".into(), name: "Noise Gate".into(), bypass: false, state: None, params: Vec::new(), sidechain: Some(source), sidechain_pre: false };
        p.track_mut(track).unwrap().inserts = vec![Some(slot)];
    }

    #[test]
    fn side_chain_sources_play_first_and_loops_are_cut() {
        let mut p = Project::new("t", 48000);
        let bass = track(&mut p, TrackKind::Audio);
        let kick = track(&mut p, TrackKind::Audio);
        keyed(&mut p, bass, kick);
        let order = p.process_order();
        let pos = |id| order.iter().position(|&i| i == id).unwrap();
        assert!(pos(kick) < pos(bass));
        assert!(p.depends(kick, bass) && !p.depends(bass, kick));
        // Audio reaching it doesn't make it a feed: soloing the kick doesn't solo the bass.
        assert!(!p.reaches(kick, bass));
        // Keying the kick from an aux the bass sends to would make a loop.
        let aux = track(&mut p, TrackKind::Aux);
        p.track_mut(bass).unwrap().sends[0] = send(aux);
        keyed(&mut p, kick, aux);
        p.prune_routes();
        let key = |id: Id| p.track(id).unwrap().inserts[0].as_ref().unwrap().sidechain;
        // One edge of the loop goes, and the ordering is sound again.
        assert!(key(kick).is_none() != key(bass).is_none());
        assert_eq!(p.process_order().len(), 3);
        // A side-chain from a track that's gone goes too.
        keyed(&mut p, aux, 999);
        p.prune_routes();
        assert_eq!(p.track(aux).unwrap().inserts[0].as_ref().unwrap().sidechain, None);
    }
}
