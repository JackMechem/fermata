//! What several models share beyond `synth_dsp`: parameter constructors, the output stage of an
//! oversampled voice bus, and the rule for going silent.
//!
//! **Going to rest.** A model stops doing work when nothing is sounding. Which sample it does so
//! on must not depend on the block size, or anything that runs on (an LFO, a noise generator)
//! would come out differently with another buffer size. So each model checks per sample, stops on
//! that very sample, and keeps count of the samples it spent at rest ([`Rest`]) in whole numbers,
//! so a free-running LFO can catch up in one step when the next note arrives.

use crate::fx::{ParamKind, ParamSpec, Unit};
use crate::synth_dsp::resample::Decimator2x;

pub const fn cont(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind: ParamKind::Continuous, log }
}

/// A panel knob marked 0 to 10.
pub const fn ten(id: u32, name: &'static str, default: f32) -> ParamSpec {
    cont(id, name, 0.0, 10.0, default, Unit::None, false)
}

pub const fn toggle(id: u32, name: &'static str, default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 0.0, max: 1.0, default, unit: Unit::None, kind: ParamKind::Toggle, log: false }
}

pub const fn choice(id: u32, name: &'static str, labels: &'static [&'static str], default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 0.0, max: (labels.len() - 1) as f32, default, unit: Unit::None, kind: ParamKind::Choice(labels), log: false }
}

pub const fn time_ms(id: u32, name: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    cont(id, name, min, max, default, Unit::Ms, true)
}

pub const fn percent(id: u32, name: &'static str, default: f32) -> ParamSpec {
    cont(id, name, 0.0, 1.0, default, Unit::Percent, false)
}

/// One output channel of an oversampled voice bus: decimator and DC blocker.
#[derive(Clone, Copy)]
pub struct OutStage {
    pub dec: Decimator2x,
    x1: f32,
    y1: f32,
}

impl OutStage {
    pub fn new() -> OutStage {
        OutStage { dec: Decimator2x::new(), x1: 0.0, y1: 0.0 }
    }

    /// Takes a value through the DC blocker, whose pole is `r`.
    #[inline]
    pub fn block_dc(&mut self, x: f32, r: f32) -> f32 {
        let y = x - self.x1 + r * self.y1;
        self.x1 = x;
        self.y1 = if y.abs() < 1e-15 { 0.0 } else { y };
        y
    }
}

impl Default for OutStage {
    fn default() -> Self {
        OutStage::new()
    }
}

/// The DC blocker's pole for a corner in Hz.
pub fn dc_pole(hz: f32, rate: f32) -> f32 {
    1.0 - std::f32::consts::TAU * hz / rate
}

/// Keeps count of the samples an instance has sat out, and how long it has left before it may.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rest {
    pub resting: bool,
    /// Samples to keep running after the last voice ended, so the tails after it can finish.
    pub hold: u32,
    /// Samples spent at rest since the last note.
    pub samples: u64,
}

impl Rest {
    /// A new instance sits at rest until its first note.
    pub fn new() -> Rest {
        Rest { resting: true, hold: 0, samples: 0 }
    }

    /// Note-on: back to work. Gives the samples that went by, for what runs free.
    #[inline]
    pub fn wake(&mut self) -> u64 {
        self.resting = false;
        std::mem::take(&mut self.samples)
    }
}

/// A frequency as a (fractional) note number, so cutoffs can move in semitones.
#[inline]
pub fn hz_to_note(hz: f32) -> f32 {
    69.0 + 12.0 * (hz.max(1.0) / 440.0).log2()
}
