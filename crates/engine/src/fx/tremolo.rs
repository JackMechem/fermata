//! Tremolo, after Logic's: an LFO moves the level up and down.
//!
//! - **Rate** in Hz, or with **Sync** a note value at the project tempo. While the transport
//!   plays, a synced LFO is locked to the song position (its peak lands on the beat), so a
//!   bounce or freeze lines up with what played; stopped, it keeps running at the tempo.
//! - **Depth** is how far the level dips: 0 % leaves the signal untouched, 100 % goes to
//!   silence at the bottom of each cycle.
//! - **Waveform:** Sine, Triangle or Square. **Smoothing** rounds the square's edges, from a
//!   quick (but never clicking) switch to a triangle-like swell.
//! - **Phase** offsets the right channel's LFO from the left's; at 180° the level moves from
//!   side to side, an auto-pan.

use super::delay::{note_beats, NOTE_LABELS};
use super::lfo::{Lfo, Smoothed};
use super::util::coeff;
use super::*;

pub const RATE: u32 = 0;
pub const SYNC: u32 = 1;
pub const NOTE: u32 = 2;
pub const DEPTH: u32 = 3;
pub const WAVEFORM: u32 = 4;
pub const SMOOTHING: u32 = 5;
pub const PHASE: u32 = 6;

pub const WAVEFORMS: &[&str] = &["Sine", "Triangle", "Square"];
const TRIANGLE: usize = 1;
const SQUARE: usize = 2;

/// The square's shortest edge, so even at 0 % Smoothing it never clicks.
const MIN_EDGE_SECS: f32 = 0.002;

static PARAMS: [ParamSpec; 7] = [
    ParamSpec { id: RATE, name: "Rate", min: 0.05, max: 20.0, default: 4.0, unit: Unit::Hz, kind: ParamKind::Continuous, log: true },
    ParamSpec { id: SYNC, name: "Sync", min: 0.0, max: 1.0, default: 1.0, unit: Unit::None, kind: ParamKind::Toggle, log: false },
    ParamSpec { id: NOTE, name: "Note", min: 0.0, max: 17.0, default: 9.0, unit: Unit::Note, kind: ParamKind::Choice(NOTE_LABELS), log: false },
    ParamSpec { id: DEPTH, name: "Depth", min: 0.0, max: 1.0, default: 0.5, unit: Unit::Percent, kind: ParamKind::Continuous, log: false },
    ParamSpec { id: WAVEFORM, name: "Waveform", min: 0.0, max: 2.0, default: 0.0, unit: Unit::None, kind: ParamKind::Choice(WAVEFORMS), log: false },
    ParamSpec { id: SMOOTHING, name: "Smoothing", min: 0.0, max: 1.0, default: 0.3, unit: Unit::Percent, kind: ParamKind::Continuous, log: false },
    ParamSpec { id: PHASE, name: "Phase", min: 0.0, max: 180.0, default: 0.0, unit: Unit::Degrees, kind: ParamKind::Continuous, log: false },
];

/// Note indices follow [`NOTE_LABELS`] (6 = 1/4, 9 = 1/8, 11 = 1/8 triplet, 12 = 1/16).
/// Waveforms: 0 Sine, 1 Triangle, 2 Square.
pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Eighth Notes", values: &[(SYNC, 1.0), (NOTE, 9.0), (DEPTH, 0.5), (WAVEFORM, 0.0)] },
    FactoryPreset { name: "Vintage Amp", values: &[(SYNC, 0.0), (RATE, 5.5), (DEPTH, 0.45), (WAVEFORM, 1.0)] },
    FactoryPreset { name: "Sixteenth Chop", values: &[(SYNC, 1.0), (NOTE, 12.0), (DEPTH, 1.0), (WAVEFORM, 2.0), (SMOOTHING, 0.1)] },
    FactoryPreset { name: "Triplet Pulse", values: &[(SYNC, 1.0), (NOTE, 11.0), (DEPTH, 0.7), (WAVEFORM, 2.0), (SMOOTHING, 0.5)] },
    FactoryPreset { name: "Slow Swell", values: &[(SYNC, 1.0), (NOTE, 3.0), (DEPTH, 0.6), (WAVEFORM, 0.0)] },
    FactoryPreset { name: "Auto-Pan", values: &[(SYNC, 1.0), (NOTE, 6.0), (DEPTH, 1.0), (WAVEFORM, 0.0), (PHASE, 180.0)] },
    FactoryPreset { name: "Helicopter", values: &[(SYNC, 0.0), (RATE, 14.0), (DEPTH, 0.9), (WAVEFORM, 2.0), (SMOOTHING, 0.2)] },
    FactoryPreset { name: "Wide Shimmer", values: &[(SYNC, 0.0), (RATE, 7.0), (DEPTH, 0.35), (WAVEFORM, 0.0), (PHASE, 90.0)] },
];

pub fn info() -> EffectInfo {
    EffectInfo { id: "builtin:tremolo", name: "Tremolo", category: "Modulation", params: &PARAMS, meters: &super::chorus::LFO_METER, sidechain: false, new: |sr| Box::new(Tremolo::new(sr)) }
}

