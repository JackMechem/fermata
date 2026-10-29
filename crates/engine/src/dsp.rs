//! The real-time mixing graph. Everything in `Engine::process` runs on the audio thread and must
//! not block or allocate; structural changes arrive as `Cmd`s and replaced objects are shipped
//! back as `Garbage` to be dropped elsewhere.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use std::cell::RefCell;
use vst3_host::{MidiEvent, PluginProcessor, TransportInfo};

use crate::automation::{value_at, AutoTarget, LaneDsp};
use crate::clip::Clip;
use crate::eq::{EqCoeffs, EqState};
use crate::fades::FadeLaw;
use crate::model::{AutoMode, CcEvent, CcKind, Id, InputSel, Note, TrackKind, CC_KINDS, INSERT_SLOTS, MASTER_ID, SEND_SLOTS};
use crate::prefs::{ClickSettings, ClickSound};
use crate::synth::BasicSynth;
use crate::tempo::{GridPoint, TempoMap};

mod offline;
pub use offline::Isolate;
use offline::Role;
mod pdc;
pub use pdc::{DelayLine, PdcGlobal, PdcParams, INSTRUMENT_BIT};
mod builtin;
pub use builtin::{clamp as builtin_clamp, to_normalized, to_plain, BuiltinDsp, FxShared};

pub const MAX_BLOCK: usize = 1024;
pub const MAX_INPUTS: usize = 32;
const MAX_MIDI: usize = 512;
/// Parameters a track can have held (touched) at once while writing automation.
const MAX_TOUCHED: usize = 16;
/// Plug-in lanes per track whose last sent value is remembered (later ones resend every block).
const MAX_AUTO_SENT: usize = 64;

#[derive(Default)]
pub struct Meter {
    l: AtomicU32,
    r: AtomicU32,
}

impl Meter {
    /// Non-negative floats order the same as their bit patterns, so fetch_max works directly.
    pub fn feed(&self, l: f32, r: f32) {
        self.l.fetch_max(l.abs().to_bits(), Ordering::Relaxed);
        self.r.fetch_max(r.abs().to_bits(), Ordering::Relaxed);
    }

    /// Peak since the last call.
    pub fn take(&self) -> (f32, f32) {
        (
            f32::from_bits(self.l.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.r.swap(0, Ordering::Relaxed)),
        )
    }
}

pub struct EngineStatus {
    pub pos: AtomicI64,
    pub playing: AtomicBool,
    pub recording: AtomicBool,
    pub master: Meter,
    pub inputs: Vec<Meter>,
    pub cpu_load: AtomicU32,
    pub cmds_done: AtomicU64,
    pub midi_activity: AtomicU32,
    pub overruns: AtomicU32,
    /// Blocks in which `Engine::process` panicked; the audio callback silenced them (see
    /// `audio_io::build_output`).
    pub engine_errors: AtomicU32,
    /// Measured device latencies (µs) and how many input frames sit in the duplex ring.
    pub input_latency_us: AtomicU64,
    pub output_latency_us: AtomicU64,
    pub ring_frames: AtomicU64,
    /// Blocks in which a plug-in put out NaNs, infinities or a runaway level (see `contain`),
    /// and the uid of the last one that did.
    pub bad_audio: AtomicU32,
    pub bad_audio_uid: AtomicU64,
    /// Times the engine fell so far behind that it skipped processing for a moment (see
    /// `Engine::process`).
    pub overloads: AtomicU32,
}

impl Default for EngineStatus {
    fn default() -> Self {
        EngineStatus {
            pos: AtomicI64::new(0),
            playing: AtomicBool::new(false),
            recording: AtomicBool::new(false),
            master: Meter::default(),
            inputs: (0..MAX_INPUTS).map(|_| Meter::default()).collect(),
            cpu_load: AtomicU32::new(0),
            cmds_done: AtomicU64::new(0),
            midi_activity: AtomicU32::new(0),
            overruns: AtomicU32::new(0),
            engine_errors: AtomicU32::new(0),
            input_latency_us: AtomicU64::new(0),
            output_latency_us: AtomicU64::new(0),
            ring_frames: AtomicU64::new(0),
            bad_audio: AtomicU32::new(0),
            bad_audio_uid: AtomicU64::new(0),
            overloads: AtomicU32::new(0),
        }
    }
}

pub enum Plugin {
    Vst(Box<PluginProcessor>),
    Synth(Box<BasicSynth>),
    /// A built-in effect (see `crate::fx`).
    Builtin(Box<BuiltinDsp>),
}

/// The loudest sample a plug-in may hand on, +60 dBFS: anything past it is a plug-in blowing up.
const RUNAWAY: f32 = 1000.0;

/// Silences NaNs and clamps infinite or runaway samples, so one unstable plug-in can't poison
/// everything after it (a NaN never leaves a filter's state, and huge values overflow
/// detectors into NaN) or reach the device. Returns whether anything had to be fixed.
fn contain(l: &mut [f32], r: &mut [f32]) -> bool {
    let mut bad = false;
    for s in l.iter_mut().chain(r.iter_mut()) {
        // NaN fails the comparison too.
        if !(s.abs() <= RUNAWAY) {
            bad = true;
            *s = if s.is_nan() { 0.0 } else { s.clamp(-RUNAWAY, RUNAWAY) };
        }
    }
    bad
}

/// Contains what a plug-in just produced, and reports it. A built-in effect that went
/// unstable is also cleared, so it recovers.
fn check_output(status: &EngineStatus, ins: &mut InsertDsp, l: &mut [f32], r: &mut [f32]) {
    if contain(l, r) {
        status.bad_audio_uid.store(ins.uid, Ordering::Relaxed);
        status.bad_audio.fetch_add(1, Ordering::Relaxed);
        if let Plugin::Builtin(b) = &mut ins.plugin {
            b.reset();
        }
    }
}

/// How long the callbacks may run late back to back before the engine sheds load. Past about
/// 200 ms the kernel kills a real-time thread that never sleeps (RLIMIT_RTTIME).
const OVERLOAD_SECS: f64 = 0.06;
/// How long it then outputs silence without processing, so the thread catches up and sleeps.
const SHED_SECS: f64 = 0.25;

/// What a built-in effect is told about the transport.
fn fx_context(t: &TransportInfo) -> crate::fx::FxContext {
    crate::fx::FxContext { sample_rate: t.sample_rate as f32, tempo: t.tempo, playing: t.playing }
}

impl Plugin {
    fn process(&mut self, l: &mut [f32], r: &mut [f32], midi: &[MidiEvent], t: &TransportInfo) {
        match self {
            Plugin::Vst(p) => p.process(l, r, midi, t),
            Plugin::Synth(s) => s.process(l, r, midi),
            Plugin::Builtin(b) => b.process(l, r, None, &fx_context(t)),
        }
    }

    /// Queues an automation value (normalized 0..1) for the next block (the built-in synth has
    /// no parameters). A built-in effect takes the block's first value and glides on its own.
    fn set_param(&mut self, id: u32, offset: i32, value: f64) {
        match self {
            Plugin::Vst(p) => p.queue_param(id, offset, value),
            Plugin::Builtin(b) if offset == 0 => b.set_normalized(id, value as f32),
            _ => {}
        }
    }
}

pub struct InsertDsp {
    pub uid: Id,
    pub bypass: bool,
    pub plugin: Plugin,
}

pub enum RegionDsp {
    Audio {
        start: i64,
        length: i64,
        offset: i64,
        gain: f32,
        clip: Arc<Clip>,
        /// Timeline (start, length) of the region's fade-in and fade-out; length 0 = none.
        fade_in: (i64, i64),
        fade_out: (i64, i64),
        /// The gain laws of the fade-in and fade-out (see `fades.rs`).
        laws: [FadeLaw; 2],
    },
    Midi { start: i64, length: i64, notes: Arc<Vec<Note>>, cc: Arc<Vec<CcEvent>> },
}

#[derive(Debug, Clone, Copy)]
pub struct TrackParams {
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    pub monitor: bool,
    pub input: InputSel,
    /// The summing stack or aux this track plays through (None = Stereo Out).
    pub output: Option<Id>,
    /// Per send slot: the aux it feeds and at what gain.
    pub sends: [Option<SendParams>; SEND_SLOTS],
    /// The part of `volume` and `mute` that comes from the folder stacks the track is in, which
    /// still applies when automation drives the track's own volume and mute.
    pub stack_gain: f32,
    pub stack_mute: bool,
}

/// A send as the engine plays it. A muted send keeps its slot at gain 0, so it fades out
/// instead of clicking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SendParams {
    pub bus: Id,
    pub gain: f32,
    pub pre_fader: bool,
}

pub struct TrackDsp {
    pub id: Id,
    pub kind: TrackKind,
    pub params: TrackParams,
    pub meter: Arc<Meter>,
    regions: Arc<Vec<RegionDsp>>,
    inserts: Vec<Option<InsertDsp>>,
    instrument: Option<InsertDsp>,
    gains: (f32, f32),
    l: Vec<f32>,
    r: Vec<f32>,
    /// What subtracks sent this block, for summing stacks.
    sum_l: Vec<f32>,
    sum_r: Vec<f32>,
    /// Index in the engine's list of the track `params.output` names.
    out_idx: Option<usize>,
    midi: Vec<MidiEvent>,
    rec: Option<rtrb::Producer<f32>>,
    sounding: [bool; 128],
    flush_notes: bool,
    eq: EqCoeffs,
    eq_state: EqState,
    /// Index in the engine's list of each send's aux, kept up to date by `resolve_routes()`.
    send_idx: [Option<usize>; SEND_SLOTS],
    /// Smoothed send gains, like `gains` for the fader.
    send_gains: [f32; SEND_SLOTS],
    automation: Arc<Vec<LaneDsp>>,
    auto_mode: AutoMode,
    /// Parameters held by the user while writing in Touch or Latch: their live value wins.
    touched: [Option<AutoTarget>; MAX_TOUCHED],
    /// The value last queued for each plug-in lane, so unchanged values aren't resent.
    auto_sent: [f32; MAX_AUTO_SENT],
    /// Send the controller values in effect at the playhead before the next block plays, so a
    /// region started in the middle sounds as it would from its start.
    chase_cc: bool,
    /// Controllers were played since the last flush, so the flush resets sustain and bend.
    cc_sent: bool,
    /// Plug-in delay compensation (see `pdc.rs`).
    pdc: PdcParams,
    delay: Option<Box<DelayLine>>,
    /// Another strip's built-in effect is keyed by this track: its post-fader output is kept
    /// here for the block (set by `resolve_routes`).
    keyed: bool,
    key_l: Vec<f32>,
    key_r: Vec<f32>,
}

