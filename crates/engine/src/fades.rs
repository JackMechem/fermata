//! Fade shapes and crossfades between audio regions, after Logic's Region inspector: a curve
//! for each fade, Speed Up and Slow Down fades, and the fade-out types that crossfade a region
//! with the one that follows it (X, EqP and X S).
//!
//! The project stores each region's [`FadeShape`] next to its fade lengths. [`crossfades`]
//! works out where adjacent or overlapping regions crossfade, and the session turns both into
//! [`FadeLaw`]s for the engine, so playback, bounces, exports, freezes and bounce-in-place all
//! render fades through the same code (`dsp::play_audio`).

use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::model::{Id, Region, RegionContent};

/// The curve fades had before shapes could be chosen: a quarter sine. It stays the default, so
/// projects saved earlier sound the same.
pub const DEFAULT_CURVE: f32 = 0.5;

/// The default of Logic's Crossfade Time setting (`EditingPrefs::crossfade_ms`).
pub const DEFAULT_XFADE_MS: f64 = 20.0;

/// What a region's fade-in does (the pop-up on the inspector's Fade In row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeInType {
    #[default]
    Fade,
    /// The playback speed rises from standstill to normal over the fade, like a tape starting.
    SpeedUp,
}

/// What a region's fade-out does (the inspector's Fade Out pop-up and Type row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeOutType {
    /// A plain fade-out ("Out").
    #[default]
    Out,
    /// Crossfade with the following region, the two gains summing to unity ("X").
    #[serde(rename = "x")]
    X,
    /// Equal-power crossfade: the two powers sum to one, so uncorrelated material doesn't dip
    /// in the middle ("EqP").
    #[serde(rename = "eqp")]
    EqP,
    /// S-curved crossfade: slow at both ends, quick through the middle ("X S").
    #[serde(rename = "xs")]
    XS,
    /// The playback speed falls to standstill over the fade, like a tape stopping.
    SlowDown,
}

impl FadeOutType {
    /// One of the types that crossfade with the following region.
    pub fn is_crossfade(self) -> bool {
        matches!(self, FadeOutType::X | FadeOutType::EqP | FadeOutType::XS)
    }
}

/// How a region's fades sound. The lengths stay in `RegionContent::Audio`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FadeShape {
    /// Curve of the fade-in, from -1 to 1 (the inspector shows -100 to 100): 0 is a straight
    /// line, positive values bow outwards (the level rises quickly, then levels off), negative
    /// ones inwards. [`DEFAULT_CURVE`] is a quarter sine.
    pub in_curve: f32,
    /// Curve of the fade-out, the same way round: positive keeps the level up longer.
    pub out_curve: f32,
    pub in_type: FadeInType,
    pub out_type: FadeOutType,
    /// A looping region crossfades each repetition into the next (equal power, over the
    /// Crossfade Time), so a loop whose end doesn't meet its start doesn't click. Logic has
    /// no such option; it is off unless chosen.
    pub loop_xfade: bool,
}

impl Default for FadeShape {
    fn default() -> Self {
        FadeShape { in_curve: DEFAULT_CURVE, out_curve: DEFAULT_CURVE, in_type: FadeInType::Fade, out_type: FadeOutType::Out, loop_xfade: false }
    }
}

impl FadeShape {
    pub fn is_default(&self) -> bool {
        *self == FadeShape::default()
    }

    /// The shape of a reversed region: what faded in now fades out, and a speed-up becomes a
    /// slow-down. A crossfade doesn't survive the reversal.
    pub fn reversed(&self) -> FadeShape {
        FadeShape {
            in_curve: self.out_curve,
            out_curve: self.in_curve,
            in_type: if self.out_type == FadeOutType::SlowDown { FadeInType::SpeedUp } else { FadeInType::Fade },
            out_type: if self.in_type == FadeInType::SpeedUp { FadeOutType::SlowDown } else { FadeOutType::Out },
            loop_xfade: self.loop_xfade,
        }
    }
}

