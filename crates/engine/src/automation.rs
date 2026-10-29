//! Track automation: the lanes as the engine plays them, evaluating a lane at a position, and
//! the curve editing behind automation writing (splicing a recorded pass into a lane and
//! thinning out redundant points). Evaluation is allocation-free for the audio thread.

use std::sync::Arc;

use crate::model::{AutoLane, AutoParam, AutoPoint, Id};

/// What a lane drives, without the display name, so the audio thread can compare it cheaply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTarget {
    Volume,
    Pan,
    Mute,
    Send(u8),
    Plugin { uid: Id, param: u32 },
}

impl From<&AutoParam> for AutoTarget {
    fn from(p: &AutoParam) -> AutoTarget {
        match p {
            AutoParam::Volume => AutoTarget::Volume,
            AutoParam::Pan => AutoTarget::Pan,
            AutoParam::Mute => AutoTarget::Mute,
            AutoParam::Send(s) => AutoTarget::Send(*s),
            AutoParam::Plugin { uid, param, .. } => AutoTarget::Plugin { uid: *uid, param: *param },
        }
    }
}

/// A lane as the engine plays it. Shipped to the audio thread inside an `Arc` and never
/// changed there; an edit sends a whole new list.
pub struct LaneDsp {
    pub target: AutoTarget,
    pub points: Vec<AutoPoint>,
}

/// The engine's copy of a track's lanes (empty lanes are left out).
pub fn lanes_dsp(lanes: &[AutoLane]) -> Arc<Vec<LaneDsp>> {
    Arc::new(
        lanes
            .iter()
            .filter(|l| !l.points.is_empty())
            .map(|l| {
                let mut points = l.points.clone();
                points.sort_by_key(|p| p.pos);
                LaneDsp { target: AutoTarget::from(&l.param), points }
            })
            .collect(),
    )
}

/// The lane's value at `pos`: straight lines between points, the first point's value before
/// it and the last point's value after it. Two points at the same position make a step (the
/// later one applies from there on). None for an empty lane.
pub fn value_at(points: &[AutoPoint], pos: i64) -> Option<f32> {
    let first = points.first()?;
    if pos < first.pos {
        return Some(first.value);
    }
    let i = points.partition_point(|p| p.pos <= pos);
    let Some(b) = points.get(i) else { return points.last().map(|p| p.value) };
    let a = points[i - 1];
    // a.pos <= pos < b.pos, so the span is never empty.
    let t = (pos - a.pos) as f64 / (b.pos - a.pos) as f64;
    Some(a.value + (b.value - a.value) * t as f32)
}

/// How far a written value may stray from the thinned curve, in the parameter's units.
pub fn tolerance(param: &AutoParam) -> f32 {
    match param {
        AutoParam::Volume | AutoParam::Send(_) => 0.05,
        AutoParam::Pan => 0.004,
        AutoParam::Mute => 0.0,
        AutoParam::Plugin { .. } => 0.002,
    }
}

