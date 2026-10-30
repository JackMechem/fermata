//! Moving regions in bars and beats: a move that takes one region from `from` to `to` moves
//! every other region by the same musical distance, counted in quarter notes through the tempo
//! map, rather than by the same number of samples. Across a tempo change a region then lands on
//! the bar or beat it was meant to (the Nudge keys by bar, beat or note value; the Tracks
//! area's drags do the same in `src/musicalMove.ts`).

use super::region_moves::RegionMove;
use super::*;
use crate::tempo::TempoMap;

/// Where a region at `pos` goes when the move takes `from` to `to`, keeping its distance in
/// quarter notes from `from` (not before 0).
pub fn musical_shift(map: &TempoMap, pos: i64, from: i64, to: i64) -> i64 {
    if pos == from {
        return to.max(0);
    }
    let dq = map.beats_at(to as f64) - map.beats_at(from as f64);
    (map.sample_at(map.beats_at(pos as f64) + dq).round() as i64).max(0)
}

impl Session {
    /// The moves that shift regions `ids` by the musical distance from `from` to `to`.
    pub fn musical_moves(&self, ids: &[Id], from: i64, to: i64) -> Vec<RegionMove> {
        let map = self.tempo_map();
        ids.iter().filter_map(|&id| self.region(id)).map(|r| RegionMove { id: r.id, start: musical_shift(&map, r.start, from, to), track: None }).collect()
    }

    /// Nudges regions by a bar, beat or note value through the tempo map, in the drag mode:
    /// `from` (the first region's start) goes to `to`, the rest keep their bars and beats.
    pub fn drag_nudge_musical(&mut self, ids: &[Id], from: i64, to: i64, automation: Option<bool>) {
        let moves = self.musical_moves(ids, from, to);
        self.drag_move_regions(&moves, automation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::TempoEvent;

    const SR: f64 = 48_000.0;

    /// 120 BPM for four bars of 4/4, then 90 BPM from bar 5.
    fn map() -> TempoMap {
        let bar5 = (16.0 * SR * 60.0 / 120.0) as i64;
        TempoMap::new(SR, &[TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: bar5, bpm: 90.0, ramp: false }], &[], 120.0, (4, 4))
    }

    #[test]
    fn regions_keep_their_bars_and_beats_across_a_tempo_change() {
        let m = map();
        let bar = |b: i64| m.bar_start(b).round() as i64;
        // The first region goes from bar 1 to bar 3; one at bar 3 goes to bar 5, past the
        // change, and one at bar 4 beat 3 to bar 6 beat 3.
        assert_eq!(musical_shift(&m, bar(1), bar(1), bar(3)), bar(3));
        assert_eq!(musical_shift(&m, bar(3), bar(1), bar(3)), bar(5));
        let b4b3 = bar(4) + (SR * 60.0 / 120.0 * 2.0) as i64;
        let b6b3 = bar(6) as f64 + SR * 60.0 / 90.0 * 2.0;
        assert!((musical_shift(&m, b4b3, bar(1), bar(3)) as f64 - b6b3).abs() <= 1.0);
        // The same number of samples would have missed beat 3 by a bar's worth of 90 vs 120.
        assert!((b4b3 + (bar(3) - bar(1)) - b6b3 as i64).abs() > 10_000);
        // Backwards, and not before 0.
        assert_eq!(musical_shift(&m, bar(6), bar(3), bar(1)), bar(4));
        assert_eq!(musical_shift(&m, bar(1), bar(3), bar(1)), 0);
    }
}
