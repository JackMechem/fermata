//! Chorus, after Logic's: a short delay whose time an LFO sweeps, mixed with the dry signal.
//! The slight, slowly changing pitch shift of the delayed copy thickens the sound.
//!
//! - **Delay** is the shortest delay; **Intensity** is how far above it the LFO sweeps (up to
//!   [`SWEEP_MS`]).
//! - **Spread** offsets the right channel's LFO from the left's by up to 180°, so the two sides
//!   move apart and a mono source comes out wide.
//! - **Mix** crossfades between dry (0 %) and only the delayed copy (100 %).
//!
//! This module also holds [`ModDelay`], the modulated delay the Flanger shares.

use super::delay::{Line, MAX_RATE};
use super::lfo::{flush, guard, unipolar_sine, Lfo, Smoothed};
use super::util::coeff;
use super::*;

pub const RATE: u32 = 0;
pub const INTENSITY: u32 = 1;
pub const DELAY: u32 = 2;
pub const SPREAD: u32 = 3;
pub const MIX: u32 = 4;

/// How far Intensity at 100 % sweeps the delay above the Delay setting, in ms.
pub const SWEEP_MS: f32 = 6.0;
/// How many octaves Intensity at 100 % sweeps the Flanger's delay up from its Delay setting.
pub const FLANGE_OCTAVES: f32 = 3.0;
/// The longest delay either effect can reach, in ms; the lines are sized for it.
const MAX_MOD_MS: f32 = 64.0;

const fn spec(id: u32, name: &'static str, min: f32, max: f32, default: f32, unit: Unit, log: bool) -> ParamSpec {
    ParamSpec { id, name, min, max, default, unit, kind: ParamKind::Continuous, log }
}

static PARAMS: [ParamSpec; 5] = [
    spec(RATE, "Rate", 0.05, 10.0, 0.8, Unit::Hz, true),
    spec(INTENSITY, "Intensity", 0.0, 1.0, 0.5, Unit::Percent, false),
    spec(DELAY, "Delay", 2.0, 40.0, 12.0, Unit::Ms, true),
    spec(SPREAD, "Spread", 0.0, 1.0, 0.5, Unit::Percent, false),
    spec(MIX, "Mix", 0.0, 1.0, 0.5, Unit::Percent, false),
];

/// The modulation effects report their LFO's position (0..1 of a cycle) so the panel can show
/// it moving.
pub(crate) static LFO_METER: [&str; 1] = ["LFO"];

pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Classic Chorus", values: &[(RATE, 0.8), (INTENSITY, 0.5), (DELAY, 12.0), (SPREAD, 0.5), (MIX, 0.5)] },
    FactoryPreset { name: "Subtle Width", values: &[(RATE, 0.35), (INTENSITY, 0.25), (DELAY, 18.0), (SPREAD, 1.0), (MIX, 0.35)] },
    FactoryPreset { name: "Lush Pad", values: &[(RATE, 0.25), (INTENSITY, 0.85), (DELAY, 20.0), (SPREAD, 1.0), (MIX, 0.5)] },
    FactoryPreset { name: "80s Clean Guitar", values: &[(RATE, 1.2), (INTENSITY, 0.6), (DELAY, 8.0), (SPREAD, 0.5), (MIX, 0.5)] },
    FactoryPreset { name: "Doubler", values: &[(RATE, 0.15), (INTENSITY, 0.3), (DELAY, 28.0), (SPREAD, 1.0), (MIX, 0.45)] },
    FactoryPreset { name: "Bass Chorus", values: &[(RATE, 0.6), (INTENSITY, 0.4), (DELAY, 10.0), (SPREAD, 0.3), (MIX, 0.35)] },
    FactoryPreset { name: "Vibrato", values: &[(RATE, 5.5), (INTENSITY, 0.35), (DELAY, 3.0), (SPREAD, 0.0), (MIX, 1.0)] },
];