/// The level (0..1, 1 at the top) of a waveform (an index into [`WAVEFORMS`]) at `phase`
/// (cycles), peaking at phase 0. `edge` is the share of the cycle the square's two edges take
/// together (0..1; 1 makes it a rounded triangle).
pub fn shape(waveform: usize, phase: f64, edge: f32) -> f32 {
    let p = phase.rem_euclid(1.0) as f32;
    // The distance to the nearest cycle start, 0..0.5.
    let d = p.min(1.0 - p);
    match waveform {
        TRIANGLE => 1.0 - 2.0 * d,
        SQUARE => {
            // A triangle from 1 to −1, clipped and rounded: the edges take `edge` of a cycle.
            let t = 1.0 - 4.0 * d;
            let v = (t / edge.max(1e-6)).clamp(-1.0, 1.0);
            0.5 + 0.5 * (v * std::f32::consts::FRAC_PI_2).sin()
        }
        _ => 0.5 + 0.5 * (std::f32::consts::TAU * p).cos(),
    }
}

/// The gain at LFO level `u` (0..1).
#[inline]
pub fn gain(depth: f32, u: f32) -> f32 {
    1.0 - depth * (1.0 - u)
}

pub struct Tremolo {
    values: [f32; PARAMS.len()],
    sr: f32,
    lfo: Lfo,
    depth: Smoothed,
    phase_offset: Smoothed,
    edge: Smoothed,
    /// The gain applied, per channel, smoothed for a millisecond so a jump in phase (a cycle
    /// wrapping round, or the waveform changing) can't click.
    out: [Smoothed; 2],
    primed: bool,
}

impl Tremolo {
    pub fn new(sample_rate: f32) -> Tremolo {
        Tremolo {
            values: std::array::from_fn(|i| PARAMS[i].default),
            sr: sample_rate,
            lfo: Lfo::default(),
            depth: Smoothed::default(),
            phase_offset: Smoothed::default(),
            edge: Smoothed::default(),
            out: [Smoothed::default(); 2],
            primed: false,
        }
    }

    fn v(&self, id: u32) -> f32 {
        self.values[id as usize]
    }

    fn synced(&self) -> bool {
        self.v(SYNC) >= 0.5
    }

    /// One LFO cycle in beats when synced.
    fn cycle_beats(&self) -> f64 {
        note_beats(self.v(NOTE).round().clamp(0.0, (NOTE_LABELS.len() - 1) as f32) as usize)
    }

    /// The LFO rate in Hz at a tempo.
    fn rate_hz(&self, tempo: f64) -> f32 {
        if self.synced() {
            (tempo.max(1.0) / 60.0 / self.cycle_beats()) as f32
        } else {
            self.v(RATE)
        }
    }
}

