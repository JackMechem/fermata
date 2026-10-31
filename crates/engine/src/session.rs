//! The non-real-time side of the engine: owns the project document, turns edits into engine
//! commands, manages recording takes, plugin instances and project files.
//!
//! Methods that touch plugin controllers (instantiation, editors, `idle`) must be called on the
//! UI/main thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use vst3_host::{PluginController, PluginDescriptor, PluginState};

use crate::audio_io::{self, AudioConfig, AudioIo, AudioStatus};
use crate::automation::lanes_dsp;
use crate::clip::Clip;
use crate::dsp::*;
use crate::fades::{DragMode, FadeInType, FadeLaw, FadeOutType, FadeShape};
use crate::midi_io::MidiIo;
use crate::model::*;
use crate::prefs::*;
use crate::recorder::{FinishedTake, Recorder, TakePreview};

mod routing;
mod multi_out;
pub use multi_out::{InstrumentOutputs, OutputInfo};
mod bounce;
pub use bounce::{AudioFormat, BipOptions, BipSource, BitDepth, BounceResult, InPlaceRender, RenderOptions};
mod range_edit;
pub use range_edit::{ClipInfo, RangeEdit};
mod options;
pub use options::{discard_autosave, recovery_info, AutosaveJob, Recovery};
mod midi_content;
pub use midi_content::MidiContent;
mod pdc;
pub use pdc::{PdcReport, StripLatency};
mod freeze;
pub use freeze::FreezeRender;
mod markers;
mod takes;
mod region_moves;
pub use region_moves::RegionMove;
mod new_track_drag;
mod tempo;
mod builtin;
pub use builtin::{builtin_effects, builtin_instruments, BuiltinInfo, BuiltinInstrument, FxTick};
mod presets;
pub use presets::EQ_SLOT;
mod insert_order;
mod crossfades;
mod region_params;
pub use region_params::{PlayedNotes, RegionParamsPatch};
mod drag_modes;
mod strip_meters;
pub use strip_meters::PeakHolds;
mod strip_settings;
mod marquee;
mod controllers;
pub use controllers::{controller_label, AssignmentRow};
mod vca;
mod vst_undo;
pub use vca::VcaPatch;
mod alternatives;
mod backups;
pub use backups::BackupInfo;
mod templates;
pub use templates::{delete_template, list_templates, rename_template, templates_dir, UserTemplate};
mod track_alternatives;
mod arrangement;
mod auto_lanes;
mod patterns;
mod arrange_cmds;
use arrange_cmds::SectionClip;

pub const BUILTIN_SYNTH: &str = "builtin:synth";
const PROJECT_FILE: &str = "project.json";
const AUDIO_DIR: &str = "Audio Files";
/// Written next to project.json every few minutes while there are unsaved changes, and offered
/// back after a crash.
pub const AUTOSAVE_FILE: &str = "project.autosave.json";

mod auto_write;
mod recovery;
pub use recovery::{discard_journals, read_journal, recording_journals, JournalTake, RecJournal, RecordingRecovery, BACKUP_FILE};
mod zero_crossings;
mod region_gain;
mod musical_moves;
mod threads;
mod track_off;
mod vst_key;
mod sampler;
pub use sampler::{prepare_files, PreparedFile, SamplerAdded, SamplerRecording, SamplerView, SamplerWave, SfzReport, ZoneView, SAMPLES_DIR};
#[cfg(test)]
mod held_notes;
#[cfg(test)]
mod instrument_tests;

#[derive(Debug, Default, Clone, Deserialize)]
pub struct TrackPatch {
    pub name: Option<String>,
    pub color: Option<String>,
    pub volume_db: Option<f32>,
    pub pan: Option<f32>,
    pub mute: Option<bool>,
    pub solo: Option<bool>,
    pub armed: Option<bool>,
    pub monitor: Option<bool>,
    pub input: Option<InputSel>,
    pub collapsed: Option<bool>,
    pub off: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tick {
    pub pos: i64,
    pub playing: bool,
    pub recording: bool,
    pub cpu: f32,
    pub cpu_peak: f32,
    pub late_blocks: u32,
    /// Threads playing tracks right now (1 unless the project needs more).
    pub threads: u32,
    pub meters: Vec<(Id, f32, f32)>,
    pub master: (f32, f32),
    pub inputs: Vec<f32>,
    pub previews: Vec<TakePreview>,
    pub midi_activity: u32,
    pub overruns: u32,
    pub latency: Latency,
    /// Post-EQ spectrum (dBFS per log band) of the strip shown in the EQ editor.
    pub spectrum: Option<(Id, Vec<f32>)>,
    /// Something went wrong out of sight (a recording couldn't be written); shown once.
    pub warning: Option<String>,
    /// Meters and live values of the built-in effects whose panels are open.
    pub fx: Vec<FxTick>,
    /// The clip indicators: each strip's highest peak (linear) since it was reset, MASTER_ID
    /// for Stereo Out (see `session/strip_meters.rs`).
    pub peak_holds: Vec<(Id, f32)>,
    /// Gain reduction (positive dB) of the strips with a built-in dynamics effect.
    pub gain_reduction: Vec<(Id, f32)>,
    /// The parameter a controller assignment is being learned for (Learn Assignment).
    pub learn: Option<String>,
    /// What the learn that just finished assigned; shown once.
    pub learned: Option<String>,
    /// Messages from hardware MIDI inputs since the last tick, oldest first (for Step Input).
    pub midi_in: Vec<[u8; 3]>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Latency {
    pub input_ms: f32,
    pub output_ms: f32,
    /// How far recordings are moved earlier to line up with what was heard.
    pub compensation_ms: f32,
}

pub struct Session {
    pub project: Project,
    pub dir: PathBuf,
    pub saved: bool,
    pub dirty: bool,
    clips: HashMap<Id, Arc<Clip>>,
    /// The real-time engine. The audio callback only `try_lock`s it and outputs silence when
    /// that fails, so nothing may hold this lock on the main thread's regular paths (ticks,
    /// edits): talk to the engine through `cmd_tx`. The exceptions are offline renders (the
    /// engine isn't playing then), the graph swap in `rebuild_graph`, and draining the
    /// command queue while no audio device runs (`send`).
    engine: Arc<Mutex<Engine>>,
    cmd_tx: rtrb::Producer<Cmd>,
    cmds_sent: u64,
    garbage_rx: rtrb::Consumer<Garbage>,
    sampler: sampler::SamplerHost,
    pub status: Arc<EngineStatus>,
    audio: Option<AudioIo>,
    pub audio_status: AudioStatus,
    pub audio_config: AudioConfig,
    midi: MidiIo,
    midi_rec_rx: rtrb::Consumer<(i64, [u8; 3])>,
    recorder: Recorder,
    controllers: HashMap<Id, PluginController>,
    meters: HashMap<Id, Arc<Meter>>,
    pub plugins: Vec<PluginDescriptor>,
    pub selected: Option<Id>,
    rec_punch: Option<i64>,
    analyzer_rx: rtrb::Consumer<f32>,
    analyzer_buf: Vec<f32>,
    analyzer_target: Option<Id>,
    rec_midi_track: Option<Id>,
    last_play_from: i64,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    auto: auto_write::AutoWriter,
    /// Settings › General, Recording, MIDI and Editing (see [`Session::set_prefs`]).
    pub prefs: Prefs,
    autosaved_at: Instant,
    /// Hash of the last autosaved document, to skip writing an unchanged one.
    autosave_hash: u64,
    /// Plug-in delay compensation last sent to the engine: per track its settings and the
    /// longest delay its line holds (see `pdc.rs`).
    pdc_sent: HashMap<Id, (PdcParams, usize, usize)>,
    pdc_global: PdcGlobal,
    pdc_report: PdcReport,
    /// How the recording in progress is cut into takes (see `session/takes.rs`).
    rec_plan: Option<crate::takes::RecPlan>,
    /// The engine's sample rate, kept here so reading it needn't lock the engine (see
    /// `engine`: the audio callback only try_locks it, and loses its block when that fails).
    /// Set where the graph is replaced, in `rebuild_graph`.
    engine_rate: f64,
    /// For the next tick's `warning`.
    warning: Option<String>,
    /// Engine panics already reported (see `engine_error_warning`).
    engine_errors_seen: u32,
    engine_error_at: Instant,
    bad_audio_seen: u32,
    bad_audio_at: Instant,
    overloads_seen: u32,
    /// What each loaded built-in effect shares with the session, by instance id (see
    /// `session/builtin.rs`), and the instances whose panels are open.
    fx: HashMap<Id, Arc<FxShared>>,
    fx_watch: std::collections::HashSet<Id>,
    /// The last transport jump sent (locate, or record from a pre-roll): the `cmds_sent` count
    /// that includes it and the position it moves to (see [`Session::position`]).
    pending_pos: Option<(u64, i64)>,
    /// Autosave at the next chance rather than after the interval: a recording, bounce or
    /// freeze just finished (see `session/recovery.rs`).
    autosave_soon: bool,
    /// The crash-recovery journal of the recording in progress.
    rec_journal: Option<PathBuf>,
    /// Journals of finished recordings whose takes aren't in a file on disk yet; the next
    /// autosave or save deletes them.
    unsaved_journals: Vec<PathBuf>,
    /// Plug-in states as of the last autosave or save, by instance id, for an emergency
    /// autosave that mustn't call into plug-ins.
    captured_states: HashMap<Id, String>,
    /// The clip indicators' held peaks (see `session/strip_meters.rs`).
    peak_holds: PeakHolds,
    /// The tempo map the engine's playback lists were last built for.
    regions_tempo: Option<crate::tempo::TempoMap>,
    /// Controller assignments (see `session/controllers.rs`).
    controls: controllers::Controls,
    /// The sequence number of the next MIDI input message the ticks haven't reported.
    midi_seen: u64,
    /// What Cut Section Between Locators took out last, for Insert Cut Section.
    section_clip: Option<SectionClip>,
    /// Plug-ins whose editor is open, watched for undo steps (see `session/vst_undo.rs`).
    vst_watches: HashMap<Id, vst_undo::Watch>,
    /// Processing Threads (see `session/threads.rs`), and whether an edit may have changed the
    /// task graph or the setting since it was last looked at.
    threads: threads::ThreadState,
    graph_dirty: bool,
}

/// A project snapshot taken before an edit.
struct UndoEntry {
    project: Project,
    label: String,
    /// Consecutive edits with the same key within a moment (a fader drag) share one entry.
    key: Option<String>,
    at: Instant,
    /// A VST3 and its state to go with the snapshot: an edit in its own editor, which the
    /// snapshot can't undo, since `restore` leaves a plug-in that's still there alone.
    plugin: Option<(Id, Arc<PluginState>)>,
}

/// `folder/<src's name>` inside `target`, numbered ("take 2.wav") if that name is already in
/// `taken` or on disk. The result is added to `taken`.
fn unique_name(target: &Path, folder: &Path, src: &Path, taken: &mut std::collections::HashSet<PathBuf>) -> PathBuf {
    let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "audio".into());
    let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut rel = folder.join(format!("{stem}{ext}"));
    let mut n = 2;
    while taken.contains(&rel) || target.join(&rel).exists() {
        rel = folder.join(format!("{stem} {n}{ext}"));
        n += 1;
    }
    taken.insert(rel.clone());
    rel
}

fn same<T: Serialize>(a: &T, b: &T) -> bool {
    serde_json::to_string(a).ok() == serde_json::to_string(b).ok()
}

/// ~/Music/Fermata, where new projects go unless Settings › General names another folder.
pub fn default_projects_root() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    home.join("Music").join("Fermata")
}