/// How regions behave when an edit makes them overlap (Logic's Drag pop-up in the Tracks area).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragMode {
    /// Regions keep their borders; the newer one is heard where they overlap.
    #[default]
    Overlap,
    /// Moving, trimming or pasting audio regions so they overlap crossfades the overlap.
    #[serde(rename = "xfade")]
    XFade,
    /// Regions can't overlap: the earlier of two overlapping regions is shortened to where
    /// the later one starts (see `drag_modes.rs`).
    NoOverlap,
    /// The mirror image of Shuffle L: regions are pushed and pulled rightwards, an edited
    /// region's end stays put and the regions before it move.
    ShuffleR,
    /// Regions stay butted together: deleting one pulls the regions after it left, an edited
    /// region's start stays put and the regions after it move.
    ShuffleL,
}

/// Fade gain at `x`, from 0 (silent) to 1 (full level), for curve `c` in -1..1.
///
/// 0 is linear. Up to 0.5 the curve is `sin(θx) / sin(θ)` with `θ = cπ`, which reaches a
/// quarter sine at 0.5 (the old fixed fade, bit for bit). Beyond that the sine is pushed
/// further, to `1 - (1 - sin)^4` at 1. Negative curves mirror positive ones through the
/// centre. Every curve runs from (0, 0) to (1, 1) and rises throughout.
pub fn curve(x: f32, c: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let c = c.clamp(-1.0, 1.0);
    if c < 0.0 {
        return 1.0 - curve(1.0 - x, -c);
    }
    if c < 1e-4 {
        return x;
    }
    if c <= 0.5 {
        let th = c * PI;
        return (x * th).sin() / th.sin();
    }
    let s = (x * FRAC_PI_2).sin();
    1.0 - (1.0 - s).powf(1.0 + 6.0 * (c - 0.5))
}

/// The S-curve crossfade's gain: a raised cosine, so `s(x) + s(1 - x) = 1`.
pub fn s_curve(x: f32) -> f32 {
    0.5 - 0.5 * (x.clamp(0.0, 1.0) * PI).cos()
}

/// Points in a [`FadeLaw::Speed`] table.
const SPEED_STEPS: usize = 256;

/// For a speed fade whose speed follows `curve(x, c)`, how far (as a fraction of the fade's
/// length) the audio at `x` is from where it plays without the fade: `D(x) = ∫ₓ¹ (1 - v)`,
/// tabulated at `SPEED_STEPS + 1` points. Speed Up plays its fade `D(x)` ahead, so it meets
/// normal playback at the fade's end; Slow Down plays `D(x)` behind, leaving it at the start.
pub fn speed_table(c: f32) -> Vec<f32> {
    const SUB: usize = 16;
    let n = SPEED_STEPS * SUB;
    let mut out = vec![0.0f32; SPEED_STEPS + 1];
    // Integrate from the top down (D(1) = 0) with the trapezoid rule.
    let mut acc = 0.0f64;
    let lag = |k: usize| 1.0 - curve(k as f32 / n as f32, c) as f64;
    for k in (0..n).rev() {
        acc += 0.5 * (lag(k) + lag(k + 1)) / n as f64;
        if k % SUB == 0 {
            out[k / SUB] = acc as f32;
        }
    }
    out
}

/// A fade's gain law as the engine plays it, for `x` from 0 (silent) to 1 (full level).
#[derive(Debug, Clone)]
pub enum FadeLaw {
    /// [`curve`] with this curve.
    Curve(f32),
    /// `1 - curve(1 - x, c)`: the incoming side of an X crossfade, so the two sides sum to one.
    Complement(f32),
    /// [`s_curve`], both sides of an X S crossfade.
    SCurve,
    /// Speed Up or Slow Down: the gain follows the curve, and the playback speed does too,
    /// read from the [`speed_table`] for the curve.
    Speed(f32, Arc<Vec<f32>>),
}

impl Default for FadeLaw {
    fn default() -> Self {
        FadeLaw::Curve(DEFAULT_CURVE)
    }
}

