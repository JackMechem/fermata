//! Built-in effects: the contract every one of them implements, and the registry that lists
//! them.
//!
//! An effect is plain DSP behind the [`Effect`] trait. The engine runs it in an insert slot like
//! a VST3 plug-in; the UI shows it in a panel built from its [`ParamSpec`]s and meters.
//!
//! **Real-time rules (the effect runs on the audio thread):**
//! - Allocate every buffer in the constructor, sized for [`MAX_BLOCK`] frames at the sample
//!   rate given.
//! - `process`, `set_param` and `reset` must not allocate, lock or block.
//!
//! **Parameters:**
//! - Values are always in plain units (dB, ms, Hz, ratio, 0..1 for percentages and toggles,
//!   choice index), clamped to the spec's range by the host before `set_param`.
//! - Ids are stable forever: saved projects and automation lanes refer to them. Add new ids;
//!   never renumber.

pub mod biquad;
pub mod chorus;
pub mod compressor;
pub mod deesser;
pub mod delay;
pub mod distortion;
mod dyn_common;
pub mod fir;
pub mod flanger;
pub mod gain;
pub mod gate;
mod lfo;
pub mod limiter;
pub mod linear_phase_eq;
pub mod loudness;
pub mod overdrive;
mod oversample;
pub mod phaser;
pub mod reverb;
pub mod tremolo;
pub mod tuner;

/// The largest block `process` is called with (the engine's `MAX_BLOCK`).
pub const MAX_BLOCK: usize = 1024;

/// How many meter values an effect can report to its panel each block.
pub const MAX_METERS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    None,
    Db,
    Ms,
    Hz,
    /// Compression ratio, shown as "4:1".
    Ratio,
    /// Stored 0..1, shown as 0–100 %.
    Percent,
    Semitones,
    /// Beats or notes for tempo-synced values; the panel formats them via the choice labels.
    Note,
    /// An angle in degrees, such as an LFO's stereo phase offset.
    Degrees,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamKind {
    Continuous,
    /// 0 = off, 1 = on.
    Toggle,
    /// An index into the labels.
    Choice(&'static [&'static str]),
}

/// One parameter, as the host and the panel see it.
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub id: u32,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: Unit,
    pub kind: ParamKind,
    /// The control moves logarithmically (frequencies, times, ratios).
    pub log: bool,
}

/// What an effect may know about the transport while processing.
#[derive(Debug, Clone, Copy)]
pub struct FxContext {
    pub sample_rate: f32,
    /// The tempo at the block start, in BPM.
    pub tempo: f64,
    pub playing: bool,
    /// The song position at the block's first sample, in quarter-note beats, so tempo-synced
    /// LFOs can lock to the bar while the transport plays.
    pub position: f64,
}

/// A side-chain (key) input: another track's signal feeding the detector.
pub type Key<'a> = Option<(&'a [f32], &'a [f32])>;

/// A built-in effect instance.
pub trait Effect: Send {
    /// Sets a parameter (plain units, already clamped).
    fn set_param(&mut self, id: u32, value: f32);
    /// The current value of a parameter.
    fn param(&self, id: u32) -> f32;
    /// Processes one block in place. `l` and `r` have the same length, at most [`MAX_BLOCK`].
    /// `key` is the side-chain signal when the effect supports one and the user picked a
    /// source; otherwise the detector listens to the input.
    fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key, ctx: &FxContext);
    /// Clears all internal state: filters, envelopes, delay lines and reverb tails.
    fn reset(&mut self);
    /// Latency in samples that the effect adds (for example a limiter's lookahead), for plug-in
    /// delay compensation. It may change only when a parameter changes.
    fn latency(&self) -> usize {
        0
    }
    /// The longest a signal keeps sounding after input stops, for bounce tails.
    fn tail_secs(&self) -> f32 {
        0.0
    }
    /// Writes the meter values listed in the effect's [`EffectInfo::meters`], in that order.
    /// Called after `process`; values are in the units the panel expects (for example gain
    /// reduction in dB as a positive number).
    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        let _ = out;
    }
    /// The engine is rendering offline (a bounce, export or freeze) or playing live again.
    /// Offline, `process` and `reset` may block, so an effect that does work in the background
    /// while playing (the Linear Phase EQ's filter design) can wait for it instead, and land
    /// changes at the same point of every render.
    fn set_offline(&mut self, offline: bool) {
        let _ = offline;
    }
}

