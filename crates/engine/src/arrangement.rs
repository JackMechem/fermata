//! The Arrangement track's arithmetic (Logic's arrangement markers). Sections tile the
//! timeline, each starting where the one before it ends. Moving, copying or deleting a section
//! rearranges the project's content with it; that is described here as a time map, a list of
//! [`Piece`]s saying where each stretch of the old timeline lands. `session/arrangement.rs`
//! applies the map to the regions; automation and positions are mapped here. Pure.

use crate::auto_move::prune;
use crate::automation::value_at;
use crate::model::{ArrangementMarker, AutoPoint, Id};

/// The names in the arrangement marker's name pop-up, as in Logic.
pub const NAMES: &[&str] = &["Intro", "Verse", "Pre-Chorus", "Chorus", "Bridge", "Solo", "Break", "Interlude", "Outro"];

/// What the Arrangement track's + button names each new section: a song's usual order, then
/// verse, chorus and bridge again.
const ADD_ORDER: &[&str] = &["Intro", "Verse", "Chorus", "Bridge", "Outro"];

/// A new section's length, in bars (Logic's default).
pub const DEFAULT_BARS: i64 = 8;

/// The name the next added section gets.
pub fn next_name(sections: &[ArrangementMarker]) -> String {
    let n = sections.len();
    let k = if n < ADD_ORDER.len() { n } else { 1 + (n - ADD_ORDER.len()) % 3 };
    ADD_ORDER[k].to_string()
}

/// Where the arrangement ends (None when it has no sections).
pub fn end(sections: &[ArrangementMarker]) -> Option<i64> {
    sections.iter().map(|s| s.end()).max()
}

/// Puts the sections back in order and end to end, from where the first one starts. Every
/// section is at least a sample long.
pub fn tile(sections: &mut [ArrangementMarker]) {
    sections.sort_by_key(|s| (s.start, s.id));
    let mut at = sections.first().map_or(0, |s| s.start.max(0));
    for s in sections.iter_mut() {
        s.start = at;
        s.length = s.length.max(1);
        at += s.length;
    }
}

/// The sections with a new one of `length` after the last (at 0 for the first).
pub fn add(sections: &[ArrangementMarker], id: Id, name: String, length: i64) -> Vec<ArrangementMarker> {
    let start = end(sections).unwrap_or(0);
    let mut out = sections.to_vec();
    out.push(ArrangementMarker { id, name, start, length: length.max(1), color: None });
    tile(&mut out);
    out
}

/// The sections with `id` resized to `length`; the ones after it move along (Logic ripples).
/// Only the markers move: the content stays where it is.
pub fn resize(sections: &[ArrangementMarker], id: Id, length: i64) -> Vec<ArrangementMarker> {
    let mut out = sections.to_vec();
    if let Some(s) = out.iter_mut().find(|s| s.id == id) {
        s.length = length.max(1);
    }
    tile(&mut out);
    out
}

/// Delete Arrangement Marker: the section goes and the one before it takes over its time (the
/// first one's time is simply no longer part of the arrangement). The content stays.
pub fn remove(sections: &[ArrangementMarker], id: Id) -> Vec<ArrangementMarker> {
    let mut out = sections.to_vec();
    out.sort_by_key(|s| s.start);
    if let Some(i) = out.iter().position(|s| s.id == id) {
        let gone = out.remove(i);
        if i > 0 {
            out[i - 1].length += gone.length;
        }
    }
    out
}

/// A stretch of the old timeline, `start..end`, and where it lands: at `dst`. `end` is
/// `i64::MAX` for the rest of the timeline. A copy is a second piece over the same stretch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    pub start: i64,
    pub end: i64,
    pub dst: i64,
    /// Lands a copy of the content: what lands there gets new ids.
    pub copy: bool,
}

impl Piece {
    pub fn delta(&self) -> i64 {
        self.dst - self.start
    }

    /// Whether `lo..hi` lies wholly inside the piece.
    pub fn contains(&self, lo: i64, hi: i64) -> bool {
        self.start <= lo && hi <= self.end
    }

    pub fn overlaps(&self, lo: i64, hi: i64) -> bool {
        self.start < hi && lo < self.end
    }

    /// Moves nothing: the content stays where it is.
    pub fn is_identity(&self) -> bool {
        !self.copy && self.dst == self.start
    }
}

/// A rearrangement: the sections afterwards, the time map for the content, and the span of
/// the section moved or copied (to select what is in it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub sections: Vec<ArrangementMarker>,
    pub pieces: Vec<Piece>,
    pub focus: Option<(i64, i64)>,
}

/// One slot of a new order: an old section by index, and a new id when it is a copy.
type Slot = (usize, Option<Id>);