pub fn info() -> EffectInfo {
    EffectInfo {
        id: "builtin:chorus",
        name: "Chorus",
        category: "Modulation",
        params: &PARAMS,
        meters: &LFO_METER,
        sidechain: false,
        new: |sr| Box::new(Chorus { values: std::array::from_fn(|i| PARAMS[i].default), core: ModDelay::new(Sweep::Linear, sr) }),
    }
}

/// How a [`ModDelay`]'s delay follows its LFO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sweep {
    /// From Delay up to Delay + depth × [`SWEEP_MS`] (Chorus).
    Linear,
    /// From Delay up by depth × [`FLANGE_OCTAVES`] octaves, so the comb's notches sweep evenly
    /// across the spectrum (Flanger).
    Octaves,
}

/// One block's settings for a [`ModDelay`], in plain units.
pub(crate) struct ModSettings {
    pub rate_hz: f32,
    pub depth: f32,
    pub delay_ms: f32,
    /// The loop gain, −1..1 (already scaled to the effect's safe ceiling).
    pub feedback: f32,
    pub spread: f32,
    pub mix: f32,
}

/// The delay (ms) at LFO position `u` (0..1).
pub(crate) fn swept_ms(sweep: Sweep, delay_ms: f32, depth: f32, u: f32) -> f32 {
    let d = match sweep {
        Sweep::Linear => delay_ms + depth * SWEEP_MS * u,
        Sweep::Octaves => delay_ms * (FLANGE_OCTAVES * depth * u).exp2(),
    };
    d.min(MAX_MOD_MS - 1.0)
}

/// A stereo delay line pair swept by one LFO, with feedback and a dry/wet mix.
pub(crate) struct ModDelay {
    sweep: Sweep,
    sr: f32,
    lines: [Line; 2],
    lfo: Lfo,
    delay_ms: Smoothed,
    depth: Smoothed,
    feedback: Smoothed,
    spread: Smoothed,
    mix: Smoothed,
    /// False until the first block after a reset, which snaps the smoothers to their targets.
    primed: bool,
}

impl ModDelay {
    pub(crate) fn new(sweep: Sweep, sample_rate: f32) -> ModDelay {
        let cap = (MAX_MOD_MS * 0.001 * sample_rate.max(MAX_RATE)) as usize + 8;
        ModDelay {
            sweep,
            sr: sample_rate,
            lines: [Line::new(cap), Line::new(cap)],
            lfo: Lfo::default(),
            delay_ms: Smoothed::default(),
            depth: Smoothed::default(),
            feedback: Smoothed::default(),
            spread: Smoothed::default(),
            mix: Smoothed::default(),
            primed: false,
        }
    }

    pub(crate) fn process(&mut self, l: &mut [f32], r: &mut [f32], s: &ModSettings, ctx: &FxContext) {
        if ctx.sample_rate > 0.0 && (ctx.sample_rate - self.sr).abs() > 0.5 && ctx.sample_rate <= MAX_RATE {
            self.sr = ctx.sample_rate;
            self.reset();
        }
        if !self.primed {
            self.primed = true;
            self.delay_ms.snap(s.delay_ms);
            self.depth.snap(s.depth);
            self.feedback.snap(s.feedback);
            self.spread.snap(s.spread);
            self.mix.snap(s.mix);
        }
        let sr = self.sr;
        let k = coeff(20.0, sr);
        // The delay time glides more slowly: a quick change of delay is a pitch jump.
        let k_delay = coeff(80.0, sr);
        let step = (s.rate_hz / sr) as f64;
        for n in 0..l.len() {
            let base = self.delay_ms.next(s.delay_ms, k_delay);
            let depth = self.depth.next(s.depth, k);
            let fb = self.feedback.next(s.feedback, k);
            let spread = self.spread.next(s.spread, k);
            let mix = self.mix.next(s.mix, k);
            let ph = self.lfo.phase();
            let u = [unipolar_sine(ph), unipolar_sine(ph + 0.5 * spread as f64)];
            let x = [l[n], r[n]];
            let mut y = [0.0; 2];
            for c in 0..2 {
                let d = swept_ms(self.sweep, base, depth, u[c]) * sr / 1000.0;
                let wet = self.lines[c].read_frac(d);
                self.lines[c].write(flush(guard(x[c] + fb * wet)));
                y[c] = x[c] + (wet - x[c]) * mix;
            }
            l[n] = y[0];
            r[n] = y[1];
            self.lfo.advance(step);
        }
    }

