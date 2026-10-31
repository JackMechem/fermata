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
    /// Which meter is the effect's gain reduction (positive dB), for dynamics effects, and its
    /// largest value since the session last took it, for the channel strip's gain-reduction
    /// meter. Kept whether or not the panel is open.
    gr_meter: Option<usize>,
    gr: AtomicU32,
}

/// The meter names under which dynamics effects report their gain reduction.
const GR_METERS: [&str; 2] = ["Gain Reduction", "Reduction"];

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
            gr_meter: info.meters.iter().position(|m| GR_METERS.contains(m)).filter(|&k| k < MAX_METERS),
            gr: AtomicU32::new(0),
        }
    }

    /// The largest gain reduction (positive dB) since the last call, or None when the effect
    /// doesn't reduce gain (isn't a dynamics effect).
    pub fn take_gain_reduction(&self) -> Option<f32> {
        self.gr_meter.map(|_| f32::from_bits(self.gr.swap(0, Ordering::Relaxed)))
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
    meter_buf: [f32; MAX_METERS],
    /// Reset and not yet run: parameter changes (automation at the start of playback or of a
    /// render) snap instead of gliding, so the first block already plays the automated value.
    fresh: bool,
}

impl BuiltinDsp {
    /// Instantiates an effect with the given parameter values (plain units; missing ones keep
    /// their defaults). Allocates: call off the audio thread.
    pub fn new(info: &EffectInfo, sample_rate: f32, values: &[(u32, f32)]) -> BuiltinDsp {
        let mut b = BuiltinDsp { fx: (info.new)(sample_rate), specs: info.params, shared: Arc::new(FxShared::new(info)), meter_buf: [0.0; MAX_METERS], fresh: true };
        for &(id, v) in values {
            b.set(id, v);
        }
        // Settle at those values before playing. Here, off the audio thread, an effect that
        // works in the background (the Linear Phase EQ's filter design) may wait for it, so it
        // doesn't start out at its defaults.
        b.fx.set_offline(true);
        b.fx.reset();
        b.fx.set_offline(false);
        b.publish();
        b
    }

    /// Puts an effect in that holds state of its own, which `EffectInfo::new` can't give it.
    #[cfg(test)]
    pub(crate) fn replace_effect_for_tests(&mut self, fx: Box<dyn Effect>) {
        self.fx = fx;
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
        // An effect that isn't running (bypassed, or on an Off track) reports no values, and
        // its panel must still show this one.
        if let Some(k) = self.specs.iter().position(|p| p.id == id) {
            self.shared.params[k].store(self.fx.param(id).to_bits(), Ordering::Relaxed);
        }
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
        let watched = self.shared.watched.load(Ordering::Relaxed);
        if watched || self.shared.gr_meter.is_some() {
            self.fx.meters(&mut self.meter_buf);
        }
        if let Some(k) = self.shared.gr_meter {
            // Non-negative floats order like their bits (see `Meter::feed`); max drops a NaN.
            self.shared.gr.fetch_max(self.meter_buf[k].max(0.0).to_bits(), Ordering::Relaxed);
        }
        if watched {
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

    /// Clears the effect for a track going Off: its tail is gone, and its meters show that
    /// rather than freezing at their last values, since it won't run again until it's back on.
    pub fn sleep(&mut self) {
        self.reset();
        self.fx.meters(&mut self.meter_buf);
        for (m, v) in self.shared.meters.iter().zip(&self.meter_buf) {
            m.store(v.to_bits(), Ordering::Relaxed);
        }
        self.shared.gr.store(0, Ordering::Relaxed);
    }

    /// Tells the effect whether the engine is rendering offline (see `Effect::set_offline`).
    pub fn set_offline(&mut self, offline: bool) {
        self.fx.set_offline(offline);
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

    #[test]
    fn dynamics_report_gain_reduction_without_their_panel() {
        let ctx = FxContext { sample_rate: 48000.0, tempo: 120.0, playing: true, position: 0.0 };
        let comp = crate::fx::find("builtin:compressor").unwrap();
        let mut b = BuiltinDsp::new(&comp, 48000.0, &[(crate::fx::compressor::param::THRESHOLD, -40.0)]);
        let shared = b.shared();
        assert_eq!(shared.take_gain_reduction(), Some(0.0));
        for _ in 0..20 {
            let (mut l, mut r) = (vec![0.5f32; 512], vec![0.5f32; 512]);
            b.process(&mut l, &mut r, None, &ctx);
        }
        let gr = shared.take_gain_reduction().unwrap();
        assert!(gr > 6.0, "gain reduction {gr}");
        assert_eq!(shared.take_gain_reduction(), Some(0.0), "taking it starts again");
        // An effect that doesn't reduce gain has no such meter.
        let reverb = crate::fx::find("builtin:reverb").unwrap();
        assert_eq!(BuiltinDsp::new(&reverb, 48000.0, &[]).shared().take_gain_reduction(), None);
        for id in ["builtin:limiter", "builtin:noise-gate", "builtin:deesser"] {
            let info = crate::fx::find(id).unwrap();
            assert!(BuiltinDsp::new(&info, 48000.0, &[]).shared().take_gain_reduction().is_some(), "{id}");
        }
    }
}
