//! The real-time mixing graph. Everything in `Engine::process` runs on the audio thread and must
//! not block or allocate; structural changes arrive as `Cmd`s and replaced objects are shipped
//! back as `Garbage` to be dropped elsewhere.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender};
use vst3_host::{MidiEvent, PluginProcessor, TransportInfo};

use crate::automation::{value_at, AutoTarget, LaneDsp};
use crate::clip::Clip;
use crate::eq::{EqCoeffs, EqState};
use crate::model::{AutoMode, CcEvent, CcKind, Id, InputSel, Note, TrackKind, CC_KINDS, INSERT_SLOTS, MASTER_ID, SEND_SLOTS};
use crate::prefs::{ClickSettings, ClickSound};
use crate::synth::BasicSynth;

mod offline;
pub use offline::Isolate;
use offline::Role;
mod pdc;
pub use pdc::{DelayLine, PdcGlobal, PdcParams, INSTRUMENT_BIT};

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
    /// Measured device latencies (µs) and how many input frames sit in the duplex ring.
    pub input_latency_us: AtomicU64,
    pub output_latency_us: AtomicU64,
    pub ring_frames: AtomicU64,
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
            input_latency_us: AtomicU64::new(0),
            output_latency_us: AtomicU64::new(0),
            ring_frames: AtomicU64::new(0),
        }
    }
}

pub enum Plugin {
    Vst(Box<PluginProcessor>),
    Synth(Box<BasicSynth>),
}

impl Plugin {
    fn process(&mut self, l: &mut [f32], r: &mut [f32], midi: &[MidiEvent], t: &TransportInfo) {
        match self {
            Plugin::Vst(p) => p.process(l, r, midi, t),
            Plugin::Synth(s) => s.process(l, r, midi),
        }
    }

