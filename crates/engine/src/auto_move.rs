//! Logic's Move Automation with Regions: when a region moves along its track, the automation
//! under it goes with it and replaces the automation where it lands. Pure, on one lane.
//!
//! The curve outside the stretches involved stays exactly as it was: where points are taken
//! away or put in, a point holding the old curve's value is added just outside (one sample
//! before the stretch, and at its end), as automation writing does in `automation::splice`.
//! The place a region leaves is bridged by a straight line between the curve's values at its
//! edges. Added points the curve doesn't need are dropped again.

use crate::automation::value_at;
use crate::model::AutoPoint;

/// A stretch of timeline that moves: `lo..hi` goes to `lo + delta..hi + delta`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shift {
    pub lo: i64,
    pub hi: i64,
    pub delta: i64,
}

/// The lane `points` (sorted) after the automation in each stretch moves by its `delta`. A
/// stretch with no points of its own carries nothing and changes nothing. Where stretches
/// land on each other, the later one in the list wins.
pub fn move_with_regions(points: &[AutoPoint], moves: &[Shift]) -> Vec<AutoPoint> {
    let moves: Vec<Shift> = moves.iter().copied().filter(|m| m.delta != 0 && m.hi > m.lo && points.iter().any(|p| p.pos >= m.lo && p.pos < m.hi)).collect();
    if moves.is_empty() {
        return points.to_vec();
    }
    // What each stretch carries, taken from the curve as it was.
    let carried: Vec<Vec<AutoPoint>> = moves.iter().map(|m| shifted(&segment(points, m.lo, m.hi), m.delta)).collect();
    let mut out = points.to_vec();
    for m in &moves {
        out = replace(&out, m.lo, m.hi, &[]);
    }
    for (m, seg) in moves.iter().zip(&carried) {
        out = replace(&out, m.lo + m.delta, m.hi + m.delta, seg);
    }
    out
}

/// The curve over samples `lo..hi` as points: its value at each end and the points between.
fn segment(points: &[AutoPoint], lo: i64, hi: i64) -> Vec<AutoPoint> {
    let at = |x: i64| AutoPoint { pos: x, value: value_at(points, x).unwrap_or(0.0) };
    let last = hi - 1;
    let mut out = vec![at(lo)];
    out.extend(points.iter().filter(|p| p.pos > lo && p.pos < last));
    if last > lo {
        out.push(at(last));
    }
    out
}

fn shifted(points: &[AutoPoint], delta: i64) -> Vec<AutoPoint> {
    points.iter().map(|p| AutoPoint { pos: p.pos + delta, ..*p }).collect()
}

/// `curve` with samples `lo..hi` replaced by `seg` (points within that range; none bridges
/// it with a straight line). Outside the range the curve is unchanged.
fn replace(curve: &[AutoPoint], lo: i64, hi: i64, seg: &[AutoPoint]) -> Vec<AutoPoint> {
    let value = |x: i64| value_at(curve, x).unwrap_or(0.0);
    // Each point and whether it was added here (and may go again if redundant).
    let mut out: Vec<(AutoPoint, bool)> = curve.iter().filter(|p| p.pos < lo).map(|p| (*p, false)).collect();
    if lo > 0 && !curve.iter().any(|p| p.pos == lo - 1) {
        out.push((AutoPoint { pos: lo - 1, value: value(lo - 1) }, true));
    }
    out.extend(seg.iter().map(|p| (*p, true)));
    if !curve.iter().any(|p| p.pos == hi) {
        out.push((AutoPoint { pos: hi, value: value(hi) }, true));
    }
    out.extend(curve.iter().filter(|p| p.pos >= hi).map(|p| (*p, false)));
    prune(out)
}

