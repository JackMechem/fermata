//! Pure helpers for the content of MIDI regions (notes plus controller events), used by the
//! session's region edits: split, trim, join and the wholesale replacement the Piano Roll sends.

use crate::model::{CcEvent, Note, CC_KINDS};

/// Sorts notes and controller events by start and clamps every field into range.
pub fn normalize(notes: &mut [Note], cc: &mut [CcEvent]) {
    for n in notes.iter_mut() {
        n.pitch = n.pitch.min(127);
        n.velocity = n.velocity.clamp(1, 127);
        n.length = n.length.max(1);
    }
    for c in cc.iter_mut() {
        let max = if c.kind == crate::model::CcKind::PitchBend { 16383 } else { 127 };
        c.value = c.value.min(max);
    }
    notes.sort_by_key(|n| (n.start, n.pitch));
    cc.sort_by_key(|c| c.start);
}

/// The controller events from `cut` on, moved so `cut` becomes 0. Each controller's last value
/// before `cut` is carried to 0 (unless that controller changes right at `cut`), so the later
/// part starts in the state the earlier part left it in.
pub fn cc_from(cc: &[CcEvent], cut: i64) -> Vec<CcEvent> {
    let mut last: [Option<CcEvent>; CC_KINDS] = [None; CC_KINDS];
    for c in cc.iter().filter(|c| c.start < cut) {
        last[c.kind.index()] = Some(*c);
    }
    let mut out: Vec<CcEvent> = last
        .iter()
        .flatten()
        .filter(|e| !cc.iter().any(|c| c.start == cut && c.kind == e.kind))
        .map(|e| CcEvent { start: 0, ..*e })
        .collect();
    out.extend(cc.iter().filter(|c| c.start >= cut).map(|c| CcEvent { start: c.start - cut, ..*c }));
    out.sort_by_key(|c| c.start);
    out
}

/// Splits region content at `cut` (relative to the region start). Notes go to the side they
/// start on; the right side's times are relative to the cut, and its controllers start where
/// the left side's left off.
#[allow(clippy::type_complexity)]
pub fn split(notes: &[Note], cc: &[CcEvent], cut: i64) -> ((Vec<Note>, Vec<CcEvent>), (Vec<Note>, Vec<CcEvent>)) {
    let left_notes = notes.iter().filter(|n| n.start < cut).copied().collect();
    let right_notes = notes.iter().filter(|n| n.start >= cut).map(|n| Note { start: n.start - cut, ..*n }).collect();
    let left_cc = cc.iter().filter(|c| c.start < cut).copied().collect();
    ((left_notes, left_cc), (right_notes, cc_from(cc, cut)))
}

/// Moves a region's start by `delta` samples while its content stays put on the timeline.
/// Trimming the front (delta > 0) drops notes that end before the new start and chases the
/// controllers to it.
pub fn trim_front(notes: &mut Vec<Note>, cc: &mut Vec<CcEvent>, delta: i64) {
    for n in notes.iter_mut() {
        n.start -= delta;
    }
    notes.retain(|n| n.start + n.length > 0);
    if delta > 0 {
        *cc = cc_from(cc, delta);
    } else {
        cc.iter_mut().for_each(|c| c.start -= delta);
    }
}

