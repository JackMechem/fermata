//! The arithmetic of Logic's arranging commands (Edit › Section Between Locators, Trim, Remove
//! Gaps and so on). Section edits are time maps, as the Arrangement track's are
//! ([`crate::arrangement::Piece`]): `session/arrange_cmds.rs` applies them to regions,
//! automation, markers and arrangement sections. The tempo and signature tracks stay as they
//! are, as they do when the Arrangement track moves a section. Pure.

use serde::Deserialize;

use crate::arrangement::Piece;
pub use crate::drag_modes::Span;
use crate::model::{ArrangementMarker, Id};

/// Edit › Section Between Locators (Logic's names in brackets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionEdit {
    /// Insert Silence Between Locators: everything from the left locator on moves right by
    /// the locators' length; regions across the left locator are cut there.
    InsertSilence,
    /// Cut Section Between Locators (Snip): the time between the locators goes, and
    /// everything after it moves left to close the gap.
    Cut,
    /// Repeat Section Between Locators: a copy of what is between the locators goes straight
    /// after them, and everything after the right locator moves right to make room.
    Repeat,
}

impl SectionEdit {
    /// The undo step's name.
    pub fn label(self) -> &'static str {
        match self {
            SectionEdit::InsertSilence => "Insert Silence Between Locators",
            SectionEdit::Cut => "Cut Section Between Locators",
            SectionEdit::Repeat => "Repeat Section Between Locators",
        }
    }
}

/// The time map for a section edit between `a` and `b` (a < b).
pub fn section_pieces(edit: SectionEdit, a: i64, b: i64) -> Vec<Piece> {
    let keep = |start, end, dst| Piece { start, end, dst, copy: false };
    let len = b - a;
    let mut out = match edit {
        SectionEdit::InsertSilence => vec![keep(0, a, 0), keep(a, i64::MAX, b)],
        SectionEdit::Cut => vec![keep(0, a, 0), keep(b, i64::MAX, a)],
        SectionEdit::Repeat => vec![keep(0, b, 0), Piece { start: a, end: b, dst: b, copy: true }, keep(b, i64::MAX, b + len)],
    };
    out.retain(|p| p.end > p.start);
    out
}

/// Where a boundary at `pos` (a section's start or end) lands: with its time, or where the
/// time after it now starts when its own was cut. Only the original pieces count, so the
/// result never goes backwards as `pos` grows.
pub fn map_edge(pieces: &[Piece], pos: i64) -> i64 {
    let orig = || pieces.iter().filter(|p| !p.copy);
    if let Some(p) = orig().find(|p| p.start <= pos && pos < p.end) {
        return pos + p.delta();
    }
    orig().filter(|p| p.start > pos).min_by_key(|p| p.start).map(|p| p.dst).unwrap_or(pos)
}

/// The Arrangement track's sections after a section edit: each edge moves with its time, so
/// a section the edit falls in grows or shrinks, and one wholly cut goes.
pub fn remap_sections(sections: &[ArrangementMarker], pieces: &[Piece]) -> Vec<ArrangementMarker> {
    sections
        .iter()
        .filter_map(|s| {
            let (start, end) = (map_edge(pieces, s.start), map_edge(pieces, s.end()));
            (end > start).then(|| ArrangementMarker { start, length: end - start, ..s.clone() })
        })
        .collect()
}

/// Remove Gaps: the regions of one track butted together in time order, the first staying
/// where it is. Returns the new start of each region that moves.
pub fn butt_together(spans: &[Span]) -> Vec<(Id, i64)> {
    let mut v = spans.to_vec();
    v.sort_by_key(|s| (s.start, s.id));
    let mut out = Vec::new();
    let Some(first) = v.first() else { return out };
    let mut at = first.end();
    for s in &v[1..] {
        if s.start != at {
            out.push((s.id, at));
        }
        at += s.len;
    }
    out
}

/// Remove Overlaps: of the regions of one track, each one still sounding when the next one
/// starts is shortened to end there. Returns the new sounding length of each one cut. A region
/// starting together with the next one is left alone (there would be nothing left of it).
pub fn overlap_trims(spans: &[Span]) -> Vec<(Id, i64)> {
    let mut v = spans.to_vec();
    v.sort_by_key(|s| (s.start, s.id));
    v.windows(2).filter(|w| w[0].end() > w[1].start && w[1].start > w[0].start).map(|w| (w[0].id, w[1].start - w[0].start)).collect()
}

