//! Track On/Off on the audio thread: a track that is off runs nothing, so it costs close to
//! nothing, unlike a muted one, whose regions, instrument and plug-ins all keep running.
//!
//! Switched off, a track first fades through its fader like a muted one (no click), then goes
//! dormant: its plug-ins are told to stop and cleared, its Channel EQ, delay line and side-chain
//! keys are emptied, and from then on each block only empties what feeders wrote into it. Its
//! plug-ins stay loaded. Switched back on, it starts clean: notes are released and chased as
//! after a locate, automation is sent afresh, and the source fades in, ahead of the plug-ins and
//! the delay line, so audio emerging later from a latent path doesn't start with a jump either.
//!
//! The session leaves the track's latency out of delay compensation while it is off, and the
//! other tracks glide to their new delays as for any other latency change (see `session/pdc.rs`).

use super::*;

/// Below about -80 dB a fading track is quiet enough to stop.
const QUIET: f32 = 1e-4;
/// How long the source fades in when a track comes back on.
const RESUME_SECS: f64 = 0.005;

/// The length of the source's fade-in, in frames.
pub(super) fn resume_frames(sr: f64) -> u32 {
    (RESUME_SECS * sr).max(1.0) as u32
}

/// Applies the rest of a fade-in of `len` frames to a block.
pub(super) fn fade_in(left: &mut u32, len: u32, l: &mut [f32], r: &mut [f32]) {
    for (a, b) in l.iter_mut().zip(r.iter_mut()) {
        if *left == 0 {
            return;
        }
        let g = 1.0 - *left as f32 / len as f32;
        *a *= g;
        *b *= g;
        *left -= 1;
    }
}

impl Plugin {
    /// Stops the plug-in and clears its state, for a track going off. A VST3 stays active
    /// (`setActive` belongs to the main thread): see `PluginProcessor::suspend`.
    fn sleep(&mut self) {
        match self {
            Plugin::Vst(p) => p.suspend(),
            Plugin::Instrument(i) => i.sleep(),
            Plugin::Builtin(b) => b.sleep(),
        }
    }

    /// A VST3's parameter edits only reach its processor in `process`. Its editor still works
    /// on an Off track, so while the user moves something there the edits go through, in a block
    /// of silence nobody hears: its saved state and what it plays when back on must have them.
    fn pass_edits(&mut self, l: &mut [f32], r: &mut [f32], t: &TransportInfo) {
        if let Plugin::Vst(p) = self {
            if p.has_edits() {
                l.fill(0.0);
                r.fill(0.0);
                p.process(l, r, &[], t);
                p.suspend();
            }
        }
    }
}

impl Plugin {
    /// `pass_edits` for a plug-in the engine skips on a track that's playing (bypassed, or left
    /// out by Low Latency Mode): the track's buffers are in use, so it brings its own.
    pub(super) fn pass_edits_aside(&mut self, t: &TransportInfo) {
        if let Plugin::Vst(p) = self {
            p.pass_edits(t);
        }
    }
}

impl TrackDsp {
    /// Whether the fader and the pre-fader sends have finished fading (post-fader sends follow
    /// the fader).
    pub(super) fn faded_out(&self) -> bool {
        let sends = self.params.sends.iter().zip(&self.send_gains).all(|(s, g)| !s.is_some_and(|s| s.pre_fader) || g.abs() < QUIET);
        self.gains.0.abs() < QUIET && self.gains.1.abs() < QUIET && sends
    }

    /// One block of an Off track: stops it if it was running, then does nothing but empty its
    /// input, since tracks that send to an Off aux keep writing into it.
    pub(super) fn sleep_block(&mut self, n: usize, transport: &TransportInfo) {
        if !self.dormant {
            self.sleep();
        }
        if matches!(self.kind, TrackKind::Summing | TrackKind::Aux) {
            self.sum_l[..n].fill(0.0);
            self.sum_r[..n].fill(0.0);
        }
        let (l, r) = (&mut self.l[..n], &mut self.r[..n]);
        for ins in self.inserts.iter_mut().flatten().chain(self.instrument.iter_mut()) {
            ins.plugin.pass_edits(l, r, transport);
        }
    }

    fn sleep(&mut self) {
        self.dormant = true;
        self.resume_left = 0;
        self.gains = (0.0, 0.0);
        self.send_gains = [0.0; SEND_SLOTS];
        self.eq_state.reset();
        for ins in self.inserts.iter_mut().flatten().chain(self.instrument.iter_mut()) {
            ins.plugin.sleep();
        }
        if let Some(d) = &mut self.delay {
            d.clear();
        }
        self.feed.clear();
        // What an effect keyed from this track reads: silence, until it's back on.
        for key in [&mut self.key_l, &mut self.key_r, &mut self.pre_key_l, &mut self.pre_key_r] {
            key.fill(0.0);
        }
        self.flush_notes = true;
        self.chase_cc = true;
        self.auto_sent = [f32::NAN; MAX_AUTO_SENT];
    }

