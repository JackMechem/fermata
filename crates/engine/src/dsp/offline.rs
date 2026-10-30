//! Offline rendering (bounces and exports): runs the real graph faster than real time on the
//! calling thread, so plug-ins, stacks and the channel strips sound exactly as they play.
//!
//! The caller holds the engine lock for the whole render. The device callback only ever
//! `try_lock`s, so it outputs silence meanwhile, as Logic does during an offline bounce.

use super::*;

/// Renders only part of the graph: one track (with the subtracks that feed it) captured off
/// the bus before the Stereo Out strip, instead of the whole mix.
#[derive(Debug, Clone)]
pub struct Isolate {
    /// The tracks that play, each with whether it is heard. Every other track is skipped.
    /// Mute and solo come from here rather than the live settings, so a muted or un-soloed
    /// track still renders.
    pub tracks: Vec<(Id, bool)>,
    /// The track whose output is captured.
    pub tap: Id,
    /// Apply the tap track's own fader and pan (otherwise it is captured at unity, centred).
    pub fader: bool,
    /// Run the tap track's Channel EQ and Audio FX plug-ins. Its instrument always plays.
    pub effects: bool,
    /// Tracks outside the render that key a side-chain inside it (and whatever feeds them):
    /// they play so the key is right, but only into each other, never into the capture.
    pub keys: Vec<Id>,
}

/// What a track does in an isolated render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    Skip,
    Member { heard: bool },
    Tap,
    /// Plays only to feed a side-chain key (see `Isolate::keys`).
    Key,
}

impl Isolate {
    /// Linear search on purpose: renders hold a handful of tracks, and this must not allocate.
    pub(super) fn role(&self, id: Id) -> Role {
        if id == self.tap {
            return Role::Tap;
        }
        match self.tracks.iter().find(|(t, _)| *t == id) {
            Some(&(_, heard)) => Role::Member { heard },
            None if self.keys.contains(&id) => Role::Key,
            None => Role::Skip,
        }
    }

    /// Whether a track playing in role `from` may feed `dst` (through its output or a send):
    /// anything that plays may, except that key-only tracks stay among themselves.
    pub(super) fn feeds(&self, from: Option<Role>, dst: Id) -> bool {
        let to = self.role(dst);
        to != Role::Skip && (from != Some(Role::Key) || to == Role::Key)
    }
}

/// Transport and monitoring state put aside while rendering.
struct Saved {
    pos: i64,
    playing: bool,
    recording: bool,
    cycle: (bool, i64, i64),
    metronome: bool,
    count_in: bool,
    midi_target: Option<Id>,
    analyzer_target: Option<Id>,
    load: f32,
    low_latency: bool,
}

/// How often (in frames) the progress callback runs.
const PROGRESS_EVERY: i64 = 1 << 16;