/// Drops points the curve doesn't need: every removed point lies within `tol` of the line the
/// remaining points draw (Ramer-Douglas-Peucker on the vertical error).
pub fn thin(points: &[AutoPoint], tol: f32) -> Vec<AutoPoint> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut spans = vec![(0usize, points.len() - 1)];
    while let Some((a, b)) = spans.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (points[a], points[b]);
        let mut worst = (0.0f32, a);
        for (k, p) in points.iter().enumerate().take(b).skip(a + 1) {
            let line = if pb.pos == pa.pos {
                // A vertical step: anything in between is a third value.
                if (p.value - pa.value).abs() < (p.value - pb.value).abs() { pa.value } else { pb.value }
            } else {
                pa.value + (pb.value - pa.value) * ((p.pos - pa.pos) as f64 / (pb.pos - pa.pos) as f64) as f32
            };
            let err = (p.value - line).abs();
            if err > worst.0 {
                worst = (err, k);
            }
        }
        if worst.0 > tol {
            keep[worst.1] = true;
            spans.push((a, worst.1));
            spans.push((worst.1, b));
        }
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Puts a recorded pass into a lane, replacing the lane's points from `start` to `end`.
///
/// The old curve carries on around the pass: it holds up to just before `start`, and when
/// `return_to_curve` is set (Touch), or the lane has points after the pass, it comes back
/// `glide` samples after `end`. Otherwise the last written value holds (Latch, Write). An
/// empty lane counts as holding `before`, the value the parameter had before writing. The
/// written stretch is thinned to within `tol`.
#[allow(clippy::too_many_arguments)]
pub fn splice(lane: &[AutoPoint], recorded: &[AutoPoint], start: i64, end: i64, before: f32, return_to_curve: bool, glide: i64, tol: f32) -> Vec<AutoPoint> {
    if recorded.is_empty() {
        return lane.to_vec();
    }
    let base: Vec<AutoPoint> = if lane.is_empty() { vec![AutoPoint { pos: 0, value: before }] } else { lane.to_vec() };
    let back = end + glide.max(1);
    let returns = return_to_curve || lane.iter().any(|p| p.pos > end);
    let mut out: Vec<AutoPoint> = base.iter().filter(|p| p.pos < start).copied().collect();
    let written_from = out.len();
    if start > 0 {
        let v = value_at(&base, start - 1).unwrap_or(before);
        if out.last().map(|p| p.pos < start - 1 || p.value != v).unwrap_or(true) {
            out.push(AutoPoint { pos: start - 1, value: v });
        }
    }
    out.extend(recorded.iter().filter(|p| p.pos >= start && p.pos <= end));
    if returns {
        let v = value_at(&base, back).unwrap_or(before);
        out.push(AutoPoint { pos: back, value: v });
    }
    let written = thin(&out[written_from..], tol);
    out.truncate(written_from);
    out.extend(written);
    if returns {
        out.extend(base.iter().filter(|p| p.pos > back));
    }
    // An empty lane that ends up holding one value everywhere needs a single point.
    if lane.is_empty() && out.iter().all(|p| p.value == before) {
        out.truncate(1);
    }
    out
}