impl FadeLaw {
    pub fn gain(&self, x: f32) -> f32 {
        match self {
            FadeLaw::Curve(c) | FadeLaw::Speed(c, _) => curve(x, *c),
            FadeLaw::Complement(c) => 1.0 - curve(1.0 - x, *c),
            FadeLaw::SCurve => s_curve(x),
        }
    }

    /// For a speed fade, `D(x)` (see [`speed_table`]); 0 otherwise. Doesn't allocate.
    pub fn lag(&self, x: f32) -> f32 {
        let FadeLaw::Speed(_, t) = self else { return 0.0 };
        let p = x.clamp(0.0, 1.0) * SPEED_STEPS as f32;
        let i = (p as usize).min(SPEED_STEPS - 1);
        let f = p - i as f32;
        t[i] + (t[i + 1] - t[i]) * f
    }

    /// The law for a region's own fade-in.
    pub fn fade_in(s: &FadeShape) -> FadeLaw {
        match s.in_type {
            FadeInType::Fade => FadeLaw::Curve(s.in_curve),
            FadeInType::SpeedUp => FadeLaw::Speed(s.in_curve, Arc::new(speed_table(s.in_curve))),
        }
    }

    /// The law for a region's own fade-out. A crossfade type with nothing to crossfade with
    /// fades out with the region's curve, as "Out" does.
    pub fn fade_out(s: &FadeShape) -> FadeLaw {
        match s.out_type {
            FadeOutType::SlowDown => FadeLaw::Speed(s.out_curve, Arc::new(speed_table(s.out_curve))),
            _ => FadeLaw::Curve(s.out_curve),
        }
    }
}

/// A crossfade between a region and the one that follows it on the track.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Crossfade {
    pub out_region: Id,
    pub in_region: Id,
    pub kind: FadeOutType,
    /// The outgoing region's fade-out curve (used by X).
    pub curve: f32,
    /// Timeline (start, length) of the outgoing region's fade. The region plays to its end,
    /// past its own end where its file has audio there.
    pub fade_out: (i64, i64),
    /// Timeline (start, length) of the incoming region's fade. The region plays from its
    /// start, before its own start where its file has audio there.
    pub fade_in: (i64, i64),
}

impl Crossfade {
    /// The gain laws of the outgoing and incoming sides.
    pub fn laws(&self) -> (FadeLaw, FadeLaw) {
        match self.kind {
            FadeOutType::EqP => (FadeLaw::Curve(DEFAULT_CURVE), FadeLaw::Curve(DEFAULT_CURVE)),
            FadeOutType::XS => (FadeLaw::SCurve, FadeLaw::SCurve),
            _ => (FadeLaw::Curve(self.curve), FadeLaw::Complement(self.curve)),
        }
    }
}

/// Whether a region can take part in a crossfade: audio (a plain region, a take folder or a
/// loop, which crossfades at its loop end) that is heard.
fn crossfadeable(r: &Region) -> bool {
    matches!(r.content, RegionContent::Audio { .. }) && !r.muted && r.length > 0
}

/// The region a crossfade from `a` goes into: the next one to start inside `a` (or right at its
/// end, its loop end for a loop) that runs on past it. None when a gap follows.
pub fn following<'a>(regions: &'a [Region], a: &Region) -> Option<&'a Region> {
    let a_end = a.end();
    regions
        .iter()
        .filter(|b| b.id != a.id && crossfadeable(b) && b.start > a.start && b.start <= a_end && b.end() > a_end)
        .min_by_key(|b| (b.start, b.id))
}