impl TrackDsp {
    pub fn new(id: Id, kind: TrackKind, params: TrackParams, meter: Arc<Meter>) -> TrackDsp {
        TrackDsp {
            id,
            kind,
            params,
            meter,
            regions: Arc::new(Vec::new()),
            inserts: (0..INSERT_SLOTS).map(|_| None).collect(),
            instrument: None,
            gains: (0.0, 0.0),
            l: vec![0.0; MAX_BLOCK],
            r: vec![0.0; MAX_BLOCK],
            sum_l: vec![0.0; MAX_BLOCK],
            sum_r: vec![0.0; MAX_BLOCK],
            out_idx: None,
            midi: Vec::with_capacity(MAX_MIDI),
            rec: None,
            sounding: [false; 128],
            flush_notes: false,
            eq: EqCoeffs::default(),
            eq_state: EqState::default(),
            send_idx: [None; SEND_SLOTS],
            send_gains: [0.0; SEND_SLOTS],
            automation: Arc::new(Vec::new()),
            auto_mode: AutoMode::Read,
            touched: [None; MAX_TOUCHED],
            auto_sent: [f32::NAN; MAX_AUTO_SENT],
            chase_cc: true,
            cc_sent: false,
            pdc: PdcParams::default(),
            delay: None,
            keyed: false,
            key_l: vec![0.0; MAX_BLOCK],
            key_r: vec![0.0; MAX_BLOCK],
        }
    }

    pub fn set_automation(&mut self, lanes: Arc<Vec<LaneDsp>>, mode: AutoMode) {
        self.automation = lanes;
        self.auto_mode = mode;
        self.auto_sent = [f32::NAN; MAX_AUTO_SENT];
    }

    fn set_touched(&mut self, target: AutoTarget, on: bool) {
        let at = self.touched.iter().position(|t| *t == Some(target));
        match (on, at) {
            (true, None) => {
                if let Some(free) = self.touched.iter_mut().find(|t| t.is_none()) {
                    *free = Some(target);
                }
            }
            (false, Some(i)) => self.touched[i] = None,
            _ => {}
        }
        self.auto_sent = [f32::NAN; MAX_AUTO_SENT];
    }

    pub fn set_eq(&mut self, c: EqCoeffs) {
        if !c.active {
            self.eq_state.reset();
        }
        self.eq = c;
    }

    pub fn set_regions(&mut self, r: Arc<Vec<RegionDsp>>) {
        self.regions = r;
    }

    pub fn set_insert(&mut self, slot: usize, ins: Option<InsertDsp>) {
        if slot < self.inserts.len() {
            self.inserts[slot] = ins;
        }
    }

    pub fn set_instrument(&mut self, ins: Option<InsertDsp>) {
        self.instrument = ins;
    }
}

pub enum Cmd {
    Play { from: i64 },
    Record { from: i64, punch: i64 },
    Stop,
    Locate(i64),
    /// The tempo map and time signatures (the old one comes back as garbage).
    Tempo(Arc<TempoMap>),
    Cycle { enabled: bool, start: i64, end: i64 },
    Metronome { on: bool, count_in: bool },
    AddTrack(Box<TrackDsp>, usize),
    RemoveTrack(Id),
    Order(Vec<Id>),
    Params(Id, TrackParams),
    MasterVolume(f32),
    Regions(Id, Arc<Vec<RegionDsp>>),
    Insert { track: Id, slot: usize, insert: Option<InsertDsp> },
    Instrument { track: Id, insert: Option<InsertDsp> },
    /// Rearranges a strip's inserts without reloading them: slot `i` takes what was in slot
    /// `order[i]`.
    ReorderInserts { track: Id, order: [u8; INSERT_SLOTS] },
    Bypass { track: Id, uid: Id, bypass: bool },
    RecordSink { track: Id, sink: Option<rtrb::Producer<f32>> },
    MidiTarget(Option<Id>),
    /// Whether record-enabled tracks play their input without the I button.
    AutoMonitor(bool),
    /// New channel EQ coefficients for a track (or MASTER_ID).
    Eq(Id, EqCoeffs),
    /// Which strip feeds the EQ editor's spectrum analyzer.
    Analyzer(Option<Id>),
    /// A track's automation lanes.
    Automation(Id, Arc<Vec<LaneDsp>>),
    AutoMode(Id, AutoMode),
    /// The user holds (or lets go of) a parameter while automation is being written.
    AutoTouch { track: Id, target: AutoTarget, on: bool },
    /// Metronome sound, level and when it plays.
    Click(ClickParams),
    /// Whether live MIDI plays the target track (it is recorded either way).
    MidiThru(bool),
    /// A track's delay compensation, with a new delay line when the old one is too short (or
    /// None to keep it).
    Pdc { track: Id, params: PdcParams, line: Option<Box<DelayLine>> },
    PdcGlobal(PdcGlobal),
    /// Sent before `Record`: armed tracks keep playing outside `range` (None: they are silent
    /// throughout), and input is only captured between the `capture` positions (otherwise from
    /// the punch on). Cleared by `Stop`.
    Autopunch { range: Option<(i64, i64)>, capture: (i64, i64) },
    /// Sets a built-in effect's parameter (plain units) on a track (or MASTER_ID).
    FxParam { track: Id, uid: Id, id: u32, value: f32 },
    /// Which track keys a built-in effect's side-chain (None: it listens to its input).
    Sidechain { track: Id, uid: Id, source: Option<Id> },
}

pub enum Garbage {
    Track(Box<TrackDsp>),
    Insert(InsertDsp),
    Regions(Arc<Vec<RegionDsp>>),
    Sink(rtrb::Producer<f32>),
    Ids(Vec<Id>),
    Automation(Arc<Vec<LaneDsp>>),
    Delay(Box<DelayLine>),
    Tempo(Arc<TempoMap>),
}

/// Metronome parameters, worked out from the project's [`ClickSettings`].
#[derive(Debug, Clone, Copy)]
pub struct ClickParams {
    pub sound: ClickSound,
    /// Linear level of an accented downbeat; other clicks are scaled from it.
    pub level: f32,
    pub accent: bool,
    pub bar_freq: f32,
    pub beat_freq: f32,
    /// Clicks per beat (1..=4).
    pub division: u32,
    pub while_playing: bool,
    pub while_recording: bool,
    pub only_count_in: bool,
}

impl ClickParams {
    pub fn from_settings(c: &ClickSettings) -> ClickParams {
        // Each sound's natural bar / beat pitches; the settings transpose them.
        let (bar, beat) = match c.sound {
            ClickSound::Classic => (1568.0, 1046.5),
            ClickSound::Click => (3135.0, 2349.0),
            ClickSound::Woodblock => (1245.0, 932.0),
            ClickSound::Beep => (1760.0, 880.0),
        };
        let st = |n: i32| 2f32.powf(n.clamp(-24, 24) as f32 / 12.0);
        ClickParams {
            sound: c.sound,
            level: 0.5 * db_to_gain(c.volume_db.clamp(-60.0, 12.0)),
            accent: c.accent,
            bar_freq: bar * st(c.bar_pitch),
            beat_freq: beat * st(c.beat_pitch),
            division: c.division.clamp(1, 4),
            while_playing: c.while_playing,
            while_recording: c.while_recording,
            only_count_in: c.only_count_in,
        }
    }
}

impl Default for ClickParams {
    fn default() -> Self {
        ClickParams::from_settings(&ClickSettings::default())
    }
}

/// The count-in's clicks: steps of `1 / div` beat counted back from the punch, at the tempo
/// and signature in effect there (as in Logic, whatever the tempo before the punch).
struct CountIn {
    /// The grid point at or after the punch (step 0) and its index in its bar.
    anchor: f64,
    anchor_sub: i64,
    step: f64,
    div: i64,
    per_bar: i64,
}

impl CountIn {
    fn at(map: &TempoMap, punch: i64, div: u32) -> CountIn {
        let at = punch as f64;
        let g = map.grid_at_or_after(map.beats_at(at), div);
        let (num, _) = map.sig_at(at);
        CountIn { anchor: g.pos, anchor_sub: g.sub as i64, step: map.samples_per_beat_at(at) / div as f64, div: div as i64, per_bar: num as i64 * div as i64 }
    }

    fn pos(&self, k: i64) -> i64 {
        (self.anchor + k as f64 * self.step).round() as i64
    }

    fn first_at_or_after(&self, pos: i64) -> i64 {
        ((pos as f64 - self.anchor) / self.step - 1e-9).ceil() as i64
    }

    fn downbeat(&self, k: i64) -> bool {
        (self.anchor_sub + k).rem_euclid(self.per_bar) == 0
    }

    fn on_beat(&self, k: i64) -> bool {
        (self.anchor_sub + k).rem_euclid(self.div) == 0
    }
}

/// One sounding metronome click. Synthesized sample by sample, without allocating.
#[derive(Default)]
struct Click {
    remaining: usize,
    len: usize,
    phase: f32,
    phase2: f32,
    freq: f32,
    amp: f32,
    sound: ClickSound,
    /// Exponential envelopes and their per-sample decay factors (Woodblock, Click).
    env1: f32,
    env2: f32,
    k1: f32,
    k2: f32,
    noise: u32,
    hp: f32,
}

