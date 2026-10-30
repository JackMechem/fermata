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

    /// Takes over the audio another line holds (as much as fits), so a line replaced by a
    /// longer one plays on without a gap. Copies, but doesn't allocate.
    pub fn take_history(&mut self, old: &DelayLine) {
        let n = (old.mask + 1).min(self.mask + 1);
        for i in 1..=n {
            let src = (old.write + old.mask + 1 - i) & old.mask;
            let dst = (self.write + self.mask + 1 - i) & self.mask;
            self.l[dst] = old.l[src];
            self.r[dst] = old.r[src];
        }
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

    /// The block just written, read through `tap`: at its delay or, while it moves, faded from
    /// the old delay to the new one. Advances the fade.
    pub(super) fn read_tap(&self, tap: &mut Tap, l: &mut [f32], r: &mut [f32]) {
        if tap.left == 0 {
            self.read(tap.delay as usize, l, r);
            return;
        }
        let n = l.len().min(r.len());
        let max = self.max_delay();
        let (old, new) = ((tap.from as usize).min(max), (tap.delay as usize).min(max));
        let base = self.write + self.mask + 1 - n;
        for i in 0..n {
            let b = (base - new + i) & self.mask;
            if tap.left == 0 {
                l[i] = self.l[b];
                r[i] = self.r[b];
                continue;
            }
            let a = (base - old + i) & self.mask;
            let g = tap.gain();
            l[i] = self.l[a] + (self.l[b] - self.l[a]) * g;
            r[i] = self.r[a] + (self.r[b] - self.r[a]) * g;
            tap.left = tap.left.saturating_sub(1);
        }
    }
}

/// How long a compensation delay takes to move to a new length.
pub const FADE_SECS: f64 = 0.015;

/// Where one reader of a delay line (a track's output or a send) reads, and its crossfade
/// while its delay changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tap {
    /// The delay it reads at (once any fade is over).
    delay: u32,
    /// The delay it fades from, and the frames of the fade left out of `len`.
    from: u32,
    left: u32,
    len: u32,
}

impl Tap {
    /// A tap reading at `delay`, not moving.
    pub fn at(delay: u32) -> Tap {
        Tap { delay, ..Default::default() }
    }

    /// Points the tap at `target` frames, fading over `fade` frames from where it reads now.
    /// A fade under way finishes first: the block after it starts the next.
    pub fn aim(&mut self, target: u32, fade: u32) {
        if self.left == 0 && target != self.delay {
            if fade == 0 {
                self.delay = target;
            } else {
                *self = Tap { delay: target, from: self.delay, left: fade, len: fade };
            }
        }
    }

    /// The tap reads the block as written (no delay, not moving), so it needn't read at all.
    pub fn is_direct(&self) -> bool {
        self.delay == 0 && self.left == 0
    }

    /// The new delay's share of the next sample: a raised cosine from 0 to 1.
    fn gain(&self) -> f32 {
        let t = 1.0 - self.left as f32 / self.len.max(1) as f32;
        0.5 - 0.5 * (std::f32::consts::PI * t).cos()
    }
}

/// A track's taps on its delay line: its output and each send.
#[derive(Debug, Clone, Copy, Default)]
pub struct PdcTaps {
    pub output: Tap,
    pub sends: [Tap; SEND_SLOTS],
    /// An aux's instrument output (see `multi_out.rs`), on a line of its own.
    pub feed: Tap,
}

impl PdcTaps {
    /// Taps settled at a track's delays, for the start of an offline render.
    pub fn at(p: &PdcParams) -> PdcTaps {
        PdcTaps { output: Tap::at(p.output), sends: p.sends.map(Tap::at), feed: Tap::at(p.feed) }
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
    /// For an aux: delay on the instrument output feeding it, when other feeders arrive later.
    pub feed: u32,
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

    const SR: f32 = 48_000.0;

    /// A 440 Hz sine through a line read by a tap, in blocks of `block` frames, with the tap
    /// aimed at `delays[k]` from block `k` on (the last one holds).
    fn through(delays: &[(usize, u32)], blocks: usize, block: usize, fade: u32) -> Vec<f32> {
        let mut d = DelayLine::new(4000);
        let mut tap = Tap::default();
        let mut out = Vec::new();
        let mut target = 0;
        for k in 0..blocks {
            if let Some(&(_, t)) = delays.iter().rev().find(|(at, _)| *at <= k) {
                target = t;
            }
            let x: Vec<f32> = (k * block..(k + 1) * block).map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / SR).sin()).collect();
            d.write(&x, &x);
            tap.aim(target, fade);
            let (mut l, mut r) = (vec![0.0; block], vec![0.0; block]);
            d.read_tap(&mut tap, &mut l, &mut r);
            assert_eq!(l, r);
            out.extend(l);
        }
        out
    }

    fn largest_step(x: &[f32]) -> f32 {
        x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn a_delay_change_fades_instead_of_jumping() {
        let fade = (FADE_SECS * SR as f64) as u32;
        // The sine's own largest step: 0.5 · 2π · 440 / 48000.
        let own = 0.5 * std::f32::consts::TAU * 440.0 / SR;
        for (from, to) in [(0u32, 38u32), (38, 2085), (2085, 0), (500, 501)] {
            let out = through(&[(0, from), (20, to)], 60, 256, fade);
            let jumped = through(&[(0, from), (20, to)], 60, 256, 0);
            let (step, click) = (largest_step(&out[512..]), largest_step(&jumped[4096..]));
            assert!(step < own * 1.1, "{from} → {to}: largest step {step} (the sine's own {own}, without the fade {click})");
            // After the fade the output is exactly the input at the new delay.
            let x = |i: usize| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / SR).sin();
            let settled = 20 * 256 + fade as usize;
            for i in settled..out.len() {
                assert_eq!(out[i], x(i - to as usize), "{from} → {to} at {i}");
            }
        }
        // Without the fade, the 38-frame jump clicks: up to 2 · 0.5 · sin(π · 440 · 38 / 48000)
        // = 0.89, depending on where in the cycle it falls.
        let jumped = through(&[(0, 0), (22, 38)], 30, 256, 0);
        let click = largest_step(&jumped[512..]);
        assert!(click > 5.0 * own, "{click}");
    }

    #[test]
    fn a_new_fade_waits_for_the_last() {
        let mut t = Tap::at(10);
        t.aim(20, 100);
        t.aim(30, 100);
        assert_eq!((t.from, t.delay), (10, 20));
        t.left = 0;
        t.aim(30, 100);
        assert_eq!((t.from, t.delay, t.left), (20, 30, 100));
        assert!(!t.is_direct() && Tap::at(0).is_direct());
    }

    #[test]
    fn a_longer_line_keeps_the_audio() {
        let mut d = DelayLine::new(100);
        let block: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        for c in block.chunks(250) {
            d.write(c, c);
        }
        let mut longer = DelayLine::new(20_000);
        longer.take_history(&d);
        let (mut l, mut r) = (vec![0.0; 10], vec![0.0; 10]);
        let next: Vec<f32> = (1000..1010).map(|i| i as f32).collect();
        longer.write(&next, &next);
        longer.read(300, &mut l, &mut r);
        assert_eq!(l, (700..710).map(|i| i as f32).collect::<Vec<_>>());
    }

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