    pub(crate) fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.lfo.set_phase(0.0);
        self.primed = false;
    }

    /// The LFO's position, for the panel.
    pub(crate) fn meters(&self, out: &mut [f32; MAX_METERS]) {
        out[0] = self.lfo.phase() as f32;
    }

    /// How long the repeats take to fall 60 dB at the longest delay the settings reach.
    pub(crate) fn tail_secs(&self, s: &ModSettings) -> f32 {
        let longest = swept_ms(self.sweep, s.delay_ms, s.depth, 1.0) * 0.001;
        let g = s.feedback.abs();
        if g < 1e-3 {
            longest
        } else {
            longest * (1.0 + (1e-3f32).ln() / g.ln())
        }
    }

    #[cfg(test)]
    pub(crate) fn has_subnormal(&self) -> bool {
        self.lines.iter().any(|l| l.has_subnormal())
    }
}

pub struct Chorus {
    values: [f32; PARAMS.len()],
    core: ModDelay,
}

impl Chorus {
    fn settings(&self) -> ModSettings {
        let v = |id: u32| self.values[id as usize];
        ModSettings { rate_hz: v(RATE), depth: v(INTENSITY), delay_ms: v(DELAY), feedback: 0.0, spread: v(SPREAD), mix: v(MIX) }
    }
}

impl Effect for Chorus {
    fn set_param(&mut self, id: u32, value: f32) {
        if let Some(v) = self.values.get_mut(id as usize) {
            *v = value;
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, ctx: &FxContext) {
        let s = self.settings();
        self.core.process(l, r, &s, ctx);
    }

    fn reset(&mut self) {
        self.core.reset();
    }

    fn tail_secs(&self) -> f32 {
        self.core.tail_secs(&self.settings())
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        self.core.meters(out);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SR: f32 = 48_000.0;
    pub(crate) const CTX: FxContext = FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: 0.0 };

    /// A 220 Hz sine on the left and 330 Hz on the right, `len` samples.
    pub(crate) fn tones(len: usize) -> (Vec<f32>, Vec<f32>) {
        let t = |f: f32, i: usize| 0.5 * (std::f32::consts::TAU * f * i as f32 / SR).sin();
        ((0..len).map(|i| t(220.0, i)).collect(), (0..len).map(|i| t(330.0, i)).collect())
    }

    pub(crate) fn run(fx: &mut dyn Effect, l: &mut [f32], r: &mut [f32]) {
        for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            fx.process(bl, br, None, &CTX);
        }
    }