/// How much audio a region's files have to spare for a crossfade: before its start, and past
/// where it stops (its loop end for a loop). A take folder has what its comp has beyond its
/// ends, in the stretches heard there (none where the comp is silent). None when a file is
/// missing.
fn spare(r: &Region, clip_frames: &impl Fn(Id) -> Option<i64>) -> Option<(i64, i64)> {
    let RegionContent::Audio { clip, offset, .. } = &r.content else { return None };
    let last = r.passes().last().map(|p| p.1).unwrap_or(r.length);
    let Some(f) = &r.takes else {
        let frames = clip_frames(*clip)?;
        return Some(((*offset).max(0), (frames - (offset + last)).max(0)));
    };
    // How far the comp stretch heard at take time `t` reaches before and after it.
    let heard = |t: i64| -> Option<Option<(i64, i64)>> {
        let Some(s) = f.comp.iter().find(|s| s.start <= t && t < s.end) else { return Some(None) };
        clip_frames(f.takes.iter().find(|k| k.id == s.take)?.clip)?;
        Some(Some((t - s.start, s.end - t)))
    };
    let before = heard(*offset)?.map(|h| h.0).unwrap_or(0);
    let past = heard(offset + last - 1)?.map(|h| h.1 - 1).unwrap_or(0);
    Some((before.max(0), past.max(0)))
}

/// Where a crossfade of `len` frames from region A (`a_start..a_end`, its file having `past`
/// frames beyond that) into region B (`b_start..b_end`, `before` frames before) goes: the
/// timeline (start, length) of A's fade-out and of B's fade-in. See [`crossfades`].
fn zone(a: (i64, i64), b: (i64, i64), past: i64, before: i64, len: i64) -> ((i64, i64), (i64, i64)) {
    let ((a_start, a_end), (b_start, b_end)) = (a, b);
    let lo = a_start.max(b_start - before);
    let hi = b_end.min(a_end + past);
    let pivot = (b_start + a_end) / 2;
    let room = hi - lo;
    if room * 4 >= len {
        let l = len.min(room);
        let s = (pivot - l / 2).clamp(lo, hi - l);
        ((s, l), (s, l))
    } else {
        let h = (len / 2).min(pivot - a_start).min(b_end - pivot).max(0);
        ((pivot - h, h), (pivot, h))
    }
}

/// The crossfades on a track. A region whose fade-out type is X, EqP or X S crossfades with
/// the [`following`] region over its fade-out length, centred on the junction (or on the
/// middle of the overlap). The crossfade needs audio from both: the outgoing region's file
/// past its end and the incoming one's before its start. Where the files run out it slides
/// and shortens to fit, and where neither has any to spare the two butt-fade at the junction
/// over half the length each, which avoids a click at the cost of a dip. The incoming region's
/// own fade-in is not used meanwhile, as in Logic.
///
/// Take folders crossfade like any region, with the takes heard at their ends. A loop
/// crossfades at its loop end, from its last repetition.
///
/// `clip_frames` gives a clip's length (None: missing, so no crossfade).
pub fn crossfades(regions: &[Region], clip_frames: impl Fn(Id) -> Option<i64>) -> Vec<Crossfade> {
    let mut order: Vec<&Region> = regions.iter().filter(|r| crossfadeable(r)).collect();
    order.sort_by_key(|r| (r.start, r.id));
    let mut out: Vec<Crossfade> = Vec::new();
    for a in order {
        let RegionContent::Audio { fade_out, fade_shape, .. } = &a.content else { continue };
        let last = a.passes().last().copied().unwrap_or((a.start, a.length));
        let len = (*fade_out).min(last.1);
        if !fade_shape.out_type.is_crossfade() || len <= 0 {
            continue;
        }
        let Some(b) = following(regions, a) else { continue };
        // One crossfade into each region.
        if out.iter().any(|x| x.in_region == b.id) {
            continue;
        }
        let (Some((_, past)), Some((before, _))) = (spare(a, &clip_frames), spare(b, &clip_frames)) else { continue };
        let (fo, fi) = zone((last.0, a.end()), (b.start, b.start + b.length), past, before, len);
        out.push(Crossfade { out_region: a.id, in_region: b.id, kind: fade_shape.out_type, curve: fade_shape.out_curve, fade_out: fo, fade_in: fi });
    }
    out
}

