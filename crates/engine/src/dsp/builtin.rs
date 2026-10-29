//! Built-in effects (see `crate::fx`) running in insert slots.
//!
//! The engine owns the effect; the session keeps an [`FxShared`] per instance to read what it
//! reports without locking: meters and live parameter values (only while the slot's panel
//! watches them), and its latency and tail.
//!
//! Automation lanes hold plug-in values normalized to 0..1, as VST3 does; [`to_plain`] and
//! [`to_normalized`] convert using the parameter's spec.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use crate::fx::{EffectInfo, Effect, FxContext, Key, ParamKind, ParamSpec, MAX_METERS};
use crate::model::Id;

/// A plain value (in the parameter's units) from a normalized 0..1 one.
pub fn to_plain(spec: &ParamSpec, norm: f32) -> f32 {
    let n = norm.clamp(0.0, 1.0);
    match spec.kind {
        ParamKind::Toggle => {
            if n >= 0.5 {
                spec.max
            } else {
                spec.min
            }
        }
        ParamKind::Choice(labels) => (n * (labels.len().max(1) - 1) as f32).round(),
        ParamKind::Continuous if spec.log && spec.min > 0.0 => spec.min * (spec.max / spec.min).powf(n),
        ParamKind::Continuous => spec.min + n * (spec.max - spec.min),
    }
}

/// The normalized 0..1 position of a plain value.
pub fn to_normalized(spec: &ParamSpec, plain: f32) -> f32 {
    let v = clamp(spec, plain);
    let n = match spec.kind {
        ParamKind::Choice(labels) => v / (labels.len().max(2) - 1) as f32,
        ParamKind::Continuous if spec.log && spec.min > 0.0 => (v / spec.min).ln() / (spec.max / spec.min).ln(),
        _ if spec.max > spec.min => (v - spec.min) / (spec.max - spec.min),
        _ => 0.0,
    };
    n.clamp(0.0, 1.0)
}

/// A value clamped to the spec's range; toggles and choices land on a whole step.
pub fn clamp(spec: &ParamSpec, v: f32) -> f32 {
    let v = if v.is_finite() { v } else { spec.default };
    match spec.kind {
        ParamKind::Toggle => {
            if v >= 0.5 {
                1.0
            } else {
                0.0
            }
        }
        ParamKind::Choice(labels) => v.round().clamp(0.0, (labels.len().max(1) - 1) as f32),
        ParamKind::Continuous => v.clamp(spec.min.min(spec.max), spec.max.max(spec.min)),
    }
}

/// What a running instance shares with the session, lock-free. Floats are stored as bits.
pub struct FxShared {
    /// The panel is open: the audio thread copies meters and parameter values after each block.
    watched: AtomicBool,
    meters: [AtomicU32; MAX_METERS],
    meter_count: usize,
    /// Parameter values in the order of the effect's specs.
    ids: Vec<u32>,
    params: Vec<AtomicU32>,
    latency: AtomicU32,
    tail: AtomicU32,
}

impl FxShared {
    fn new(info: &EffectInfo) -> FxShared {
        FxShared {
            watched: AtomicBool::new(false),
            meters: std::array::from_fn(|_| AtomicU32::new(0)),
            meter_count: info.meters.len().min(MAX_METERS),
            ids: info.params.iter().map(|p| p.id).collect(),
            params: info.params.iter().map(|p| AtomicU32::new(p.default.to_bits())).collect(),
            latency: AtomicU32::new(0),
            tail: AtomicU32::new(0),
        }
    }

    pub fn set_watched(&self, on: bool) {
        self.watched.store(on, Ordering::Relaxed);
    }

    /// The latest meter values, in the order of the effect's `meters`.
    pub fn meters(&self) -> Vec<f32> {
        self.meters[..self.meter_count].iter().map(|m| f32::from_bits(m.load(Ordering::Relaxed))).collect()
    }

    /// The values the effect is running with (automation included), as (id, plain value).
    pub fn params(&self) -> Vec<(u32, f32)> {
        self.ids.iter().zip(&self.params).map(|(&id, v)| (id, f32::from_bits(v.load(Ordering::Relaxed)))).collect()
    }

    /// The effect's latency (frames), as of the last parameter change.
    pub fn latency(&self) -> u32 {
        self.latency.load(Ordering::Relaxed)
    }

    pub fn tail_secs(&self) -> f32 {
        f32::from_bits(self.tail.load(Ordering::Relaxed))
    }
}

