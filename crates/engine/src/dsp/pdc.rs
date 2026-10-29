//! Plug-in delay compensation on the audio thread.
//!
//! The session works out how late each strip's audio is (see `session/pdc.rs`) and gives every
//! track a [`PdcParams`] and, when it needs one, a [`DelayLine`]. The line holds the strip's
//! signal after its plug-ins; the output and each send read it at their own delay, so a send
//! into a latent aux can leave earlier than the same track's dry signal and both still reach
//! Stereo Out in step.

use super::MAX_BLOCK;
use crate::model::SEND_SLOTS;

/// Bit in [`PdcParams::low_latency_skip`] for the instrument slot (insert slots use bits 0..8).
pub const INSTRUMENT_BIT: u16 = 1 << 15;

/// A stereo ring buffer with several read taps. Allocated by the session and sent to the
/// engine in a `Cmd::Pdc`; the audio thread only reads and writes it.
pub struct DelayLine {
    l: Vec<f32>,
    r: Vec<f32>,
    mask: usize,
    /// Where the next block is written.
    write: usize,
}

impl DelayLine {
    /// A line for delays up to at least `max_delay` frames. Allocates, so never call it on the
    /// audio thread. The length is a power of two so wrapping is a mask.
    pub fn new(max_delay: usize) -> DelayLine {
        let cap = (max_delay + MAX_BLOCK).next_power_of_two();
        DelayLine { l: vec![0.0; cap], r: vec![0.0; cap], mask: cap - 1, write: 0 }
    }

    /// The longest delay a read can have.
    pub fn max_delay(&self) -> usize {
        self.mask + 1 - MAX_BLOCK
    }

    /// Forgets the audio it holds (before an offline render).
    pub fn clear(&mut self) {
        self.l.fill(0.0);
        self.r.fill(0.0);
    }

    /// Appends a block (at most `MAX_BLOCK` frames).
    pub(super) fn write(&mut self, l: &[f32], r: &[f32]) {
        let n = l.len().min(r.len());
        for i in 0..n {
            let at = (self.write + i) & self.mask;
            self.l[at] = l[i];
            self.r[at] = r[i];
        }
        self.write = (self.write + n) & self.mask;
    }

    /// The block just written, `delay` frames late, into `l` and `r` (the same length as the
    /// block that was written).
    pub(super) fn read(&self, delay: usize, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let delay = delay.min(self.max_delay());
        let start = (self.write + self.mask + 1 - n - delay) & self.mask;
        for i in 0..n {
            let at = (start + i) & self.mask;
            l[i] = self.l[at];
            r[i] = self.r[at];
        }
    }
}

/// One track's delay compensation, as the session worked it out. All values are frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PdcParams {
    /// Delay before the track's output (to its stack, aux or Stereo Out).
    pub output: u32,
    /// Delay before each send.
    pub sends: [u32; SEND_SLOTS],
    /// How far the audio arriving at this strip lags the timeline (its feeders' latency).
    /// Plug-in automation and the plug-ins' transport are shifted by this much.
    pub input_time: u32,
    /// How far its output lags the timeline. Fader, pan and send automation are shifted by
    /// this much, and an isolated render of this track skips it.
    pub output_time: u32,
    /// The latency of the strip's running (not bypassed) plug-ins, which a render without
    /// effects doesn't go through.
    pub active_latency: u32,
    /// Plug-ins Low Latency Mode turns off on this track while it is live: bits 0..8 are
    /// insert slots, [`INSTRUMENT_BIT`] the instrument.
    pub low_latency_skip: u16,
}

impl PdcParams {
    /// The longest read this track makes from its delay line.
    pub fn max_delay(&self) -> u32 {
        self.sends.iter().copied().fold(self.output, u32::max)
    }
}

/// Settings that apply to the whole graph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PdcGlobal {
    /// How far Stereo Out lags the timeline (the longest path, plus Stereo Out's own
    /// plug-ins). The click is delayed by this much, and full-mix renders skip it.
    pub total: u32,
    /// Low Latency Mode: live tracks drop the plug-ins in their `low_latency_skip`.
    pub low_latency: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_read_the_past() {
        let mut d = DelayLine::new(100);
        assert!(d.max_delay() >= 100);
        let mut t = 0.0f32;
        let mut out = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        for _ in 0..40 {
            let block: Vec<f32> = (0..64).map(|_| {
                t += 1.0;
                t
            }).collect();
            d.write(&block, &block);
            d.read(37, &mut out, &mut out_r);
            // Sample i of this block is `block[i]`; 37 frames earlier it was 37 less.
            for i in 0..64 {
                let want = (block[i] - 37.0).max(0.0);
                assert_eq!(out[i], want);
                assert_eq!(out_r[i], want);
            }
            d.read(0, &mut out, &mut out_r);
            assert_eq!(out, block);
        }
    }
}