/// Lays the sections out in `order` from where the arrangement starts, and the time map that
/// takes the content along. Time before the arrangement stays put; time after it moves by as
/// much as the arrangement grew or shrank.
fn layout(old: &[ArrangementMarker], order: &[Slot], focus: usize) -> Plan {
    let s0 = old[0].start;
    let e0 = old.last().unwrap().end();
    let mut at = s0;
    let mut sections = Vec::new();
    let mut pieces = Vec::new();
    let mut span = None;
    if s0 > 0 {
        pieces.push(Piece { start: 0, end: s0, dst: 0, copy: false });
    }
    for (k, &(i, new_id)) in order.iter().enumerate() {
        let s = &old[i];
        sections.push(ArrangementMarker { id: new_id.unwrap_or(s.id), start: at, ..s.clone() });
        pieces.push(Piece { start: s.start, end: s.end(), dst: at, copy: new_id.is_some() });
        if k == focus {
            span = Some((at, at + s.length));
        }
        at += s.length;
    }
    pieces.push(Piece { start: e0, end: i64::MAX, dst: at, copy: false });
    Plan { sections, pieces: merge(pieces), focus: span }
}

/// Sorts the pieces and joins neighbours that are next to each other both before and after,
/// so content is only cut where it really comes apart.
fn merge(mut pieces: Vec<Piece>) -> Vec<Piece> {
    pieces.retain(|p| p.end > p.start);
    pieces.sort_by_key(|p| (p.start, p.copy, p.dst));
    let mut out: Vec<Piece> = Vec::with_capacity(pieces.len());
    for p in pieces {
        if let Some(q) = out.last_mut() {
            if q.end == p.start && q.copy == p.copy && q.dst.checked_add(q.end - q.start) == Some(p.dst) {
                q.end = p.end;
                continue;
            }
        }
        out.push(p);
    }
    out
}

/// Where a section goes in the order: before section `before`, or at the end.
fn sorted_with_slot(sections: &[ArrangementMarker], before: Option<Id>) -> Option<(Vec<ArrangementMarker>, usize)> {
    let mut old = sections.to_vec();
    tile(&mut old);
    let at = match before {
        Some(b) => old.iter().position(|s| s.id == b)?,
        None => old.len(),
    };
    Some((old, at))
}

/// Moves section `id` (and everything in it) to just before section `before` (None: after the
/// last). The sections in between close up or make room. None if it doesn't move.
pub fn move_section(sections: &[ArrangementMarker], id: Id, before: Option<Id>) -> Option<Plan> {
    let (old, at) = sorted_with_slot(sections, before)?;
    let from = old.iter().position(|s| s.id == id)?;
    if at == from || at == from + 1 {
        return None;
    }
    let mut order: Vec<Slot> = (0..old.len()).filter(|&i| i != from).map(|i| (i, None)).collect();
    let to = if at > from { at - 1 } else { at };
    order.insert(to, (from, None));
    Some(layout(&old, &order, to))
}

/// Copies section `id` with its content to just before section `before` (None: after the
/// last), as `new_id`. Everything after the copy moves later by its length.
pub fn copy_section(sections: &[ArrangementMarker], id: Id, before: Option<Id>, new_id: Id) -> Option<Plan> {
    let (old, at) = sorted_with_slot(sections, before)?;
    let from = old.iter().position(|s| s.id == id)?;
    let mut order: Vec<Slot> = (0..old.len()).map(|i| (i, None)).collect();
    order.insert(at, (from, Some(new_id)));
    Some(layout(&old, &order, at))
}

/// Deletes section `id` and its content; everything after it moves earlier to close the gap.
pub fn delete_section(sections: &[ArrangementMarker], id: Id) -> Option<Plan> {
    let (old, _) = sorted_with_slot(sections, None)?;
    let gone = old.iter().position(|s| s.id == id)?;
    let order: Vec<Slot> = (0..old.len()).filter(|&i| i != gone).map(|i| (i, None)).collect();
    Some(layout(&old, &order, usize::MAX))
}

/// Where position `pos` lands: once per piece that holds it (the original first), or nowhere
/// if its time was deleted.
pub fn map_pos(pieces: &[Piece], pos: i64) -> Vec<i64> {
    let mut out: Vec<i64> = pieces.iter().filter(|p| !p.copy && p.start <= pos && pos < p.end).map(|p| pos + p.delta()).collect();
    out.extend(pieces.iter().filter(|p| p.copy && p.start <= pos && pos < p.end).map(|p| pos + p.delta()));
    out
}

