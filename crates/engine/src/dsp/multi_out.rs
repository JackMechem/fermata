//! An aux on an instrument's extra output (see `session/multi_out.rs`), on the audio thread.
//!
//! The instrument plays earlier in the block (`Project::process_order` treats the output like a
//! send), and the plug-in keeps each output bus's audio in its own buffers until its next block.
//! So the aux reads the bus when its turn comes, adding it to what its feeders wrote: same
//! block, no copies on the instrument's side, and nothing to do for a bus no aux reads.
//!
//! The output leaves the instrument ahead of its strip, so the instrument's fader, mute and
//! inserts don't touch it. Its Off switch does: the output fades as the strip does.

use super::pdc::Tap;
use super::*;

/// Where an aux's instrument output comes from. The line stays None until the aux has other
/// feeders that arrive later than the output.
#[derive(Default)]
pub struct OutputFeed {
    /// Index in the engine's list of the instrument track (set by `resolve_routes`).
    pub(super) source: Option<usize>,
    gain: f32,
    line: Option<Box<DelayLine>>,
}

/// What `OutputFeed::pull` needs to know about this block.
pub(super) struct Pull {
    pub bus: usize,
    pub smooth: f32,
    /// Jump to the gain instead of gliding (the start of a render).
    pub snap: bool,
    /// The delay to read at, and how long moving to it takes.
    pub delay: u32,
    pub fade: u32,
}

impl OutputFeed {
    /// Puts in a longer line, returning the old one. It takes over the audio, as a strip's does.
    pub(super) fn set_line(&mut self, line: Option<Box<DelayLine>>) -> Option<Box<DelayLine>> {
        let mut line = line?;
        if let Some(old) = &self.line {
            line.take_history(old);
        }
        self.line.replace(line)
    }

    /// Forgets the audio on its way (before an offline render, and while the aux is off).
    pub(super) fn clear(&mut self) {
        self.gain = 0.0;
        if let Some(d) = &mut self.line {
            d.clear();
        }
    }

    /// Adds this block of the output to `out`. `src` is the instrument's track, played earlier in
    /// the block, `scratch` two buffers of the block's length.
    pub(super) fn pull(&mut self, src: Option<&TrackDsp>, p: &Pull, tap: &mut Tap, out: (&mut [f32], &mut [f32]), scratch: (&mut [f32], &mut [f32])) {
        let (l, r) = out;
        let (sl, sr) = scratch;
        // A plug-in that didn't run this block (off, bypassed, skipped) has last block's audio
        // there, and one with fewer outputs has no such bus.
        let bus = src.filter(|s| s.instrument_ran).and_then(|s| s.instrument.as_ref()).and_then(|i| match &i.plugin {
            Plugin::Vst(v) => v.output_bus(p.bus, l.len()),
            _ => None,
        });
        match bus {
            Some((a, b)) => {
                sl.copy_from_slice(a);
                sr.copy_from_slice(b);
            }
            None if self.line.is_none() => {
                self.gain = 0.0;
                return;
            }
            None => {
                sl.fill(0.0);
                sr.fill(0.0);
            }
        }
        if let Some(d) = &mut self.line {
            d.write(sl, sr);
            tap.aim(p.delay, p.fade);
            if !tap.is_direct() {
                d.read_tap(tap, sl, sr);
            }
        }
        let target = if src.is_some_and(|s| !s.params.off) { 1.0 } else { 0.0 };
        let mut g = if p.snap { target } else { self.gain };
        for i in 0..l.len() {
            g += (target - g) * p.smooth;
            l[i] += sl[i] * g;
            r[i] += sr[i] * g;
        }
        self.gain = g;
    }
}