/// Where Trim Region End to Next Region and Fill Within Locators make a region end: at the
/// start of the next region on its track (`others`: the other regions' starts), or at `limit`
/// when that comes first. None when nothing follows and there is no limit.
pub fn end_at_next(start: i64, others: &[i64], limit: Option<i64>) -> Option<i64> {
    let next = others.iter().copied().filter(|&o| o > start).min();
    match (next, limit) {
        (Some(n), Some(l)) => Some(n.min(l)),
        (n, l) => n.or(l),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrangement::{map_pos, remap_points};
    use crate::automation::value_at;
    use crate::model::AutoPoint;

    fn sec(id: Id, start: i64, length: i64) -> ArrangementMarker {
        ArrangementMarker { id, name: format!("S{id}"), start, length, color: None }
    }

    fn spans(v: &[ArrangementMarker]) -> Vec<(Id, i64, i64)> {
        v.iter().map(|s| (s.id, s.start, s.end())).collect()
    }

    fn sp(id: Id, start: i64, len: i64) -> Span {
        Span { id, start, len }
    }

    #[test]
    fn insert_silence_moves_everything_from_the_left_locator() {
        let p = section_pieces(SectionEdit::InsertSilence, 100, 150);
        assert_eq!(map_pos(&p, 99), vec![99]);
        assert_eq!(map_pos(&p, 100), vec![150]);
        assert_eq!(map_pos(&p, 1000), vec![1050]);
        // At the project start there is nothing before it.
        assert_eq!(section_pieces(SectionEdit::InsertSilence, 0, 10), vec![Piece { start: 0, end: i64::MAX, dst: 10, copy: false }]);
    }

    #[test]
    fn cut_section_closes_the_gap() {
        let p = section_pieces(SectionEdit::Cut, 100, 150);
        assert_eq!(map_pos(&p, 99), vec![99]);
        assert!(map_pos(&p, 120).is_empty());
        assert_eq!(map_pos(&p, 150), vec![100]);
        assert_eq!(map_edge(&p, 120), 100, "a cut edge lands where the time after it starts");
    }

    #[test]
    fn repeat_section_copies_it_after_the_right_locator() {
        let p = section_pieces(SectionEdit::Repeat, 100, 150);
        assert_eq!(map_pos(&p, 120), vec![120, 170], "the original first, then the copy");
        assert_eq!(map_pos(&p, 150), vec![200]);
        assert_eq!(map_pos(&p, 50), vec![50]);
        assert_eq!(map_edge(&p, 120), 120);
    }

    #[test]
    fn sections_grow_shrink_or_go_with_their_time() {
        let v = vec![sec(1, 0, 100), sec(2, 100, 100), sec(3, 200, 100)];
        // Silence inside section 2: it grows, section 3 moves.
        let got = remap_sections(&v, &section_pieces(SectionEdit::InsertSilence, 150, 170));
        assert_eq!(spans(&got), vec![(1, 0, 100), (2, 100, 220), (3, 220, 320)]);
        // Cutting all of section 2 and part of 3.
        let got = remap_sections(&v, &section_pieces(SectionEdit::Cut, 100, 250));
        assert_eq!(spans(&got), vec![(1, 0, 100), (3, 100, 150)]);
        // Repeating part of section 1: it takes in the copy.
        let got = remap_sections(&v, &section_pieces(SectionEdit::Repeat, 50, 100));
        assert_eq!(spans(&got), vec![(1, 0, 150), (2, 150, 250), (3, 250, 350)]);
    }

    #[test]
    fn automation_follows_a_cut_and_a_repeat() {
        let lane = vec![AutoPoint { pos: 0, value: 0.0, curve: 0.0 }, AutoPoint { pos: 200, value: 1.0, curve: 0.0 }];
        let at = |v: &[AutoPoint], x| value_at(v, x).unwrap();
        let cut = remap_points(&lane, &section_pieces(SectionEdit::Cut, 50, 150));
        assert!((at(&cut, 40) - 0.2).abs() < 1e-4 && (at(&cut, 60) - 0.8).abs() < 1e-4, "{cut:?}");
        let rep = remap_points(&lane, &section_pieces(SectionEdit::Repeat, 50, 150));
        for x in [60, 100, 140] {
            assert!((at(&rep, x) - at(&lane, x)).abs() < 1e-4);
            assert!((at(&rep, x + 100) - at(&lane, x)).abs() < 1e-4, "the copy at {}", x + 100);
        }
        assert!((at(&rep, 290) - at(&lane, 190)).abs() < 1e-4);
    }

    #[test]
    fn remove_gaps_butts_the_regions_together() {
        assert_eq!(butt_together(&[sp(2, 500, 100), sp(1, 100, 200), sp(3, 900, 50)]), vec![(2, 300), (3, 400)]);
        assert!(butt_together(&[sp(1, 0, 10), sp(2, 10, 10)]).is_empty());
        // Overlapping regions are pulled apart into a row as well.
        assert_eq!(butt_together(&[sp(1, 0, 100), sp(2, 50, 100)]), vec![(2, 100)]);
    }

    #[test]
    fn remove_overlaps_trims_the_earlier_region() {
        assert_eq!(overlap_trims(&[sp(1, 0, 100), sp(2, 60, 100), sp(3, 300, 10)]), vec![(1, 60)]);
        assert!(overlap_trims(&[sp(1, 0, 100), sp(2, 0, 50)]).is_empty());
        assert!(overlap_trims(&[sp(1, 0, 100), sp(2, 100, 50)]).is_empty());
        // A region covering several: it ends where the next one starts.
        assert_eq!(overlap_trims(&[sp(1, 0, 1000), sp(2, 200, 100), sp(3, 250, 100)]), vec![(1, 200), (2, 50)]);
    }

    #[test]
    fn ends_at_the_next_region_or_the_limit() {
        assert_eq!(end_at_next(100, &[50, 400, 300], None), Some(300));
        assert_eq!(end_at_next(100, &[50], None), None);
        assert_eq!(end_at_next(100, &[50], Some(250)), Some(250));
        assert_eq!(end_at_next(100, &[200], Some(250)), Some(200));
    }
}
