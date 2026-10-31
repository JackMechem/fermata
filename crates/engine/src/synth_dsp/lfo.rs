//! An LFO, free-running or locked to the transport.
//!
//! Locked, its phase comes from the song position at the start of every block while the
//! transport plays, so it lands on the bar however playback got there; stopped, it runs on at the
//! tempo. Between blocks' starts it advances sample by sample, which gives the same values for
//! any block size at a steady tempo.

use super::analog::Rng;
use crate::fx::FxContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoWave {
    Sine,
    Triangle,
    /// Rises.
    Saw,
    Square,
    /// A new random value each cycle.
    SampleHold,
}

/// How fast an LFO runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LfoRate {
    Hz(f32),
    /// Cycles per quarter-note beat.
    PerBeat(f64),
}

#[derive(Debug, Clone, Copy)]
pub struct Lfo {
    phase: f64,
    /// Added to the phase from the transport, in cycles.
    offset: f64,
    wave: LfoWave,
    rate: LfoRate,
    /// Cycles per sample at the block's tempo.
    inc: f64,
    held: f32,
    rng: Rng,
    seed: u64,
}

impl Lfo {
    /// `seed` drives sample-and-hold.
    pub fn new(wave: LfoWave, rate: LfoRate, seed: u64) -> Lfo {
        Lfo { phase: 0.0, offset: 0.0, wave, rate, inc: 0.0, held: 0.0, rng: Rng::new(seed), seed }
    }

    pub fn set_wave(&mut self, wave: LfoWave) {
        self.wave = wave;
    }

    pub fn set_rate(&mut self, rate: LfoRate) {
        self.rate = rate;
    }

    /// The phase offset (cycles) added to the transport's.
    pub fn set_offset(&mut self, cycles: f64) {
        self.offset = cycles;
    }

    /// Back to the start: phase zero, and the same random values as before.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.held = 0.0;
        self.rng = Rng::new(self.seed);
    }

    /// Call at the start of each block, before its `tick`s, with `samples_per_tick` the number
    /// of samples per tick at its rate relative to `ctx.sample_rate` (1 unless ticking at a
    /// control rate).
    #[inline]
    pub fn begin_block(&mut self, ctx: &FxContext, samples_per_tick: f64) {
        match self.rate {
            LfoRate::Hz(hz) => self.inc = hz as f64 * samples_per_tick / ctx.sample_rate as f64,
            LfoRate::PerBeat(per_beat) => {
                self.inc = per_beat * ctx.tempo / 60.0 * samples_per_tick / ctx.sample_rate as f64;
                if ctx.playing {
                    let want = (ctx.position * per_beat + self.offset).rem_euclid(1.0);
                    if want < self.phase && self.phase - want > 0.5 {
                        self.held = self.rng.bipolar();
                    }
                    self.phase = want;
                }
            }
        }
    }

    /// One value, -1..1.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        let p = self.phase as f32;
        let v = match self.wave {
            LfoWave::Sine => (p * std::f32::consts::TAU).sin(),
            LfoWave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
            LfoWave::Saw => 2.0 * p - 1.0,
            LfoWave::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoWave::SampleHold => self.held,
        };
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.held = self.rng.bipolar();
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(position: f64, playing: bool) -> FxContext {
        FxContext { sample_rate: 48000.0, tempo: 120.0, playing, position }
    }

    #[test]
    fn free_lfo_runs_at_its_rate_and_ignores_the_transport() {
        let mut l = Lfo::new(LfoWave::Saw, LfoRate::Hz(4.0), 1);
        l.begin_block(&ctx(7.3, true), 1.0);
        let vals: Vec<f32> = (0..48000).map(|_| l.tick()).collect();
        let wraps = vals.windows(2).filter(|w| w[1] < w[0]).count();
        assert_eq!(wraps, 3, "4 cycles in a second start 3 new ones past the first");
    }

    #[test]
    fn synced_lfo_locks_to_the_song_position() {
        // A quarter-note cycle at 120 BPM is half a second.
        let mut l = Lfo::new(LfoWave::Saw, LfoRate::PerBeat(1.0), 1);
        l.begin_block(&ctx(10.25, true), 1.0);
        assert!((l.tick() - -0.5).abs() < 1e-5, "a quarter of the way through the beat");
        l.begin_block(&ctx(3.75, true), 1.0);
        assert!((l.tick() - 0.5).abs() < 1e-5, "a jump follows at once");
        // Stopped: it runs on from where it is, at the tempo.
        l.begin_block(&ctx(99.0, false), 1.0);
        let a = l.tick();
        assert!((a - 0.5 - 2.0 / 24000.0).abs() < 1e-3);
    }

    #[test]
    fn block_size_doesnt_matter_at_a_steady_tempo() {
        let run = |block: usize| {
            let mut l = Lfo::new(LfoWave::Sine, LfoRate::PerBeat(0.5), 1);
            let mut out = Vec::new();
            let mut at = 0usize;
            while at < 48000 {
                let n = block.min(48000 - at);
                let beats = at as f64 / 24000.0;
                l.begin_block(&ctx(beats, true), 1.0);
                out.extend((0..n).map(|_| l.tick()));
                at += n;
            }
            out
        };
        let (a, b) = (run(64), run(1024));
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-3, "{x} {y}");
        }
    }

    #[test]
    fn sample_hold_repeats_with_its_seed_after_a_reset() {
        let mut l = Lfo::new(LfoWave::SampleHold, LfoRate::Hz(100.0), 9);
        l.begin_block(&ctx(0.0, false), 1.0);
        let a: Vec<f32> = (0..4800).map(|_| l.tick()).collect();
        l.reset();
        let b: Vec<f32> = (0..4800).map(|_| l.tick()).collect();
        assert_eq!(a, b);
        assert!(a.iter().any(|&v| v != a[0]));
    }
}