/// Gives plug-in lanes the new instance ids of copied plug-ins (`map` is old -> new).
pub fn remap_plugins(lanes: &mut [AutoLane], map: &[(Id, Id)]) {
    for l in lanes {
        if let AutoParam::Plugin { uid, .. } = &mut l.param {
            if let Some((_, new)) = map.iter().find(|(old, _)| old == uid) {
                *uid = *new;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(v: &[(i64, f32)]) -> Vec<AutoPoint> {
        v.iter().map(|&(pos, value)| AutoPoint { pos, value }).collect()
    }

    #[test]
    fn empty_lane_has_no_value() {
        assert_eq!(value_at(&[], 100), None);
    }

    #[test]
    fn holds_outside_the_points() {
        let p = pts(&[(100, -6.0), (200, 0.0)]);
        assert_eq!(value_at(&p, 0), Some(-6.0));
        assert_eq!(value_at(&p, 100), Some(-6.0));
        assert_eq!(value_at(&p, 200), Some(0.0));
        assert_eq!(value_at(&p, 10_000), Some(0.0));
    }

    #[test]
    fn interpolates_linearly() {
        let p = pts(&[(0, 0.0), (100, 1.0), (300, -1.0)]);
        assert!((value_at(&p, 50).unwrap() - 0.5).abs() < 1e-6);
        assert!((value_at(&p, 200).unwrap() - 0.0).abs() < 1e-6);
        assert!((value_at(&p, 250).unwrap() + 0.5).abs() < 1e-6);
    }

    #[test]
    fn same_position_is_a_step() {
        let p = pts(&[(0, 0.0), (100, 0.0), (100, 1.0), (200, 1.0)]);
        assert_eq!(value_at(&p, 99), Some(0.0));
        assert_eq!(value_at(&p, 100), Some(1.0));
        assert_eq!(value_at(&p, 150), Some(1.0));
    }

    #[test]
    fn single_point_holds_everywhere() {
        let p = pts(&[(500, 0.25)]);
        assert_eq!(value_at(&p, 0), Some(0.25));
        assert_eq!(value_at(&p, 9999), Some(0.25));
    }

    #[test]
    fn thinning_keeps_corners_only() {
        // A ramp up, a hold, a ramp down, sampled densely.
        let mut v = Vec::new();
        for i in 0..=10 {
            v.push((i * 10, i as f32));
        }
        for i in 11..=20 {
            v.push((i * 10, 10.0));
        }
        for i in 21..=30 {
            v.push((i * 10, 10.0 - (i - 20) as f32));
        }
        let t = thin(&pts(&v), 0.01);
        assert_eq!(t, pts(&[(0, 0.0), (100, 10.0), (200, 10.0), (300, 0.0)]));
    }

    #[test]
    fn thinning_respects_tolerance() {
        let p = pts(&[(0, 0.0), (10, 0.03), (20, 0.0)]);
        assert_eq!(thin(&p, 0.05).len(), 2);
        assert_eq!(thin(&p, 0.01).len(), 3);
    }

    #[test]
    fn touch_returns_to_the_old_curve() {
        let lane = pts(&[(0, 0.0), (1000, 0.0)]);
        let rec = pts(&[(200, -10.0), (400, -10.0)]);
        let out = splice(&lane, &rec, 200, 400, 0.0, true, 50, 0.0);
        assert_eq!(value_at(&out, 100), Some(0.0));
        assert_eq!(value_at(&out, 300), Some(-10.0));
        assert_eq!(value_at(&out, 450), Some(0.0));
        assert_eq!(value_at(&out, 1000), Some(0.0));
    }

    #[test]
    fn latch_holds_the_last_value() {
        let lane = pts(&[(0, 0.0)]);
        let rec = pts(&[(200, -3.0), (400, -12.0)]);
        let out = splice(&lane, &rec, 200, 400, 0.0, false, 50, 0.0);
        assert_eq!(value_at(&out, 100), Some(0.0));
        assert_eq!(value_at(&out, 5000), Some(-12.0));
    }

    #[test]
    fn latch_keeps_later_points() {
        let lane = pts(&[(0, 0.0), (2000, 6.0)]);
        let rec = pts(&[(200, -3.0), (400, -3.0)]);
        let out = splice(&lane, &rec, 200, 400, 0.0, false, 50, 0.0);
        assert_eq!(value_at(&out, 2000), Some(6.0));
        assert_eq!(value_at(&out, 300), Some(-3.0));
    }

    #[test]
    fn writing_an_empty_lane_keeps_the_value_before() {
        let rec = pts(&[(1000, -6.0), (2000, -6.0)]);
        let out = splice(&[], &rec, 1000, 2000, -2.0, false, 50, 0.0);
        assert_eq!(value_at(&out, 0), Some(-2.0));
        assert_eq!(value_at(&out, 1500), Some(-6.0));
        assert_eq!(value_at(&out, 9000), Some(-6.0));
    }

    #[test]
    fn params_serialize_as_the_ui_expects() {
        let j = |p: &AutoParam| serde_json::to_string(p).unwrap();
        assert_eq!(j(&AutoParam::Volume), r#"{"type":"volume"}"#);
        assert_eq!(j(&AutoParam::Send(2)), r#"{"type":"send","value":2}"#);
        let p = AutoParam::Plugin { uid: 7, param: 42, name: "Cutoff".into() };
        assert_eq!(j(&p), r#"{"type":"plugin","value":{"uid":7,"param":42,"name":"Cutoff"}}"#);
        let back: AutoParam = serde_json::from_str(&j(&p)).unwrap();
        assert!(back.same(&p) && !back.same(&AutoParam::Plugin { uid: 7, param: 43, name: "Cutoff".into() }));
    }

    #[test]
    fn replaces_points_inside_the_pass() {
        let lane = pts(&[(0, 0.0), (300, 5.0), (600, 0.0)]);
        let rec = pts(&[(200, 1.0), (400, 1.0)]);
        let out = splice(&lane, &rec, 200, 400, 0.0, true, 10, 0.0);
        assert!(!out.iter().any(|p| p.pos == 300));
    }
}
