//! Logic's No Overlap, Shuffle L and Shuffle R drag modes, as pure functions on one track's
//! lane. The session applies them after a move, trim, delete or paste in the Tracks area
//! (`session/drag_modes.rs`); `src/dragModes.ts` mirrors them to draw a drag's outcome.
//!
//! The rules (Logic Pro's "Drag modes" with the cases its manual leaves open settled):
//!
//! - **No Overlap.** Of two regions that overlap after an edit, the one that starts earlier is
//!   shortened to end where the later one starts, whichever of the two was edited. So a region
//!   dropped into the middle of a longer one cuts the longer one off there (its tail is not
//!   kept), and a region dropped over the start of a shorter one is itself cut off at that
//!   start. Of two that start together, the one on top (the edited one) stays, and the other
//!   is overwritten: it goes. Overlaps that were there before and that the edit doesn't touch
//!   are left alone.
//! - **Shuffle L.** Regions stay butted together.
//!   - Deleting a region moves every region after it left by its length.
//!   - Trimming either edge keeps the region's start where it is and moves the regions after
//!     it by as much as the region grew or shrank.
//!   - A region dropped onto another goes before it if it lands on that region's first half,
//!     and after it if on its second half: dragging a region across its neighbour swaps them.
//!     Dropped in a gap, it snaps to the end of the region before it; with nothing before it,
//!     it stays where it was dropped. The regions after it move right only as far as they
//!     must to make room, and the place it came from closes as for a delete. Several regions
//!     dropped together keep their spacing and move as one block.
//!   - Pasting and Alt-drag copies are placed in the same way, without anything leaving.
//! - **Shuffle R** is the mirror image: deleting a region moves the regions before it right,
//!   a trim keeps the region's end, and dropped regions snap to the start of the region after
//!   them. Where that would push regions before the project start, the whole lane moves right
//!   instead, which keeps it butted.
//!
//! A region's length here is how long it sounds, its loop included.

use crate::model::Id;

/// A region on a lane as the drag modes see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub id: Id,
    /// Timeline position in samples.
    pub start: i64,
    /// How long it sounds, loop included.
    pub len: i64,
}

impl Span {
    pub fn end(&self) -> i64 {
        self.start + self.len
    }
}

/// Which way the Shuffle modes close gaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Shuffle L: regions move left, edited regions keep their start.
    Left,
    /// Shuffle R: regions move right, edited regions keep their end.
    Right,
}

/// No Overlap: the regions an edit of `edited` leaves overlapping, each with the length it is
/// shortened to (0: it is covered from its start and goes). `lane` is in the track's order,
/// where later regions are on top. Only pairs with an edited region in them count.
pub fn no_overlap(lane: &[Span], edited: &[Id]) -> Vec<(Id, i64)> {
    let mut out = Vec::new();
    for (i, a) in lane.iter().enumerate() {
        let mut end = a.end();
        for (j, b) in lane.iter().enumerate() {
            if i == j || !(edited.contains(&a.id) || edited.contains(&b.id)) {
                continue;
            }
            let later = b.start > a.start || (b.start == a.start && j > i);
            if later && b.start < end {
                end = b.start;
            }
        }
        if end < a.end() {
            out.push((a.id, end - a.start));
        }
    }
    out
}

/// Shuffle: where every region of a lane starts after `leaving` are taken off it and
/// `arriving` are dropped onto it at their own starts. `lane` is the lane before the edit;
/// a region moved along its own lane is both leaving and arriving. Returns each region the
/// lane ends up with and its start.
pub fn shuffle(lane: &[Span], leaving: &[Id], arriving: &[Span], side: Side) -> Vec<(Id, i64)> {
    match side {
        Side::Left => starts(&shuffle_left(lane, leaving, arriving)),
        Side::Right => unmirror(&shuffle_left(&mirror(lane), leaving, &mirror(arriving))),
    }
}

/// Shuffle after region `id` of `lane` changes length to `len` by a trim of either edge:
/// Shuffle L keeps its start and moves the regions after it, Shuffle R keeps its end and
/// moves the regions before it. Returns every region's start.
pub fn shuffle_resize(lane: &[Span], id: Id, len: i64, side: Side) -> Vec<(Id, i64)> {
    match side {
        Side::Left => starts(&resize_left(lane, id, len)),
        Side::Right => unmirror(&resize_left(&mirror(lane), id, len)),
    }
}