impl Click {
    fn start(sound: ClickSound, freq: f32, amp: f32, sr: f32) -> Click {
        let ms = match sound {
            ClickSound::Classic => 35.0,
            ClickSound::Click => 14.0,
            ClickSound::Woodblock => 70.0,
            ClickSound::Beep => 70.0,
        };
        let len = (ms * 0.001 * sr) as usize;
        let decay = |secs: f32| (-1.0 / (secs * sr)).exp();
        let (k1, k2) = match sound {
            ClickSound::Woodblock => (decay(0.016), decay(0.006)),
            ClickSound::Click => (decay(0.0025), decay(0.0012)),
            _ => (1.0, 1.0),
        };
        Click { remaining: len, len, freq, amp, sound, env1: 1.0, env2: 1.0, k1, k2, noise: 0x9E37_79B9, ..Click::default() }
    }

    fn next(&mut self, sr: f32) -> f32 {
        use std::f32::consts::TAU;
        let age = self.len - self.remaining;
        let s = match self.sound {
            ClickSound::Classic => {
                let env = self.remaining as f32 / self.len.max(1) as f32;
                (self.phase * TAU).sin() * env * env
            }
            ClickSound::Click => {
                // High-passed noise for the transient plus a short pitched body.
                self.noise ^= self.noise << 13;
                self.noise ^= self.noise >> 17;
                self.noise ^= self.noise << 5;
                let n = (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0;
                let hi = n - self.hp;
                self.hp += 0.35 * hi;
                0.8 * hi * self.env2 + (self.phase * TAU).sin() * self.env1
            }
            ClickSound::Woodblock => {
                let attack = (age as f32 / (0.0008 * sr)).min(1.0);
                attack * 0.72 * ((self.phase * TAU).sin() * self.env1 + 0.45 * (self.phase2 * TAU).sin() * self.env2)
            }
            ClickSound::Beep => {
                // Rounded square (fundamental plus a little third harmonic), 3 ms edges.
                let edge = (0.003 * sr).max(1.0);
                let env = (age as f32 / edge).min(1.0).min(self.remaining as f32 / edge);
                let p = self.phase * TAU;
                0.8 * (p.sin() + (3.0 * p).sin() / 4.0) * env
            }
        };
        self.phase = (self.phase + self.freq / sr) % 1.0;
        self.phase2 = (self.phase2 + self.freq * 2.73 / sr) % 1.0;
        self.env1 *= self.k1;
        self.env2 *= self.k2;
        self.remaining -= 1;
        s * self.amp
    }
}

pub struct Engine {
    pub sample_rate: f64,
    tracks: Vec<Box<TrackDsp>>,
    master_inserts: Vec<Option<InsertDsp>>,
    master_volume: f32,
    master_gain: f32,
    playing: bool,
    recording: bool,
    pos: i64,
    punch: i64,
    tempo: Arc<TempoMap>,
    cycle: (bool, i64, i64),
    metronome: bool,
    count_in: bool,
    click: Click,
    click_params: ClickParams,
    midi_thru: bool,
    midi_target: Option<Id>,
    auto_monitor: bool,
    master_eq: EqCoeffs,
    master_eq_state: EqState,
    analyzer_target: Option<Id>,
    analyzer_tx: rtrb::Producer<f32>,
    /// Commands from the session. A wait-free single-producer ring: the audio thread must never
    /// spin waiting on a sender (a crossbeam channel can, while a preempted sender finishes a
    /// write, which with real-time priority starves the sender and trips RLIMIT_RTTIME).
    cmd_rx: rtrb::Consumer<Cmd>,
    /// Replaced objects back to the session, to be dropped off the audio thread.
    garbage: RefCell<rtrb::Producer<Garbage>>,
    pub status: Arc<EngineStatus>,
    live_midi: rtrb::Consumer<[u8; 3]>,
    midi_rec: rtrb::Producer<(i64, [u8; 3])>,
    live: Vec<MidiEvent>,
    bus_l: Vec<f32>,
    bus_r: Vec<f32>,
    inputs: Vec<Vec<f32>>,
    load: f32,
    /// How long the callbacks have been taking longer than the audio they produce, and how
    /// many callbacks are left to skip after an overload.
    behind_secs: f64,
    shedding: u32,
    /// An offline render is running: it may take as long as it needs.
    offline: bool,
    /// Set while an offline render plays only some tracks (see `offline.rs`).
    isolate: Option<Isolate>,
    /// Jump faders straight to their targets on the next block instead of gliding, so a
    /// render starts (and live playback resumes) at the right levels.
    snap_gains: bool,
    pdc: PdcGlobal,
    /// Scratch for a send that reads the delay line at its own tap, and the fader gains of
    /// the block, which post-fader sends at another tap apply again.
    tap_l: Vec<f32>,
    tap_r: Vec<f32>,
    gain_l: Vec<f32>,
    gain_r: Vec<f32>,
    /// See `Cmd::Autopunch`.
    punch_range: Option<(i64, i64)>,
    capture: Option<(i64, i64)>,
}

fn db_to_gain(db: f32) -> f32 {
    if db <= -96.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

pub fn volume_gain(db: f32) -> f32 {
    db_to_gain(db)
}

/// `t` moved to timeline position `pos` (also for plug-ins on a strip whose audio lags the
/// playhead): the tempo, signature and musical position there.
fn transport_at(map: &TempoMap, t: &TransportInfo, pos: i64) -> TransportInfo {
    let at = pos as f64;
    let bar = map.bar_at(at);
    let (num, den) = map.sig_at_bar(bar);
    TransportInfo {
        tempo: map.bpm_at(at),
        sig_num: num as i32,
        sig_den: den as i32,
        position_samples: pos,
        position_quarters: map.beats_at(at),
        bar_start_quarters: map.bar_start_q(bar),
        ..*t
    }
}

/// Mixes the audio regions playing in this block into `l`/`r`, except inside `quiet` (timeline
/// samples, start inclusive), where an Autopunch recording is replacing them.
fn play_audio(regions: &[RegionDsp], pos: i64, l: &mut [f32], r: &mut [f32], quiet: (i64, i64)) {
    let block_end = pos + l.len() as i64;
    for reg in regions {
        let RegionDsp::Audio { start, length, offset, gain, clip, fade_in, fade_out, laws } = reg else { continue };
        let (rs, re) = (*start, *start + *length);
        if re <= pos || rs >= block_end {
            continue;
        }
        let from = rs.max(pos);
        let to = re.min(block_end);
        let ch = &clip.channels;
        let frames = clip.frames() as i64;
        for tl in from..to {
            if tl >= quiet.0 && tl < quiet.1 {
                continue;
            }
            let src = tl - rs + offset;
            // Fades: `x` runs from 0 (silent) to 1 (full level). A speed fade also moves where
            // the audio is read from, by `lag` frames.
            let mut g = *gain;
            let mut lag = 0.0f64;
            if tl < fade_in.0 + fade_in.1 && fade_in.1 > 0 {
                let x = (tl - fade_in.0) as f32 / fade_in.1 as f32;
                g *= laws[0].gain(x);
                lag += laws[0].lag(x) as f64 * fade_in.1 as f64;
            }
            if tl >= fade_out.0 && fade_out.1 > 0 {
                let x = (fade_out.0 + fade_out.1 - tl) as f32 / fade_out.1 as f32;
                g *= laws[1].gain(x);
                lag -= laws[1].lag(x) as f64 * fade_out.1 as f64;
            }
            let i = (tl - pos) as usize;
            if lag == 0.0 {
                if src < 0 || src >= frames {
                    continue;
                }
                let a = ch[0][src as usize] * g;
                let b = if ch.len() > 1 { ch[1][src as usize] * g } else { a };
                l[i] += a;
                r[i] += b;
            } else {
                // Between two frames: interpolate.
                let at = src as f64 + lag;
                let k = at.floor();
                let f = (at - k) as f32;
                let k = k as i64;
                let read = |c: &[f32], j: i64| if j >= 0 && j < frames { c[j as usize] } else { 0.0 };
                let tap = |c: &[f32]| read(c, k) + (read(c, k + 1) - read(c, k)) * f;
                let a = tap(&ch[0]) * g;
                let b = if ch.len() > 1 { tap(&ch[1]) * g } else { a };
                l[i] += a;
                r[i] += b;
            }
        }
    }
}

/// Mixes audio regions into `l`/`r` for the block starting at `pos`, exactly as playback does
/// (for renders that mix regions outside the engine, such as Join).
pub fn mix_regions(regions: &[RegionDsp], pos: i64, l: &mut [f32], r: &mut [f32]) {
    play_audio(regions, pos, l, r, (0, 0));
}

/// The side-chain signal for a built-in effect keyed by the track at `idx` (played earlier in
/// the block): its post-fader output, or None when there is none (no source, or one an
/// isolated render leaves out).
fn key_signal<'a>(tracks: &'a [Box<TrackDsp>], idx: Option<usize>, iso: Option<&Isolate>, n: usize) -> crate::fx::Key<'a> {
    let src = tracks.get(idx?)?;
    if !src.keyed || src.kind == TrackKind::Folder || iso.map(|i| i.role(src.id) == Role::Skip).unwrap_or(false) {
        return None;
    }
    Some((&src.key_l[..n], &src.key_r[..n]))
}

/// Adds `gain * (l, r)` into an aux's (or summing stack's) input, ramping the gain to `target`.
fn mix_send(dst: &mut TrackDsp, l: &[f32], r: &[f32], gain: &mut f32, target: f32, smooth: f32) {
    let mut g = *gain;
    if g == 0.0 && target == 0.0 {
        return;
    }
    let (dl, dr) = (&mut dst.sum_l[..l.len()], &mut dst.sum_r[..r.len()]);
    for i in 0..l.len() {
        g += (target - g) * smooth;
        dl[i] += l[i] * g;
        dr[i] += r[i] * g;
    }
    // Settle exactly on silence so idle sends cost nothing.
    *gain = if target == 0.0 && g.abs() < 1e-6 { 0.0 } else { g };
}