    /// Back on: `fade` frames of fade-in. The delay line holds only silence, so its taps go
    /// straight to their delays.
    pub(super) fn wake(&mut self, fade: u32) {
        self.dormant = false;
        self.resume_left = fade;
        self.pdc_taps = pdc::PdcTaps::at(&self.pdc);
        // The flush set when it went off releases these with the notes it played itself.
        self.adopt_live_notes();
    }

    /// The next flush releases the keys held on this track with the notes it played itself.
    pub(super) fn adopt_live_notes(&mut self) {
        for (sounding, held) in self.sounding.iter_mut().zip(&mut self.live_held) {
            *sounding |= std::mem::take(held);
        }
    }
}

/// Follows the notes played live on a track, so they can be released after it was off.
pub(super) fn note_held(held: &mut [bool; 128], m: [u8; 3]) {
    match m[0] & 0xF0 {
        0x90 if m[2] > 0 => held[m[1] as usize & 127] = true,
        0x80 | 0x90 => held[m[1] as usize & 127] = false,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{Effect, EffectInfo, FxContext, Key};
    use std::sync::atomic::AtomicUsize;

    const SR: f64 = 48000.0;
    const BLOCK: usize = 256;

    /// Counts its blocks and keeps the loudest key it heard, in statics, as `EffectInfo::new`
    /// is a plain function. Each test has its own `Probe` number, as tests may run in parallel.
    struct Probe(usize);
    static BLOCKS: [AtomicUsize; 6] = [const { AtomicUsize::new(0) }; 6];
    static RESETS: [AtomicUsize; 6] = [const { AtomicUsize::new(0) }; 6];
    static KEY: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];

    impl Effect for Probe {
        fn set_param(&mut self, _: u32, _: f32) {}
        fn param(&self, _: u32) -> f32 {
            0.0
        }
        fn process(&mut self, _: &mut [f32], _: &mut [f32], key: Key, _: &FxContext) {
            BLOCKS[self.0].fetch_add(1, Ordering::Relaxed);
            if let Some((l, _)) = key {
                let peak = l.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                KEY[self.0].store(peak.to_bits(), Ordering::Relaxed);
            }
        }
        fn reset(&mut self) {
            RESETS[self.0].fetch_add(1, Ordering::Relaxed);
        }
    }

    fn probe_info(k: usize) -> EffectInfo {
        let new: fn(f32) -> Box<dyn Effect> = match k {
            0 => |_| Box::new(Probe(0)),
            1 => |_| Box::new(Probe(1)),
            2 => |_| Box::new(Probe(2)),
            3 => |_| Box::new(Probe(3)),
            4 => |_| Box::new(Probe(4)),
            _ => |_| Box::new(Probe(5)),
        };
        EffectInfo { id: "test:probe", name: "Probe", category: "Utility", params: &[], meters: &[], sidechain: true, new }
    }

    fn probe(k: usize, key: Option<Id>) -> InsertDsp {
        InsertDsp { uid: 900 + k as Id, bypass: false, plugin: Plugin::Builtin(Box::new(BuiltinDsp::new(&probe_info(k), SR as f32, &[]))), key: KeyRoute::new(key, false) }
    }

    fn blocks(k: usize) -> usize {
        BLOCKS[k].load(Ordering::Relaxed)
    }

    fn params() -> TrackParams {
        TrackParams {
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            armed: false,
            monitor: false,
            input: InputSel::None,
            output: None,
            sends: [None; SEND_SLOTS],
            stack_gain: 1.0,
            stack_mute: false,
            folder: None,
            own_gain: 1.0,
            own_mute: false,
            bus_mute: false,
            vca_gain: 1.0,
            vca_mute: false,
            off: false,
            instrument_output: None,
        }
    }

    struct Rig {
        engine: Engine,
        tx: rtrb::Producer<Cmd>,
        _keep: (rtrb::Consumer<Garbage>, rtrb::Producer<[u8; 3]>, rtrb::Consumer<(i64, [u8; 3])>, rtrb::Consumer<f32>),
    }

    impl Rig {
        fn new() -> Rig {
            let (tx, rx) = rtrb::RingBuffer::new(256);
            let (gtx, grx) = rtrb::RingBuffer::new(256);
            let (live_tx, live_rx) = rtrb::RingBuffer::new(16);
            let (rec_tx, rec_rx) = rtrb::RingBuffer::new(16);
            let (an_tx, an_rx) = rtrb::RingBuffer::new(16);
            Rig { engine: Engine::new(SR, rx, gtx, Arc::new(EngineStatus::default()), live_rx, rec_tx, an_tx), tx, _keep: (grx, live_tx, rec_rx, an_rx) }
        }

        fn send(&mut self, c: Cmd) {
            self.tx.push(c).ok().unwrap();
        }

        /// A track playing a long 440 Hz sine (or nothing, for a bus).
        fn track(&mut self, id: Id, kind: TrackKind, p: TrackParams, tone: bool) -> Arc<Meter> {
            let meter = Arc::new(Meter::default());
            let mut t = TrackDsp::new(id, kind, p, meter.clone());
            if tone {
                t.set_regions(sine());
            }
            self.send(Cmd::AddTrack(Box::new(t), usize::MAX));
            meter
        }

        fn set(&mut self, id: Id, p: TrackParams) {
            self.send(Cmd::Params(id, p));
        }

        /// Plays `frames` in blocks, returning the left channel.
        fn run(&mut self, frames: usize) -> Vec<f32> {
            let mut out = Vec::new();
            let mut buf = vec![0.0f32; BLOCK * 2];
            for _ in 0..frames.div_ceil(BLOCK) {
                self.engine.process(&[], 0, &mut buf, 2);
                out.extend(buf.chunks(2).map(|f| f[0]));
            }
            out
        }
    }

    fn sine() -> Arc<Vec<RegionDsp>> {
        let len = SR as usize * 10;
        let x: Vec<f32> = (0..len).map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / SR as f32).sin()).collect();
        let clip = Arc::new(Clip::from_channels("sine.wav".into(), SR as u32, vec![x.clone(), x]));
        Arc::new(vec![RegionDsp::Audio { start: 0, length: len as i64, offset: 0, gain: 1.0, clip, fade_in: (0, 0), fade_out: (len as i64, 0), laws: Default::default() }])
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, s| m.max(s.abs()))
    }

    fn largest_step(x: &[f32]) -> f32 {
        x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    /// The sine's own largest step: 0.5 · 2π · 440 / 48000, plus a little.
    const SINE_STEP: f32 = 0.03;

    #[test]
    fn an_off_track_is_silent_and_runs_nothing() {
        let mut r = Rig::new();
        let meter = r.track(1, TrackKind::Audio, params(), true);
        r.send(Cmd::Insert { track: 1, slot: 0, insert: Some(probe(0, None)) });
        r.send(Cmd::Play { from: 0 });
        assert!(peak(&r.run(4800)) > 0.4);
        assert!(blocks(0) > 0);
        assert!(meter.take().0 > 0.4);

        r.set(1, TrackParams { off: true, ..params() });
        let fade = r.run(4800);
        assert!(peak(&fade[4800 - BLOCK..]) == 0.0, "quiet by the end of the fade");
        let (ran, resets) = (blocks(0), RESETS[0].load(Ordering::Relaxed));
        assert!(resets > 0, "its state was cleared");
        meter.take();
        let out = r.run(4800);
        assert_eq!(peak(&out), 0.0);
        assert_eq!(blocks(0), ran, "the plug-in isn't processed");
        assert_eq!(meter.take(), (0.0, 0.0), "the meter falls to nothing");
        assert!(r.engine.tracks[0].dormant);
    }

    #[test]
    fn switching_off_and_on_doesnt_click() {
        let mut r = Rig::new();
        r.track(1, TrackKind::Audio, params(), true);
        r.send(Cmd::Play { from: 0 });
        r.run(4800);
        r.set(1, TrackParams { off: true, ..params() });
        let going = r.run(9600);
        assert!(largest_step(&going) < SINE_STEP, "off: {}", largest_step(&going));
        assert!(r.engine.tracks[0].dormant);
        r.set(1, params());
        let back = r.run(9600);
        assert!(largest_step(&back) < SINE_STEP, "on: {}", largest_step(&back));
        assert!(peak(&back[4800..]) > 0.45, "it plays again");
        assert!(!r.engine.tracks[0].dormant);
    }

    #[test]
    fn audio_coming_back_through_a_delay_line_fades_in_too() {
        let mut r = Rig::new();
        r.track(1, TrackKind::Audio, params(), true);
        let delay = 1000;
        let pdc = PdcParams { output: delay, output_time: delay, ..Default::default() };
        r.send(Cmd::Pdc { track: 1, params: pdc, line: Some(Box::new(DelayLine::new(8192))), feed_line: None });
        r.send(Cmd::Play { from: 0 });
        r.run(4800);
        r.set(1, TrackParams { off: true, ..params() });
        r.run(9600);
        r.set(1, params());
        let back = r.run(9600);
        assert!(largest_step(&back) < SINE_STEP, "{}", largest_step(&back));
        assert_eq!(peak(&back[..delay as usize]), 0.0, "the line was emptied: no audio from before");
    }

    #[test]
    fn an_off_aux_passes_nothing_and_its_senders_keep_playing() {
        let mut r = Rig::new();
        let mut sends = [None; SEND_SLOTS];
        sends[0] = Some(SendParams { bus: 2, gain: 1.0, pre_fader: false });
        r.track(1, TrackKind::Audio, TrackParams { sends, volume: 0.0, ..params() }, true);
        r.track(2, TrackKind::Aux, params(), false);
        r.send(Cmd::Insert { track: 2, slot: 0, insert: Some(probe(1, None)) });
        let sender = TrackParams { sends, ..params() };
        r.set(1, sender);
        r.send(Cmd::Play { from: 0 });
        assert!(peak(&r.run(4800)[2400..]) > 0.9, "dry and send");

        r.set(2, TrackParams { off: true, ..params() });
        r.run(9600);
        let ran = blocks(1);
        let out = r.run(4800);
        assert!((peak(&out) - 0.5).abs() < 0.01, "the sender plays on alone: {}", peak(&out));
        assert_eq!(blocks(1), ran);

        // Nothing piled up at the aux's input meanwhile.
        r.set(2, params());
        let back = r.run(4800);
        assert!(peak(&back) < 1.02, "{}", peak(&back));
        assert!(largest_step(&back) < 2.0 * SINE_STEP, "{}", largest_step(&back));
    }

    #[test]
    fn a_key_from_an_off_track_is_silence() {
        let mut r = Rig::new();
        r.track(1, TrackKind::Audio, params(), true);
        r.track(2, TrackKind::Audio, params(), false);
        r.send(Cmd::Insert { track: 2, slot: 0, insert: Some(probe(2, Some(1))) });
        r.send(Cmd::Play { from: 0 });
        r.run(4800);
        assert!(f32::from_bits(KEY[2].load(Ordering::Relaxed)) > 0.4);
        r.set(1, TrackParams { off: true, ..params() });
        r.run(9600);
        let ran = blocks(2);
        r.run(BLOCK);
        assert!(blocks(2) > ran, "the keyed effect keeps running");
        assert_eq!(f32::from_bits(KEY[2].load(Ordering::Relaxed)), 0.0);
    }

    #[test]
    fn a_track_in_a_take_stays_on_until_the_take_ends() {
        let mut r = Rig::new();
        let armed = TrackParams { armed: true, input: InputSel::Mono(0), ..params() };
        r.track(1, TrackKind::Audio, armed, false);
        r.send(Cmd::Insert { track: 1, slot: 0, insert: Some(probe(3, None)) });
        let (sink, mut rec) = rtrb::RingBuffer::new(BLOCK * 64);
        r.send(Cmd::RecordSink { track: 1, sink: Some(sink) });
        r.send(Cmd::Record { from: 0, punch: 0 });
        r.run(BLOCK);
        r.set(1, TrackParams { off: true, ..armed });
        let before = rec.slots();
        r.run(9600);
        assert!(rec.slots() > before + 9000, "input is still captured");
        assert!(!r.engine.tracks[0].dormant);
        while rec.pop().is_ok() {}

        // Once the take is over, it goes off.
        r.send(Cmd::Stop);
        r.run(9600);
        assert!(r.engine.tracks[0].dormant);
    }

    #[test]
    fn a_new_off_track_never_runs_and_a_render_leaves_it_out() {
        let mut r = Rig::new();
        r.track(1, TrackKind::Audio, TrackParams { off: true, ..params() }, true);
        r.send(Cmd::Insert { track: 1, slot: 0, insert: Some(probe(4, None)) });
        let out = r.engine.render(0, 4800, 0, None, &mut |_| {});
        assert_eq!(peak(&out[0]), 0.0);
        assert_eq!(blocks(4), 0);
        // Back on, a render starts at full level from its first sample: no resume fade.
        r.set(1, params());
        let out = r.engine.render(0, 4800, 0, None, &mut |_| {});
        assert!(peak(&out[0][1..]) > 0.45);
        assert!(blocks(4) > 0);
        assert_eq!(r.engine.tracks[0].resume_left, 0);
    }

    #[test]
    fn a_note_held_when_a_track_goes_off_is_released_when_it_comes_back() {
        let mut t = TrackDsp::new(1, TrackKind::Instrument, params(), Arc::new(Meter::default()));
        note_held(&mut t.live_held, [0x90, 60, 100]);
        note_held(&mut t.live_held, [0x90, 62, 100]);
        note_held(&mut t.live_held, [0x90, 62, 0]);
        t.sleep();
        t.wake(0);
        assert!(t.flush_notes && t.sounding[60] && !t.sounding[62]);
    }

    #[test]
    fn the_resume_fade_ramps_from_silence_to_the_signal() {
        let (mut l, mut r) = (vec![1.0f32; 8], vec![1.0f32; 8]);
        let mut left = 4;
        fade_in(&mut left, 4, &mut l, &mut r);
        assert_eq!(l, [0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(left, 0);
    }
}