/// An automation lane (sorted points) after the time map. Inside every piece the curve is
/// exactly what it was: a point holding the old curve's value is added at each end of a piece
/// (its first and last sample), and dropped again where the curve doesn't need it. A curved
/// segment cut at a piece's edge is approximated: the added points are straight.
pub fn remap_points(points: &[AutoPoint], pieces: &[Piece]) -> Vec<AutoPoint> {
    if points.is_empty() {
        return Vec::new();
    }
    let value = |x: i64| value_at(points, x).unwrap_or(0.0);
    let has = |x: i64| points.iter().any(|q| q.pos == x);
    let mut out: Vec<(AutoPoint, bool)> = Vec::new();
    for p in pieces {
        let d = p.delta();
        if !has(p.start) {
            out.push((AutoPoint { pos: p.dst, value: value(p.start), curve: 0.0 }, true));
        }
        out.extend(points.iter().filter(|q| q.pos >= p.start && q.pos < p.end).map(|q| (AutoPoint { pos: q.pos + d, value: q.value, curve: q.curve }, false)));
        let last = p.end.saturating_sub(1);
        if p.end != i64::MAX && last > p.start && !has(last) {
            out.push((AutoPoint { pos: last + d, value: value(last), curve: 0.0 }, true));
        }
    }
    // Pieces land on separate stretches, so this only orders them (steps keep their order).
    out.sort_by_key(|(q, _)| q.pos);
    prune(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(id: Id, start: i64, length: i64) -> ArrangementMarker {
        ArrangementMarker { id, name: format!("S{id}"), start, length, color: None }
    }

    /// Three sections: 1 at 100..200, 2 at 200..250, 3 at 250..400.
    fn three() -> Vec<ArrangementMarker> {
        vec![sec(1, 100, 100), sec(2, 200, 50), sec(3, 250, 150)]
    }

    fn spans(v: &[ArrangementMarker]) -> Vec<(Id, i64, i64)> {
        v.iter().map(|s| (s.id, s.start, s.end())).collect()
    }

    fn pt(pos: i64, value: f32) -> AutoPoint {
        AutoPoint { pos, value, curve: 0.0 }
    }

    fn at(points: &[AutoPoint], x: i64) -> f32 {
        value_at(points, x).unwrap()
    }

    #[test]
    fn sections_tile_without_gaps() {
        let mut v = vec![sec(2, 500, 50), sec(1, 100, 100), sec(3, 900, 0)];
        tile(&mut v);
        assert_eq!(spans(&v), vec![(1, 100, 200), (2, 200, 250), (3, 250, 251)]);
        let v = add(&v, 4, "Outro".into(), 80);
        assert_eq!(spans(&v).last(), Some(&(4, 251, 331)));
        assert_eq!(spans(&add(&[], 1, "Intro".into(), 80)), vec![(1, 0, 80)]);
    }

    #[test]
    fn names_follow_a_song() {
        let mut v = Vec::new();
        let mut names = Vec::new();
        for id in 1..=8 {
            let n = next_name(&v);
            v = add(&v, id, n.clone(), 10);
            names.push(n);
        }
        assert_eq!(names, ["Intro", "Verse", "Chorus", "Bridge", "Outro", "Verse", "Chorus", "Bridge"]);
        assert!(names.iter().all(|n| NAMES.contains(&n.as_str())));
    }

    #[test]
    fn resizing_ripples_the_later_sections() {
        let v = resize(&three(), 1, 40);
        assert_eq!(spans(&v), vec![(1, 100, 140), (2, 140, 190), (3, 190, 340)]);
        let v = resize(&three(), 3, 0);
        assert_eq!(spans(&v)[2], (3, 250, 251), "never shorter than a sample");
    }

    #[test]
    fn removing_a_marker_gives_its_time_to_the_one_before() {
        assert_eq!(spans(&remove(&three(), 2)), vec![(1, 100, 250), (3, 250, 400)]);
        assert_eq!(spans(&remove(&three(), 1)), vec![(2, 200, 250), (3, 250, 400)]);
    }

    #[test]
    fn moving_a_section_swaps_its_time_with_the_ones_it_passes() {
        // Section 3 before section 2.
        let plan = move_section(&three(), 3, Some(2)).unwrap();
        assert_eq!(spans(&plan.sections), vec![(1, 100, 200), (3, 200, 350), (2, 350, 400)]);
        assert_eq!(plan.focus, Some((200, 350)));
        assert_eq!(
            plan.pieces,
            vec![
                // Before the arrangement and section 1 stay; 2 and 3 trade places; the rest stays.
                Piece { start: 0, end: 200, dst: 0, copy: false },
                Piece { start: 200, end: 250, dst: 350, copy: false },
                Piece { start: 250, end: 400, dst: 200, copy: false },
                Piece { start: 400, end: i64::MAX, dst: 400, copy: false },
            ]
        );
        assert_eq!(map_pos(&plan.pieces, 260), vec![210]);
        assert_eq!(map_pos(&plan.pieces, 210), vec![360]);
        assert_eq!(map_pos(&plan.pieces, 50), vec![50]);
        // Section 1 to the end: the others move up by its length.
        let plan = move_section(&three(), 1, None).unwrap();
        assert_eq!(spans(&plan.sections), vec![(2, 100, 150), (3, 150, 300), (1, 300, 400)]);
        assert_eq!(plan.pieces[1], Piece { start: 100, end: 200, dst: 300, copy: false });
        assert_eq!(plan.pieces[2], Piece { start: 200, end: 400, dst: 100, copy: false });
        // Onto its own place: nothing to do.
        assert!(move_section(&three(), 2, Some(2)).is_none());
        assert!(move_section(&three(), 2, Some(3)).is_none());
        assert!(move_section(&three(), 3, None).is_none());
        assert!(move_section(&three(), 9, None).is_none());
    }

    #[test]
    fn copying_a_section_makes_room_for_it() {
        // A copy of section 1 between 2 and 3.
        let plan = copy_section(&three(), 1, Some(3), 7).unwrap();
        assert_eq!(spans(&plan.sections), vec![(1, 100, 200), (2, 200, 250), (7, 250, 350), (3, 350, 500)]);
        assert_eq!(plan.focus, Some((250, 350)));
        assert_eq!(map_pos(&plan.pieces, 150), vec![150, 300], "the original first, then the copy");
        assert_eq!(map_pos(&plan.pieces, 300), vec![400]);
        assert_eq!(map_pos(&plan.pieces, 1000), vec![1100], "time after the arrangement moves along");
        assert!(plan.pieces.iter().any(|p| p.copy && p.start == 100 && p.end == 200));
    }

    #[test]
    fn deleting_a_section_closes_the_gap() {
        let plan = delete_section(&three(), 2).unwrap();
        assert_eq!(spans(&plan.sections), vec![(1, 100, 200), (3, 200, 350)]);
        assert!(map_pos(&plan.pieces, 220).is_empty(), "its content goes");
        assert_eq!(map_pos(&plan.pieces, 300), vec![250]);
        assert_eq!(map_pos(&plan.pieces, 450), vec![400]);
        assert_eq!(map_pos(&plan.pieces, 150), vec![150]);
        // The only section.
        let plan = delete_section(&[sec(1, 100, 100)], 1).unwrap();
        assert!(plan.sections.is_empty());
        assert_eq!((map_pos(&plan.pieces, 50), map_pos(&plan.pieces, 150), map_pos(&plan.pieces, 250)), (vec![50], vec![], vec![150]));
    }

    #[test]
    fn automation_moves_with_its_section_and_keeps_its_shape() {
        // A ramp from 0 to 1 over section 3 (250..400) on a lane at 0.5 elsewhere, and a step
        // in section 2.
        let lane = vec![pt(0, 0.5), pt(220, 0.5), pt(220, 0.8), pt(250, 0.0), pt(399, 1.0), pt(400, 0.5)];
        let plan = move_section(&three(), 3, Some(2)).unwrap();
        let moved = remap_points(&lane, &plan.pieces);
        assert!(moved.windows(2).all(|w| w[0].pos <= w[1].pos), "{moved:?}");
        // Every position inside each piece plays what it did before the move.
        for p in &plan.pieces {
            let hi = p.end.min(500);
            for x in (p.start..hi).step_by(7).chain([p.start, hi - 1]) {
                assert!((at(&moved, x + p.delta()) - at(&lane, x)).abs() < 1e-5, "{x} -> {}: {} vs {}", x + p.delta(), at(&moved, x + p.delta()), at(&lane, x));
            }
        }
        // No more points than the curve needs: no flat stretches get extra points.
        let flat = remap_points(&[pt(0, -6.0)], &plan.pieces);
        assert_eq!(flat, vec![pt(0, -6.0)]);
    }

    #[test]
    fn automation_is_copied_and_deleted_with_its_section() {
        let lane = vec![pt(100, 0.0), pt(199, 1.0), pt(200, 0.2), pt(400, 0.2)];
        let plan = copy_section(&three(), 1, Some(3), 7).unwrap();
        let copied = remap_points(&lane, &plan.pieces);
        for x in [100, 150, 199] {
            assert!((at(&copied, x + 150) - at(&lane, x)).abs() < 1e-5, "copy at {x}");
            assert!((at(&copied, x) - at(&lane, x)).abs() < 1e-5, "original at {x}");
        }
        assert!((at(&copied, 400) - 0.2).abs() < 1e-5);
        let plan = delete_section(&three(), 1).unwrap();
        let gone = remap_points(&lane, &plan.pieces);
        assert!((at(&gone, 50) - 0.0).abs() < 1e-5 && (at(&gone, 99) - 0.0).abs() < 1e-5, "{gone:?}");
        assert!((at(&gone, 100) - 0.2).abs() < 1e-5 && (at(&gone, 300) - 0.2).abs() < 1e-5, "{gone:?}");
    }
}