impl Engine {
    pub fn new(
        sample_rate: f64,
        cmd_rx: rtrb::Consumer<Cmd>,
        garbage: rtrb::Producer<Garbage>,
        status: Arc<EngineStatus>,
        live_midi: rtrb::Consumer<[u8; 3]>,
        midi_rec: rtrb::Producer<(i64, [u8; 3])>,
        analyzer_tx: rtrb::Producer<f32>,
    ) -> Engine {
        Engine {
            sample_rate,
            tracks: Vec::with_capacity(256),
            master_inserts: (0..INSERT_SLOTS).map(|_| None).collect(),
            master_volume: 1.0,
            master_gain: 1.0,
            playing: false,
            recording: false,
            pos: 0,
            punch: 0,
            tempo: Arc::new(TempoMap::constant(sample_rate, 120.0, 4, 4)),
            cycle: (false, 0, 0),
            metronome: false,
            count_in: false,
            click: Click::default(),
            click_params: ClickParams::default(),
            midi_thru: true,
            midi_target: None,
            auto_monitor: false,
            master_eq: EqCoeffs::default(),
            master_eq_state: EqState::default(),
            analyzer_target: None,
            analyzer_tx,
            cmd_rx,
            garbage: RefCell::new(garbage),
            status,
            live_midi,
            midi_rec,
            live: Vec::with_capacity(MAX_MIDI),
            bus_l: vec![0.0; MAX_BLOCK],
            bus_r: vec![0.0; MAX_BLOCK],
            inputs: (0..MAX_INPUTS).map(|_| vec![0.0; MAX_BLOCK]).collect(),
            load: 0.0,
            behind_secs: 0.0,
            shedding: 0,
            offline: false,
            isolate: None,
            snap_gains: false,
            pdc: PdcGlobal::default(),
            tap_l: vec![0.0; MAX_BLOCK],
            tap_r: vec![0.0; MAX_BLOCK],
            gain_l: vec![0.0; MAX_BLOCK],
            gain_r: vec![0.0; MAX_BLOCK],
            punch_range: None,
            capture: None,
        }
    }

    /// Replaces the whole graph (project load, sample-rate change). Called with the engine lock
    /// held from a non-audio thread; returns the old graph for the caller to drop.
    pub fn replace_graph(
        &mut self,
        sample_rate: f64,
        tracks: Vec<Box<TrackDsp>>,
        master_inserts: Vec<Option<InsertDsp>>,
    ) -> (Vec<Box<TrackDsp>>, Vec<Option<InsertDsp>>) {
        self.sample_rate = sample_rate;
        self.playing = false;
        self.recording = false;
        self.status.playing.store(false, Ordering::Relaxed);
        self.status.recording.store(false, Ordering::Relaxed);
        let old_t = std::mem::replace(&mut self.tracks, tracks);
        let old_m = std::mem::replace(&mut self.master_inserts, master_inserts);
        self.resolve_routes();
        (old_t, old_m)
    }

    fn discard(&self, g: Garbage) {
        // The ring is bounded; if it is somehow full, leaking beats blocking the audio thread.
        if let Err(rtrb::PushError::Full(g)) = self.garbage.borrow_mut().push(g) {
            std::mem::forget(g);
        }
    }

    fn track_mut(&mut self, id: Id) -> Option<&mut Box<TrackDsp>> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Finds where each track's output and sends go. Receivers come after their senders in the
    /// list (see `Project::process_order`); a route pointing backwards is ignored, and an
    /// output then plays straight to Stereo Out.
    fn resolve_routes(&mut self) {
        for i in 0..self.tracks.len() {
            let find = |id: Id| self.tracks.iter().position(|t| t.id == id).filter(|&j| j > i);
            let out = self.tracks[i].params.output.and_then(find);
            let mut sends = [None; SEND_SLOTS];
            for (k, s) in self.tracks[i].params.sends.iter().enumerate() {
                sends[k] = s.and_then(|s| find(s.bus));
            }
            let t = &mut self.tracks[i];
            t.out_idx = out;
            t.send_idx = sends;
        }
        self.resolve_keys();
    }

    /// Finds each built-in effect's side-chain source. A key must come from a track played
    /// earlier in the list (Stereo Out's plug-ins can be keyed by any track); others hear
    /// nothing.
    fn resolve_keys(&mut self) {
        for t in &mut self.tracks {
            t.keyed = false;
        }
        let n = self.tracks.len();
        for i in 0..=n {
            let count = if i < n { self.tracks[i].inserts.len() } else { self.master_inserts.len() };
            for k in 0..count {
                let slots = if i < n { &self.tracks[i].inserts } else { &self.master_inserts };
                let Some(InsertDsp { plugin: Plugin::Builtin(b), .. }) = &slots[k] else { continue };
                let key = b.key;
                let idx = key.and_then(|id| self.tracks[..i].iter().position(|t| t.id == id));
                let slots = if i < n { &mut self.tracks[i].inserts } else { &mut self.master_inserts };
                if let Some(InsertDsp { plugin: Plugin::Builtin(b), .. }) = &mut slots[k] {
                    b.key_idx = idx;
                }
                if let Some(j) = idx {
                    self.tracks[j].keyed = true;
                }
            }
        }
    }

    /// Clears every built-in effect's state (envelopes, delay lines, tails), after a jump in
    /// time and around offline renders, so nothing from before rings into what plays next.
    fn reset_effects(&mut self) {
        let slots = self.tracks.iter_mut().flat_map(|t| t.inserts.iter_mut()).chain(self.master_inserts.iter_mut());
        for ins in slots.flatten() {
            if let Plugin::Builtin(b) = &mut ins.plugin {
                b.reset();
            }
        }
    }

    /// The built-in effect `uid` on a track's (or Stereo Out's) inserts.
    fn builtin_mut(&mut self, track: Id, uid: Id) -> Option<&mut BuiltinDsp> {
        let slots = if track == MASTER_ID { &mut self.master_inserts } else { &mut self.track_mut(track)?.inserts };
        slots.iter_mut().flatten().find(|i| i.uid == uid).and_then(|i| match &mut i.plugin {
            Plugin::Builtin(b) => Some(&mut **b),
            _ => None,
        })
    }

    fn flush_all_notes(&mut self) {
        for t in &mut self.tracks {
            t.flush_notes = true;
            t.chase_cc = true;
        }
    }

    fn chase_all_cc(&mut self) {
        for t in &mut self.tracks {
            t.chase_cc = true;
        }
    }

    /// After a jump in time, plug-in automation is resent even where the value didn't change,
    /// since the plug-in may have been moved by hand in between.
    fn resend_automation(&mut self) {
        for t in &mut self.tracks {
            t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
        }
    }