    /// Queues an automation value for the next block (the built-in synth has no parameters).
    fn set_param(&mut self, id: u32, offset: i32, value: f64) {
        if let Plugin::Vst(p) = self {
            p.queue_param(id, offset, value);
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
    Tempo { bpm: f64, num: u32, den: u32 },
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
}

pub enum Garbage {
    Track(Box<TrackDsp>),
    Insert(InsertDsp),
    Regions(Arc<Vec<RegionDsp>>),
    Sink(rtrb::Producer<f32>),
    Ids(Vec<Id>),
    Automation(Arc<Vec<LaneDsp>>),
    Delay(Box<DelayLine>),
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
    tempo: f64,
    num: u32,
    den: u32,
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
    cmd_rx: Receiver<Cmd>,
    garbage: Sender<Garbage>,
    pub status: Arc<EngineStatus>,
    live_midi: rtrb::Consumer<[u8; 3]>,
    midi_rec: rtrb::Producer<(i64, [u8; 3])>,
    live: Vec<MidiEvent>,
    bus_l: Vec<f32>,
    bus_r: Vec<f32>,
    inputs: Vec<Vec<f32>>,
    load: f32,
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

/// Fade gain for `x` from 0 (silent) to 1 (full): a quarter sine, smooth at both ends of the
/// level change that matters most to the ear.
fn fade_curve(x: f32) -> f32 {
    (x.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin()
}

/// `t` moved to timeline position `pos`, for plug-ins on a strip whose audio lags the playhead.
fn transport_at(t: &TransportInfo, pos: i64) -> TransportInfo {
    let quarters_per_beat = 4.0 / t.sig_den.max(1) as f64;
    let spb = t.sample_rate * 60.0 / t.tempo.max(1.0) * quarters_per_beat;
    let beats = pos as f64 / spb;
    let bar = (beats / t.sig_num.max(1) as f64).floor();
    TransportInfo {
        position_samples: pos,
        position_quarters: beats * quarters_per_beat,
        bar_start_quarters: bar * t.sig_num.max(1) as f64 * quarters_per_beat,
        ..*t
    }
}

/// Mixes the audio regions that overlap `[pos, pos + l.len())` into `l` and `r`.
/// Mixes the audio regions playing in this block into `l`/`r`, except inside `quiet` (timeline
/// samples, start inclusive), where an Autopunch recording is replacing them.
fn play_audio(regions: &[RegionDsp], pos: i64, l: &mut [f32], r: &mut [f32], quiet: (i64, i64)) {
    let block_end = pos + l.len() as i64;
    for reg in regions {
        let RegionDsp::Audio { start, length, offset, gain, clip, fade_in, fade_out } = reg else { continue };
        let (rs, re) = (*start, *start + *length);
        if re <= pos || rs >= block_end {
            continue;
        }
        let from = rs.max(pos);
        let to = re.min(block_end);
        let ch = &clip.channels;
        let frames = clip.frames() as i64;
        for tl in from..to {
            let src = tl - rs + offset;
            if src < 0 || src >= frames || (tl >= quiet.0 && tl < quiet.1) {
                continue;
            }
            let i = (tl - pos) as usize;
            let mut g = *gain;
            if tl < fade_in.0 + fade_in.1 && fade_in.1 > 0 {
                g *= fade_curve((tl - fade_in.0) as f32 / fade_in.1 as f32);
            }
            if tl >= fade_out.0 && fade_out.1 > 0 {
                g *= fade_curve((fade_out.0 + fade_out.1 - tl) as f32 / fade_out.1 as f32);
            }
            let a = ch[0][src as usize] * g;
            let b = if ch.len() > 1 { ch[1][src as usize] * g } else { a };
            l[i] += a;
            r[i] += b;
        }
    }
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
        cmd_rx: Receiver<Cmd>,
        garbage: Sender<Garbage>,
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
            tempo: 120.0,
            num: 4,
            den: 4,
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
            garbage,
            status,
            live_midi,
            midi_rec,
            live: Vec::with_capacity(MAX_MIDI),
            bus_l: vec![0.0; MAX_BLOCK],
            bus_r: vec![0.0; MAX_BLOCK],
            inputs: (0..MAX_INPUTS).map(|_| vec![0.0; MAX_BLOCK]).collect(),
            load: 0.0,
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
        // The channel is bounded; if it is somehow full, leaking beats blocking the audio thread.
        if let Err(e) = self.garbage.try_send(g) {
            std::mem::forget(e.into_inner());
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
                self.pos = from;
                self.playing = true;
                self.recording = false;
                self.resend_automation();
                self.chase_all_cc();
            }
            Cmd::Record { from, punch } => {
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
                self.pos = p;
                self.flush_all_notes();
                self.resend_automation();
            }
            Cmd::Tempo { bpm, num, den } => {
                self.tempo = bpm.clamp(5.0, 990.0);
                self.num = num.max(1);
                self.den = den.max(1);
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
                    self.track_mut(track).and_then(|t| t.inserts.get_mut(slot).and_then(|s| std::mem::replace(s, insert)))
                };
                if let Some(o) = old {
                    self.discard(Garbage::Insert(o));
                }
            }
            Cmd::Instrument { track, insert } => {
                let old = self.track_mut(track).and_then(|t| std::mem::replace(&mut t.instrument, insert));
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
        }
    }

    fn samples_per_beat(&self) -> f64 {
        self.sample_rate * 60.0 / self.tempo * (4.0 / self.den as f64)
    }

    /// `input` holds `frames * in_ch` interleaved samples; `output` holds `frames * out_ch`.
    pub fn process(&mut self, input: &[f32], in_ch: usize, output: &mut [f32], out_ch: usize) {
        let started = Instant::now();
        let frames = if out_ch == 0 { 0 } else { output.len() / out_ch };

        while let Ok(cmd) = self.cmd_rx.try_recv() {
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
            let l = (started.elapsed().as_secs_f64() / budget) as f32;
            self.load = self.load * 0.9 + l * 0.1;
            self.status.cpu_load.store(self.load.to_bits(), Ordering::Relaxed);
        }
    }

    fn transport(&self) -> TransportInfo {
        let t = TransportInfo {
            playing: self.playing,
            recording: self.recording,
            sample_rate: self.sample_rate,
            tempo: self.tempo,
            sig_num: self.num as i32,
            sig_den: self.den as i32,
            position_samples: self.pos,
            cycle: None,
            ..Default::default()
        };
        let mut t = transport_at(&t, self.pos);
        if self.cycle.0 {
            let spq = self.samples_per_beat() * self.den as f64 / 4.0;
            t.cycle = Some((self.cycle.1 as f64 / spq, self.cycle.2 as f64 / spq));
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
            let (head, tail) = self.tracks.split_at_mut(ti + 1);
            let t = &mut *head[ti];
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
            let transport = if pdc.input_time > 0 { transport_at(&transport, fx_at) } else { transport };

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
                            let first = (capture_from - pos).max(0) as usize;
                            let last = (capture_end - pos).min(n as i64) as usize;
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
                    ins.plugin.process(l, r, &[], &transport);
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
                Some(_) => true,
                None => !p.mute && (!any_solo || p.solo),
            };
            // Pre-fader sends tap the signal after the plug-ins, before the fader and pan. Mute
            // and solo still silence them, as in Logic.
            for k in 0..SEND_SLOTS {
                match (p.sends[k], t.send_idx[k]) {
                    // An isolated render leaves out auxes it doesn't play; their buses must
                    // not fill up meanwhile, or they'd burst out when playback resumes.
                    (Some(s), Some(j)) if j > ti && iso.map(|i| i.role(tail[j - ti - 1].id) != Role::Skip).unwrap_or(true) => {
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
            if let (true, Some(d)) = (delayed && pdc.output > 0, &t.delay) {
                d.read(pdc.output as usize, l, r);
            }
            let (dl, dr) = match t.out_idx {
                // An isolated render captures the tap track, and anything feeding a stack
                // outside the render, straight off the bus.
                Some(j) if j > ti && !tap && iso.map(|i| i.role(tail[j - ti - 1].id) != Role::Skip).unwrap_or(true) => {
                    let o = &mut *tail[j - ti - 1];
                    (&mut o.sum_l[..n], &mut o.sum_r[..n])
                }
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
            // Post-fader sends take what the fader and pan left (already silent when muted).
            // One at another delay than the output reads its own tap and applies the same
            // fader gains.
            for k in 0..SEND_SLOTS {
                if let (Some(s), Some(j)) = (p.sends[k], t.send_idx[k]) {
                    if !s.pre_fader && j > ti && iso.map(|i| i.role(tail[j - ti - 1].id) != Role::Skip).unwrap_or(true) {
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
                ins.plugin.process(bl, br, &[], &transport);
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
        let div = cp.division.clamp(1, 4) as i64;
        let step = self.samples_per_beat() / div as f64;
        let per_bar = self.num as i64 * div;
        let mut k = (pos as f64 / step).ceil() as i64;
        let mut next = if clicking { (k as f64 * step).round() as i64 } else { i64::MAX };
        let srf = sr as f32;
        for i in 0..n {
            if pos + i as i64 >= next {
                let down = cp.accent && k.rem_euclid(per_bar) == 0;
                let on_beat = k.rem_euclid(div) == 0;
                let (freq, amp) = if down {
                    (cp.bar_freq, cp.level)
                } else if on_beat {
                    (cp.beat_freq, cp.level * 0.64)
                } else {
                    (cp.beat_freq, cp.level * 0.36)
                };
                self.click = Click::start(cp.sound, freq, amp, srf);
                k += 1;
                next = (k as f64 * step).round() as i64;
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

    struct Rig {
        engine: Engine,
        tx: Sender<Cmd>,
        _keep: (Receiver<Garbage>, rtrb::Producer<[u8; 3]>, rtrb::Consumer<(i64, [u8; 3])>, rtrb::Consumer<f32>),
    }

    fn rig() -> Rig {
        let (tx, rx) = crossbeam_channel::bounded(64);
        let (gtx, grx) = crossbeam_channel::bounded(64);
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
        r.tx.send(Cmd::Tempo { bpm: 480.0, num: 4, den: 4 }).unwrap();
        r.tx.send(Cmd::Metronome { on: true, count_in: false }).unwrap();
        r.tx.send(Cmd::Click(ClickParams::from_settings(&c))).unwrap();
        r.tx.send(if recording { Cmd::Record { from: 0, punch: 0 } } else { Cmd::Play { from: 0 } }).unwrap();
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
            r.tx.send(Cmd::Metronome { on: true, count_in: false }).unwrap();
            r.tx.send(Cmd::PdcGlobal(PdcGlobal { total, low_latency: false })).unwrap();
            r.tx.send(Cmd::Play { from: 0 }).unwrap();
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
}