/// A built-in effect's description in the registry.
pub struct EffectInfo {
    /// The plug-in id used in insert slots and saved projects, for example "builtin:compressor".
    pub id: &'static str,
    pub name: &'static str,
    /// The Library and slot menu category: "Dynamics", "Reverb", "Delay", "Modulation",
    /// "Distortion", "Utility", "Metering", "EQ".
    pub category: &'static str,
    pub params: &'static [ParamSpec],
    /// Names of the meter values the effect reports, in order.
    pub meters: &'static [&'static str],
    /// The effect can take a side-chain input.
    pub sidechain: bool,
    /// Makes an instance at a sample rate, with every parameter at its default.
    pub new: fn(sample_rate: f32) -> Box<dyn Effect>,
}

impl EffectInfo {
    pub fn param(&self, id: u32) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.id == id)
    }
}

/// Every built-in effect. To add one: write `fx/<name>.rs` with a `pub fn info() -> EffectInfo`
/// and list it here.
pub fn registry() -> Vec<EffectInfo> {
    vec![
        compressor::info(),
        limiter::info(),
        gate::info(),
        deesser::info(),
        reverb::info(),
        delay::info(),
        tuner::info(),
        loudness::info(),
        chorus::info(),
        flanger::info(),
        phaser::info(),
        tremolo::info(),
        overdrive::info(),
        distortion::info(),
        gain::info(),
        linear_phase_eq::info(),
    ]
}

/// The registry entry for a plug-in id.
pub fn find(id: &str) -> Option<EffectInfo> {
    registry().into_iter().find(|e| e.id == id)
}

/// The factory presets of a built-in effect (empty for unknown ids).
pub fn factory_presets(id: &str) -> &'static [FactoryPreset] {
    match id {
        "builtin:noise-gate" => gate::PRESETS,
        "builtin:deesser" => deesser::PRESETS,
        "builtin:compressor" => compressor::PRESETS,
        "builtin:limiter" => limiter::PRESETS,
        "builtin:reverb" => reverb::PRESETS,
        "builtin:delay" => delay::PRESETS,
        "builtin:tuner" => tuner::PRESETS,
        "builtin:loudness" => loudness::PRESETS,
        "builtin:chorus" => chorus::PRESETS,
        "builtin:flanger" => flanger::PRESETS,
        "builtin:phaser" => phaser::PRESETS,
        "builtin:tremolo" => tremolo::PRESETS,
        "builtin:overdrive" => overdrive::PRESETS,
        "builtin:distortion" => distortion::PRESETS,
        "builtin:gain" => gain::PRESETS,
        "builtin:linear-phase-eq" => linear_phase_eq::PRESETS,
        _ => &[],
    }
}

/// Shared helpers for writing effects.
pub mod util {
    pub fn db_to_gain(db: f32) -> f32 {
        10f32.powf(db / 20.0)
    }

    pub fn gain_to_db(g: f32) -> f32 {
        20.0 * g.max(1e-9).log10()
    }

    /// The one-pole smoothing coefficient for a time constant in ms.
    pub fn coeff(ms: f32, sample_rate: f32) -> f32 {
        if ms <= 0.0 {
            1.0
        } else {
            1.0 - (-1.0 / (ms * 0.001 * sample_rate)).exp()
        }
    }
}

/// A factory preset that ships with an effect: a name and the parameter values it sets.
/// Parameters it doesn't list stay at their defaults. Each effect module exposes its list as
/// `pub const PRESETS: &[FactoryPreset]`; user presets are saved separately, as files.
pub struct FactoryPreset {
    pub name: &'static str,
    pub values: &'static [(u32, f32)],
}