/// A built-in effect in an insert slot.
pub struct BuiltinDsp {
    fx: Box<dyn Effect>,
    specs: &'static [ParamSpec],
    shared: Arc<FxShared>,
    /// The track whose output keys the effect (the slot's side-chain) and, kept up to date by
    /// `Engine::resolve_routes`, its index in the engine's list.
    pub(super) key: Option<Id>,
    pub(super) key_idx: Option<usize>,
    meter_buf: [f32; MAX_METERS],
    /// Reset and not yet run: parameter changes (automation at the start of playback or of a
    /// render) snap instead of gliding, so the first block already plays the automated value.
    fresh: bool,
}

impl BuiltinDsp {
    /// Instantiates an effect with the given parameter values (plain units; missing ones keep
    /// their defaults). Allocates: call off the audio thread.
    pub fn new(info: &EffectInfo, sample_rate: f32, values: &[(u32, f32)], key: Option<Id>) -> BuiltinDsp {
        let mut b = BuiltinDsp { fx: (info.new)(sample_rate), specs: info.params, shared: Arc::new(FxShared::new(info)), key, key_idx: None, meter_buf: [0.0; MAX_METERS], fresh: true };
        for &(id, v) in values {
            b.set(id, v);
        }
        b.publish();
        b
    }

    pub fn shared(&self) -> Arc<FxShared> {
        self.shared.clone()
    }

    fn spec(&self, id: u32) -> Option<&'static ParamSpec> {
        self.specs.iter().find(|p| p.id == id)
    }

    /// Sets a parameter in plain units. Unknown ids are ignored.
    pub fn set(&mut self, id: u32, value: f32) {
        let Some(spec) = self.spec(id) else { return };
        self.fx.set_param(id, clamp(spec, value));
        self.publish();
    }

    /// Sets a parameter from an automation lane's normalized value.
    pub fn set_normalized(&mut self, id: u32, norm: f32) {
        if let Some(spec) = self.spec(id) {
            if self.fx.param(id) != to_plain(spec, norm) {
                self.set(id, to_plain(spec, norm));
                if self.fresh {
                    // Nothing has played since the reset, so resetting again only snaps the
                    // effect's smoothed parameters to the new value.
                    self.fx.reset();
                }
            }
        }
    }

    /// Latency and tail may change with any parameter.
    fn publish(&self) {
        self.shared.latency.store(self.fx.latency().min(u32::MAX as usize) as u32, Ordering::Relaxed);
        self.shared.tail.store(self.fx.tail_secs().to_bits(), Ordering::Relaxed);
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key, ctx: &FxContext) {
        self.fresh = false;
        self.fx.process(l, r, key, ctx);
        if self.shared.watched.load(Ordering::Relaxed) {
            self.fx.meters(&mut self.meter_buf);
            for (m, v) in self.shared.meters.iter().zip(&self.meter_buf) {
                m.store(v.to_bits(), Ordering::Relaxed);
            }
            for (spec, v) in self.specs.iter().zip(&self.shared.params) {
                v.store(self.fx.param(spec.id).to_bits(), Ordering::Relaxed);
            }
        }
    }

    pub fn reset(&mut self) {
        self.fx.reset();
        self.fresh = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::Unit;

    fn spec(kind: ParamKind, min: f32, max: f32, log: bool) -> ParamSpec {
        ParamSpec { id: 0, name: "x", min, max, default: min, unit: Unit::None, kind, log }
    }

    #[test]
    fn normalized_values_round_trip() {
        let lin = spec(ParamKind::Continuous, -60.0, 0.0, false);
        assert_eq!(to_plain(&lin, 0.5), -30.0);
        assert!((to_normalized(&lin, -15.0) - 0.75).abs() < 1e-6);
        let log = spec(ParamKind::Continuous, 20.0, 20000.0, true);
        assert!((to_plain(&log, 0.5) - 632.455).abs() < 0.01);
        assert!((to_normalized(&log, 2000.0) - 2.0 / 3.0).abs() < 1e-5);
        let choice = spec(ParamKind::Choice(&["a", "b", "c"]), 0.0, 2.0, false);
        assert_eq!(to_plain(&choice, 0.6), 1.0);
        assert_eq!(to_normalized(&choice, 2.0), 1.0);
        let toggle = spec(ParamKind::Toggle, 0.0, 1.0, false);
        assert_eq!((to_plain(&toggle, 0.4), to_plain(&toggle, 0.7)), (0.0, 1.0));
        assert_eq!(clamp(&choice, 7.3), 2.0);
        assert_eq!(clamp(&lin, f32::NAN), -60.0);
    }
}