fn starts(lane: &[Span]) -> Vec<(Id, i64)> {
    lane.iter().map(|s| (s.id, s.start)).collect()
}

/// Shuffle R is Shuffle L with time running backwards: a span becomes `-end..-start`.
fn mirror(lane: &[Span]) -> Vec<Span> {
    lane.iter().map(|s| Span { start: -s.end(), ..*s }).collect()
}

/// Back from mirrored time. Nothing may start before 0, so a lane pushed past it moves right
/// as a whole.
fn unmirror(lane: &[Span]) -> Vec<(Id, i64)> {
    let real: Vec<(Id, i64)> = lane.iter().map(|s| (s.id, -s.end())).collect();
    let short = -real.iter().map(|&(_, s)| s).min().unwrap_or(0).min(0);
    real.into_iter().map(|(id, s)| (id, s + short)).collect()
}

/// The regions staying on the lane, by start, after the regions in `gone` (positions before
/// the edit) are taken out: each pulls the regions starting after it left by its length, but
/// no further than its own start.
fn close(stay: &[Span], gone: &[Span]) -> Vec<Span> {
    let mut out = stay.to_vec();
    let mut gone = gone.to_vec();
    // From the right, so each removal sees the positions it had.
    gone.sort_by_key(|g| std::cmp::Reverse(g.start));
    for g in gone {
        for r in out.iter_mut().filter(|r| r.start > g.start) {
            r.start = (r.start - g.len).max(g.start);
        }
    }
    out
}

/// Where a dropped block goes, relative to the regions staying on the lane (indices into
/// them, ordered by start).
enum Slot {
    Before(usize),
    After(usize),
    /// Nothing before the drop position: it stays there.
    At,
}

/// The slot for a block dropped at `p`, judged against the lane as the user saw it.
fn slot(stay: &[Span], p: i64) -> Slot {
    if let Some(i) = stay.iter().rposition(|s| s.start <= p && p < s.end()) {
        let s = stay[i];
        return if 2 * (p - s.start) < s.len { Slot::Before(i) } else { Slot::After(i) };
    }
    match stay.iter().rposition(|s| s.start < p) {
        Some(i) => Slot::After(i),
        None => Slot::At,
    }
}

fn shuffle_left(lane: &[Span], leaving: &[Id], arriving: &[Span]) -> Vec<Span> {
    let moving = |id: Id| leaving.contains(&id) || arriving.iter().any(|a| a.id == id);
    let mut stay: Vec<Span> = lane.iter().filter(|s| !moving(s.id)).copied().collect();
    stay.sort_by_key(|s| s.start);
    let gone: Vec<Span> = lane.iter().filter(|s| leaving.contains(&s.id)).copied().collect();
    // Closing keeps the order (each region's new start rises with its old one), so indices
    // into `stay` still apply.
    let mut out = close(&stay, &gone);
    let (Some(p), Some(end)) = (arriving.iter().map(|a| a.start).min(), arriving.iter().map(|a| a.end()).max()) else { return out };
    let len = end - p;
    let (at, first) = match slot(&stay, p) {
        Slot::Before(i) => (out[i].start, i),
        Slot::After(i) => (out[..=i].iter().map(|s| s.end()).max().unwrap_or(0), i + 1),
        Slot::At => (out.first().map_or(p, |s| s.start.min(p)), 0),
    };
    if let Some(next) = out.get(first) {
        let push = (at + len - next.start).max(0);
        for r in &mut out[first..] {
            r.start += push;
        }
    }
    out.extend(arriving.iter().map(|a| Span { start: at + (a.start - p), ..*a }));
    out
}