    fn apply(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play { from } => {
                if from != self.pos {
                    self.reset_effects();
                }
                self.pos = from;
                self.playing = true;
                self.recording = false;
                self.resend_automation();
                self.chase_all_cc();
            }
            Cmd::Record { from, punch } => {
                if from != self.pos {
                    self.reset_effects();
                }
                self.pos = from;
                self.punch = punch;
                self.playing = true;
                self.recording = true;
                self.resend_automation();
                self.chase_all_cc();
            }
            Cmd::Stop => {
                self.playing = false;
                self.recording = false;
                self.punch_range = None;
                self.capture = None;
                self.flush_all_notes();
            }
            Cmd::Locate(p) => {
                if p != self.pos {
                    self.reset_effects();
                }
                self.pos = p;
                self.flush_all_notes();
                self.resend_automation();
            }
            Cmd::Tempo(map) => {
                let old = std::mem::replace(&mut self.tempo, map);
                self.discard(Garbage::Tempo(old));
            }
            Cmd::Cycle { enabled, start, end } => self.cycle = (enabled && end > start, start, end),
            Cmd::Metronome { on, count_in } => {
                self.metronome = on;
                self.count_in = count_in;
            }
            Cmd::AddTrack(t, idx) => {
                let idx = idx.min(self.tracks.len());
                self.tracks.insert(idx, t);
                self.resolve_routes();
            }
            Cmd::RemoveTrack(id) => {
                if let Some(i) = self.tracks.iter().position(|t| t.id == id) {
                    let t = self.tracks.remove(i);
                    self.discard(Garbage::Track(t));
                }
                self.resolve_routes();
            }
            Cmd::Order(ids) => {
                self.tracks.sort_by_key(|t| ids.iter().position(|&i| i == t.id).unwrap_or(usize::MAX));
                self.discard(Garbage::Ids(ids));
                self.resolve_routes();
            }
            Cmd::Params(id, p) => {
                if let Some(t) = self.track_mut(id) {
                    let bus = |s: &Option<SendParams>| s.map(|s| s.bus);
                    let rerouted = t.params.output != p.output || t.params.sends.iter().map(bus).ne(p.sends.iter().map(bus));
                    t.params = p;
                    if rerouted {
                        self.resolve_routes();
                    }
                }
            }
            Cmd::MasterVolume(v) => self.master_volume = v,
            Cmd::Regions(id, r) => {
                if let Some(t) = self.track_mut(id) {
                    let old = std::mem::replace(&mut t.regions, r);
                    t.flush_notes = true;
                    self.discard(Garbage::Regions(old));
                }
            }
            Cmd::Insert { track, slot, insert } => {
                let old = if track == MASTER_ID {
                    self.master_inserts.get_mut(slot).and_then(|s| std::mem::replace(s, insert))
                } else {
                    // A new instance starts at its own values: automation is sent to it afresh,
                    // even if its lanes came before it (an undo restores lanes first).
                    self.track_mut(track).and_then(|t| {
                        t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
                        t.inserts.get_mut(slot).and_then(|s| std::mem::replace(s, insert))
                    })
                };
                if let Some(o) = old {
                    self.discard(Garbage::Insert(o));
                }
                self.resolve_keys();
            }
            Cmd::ReorderInserts { track, order } => {
                let slots = if track == MASTER_ID { Some(&mut self.master_inserts) } else { self.track_mut(track).map(|t| &mut t.inserts) };
                if let Some(slots) = slots {
                    // Moved out and back in through a fixed array: nothing is allocated.
                    let mut taken: [Option<InsertDsp>; INSERT_SLOTS] = std::array::from_fn(|i| slots.get_mut(i).and_then(Option::take));
                    for (i, &from) in order.iter().enumerate() {
                        if let Some(s) = slots.get_mut(i) {
                            *s = taken[from as usize].take();
                        }
                    }
                }
                self.resolve_keys();
            }
            Cmd::Instrument { track, insert } => {
                let old = self.track_mut(track).and_then(|t| {
                    t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
                    std::mem::replace(&mut t.instrument, insert)
                });
                if let Some(o) = old {
                    self.discard(Garbage::Insert(o));
                }
            }
            Cmd::Bypass { track, uid, bypass } => {
                let set = |s: &mut Option<InsertDsp>| {
                    if let Some(i) = s {
                        if i.uid == uid {
                            i.bypass = bypass;
                        }
                    }
                };
                if track == MASTER_ID {
                    self.master_inserts.iter_mut().for_each(set);
                } else if let Some(t) = self.track_mut(track) {
                    t.inserts.iter_mut().for_each(set);
                    set(&mut t.instrument);
                }
            }
            Cmd::RecordSink { track, sink } => {
                let old = self.track_mut(track).and_then(|t| std::mem::replace(&mut t.rec, sink));
                if let Some(o) = old {
                    self.discard(Garbage::Sink(o));
                }
            }
            Cmd::AutoMonitor(on) => self.auto_monitor = on,
            Cmd::Eq(id, c) => {
                if id == MASTER_ID {
                    if !c.active {
                        self.master_eq_state.reset();
                    }
                    self.master_eq = c;
                } else if let Some(t) = self.track_mut(id) {
                    t.set_eq(c);
                }
            }
            Cmd::Analyzer(t) => self.analyzer_target = t,
            Cmd::MidiTarget(t) => {
                self.midi_target = t;
                self.flush_all_notes();
            }
            Cmd::Automation(id, lanes) => {
                if let Some(t) = self.track_mut(id) {
                    let old = std::mem::replace(&mut t.automation, lanes);
                    t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
                    self.discard(Garbage::Automation(old));
                } else {
                    self.discard(Garbage::Automation(lanes));
                }
            }
            Cmd::AutoMode(id, mode) => {
                if let Some(t) = self.track_mut(id) {
                    t.auto_mode = mode;
                    t.auto_sent = [f32::NAN; MAX_AUTO_SENT];
                }
            }
            Cmd::AutoTouch { track, target, on } => {
                if let Some(t) = self.track_mut(track) {
                    t.set_touched(target, on);
                }
            }
            Cmd::Click(c) => self.click_params = c,
            Cmd::MidiThru(on) => {
                self.midi_thru = on;
                self.flush_all_notes();
            }
            Cmd::Pdc { track, params, line } => {
                let old = match self.track_mut(track) {
                    Some(t) => {
                        t.pdc = params;
                        match line {
                            Some(l) => t.delay.replace(l),
                            None => None,
                        }
                    }
                    None => line,
                };
                if let Some(o) = old {
                    self.discard(Garbage::Delay(o));
                }
            }
            Cmd::PdcGlobal(g) => self.pdc = g,
            Cmd::Autopunch { range, capture } => {
                self.punch_range = range;
                self.capture = Some(capture);
            }
            Cmd::FxParam { track, uid, id, value } => {
                if let Some(b) = self.builtin_mut(track, uid) {
                    b.set(id, value);
                }
            }
            Cmd::Sidechain { track, uid, source } => {
                if let Some(b) = self.builtin_mut(track, uid) {
                    b.key = source;
                }
                self.resolve_keys();
            }
        }
    }

    /// `input` holds `frames * in_ch` interleaved samples; `output` holds `frames * out_ch`.
    pub fn process(&mut self, input: &[f32], in_ch: usize, output: &mut [f32], out_ch: usize) {
        let started = Instant::now();
        let _ftz = crate::denormals::NoDenormals::new();
        let frames = if out_ch == 0 { 0 } else { output.len() / out_ch };

        while let Ok(cmd) = self.cmd_rx.pop() {
            self.apply(cmd);
            self.status.cmds_done.fetch_add(1, Ordering::Relaxed);
        }

        self.live.clear();
        while let Ok(m) = self.live_midi.pop() {
            if self.live.len() < self.live.capacity() {
                self.live.push(MidiEvent { offset: 0, data: m });
            }
            self.status.midi_activity.fetch_add(1, Ordering::Relaxed);
        }

        // After an overload: silence, nothing processed, until the thread has caught up. The
        // session stops the transport when it sees the overload count change.
        if self.shedding > 0 {
            self.shedding -= 1;
            output.fill(0.0);
            return;
        }

        let mut done = 0;
        while done < frames {
            let mut n = (frames - done).min(MAX_BLOCK);
            let (cyc_on, cs, ce) = self.cycle;
            // Cycle recording loops too: each pass becomes a take.
            let looping = self.playing && cyc_on;
            if looping && self.pos < ce && self.pos + n as i64 > ce {
                n = (ce - self.pos) as usize;
            }
            let in_slice = if in_ch > 0 && input.len() >= (done + n) * in_ch {
                &input[done * in_ch..(done + n) * in_ch]
            } else {
                &[][..]
            };
            self.process_block(in_slice, in_ch, n);
            // The last line of defence: the plug-ins are checked as they run.
            contain(&mut self.bus_l[..n], &mut self.bus_r[..n]);
            for i in 0..n {
                let o = (done + i) * out_ch;
                if out_ch >= 2 {
                    output[o] = self.bus_l[i];
                    output[o + 1] = self.bus_r[i];
                    for c in 2..out_ch {
                        output[o + c] = 0.0;
                    }
                } else {
                    output[o] = 0.5 * (self.bus_l[i] + self.bus_r[i]);
                }
            }
            // Live MIDI is only delivered with the first sub-block.
            self.live.clear();
            if self.playing {
                self.pos += n as i64;
                if looping && self.pos >= ce {
                    self.pos = cs;
                    self.flush_all_notes();
                }
            }
            done += n;
        }

        self.status.pos.store(self.pos, Ordering::Relaxed);
        self.status.playing.store(self.playing, Ordering::Relaxed);
        self.status.recording.store(self.recording, Ordering::Relaxed);

        if frames > 0 {
            let budget = frames as f64 / self.sample_rate;
            let spent = started.elapsed().as_secs_f64();
            let l = (spent / budget) as f32;
            self.load = self.load * 0.9 + l * 0.1;
            self.status.cpu_load.store(self.load.to_bits(), Ordering::Relaxed);
            // A callback that takes longer than the audio it makes leaves the thread no time
            // to sleep. Kept up, the kernel kills the whole app, so give up a moment of audio.
            self.behind_secs = if spent > budget && !self.offline { self.behind_secs + spent } else { 0.0 };
            if self.behind_secs > OVERLOAD_SECS {
                self.behind_secs = 0.0;
                self.shedding = (SHED_SECS / budget).ceil() as u32;
                self.flush_all_notes();
                self.status.overloads.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// What plug-ins are told about the transport for the block starting at the playhead.
    pub fn transport(&self) -> TransportInfo {
        let t = TransportInfo {
            playing: self.playing,
            recording: self.recording,
            sample_rate: self.sample_rate,
            cycle: None,
            ..Default::default()
        };
        let mut t = transport_at(&self.tempo, &t, self.pos);
        if self.cycle.0 {
            t.cycle = Some((self.tempo.beats_at(self.cycle.1 as f64), self.tempo.beats_at(self.cycle.2 as f64)));
        }
        t
    }

    fn process_block(&mut self, input: &[f32], in_ch: usize, n: usize) {
        let in_ch = in_ch.min(MAX_INPUTS);
        let stride = input.len() / n.max(1);
        for c in 0..in_ch {
            let buf = &mut self.inputs[c];
            let mut peak = 0.0f32;
            for i in 0..n {
                let s = if stride > 0 { input[i * stride + c] } else { 0.0 };
                buf[i] = s;
                peak = peak.max(s.abs());
            }
            self.status.inputs[c].feed(peak, peak);
        }
        for c in in_ch..MAX_INPUTS {
            self.inputs[c][..n].fill(0.0);
        }

        self.bus_l[..n].fill(0.0);
        self.bus_r[..n].fill(0.0);
        // Isolated renders work out audibility themselves (see `Isolate`).
        let any_solo = self.isolate.is_none() && self.tracks.iter().any(|t| t.params.solo);
        let iso = self.isolate.as_ref();
        let snap_gains = self.snap_gains;
        let transport = self.transport();
        let (pos, playing, recording, punch, sr) = (self.pos, self.playing, self.recording, self.punch, self.sample_rate);
        let punch_range = self.punch_range;
        let (capture_from, capture_end) = self.capture.unwrap_or((punch, i64::MAX));
        let auto_monitor = self.auto_monitor;
        let low_latency = self.pdc.low_latency;
        let block_end = pos + n as i64;
        let smooth = 1.0 - (-1.0 / (0.005 * sr as f32)).exp();

        for ti in 0..self.tracks.len() {
            let (before, rest) = self.tracks.split_at_mut(ti);
            let (cur, tail) = rest.split_first_mut().unwrap();
            let t = &mut **cur;
            let role = iso.map(|i| i.role(t.id));
            if role == Some(Role::Skip) {
                continue;
            }
            let tap = role == Some(Role::Tap);
            let fx = !tap || iso.map(|i| i.effects).unwrap_or(true);
            let (l, r) = (&mut t.l[..n], &mut t.r[..n]);
            l.fill(0.0);
            r.fill(0.0);
            t.midi.clear();
            let mut p = t.params;
            let pdc = t.pdc;
            // A live track (recording, or monitored in Low Latency Mode) skips its delay
            // compensation, so the performer isn't kept waiting (see session/pdc.rs).
            let monitored = match t.kind {
                TrackKind::Audio => p.monitor || (p.armed && auto_monitor),
                TrackKind::Instrument => p.armed,
                _ => false,
            };
            let live = (recording && p.armed && t.kind == TrackKind::Audio) || (low_latency && monitored);
            let ll_skip = if low_latency && monitored { pdc.low_latency_skip } else { 0 };
            // The strip's plug-ins hear the timeline `input_time` late, and its fader
            // `output_time` late: automation and the plug-ins' transport follow the audio.
            let fx_at = pos - pdc.input_time as i64;
            let mix_at = pos - pdc.output_time as i64;
            let transport = if pdc.input_time > 0 { transport_at(&self.tempo, &transport, fx_at) } else { transport };

            // Automation overrides the fader, pan and mute (and drives plug-in parameters).
            // Write mode plays the live values, which are being recorded over the lanes.
            if playing && matches!(t.auto_mode, AutoMode::Read | AutoMode::Touch | AutoMode::Latch) {
                for (k, lane) in t.automation.iter().enumerate() {
                    if t.touched.contains(&Some(lane.target)) {
                        continue;
                    }
                    let at = if matches!(lane.target, AutoTarget::Plugin { .. }) { fx_at } else { mix_at };
                    let last = at + n as i64 - 1;
                    let Some(v) = value_at(&lane.points, at) else { continue };
                    match lane.target {
                        AutoTarget::Volume => p.volume = volume_gain(v) * p.stack_gain,
                        AutoTarget::Pan => p.pan = v,
                        AutoTarget::Mute => p.mute = v >= 0.5 || p.stack_mute,
                        // A muted send (gain 0) stays silent.
                        AutoTarget::Send(slot) => {
                            if let Some(Some(s)) = p.sends.get_mut(slot as usize) {
                                if s.gain > 0.0 {
                                    s.gain = volume_gain(v);
                                }
                            }
                        }
                        AutoTarget::Plugin { uid, param } => {
                            // Two points per block: the plug-in ramps between them.
                            let v1 = value_at(&lane.points, last).unwrap_or(v);
                            let sent = t.auto_sent.get(k).copied().unwrap_or(f32::NAN);
                            if v == sent && v1 == sent {
                                continue;
                            }
                            let slot = t.inserts.iter_mut().flatten().chain(t.instrument.iter_mut()).find(|i| i.uid == uid);
                            if let Some(ins) = slot {
                                ins.plugin.set_param(param, 0, v as f64);
                                if v1 != v {
                                    ins.plugin.set_param(param, n as i32 - 1, v1 as f64);
                                }
                            }
                            if let Some(s) = t.auto_sent.get_mut(k) {
                                *s = v1;
                            }
                        }
                    }
                }
            }

            match t.kind {
                // A folder only shapes its subtracks' settings (worked out by the session).
                TrackKind::Folder => continue,
                TrackKind::Summing | TrackKind::Aux => {
                    l.copy_from_slice(&t.sum_l[..n]);
                    r.copy_from_slice(&t.sum_r[..n]);
                    t.sum_l[..n].fill(0.0);
                    t.sum_r[..n].fill(0.0);
                }
                TrackKind::Audio => {
                    // A track being recorded is silent, or with Autopunch only inside the range.
                    let quiet = match (recording && p.armed, punch_range) {
                        (false, _) => (0, 0),
                        (true, Some(r)) => r,
                        (true, None) => (i64::MIN, i64::MAX),
                    };
                    if playing && !(quiet.0 <= pos && quiet.1 >= block_end) {
                        play_audio(&t.regions, pos, l, r, quiet);
                    }
                    // Input is heard with the I button, or on armed tracks when auto monitoring is
                    // on. Interfaces with direct monitoring want it off to avoid hearing a
                    // delayed second copy.
                    if p.monitor || (p.armed && auto_monitor) {
                        match p.input {
                            InputSel::Mono(c) if (c as usize) < MAX_INPUTS => {
                                let src = &self.inputs[c as usize][..n];
                                for i in 0..n {
                                    l[i] += src[i];
                                    r[i] += src[i];
                                }
                            }
                            InputSel::Stereo(c) if (c as usize + 1) < MAX_INPUTS => {
                                for i in 0..n {
                                    l[i] += self.inputs[c as usize][i];
                                    r[i] += self.inputs[c as usize + 1][i];
                                }
                            }
                            _ => {}
                        }
                    }
                    if recording && p.armed && block_end > capture_from && pos < capture_end {
                        if let Some(sink) = &mut t.rec {
                            // The capture bounds can be the i64 extremes ("no punch-out"), and
                            // `pos` is negative during a count-in from the start, so plain
                            // subtraction would overflow.
                            let first = capture_from.saturating_sub(pos).clamp(0, n as i64) as usize;
                            let last = capture_end.saturating_sub(pos).clamp(0, n as i64) as usize;
                            let mut lost = false;
                            for i in first..last {
                                match p.input {
                                    InputSel::Mono(c) => lost |= sink.push(self.inputs[c as usize % MAX_INPUTS][i]).is_err(),
                                    InputSel::Stereo(c) => {
                                        let c = c as usize % (MAX_INPUTS - 1);
                                        lost |= sink.push(self.inputs[c][i]).is_err();
                                        lost |= sink.push(self.inputs[c + 1][i]).is_err();
                                    }
                                    InputSel::None => {}
                                }
                            }
                            if lost {
                                self.status.overruns.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
                TrackKind::Instrument => {
                    if t.flush_notes {
                        for pitch in 0..128 {
                            if t.sounding[pitch] {
                                t.midi.push(MidiEvent { offset: 0, data: [0x80, pitch as u8, 0] });
                                t.sounding[pitch] = false;
                            }
                        }
                        if t.cc_sent {
                            // Don't leave the pedal down or the pitch bent after a stop.
                            t.midi.push(MidiEvent { offset: 0, data: [0xB0, 64, 0] });
                            t.midi.push(MidiEvent { offset: 0, data: [0xE0, 0, 64] });
                            t.cc_sent = false;
                        }
                        if let Some(InsertDsp { plugin: Plugin::Synth(s), .. }) = &mut t.instrument {
                            s.all_notes_off();
                        }
                        t.flush_notes = false;
                    }
                    if playing && t.chase_cc {
                        t.chase_cc = false;
                        // The latest value of each controller before the playhead, in the
                        // region that covers it.
                        let mut last: [Option<u16>; CC_KINDS] = [None; CC_KINDS];
                        for reg in t.regions.iter() {
                            let RegionDsp::Midi { start, length, cc, .. } = reg else { continue };
                            if pos < *start || pos >= *start + *length {
                                continue;
                            }
                            let before = cc.partition_point(|c| c.start < pos - *start);
                            for c in &cc[..before] {
                                last[c.kind.index()] = Some(c.value);
                            }
                        }
                        for (i, v) in last.iter().enumerate() {
                            if let Some(v) = v {
                                if t.midi.len() < t.midi.capacity() {
                                    t.midi.push(MidiEvent { offset: 0, data: CcKind::from_index(i).message(*v) });
                                    t.cc_sent = true;
                                }
                            }
                        }
                    }
                    if playing {
                        for reg in t.regions.iter() {
                            let RegionDsp::Midi { start, length, notes, cc } = reg else { continue };
                            let (rs, re) = (*start, *start + *length);
                            if re <= pos || rs >= block_end {
                                continue;
                            }
                            let first = cc.partition_point(|c| rs + c.start < pos);
                            for c in &cc[first..] {
                                let at = rs + c.start;
                                if at >= block_end || at >= re || t.midi.len() >= t.midi.capacity() {
                                    break;
                                }
                                t.midi.push(MidiEvent { offset: (at - pos) as u32, data: c.kind.message(c.value) });
                                t.cc_sent = true;
                            }
                            for note in notes.iter() {
                                let on = rs + note.start;
                                let off = (on + note.length).min(re);
                                if on >= block_end {
                                    break;
                                }
                                if t.midi.len() + 2 > t.midi.capacity() {
                                    break;
                                }
                                if on >= pos && on < re {
                                    t.midi.push(MidiEvent { offset: (on - pos) as u32, data: [0x90, note.pitch, note.velocity.max(1)] });
                                    t.sounding[note.pitch as usize & 127] = true;
                                }
                                if off >= pos && off < block_end && on < re {
                                    t.midi.push(MidiEvent { offset: (off - pos) as u32, data: [0x80, note.pitch, 0] });
                                    t.sounding[note.pitch as usize & 127] = false;
                                }
                            }
                        }
                    }
                    if self.midi_target == Some(t.id) {
                        for m in &self.live {
                            if self.midi_thru && t.midi.len() < t.midi.capacity() {
                                t.midi.push(*m);
                            }
                            if recording && p.armed && pos >= capture_from && pos < capture_end {
                                let _ = self.midi_rec.push((pos, m.data));
                            }
                        }
                    }
                    t.midi.sort_by_key(|m| m.offset);
                    if let Some(ins) = &mut t.instrument {
                        if !ins.bypass && ll_skip & INSTRUMENT_BIT == 0 {
                            ins.plugin.process(l, r, &t.midi, &transport);
                            check_output(&self.status, ins, l, r);
                        }
                    }
                    // A frozen instrument track plays its freeze file instead.
                    if playing {
                        play_audio(&t.regions, pos, l, r, (0, 0));
                    }
                }
            }

            if fx {
                t.eq_state.process(&t.eq, l, r);
            }
            if self.analyzer_target == Some(t.id) {
                for i in 0..n {
                    let _ = self.analyzer_tx.push(0.5 * (l[i] + r[i]));
                }
            }

            for (k, slot) in t.inserts.iter_mut().enumerate().filter(|_| fx) {
                let Some(ins) = slot else { continue };
                if !ins.bypass && ll_skip & (1 << k) == 0 {
                    match &mut ins.plugin {
                        Plugin::Builtin(b) => {
                            let key = key_signal(before, b.key_idx, iso, n);
                            b.process(l, r, key, &fx_context(&transport));
                        }
                        p => p.process(l, r, &[], &transport),
                    }
                    check_output(&self.status, ins, l, r);
                }
            }
            // The delay line keeps the post-plug-in signal; the output and the sends read it
            // at their own delays.
            let delayed = !live && pdc.max_delay() > 0 && t.delay.is_some();
            if delayed {
                if let Some(d) = &mut t.delay {
                    d.write(l, r);
                }
            }

            let audible = match role {
                Some(Role::Member { heard }) => heard,
                Some(Role::Key) => !p.mute,
                Some(_) => true,
                None => !p.mute && (!any_solo || p.solo),
            };
            // Pre-fader sends tap the signal after the plug-ins, before the fader and pan. Mute
            // and solo still silence them, as in Logic.
            for k in 0..SEND_SLOTS {
                match (p.sends[k], t.send_idx[k]) {
                    // An isolated render leaves out auxes it doesn't play; their buses must
                    // not fill up meanwhile, or they'd burst out when playback resumes.
                    (Some(s), Some(j)) if j > ti && iso.map(|i| i.feeds(role, tail[j - ti - 1].id)).unwrap_or(true) => {
                        if s.pre_fader {
                            let target = if audible { s.gain } else { 0.0 };
                            match &t.delay {
                                Some(d) if delayed && pdc.sends[k] > 0 => {
                                    let (sl, sr) = (&mut self.tap_l[..n], &mut self.tap_r[..n]);
                                    d.read(pdc.sends[k] as usize, sl, sr);
                                    mix_send(&mut tail[j - ti - 1], sl, sr, &mut t.send_gains[k], target, smooth);
                                }
                                _ => mix_send(&mut tail[j - ti - 1], l, r, &mut t.send_gains[k], target, smooth),
                            }
                        }
                    }
                    _ => t.send_gains[k] = 0.0,
                }
            }
            let vol = if audible { p.volume } else { 0.0 };
            let mono_source = matches!(p.input, InputSel::Mono(_)) && t.kind == TrackKind::Audio;
            let (tl, tr) = if tap && !iso.map(|i| i.fader).unwrap_or(true) {
                (1.0, 1.0)
            } else if mono_source {
                // -3 dB compensated pan law: unity in the centre.
                let a = (p.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
                (a.cos() * std::f32::consts::SQRT_2 * vol, a.sin() * std::f32::consts::SQRT_2 * vol)
            } else {
                // Balance for stereo sources.
                let pan = p.pan.clamp(-1.0, 1.0);
                ((1.0 - pan.max(0.0)) * vol, (1.0 + pan.min(0.0)) * vol)
            };
            let (mut gl, mut gr) = if snap_gains { (tl, tr) } else { t.gains };
            let (mut pl, mut pr) = (0.0f32, 0.0f32);
            // A side-chain key leaves as the signal leaves the plug-ins, before the
            // compensation delay, with the fader applied below.
            if t.keyed {
                t.key_l[..n].copy_from_slice(l);
                t.key_r[..n].copy_from_slice(r);
            }
            if let (true, Some(d)) = (delayed && pdc.output > 0, &t.delay) {
                d.read(pdc.output as usize, l, r);
            }
            let (dl, dr) = match t.out_idx {
                // An isolated render captures the tap track, and anything feeding a stack
                // outside the render, straight off the bus.
                Some(j) if j > ti && !tap && iso.map(|i| i.feeds(role, tail[j - ti - 1].id)).unwrap_or(true) => {
                    let o = &mut *tail[j - ti - 1];
                    (&mut o.sum_l[..n], &mut o.sum_r[..n])
                }
                // A track that only keys a side-chain in an isolated render isn't heard.
                _ if role == Some(Role::Key) => (&mut self.tap_l[..n], &mut self.tap_r[..n]),
                _ => (&mut self.bus_l[..n], &mut self.bus_r[..n]),
            };
            for i in 0..n {
                gl += (tl - gl) * smooth;
                gr += (tr - gr) * smooth;
                self.gain_l[i] = gl;
                self.gain_r[i] = gr;
                l[i] *= gl;
                r[i] *= gr;
                pl = pl.max(l[i].abs());
                pr = pr.max(r[i].abs());
                dl[i] += l[i];
                dr[i] += r[i];
            }
            t.gains = (gl, gr);
            t.meter.feed(pl, pr);
            if t.keyed {
                for i in 0..n {
                    t.key_l[i] *= self.gain_l[i];
                    t.key_r[i] *= self.gain_r[i];
                }
            }
            // Post-fader sends take what the fader and pan left (already silent when muted).
            // One at another delay than the output reads its own tap and applies the same
            // fader gains.
            for k in 0..SEND_SLOTS {
                if let (Some(s), Some(j)) = (p.sends[k], t.send_idx[k]) {
                    if !s.pre_fader && j > ti && iso.map(|i| i.feeds(role, tail[j - ti - 1].id)).unwrap_or(true) {
                        match &t.delay {
                            Some(d) if delayed && pdc.sends[k] != pdc.output => {
                                let (sl, sr) = (&mut self.tap_l[..n], &mut self.tap_r[..n]);
                                d.read(pdc.sends[k] as usize, sl, sr);
                                for i in 0..n {
                                    sl[i] *= self.gain_l[i];
                                    sr[i] *= self.gain_r[i];
                                }
                                mix_send(&mut tail[j - ti - 1], sl, sr, &mut t.send_gains[k], s.gain, smooth);
                            }
                            _ => mix_send(&mut tail[j - ti - 1], l, r, &mut t.send_gains[k], s.gain, smooth),
                        }
                    }
                }
            }
        }

        // Isolated renders are captured before the Stereo Out strip.
        let master_fx = self.isolate.is_none();
        let (bl, br) = (&mut self.bus_l[..n], &mut self.bus_r[..n]);
        if master_fx {
            self.master_eq_state.process(&self.master_eq, bl, br);
        }
        if self.analyzer_target == Some(MASTER_ID) {
            for i in 0..n {
                let _ = self.analyzer_tx.push(0.5 * (bl[i] + br[i]));
            }
        }
        for ins in self.master_inserts.iter_mut().flatten().filter(|_| master_fx) {
            if !ins.bypass {
                match &mut ins.plugin {
                    Plugin::Builtin(b) => b.process(bl, br, key_signal(&self.tracks, b.key_idx, None, n), &fx_context(&transport)),
                    p => p.process(bl, br, &[], &transport),
                }
                check_output(&self.status, ins, bl, br);
            }
        }
        let master_target = if master_fx { self.master_volume } else { 1.0 };
        let mut g = if snap_gains { master_target } else { self.master_gain };
        let (mut pl, mut pr) = (0.0f32, 0.0f32);
        for i in 0..n {
            g += (master_target - g) * smooth;
            bl[i] *= g;
            br[i] *= g;
            pl = pl.max(bl[i].abs());
            pr = pr.max(br[i].abs());
        }
        self.master_gain = g;
        self.status.master.feed(pl, pr);
        self.snap_gains = false;

        // The click goes straight to the output, after the Stereo Out fader, as in Logic. The
        // count-in always clicks; otherwise the metronome follows its settings.
        let cp = self.click_params;
        // The music reaches the output `pdc.total` late; the click waits for it.
        let pos = pos - self.pdc.total as i64;
        let counting_in = recording && self.count_in && pos < punch;
        let metro = self.metronome && if recording { cp.while_recording && !cp.only_count_in } else { cp.while_playing };
        let clicking = playing && (metro || counting_in);
        let div = cp.division.clamp(1, 4);
        let map = &*self.tempo;
        // The clicks follow the tempo map's beats (and their subdivisions); the count-in
        // instead counts back from the punch at the tempo and signature in effect there.
        let count_in = CountIn::at(map, punch, div);
        let mut tick = if counting_in { None } else { Some(map.grid_at_or_after(map.beats_at(pos as f64), div)) };
        let mut k = if counting_in { count_in.first_at_or_after(pos) } else { 0 };
        let click_pos = |tick: &mut Option<GridPoint>, k: i64| -> i64 {
            if let Some(g) = tick {
                return g.pos.round() as i64;
            }
            let at = count_in.pos(k);
            if at < punch {
                return at;
            }
            // The count-in hands over to the map's grid at the punch, if the metronome plays on.
            let g = map.grid_at_or_after(map.beats_at(punch as f64), div);
            *tick = Some(g);
            if metro {
                g.pos.round() as i64
            } else {
                i64::MAX
            }
        };
        let mut next = if clicking { click_pos(&mut tick, k) } else { i64::MAX };
        let srf = sr as f32;
        for i in 0..n {
            if pos + i as i64 >= next {
                let (down, on_beat) = match &tick {
                    Some(g) => (cp.accent && g.downbeat(), g.on_beat()),
                    None => (cp.accent && count_in.downbeat(k), count_in.on_beat(k)),
                };
                let (freq, amp) = if down {
                    (cp.bar_freq, cp.level)
                } else if on_beat {
                    (cp.beat_freq, cp.level * 0.64)
                } else {
                    (cp.beat_freq, cp.level * 0.36)
                };
                self.click = Click::start(cp.sound, freq, amp, srf);
                match &mut tick {
                    Some(g) => *g = map.grid_after(g),
                    None => k += 1,
                }
                next = click_pos(&mut tick, k);
            }
            if self.click.remaining > 0 {
                let s = self.click.next(srf);
                self.bus_l[i] += s;
                self.bus_r[i] += s;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At an absurd sample rate every callback is "late", like a machine that can't keep up:
    /// the engine must notice, report it and go quiet instead of grinding on.
    #[test]
    fn a_sustained_overload_sheds_load() {
        let (_tx, rx) = rtrb::RingBuffer::new(64);
        let (gtx, _grx) = rtrb::RingBuffer::new(64);
        let (_live_tx, live_rx) = rtrb::RingBuffer::new(16);
        let (rec_tx, _rec_rx) = rtrb::RingBuffer::new(16);
        let (an_tx, _an_rx) = rtrb::RingBuffer::new(16);
        let status = Arc::new(EngineStatus::default());
        let mut e = Engine::new(1e12, rx, gtx, status.clone(), live_rx, rec_tx, an_tx);
        let mut out = vec![1.0f32; 512 * 2];
        let started = Instant::now();
        while status.overloads.load(Ordering::Relaxed) == 0 && started.elapsed().as_secs() < 5 {
            e.process(&[], 0, &mut out, 2);
        }
        assert_eq!(status.overloads.load(Ordering::Relaxed), 1);
        out.fill(1.0);
        e.process(&[], 0, &mut out, 2);
        assert!(out.iter().all(|&s| s == 0.0), "the next callback is silence");
        assert!(e.shedding > 0);
    }

    #[test]
    fn contain_silences_nans_and_clamps_runaways() {
        let mut l = vec![0.5, f32::NAN, 1e30, -f32::INFINITY];
        let mut r = vec![-0.25; 4];
        assert!(contain(&mut l, &mut r));
        assert_eq!(l, [0.5, 0.0, RUNAWAY, -RUNAWAY]);
        assert!(!contain(&mut l, &mut r), "contained audio passes untouched");
        assert_eq!(r, [-0.25; 4]);
    }

    struct Rig {
        engine: Engine,
        tx: rtrb::Producer<Cmd>,
        _keep: (rtrb::Consumer<Garbage>, rtrb::Producer<[u8; 3]>, rtrb::Consumer<(i64, [u8; 3])>, rtrb::Consumer<f32>),
    }

    fn rig() -> Rig {
        let (tx, rx) = rtrb::RingBuffer::new(64);
        let (gtx, grx) = rtrb::RingBuffer::new(64);
        let (live_tx, live_rx) = rtrb::RingBuffer::new(16);
        let (rec_tx, rec_rx) = rtrb::RingBuffer::new(16);
        let (an_tx, an_rx) = rtrb::RingBuffer::new(16);
        let engine = Engine::new(48000.0, rx, gtx, Arc::new(EngineStatus::default()), live_rx, rec_tx, an_tx);
        Rig { engine, tx, _keep: (grx, live_tx, rec_rx, an_rx) }
    }

    /// Plays one second (two bars at 480 BPM) with the metronome on and returns the peak level
    /// and the number of clicks heard.
    fn render(c: ClickSettings, recording: bool) -> (f32, usize) {
        let mut r = rig();
        r.tx.push(Cmd::Tempo(Arc::new(TempoMap::constant(48000.0, 480.0, 4, 4)))).ok().unwrap();
        r.tx.push(Cmd::Metronome { on: true, count_in: false }).ok().unwrap();
        r.tx.push(Cmd::Click(ClickParams::from_settings(&c))).ok().unwrap();
        r.tx.push(if recording { Cmd::Record { from: 0, punch: 0 } } else { Cmd::Play { from: 0 } }).ok().unwrap();
        let mut out = vec![0.0f32; 48000 * 2];
        r.engine.process(&[], 0, &mut out, 2);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        // A click starts wherever the output wakes up after at least 5 ms of silence.
        let mut clicks = 0;
        let mut quiet = usize::MAX;
        for f in out.chunks(2) {
            if f[0].abs() > 1e-4 {
                if quiet > 240 {
                    clicks += 1;
                }
                quiet = 0;
            } else {
                quiet = quiet.saturating_add(1);
            }
        }
        (peak, clicks)
    }

    #[test]
    fn default_click_matches_the_original_level() {
        let (peak, clicks) = render(ClickSettings::default(), false);
        assert!((peak - 0.5).abs() < 0.03, "peak {peak}");
        assert_eq!(clicks, 8);
    }

    #[test]
    fn click_volume_scales_the_level() {
        let (loud, _) = render(ClickSettings::default(), false);
        let (quiet, _) = render(ClickSettings { volume_db: -12.0, ..Default::default() }, false);
        let db = 20.0 * (quiet / loud).log10();
        assert!((db + 12.0).abs() < 0.5, "{db} dB");
    }

    #[test]
    fn accent_off_levels_the_downbeat() {
        let (peak, _) = render(ClickSettings { accent: false, ..Default::default() }, false);
        assert!((peak - 0.32).abs() < 0.03, "peak {peak}");
    }

    #[test]
    fn click_follows_play_and_record_switches() {
        let (p, _) = render(ClickSettings { while_playing: false, ..Default::default() }, false);
        assert_eq!(p, 0.0);
        let (p, _) = render(ClickSettings { while_playing: false, ..Default::default() }, true);
        assert!(p > 0.1);
        let (p, _) = render(ClickSettings { while_recording: false, ..Default::default() }, true);
        assert_eq!(p, 0.0);
        let (p, _) = render(ClickSettings { only_count_in: true, ..Default::default() }, true);
        assert_eq!(p, 0.0);
    }

    #[test]
    fn click_waits_for_compensated_music() {
        let first = |total: u32| {
            let mut r = rig();
            r.tx.push(Cmd::Metronome { on: true, count_in: false }).ok().unwrap();
            r.tx.push(Cmd::PdcGlobal(PdcGlobal { total, low_latency: false })).ok().unwrap();
            r.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
            let mut out = vec![0.0f32; 4800 * 2];
            r.engine.process(&[], 0, &mut out, 2);
            out.chunks(2).position(|f| f[0].abs() > 1e-6)
        };
        let (now, later) = (first(0).unwrap(), first(1000).unwrap());
        assert!(now < 4, "{now}");
        assert_eq!(later - now, 1000);
    }

    #[test]
    fn divisions_add_clicks_and_every_sound_is_audible() {
        let (_, clicks) = render(ClickSettings { division: 2, ..Default::default() }, false);
        assert_eq!(clicks, 16);
        for sound in [ClickSound::Classic, ClickSound::Click, ClickSound::Woodblock, ClickSound::Beep] {
            let (peak, clicks) = render(ClickSettings { sound, ..Default::default() }, false);
            assert!(peak > 0.15 && peak < 1.0, "{sound:?} peak {peak}");
            assert_eq!(clicks, 8, "{sound:?}");
        }
    }
    /// Frames where a click starts (output waking up after 5 ms of silence) and its peak. A
    /// click's first sample is silent (its sine starts at 0), so the frame before is reported.
    fn onsets(out: &[f32]) -> Vec<(usize, f32)> {
        let mut found: Vec<(usize, f32)> = Vec::new();
        let mut quiet = usize::MAX;
        for (i, f) in out.chunks(2).enumerate() {
            if f[0].abs() > 1e-4 {
                if quiet > 240 {
                    found.push((i.saturating_sub(1), 0.0));
                }
                if let Some(last) = found.last_mut() {
                    last.1 = last.1.max(f[0].abs());
                }
                quiet = 0;
            } else {
                quiet = quiet.saturating_add(1);
            }
        }
        found
    }

    fn play_clicks(map: TempoMap, from: i64, punch: Option<i64>, frames: usize) -> Vec<(usize, f32)> {
        let mut r = rig();
        r.tx.push(Cmd::Tempo(Arc::new(map))).ok().unwrap();
        r.tx.push(Cmd::Metronome { on: true, count_in: punch.is_some() }).ok().unwrap();
        r.tx.push(match punch {
            Some(punch) => Cmd::Record { from, punch },
            None => Cmd::Play { from },
        })
        .ok()
        .unwrap();
        let mut out = vec![0.0f32; frames * 2];
        r.engine.process(&[], 0, &mut out, 2);
        onsets(&out)
    }

    #[test]
    fn clicks_follow_tempo_and_signature_changes() {
        use crate::tempo::{SigEvent, TempoEvent};
        // 240 BPM (12000 samples a quarter), 480 BPM from 1 s; 3/4 from bar 2.
        let map = TempoMap::new(
            48000.0,
            &[TempoEvent { pos: 0, bpm: 240.0, ramp: false }, TempoEvent { pos: 48000, bpm: 480.0, ramp: false }],
            &[SigEvent { bar: 2, num: 3, den: 4 }],
            240.0,
            (4, 4),
        );
        let clicks = play_clicks(map, 0, None, 96000);
        let at: Vec<usize> = clicks.iter().map(|c| c.0).collect();
        // Bar 1: four quarters of 12000. Bar 2 starts at 48000; from there a quarter is 6000.
        let want: Vec<usize> = (0..4).map(|k| k * 12000).chain((0..8).map(|k| 48000 + k * 6000)).collect();
        assert_eq!(at, want);
        // Downbeats (accented) at bars 1, 2 (48000), 3 (66000) and 4 (84000).
        let loud: Vec<usize> = clicks.iter().filter(|c| c.1 > 0.4).map(|c| c.0).collect();
        assert_eq!(loud, vec![0, 48000, 66000, 84000]);
    }

    #[test]
    fn count_in_uses_the_tempo_at_the_punch() {
        use crate::tempo::TempoEvent;
        // 480 BPM until the punch at 2 s, then 240 BPM: a 4/4 count-in bar is 48000 samples
        // at the punch's tempo, whatever the tempo before it.
        let map = TempoMap::new(48000.0, &[TempoEvent { pos: 0, bpm: 480.0, ramp: false }, TempoEvent { pos: 96000, bpm: 240.0, ramp: false }], &[], 480.0, (4, 4));
        let clicks = play_clicks(map, 48000, Some(96000), 48000 + 24000);
        let at: Vec<usize> = clicks.iter().map(|c| c.0 + 48000).collect();
        assert_eq!(at, vec![48000, 60000, 72000, 84000, 96000, 108000]);
        assert!(clicks[0].1 > 0.4 && clicks[1].1 < 0.4 && clicks[4].1 > 0.4);
    }

    #[test]
    fn plugins_get_the_position_in_the_map() {
        use crate::tempo::{SigEvent, TempoEvent};
        let map = TempoMap::new(48000.0, &[TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: 96000, bpm: 60.0, ramp: true }, TempoEvent { pos: 192000, bpm: 120.0, ramp: false }], &[SigEvent { bar: 2, num: 7, den: 8 }], 120.0, (4, 4));
        let mut r = rig();
        r.tx.push(Cmd::Tempo(Arc::new(map))).ok().unwrap();
        r.tx.push(Cmd::Locate(144000)).ok().unwrap();
        r.engine.process(&[], 0, &mut [], 0);
        let t = r.engine.transport();
        // Halfway through the 60 → 120 ramp: 90 BPM, and 4 + 1 s × 75 BPM / 60 quarters.
        assert!((t.tempo - 90.0).abs() < 1e-9);
        assert!((t.position_quarters - 5.25).abs() < 1e-9);
        assert_eq!((t.sig_num, t.sig_den), (7, 8));
        assert!((t.bar_start_quarters - 4.0).abs() < 1e-9);
    }
}