fn untitled_dir(root: &Path) -> PathBuf {
    for i in 1.. {
        let name = if i == 1 { "Untitled.fermata".to_string() } else { format!("Untitled {i}.fermata") };
        let p = root.join(name);
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}

fn encode_state(st: &PluginState) -> String {
    let b = base64::engine::general_purpose::STANDARD;
    format!("{}:{}", b.encode(&st.component), b.encode(&st.controller))
}

fn decode_state(s: &str) -> PluginState {
    let b = base64::engine::general_purpose::STANDARD;
    let (a, c) = s.split_once(':').unwrap_or((s, ""));
    PluginState { component: b.decode(a).unwrap_or_default(), controller: b.decode(c).unwrap_or_default() }
}

impl Session {
    pub fn new(audio_config: AudioConfig, plugins: Vec<PluginDescriptor>) -> Session {
        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(4096);
        let (garbage_tx, garbage_rx) = rtrb::RingBuffer::new(4096);
        let status = Arc::new(EngineStatus::default());
        let (live_tx, live_rx) = rtrb::RingBuffer::new(1024);
        let (rec_tx, rec_rx) = rtrb::RingBuffer::new(8192);
        let (an_tx, an_rx) = rtrb::RingBuffer::new(1 << 16);
        let engine = Arc::new(Mutex::new(Engine::new(
            audio_config.sample_rate as f64,
            cmd_rx,
            garbage_tx,
            status.clone(),
            live_rx,
            rec_tx,
            an_tx,
        )));
        let mut midi = MidiIo::new(live_tx);
        midi.refresh();
        let engine_rate = audio_config.sample_rate as f64;
        let mut s = Session {
            project: Project::new("Untitled", audio_config.sample_rate),
            dir: untitled_dir(&default_projects_root()),
            saved: false,
            dirty: false,
            clips: HashMap::new(),
            engine,
            cmd_tx,
            cmds_sent: 0,
            garbage_rx,
            status,
            audio: None,
            audio_status: AudioStatus::default(),
            audio_config,
            midi,
            midi_rec_rx: rec_rx,
            recorder: Recorder::new(),
            sampler: Default::default(),
            controllers: HashMap::new(),
            meters: HashMap::new(),
            plugins,
            selected: None,
            rec_punch: None,
            analyzer_rx: an_rx,
            analyzer_buf: vec![0.0; crate::eq::FFT_SIZE],
            analyzer_target: None,
            rec_midi_track: None,
            last_play_from: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            auto: Default::default(),
            prefs: Prefs::default(),
            autosaved_at: Instant::now(),
            autosave_hash: 0,
            pdc_sent: HashMap::new(),
            pdc_global: PdcGlobal::default(),
            pdc_report: PdcReport::default(),
            rec_plan: None,
            engine_rate,
            warning: None,
            engine_errors_seen: 0,
            engine_error_at: Instant::now() - Duration::from_secs(60),
            bad_audio_seen: 0,
            overloads_seen: 0,
            bad_audio_at: Instant::now() - Duration::from_secs(60),
            fx: HashMap::new(),
            fx_watch: Default::default(),
            pending_pos: None,
            autosave_soon: false,
            rec_journal: None,
            unsaved_journals: Vec::new(),
            captured_states: HashMap::new(),
            peak_holds: PeakHolds::default(),
            regions_tempo: None,
            controls: Default::default(),
            midi_seen: 0,
            section_clip: None,
            vst_watches: HashMap::new(),
            threads: threads::ThreadState { topology: crate::threads::Topology::detect(), ..Default::default() },
            graph_dirty: true,
        };
        s.restart_audio();
        s.sync_globals();
        s
    }

    // ---- engine plumbing ------------------------------------------------------------------

    fn send(&mut self, cmd: Cmd) {
        self.graph_dirty |= threads::changes_routes(&cmd);
        if self.cmd_tx.push(cmd).is_ok() {
            self.cmds_sent += 1;
        }
        if self.audio.is_none() {
            // No device is running, so drain the queue here to keep the graph current.
            self.engine.lock().process(&[], 0, &mut [], 0);
        }
    }

    /// Blocks until the audio thread has applied every command sent so far.
    fn wait_for_engine(&self) {
        let deadline = Instant::now() + Duration::from_millis(1000);
        while self.status.cmds_done.load(Ordering::Relaxed) < self.cmds_sent && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn sample_rate(&self) -> f64 {
        self.engine_rate
    }

    pub fn restart_audio(&mut self) {
        self.audio = None;
        let (io, status) = audio_io::start(self.engine.clone(), &self.audio_config);
        if !status.running {
            log::error!("audio failed to start: {:?}", status.error);
        }
        let rate = if status.running { status.sample_rate } else { self.audio_config.sample_rate };
        self.audio = io;
        self.audio_status = status;
        // Whether the workers should be real-time threads depends on the device.
        self.graph_dirty = true;
        // The command counter restarts with the engine's view of it.
        self.cmds_sent = self.status.cmds_done.load(Ordering::Relaxed);
        self.pending_pos = None;
        if rate != self.project.sample_rate || (self.engine_rate as u32) != rate {
            // The project runs at the device's rate, and that's saved with it.
            if rate != self.project.sample_rate {
                self.dirty = true;
            }
            self.project.sample_rate = rate;
            self.rebuild_graph();
        }
    }

    pub fn set_audio_config(&mut self, cfg: AudioConfig) {
        let reopen = !cfg.same_devices(&self.audio_config) || self.audio.is_none();
        self.graph_dirty |= cfg.processing_threads != self.audio_config.processing_threads;
        self.audio_config = cfg;
        if reopen {
            self.restart_audio();
        }
        self.send(Cmd::AutoMonitor(self.audio_config.auto_monitor));
        self.sync_parallel();
    }

    /// Round trip from what the performer heard to when their playing reached the engine:
    /// output latency + input latency + frames waiting in the duplex ring, plus the user's
    /// manual Recording Delay.
    fn record_latency_frames(&self) -> i64 {
        let sr = self.sample_rate();
        let us = self.status.input_latency_us.load(Ordering::Relaxed) + self.status.output_latency_us.load(Ordering::Relaxed);
        let ring = self.status.ring_frames.load(Ordering::Relaxed) as f64;
        let manual = self.audio_config.recording_delay_ms as f64 * sr / 1000.0;
        // With delay compensation, the playback the performer played along to left Stereo Out
        // that much late too.
        let pdc = self.pdc_global.total as f64;
        (us as f64 * sr / 1e6 + ring + manual + pdc).round() as i64
    }

    pub fn latency(&self) -> Latency {
        let sr = self.sample_rate().max(1.0);
        Latency {
            input_ms: self.status.input_latency_us.load(Ordering::Relaxed) as f32 / 1000.0,
            output_ms: self.status.output_latency_us.load(Ordering::Relaxed) as f32 / 1000.0,
            compensation_ms: (self.record_latency_frames() as f64 * 1000.0 / sr) as f32,
        }
    }

    fn sync_globals(&mut self) {
        let p = &self.project;
        let cmds = [
            Cmd::Tempo(Arc::new(crate::tempo::TempoMap::from_project(p))),
            Cmd::Cycle { enabled: p.cycle.enabled, start: p.cycle.start, end: p.cycle.end },
            Cmd::Metronome { on: p.metronome, count_in: p.count_in },
            Cmd::MasterVolume(volume_gain(p.master.volume_db)),
            Cmd::AutoMonitor(self.audio_config.auto_monitor),
            Cmd::Eq(MASTER_ID, crate::eq::coefficients(&p.master.eq, self.engine_rate)),
            Cmd::Analyzer(self.analyzer_target),
            Cmd::Click(ClickParams::from_settings(&p.click)),
            Cmd::MidiThru(self.prefs.midi.thru),
            Cmd::Chase { notes: self.prefs.midi.chase_notes, controllers: self.prefs.midi.chase_controllers },
        ];
        for c in cmds {
            self.send(c);
        }
        self.update_midi_target();
        self.update_pdc();
        // Region quantize and delay in ticks follow the tempo map. Only when it moved: new
        // playback lists release and chase the track's notes.
        let tempo = crate::tempo::TempoMap::from_project(&self.project);
        if self.regions_tempo.as_ref() != Some(&tempo) {
            self.regions_tempo = Some(tempo);
            self.push_tempo_regions();
        }
    }

    /// What the engine plays for a track, with its stacks applied: a folder's fader scales its
    /// subtracks, a muted stack mutes them, and solo reaches both down (a soloed stack's
    /// subtracks) and up (the stacks a soloed subtrack plays through).
    fn track_params(p: &Project, t: &Track) -> TrackParams {
        let ancestors = p.ancestors(t.id);
        let mut volume_db = t.volume_db;
        let mut mute = t.mute;
        let mut solo = t.solo;
        let mut stack_db = 0.0;
        let mut stack_mute = false;
        let mut bus_mute = false;
        for a in ancestors.iter().filter_map(|id| p.track(*id)) {
            if a.kind == TrackKind::Folder {
                volume_db += a.volume_db;
                stack_db += a.volume_db;
            } else {
                bus_mute |= a.mute;
            }
            mute |= a.mute;
            stack_mute |= a.mute;
            solo |= a.solo;
        }
        // A VCA offsets the fader without moving it and mutes with it; like a folder's, both
        // still apply when automation drives the track's own volume and mute.
        let (vca_db, vca_mute) = crate::vca::offset(&p.vcas, t.vca.filter(|_| crate::vca::can_assign(t.kind)));
        volume_db += vca_db;
        stack_db += vca_db;
        mute |= vca_mute;
        stack_mute |= vca_mute;
        if t.kind.is_stack() {
            solo |= p.descendants(t.id).iter().filter_map(|id| p.track(*id)).any(|d| d.solo);
        }
        // Solo follows the routing too: a soloed track keeps the auxes it feeds audible, and a
        // soloed aux keeps the tracks feeding it audible. An aux on an instrument's extra output
        // doesn't need the instrument's strip audible, so that stays out of the solo.
        if !solo && p.tracks.iter().any(|y| y.solo) {
            let soloed = |y: &Track| y.solo || p.ancestors(y.id).iter().filter_map(|a| p.track(*a)).any(|a| a.solo);
            solo = p.tracks.iter().filter(|y| y.id != t.id).any(|y| (soloed(y) && p.reaches(y.id, t.id)) || (y.kind == TrackKind::Aux && y.solo && p.reaches_past_fader(t.id, y.id)));
        }
        // Summing stacks and auxes are buses; a folder passes its subtracks on to wherever it
        // plays. An explicit output overrides the stack.
        let output = p.output_of(t.id);
        let mut sends = [None; SEND_SLOTS];
        if t.kind != TrackKind::Folder {
            for (k, s) in t.sends.iter().take(SEND_SLOTS).enumerate() {
                if let Some(s) = s.filter(|s| s.bus != t.id && p.is_aux(s.bus)) {
                    let gain = if s.muted { 0.0 } else { volume_gain(s.level_db) };
                    sends[k] = Some(SendParams { bus: s.bus, gain, pre_fader: s.pre_fader });
                }
            }
        }
        TrackParams {
            volume: volume_gain(volume_db.max(-96.0)),
            pan: t.pan,
            mute,
            solo,
            armed: t.armed && t.kind.can_record(),
            monitor: t.monitor,
            input: t.input,
            output,
            sends,
            stack_gain: volume_gain(stack_db.max(-96.0)),
            stack_mute,
            folder: ancestors.iter().copied().find(|id| p.track(*id).map(|a| a.kind == TrackKind::Folder).unwrap_or(false)),
            own_gain: volume_gain(t.volume_db.max(-96.0)),
            own_mute: t.mute,
            bus_mute,
            vca_gain: volume_gain(vca_db.max(-96.0)),
            vca_mute,
            off: p.is_off(t.id),
            instrument_output: t.instrument_output,
        }
    }

    /// Sends every track's settings (stacks make one track's change reach others).
    fn push_all_params(&mut self) {
        let params: Vec<(Id, TrackParams)> = self.project.tracks.iter().map(|t| (t.id, Self::track_params(&self.project, t))).collect();
        for (id, p) in params {
            self.send(Cmd::Params(id, p));
        }
        // Routing may have changed, and with it the delays that line the paths up.
        self.update_pdc();
    }

    /// After tracks are added, removed, moved or restacked: keeps subtracks under their stack
    /// and tells the engine the processing order and routing.
    fn sync_tracks(&mut self) {
        self.project.prune_routes();
        self.project.normalize_tree();
        self.send(Cmd::Order(self.project.process_order()));
        self.push_all_params();
        self.update_midi_target();
    }

    /// Builds the playback list for a track. Audio regions don't stack: a newer region (later in
    /// the list) hides whatever older audio it covers, like Logic's "No Overlap" playback, but
    /// without destroying the older regions.
    ///
    /// A looping region plays as one entry per pass. Loop repetitions sit beneath every region
    /// body (they are laid down first), so a loop dragged over another region never hides it,
    /// as in Logic where the region covers the loop. The fade-in shapes the start of the first
    /// pass and the fade-out the end of the last one, at the loop end.
    ///
    /// Two regions that crossfade (see [`crate::fades::crossfades`]) both play across the
    /// crossfade, the outgoing one past its end and the incoming one from before its start;
    /// outside it, the earlier one is heard before and the later one after, whichever is newer.
    fn regions_dsp(&self, t: &Track) -> Arc<Vec<RegionDsp>> {
        if let Some(f) = self.freeze_dsp(t) {
            return f;
        }
        struct Seg {
            region: Id,
            start: i64,
            end: i64,
            offset: i64,
            gain: f32,
            clip: Arc<Clip>,
            fade_in: (i64, i64),
            fade_out: (i64, i64),
            laws: [FadeLaw; 2],
        }
        let mut segs: Vec<Seg> = Vec::new();
        let mut out = Vec::new();
        // Cuts away the older audio under `rs..re`, except a crossfade partner's (`keep`).
        let cut = |segs: &mut Vec<Seg>, rs: i64, re: i64, keep: &[Id]| {
            let mut kept = Vec::with_capacity(segs.len() + 1);
            for sg in segs.drain(..) {
                if sg.end <= rs || sg.start >= re || keep.contains(&sg.region) {
                    kept.push(sg);
                    continue;
                }
                if sg.start < rs {
                    kept.push(Seg { start: sg.start, end: rs, clip: sg.clip.clone(), laws: sg.laws.clone(), ..sg });
                }
                if sg.end > re {
                    kept.push(Seg { start: re, offset: sg.offset + (re - sg.start), ..sg });
                }
            }
            *segs = kept;
        };
        // Lays a new piece of audio over what's there.
        let lay = |segs: &mut Vec<Seg>, new: Seg, keep: &[Id]| {
            cut(segs, new.start, new.end, keep);
            segs.push(new);
        };
        // Regions with playback parameters (see `session/region_params.rs`).
        let map = self.params_map(t);
        let delayed = map.as_ref().and_then(|m| Self::delay_audio(t, m));
        let regions = delayed.as_deref().unwrap_or(&t.regions);
        // Pattern regions play the notes their patterns make (see `session/patterns.rs`).
        let patterns = self.realize_patterns(regions);
        let regions = patterns.as_deref().unwrap_or(regions);
        // Settings › Editing › Crossfade Time and Curve, for comp boundaries and loop seams.
        let xfade = self.crossfade_frames();
        let comp_law = FadeLaw::Curve(self.prefs.editing.crossfade_curve.clamp(-1.0, 1.0));
        let clip_frames = |c: Id| self.clips.get(&c).map(|c| c.frames() as i64);
        let crossfades = crate::fades::crossfades(regions, clip_frames);
        // Loop repetitions first, then the region bodies, so bodies always win.
        for repeats in [true, false] {
            for r in regions.iter().filter(|r| !r.muted) {
                let passes = r.passes();
                let picked = if repeats { &passes[1..] } else { &passes[..1] };
                match &r.content {
                    // A take folder plays its comp: each pass covers what's under it, then its
                    // pieces go in side by side so their crossfades overlap. The folder's own
                    // fades take its shape, the comp's crossfades the Crossfade Curve. Crossfades
                    // with neighbours replace the folder's own fades at those ends.
                    RegionContent::Audio { gain_db, fade_in, fade_out, fade_shape, .. } if r.takes.is_some() => {
                        let folder = r.takes.as_ref().unwrap();
                        let last = passes.last().map(|p| p.1).unwrap_or(r.length);
                        let fi = (*fade_in).clamp(0, r.length);
                        let fo = if r.is_looped() { (*fade_out).clamp(0, last) } else { (*fade_out).clamp(0, r.length - fi) };
                        let x_in = crossfades.iter().find(|x| x.in_region == r.id);
                        let x_out = crossfades.iter().find(|x| x.out_region == r.id);
                        let own_in = x_in.map(|x| (x.fade_in, x.laws().1)).unwrap_or(((r.start, fi), FadeLaw::fade_in(fade_shape)));
                        let own_out = x_out.map(|x| (x.fade_out, x.laws().0)).unwrap_or(((r.end() - fo, fo), FadeLaw::fade_out(fade_shape)));
                        let keep: Vec<Id> = x_in.map(|x| x.out_region).into_iter().chain(x_out.map(|x| x.in_region)).collect();
                        for &(ps, pl) in picked {
                            let mut edges = crate::takes::PassEdges::default();
                            if let (Some(x), true) = (x_in, ps == r.start) {
                                (edges.lead, edges.fade_in) = (ps - x.fade_in.0, Some(x.fade_in));
                            }
                            if let (Some(x), true) = (x_out, ps + pl >= r.end()) {
                                (edges.tail, edges.fade_out) = (x.fade_out.0 + x.fade_out.1 - (ps + pl), Some(x.fade_out));
                            }
                            cut(&mut segs, ps - edges.lead.max(0), ps + pl + edges.tail.max(0), &keep);
                            for p in crate::takes::pass_pieces_edged(r, folder, ps, pl, xfade, edges) {
                                let Some(c) = self.clips.get(&p.clip) else { continue };
                                let gain = volume_gain(*gain_db + p.gain_db);
                                let law_in = if own_in.0 .1 > 0 && p.fade_in == own_in.0 { own_in.1.clone() } else { comp_law.clone() };
                                let law_out = if own_out.0 .1 > 0 && p.fade_out == own_out.0 { own_out.1.clone() } else { comp_law.clone() };
                                segs.push(Seg { region: r.id, start: p.start, end: p.end, offset: p.offset, gain, clip: c.clone(), fade_in: p.fade_in, fade_out: p.fade_out, laws: [law_in, law_out] });
                            }
                        }
                    }
                    RegionContent::Audio { clip, offset, gain_db, fade_in, fade_out, fade_shape } => {
                        let Some(c) = self.clips.get(clip) else { continue };
                        let (end, last) = (r.end(), passes.last().map(|p| p.1).unwrap_or(r.length));
                        let fi = (*fade_in).clamp(0, r.length);
                        let fo = if r.is_looped() { (*fade_out).clamp(0, last) } else { (*fade_out).clamp(0, r.length - fi) };
                        // The first pass fades in and the last fades out, with the region's own
                        // fades or a crossfade, which stretches the region to cover it.
                        let mut first_in = ((r.start, fi), FadeLaw::fade_in(fade_shape));
                        let mut last_out = ((end - fo, fo), FadeLaw::fade_out(fade_shape));
                        let mut keep = Vec::new();
                        if let Some(x) = crossfades.iter().find(|x| x.in_region == r.id) {
                            first_in = (x.fade_in, x.laws().1);
                            keep.push(x.out_region);
                        }
                        if let Some(x) = crossfades.iter().find(|x| x.out_region == r.id) {
                            last_out = (x.fade_out, x.laws().0);
                            keep.push(x.in_region);
                        }
                        // Repetitions that crossfade overlap each other at the seams.
                        let seams = crate::fades::loop_seams(r, clip_frames, xfade);
                        if !seams.is_empty() {
                            keep.push(r.id);
                        }
                        let seam_law = FadeLaw::Curve(crate::fades::DEFAULT_CURVE);
                        let (first, n) = (r.start, passes.len());
                        for &(ps, pl) in picked {
                            let k = passes.iter().position(|p| p.0 == ps).unwrap_or(0);
                            let (fin, law_in) = match (ps == first, k.checked_sub(1).and_then(|j| seams.get(j))) {
                                (true, _) => first_in.clone(),
                                (false, Some(z)) => (z.1, seam_law.clone()),
                                (false, None) => ((ps, 0), FadeLaw::default()),
                            };
                            let (fout, law_out) = match (k + 1 == n, seams.get(k)) {
                                (true, _) => last_out.clone(),
                                (false, Some(z)) => (z.0, seam_law.clone()),
                                (false, None) => ((ps + pl, 0), FadeLaw::default()),
                            };
                            let (s, e) = (ps.min(fin.0), (ps + pl).max(fout.0 + fout.1));
                            let seg = Seg { region: r.id, start: s, end: e, offset: *offset + (s - ps), gain: volume_gain(*gain_db), clip: c.clone(), fade_in: fin, fade_out: fout, laws: [law_in, law_out] };
                            lay(&mut segs, seg, &keep);
                        }
                    }
                    RegionContent::Midi { .. } if !r.params.is_default() => {
                        out.extend(Self::midi_passes(r, picked, map.as_ref().unwrap()));
                    }
                    RegionContent::Midi { notes, cc } => {
                        if picked.is_empty() {
                            continue;
                        }
                        let mut n: Vec<Note> = notes.iter().filter(|n| !n.muted).copied().collect();
                        n.sort_by_key(|n| n.start);
                        let mut c = cc.clone();
                        c.sort_by_key(|c| c.start);
                        // Every pass shares one note list and one controller list.
                        let (n, c) = (Arc::new(n), Arc::new(c));
                        for &(ps, pl) in picked {
                            out.push(RegionDsp::Midi { start: ps, length: pl, notes: n.clone(), cc: c.clone() });
                        }
                    }
                }
            }
        }
        segs.sort_by_key(|s| s.start);
        out.extend(segs.into_iter().map(|s| RegionDsp::Audio {
            start: s.start,
            length: s.end - s.start,
            offset: s.offset,
            gain: s.gain,
            clip: s.clip,
            fade_in: s.fade_in,
            fade_out: s.fade_out,
            laws: s.laws,
        }));
        Arc::new(out)
    }

    fn push_regions(&mut self, track: Id) {
        if let Some(t) = self.project.track(track) {
            let r = self.regions_dsp(t);
            self.send(Cmd::Regions(track, r));
        }
    }

    fn instantiate_slot(&mut self, slot: &PluginSlot) -> Result<InsertDsp> {
        let sr = self.sample_rate();
        // Undo frees instance ids for reuse: one that held a built-in effect may come back as
        // another kind of plug-in, which mustn't inherit its latency or meters.
        if !builtin::is_builtin(&slot.plugin_id) {
            self.fx.remove(&slot.uid);
        }
        let plugin = if let Some(info) = crate::fx::find(&slot.plugin_id) {
            self.instantiate_builtin(&info, slot)
        } else if let Some(info) = crate::instrument::find(&slot.plugin_id) {
            self.instantiate_instrument(&info, slot)
        } else {
            let desc = self.find_plugin(&slot.plugin_id).ok_or_else(|| anyhow!("plug-in not found: {}", slot.name))?.clone();
            let (proc, ctrl) = vst3_host::instantiate_keyed(&desc, sr, MAX_BLOCK, slot.sidechain.is_some()).map_err(|e| anyhow!(e))?;
            if let Some(st) = &slot.state {
                ctrl.load_state(&decode_state(st));
            }
            self.controllers.insert(slot.uid, ctrl);
            Plugin::Vst(Box::new(proc))
        };
        Ok(InsertDsp { uid: slot.uid, bypass: slot.bypass, plugin, key: KeyRoute::new(slot.sidechain, slot.sidechain_pre) })
    }

    fn build_track(&mut self, t: &Track) -> Box<TrackDsp> {
        let meter = self.meters.entry(t.id).or_default().clone();
        let mut dsp = TrackDsp::new(t.id, t.kind, Self::track_params(&self.project, t), meter);
        dsp.set_eq(self.eq_coeffs(t));
        dsp.set_regions(self.regions_dsp(t));
        dsp.set_automation(lanes_dsp(&t.automation), t.auto_mode);
        // A new engine track starts without delay compensation; update_pdc sends it.
        self.pdc_sent.remove(&t.id);
        if t.frozen.is_some() {
            // A frozen track plays its freeze file; its plug-ins stay unloaded.
            return Box::new(dsp);
        }
        if let Some(slot) = &t.instrument {
            match self.instantiate_slot(slot) {
                Ok(i) => dsp.set_instrument(Some(i)),
                Err(e) => log::error!("{}: {e}", t.name),
            }
        }
        for (i, slot) in t.inserts.iter().enumerate() {
            if let Some(slot) = slot {
                match self.instantiate_slot(slot) {
                    Ok(ins) => dsp.set_insert(i, Some(ins)),
                    Err(e) => log::error!("{}: {e}", t.name),
                }
            }
        }
        Box::new(dsp)
    }

    /// Recreates every track and plugin from the project document.
    fn rebuild_graph(&mut self) {
        let snapshot = self.capture_plugin_states();
        for c in self.controllers.values_mut() {
            c.close_editor();
        }
        let old_controllers = std::mem::take(&mut self.controllers);
        self.fx.clear();
        let tracks: Vec<Track> = snapshot.tracks.clone();
        let mut built: Vec<Box<TrackDsp>> = tracks.iter().map(|t| self.build_track(t)).collect();
        // The engine plays tracks in this order; routes only point forwards.
        let order = snapshot.process_order();
        built.sort_by_key(|t| order.iter().position(|&i| i == t.id).unwrap_or(usize::MAX));
        let mut master = Vec::new();
        for slot in &snapshot.master.inserts {
            master.push(slot.as_ref().and_then(|s| self.instantiate_slot(s).map_err(|e| log::error!("{e}")).ok()));
        }
        let sr = self.project.sample_rate as f64;
        // The one lock outside an offline render: a swap, with the old graph dropped after.
        let old = self.engine.lock().replace_graph(sr, built, master);
        self.engine_rate = sr;
        self.graph_dirty = true;
        drop(old);
        drop(old_controllers);
        // Another project (or the same one reloaded): its strips start without clips.
        self.peak_holds.reset(None);
        self.sync_master_automation();
        self.sync_globals();
    }

    /// The project with each plugin's current state embedded (for saving or rebuilding).
    fn capture_plugin_states(&self) -> Project {
        let mut p = self.project.clone();
        let fill = |s: &mut Option<PluginSlot>, ctrls: &HashMap<Id, PluginController>| {
            if let Some(slot) = s {
                if let Some(c) = ctrls.get(&slot.uid) {
                    slot.state = Some(encode_state(&c.save_state()));
                }
            }
        };
        for t in &mut p.tracks {
            fill(&mut t.instrument, &self.controllers);
            for s in &mut t.inserts {
                fill(s, &self.controllers);
            }
        }
        for s in &mut p.master.inserts {
            fill(s, &self.controllers);
        }
        p
    }

    fn midi_target_id(&self) -> Option<Id> {
        let sel = self.selected.filter(|id| self.project.track(*id).map(|t| t.kind == TrackKind::Instrument).unwrap_or(false));
        sel.or_else(|| self.project.tracks.iter().find(|t| t.kind == TrackKind::Instrument && t.armed && !self.project.is_off(t.id)).map(|t| t.id))
    }

    fn update_midi_target(&mut self) {
        let target = self.midi_target_id();
        self.send(Cmd::MidiTarget(target));
    }

    // ---- undo -----------------------------------------------------------------------------

    /// Records the project as it is before an edit. `key` merges a burst of edits (a fader drag)
    /// into one step; `plugins` also captures plug-in settings, for edits that remove plug-ins.
    /// Returns whether a new step was pushed (false when merged into the last one), for
    /// [`Session::abandon_checkpoint`].
    pub fn checkpoint(&mut self, label: &str, key: Option<String>, plugins: bool) -> bool {
        // An edit just made in a plug-in's editor comes first.
        self.flush_vst_edits();
        self.push_checkpoint(label, key, plugins)
    }

    fn push_checkpoint(&mut self, label: &str, key: Option<String>, plugins: bool) -> bool {
        self.redo_stack.clear();
        if let (Some(k), Some(top)) = (&key, self.undo_stack.last_mut()) {
            if top.key.as_ref() == Some(k) && top.at.elapsed() < Duration::from_millis(1500) {
                top.at = Instant::now();
                return false;
            }
        }
        let project = if plugins { self.capture_plugin_states() } else { self.project.clone() };
        self.undo_stack.push(UndoEntry { project, label: label.to_string(), key, at: Instant::now(), plugin: None });
        let limit = self.prefs.general.undo_limit.max(1);
        if self.undo_stack.len() > limit {
            self.undo_stack.drain(..self.undo_stack.len() - limit);
        }
        true
    }

    /// Takes back the step [`Session::checkpoint`] just pushed, after its edit failed: anything
    /// the edit changed before failing is put back, and no undo entry is left for it.
    pub fn abandon_checkpoint(&mut self) {
        let Some(e) = self.undo_stack.pop() else { return };
        if !same(&e.project, &self.project) {
            self.restore(e.project);
        }
    }

    pub fn undo_label(&self) -> Option<String> {
        self.undo_stack.last().map(|e| e.label.clone())
    }

    pub fn redo_label(&self) -> Option<String> {
        self.redo_stack.last().map(|e| e.label.clone())
    }

    /// Steps back one edit. Returns its label.
    pub fn undo(&mut self) -> Option<String> {
        self.flush_vst_edits();
        let e = self.undo_stack.pop()?;
        let label = e.label.clone();
        let back = self.step_to(e);
        self.redo_stack.push(back);
        Some(label)
    }

    pub fn redo(&mut self) -> Option<String> {
        self.flush_vst_edits();
        let e = self.redo_stack.pop()?;
        let label = e.label.clone();
        let back = self.step_to(e);
        self.undo_stack.push(back);
        Some(label)
    }

    /// Brings the project to an undo or redo step, and returns the step that comes back here.
    fn step_to(&mut self, e: UndoEntry) -> UndoEntry {
        let now = self.capture_plugin_states();
        let plugin = e.plugin.as_ref().map(|(uid, _)| (*uid, self.vst_state_now(*uid)));
        let back = UndoEntry { project: now, label: e.label.clone(), key: None, at: Instant::now(), plugin };
        self.restore(e.project);
        if let Some((uid, st)) = e.plugin {
            self.restore_vst_state(uid, st);
        }
        back
    }

    /// Brings the engine to `target`, touching only what differs, so plug-ins that didn't
    /// change keep running (and keep their state).
    fn restore(&mut self, target: Project) {
        self.auto_abort();
        let current = std::mem::replace(&mut self.project, target);
        // The drag mode is a setting, not part of the edit history.
        self.project.drag_mode = current.drag_mode;
        // Neither is the project alternative's name, which is renamed on disk too.
        self.project.alternative = current.alternative.clone();
        let uid = |s: &Option<PluginSlot>| s.as_ref().map(|s| s.uid);

        // Tracks that no longer exist.
        let gone: Vec<&Track> = current.tracks.iter().filter(|t| self.project.track(t.id).is_none()).collect();
        for t in gone {
            for slot in t.inserts.iter().flatten().chain(t.instrument.iter()) {
                self.controllers.remove(&slot.uid);
            }
            self.meters.remove(&t.id);
            self.send(Cmd::RemoveTrack(t.id));
        }

        let tracks = self.project.tracks.clone();
        for (idx, t) in tracks.iter().enumerate() {
            let Some(cur) = current.track(t.id) else {
                // Came back: rebuild it, plug-ins restored from the snapshot's state.
                let dsp = self.build_track(t);
                self.send(Cmd::AddTrack(dsp, idx));
                continue;
            };
            if !same(&cur.regions, &t.regions) {
                self.push_regions(t.id);
            }
            if !same(&cur.eq, &t.eq) {
                let c = self.eq_coeffs(t);
                self.send(Cmd::Eq(t.id, c));
            }
            if !same(&cur.automation, &t.automation) {
                self.send(Cmd::Automation(t.id, lanes_dsp(&t.automation)));
                self.resync_builtins(t.id);
            }
            if cur.auto_mode != t.auto_mode {
                self.send(Cmd::AutoMode(t.id, t.auto_mode));
                self.resync_builtins(t.id);
            }
            // Freezing loads and unloads plug-ins itself.
            if self.restore_freeze(cur, t) {
                continue;
            }
            if uid(&cur.instrument) != uid(&t.instrument) {
                if let Some(old) = &cur.instrument {
                    self.controllers.remove(&old.uid);
                }
                let ins = t.instrument.as_ref().and_then(|s| self.instantiate_slot(s).map_err(|e| log::error!("{e}")).ok());
                self.send(Cmd::Instrument { track: t.id, insert: ins });
            } else if let (Some(a), Some(b)) = (&cur.instrument, &t.instrument) {
                if a.bypass != b.bypass {
                    self.send(Cmd::Bypass { track: t.id, uid: b.uid, bypass: b.bypass });
                }
                self.restore_builtin(t.id, a, b);
            }
            let reordered = self.restore_insert_order(t.id, &cur.inserts, &t.inserts);
            let had_inserts = reordered.as_deref().unwrap_or(&cur.inserts);
            for (slot, want) in t.inserts.iter().enumerate() {
                let had = had_inserts.get(slot).cloned().flatten();
                if uid(&had) != uid(want) {
                    if let Some(old) = &had {
                        self.controllers.remove(&old.uid);
                    }
                    let ins = want.as_ref().and_then(|s| self.instantiate_slot(s).map_err(|e| log::error!("{e}")).ok());
                    self.send(Cmd::Insert { track: t.id, slot, insert: ins });
                } else if let (Some(a), Some(b)) = (&had, want) {
                    if a.bypass != b.bypass {
                        self.send(Cmd::Bypass { track: t.id, uid: b.uid, bypass: b.bypass });
                    }
                    self.restore_builtin(t.id, a, b);
                }
            }
        }
        self.send(Cmd::Order(self.project.process_order()));
        self.push_all_params();

        // Stereo Out
        let master = self.project.master.clone();
        let reordered = self.restore_insert_order(MASTER_ID, &current.master.inserts, &master.inserts);
        let had_inserts = reordered.as_deref().unwrap_or(&current.master.inserts);
        for (slot, want) in master.inserts.iter().enumerate() {
            let had = had_inserts.get(slot).cloned().flatten();
            if uid(&had) != uid(want) {
                if let Some(old) = &had {
                    self.controllers.remove(&old.uid);
                }
                let ins = want.as_ref().and_then(|s| self.instantiate_slot(s).map_err(|e| log::error!("{e}")).ok());
                self.send(Cmd::Insert { track: MASTER_ID, slot, insert: ins });
            } else if let (Some(a), Some(b)) = (&had, want) {
                if a.bypass != b.bypass {
                    self.send(Cmd::Bypass { track: MASTER_ID, uid: b.uid, bypass: b.bypass });
                }
                self.restore_builtin(MASTER_ID, a, b);
            }
        }
        if !same(&current.master.automation, &master.automation) || current.master.auto_mode != master.auto_mode {
            self.sync_master_automation();
            self.resync_builtins(MASTER_ID);
        }

        // Recordings and imports made after the snapshot stay loaded, so redo can bring them back.
        if self.selected.map(|id| self.project.track(id).is_none()).unwrap_or(false) {
            self.selected = self.project.tracks.first().map(|t| t.id);
        }
        self.sync_globals();
        self.dirty = true;
    }

    // ---- transport --------------------------------------------------------------------------

    /// The playhead. Until the engine has applied the last locate (or record start), that's
    /// where it was sent: an audio callback already under way when it was sent still publishes
    /// the old position, so `status.pos` alone could jump back to it for a block, and a
    /// `play()` or `record()` right after a locate would start from there.
    pub fn position(&self) -> i64 {
        match self.pending_pos {
            Some((seq, pos)) if self.status.cmds_done.load(Ordering::Acquire) < seq => pos,
            _ => self.status.pos.load(Ordering::Relaxed),
        }
    }

    pub fn is_playing(&self) -> bool {
        self.status.playing.load(Ordering::Relaxed)
    }

    pub fn is_recording(&self) -> bool {
        self.rec_punch.is_some()
    }

    pub fn play(&mut self) {
        if self.is_playing() {
            return;
        }
        let from = self.position();
        self.last_play_from = from;
        self.send(Cmd::Play { from });
        self.auto_play_started(from);
    }

    pub fn stop(&mut self) {
        if !self.is_playing() && self.rec_punch.is_none() {
            // A second Stop returns to the left locator with Cycle on, otherwise the start.
            let to = if self.project.cycle.enabled { self.project.cycle.start } else { 0 };
            self.locate(to);
            return;
        }
        let stopped_at = self.position();
        self.send(Cmd::Stop);
        if self.rec_punch.is_some() {
            self.wait_for_engine();
            self.finish_recording();
        }
        // After the takes are in, so undoing the written automation keeps them.
        self.auto_stopped(stopped_at);
    }

    pub fn toggle_play(&mut self) {
        if self.is_playing() {
            self.stop();
        } else {
            self.play();
        }
    }

    pub fn locate(&mut self, pos: i64) {
        let from = self.position();
        self.send(Cmd::Locate(pos.max(0)));
        self.pending_pos = Some((self.cmds_sent, pos.max(0)));
        self.auto_jumped(from, pos.max(0));
    }

    pub fn record(&mut self) -> Result<()> {
        if self.rec_punch.is_some() {
            return Ok(());
        }
        let playing = self.is_playing();
        let playhead = self.position();
        let cycle = Some((self.project.cycle.start, self.project.cycle.end)).filter(|c| self.project.cycle.enabled && c.1 > c.0);
        // Cycle recording starts at the left locator (or carries on when already cycling), so
        // every pass lines up with the ones before it.
        let punch = match cycle {
            Some((cs, ce)) if !(playing && playhead >= cs && playhead < ce) => cs,
            _ => playhead,
        };
        let rec = &self.prefs.recording;
        let from = if playing {
            punch
        } else if self.project.count_in {
            // Whole bars at the tempo and signature in effect at the punch.
            let bar = self.tempo_map().samples_per_bar_at(punch as f64);
            punch - (bar * rec.count_in_bars.clamp(1, 4) as f64).round() as i64
        } else {
            // Pre-roll: play a moment of the song first, then drop into record at the playhead.
            punch - (rec.pre_roll_secs.clamp(0.0, 30.0) as f64 * self.sample_rate()).round() as i64
        };
        let depth = self.project.bit_depth.unwrap_or(rec.bit_depth);

        // An Off track can't record: it keeps its R button, ready for when it's back on.
        let enabled = self.project.tracks.iter().filter(|t| t.armed && t.frozen.is_none());
        let armed: Vec<Track> = enabled.clone().filter(|t| !self.project.is_off(t.id)).cloned().collect();
        if armed.is_empty() {
            return Err(anyhow!(if enabled.count() > 0 { "Every record-enabled track is off. Turn one on to record." } else { "No tracks are record-enabled" }));
        }
        let (plan, quiet, capture) = self.record_plan(from, punch, cycle)?;
        self.checkpoint("Record", None, false);
        let sr = self.sample_rate() as u32;
        for t in armed.iter().filter(|t| t.kind == TrackKind::Audio && t.input != InputSel::None) {
            let path = self.next_take_path(&t.name);
            let sink = self.recorder.begin(t.id, path, t.input.channels(), sr, capture.0, depth)?;
            self.send(Cmd::RecordSink { track: t.id, sink: Some(sink) });
        }
        while self.midi_rec_rx.pop().is_ok() {}
        let target = self.midi_target_id();
        let armed_inst: Vec<Id> = armed.iter().filter(|t| t.kind == TrackKind::Instrument).map(|t| t.id).collect();
        self.rec_midi_track = match target {
            Some(t) if armed_inst.contains(&t) => Some(t),
            _ => armed_inst.first().copied(),
        };
        if let Some(id) = self.rec_midi_track {
            if Some(id) != target {
                self.send(Cmd::MidiTarget(Some(id)));
            }
        }
        self.rec_punch = Some(punch);
        self.rec_plan = Some(plan);
        self.start_journal();
        self.last_play_from = punch;
        self.send(Cmd::Autopunch { range: quiet, capture });
        self.send(Cmd::Record { from, punch });
        self.pending_pos = Some((self.cmds_sent, from));
        if !playing {
            self.auto_play_started(from.max(0));
        }
        Ok(())
    }

    fn next_take_path(&self, track_name: &str) -> PathBuf {
        let clean = |n: &str| -> String { n.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect() };
        let clean = match self.prefs.recording.take_names {
            TakeNames::ProjectTrack => format!("{} - {}", clean(&self.project.name), clean(track_name)),
            _ => clean(track_name),
        };
        let dir = self.recording_dir();
        for i in 1.. {
            let p = dir.join(format!("{clean}#{i:02}.wav"));
            if !p.exists() {
                return p;
            }
        }
        unreachable!()
    }

    fn finish_recording(&mut self) {
        if self.rec_punch.is_none() {
            return;
        }
        let takes = self.recorder.finish_all();
        let comp = self.record_latency_frames();
        self.place_recording(takes, comp);
        self.recording_finished();
    }

    /// Turns what the recording in progress (`rec_punch`, `rec_plan`) captured into regions:
    /// the audio `takes`, moved `comp` frames earlier, and the MIDI. Crash recovery replays an
    /// interrupted recording through here too (`session/recovery.rs`).
    fn place_recording(&mut self, takes: Vec<(Id, FinishedTake)>, comp: i64) {
        let Some(punch) = self.rec_punch.take() else { return };
        let plan = self.rec_plan.take();
        let stop_pos = self.position();
        // MIDI only travels one way: the performer reacts to audio that left late.
        let midi_comp = (self.status.output_latency_us.load(Ordering::Relaxed) as f64 * self.sample_rate() / 1e6).round() as i64 + self.pdc_global.total as i64;
        let mut touched = Vec::new();
        for (track, take) in takes {
            if let Some(e) = &take.error {
                self.warning = Some(e.clone());
            }
            let frames = take.channels.first().map(|c| c.len()).unwrap_or(0);
            self.send(Cmd::RecordSink { track, sink: None });
            if frames == 0 {
                let _ = std::fs::remove_file(&take.path);
                continue;
            }
            let clip_id = self.project.alloc_id();
            let rel = take.path.strip_prefix(&self.dir).unwrap_or(&take.path).to_string_lossy().to_string();
            let name = take.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let clip = Clip::from_channels(take.path.clone(), self.sample_rate() as u32, take.channels);
            self.clips.insert(clip_id, Arc::new(clip));
            self.project.clips.push(ClipRef { id: clip_id, path: rel });
            let region_id = self.project.alloc_id();
            // Line the take up with what was being heard; trim anything shifted before zero.
            let shifted = take.start - comp;
            let offset = (-shifted).max(0);
            let length = frames as i64 - offset;
            if length <= 0 {
                continue;
            }
            let name = match self.prefs.recording.take_names {
                TakeNames::Track => self.project.track(track).map(|t| t.name.clone()).unwrap_or(name),
                _ => name,
            };
            let fade = self.default_fade(length);
            let region = Region {
                id: region_id,
                name,
                start: shifted.max(0),
                length,
                muted: false,
                color: None,
                content: RegionContent::Audio { clip: clip_id, offset, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() },
                loop_end: None,
                takes: None,
                params: Default::default(),
                pattern: None,
            };
            // Cycle passes become a take folder; Autopunch trims to the punch range.
            let Some(region) = self.shape_take(plan, track, region, take.start, frames as i64) else {
                touched.push(track);
                continue;
            };
            if let Some(dest) = self.place_take(track, region) {
                touched.push(track);
                touched.push(dest);
            }
        }

        // MIDI: pair note-ons with note-offs; notes still held end where recording stopped.
        let mut events = Vec::new();
        while let Ok((pos, d)) = self.midi_rec_rx.pop() {
            events.push(((pos - midi_comp).max(0), d));
        }
        if let (Some(track), false) = (self.rec_midi_track.take(), events.is_empty()) {
            let map = self.tempo_map();
            let w = takes::midi_window(plan, punch, stop_pos);
            // From the bar line at or before the first event to the one at or after the last.
            let start = map.bar_start(map.bar_at(w.lo as f64)) as i64;
            let hi = w.hi.max(w.lo + 1) as f64;
            let last = map.bar_at(hi);
            let end = if map.bar_start(last) >= hi { map.bar_start(last) } else { map.bar_start(last + 1) } as i64;
            if self.prefs.recording.midi_mode() == MidiOverlap::TakeFolders {
                // Each cycle pass is a take; a held note ends at the cycle end, or where
                // recording stopped in the last pass.
                let passes = crate::takes::midi_passes(&events);
                let n = passes.len();
                let takes: Vec<(Vec<Note>, Vec<CcEvent>)> = passes
                    .iter()
                    .enumerate()
                    .map(|(k, p)| takes::midi_take(p, &w, start, if k + 1 == n { stop_pos } else { w.hi }))
                    .filter(|(n, c)| !n.is_empty() || !c.is_empty())
                    .collect();
                if !takes.is_empty() {
                    self.place_midi_takes(track, start, end, takes);
                    touched.push(track);
                }
                events.clear();
            }
            let (notes, cc) = takes::midi_take(&events, &w, start, stop_pos);
            if !notes.is_empty() || !cc.is_empty() {
                let merged = match self.prefs.recording.midi_mode() {
                    MidiOverlap::NewRegion | MidiOverlap::TakeFolders => false,
                    MidiOverlap::Merge => self.merge_midi_take(track, start, end, &notes, &cc),
                    // From the punch-in (or where recording started) to where it stopped.
                    MidiOverlap::Replace => self.replace_midi_take(track, (w.lo, w.hi), start, end, &notes, &cc),
                };
                let id = self.project.alloc_id();
                if let (false, Some(t)) = (merged, self.project.track_mut(track)) {
                    let name = t.name.clone();
                    t.regions.push(Region { id, name, start, length: end - start, muted: false, color: None, content: RegionContent::Midi { notes, cc }, loop_end: None, takes: None, params: Default::default(), pattern: None });
                }
                touched.push(track);
            }
        }
        touched.sort_unstable();
        touched.dedup();
        for t in touched {
            self.push_regions(t);
        }
        self.update_midi_target();
        self.dirty = true;
    }

    pub fn set_cycle(&mut self, enabled: bool, start: i64, end: i64) {
        let (start, end) = (start.min(end).max(0), start.max(end).max(0));
        self.project.cycle = Cycle { enabled: enabled && end > start, start, end };
        let c = self.project.cycle.clone();
        self.send(Cmd::Cycle { enabled: c.enabled, start: c.start, end: c.end });
    }

    pub fn set_tempo(&mut self, bpm: f64) {
        self.project.tempo = bpm.clamp(5.0, 990.0);
        // The base tempo is the Tempo track's first event.
        if let Some(first) = self.project.tempo_map.first_mut() {
            first.bpm = self.project.tempo;
        }
        self.dirty = true;
        self.sync_globals();
    }

    pub fn set_signature(&mut self, num: u32, den: u32) {
        self.project.sig_num = num.clamp(1, 32);
        self.project.sig_den = if [1, 2, 4, 8, 16].contains(&den) { den } else { 4 };
        if let Some(first) = self.project.signatures.first_mut() {
            first.num = self.project.sig_num;
            first.den = self.project.sig_den;
        }
        self.dirty = true;
        self.sync_globals();
    }

    pub fn set_key(&mut self, key: String) {
        self.project.key = key;
        self.dirty = true;
    }

    // Metronome and Count-in are transport switches, not undo steps (as in Logic), but they're
    // saved with the project, so they count as changes.
    pub fn set_metronome(&mut self, on: bool) {
        self.project.metronome = on;
        self.dirty = true;
        self.sync_globals();
    }

    pub fn set_count_in(&mut self, on: bool) {
        self.project.count_in = on;
        self.dirty = true;
        self.sync_globals();
    }

    pub fn set_master_volume(&mut self, db: f32) {
        self.project.master.volume_db = db.clamp(-96.0, 6.0);
        self.send(Cmd::MasterVolume(volume_gain(self.project.master.volume_db)));
    }

    // ---- tracks -----------------------------------------------------------------------------

    pub fn add_track(&mut self, kind: TrackKind, input: InputSel, instrument: Option<String>, index: Option<usize>) -> Result<Id> {
        let id = self.project.alloc_id();
        let count = self.project.tracks.iter().filter(|t| t.kind == kind).count() + 1;
        let bus = if kind == TrackKind::Aux { self.free_bus() } else { 0 };
        let (name, color) = match kind {
            TrackKind::Audio => (format!("Audio {count}"), AUDIO_COLOR),
            TrackKind::Instrument => (format!("Inst {count}"), INSTRUMENT_COLOR),
            TrackKind::Folder => (format!("Folder {count}"), STACK_COLOR),
            TrackKind::Summing => (format!("Sum {count}"), STACK_COLOR),
            TrackKind::Aux => (format!("Aux {bus}"), AUX_COLOR),
        };
        let mut track = Track {
            id,
            kind,
            name,
            color: color.into(),
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            armed: false,
            monitor: false,
            input: if kind == TrackKind::Audio { input } else { InputSel::None },
            instrument: None,
            inserts: vec![None; INSERT_SLOTS],
            regions: Vec::new(),
            eq: ChannelEq::default(),
            parent: None,
            collapsed: false,
            group: None,
            hidden: false,
            sends: vec![None; SEND_SLOTS],
            output: None,
            bus,
            automation: Vec::new(),
            auto_mode: AutoMode::Read,
            frozen: None,
            vca: None,
            alternatives: Vec::new(),
            alternative: String::new(),
            auto_lanes: Vec::new(),
            instrument_output: None,
            off: false,
        };
        if kind == TrackKind::Instrument {
            let pid = instrument.unwrap_or_else(|| BUILTIN_SYNTH.to_string());
            let built_in = crate::instrument::find(&pid);
            let name = match &built_in {
                Some(i) => i.name.to_string(),
                None => self.find_plugin(&pid).map(|d| d.name.clone()).unwrap_or_else(|| "Instrument".into()),
            };
            track.name = if pid == BUILTIN_SYNTH { format!("Inst {count}") } else { name.clone() };
            let params = built_in.map(|i| i.params.iter().map(|p| (p.id, p.default)).collect()).unwrap_or_default();
            track.instrument = Some(PluginSlot { uid: self.project.alloc_id(), plugin_id: pid, name, bypass: false, state: None, params, sidechain: None, sidechain_pre: false });
        }
        let dsp = self.build_track(&track);
        let idx = index.unwrap_or(self.project.tracks.len()).min(self.project.tracks.len());
        // A track created among a stack's subtracks joins that stack.
        let parent = self.project.tracks.get(idx).and_then(|t| t.parent);
        self.project.tracks.insert(idx, track);
        self.project.tracks[idx].parent = parent;
        self.send(Cmd::AddTrack(dsp, idx));
        self.sync_tracks();
        self.dirty = true;
        Ok(id)
    }

    pub fn delete_track(&mut self, id: Id) {
        self.delete_tracks(&[id]);
    }

    /// Deletes tracks; a stack goes with its subtracks.
    pub fn delete_tracks(&mut self, ids: &[Id]) {
        let mut all: Vec<Id> = Vec::new();
        for &id in ids {
            if self.project.track(id).is_some() && !all.contains(&id) {
                all.push(id);
                all.extend(self.project.descendants(id).into_iter().filter(|d| !ids.contains(d)));
            }
        }
        if all.is_empty() {
            return;
        }
        let first = self.project.tracks.iter().position(|t| all.contains(&t.id)).unwrap_or(0);
        for id in &all {
            let Some(i) = self.project.tracks.iter().position(|t| t.id == *id) else { continue };
            let t = self.project.tracks.remove(i);
            for slot in t.inserts.iter().flatten().chain(t.instrument.iter()) {
                self.controllers.remove(&slot.uid);
            }
            self.meters.remove(id);
            self.send(Cmd::RemoveTrack(*id));
        }
        if self.selected.map(|s| all.contains(&s)).unwrap_or(false) {
            self.selected = self.project.tracks.get(first.min(self.project.tracks.len().saturating_sub(1))).map(|t| t.id);
        }
        self.prune_groups();
        self.sync_tracks();
        self.dirty = true;
    }

    /// Tracks without regions (stacks that still hold tracks are kept), as Logic's Delete
    /// Unused Tracks. Returns how many went.
    pub fn delete_unused_tracks(&mut self) -> usize {
        let mut unused: Vec<Id> = self
            .project
            .tracks
            .iter()
            .filter(|t| t.kind != TrackKind::Aux && t.regions.is_empty() && t.alternatives.iter().all(|a| a.regions.is_empty()) && !(t.kind.is_stack() && self.project.tracks.iter().any(|c| c.parent == Some(t.id))))
            .map(|t| t.id)
            .collect();
        // An aux is used while a track that stays sends or routes to it. Auxes can feed each
        // other, so repeat until nothing changes.
        loop {
            let p = &self.project;
            let idle: Vec<Id> = p
                .tracks
                .iter()
                .filter(|a| a.kind == TrackKind::Aux && !unused.contains(&a.id))
                .filter(|a| !p.tracks.iter().any(|t| t.id != a.id && !unused.contains(&t.id) && p.feeds(t.id).contains(&a.id)))
                .map(|a| a.id)
                .collect();
            if idle.is_empty() {
                break;
            }
            unused.extend(idle);
        }
        self.delete_tracks(&unused);
        unused.len()
    }

    /// Reorders tracks, optionally moving some into or out of stacks (`parents`).
    pub fn reorder_tracks(&mut self, ids: Vec<Id>) {
        self.move_tracks(ids, &[]);
    }

    pub fn move_tracks(&mut self, ids: Vec<Id>, parents: &[(Id, Option<Id>)]) {
        self.project.tracks.sort_by_key(|t| ids.iter().position(|&i| i == t.id).unwrap_or(usize::MAX));
        for &(id, parent) in parents {
            // A track can't go into itself or one of its own subtracks.
            let ok = parent.map(|p| p != id && !self.project.descendants(id).contains(&p) && self.project.track(p).map(|t| t.kind.is_stack()).unwrap_or(false)).unwrap_or(true);
            // Auxes stay out of stacks, which keeps feedback loops out of the stack tree.
            let ok = ok && (parent.is_none() || !self.project.is_aux(id));
            if let (true, Some(t)) = (ok, self.project.track_mut(id)) {
                t.parent = parent;
            }
        }
        self.sync_tracks();
        self.dirty = true;
    }

    /// A name for a copy of `base`: a trailing number counts up ("Audio 1" -> "Audio 2"),
    /// otherwise one is added.
    fn copy_name(&self, base: &str) -> String {
        let trimmed = base.trim_end_matches(|c: char| c.is_ascii_digit());
        let (stem, n) = match base[trimmed.len()..].parse::<u32>() {
            Ok(n) if trimmed.ends_with(' ') || trimmed.is_empty() => (trimmed.to_string(), n + 1),
            _ => (format!("{base} "), 2),
        };
        (n..).map(|k| format!("{stem}{k}")).find(|name| !self.project.tracks.iter().any(|t| &t.name == name)).unwrap()
    }

    /// The slot with a fresh instance id and the running plug-in's current settings.
    fn copy_slot(&mut self, slot: &PluginSlot) -> PluginSlot {
        let state = self.controllers.get(&slot.uid).map(|c| encode_state(&c.save_state())).or_else(|| slot.state.clone());
        PluginSlot { uid: self.project.alloc_id(), state, ..slot.clone() }
    }

    /// Duplicates tracks below the originals: settings, plug-ins (with their current settings)
    /// and, with `content`, the regions. A stack is copied with its subtracks. Returns the
    /// copies of the given tracks.
    pub fn duplicate_tracks(&mut self, ids: &[Id], content: bool) -> Vec<Id> {
        let mut roots: Vec<Id> = self.project.tracks.iter().map(|t| t.id).filter(|id| ids.contains(id)).collect();
        // Copying a stack already copies its subtracks.
        let anc: Vec<Vec<Id>> = roots.iter().map(|&id| self.project.ancestors(id)).collect();
        let keep: Vec<bool> = anc.iter().map(|a| !a.iter().any(|x| roots.contains(x))).collect();
        let mut k = keep.iter();
        roots.retain(|_| *k.next().unwrap());
        let mut made = Vec::new();
        for root in roots {
            let mut family = vec![root];
            family.extend(self.project.descendants(root));
            let mut map: HashMap<Id, Id> = HashMap::new();
            let mut copies = Vec::new();
            for id in &family {
                let src = self.project.track(*id).unwrap().clone();
                let new_id = self.project.alloc_id();
                map.insert(*id, new_id);
                let mut t = src.clone();
                t.id = new_id;
                t.name = self.copy_name(&src.name);
                t.solo = false;
                t.armed = false;
                // A copy of a frozen track is live: it gets its own plug-ins from their saved
                // settings rather than sharing the freeze file.
                t.frozen = None;
                if t.kind == TrackKind::Aux {
                    t.bus = self.free_bus();
                }
                t.instrument = src.instrument.as_ref().map(|s| self.copy_slot(s));
                t.inserts = src.inserts.iter().map(|s| s.as_ref().map(|s| self.copy_slot(s))).collect();
                // Automation goes with the content, onto the copied plug-ins.
                if content {
                    let old = src.inserts.iter().flatten().chain(src.instrument.iter()).map(|s| s.uid);
                    let new = t.inserts.iter().flatten().chain(t.instrument.iter()).map(|s| s.uid);
                    crate::automation::remap_plugins(&mut t.automation, &old.zip(new).collect::<Vec<_>>());
                } else {
                    t.automation.clear();
                }
                t.regions = if content {
                    src.regions.iter().map(|r| Region { id: self.project.alloc_id(), ..r.clone() }).collect()
                } else {
                    Vec::new()
                };
                // Track alternatives are content too, and their regions need ids of their own:
                // a copy sharing them would edit the original's regions once both play them.
                if content {
                    for r in t.alternatives.iter_mut().flat_map(|a| a.regions.iter_mut()) {
                        r.id = self.project.alloc_id();
                    }
                } else {
                    t.alternatives.clear();
                    t.alternative.clear();
                }
                // Extra lanes of the original's plug-ins show the copies' parameters.
                let old = src.inserts.iter().flatten().chain(src.instrument.iter()).map(|s| s.uid);
                let new = t.inserts.iter().flatten().chain(t.instrument.iter()).map(|s| s.uid);
                let map: Vec<(Id, Id)> = old.zip(new).collect();
                for p in &mut t.auto_lanes {
                    if let AutoParam::Plugin { uid, .. } = p {
                        if let Some((_, n)) = map.iter().find(|(o, _)| o == uid) {
                            *uid = *n;
                        }
                    }
                }
                copies.push(t);
            }
            for t in &mut copies {
                if t.id != map[&root] {
                    t.parent = t.parent.map(|p| *map.get(&p).unwrap_or(&p));
                }
                // So the new track's name doesn't count as taken twice.
            }
            let end = self.project.tracks.iter().rposition(|t| family.contains(&t.id)).unwrap_or(0) + 1;
            for (k, t) in copies.into_iter().enumerate() {
                let dsp = self.build_track(&t);
                self.project.tracks.insert(end + k, t);
                self.send(Cmd::AddTrack(dsp, end + k));
            }
            made.push(map[&root]);
        }
        self.sync_tracks();
        self.dirty = true;
        made
    }

    /// Puts tracks into a new track stack, created above the first of them. They must share
    /// a parent; the ones that don't are left out.
    pub fn create_stack(&mut self, ids: &[Id], summing: bool) -> Option<Id> {
        let order: Vec<Id> = self.project.tracks.iter().map(|t| t.id).filter(|id| ids.contains(id)).collect();
        let first = *order.first()?;
        let parent = self.project.track(first)?.parent;
        let members: Vec<Id> = order.into_iter().filter(|id| self.project.track(*id).map(|t| t.parent == parent && t.kind != TrackKind::Aux).unwrap_or(false)).collect();
        let first = *members.first()?;
        let idx = self.project.tracks.iter().position(|t| t.id == first)?;
        let kind = if summing { TrackKind::Summing } else { TrackKind::Folder };
        let id = self.add_track(kind, InputSel::None, None, Some(idx)).ok()?;
        let color = self.project.track(first).map(|t| t.color.clone()).unwrap_or_else(|| STACK_COLOR.into());
        if let Some(t) = self.project.track_mut(id) {
            t.parent = parent;
            t.color = color;
        }
        for m in &members {
            self.project.track_mut(*m).unwrap().parent = Some(id);
        }
        self.sync_tracks();
        self.dirty = true;
        Some(id)
    }

    /// Removes a stack's main track; its subtracks stay, one level up.
    pub fn flatten_stack(&mut self, id: Id) {
        let Some(t) = self.project.track(id) else { return };
        if !t.kind.is_stack() {
            return;
        }
        let up = t.parent;
        for c in self.project.tracks.iter_mut().filter(|c| c.parent == Some(id)) {
            c.parent = up;
        }
        self.delete_tracks(&[id]);
    }

    /// Hides tracks (or shows them again). A stack takes its subtracks with it. Hidden tracks
    /// keep playing, as in Logic; only the views leave them out.
    pub fn set_tracks_hidden(&mut self, ids: &[Id], hidden: bool) {
        let mut all = ids.to_vec();
        for &id in ids {
            all.extend(self.project.descendants(id));
        }
        for t in self.project.tracks.iter_mut().filter(|t| all.contains(&t.id)) {
            t.hidden = hidden;
        }
        self.dirty = true;
    }

    // ---- groups -------------------------------------------------------------------------------

    /// A new group holding `ids` (leaving any group they were in). Returns its id.
    pub fn create_group(&mut self, ids: &[Id]) -> Id {
        let id = self.project.alloc_id();
        let n = (1..).find(|n| !self.project.groups.iter().any(|g| g.name == format!("Group {n}"))).unwrap();
        let colors = ["#e0a030", "#d9534f", "#5cb85c", "#5bc0de", "#b07cd8", "#e0709a"];
        self.project.groups.push(Group {
            id,
            name: format!("Group {n}"),
            color: colors[(n as usize - 1) % colors.len()].into(),
            enabled: true,
            volume: true,
            pan: false,
            mute: true,
            solo: true,
            record: true,
            select: true,
            editing: true,
        });
        self.set_track_group(ids, Some(id));
        id
    }

    pub fn update_group(&mut self, g: Group) {
        if let Some(cur) = self.project.groups.iter_mut().find(|x| x.id == g.id) {
            *cur = g;
            self.dirty = true;
        }
    }

    pub fn delete_group(&mut self, id: Id) {
        for t in self.project.tracks.iter_mut().filter(|t| t.group == Some(id)) {
            t.group = None;
        }
        self.project.groups.retain(|g| g.id != id);
        self.dirty = true;
    }

    pub fn set_track_group(&mut self, ids: &[Id], group: Option<Id>) {
        let group = group.filter(|g| self.project.group(*g).is_some());
        for t in self.project.tracks.iter_mut().filter(|t| ids.contains(&t.id)) {
            t.group = group;
        }
        self.prune_groups();
        self.dirty = true;
    }

    /// Drops groups nobody is in.
    fn prune_groups(&mut self) {
        let used: Vec<Id> = self.project.tracks.iter().filter_map(|t| t.group).collect();
        self.project.groups.retain(|g| used.contains(&g.id));
    }

    /// The other tracks a change to `id` reaches through its group, if `link` is shared.
    fn group_peers(&self, id: Id, link: impl Fn(&Group) -> bool) -> Vec<Id> {
        let Some(g) = self.project.track(id).and_then(|t| t.group).and_then(|g| self.project.group(g)) else { return Vec::new() };
        if !g.enabled || !link(g) {
            return Vec::new();
        }
        self.project.tracks.iter().filter(|t| t.group == Some(g.id) && t.id != id).map(|t| t.id).collect()
    }

    /// `update_track` as the user means it: groups pass shared changes on to their other
    /// members (volume and pan as relative moves, the switches as the same state).
    pub fn update_track_linked(&mut self, id: Id, patch: TrackPatch) {
        let Some(t) = self.project.track(id).cloned() else { return };
        let mut peers: Vec<(Id, TrackPatch)> = Vec::new();
        let mut add = |ids: Vec<Id>, f: &dyn Fn(&mut TrackPatch, &Track)| {
            for pid in ids {
                let Some(pt) = self.project.track(pid) else { continue };
                let i = match peers.iter().position(|(x, _)| *x == pid) {
                    Some(i) => i,
                    None => {
                        peers.push((pid, TrackPatch::default()));
                        peers.len() - 1
                    }
                };
                f(&mut peers[i].1, pt);
            }
        };
        if let Some(v) = patch.volume_db {
            let d = v - t.volume_db;
            add(self.group_peers(id, |g| g.volume), &|p, pt| p.volume_db = Some(pt.volume_db + d));
        }
        if let Some(v) = patch.pan {
            let d = v - t.pan;
            add(self.group_peers(id, |g| g.pan), &|p, pt| p.pan = Some(pt.pan + d));
        }
        if let Some(v) = patch.mute {
            add(self.group_peers(id, |g| g.mute), &|p, _| p.mute = Some(v));
        }
        if let Some(v) = patch.solo {
            add(self.group_peers(id, |g| g.solo), &|p, _| p.solo = Some(v));
        }
        if let Some(v) = patch.armed {
            add(self.group_peers(id, |g| g.record), &|p, pt| p.armed = Some(v && !pt.kind.is_stack()));
        }
        // The values before the move, for automation writing.
        let before = |p: &Project, tid: Id, patch: &TrackPatch| {
            let t = p.track(tid);
            let was = |on: bool, f: &dyn Fn(&Track) -> f32| if on { t.map(f) } else { None };
            (tid, [was(patch.volume_db.is_some(), &|t| t.volume_db), was(patch.pan.is_some(), &|t| t.pan), was(patch.mute.is_some(), &|t| t.mute as u8 as f32)])
        };
        let mut moved = vec![before(&self.project, id, &patch)];
        moved.extend(peers.iter().map(|(pid, p)| before(&self.project, *pid, p)));
        self.update_track(id, patch);
        for (pid, p) in peers {
            self.update_track(pid, p);
        }
        // While playing in Touch, Latch or Write, the moves are recorded as automation.
        for (tid, was) in moved {
            self.auto_moved(tid, was);
        }
    }

    pub fn update_track(&mut self, id: Id, patch: TrackPatch) {
        let solo_changed = patch.solo.is_some();
        let recording = self.rec_punch.is_some();
        let Some(t) = self.project.track_mut(id) else { return };
        if let Some(v) = patch.name {
            t.name = v;
        }
        if let Some(v) = patch.color {
            t.color = v;
        }
        if let Some(v) = patch.volume_db {
            t.volume_db = v.clamp(-96.0, 6.0);
        }
        if let Some(v) = patch.pan {
            t.pan = v.clamp(-1.0, 1.0);
        }
        if let Some(v) = patch.mute {
            t.mute = v;
        }
        if let Some(v) = patch.solo {
            t.solo = v;
        }
        if let Some(v) = patch.armed {
            t.armed = v && t.kind.can_record() && t.frozen.is_none();
        }
        if let Some(v) = patch.monitor {
            t.monitor = v;
        }
        if let Some(v) = patch.input {
            if t.kind == TrackKind::Audio {
                t.input = v;
            }
        }
        if let Some(v) = patch.collapsed {
            t.collapsed = v;
        }
        // Not while recording: `commands.rs` says so (see `Session::off_refused`).
        let off_changed = !recording && patch.off.is_some_and(|v| v != t.off);
        if off_changed {
            t.off = patch.off.unwrap_or(t.off);
        }
        // Stacks pass changes on to their subtracks, a subtrack's solo reaches its stacks, and
        // solo follows sends and outputs. On/Off changes delay compensation too.
        if t.kind.is_stack() || t.parent.is_some() || solo_changed || off_changed {
            self.push_all_params();
        } else {
            let params = Self::track_params(&self.project, self.project.track(id).unwrap());
            self.send(Cmd::Params(id, params));
        }
        self.update_midi_target();
        self.dirty = true;
    }

    /// Replaces a strip's channel EQ settings (`MASTER_ID` for Stereo Out).
    pub fn set_eq(&mut self, track: Id, eq: ChannelEq) {
        let coeffs = crate::eq::coefficients(&eq, self.sample_rate());
        if track == MASTER_ID {
            self.project.master.eq = eq;
        } else if let Some(t) = self.project.track_mut(track) {
            t.eq = eq;
        } else {
            return;
        }
        // A frozen track's EQ is in its freeze file; `frozen_guard` puts the edit back.
        if !self.is_frozen(track) {
            self.send(Cmd::Eq(track, coeffs));
        }
        self.dirty = true;
    }

    /// Chooses which strip feeds the analyzer (None turns it off).
    pub fn set_analyzer(&mut self, target: Option<Id>) {
        self.analyzer_target = target;
        self.send(Cmd::Analyzer(target));
    }

    pub fn select_track(&mut self, id: Option<Id>) {
        self.selected = id;
        self.update_midi_target();
    }

    // ---- regions ----------------------------------------------------------------------------

    fn find_region(&self, id: Id) -> Option<(Id, usize)> {
        self.project.tracks.iter().find_map(|t| t.regions.iter().position(|r| r.id == id).map(|i| (t.id, i)))
    }

    /// Moves a region in time and optionally onto another track of the same kind.
    pub fn move_region(&mut self, id: Id, start: i64, track: Option<Id>) {
        let Some((from, idx)) = self.find_region(id) else { return };
        let pairs = self.crossfade_pairs(&[id]);
        let to = track.unwrap_or(from);
        let kind_ok = match (self.project.track(from), self.project.track(to)) {
            (Some(a), Some(b)) => a.kind == b.kind,
            _ => false,
        };
        let to = if kind_ok { to } else { from };
        // Moved regions come to the front, so they win where they now overlap.
        let mut r = self.project.track_mut(from).unwrap().regions.remove(idx);
        r.move_to(start.max(0));
        self.project.track_mut(to).unwrap().regions.push(r);
        self.push_regions(from);
        if to != from {
            self.push_regions(to);
        }
        self.dirty = true;
        self.auto_crossfade(&[id], pairs);
    }

    /// Trims a region's edges. `start`/`length` are timeline samples. A loop keeps its end and
    /// repeats the new window; it goes when the region now reaches past it.
    pub fn resize_region(&mut self, id: Id, start: i64, length: i64) {
        let Some((track, idx)) = self.find_region(id) else { return };
        let pairs = self.crossfade_pairs(&[id]);
        let clips = &self.clips;
        let t = self.project.track_mut(track).unwrap();
        let r = &mut t.regions[idx];
        let delta = start - r.start;
        let mut start = start;
        let mut length = length.max(1);
        if let RegionContent::Audio { clip, offset, .. } = &mut r.content {
            // A take folder's offset is in take time, which runs as far as its takes do.
            let frames = match &r.takes {
                Some(f) => f.takes.iter().map(|t| t.end).max().unwrap_or(1),
                None => clips.get(clip).map(|c| c.frames() as i64).unwrap_or(i64::MAX),
            };
            let new_off = (*offset + delta).clamp(0, frames - 1);
            start = r.start + (new_off - *offset);
            length = length.min(frames - new_off);
            *offset = new_off;
        } else if r.takes.is_some() {
            // A MIDI take folder keeps its takes whole: it shows the take's notes in its new span.
            crate::takes::retrim_midi_folder(r, start.max(0), length);
        } else if let RegionContent::Midi { notes, cc } = &mut r.content {
            crate::midi::trim_front(notes, cc, delta);
        }
        r.start = start.max(0);
        r.length = length;
        if !r.is_looped() {
            r.loop_end = None;
        }
        self.push_regions(track);
        self.dirty = true;
        self.auto_crossfade(&[id], pairs);
    }

    pub fn delete_regions(&mut self, ids: &[Id]) {
        let mut touched = Vec::new();
        for t in &mut self.project.tracks {
            let before = t.regions.len();
            t.regions.retain(|r| !ids.contains(&r.id));
            if t.regions.len() != before {
                touched.push(t.id);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
    }

    pub fn set_region_muted(&mut self, id: Id, muted: bool) {
        let Some((track, idx)) = self.find_region(id) else { return };
        self.project.track_mut(track).unwrap().regions[idx].muted = muted;
        self.push_regions(track);
        self.dirty = true;
    }

    pub fn rename_region(&mut self, id: Id, name: String) {
        if let Some((track, idx)) = self.find_region(id) {
            self.project.track_mut(track).unwrap().regions[idx].name = name;
            self.dirty = true;
        }
    }

    /// Splits the given regions (every region if empty) at `pos`. A loop that `pos` falls in is
    /// turned into real copies first, as Logic does.
    pub fn split_at(&mut self, pos: i64, region_ids: &[Id]) {
        let looped: Vec<Id> = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.regions.iter())
            .filter(|r| (region_ids.is_empty() || region_ids.contains(&r.id)) && r.is_looped() && pos > r.start && pos < r.end())
            .map(|r| r.id)
            .collect();
        let mut picked = region_ids.to_vec();
        if !looped.is_empty() {
            let copies = self.convert_loops(&looped);
            if !picked.is_empty() {
                picked.extend(copies);
            }
        }
        let region_ids = &picked[..];
        let mut targets = Vec::new();
        for (ti, t) in self.project.tracks.iter().enumerate() {
            for (ri, r) in t.regions.iter().enumerate() {
                let picked = region_ids.is_empty() || region_ids.contains(&r.id);
                if picked && pos > r.start && pos < r.start + r.length {
                    // Snap Edits to Zero Crossings cuts each region at its own crossing.
                    targets.push((ti, ri, self.zero_cut(r, pos)));
                }
            }
        }
        let mut touched = Vec::new();
        for (ti, ri, pos) in targets {
            let new_id = self.project.alloc_id();
            let t = &mut self.project.tracks[ti];
            let r = &mut t.regions[ri];
            let cut = pos - r.start;
            let mut right = r.clone();
            right.id = new_id;
            right.start = pos;
            right.length = r.length - cut;
            match (&mut right.content, &mut r.content) {
                (RegionContent::Audio { offset, .. }, _) => *offset += cut,
                (RegionContent::Midi { notes, cc }, RegionContent::Midi { notes: left, cc: left_cc }) => {
                    let ((ln, lc), (rn, rc)) = crate::midi::split(notes, cc, cut);
                    (*left, *left_cc, *notes, *cc) = (ln, lc, rn, rc);
                    // The right half of a MIDI take folder starts later in its takes.
                    if let Some(f) = &mut right.takes {
                        f.offset += cut;
                    }
                }
                _ => {}
            }
            r.length = cut;
            t.regions.push(right);
            touched.push(t.id);
        }
        touched.dedup();
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
    }

    fn region_mut(&mut self, id: Id) -> Option<&mut Region> {
        let (track, idx) = self.find_region(id)?;
        self.project.track_mut(track).map(|t| &mut t.regions[idx])
    }

    /// Applies `f` to each region and refreshes the tracks they're on.
    fn edit_regions(&mut self, ids: &[Id], mut f: impl FnMut(&mut Region, &HashMap<Id, Arc<Clip>>)) {
        let mut touched = Vec::new();
        for &id in ids {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let clips = &self.clips;
            let t = self.project.track_mut(track).unwrap();
            f(&mut t.regions[idx], clips);
            if !touched.contains(&track) {
                touched.push(track);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
    }

    fn region(&self, id: Id) -> Option<&Region> {
        let (track, idx) = self.find_region(id)?;
        self.project.track(track).map(|t| &t.regions[idx])
    }

    /// Loops a region up to `loop_end` (a timeline sample), or stops it looping when that is
    /// None or doesn't reach past the region's end (dragging the loop back to the region).
    pub fn set_region_loop(&mut self, id: Id, loop_end: Option<i64>) {
        self.edit_regions(&[id], |r, _| {
            r.loop_end = loop_end;
            if !r.is_looped() {
                r.loop_end = None;
            }
        });
    }

    /// Logic's Loop switch. On, each region loops up to the next region on its track, or for
    /// four times its length when nothing follows it; off removes the loop.
    pub fn set_regions_looped(&mut self, ids: &[Id], on: bool) {
        let mut ends = Vec::new();
        for &id in ids {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let t = self.project.track(track).unwrap();
            let r = &t.regions[idx];
            let own = r.start + r.length;
            let next = t.regions.iter().filter(|o| o.id != id && o.start >= own).map(|o| o.start).min();
            ends.push((id, on.then(|| next.unwrap_or(r.start + 4 * r.length))));
        }
        for (id, end) in ends {
            self.set_region_loop(id, end);
        }
    }

    /// Convert Loops to Real Copies: each looping region becomes one plain region per pass.
    /// The original stays as the first pass; the copies go to the front of the track's list,
    /// beneath every other region, which is where the loop played. Returns the new copies.
    pub fn convert_loops(&mut self, ids: &[Id]) -> Vec<Id> {
        let mut made = Vec::new();
        let mut touched = Vec::new();
        for &id in ids {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let r = self.project.track(track).unwrap().regions[idx].clone();
            if !r.is_looped() {
                continue;
            }
            let mut copies = r.real_copies();
            let first = copies.remove(0);
            for c in &mut copies {
                c.id = self.project.alloc_id();
                made.push(c.id);
            }
            let t = self.project.track_mut(track).unwrap();
            t.regions[idx] = first;
            t.regions.splice(0..0, copies);
            if !touched.contains(&track) {
                touched.push(track);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
        made
    }

    /// Whether `r` can go on `track`: its kind, and for audio a clip that's loaded.
    fn region_fits(&self, track: Id, r: &Region) -> bool {
        match (self.project.track(track).map(|t| t.kind), &r.content) {
            (Some(TrackKind::Audio), RegionContent::Audio { clip, .. }) => self.clips.contains_key(clip),
            (Some(TrackKind::Instrument), RegionContent::Midi { .. }) => true,
            _ => false,
        }
    }

    /// Places copies of `regions` (each with the track it goes on), shifted by `offset`.
    /// Regions that don't fit their track's kind are skipped. Returns the new ids.
    pub fn paste_regions(&mut self, items: &[(Id, Region)], offset: i64) -> Vec<Id> {
        let mut made = Vec::new();
        let mut touched = Vec::new();
        for (track, r) in items {
            if !self.region_fits(*track, r) {
                continue;
            }
            let id = self.project.alloc_id();
            let mut copy = Region { id, ..r.clone() };
            copy.move_to((r.start + offset).max(0));
            self.project.track_mut(*track).unwrap().regions.push(copy);
            made.push(id);
            if !touched.contains(track) {
                touched.push(*track);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
        self.auto_crossfade(&made, Vec::new());
        made
    }

    /// Repeats the selection `times` times straight after itself, as Logic's Repeat Regions.
    pub fn repeat_regions(&mut self, ids: &[Id], times: usize) -> Vec<Id> {
        let items: Vec<(Id, Region)> = ids.iter().filter_map(|id| self.find_region(*id)).map(|(t, i)| (t, self.project.track(t).unwrap().regions[i].clone())).collect();
        let (Some(lo), Some(hi)) = (items.iter().map(|(_, r)| r.start).min(), items.iter().map(|(_, r)| r.end()).max()) else { return Vec::new() };
        let mut made = Vec::new();
        for k in 1..=times.clamp(1, 256) as i64 {
            made.extend(self.paste_regions(&items, (hi - lo) * k));
        }
        made
    }

    /// Sets (`db`) or changes (`delta`) region gain.
    pub fn set_region_gain(&mut self, ids: &[Id], db: Option<f32>, delta: Option<f32>) {
        self.edit_regions(ids, |r, _| {
            if let RegionContent::Audio { gain_db, .. } = &mut r.content {
                let v = db.unwrap_or(*gain_db) + delta.unwrap_or(0.0);
                *gain_db = v.clamp(-96.0, 30.0);
            }
        });
    }

    /// Sets each audio region's gain so its loudest sample reaches 0 dBFS.
    pub fn normalize_regions(&mut self, ids: &[Id]) {
        self.edit_regions(ids, |r, clips| {
            let len = r.length;
            if let RegionContent::Audio { clip, offset, gain_db, .. } = &mut r.content {
                let Some(c) = clips.get(clip) else { return };
                let (a, b) = ((*offset).max(0) as usize, ((*offset + len).max(0) as usize).min(c.frames()));
                let peak = c.channels.iter().flat_map(|ch| ch[a.min(b)..b].iter()).fold(0.0f32, |m, s| m.max(s.abs()));
                if peak > 1e-6 {
                    *gain_db = (-20.0 * peak.log10()).clamp(-96.0, 30.0);
                }
            }
        });
    }

    /// Sets fade lengths (frames); None leaves that end alone.
    pub fn set_region_fades(&mut self, ids: &[Id], fade_in: Option<i64>, fade_out: Option<i64>) {
        self.edit_regions(ids, |r, _| {
            let len = r.length;
            if let RegionContent::Audio { fade_in: fi, fade_out: fo, .. } = &mut r.content {
                if let Some(v) = fade_in {
                    *fi = v.clamp(0, len);
                }
                if let Some(v) = fade_out {
                    *fo = v.clamp(0, len);
                }
                // The two can't overlap.
                if *fi + *fo > len {
                    if fade_in.is_some() {
                        *fo = len - *fi;
                    } else {
                        *fi = len - *fo;
                    }
                }
            }
        });
    }

    pub fn set_region_color(&mut self, ids: &[Id], color: Option<String>) {
        self.edit_regions(ids, |r, _| r.color = color.clone());
    }

    pub fn transpose_regions(&mut self, ids: &[Id], semitones: i32) {
        self.edit_regions(ids, |r, _| {
            if let RegionContent::Midi { notes, .. } = &mut r.content {
                for n in notes.iter_mut() {
                    n.pitch = (n.pitch as i32 + semitones).clamp(0, 127) as u8;
                }
            }
        });
    }

    /// Moves note starts to the nearest `grid` step on the timeline.
    /// Quantizes note starts to `grid` samples at the base tempo (a note value), counted in
    /// musical time from each bar line, so the grid follows tempo and signature changes.
    pub fn quantize_regions(&mut self, ids: &[Id], grid: i64) {
        if grid <= 0 {
            return;
        }
        let map = self.tempo_map();
        let mut step = grid as f64 / (60.0 * self.project.sample_rate as f64 / self.project.tempo);
        // The grid arrives rounded to whole samples; use the note value it stands for.
        let per_whole = 4.0 / step;
        if (per_whole - per_whole.round()).abs() < 0.01 * per_whole {
            step = 4.0 / per_whole.round();
        }
        self.edit_regions(ids, |r, _| {
            let rs = r.start;
            if let RegionContent::Midi { notes, .. } = &mut r.content {
                for n in notes.iter_mut() {
                    let at = (rs + n.start) as f64;
                    let q = map.sample_at(map.snap_q(map.beats_at(at), step)).round() as i64;
                    n.start = (q - rs).max(0);
                }
                notes.sort_by_key(|n| n.start);
            }
        });
    }

    /// An empty MIDI region on an instrument track.
    pub fn create_midi_region(&mut self, track: Id, start: i64, length: i64) -> Result<Id> {
        self.check_unfrozen(&[track])?;
        match self.project.track(track) {
            Some(t) if t.kind == TrackKind::Instrument => {}
            _ => return Err(anyhow!("MIDI regions go on software instrument tracks")),
        }
        let id = self.project.alloc_id();
        let name = self.project.track(track).unwrap().name.clone();
        self.project.track_mut(track).unwrap().regions.push(Region { id, name, start: start.max(0), length: length.max(1), muted: false, color: None, content: RegionContent::Midi { notes: Vec::new(), cc: Vec::new() }, loop_end: None, takes: None, params: Default::default(), pattern: None });
        self.push_regions(track);
        self.dirty = true;
        Ok(id)
    }

    /// Moves regions onto new tracks with their tracks' settings (one per source track).
    pub fn regions_to_new_track(&mut self, ids: &[Id]) -> Vec<Id> {
        let mut by_track: Vec<(Id, Vec<Id>)> = Vec::new();
        for &id in ids {
            let Some((t, _)) = self.find_region(id) else { continue };
            match by_track.iter_mut().find(|(x, _)| *x == t) {
                Some((_, v)) => v.push(id),
                None => by_track.push((t, vec![id])),
            }
        }
        let mut made = Vec::new();
        for (src, regs) in by_track {
            let Some(&dst) = self.duplicate_tracks(&[src], false).first() else { continue };
            for id in regs {
                let start = self.find_region(id).map(|(t, i)| self.project.track(t).unwrap().regions[i].start).unwrap_or(0);
                self.move_region(id, start, Some(dst));
            }
            made.push(dst);
        }
        made
    }

    /// The audio a region plays, rendered with its gain and fades (frames x channels).
    fn render_region(&self, r: &Region, channels: usize) -> Vec<Vec<f32>> {
        if let Some(f) = &r.takes {
            return self.render_comp(r, f, channels);
        }
        let mut out = vec![vec![0.0f32; r.length.max(0) as usize]; channels];
        let RegionContent::Audio { clip, offset, gain_db, fade_in, fade_out, fade_shape } = &r.content else { return out };
        let Some(c) = self.clips.get(clip) else { return out };
        let g = volume_gain(*gain_db);
        let (fi, fo) = (*fade_in, *fade_out);
        let (law_in, law_out) = (FadeLaw::Curve(fade_shape.in_curve), FadeLaw::Curve(fade_shape.out_curve));
        for i in 0..r.length.max(0) {
            let src = offset + i;
            if src < 0 || src as usize >= c.frames() {
                continue;
            }
            let mut gain = g;
            if fi > 0 && i < fi {
                gain *= law_in.gain(i as f32 / fi as f32);
            }
            if fo > 0 && i >= r.length - fo {
                gain *= law_out.gain((r.length - i) as f32 / fo as f32);
            }
            for (ch, o) in out.iter_mut().enumerate() {
                let s = c.channels[ch.min(c.channels.len() - 1)][src as usize];
                o[i as usize] = s * gain;
            }
        }
        out
    }

    fn region_channels(&self, r: &Region) -> usize {
        match &r.content {
            RegionContent::Audio { clip, .. } => self.clips.get(clip).map(|c| c.channels.len().min(2)).unwrap_or(1),
            _ => 0,
        }
    }

    /// Replaces each audio region with a reversed copy of the audio it plays.
    pub fn reverse_regions(&mut self, ids: &[Id]) -> Result<()> {
        let rate = self.sample_rate() as u32;
        for &id in ids {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let r = self.project.track(track).unwrap().regions[idx].clone();
            let RegionContent::Audio { clip, offset, gain_db, fade_in, fade_out, fade_shape } = r.content else { continue };
            // Render without gain and fades, which stay adjustable on the new region.
            let plain = Region { content: RegionContent::Audio { clip, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() }, ..r.clone() };
            let mut audio = self.render_region(&plain, self.region_channels(&r));
            audio.iter_mut().for_each(|c| c.reverse());
            let path = self.unique_audio_path(&format!("{} Reversed", r.name));
            crate::decode::write_wav(&path, rate, &audio)?;
            let clip = self.add_clip(&path, Clip::from_channels(path.clone(), rate, audio));
            if let Some(reg) = self.region_mut(id) {
                reg.content = RegionContent::Audio { clip, offset: 0, gain_db, fade_in: fade_out, fade_out: fade_in, fade_shape: fade_shape.reversed() };
                // A reversed take folder is plain audio.
                reg.takes = None;
            }
            self.push_regions(track);
        }
        self.dirty = true;
        Ok(())
    }

    /// Joins the selected regions on each track into one. MIDI notes are merged; audio is
    /// mixed into a new file, as it plays (later regions win where they overlap). Returns the
    /// new regions.
    pub fn join_regions(&mut self, ids: &[Id]) -> Result<Vec<Id>> {
        // Loops join as the copies they play.
        let looped: Vec<Id> = ids.iter().copied().filter(|id| self.region(*id).map(|r| r.is_looped()).unwrap_or(false)).collect();
        let mut ids = ids.to_vec();
        ids.extend(self.convert_loops(&looped));
        let ids = &ids[..];
        let rate = self.sample_rate() as u32;
        let map = self.tempo_map();
        let mut made = Vec::new();
        let tracks: Vec<Id> = self.project.tracks.iter().map(|t| t.id).collect();
        for track in tracks {
            let t = self.project.track(track).unwrap();
            // Joined as they play, with their playback parameters applied.
            let picked: Vec<Region> = t.regions.iter().filter(|r| ids.contains(&r.id)).map(|r| crate::region_params::bake(r, &map)).collect();
            if picked.len() < 2 {
                continue;
            }
            let lo = picked.iter().map(|r| r.start).min().unwrap();
            let hi = picked.iter().map(|r| r.end()).max().unwrap();
            let name = picked.iter().min_by_key(|r| r.start).unwrap().name.clone();
            let content = if t.kind == TrackKind::Instrument {
                let parts = picked.iter().filter_map(|r| match &r.content {
                    RegionContent::Midi { notes, cc } => Some((r.start - lo, notes.as_slice(), cc.as_slice())),
                    _ => None,
                });
                let (notes, cc) = crate::midi::join(parts);
                RegionContent::Midi { notes, cc }
            } else {
                let ch = picked.iter().map(|r| self.region_channels(r)).max().unwrap_or(1).max(1);
                let mix = self.mix_regions(t, &picked, lo, hi, ch);
                let path = self.unique_audio_path(&format!("{name} Joined"));
                crate::decode::write_wav(&path, rate, &mix)?;
                let clip = self.add_clip(&path, Clip::from_channels(path.clone(), rate, mix));
                RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() }
            };
            let id = self.project.alloc_id();
            let color = picked[0].color.clone();
            let t = self.project.track_mut(track).unwrap();
            t.regions.retain(|r| !ids.contains(&r.id));
            t.regions.push(Region { id, name, start: lo, length: hi - lo, muted: false, color, content, loop_end: None, takes: None, params: Default::default(), pattern: None });
            self.push_regions(track);
            made.push(id);
        }
        self.dirty = true;
        Ok(made)
    }

    /// Decodes a file for the project. A WAV already at the project rate is referenced where it
    /// is; anything else (other formats or rates) is converted into the project's Audio Files.
    fn prepare_import(&self, path: &Path) -> Result<(PathBuf, Clip)> {
        let d = crate::decode::decode(path)?;
        let rate = self.sample_rate() as u32;
        if d.is_wav && d.sample_rate == rate {
            return Ok((path.to_path_buf(), Clip::from_channels(path.to_path_buf(), rate, d.channels)));
        }
        let channels = crate::decode::resample(d.channels, d.sample_rate, rate)?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Import".into());
        let dest = self.unique_audio_path(&stem);
        crate::decode::write_wav(&dest, rate, &channels)?;
        Ok((dest.clone(), Clip::from_channels(dest, rate, channels)))
    }

    /// An unused file name in the project's Audio Files folder.
    fn unique_audio_path(&self, stem: &str) -> PathBuf {
        let dir = self.dir.join(AUDIO_DIR);
        let mut dest = dir.join(format!("{stem}.wav"));
        for i in 2.. {
            if !dest.exists() {
                break;
            }
            dest = dir.join(format!("{stem} {i}.wav"));
        }
        dest
    }

    fn add_clip(&mut self, path: &Path, clip: Clip) -> Id {
        let id = self.project.alloc_id();
        let rel = path.strip_prefix(&self.dir).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| path.to_string_lossy().to_string());
        self.clips.insert(id, Arc::new(clip));
        self.project.clips.push(ClipRef { id, path: rel });
        id
    }

    pub fn import_audio(&mut self, path: &Path, track: Id, start: i64) -> Result<Id> {
        self.check_unfrozen(&[track])?;
        match self.project.track(track) {
            Some(t) if t.kind == TrackKind::Audio => {}
            Some(_) => return Err(anyhow!("audio files can only go on audio tracks")),
            None => return Err(anyhow!("no such track")),
        }
        let (stored, clip) = self.prepare_import(path)?;
        let frames = clip.frames() as i64;
        let clip_id = self.add_clip(&stored, clip);
        let id = self.project.alloc_id();
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let fade = self.default_fade(frames);
        let t = self.project.track_mut(track).unwrap();
        t.regions.push(Region { id, name, start: start.max(0), length: frames, muted: false, color: None, content: RegionContent::Audio { clip: clip_id, offset: 0, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() }, loop_end: None, takes: None, params: Default::default(), pattern: None });
        self.push_regions(track);
        self.dirty = true;
        Ok(id)
    }

    /// Imports files dropped onto the project. With `track` set (an audio track), the first file
    /// lands on it and the rest on new tracks below; otherwise every file gets a new track,
    /// named after the file, inserted at `index` (the end when None). Returns per-file errors.
    pub fn import_audio_files(&mut self, paths: &[PathBuf], track: Option<Id>, start: i64, index: Option<usize>) -> Vec<String> {
        let mut errors = Vec::new();
        let mut files = paths.iter().filter(|p| crate::decode::is_audio_file(p) || p.is_file());
        let target = track.filter(|id| self.project.track(*id).map(|t| t.kind == TrackKind::Audio).unwrap_or(false));
        let mut next_index = match (target, index) {
            (Some(id), _) => self.project.tracks.iter().position(|t| t.id == id).map(|i| i + 1).unwrap_or(self.project.tracks.len()),
            (None, Some(i)) => i.min(self.project.tracks.len()),
            (None, None) => self.project.tracks.len(),
        };
        if let Some(id) = target {
            if let Some(first) = files.next() {
                if let Err(e) = self.import_audio(first, id, start) {
                    errors.push(format!("{}: {e}", first.display()));
                }
            }
        }
        for path in files {
            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Audio".into());
            match self.prepare_import(path) {
                Ok((stored, clip)) => {
                    let input = if clip.channels.len() > 1 { InputSel::Stereo(0) } else { InputSel::Mono(0) };
                    let frames = clip.frames() as i64;
                    let id = match self.add_track(TrackKind::Audio, input, None, Some(next_index)) {
                        Ok(id) => id,
                        Err(e) => {
                            errors.push(format!("{}: {e}", path.display()));
                            continue;
                        }
                    };
                    next_index += 1;
                    let clip_id = self.add_clip(&stored, clip);
                    let rid = self.project.alloc_id();
                    let fade = self.default_fade(frames);
                    let t = self.project.track_mut(id).unwrap();
                    t.name = name.clone();
                    t.regions.push(Region { id: rid, name, start: start.max(0), length: frames, muted: false, color: None, content: RegionContent::Audio { clip: clip_id, offset: 0, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() }, loop_end: None, takes: None, params: Default::default(), pattern: None });
                    self.push_regions(id);
                    self.select_track(Some(id));
                }
                Err(e) => errors.push(format!("{}: {e}", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
            }
        }
        self.dirty = true;
        self.request_autosave();
        errors
    }

    /// The clip's absolute peak level.
    pub fn clip_peak(&self, id: Id) -> Option<f32> {
        self.clips.get(&id).map(|c| c.channels.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs())))
    }

    /// See [`Clip::waveform`].
    pub fn clip_waveform(&self, id: Id) -> Option<Vec<u8>> {
        self.clips.get(&id).map(|c| c.waveform())
    }

    // ---- plug-ins ---------------------------------------------------------------------------

    fn slot_ref(&mut self, track: Id, slot: usize) -> Option<&mut Option<PluginSlot>> {
        if track == MASTER_ID {
            return self.project.master.inserts.get_mut(slot);
        }
        let t = self.project.track_mut(track)?;
        if slot == usize::MAX {
            Some(&mut t.instrument)
        } else {
            t.inserts.get_mut(slot)
        }
    }

    /// Loads a plug-in into an insert slot, or the instrument slot when `slot == usize::MAX`.
    pub fn set_plugin(&mut self, track: Id, slot: usize, plugin_id: &str) -> Result<Id> {
        self.check_unfrozen(&[track])?;
        let builtin = crate::fx::find(plugin_id);
        let instrument = crate::instrument::find(plugin_id);
        let name = if let Some(info) = &instrument {
            if slot != usize::MAX {
                return Err(anyhow!("{} is an instrument: it goes in the instrument slot", info.name));
            }
            info.name.to_string()
        } else if let Some(info) = &builtin {
            if slot == usize::MAX {
                return Err(anyhow!("{} is an audio effect: it goes in an Audio FX slot", info.name));
            }
            info.name.to_string()
        } else {
            self.find_plugin(plugin_id).map(|d| d.name.clone()).ok_or_else(|| anyhow!("unknown plug-in"))?
        };
        // A built-in plug-in's values are all written out, so the project file reads clearly.
        let specs = builtin.map(|i| i.params).or(instrument.map(|i| i.params)).unwrap_or_default();
        let params = specs.iter().map(|p| (p.id, p.default)).collect();
        let uid = self.project.alloc_id();
        let new_slot = PluginSlot { uid, plugin_id: plugin_id.to_string(), name, bypass: false, state: None, params, sidechain: None, sidechain_pre: false };
        let ins = self.instantiate_slot(&new_slot)?;
        let old = self.slot_ref(track, slot).ok_or_else(|| anyhow!("no such slot"))?.replace(new_slot);
        if let Some(o) = old {
            self.controllers.remove(&o.uid);
        }
        if slot == usize::MAX {
            self.send(Cmd::Instrument { track, insert: Some(ins) });
        } else {
            self.send(Cmd::Insert { track, slot, insert: Some(ins) });
        }
        self.update_pdc();
        self.dirty = true;
        Ok(uid)
    }

    pub fn remove_plugin(&mut self, track: Id, slot: usize) {
        if self.is_frozen(track) {
            return;
        }
        let old = self.slot_ref(track, slot).and_then(|s| s.take());
        if let Some(o) = old {
            if let Some(mut c) = self.controllers.remove(&o.uid) {
                c.close_editor();
            }
        }
        if slot == usize::MAX {
            self.send(Cmd::Instrument { track, insert: None });
        } else {
            self.send(Cmd::Insert { track, slot, insert: None });
        }
        self.update_pdc();
        self.dirty = true;
    }

    pub fn set_bypass(&mut self, track: Id, slot: usize, bypass: bool) {
        if self.is_frozen(track) {
            return;
        }
        let Some(Some(s)) = self.slot_ref(track, slot) else { return };
        s.bypass = bypass;
        let uid = s.uid;
        self.send(Cmd::Bypass { track, uid, bypass });
        self.update_pdc();
    }

    pub fn open_editor(&mut self, uid: Id, title: &str) -> Result<()> {
        let c = self.controllers.get_mut(&uid).ok_or_else(|| anyhow!("this plug-in has no editor"))?;
        c.open_editor(title).map_err(|e| anyhow!(e))?;
        self.watch_vst_edits(uid, false);
        Ok(())
    }

    pub fn open_editors(&self) -> Vec<Id> {
        self.controllers.iter().filter(|(_, c)| c.editor_open()).map(|(&id, _)| id).collect()
    }

    pub fn set_plugins(&mut self, list: Vec<PluginDescriptor>) {
        self.plugins = list;
    }

    /// The installed plug-in a slot's id names. Ids start with the bundle's path, so when that
    /// path isn't installed (a project from another computer or operating system, or a moved
    /// bundle), the same class (`cid`) installed elsewhere stands in.
    pub fn find_plugin(&self, id: &str) -> Option<&PluginDescriptor> {
        self.plugins.iter().find(|d| d.id == id).or_else(|| {
            let (_, cid) = id.rsplit_once('|')?;
            self.plugins.iter().find(|d| d.cid.eq_ignore_ascii_case(cid))
        })
    }

    /// VST3 slots that aren't running: the plug-in isn't installed here (or failed to load).
    /// They stay in the project with their saved state, so a project opened where a plug-in is
    /// missing keeps its settings when saved, for the computer that has it.
    pub fn missing_plugins(&self) -> Vec<&PluginSlot> {
        let p = &self.project;
        let tracks = p.tracks.iter().filter(|t| t.frozen.is_none()).flat_map(|t| t.instrument.iter().chain(t.inserts.iter().flatten()));
        tracks
            .chain(p.master.inserts.iter().flatten())
            .filter(|s| !builtin::is_builtin(&s.plugin_id) && !self.controllers.contains_key(&s.uid))
            .collect()
    }

    // ---- periodic work ----------------------------------------------------------------------

    /// Main-thread housekeeping: plugin controller idle, freeing replaced DSP objects, keeping
    /// MIDI inputs connected and automation (plug-in editors following it, writes from them).
    /// Returns true if the set of open editors changed or automation was written, when the UI
    /// needs a new snapshot.
    pub fn idle(&mut self) -> bool {
        self.sync_parallel();
        let before = self.open_editors().len();
        let mut latency_changed = false;
        for c in self.controllers.values_mut() {
            c.idle();
            latency_changed |= c.take_latency_changed();
        }
        latency_changed |= self.builtin_latency_changed();
        if latency_changed {
            self.update_pdc();
        }
        while let Ok(g) = self.garbage_rx.pop() {
            self.sampler_garbage(g);
        }
        self.sampler_idle();
        let written = self.auto_idle();
        let stepped = self.vst_undo_idle();
        let controlled = self.controller_idle();
        before != self.open_editors().len() || written || latency_changed || controlled || stepped
    }

    /// Plays a MIDI message on the current MIDI target track (the selected instrument track).
    pub fn send_midi(&self, data: [u8; 3]) {
        self.midi.inject(data);
    }

    /// Name of the track live MIDI currently plays, if any.
    pub fn midi_target_name(&self) -> Option<String> {
        self.midi_target_id().and_then(|id| self.project.track(id)).map(|t| t.name.clone())
    }

    pub fn refresh_midi(&mut self) {
        self.midi.refresh();
    }

    pub fn midi_inputs(&self) -> Vec<String> {
        self.midi.connected()
    }

    pub fn tick(&mut self) -> Tick {
        self.check_overload();
        // Keep the newest FFT_SIZE analyzer samples.
        let fresh = self.analyzer_rx.slots();
        let got = fresh > 0;
        if let Ok(chunk) = self.analyzer_rx.read_chunk(fresh) {
            let n = self.analyzer_buf.len();
            let (a, b) = chunk.as_slices();
            let incoming: Vec<f32> = a.iter().chain(b.iter()).copied().collect();
            chunk.commit_all();
            if incoming.len() >= n {
                self.analyzer_buf.copy_from_slice(&incoming[incoming.len() - n..]);
            } else {
                self.analyzer_buf.rotate_left(incoming.len());
                self.analyzer_buf[n - incoming.len()..].copy_from_slice(&incoming);
            }
        }
        let spectrum = match (self.analyzer_target, got) {
            (Some(id), true) => Some((id, crate::eq::spectrum(&self.analyzer_buf, self.sample_rate()))),
            _ => None,
        };
        let meters: Vec<(Id, f32, f32)> = self.meters.iter().map(|(&id, m)| {
            let (l, r) = m.take();
            (id, l, r)
        }).collect();
        let master = self.status.master.take();
        let peak_holds = self.hold_peaks(&meters, master);
        let n_in = self.audio_status.input_channels as usize;
        Tick {
            pos: self.position(),
            playing: self.is_playing(),
            recording: self.status.recording.load(Ordering::Relaxed),
            cpu: f32::from_bits(self.status.cpu_load.load(Ordering::Relaxed)),
            cpu_peak: f32::from_bits(self.status.cpu_peak.load(Ordering::Relaxed)),
            late_blocks: self.status.late_blocks.load(Ordering::Relaxed),
            threads: self.status.threads.load(Ordering::Relaxed),
            meters,
            master,
            inputs: self.status.inputs[..n_in.min(MAX_INPUTS)].iter().map(|m| m.take().0).collect(),
            previews: self.recorder.previews(),
            midi_activity: self.status.midi_activity.load(Ordering::Relaxed),
            overruns: self.status.overruns.load(Ordering::Relaxed),
            latency: self.latency(),
            spectrum,
            warning: self.warning.take().or_else(|| self.recorder.take_errors()).or_else(|| self.engine_error_warning()).or_else(|| self.bad_audio_warning()),
            fx: self.fx_ticks(),
            peak_holds,
            gain_reduction: self.gain_reductions(),
            learn: self.learning(),
            learned: self.take_learned(),
            midi_in: self.midi_since_last_tick(),
        }
    }

    /// What hardware MIDI inputs sent since the last call, from the activity log.
    fn midi_since_last_tick(&mut self) -> Vec<[u8; 3]> {
        let log = self.midi.recent();
        let fresh: Vec<[u8; 3]> = log.iter().filter(|e| e.seq >= self.midi_seen).map(|e| e.data).collect();
        if let Some(last) = log.last() {
            self.midi_seen = last.seq + 1;
        }
        fresh
    }

    /// A warning when the audio callback had to silence blocks because the engine panicked
    /// (the panic message itself goes to the log). Reported at most every few seconds.
    fn engine_error_warning(&mut self) -> Option<String> {
        let n = self.status.engine_errors.load(Ordering::Relaxed);
        if n == self.engine_errors_seen || self.engine_error_at.elapsed() < Duration::from_secs(5) {
            return None;
        }
        self.engine_errors_seen = n;
        self.engine_error_at = Instant::now();
        Some("The audio engine hit an internal error and skipped some audio. Please report it, with the terminal output.".into())
    }

    /// Like Logic's System Overload: when the engine had to skip audio to keep up, playback
    /// (and recording) stops and the user hears why.
    fn check_overload(&mut self) {
        let n = self.status.overloads.load(Ordering::Relaxed);
        if n == self.overloads_seen {
            return;
        }
        self.overloads_seen = n;
        let was_running = self.is_playing() || self.is_recording();
        if was_running {
            self.stop();
        }
        let what = if was_running { "so playback stopped" } else { "so it skipped a moment of audio" };
        let msg = format!("System overload: the audio engine couldn't keep up, {what}. Try a larger buffer size in Settings › Audio, or bypass or freeze heavy plug-ins.");
        self.warning.get_or_insert(msg);
    }

    /// Names the plug-in the engine caught putting out NaNs or a runaway level (at most every
    /// 5 seconds while it keeps doing it).
    fn bad_audio_warning(&mut self) -> Option<String> {
        let n = self.status.bad_audio.load(Ordering::Relaxed);
        if n == self.bad_audio_seen || self.bad_audio_at.elapsed() < Duration::from_secs(5) {
            return None;
        }
        self.bad_audio_seen = n;
        self.bad_audio_at = Instant::now();
        let uid = self.status.bad_audio_uid.load(Ordering::Relaxed);
        let p = &self.project;
        let found = p.tracks.iter().find_map(|t| t.inserts.iter().flatten().chain(t.instrument.iter()).find(|s| s.uid == uid).map(|s| (s.name.clone(), t.name.clone())));
        let found = found.or_else(|| p.master.inserts.iter().flatten().find(|s| s.uid == uid).map(|s| (s.name.clone(), "Stereo Out".to_string())));
        let (plugin, track) = found.unwrap_or_else(|| ("A plug-in".into(), "a track".into()));
        Some(format!("{plugin} on {track} put out invalid audio (NaN or a runaway level), so Fermata muted it to protect the rest of the mix. If it keeps happening, bypass it or reset its settings."))
    }

    // ---- files ------------------------------------------------------------------------------

    pub fn project_path(&self) -> PathBuf {
        self.dir.clone()
    }

    pub fn new_project(&mut self) {
        if self.rec_punch.is_some() {
            self.stop();
        }
        self.sampler_record_cancel();
        self.send(Cmd::Stop);
        self.controllers.clear();
        self.vst_watches.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.auto = Default::default();
        self.project = Project::new("Untitled", self.sample_rate() as u32);
        self.project.click = self.prefs.click;
        self.dir = untitled_dir(&self.projects_root());
        self.saved = false;
        self.dirty = false;
        self.forget_unsaved();
        self.clips.clear();
        self.selected = None;
        self.locate(0);
        self.rebuild_graph();
        self.sampler.pool.forget_unused();
    }

    /// Forgets every undo and redo step. The Project Chooser builds a new project's template
    /// out of ordinary edits, which shouldn't be undoable back to an empty project.
    pub fn clear_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Saves into `dir` (a `.fermata` folder). Saving somewhere new copies the audio files.
    pub fn save(&mut self, dir: Option<PathBuf>) -> Result<PathBuf> {
        let target = dir.unwrap_or_else(|| self.dir.clone());
        let target = if target.extension().map(|e| e == "fermata").unwrap_or(false) { target } else { target.with_extension("fermata") };
        std::fs::create_dir_all(target.join(AUDIO_DIR))?;
        self.ensure_freeze_files();
        if target != self.dir {
            // Every file the project uses ends up inside the new folder: its own files keep
            // their subfolder (Audio Files, Freeze Files), files from elsewhere (a custom
            // Recording Folder, say) go into Audio Files, and a name already taken gets a number.
            let mut taken = std::collections::HashSet::new();
            let mut copied: HashMap<PathBuf, String> = HashMap::new();
            for c in &mut self.project.clips {
                let src = if Path::new(&c.path).is_absolute() { PathBuf::from(&c.path) } else { self.dir.join(&c.path) };
                if src.starts_with(&target) {
                    continue;
                }
                // Clips sharing a file share the copy.
                if let Some(rel) = copied.get(&src) {
                    c.path = rel.clone();
                    continue;
                }
                let folder = match src.strip_prefix(&self.dir).ok().and_then(|rel| rel.parent()) {
                    Some(sub) if !sub.as_os_str().is_empty() => sub.to_path_buf(),
                    _ => PathBuf::from(AUDIO_DIR),
                };
                let rel = unique_name(&target, &folder, &src, &mut taken);
                if src.exists() {
                    std::fs::create_dir_all(target.join(&folder))?;
                    std::fs::copy(&src, target.join(&rel)).with_context(|| format!("copying {}", src.display()))?;
                }
                c.path = rel.to_string_lossy().to_string();
                copied.insert(src, c.path.clone());
            }
            self.sampler_copy_samples(&target)?;
        }
        self.project.name = target.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
        // The first alternative is named after the project, and keeps that name (alternatives.rs).
        if self.project.alternative.is_empty() {
            self.project.alternative = self.project.name.clone();
        }
        let doc = self.capture_plugin_states();
        self.write_project(&target, serde_json::to_string_pretty(&doc)?.as_bytes())?;
        self.saved_doc(&doc);
        // The saved file supersedes any autosave, here and where the project used to be.
        let _ = std::fs::remove_file(target.join(AUTOSAVE_FILE));
        let _ = std::fs::remove_file(self.dir.join(AUTOSAVE_FILE));
        self.autosave_hash = 0;
        self.dir = target.clone();
        self.saved = true;
        self.dirty = false;
        Ok(target)
    }

    /// Opens a project. A missing or damaged project.json falls back to the autosave, then to
    /// the backup of the previous save, with a warning (see `session/recovery.rs`).
    pub fn open(&mut self, dir: &Path) -> Result<()> {
        self.open_checked(dir)
    }

    /// Opens the project document `file` inside the project folder `dir`.
    fn open_from(&mut self, dir: &Path, file: &str) -> Result<()> {
        let dir = if dir.is_file() { dir.parent().unwrap_or(dir).to_path_buf() } else { dir.to_path_buf() };
        let text = std::fs::read_to_string(dir.join(file)).with_context(|| format!("reading {}", dir.display()))?;
        let project: Project = serde_json::from_str(&text)?;
        if self.rec_punch.is_some() {
            self.stop();
        }
        self.sampler_record_cancel();
        self.send(Cmd::Stop);
        // Instance ids restart in every project, so old controllers must not leak state into
        // the new one.
        self.controllers.clear();
        self.vst_watches.clear();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.auto = Default::default();
        self.forget_unsaved();
        let mut clips = HashMap::new();
        for c in &project.clips {
            let p = if Path::new(&c.path).is_absolute() { PathBuf::from(&c.path) } else { dir.join(&c.path) };
            match Clip::load(&p) {
                Ok(clip) => {
                    clips.insert(c.id, Arc::new(clip));
                }
                Err(e) => log::error!("missing audio file {}: {e}", p.display()),
            }
        }
        let project_rate = project.sample_rate;
        self.clips = clips;
        self.project = project;
        self.dir = dir;
        self.saved = true;
        self.dirty = false;
        self.selected = self.project.tracks.first().map(|t| t.id);
        self.locate(0);
        if project_rate != self.audio_status.sample_rate && self.audio_config.sample_rate != project_rate {
            // Switch the device to the project's rate, as Logic does.
            self.audio_config.sample_rate = project_rate;
            self.restart_audio();
        }
        self.project.sample_rate = self.sample_rate() as u32;
        self.project.prune_routes();
        self.rebuild_graph();
        let mut missing: Vec<&str> = self.missing_plugins().iter().map(|s| s.name.as_str()).collect();
        missing.sort_unstable();
        missing.dedup();
        if !missing.is_empty() {
            self.warning = Some(format!("Not installed: {}. Their settings are kept with the project.", missing.join(", ")));
        }
        self.sampler.pool.forget_unused();
        self.sampler_warn_missing();
        Ok(())
    }
}