/// Drops added points that the curve is the same without: on the line between their
/// neighbours, or repeating the value an end of the lane holds anyway.
fn prune(mut pts: Vec<(AutoPoint, bool)>) -> Vec<AutoPoint> {
    let same = |a: f32, b: f32| (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0);
    let mut i = 0;
    while i < pts.len() {
        let (p, added) = pts[i];
        let prev = i.checked_sub(1).map(|k| pts[k].0);
        let next = pts.get(i + 1).map(|q| q.0);
        let redundant = added
            && match (prev, next) {
                (None, None) => false,
                (None, Some(n)) => same(n.value, p.value),
                (Some(a), None) => same(a.value, p.value),
                (Some(a), Some(b)) if a.pos == p.pos || b.pos == p.pos => (a.pos == p.pos && same(a.value, p.value)) || (b.pos == p.pos && same(b.value, p.value)),
                (Some(a), Some(b)) => same(a.value + (b.value - a.value) * ((p.pos - a.pos) as f64 / (b.pos - a.pos) as f64) as f32, p.value),
            };
        if redundant {
            pts.remove(i);
        } else {
            i += 1;
        }
    }
    pts.into_iter().map(|(p, _)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(pos: i64, value: f32) -> AutoPoint {
        AutoPoint { pos, value }
    }

    fn at(points: &[AutoPoint], x: i64) -> f32 {
        value_at(points, x).unwrap()
    }

    #[test]
    fn the_curve_under_a_region_moves_with_it() {
        // A bump at 100..200 on a flat lane at -10 dB; the region moves to 500..600.
        let lane = vec![pt(0, -10.0), pt(110, -10.0), pt(120, 0.0), pt(180, 0.0), pt(190, -10.0), pt(1000, -10.0)];
        let moved = move_with_regions(&lane, &[Shift { lo: 100, hi: 200, delta: 400 }]);
        // The bump is now at 520..580, and the old place is a straight line again.
        for x in [520, 550, 580] {
            assert!((at(&moved, x) - 0.0).abs() < 1e-4, "{x}: {}", at(&moved, x));
        }
        for x in [0, 50, 150, 250, 499, 600, 800, 1000] {
            assert!((at(&moved, x) - -10.0).abs() < 1e-4, "{x}: {}", at(&moved, x));
        }
        // The flat stretches need no extra points: just the bump's four and the lane's ends.
        assert_eq!(moved.len(), 6, "{moved:?}");
        assert!(moved.windows(2).all(|w| w[0].pos <= w[1].pos));
    }

    #[test]
    fn the_destination_is_replaced_and_the_rest_kept() {
        // A ramp from 0 to 1 over 0..1000, and a step at 600 that the move lands on.
        let lane = vec![pt(0, 0.0), pt(150, 0.9), pt(600, 0.6), pt(600, 0.2), pt(1000, 1.0)];
        let moved = move_with_regions(&lane, &[Shift { lo: 100, hi: 200, delta: 450 }]);
        // Under the region at its new place (550..650), the old curve at 100..200.
        for x in [550, 600, 640] {
            assert!((at(&moved, x) - at(&lane, x - 450)).abs() < 1e-4, "{x}");
        }
        // Outside the two stretches, the curve is untouched.
        for x in [0, 50, 99, 200, 300, 549, 650, 800, 1000] {
            assert!((at(&moved, x) - at(&lane, x)).abs() < 1e-4, "{x}: {} vs {}", at(&moved, x), at(&lane, x));
        }
        // The stretch left behind is a straight line from the value at 99 to the one at 200.
        let (a, b) = (at(&lane, 99), at(&lane, 200));
        let mid = at(&moved, 150);
        assert!((mid - (a + (b - a) * 51.0 / 101.0)).abs() < 1e-3, "{mid}");
    }

    #[test]
    fn nothing_to_carry_changes_nothing() {
        let lane = vec![pt(0, 0.0), pt(1000, 1.0)];
        assert_eq!(move_with_regions(&lane, &[Shift { lo: 100, hi: 200, delta: 300 }]), lane);
        assert_eq!(move_with_regions(&lane, &[Shift { lo: 0, hi: 200, delta: 0 }]), lane);
        assert!(move_with_regions(&[], &[Shift { lo: 0, hi: 200, delta: 10 }]).is_empty());
    }

    #[test]
    fn overlapping_moves_read_the_old_curve() {
        // Two regions swap places: each carries its own automation.
        let lane = vec![pt(0, 0.0), pt(50, 0.25), pt(99, 0.25), pt(100, 0.75), pt(150, 0.75), pt(199, 0.75), pt(200, 0.0)];
        let moved = move_with_regions(&lane, &[Shift { lo: 0, hi: 100, delta: 100 }, Shift { lo: 100, hi: 200, delta: -100 }]);
        assert!((at(&moved, 50) - 0.75).abs() < 1e-4, "{moved:?}");
        assert!((at(&moved, 150) - 0.25).abs() < 1e-4, "{moved:?}");
        assert!((at(&moved, 250) - 0.0).abs() < 1e-4, "{moved:?}");
    }
}