impl Effect for Tremolo {
    fn set_param(&mut self, id: u32, value: f32) {
        if let Some(v) = self.values.get_mut(id as usize) {
            *v = value;
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, ctx: &FxContext) {
        if ctx.sample_rate > 0.0 && (ctx.sample_rate - self.sr).abs() > 0.5 {
            self.sr = ctx.sample_rate;
        }
        let sr = self.sr;
        let rate = self.rate_hz(ctx.tempo);
        if self.synced() && ctx.playing {
            // Lock to the song position: the LFO's top falls on the beat grid.
            self.lfo.set_phase(ctx.position / self.cycle_beats());
        }
        let waveform = (self.v(WAVEFORM).round() as usize).min(SQUARE);
        let depth_t = self.v(DEPTH);
        let offset_t = self.v(PHASE) / 360.0;
        // The square's edges: a share of the cycle, each never shorter than MIN_EDGE_SECS.
        let edge_t = self.v(SMOOTHING).max(2.0 * MIN_EDGE_SECS * rate).min(1.0);
        let step = (rate / sr) as f64;
        if !self.primed {
            self.primed = true;
            self.depth.snap(depth_t);
            self.phase_offset.snap(offset_t);
            self.edge.snap(edge_t);
            let ph = self.lfo.phase();
            self.out[0].snap(gain(depth_t, shape(waveform, ph, edge_t)));
            self.out[1].snap(gain(depth_t, shape(waveform, ph + offset_t as f64, edge_t)));
        }
        let k = coeff(20.0, sr);
        let k_out = coeff(1.0, sr);
        for n in 0..l.len() {
            let depth = self.depth.next(depth_t, k);
            let offset = self.phase_offset.next(offset_t, k);
            let edge = self.edge.next(edge_t, k);
            let ph = self.lfo.phase();
            let gl = self.out[0].next(gain(depth, shape(waveform, ph, edge)), k_out);
            let gr = self.out[1].next(gain(depth, shape(waveform, ph + offset as f64, edge)), k_out);
            l[n] *= gl;
            r[n] *= gr;
            self.lfo.advance(step);
        }
    }

    fn reset(&mut self) {
        self.lfo.set_phase(0.0);
        self.primed = false;
    }

    fn meters(&self, out: &mut [f32; MAX_METERS]) {
        out[0] = self.lfo.phase() as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::super::chorus::tests::{check_extremes, check_presets, run, tones, SR};
    use super::*;

    #[test]
    fn depth_zero_is_unity() {
        for wave in 0..3 {
            let mut t = Tremolo::new(SR);
            t.set_param(DEPTH, 0.0);
            t.set_param(WAVEFORM, wave as f32);
            t.set_param(PHASE, 180.0);
            let (l0, r0) = tones(20_000);
            let (mut l, mut r) = (l0.clone(), r0.clone());
            run(&mut t, &mut l, &mut r);
            assert_eq!((l, r), (l0, r0), "waveform {wave}");
        }
    }

    #[test]
    fn shapes_peak_at_zero_and_dip_halfway() {
        for w in 0..3 {
            assert!((shape(w, 0.0, 0.3) - 1.0).abs() < 1e-6, "{w}");
            assert!(shape(w, 0.5, 0.3).abs() < 1e-6, "{w}");
            for i in 0..100 {
                let s = shape(w, i as f64 / 100.0, 0.3);
                assert!((0.0..=1.0).contains(&s));
            }
        }
        // The square holds flat between its edges; with full smoothing it is a rounded triangle.
        assert_eq!(shape(SQUARE, 0.1, 0.2), 1.0);
        assert_eq!(shape(SQUARE, 0.4, 0.2), 0.0);
        assert!((shape(SQUARE, 0.25, 0.2) - 0.5).abs() < 1e-6);
        assert!(shape(SQUARE, 0.1, 1.0) < 0.99);
    }

    /// The gain applied to a constant signal of 1, with the song position moving on as it
    /// plays.
    fn gains(t: &mut Tremolo, len: usize, ctx: &FxContext) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![1.0; len];
        let mut r = vec![1.0; len];
        let mut ctx = *ctx;
        for (bl, br) in l.chunks_mut(480).zip(r.chunks_mut(480)) {
            t.process(bl, br, None, &ctx);
            if ctx.playing {
                ctx.position += bl.len() as f64 / SR as f64 * ctx.tempo / 60.0;
            }
        }
        (l, r)
    }

    #[test]
    fn synced_cycles_follow_the_tempo_and_the_song_position() {
        let mut t = Tremolo::new(SR);
        t.set_param(NOTE, 6.0); // 1/4: one cycle per beat, 0.5 s at 120 BPM
        t.set_param(DEPTH, 1.0);
        let ctx = FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: 0.0 };
        let (l, _) = gains(&mut t, 48_000, &ctx);
        // Peaks on the beats, silent between.
        for i in [0usize, 24_000] {
            assert!(l[i] > 0.999, "{i}: {}", l[i]);
        }
        assert!(l[12_000] < 1e-3 && l[36_000] < 1e-3);
        // Starting half a beat in, the LFO starts at its bottom.
        t.reset();
        let ctx = FxContext { position: 0.5, ..ctx };
        let (l, _) = gains(&mut t, 480, &ctx);
        assert!(l[0] < 1e-3, "{}", l[0]);
    }

    #[test]
    fn free_rate_and_auto_pan() {
        let mut t = Tremolo::new(SR);
        t.set_param(SYNC, 0.0);
        t.set_param(RATE, 2.0);
        t.set_param(DEPTH, 1.0);
        t.set_param(PHASE, 180.0);
        let ctx = FxContext { sample_rate: SR, tempo: 97.0, playing: false, position: 3.3 };
        let (l, r) = gains(&mut t, 48_000, &ctx);
        // 2 Hz: tops at 0, 0.5 s; the right side is opposite.
        assert!(l[0] > 0.999 && l[24_000] > 0.999 && l[12_000] < 1e-3);
        assert!(r[0] < 1e-3 && r[12_000] > 0.999);
        // The sides always add to one: the level moves across, it doesn't pump.
        assert!(l.iter().zip(&r).all(|(a, b)| (a + b - 1.0).abs() < 0.02));
    }

    #[test]
    fn square_edges_never_click() {
        let mut t = Tremolo::new(SR);
        t.set_param(SYNC, 0.0);
        t.set_param(RATE, 20.0);
        t.set_param(DEPTH, 1.0);
        t.set_param(WAVEFORM, 2.0);
        t.set_param(SMOOTHING, 0.0);
        let ctx = FxContext { sample_rate: SR, tempo: 120.0, playing: false, position: 0.0 };
        let (l, _) = gains(&mut t, 48_000, &ctx);
        let step = l.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        // A 2 ms edge at 48 kHz is 96 samples; a click would step by up to 1.
        assert!(step < 0.05, "largest step {step}");
        // It still reaches both ends.
        assert!(l.iter().any(|&x| x > 0.999) && l.iter().any(|&x| x < 1e-3));
    }

    #[test]
    fn extremes_and_presets_are_clean() {
        check_extremes(&info(), 1.0001);
        check_presets(&info(), PRESETS);
    }
}