/// The crossfades between a looping region's repetitions when it has [`FadeShape::loop_xfade`]
/// on: for each repetition after the first, the timeline (start, length) of the fade-out of
/// the pass before it and of its own fade-in, `len` frames centred on the seam where the file
/// has audio to spare (as [`crossfades`] places them). Empty for anything else.
pub fn loop_seams(r: &Region, clip_frames: impl Fn(Id) -> Option<i64>, len: i64) -> Vec<((i64, i64), (i64, i64))> {
    let RegionContent::Audio { clip, offset, fade_shape, .. } = &r.content else { return Vec::new() };
    if !fade_shape.loop_xfade || !r.is_looped() || r.takes.is_some() || len <= 0 {
        return Vec::new();
    }
    let Some(frames) = clip_frames(*clip) else { return Vec::new() };
    let (before, past) = ((*offset).max(0), (frames - (offset + r.length)).max(0));
    let passes = r.passes();
    passes.windows(2).map(|w| zone((w[0].0, w[0].0 + w[0].1), (w[1].0, w[1].0 + w[1].1), past, before, len.min(r.length))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_run_from_silence_to_full_and_rise() {
        for c in [-1.0, -0.7, -0.5, -0.2, 0.0, 0.2, 0.5, 0.8, 1.0] {
            assert_eq!(curve(0.0, c), 0.0, "c = {c}");
            assert!((curve(1.0, c) - 1.0).abs() < 1e-6, "c = {c}");
            let mut prev = 0.0;
            for k in 1..=1000 {
                let g = curve(k as f32 / 1000.0, c);
                assert!(g >= prev - 1e-6, "c = {c} falls at {k}");
                prev = g;
            }
        }
        // Out of range clamps.
        assert_eq!(curve(-0.5, 0.3), 0.0);
        assert_eq!(curve(1.5, 0.3), 1.0);
    }

    #[test]
    fn curve_shapes() {
        for k in 0..=100 {
            let x = k as f32 / 100.0;
            assert_eq!(curve(x, 0.0), x, "0 is linear");
            // The default reproduces the fixed quarter sine fades had before, exactly.
            assert_eq!(curve(x, DEFAULT_CURVE), (x * FRAC_PI_2).sin());
            // Negative curves mirror positive ones through the centre.
            assert!((curve(x, -0.6) - (1.0 - curve(1.0 - x, 0.6))).abs() < 1e-6);
        }
        // Positive bows out, negative in, more as the value grows.
        assert!(curve(0.25, 1.0) > curve(0.25, 0.8) && curve(0.25, 0.8) > curve(0.25, 0.5));
        assert!(curve(0.25, 0.5) > curve(0.25, 0.2) && curve(0.25, 0.2) > 0.25);
        assert!(curve(0.25, -0.5) < 0.25);
        // Continuous across the joins of the formula.
        assert!((curve(0.3, 0.5001) - curve(0.3, 0.5)).abs() < 1e-3);
        assert!((curve(0.3, 1e-3) - 0.3).abs() < 1e-3);
    }

    #[test]
    fn crossfade_gains_sum() {
        let mk = |kind| Crossfade { out_region: 1, in_region: 2, kind, curve: 0.0, fade_out: (0, 100), fade_in: (0, 100) };
        for k in 0..=200 {
            let p = k as f32 / 200.0;
            // The outgoing side is at 1 - p of the way to full level, the incoming at p.
            let (o, i) = mk(FadeOutType::EqP).laws();
            let (go, gi) = (o.gain(1.0 - p), i.gain(p));
            assert!((go * go + gi * gi - 1.0).abs() < 1e-5, "equal power at {p}");
            for c in [0.0, 0.5, -0.8] {
                let x = Crossfade { curve: c, ..mk(FadeOutType::X) };
                let (o, i) = x.laws();
                assert!((o.gain(1.0 - p) + i.gain(p) - 1.0).abs() < 1e-6, "X sums to unity at {p}, curve {c}");
            }
            let (o, i) = mk(FadeOutType::XS).laws();
            assert!((o.gain(1.0 - p) + i.gain(p) - 1.0).abs() < 1e-6, "X S sums to unity at {p}");
        }
        // Linear X is a straight line; X S is flat at both ends.
        let (_, i) = mk(FadeOutType::X).laws();
        assert!((i.gain(0.3) - 0.3).abs() < 1e-6);
        assert!(s_curve(0.05) < 0.01 && s_curve(0.95) > 0.99);
    }

    #[test]
    fn speed_table_integrates_the_lost_time() {
        // Linear speed ramp: D(x) = ∫ₓ¹ (1 - u) du = (1 - x)² / 2.
        let t = speed_table(0.0);
        let law = FadeLaw::Speed(0.0, Arc::new(t));
        for k in 0..=10 {
            let x = k as f32 / 10.0;
            assert!((law.lag(x) - (1.0 - x).powi(2) / 2.0).abs() < 1e-4, "at {x}");
        }
        assert_eq!(law.lag(1.0), 0.0);
        assert_eq!(FadeLaw::Curve(0.0).lag(0.5), 0.0);
    }

    fn region(id: Id, start: i64, length: i64, offset: i64, fade_out: i64, out_type: FadeOutType) -> Region {
        Region {
            id,
            name: format!("R{id}"),
            start,
            length,
            muted: false,
            color: None,
            content: RegionContent::Audio { clip: 1, offset, gain_db: 0.0, fade_in: 0, fade_out, fade_shape: FadeShape { out_type, ..FadeShape::default() } },
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: None,
        }
    }

    #[test]
    fn crossfade_zones() {
        let frames = |_| Some(100_000);
        // Adjacent, with plenty of audio either side: centred on the junction.
        let rs = vec![region(1, 1000, 1000, 5000, 200, FadeOutType::EqP), region(2, 2000, 1000, 8000, 0, FadeOutType::Out)];
        let x = crossfades(&rs, frames);
        assert_eq!(x.len(), 1);
        assert_eq!((x[0].out_region, x[0].in_region, x[0].fade_out, x[0].fade_in), (1, 2, (1900, 200), (1900, 200)));
        // Overlapping: centred on the overlap.
        let rs = vec![region(1, 1000, 1000, 5000, 400, FadeOutType::X), region(2, 1800, 1000, 8000, 0, FadeOutType::Out)];
        assert_eq!(crossfades(&rs, frames)[0].fade_out, (1700, 400));
        // The incoming region's file starts right at it: the crossfade slides after the junction.
        let rs = vec![region(1, 1000, 1000, 5000, 200, FadeOutType::X), region(2, 2000, 1000, 0, 0, FadeOutType::Out)];
        assert_eq!(crossfades(&rs, frames)[0].fade_in, (2000, 200));
        // Neither file has audio to spare: butt fades of half the length each.
        let rs = vec![region(1, 1000, 1000, 99_000, 200, FadeOutType::X), region(2, 2000, 1000, 0, 0, FadeOutType::Out)];
        let x = crossfades(&rs, frames);
        assert_eq!((x[0].fade_out, x[0].fade_in), ((1900, 100), (2000, 100)));
        // A gap, an Out type, or no fade length: no crossfade.
        let rs = vec![region(1, 1000, 1000, 5000, 200, FadeOutType::X), region(2, 2001, 1000, 8000, 0, FadeOutType::Out)];
        assert!(crossfades(&rs, frames).is_empty());
        let rs = vec![region(1, 1000, 1000, 5000, 200, FadeOutType::Out), region(2, 2000, 1000, 8000, 0, FadeOutType::Out)];
        assert!(crossfades(&rs, frames).is_empty());
        let rs = vec![region(1, 1000, 1000, 5000, 0, FadeOutType::X), region(2, 2000, 1000, 8000, 0, FadeOutType::Out)];
        assert!(crossfades(&rs, frames).is_empty());
        // A region inside another isn't the following one.
        let rs = vec![region(1, 1000, 1000, 5000, 200, FadeOutType::X), region(2, 1200, 300, 8000, 0, FadeOutType::Out)];
        assert!(crossfades(&rs, frames).is_empty());
    }

    #[test]
    fn folders_and_loops_crossfade() {
        use crate::model::{CompSegment, Take, TakeFolder};
        let frames = |_| Some(100_000);
        // A take folder whose comp has audio either side of its ends, then a plain region.
        let mut folder = region(1, 1000, 1000, 500, 200, FadeOutType::EqP);
        let take = Take { id: 7, name: "Take 1".into(), clip: 3, offset: 0, start: 0, end: 5000, gain_db: 0.0, notes: Vec::new(), cc: Vec::new() };
        folder.takes = Some(TakeFolder { takes: vec![take], comp: vec![CompSegment { start: 0, end: 5000, take: 7 }], ..TakeFolder::new() });
        let rs = vec![folder.clone(), region(2, 2000, 1000, 8000, 0, FadeOutType::Out)];
        assert_eq!(crossfades(&rs, frames)[0].fade_out, (1900, 200));
        // The comp ends right at the folder's end: the crossfade slides before the junction,
        // the next region playing from earlier in its file.
        folder.takes.as_mut().unwrap().comp[0].end = 1500;
        let rs = vec![folder, region(2, 2000, 1000, 8000, 0, FadeOutType::Out)];
        assert_eq!(crossfades(&rs, frames)[0].fade_out, (1800, 200));
        // A loop crossfades at its loop end, into a region starting there.
        let mut looped = region(1, 1000, 1000, 5000, 200, FadeOutType::X);
        looped.loop_end = Some(3500);
        let rs = vec![looped.clone(), region(2, 3500, 1000, 8000, 0, FadeOutType::Out)];
        let x = crossfades(&rs, frames);
        assert_eq!((x[0].in_region, x[0].fade_out), (2, (3400, 200)));
        assert!(loop_seams(&looped, frames, 100).is_empty(), "seams only when asked for");
        if let RegionContent::Audio { fade_shape, .. } = &mut looped.content {
            fade_shape.loop_xfade = true;
        }
        // Two seams (at 2000 and 3000), each centred.
        assert_eq!(loop_seams(&looped, frames, 100), vec![((1950, 100), (1950, 100)), ((2950, 100), (2950, 100))]);
        // At the start of its file: no audio before, so the seam slides after it.
        let mut at_zero = looped.clone();
        if let RegionContent::Audio { offset, .. } = &mut at_zero.content {
            *offset = 0;
        }
        assert_eq!(loop_seams(&at_zero, frames, 100)[0].1, (2000, 100));
    }

    #[test]
    fn reversed_shape_swaps_ends() {
        let s = FadeShape { in_curve: 0.2, out_curve: -0.4, in_type: FadeInType::SpeedUp, out_type: FadeOutType::EqP, loop_xfade: false };
        let r = s.reversed();
        assert_eq!((r.in_curve, r.out_curve, r.in_type, r.out_type), (-0.4, 0.2, FadeInType::Fade, FadeOutType::SlowDown));
    }

    #[test]
    fn old_projects_load_with_the_default_shape() {
        let c: RegionContent = serde_json::from_str(r#"{"type":"audio","clip":3,"offset":0,"gain_db":0,"fade_in":10,"fade_out":20}"#).unwrap();
        let RegionContent::Audio { fade_shape, .. } = c else { panic!() };
        assert!(fade_shape.is_default());
        // And an unchanged shape isn't written out.
        let back = serde_json::to_string(&RegionContent::Audio { clip: 3, offset: 0, gain_db: 0.0, fade_in: 10, fade_out: 20, fade_shape }).unwrap();
        assert!(!back.contains("fade_shape"));
        let s: FadeShape = serde_json::from_str(r#"{"out_type":"xs"}"#).unwrap();
        assert_eq!((s.out_type, s.in_curve), (FadeOutType::XS, DEFAULT_CURVE));
    }
}