/// Merges several regions' content into one starting at 0. Each part comes with its offset
/// from the joined region's start. Controller events that only repeat the value already in
/// effect (as a split leaves at the cut) are dropped, so split-then-join gives back the original.
pub fn join<'a>(parts: impl IntoIterator<Item = (i64, &'a [Note], &'a [CcEvent])>) -> (Vec<Note>, Vec<CcEvent>) {
    let mut notes = Vec::new();
    let mut cc = Vec::new();
    for (off, n, c) in parts {
        notes.extend(n.iter().map(|x| Note { start: x.start + off, ..*x }));
        cc.extend(c.iter().map(|x| CcEvent { start: x.start + off, ..*x }));
    }
    notes.sort_by_key(|n| n.start);
    cc.sort_by_key(|c| c.start);
    let mut last: [Option<u16>; CC_KINDS] = [None; CC_KINDS];
    cc.retain(|c| {
        let k = c.kind.index();
        let repeat = last[k] == Some(c.value);
        last[k] = Some(c.value);
        !repeat
    });
    (notes, cc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CcKind;

    fn note(start: i64, pitch: u8) -> Note {
        Note { start, length: 10, pitch, velocity: 100, muted: false }
    }

    fn cc(start: i64, kind: CcKind, value: u16) -> CcEvent {
        CcEvent { start, kind, value }
    }

    const MOD: CcKind = CcKind::Controller(1);

    #[test]
    fn split_cuts_and_chases_controllers() {
        let notes = vec![note(0, 60), note(50, 62), note(100, 64)];
        let events = vec![cc(0, MOD, 10), cc(40, MOD, 20), cc(60, CcKind::PitchBend, 9000), cc(100, MOD, 30), cc(120, MOD, 40)];
        let ((ln, lc), (rn, rc)) = split(&notes, &events, 100);
        assert_eq!(ln.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![60, 62]);
        assert_eq!(rn.len(), 1);
        assert_eq!(rn[0].start, 0);
        assert_eq!(lc, vec![cc(0, MOD, 10), cc(40, MOD, 20), cc(60, CcKind::PitchBend, 9000)]);
        // Mod changes right at the cut, so only the bend is carried over.
        assert_eq!(rc, vec![cc(0, CcKind::PitchBend, 9000), cc(0, MOD, 30), cc(20, MOD, 40)]);
    }

    #[test]
    fn split_carries_last_value_to_the_cut() {
        let events = vec![cc(0, MOD, 10), cc(40, MOD, 20)];
        let (_, (_, rc)) = split(&[], &events, 70);
        assert_eq!(rc, vec![cc(0, MOD, 20)]);
    }

    #[test]
    fn join_merges_in_order() {
        let a_notes = vec![note(0, 60)];
        let a_cc = vec![cc(0, MOD, 1), cc(90, MOD, 2)];
        let b_notes = vec![note(5, 67)];
        let b_cc = vec![cc(0, MOD, 3), cc(10, CcKind::Aftertouch, 50)];
        let (n, c) = join([(100, b_notes.as_slice(), b_cc.as_slice()), (0, a_notes.as_slice(), a_cc.as_slice())]);
        assert_eq!(n.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>(), vec![(0, 60), (105, 67)]);
        assert_eq!(c, vec![cc(0, MOD, 1), cc(90, MOD, 2), cc(100, MOD, 3), cc(110, CcKind::Aftertouch, 50)]);
    }

    #[test]
    fn split_then_join_round_trips_notes() {
        let notes = vec![note(0, 60), note(30, 61), note(80, 62)];
        let events = vec![cc(0, MOD, 5), cc(50, MOD, 6), cc(90, MOD, 7)];
        let ((ln, lc), (rn, rc)) = split(&notes, &events, 40);
        let (n, c) = join([(0, ln.as_slice(), lc.as_slice()), (40, rn.as_slice(), rc.as_slice())]);
        assert_eq!(n.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>(), vec![(0, 60), (30, 61), (80, 62)]);
        // The value chased to the cut repeats what was in effect, so it goes again.
        assert_eq!(c, events);
    }

    #[test]
    fn trim_front_shifts_and_chases() {
        let mut notes = vec![note(0, 60), note(50, 62)];
        let mut events = vec![cc(10, MOD, 1), cc(70, MOD, 2)];
        trim_front(&mut notes, &mut events, 30);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].start, 20);
        assert_eq!(events, vec![cc(0, MOD, 1), cc(40, MOD, 2)]);
        // Extending the front back out only shifts.
        trim_front(&mut notes, &mut events, -30);
        assert_eq!(notes[0].start, 50);
        assert_eq!(events, vec![cc(30, MOD, 1), cc(70, MOD, 2)]);
    }

    #[test]
    fn json_shapes_match_the_ui() {
        // What api.ts sends: kinds tagged by "type", notes from older projects without "muted".
        let c: Vec<CcEvent> = serde_json::from_str(r#"[{"start":5,"kind":{"type":"controller","cc":64},"value":127},{"start":9,"kind":{"type":"pitch_bend"},"value":8192},{"start":9,"kind":{"type":"aftertouch"},"value":3}]"#).unwrap();
        assert_eq!(c, vec![cc(5, CcKind::Controller(64), 127), cc(9, CcKind::PitchBend, 8192), cc(9, CcKind::Aftertouch, 3)]);
        assert_eq!(serde_json::to_string(&CcKind::PitchBend).unwrap(), r#"{"type":"pitch_bend"}"#);
        let n: Note = serde_json::from_str(r#"{"start":1,"length":2,"pitch":60,"velocity":90}"#).unwrap();
        assert!(!n.muted);
        let r: crate::model::RegionContent = serde_json::from_str(r#"{"type":"midi","notes":[]}"#).unwrap();
        assert!(matches!(r, crate::model::RegionContent::Midi { cc, .. } if cc.is_empty()));
    }

    #[test]
    fn normalize_sorts_and_clamps() {
        let mut notes = vec![Note { start: 20, length: 0, pitch: 200, velocity: 0, muted: false }, note(5, 60)];
        let mut events = vec![cc(9, MOD, 500), cc(1, CcKind::PitchBend, 20000)];
        normalize(&mut notes, &mut events);
        assert_eq!(notes[0].start, 5);
        assert_eq!((notes[1].pitch, notes[1].velocity, notes[1].length), (127, 1, 1));
        assert_eq!(events, vec![cc(1, CcKind::PitchBend, 16383), cc(9, MOD, 127)]);
    }
}