    /// Every factory preset's values are in range, and each runs cleanly on noise.
    pub(crate) fn check_presets(info: &EffectInfo, presets: &[FactoryPreset]) {
        assert!(presets.len() >= 5, "{}: {} presets", info.name, presets.len());
        for p in presets {
            let mut fx = (info.new)(SR);
            for &(id, v) in p.values {
                let spec = info.param(id).unwrap_or_else(|| panic!("{}: no param {id}", p.name));
                assert!(v >= spec.min && v <= spec.max, "{}: {} = {v}", p.name, spec.name);
                fx.set_param(id, v);
            }
            let (mut l, mut r) = tones(48_000);
            run(fx.as_mut(), &mut l, &mut r);
            assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 4.0), "{}", p.name);
        }
    }

    /// The effect at every parameter's extremes, on full-scale noise: finite and bounded.
    pub(crate) fn check_extremes(info: &EffectInfo, bound: f32) {
        let mut rng = 0x1234_5678u32;
        let mut noise = || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        for corner in 0..(1 << info.params.len().min(10)) {
            let mut fx = (info.new)(SR);
            for (i, p) in info.params.iter().enumerate() {
                fx.set_param(p.id, if corner & (1 << i) != 0 { p.max } else { p.min });
            }
            let mut peak = 0.0f32;
            for _ in 0..20 {
                let mut l: Vec<f32> = (0..512).map(|_| noise()).collect();
                let mut r: Vec<f32> = (0..512).map(|_| noise()).collect();
                fx.process(&mut l, &mut r, None, &CTX);
                for x in l.iter().chain(&r) {
                    assert!(x.is_finite(), "{}: corner {corner:b}", info.name);
                    peak = peak.max(x.abs());
                }
            }
            assert!(peak < bound, "{}: corner {corner:b} peaks at {peak}", info.name);
        }
    }

    fn chorus() -> Chorus {
        Chorus { values: std::array::from_fn(|i| PARAMS[i].default), core: ModDelay::new(Sweep::Linear, SR) }
    }

    #[test]
    fn mix_at_zero_is_the_dry_signal() {
        let mut c = chorus();
        c.set_param(MIX, 0.0);
        c.set_param(INTENSITY, 1.0);
        let (l0, r0) = tones(20_000);
        let (mut l, mut r) = (l0.clone(), r0.clone());
        run(&mut c, &mut l, &mut r);
        assert_eq!(l, l0);
        assert_eq!(r, r0);
    }

    #[test]
    fn wet_is_the_input_delayed_by_the_delay_setting() {
        // With no sweep, the delayed copy is exactly the input Delay ms later.
        let mut c = chorus();
        c.set_param(INTENSITY, 0.0);
        c.set_param(DELAY, 10.0);
        c.set_param(MIX, 1.0);
        let mut l = vec![0.0; 2000];
        let mut r = vec![0.0; 2000];
        l[0] = 1.0;
        r[0] = 1.0;
        run(&mut c, &mut l, &mut r);
        assert!((l[480] - 1.0).abs() < 1e-6 && l.iter().enumerate().all(|(i, x)| i == 480 || x.abs() < 1e-6));
    }

    #[test]
    fn the_sweep_follows_intensity_and_spread_moves_the_sides_apart() {
        assert_eq!(swept_ms(Sweep::Linear, 12.0, 0.5, 0.0), 12.0);
        assert_eq!(swept_ms(Sweep::Linear, 12.0, 0.5, 1.0), 15.0);
        // Mono in: without spread both sides match; with it they differ.
        for (spread, same) in [(0.0, true), (1.0, false)] {
            let mut c = chorus();
            c.set_param(SPREAD, spread);
            c.set_param(INTENSITY, 1.0);
            let (mut l, _) = tones(24_000);
            let mut r = l.clone();
            run(&mut c, &mut l, &mut r);
            let diff = l.iter().zip(&r).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
            assert_eq!(diff < 1e-6, same, "spread {spread}: {diff}");
        }
    }

    #[test]
    fn parameter_changes_glide() {
        // Jumping Delay from 2 to 40 ms mid-stream doesn't step the output.
        let mut c = chorus();
        c.set_param(DELAY, 2.0);
        c.set_param(MIX, 1.0);
        let (mut l, mut r) = tones(48_000);
        let slope = std::f32::consts::TAU * 330.0 * 0.5 / SR;
        let mut worst = 0.0f32;
        let mut last = 0.0;
        for (i, (bl, br)) in l.chunks_mut(512).zip(r.chunks_mut(512)).enumerate() {
            if i == 40 {
                c.set_param(DELAY, 40.0);
                c.set_param(INTENSITY, 1.0);
            }
            c.process(bl, br, None, &CTX);
            for &y in br.iter() {
                if i > 2 {
                    worst = worst.max((y - last).abs());
                }
                last = y;
            }
        }
        assert!(worst < 2.0 * slope, "step {worst} vs slope {slope}");
    }

    #[test]
    fn extremes_and_presets_are_clean() {
        check_extremes(&info(), 2.6);
        check_presets(&info(), PRESETS);
        let mut c = chorus();
        let mut l = vec![0.0; 48_000];
        let mut r = l.clone();
        l[0] = 1.0;
        run(&mut c, &mut l, &mut r);
        assert!(!c.core.has_subnormal());
        assert!((c.tail_secs() - 0.015).abs() < 1e-6);
    }
}