impl Engine {
    /// Plays `[from, to)` and then `tail` more frames with the transport stopped (so effects
    /// ring out but nothing new starts), returning the stereo result. `isolate` limits the
    /// render to one track; None renders the full mix through Stereo Out.
    ///
    /// Transport, cycle, metronome and the live MIDI target are restored afterwards, and every
    /// instrument gets its note-offs. Allocates the result on the calling thread; `process`
    /// itself stays allocation-free.
    pub fn render(&mut self, from: i64, to: i64, tail: i64, isolate: Option<Isolate>, progress: &mut dyn FnMut(f64)) -> Vec<Vec<f32>> {
        // Apply edits that are still queued, so the render hears the latest settings.
        self.process(&[], 0, &mut [], 0);
        let saved = Saved {
            pos: self.pos,
            playing: self.playing,
            recording: self.recording,
            cycle: self.cycle,
            metronome: self.metronome,
            count_in: self.count_in,
            midi_target: self.midi_target,
            analyzer_target: self.analyzer_target,
            load: self.load,
            low_latency: self.pdc.low_latency,
        };

        let from = from.max(0);
        let to = to.max(from);
        // What's captured lags the timeline by the plug-in latency on its way (see `pdc.rs`):
        // render that much longer and drop the start, so the result lines up with `from`.
        let latency = self.render_latency(isolate.as_ref()) as i64;
        let total = (to - from + tail.max(0) + latency) as usize;
        let mut out = vec![Vec::with_capacity(total), Vec::with_capacity(total)];
        let mut buf = vec![0.0f32; MAX_BLOCK * 2];

        // Whatever was sounding live stops; the render starts clean at `from`.
        self.flush_all_notes();
        self.click.remaining = 0;
        self.pos = from;
        self.playing = true;
        self.recording = false;
        self.cycle.0 = false;
        self.metronome = false;
        self.count_in = false;
        // Live MIDI and the analyzer stay out of the render.
        self.midi_target = None;
        self.analyzer_target = None;
        self.isolate = isolate;
        self.offline = true;
        self.snap_gains = true;
        // Nothing is live in a render, and no audio from before it leaks in.
        self.pdc.low_latency = false;
        self.reset_effects();
        for t in &mut self.tracks {
            if let Some(d) = &mut t.delay {
                d.clear();
            }
        }

        let mut done = 0i64;
        let mut next_report = 0i64;
        let mut run = |e: &mut Engine, frames: i64, out: &mut Vec<Vec<f32>>| {
            let mut left = frames;
            while left > 0 {
                let n = left.min(MAX_BLOCK as i64) as usize;
                e.process(&[], 0, &mut buf[..n * 2], 2);
                for f in buf[..n * 2].chunks_exact(2) {
                    out[0].push(f[0]);
                    out[1].push(f[1]);
                }
                left -= n as i64;
                done += n as i64;
                if done >= next_report {
                    progress(done as f64 / total.max(1) as f64);
                    next_report = done + PROGRESS_EVERY;
                }
            }
        };
        run(self, to - from, &mut out);
        // The tail: notes end and the transport stops, but instruments and plug-ins keep
        // processing so releases, reverbs and delays are captured.
        self.playing = false;
        self.flush_all_notes();
        run(self, tail.max(0) + latency, &mut out);
        for c in &mut out {
            c.drain(..(latency as usize).min(c.len()));
        }

        self.flush_all_notes();
        // Live playback starts clean too, not with the render's ringing tail.
        self.reset_effects();
        // What the render played isn't for the meters, nor for the clip indicators they feed.
        for t in &self.tracks {
            t.meter.take();
        }
        self.status.master.take();
        self.isolate = None;
        self.offline = false;
        self.pos = saved.pos;
        self.playing = saved.playing;
        self.recording = saved.recording;
        self.cycle = saved.cycle;
        self.metronome = saved.metronome;
        self.count_in = saved.count_in;
        self.midi_target = saved.midi_target;
        self.analyzer_target = saved.analyzer_target;
        self.load = saved.load;
        self.pdc.low_latency = saved.low_latency;
        self.snap_gains = true;
        self.status.pos.store(self.pos, Ordering::Relaxed);
        self.status.playing.store(self.playing, Ordering::Relaxed);
        self.status.recording.store(self.recording, Ordering::Relaxed);
        self.status.cpu_load.store(self.load.to_bits(), Ordering::Relaxed);
        out
    }

    /// How far a render's capture lags the timeline: Stereo Out's total latency for the full
    /// mix, else the tap track's output time (less its own plug-ins when they're skipped).
    fn render_latency(&self, isolate: Option<&Isolate>) -> u32 {
        let Some(iso) = isolate else { return self.pdc.total };
        let Some(t) = self.tracks.iter().find(|t| t.id == iso.tap) else { return 0 };
        if iso.effects {
            t.pdc.output_time
        } else {
            t.pdc.output_time.saturating_sub(t.pdc.active_latency)
        }
    }
}