fn resize_left(lane: &[Span], id: Id, len: i64) -> Vec<Span> {
    let Some(me) = lane.iter().find(|s| s.id == id).copied() else { return lane.to_vec() };
    let d = len - me.len;
    lane.iter()
        .map(|s| {
            let start = if s.id == id {
                me.start
            } else if s.start > me.start {
                // Shrinking pulls the regions after it in, but never past its start.
                (s.start + d).max(me.start)
            } else {
                s.start
            };
            Span { start, len: if s.id == id { len } else { s.len }, ..*s }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: Id, start: i64, len: i64) -> Span {
        Span { id, start, len }
    }

    /// Sorted (id, start) for comparing.
    fn sorted(mut v: Vec<(Id, i64)>) -> Vec<(Id, i64)> {
        v.sort();
        v
    }

    #[test]
    fn no_overlap_shortens_the_earlier_region() {
        // 2 moved over the end of 1: 1 is cut where 2 starts.
        let lane = [s(1, 0, 100), s(2, 60, 100)];
        assert_eq!(no_overlap(&lane, &[2]), vec![(1, 60)]);
        // 1 moved over the start of 2 (1 is still the earlier one): 1 is cut.
        assert_eq!(no_overlap(&lane, &[1]), vec![(1, 60)]);
        // A short region dropped inside a long one cuts the long one off; its tail goes.
        let lane = [s(1, 0, 1000), s(2, 300, 100)];
        assert_eq!(no_overlap(&lane, &[2]), vec![(1, 300)]);
        // A long region dropped over a short one is cut at the short one's start.
        let lane = [s(1, 300, 100), s(2, 0, 1000)];
        assert_eq!(no_overlap(&lane, &[2]), vec![(2, 300)]);
        // Same start: the one on top (later in the list) stays; the other goes.
        let lane = [s(1, 0, 100), s(2, 0, 50)];
        assert_eq!(no_overlap(&lane, &[2]), vec![(1, 0)]);
        // Touching isn't overlapping.
        assert!(no_overlap(&[s(1, 0, 100), s(2, 100, 50)], &[2]).is_empty());
        // An old overlap the edit doesn't touch stays.
        let lane = [s(1, 0, 100), s(2, 50, 100), s(3, 500, 10)];
        assert!(no_overlap(&lane, &[3]).is_empty());
        // The nearest later start wins.
        let lane = [s(1, 0, 1000), s(2, 600, 10), s(3, 200, 10)];
        assert_eq!(no_overlap(&lane, &[1]), vec![(1, 200)]);
    }

    #[test]
    fn shuffle_l_delete_pulls_the_following_regions_left() {
        let lane = [s(1, 0, 10), s(2, 10, 20), s(3, 30, 10), s(4, 50, 10)];
        assert_eq!(sorted(shuffle(&lane, &[2], &[], Side::Left)), vec![(1, 0), (3, 10), (4, 30)]);
        // Two at once add up; a gap after them stays.
        assert_eq!(sorted(shuffle(&lane, &[1, 2], &[], Side::Left)), vec![(3, 0), (4, 20)]);
        // Shuffle R: the regions before move right instead.
        assert_eq!(sorted(shuffle(&lane, &[3], &[], Side::Right)), vec![(1, 10), (2, 20), (4, 50)]);
    }

    #[test]
    fn shuffle_l_drag_swaps_neighbours() {
        let lane = [s(1, 0, 10), s(2, 10, 20), s(3, 30, 10)];
        // 1 dropped on the second half of 2: they swap.
        let moved = shuffle(&lane, &[1], &[s(1, 22, 10)], Side::Left);
        assert_eq!(sorted(moved), vec![(1, 20), (2, 0), (3, 30)]);
        // On its first half: nothing changes.
        assert_eq!(sorted(shuffle(&lane, &[1], &[s(1, 14, 10)], Side::Left)), vec![(1, 0), (2, 10), (3, 30)]);
        // 3 dropped on the first half of 1 goes first.
        assert_eq!(sorted(shuffle(&lane, &[3], &[s(3, 2, 10)], Side::Left)), vec![(1, 10), (2, 20), (3, 0)]);
        // Dropped past the end in a gap: it snaps to the end of the last region, and the
        // place it left closes.
        assert_eq!(sorted(shuffle(&lane, &[1], &[s(1, 90, 10)], Side::Left)), vec![(1, 30), (2, 0), (3, 20)]);
    }

    #[test]
    fn shuffle_l_insert_makes_room_only_as_needed() {
        let lane = [s(1, 0, 10), s(2, 50, 10)];
        // Into the gap after 1: snaps to 1's end; 2 has room and stays.
        assert_eq!(sorted(shuffle(&lane, &[], &[s(9, 30, 10)], Side::Left)), vec![(1, 0), (2, 50), (9, 10)]);
        // A longer one pushes 2 along by what it needs.
        assert_eq!(sorted(shuffle(&lane, &[], &[s(9, 30, 45)], Side::Left)), vec![(1, 0), (2, 55), (9, 10)]);
        // Before everything, with nothing before it: stays where it was dropped.
        let lane = [s(1, 100, 10)];
        assert_eq!(sorted(shuffle(&lane, &[], &[s(9, 20, 10)], Side::Left)), vec![(1, 100), (9, 20)]);
        // A block keeps its spacing (6..11 and 15..20, dropped on 1's second half).
        let lane = [s(1, 0, 10), s(2, 10, 10)];
        let block = [s(8, 6, 5), s(9, 15, 5)];
        assert_eq!(sorted(shuffle(&lane, &[], &block, Side::Left)), vec![(1, 0), (2, 24), (8, 10), (9, 19)]);
    }

    #[test]
    fn shuffle_moves_between_lanes() {
        // Leaving one lane closes it; arriving on another opens it.
        let a = [s(1, 0, 10), s(2, 10, 10), s(3, 20, 10)];
        assert_eq!(sorted(shuffle(&a, &[2], &[], Side::Left)), vec![(1, 0), (3, 10)]);
        let b = [s(4, 0, 10), s(5, 10, 10)];
        assert_eq!(sorted(shuffle(&b, &[], &[s(2, 1, 10)], Side::Left)), vec![(2, 0), (4, 10), (5, 20)]);
    }

    #[test]
    fn shuffle_r_is_the_mirror() {
        let lane = [s(1, 100, 10), s(2, 110, 20), s(3, 130, 10)];
        // 3's end dropped onto 2's second half: it stays after 2.
        assert_eq!(sorted(shuffle(&lane, &[3], &[s(3, 112, 10)], Side::Right)), vec![(1, 100), (2, 110), (3, 130)]);
        // Onto 2's first half: they swap, and the lane still ends at 140.
        assert_eq!(sorted(shuffle(&lane, &[3], &[s(3, 101, 10)], Side::Right)), vec![(1, 100), (2, 120), (3, 110)]);
        // Dropped in a gap before everything, it snaps to the start of the region after it.
        let lane = [s(1, 100, 10)];
        assert_eq!(sorted(shuffle(&lane, &[], &[s(9, 20, 10)], Side::Right)), vec![(1, 100), (9, 90)]);
        // Dropped over the end of the last region, it pushes the lane left; past the project
        // start, the lane moves right as a whole instead.
        let lane = [s(1, 0, 10), s(2, 10, 10)];
        assert_eq!(sorted(shuffle(&lane, &[], &[s(9, 15, 10)], Side::Right)), vec![(1, 0), (2, 10), (9, 20)]);
    }

    #[test]
    fn shuffle_resize_keeps_the_anchored_edge() {
        let lane = [s(1, 0, 10), s(2, 10, 20), s(3, 30, 10), s(4, 60, 10)];
        // Shuffle L: 2 grows by 5 (either edge): it keeps its start, 3 and 4 move right.
        assert_eq!(sorted(shuffle_resize(&lane, 2, 25, Side::Left)), vec![(1, 0), (2, 10), (3, 35), (4, 65)]);
        // Shrinks by 15: they move left.
        assert_eq!(sorted(shuffle_resize(&lane, 2, 5, Side::Left)), vec![(1, 0), (2, 10), (3, 15), (4, 45)]);
        // Shuffle R: 3 shrinks by 5: it keeps its end (40), 1 and 2 move right.
        assert_eq!(sorted(shuffle_resize(&lane, 3, 5, Side::Right)), vec![(1, 5), (2, 15), (3, 35), (4, 60)]);
        // Shuffle R growing past the start moves the lane right.
        assert_eq!(sorted(shuffle_resize(&lane, 1, 15, Side::Right)), vec![(1, 0), (2, 15), (3, 35), (4, 65)]);
    }
}
