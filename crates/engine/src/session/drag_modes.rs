//! Region edits as the Tracks area makes them: in the project's drag mode (Logic's Drag
//! pop-up), and with Move Automation with Regions. No Overlap and the Shuffle modes are
//! applied on top of the plain edits (`move_regions`, `resize_region` and so on), whose X-Fade
//! handling stays as it is. The rules themselves are in `drag_modes.rs` and `auto_move.rs`.
//!
//! The Tauri commands for user edits call these; edits made on the way to something else
//! (range edits, Bounce in Place, Move to New Track, Repeat) use the plain ones, so they
//! don't shuffle a lane behind the user's back.

use super::region_moves::RegionMove;
use super::*;
use crate::auto_move::{move_with_regions, Shift};
use crate::drag_modes::{no_overlap, shuffle, shuffle_resize, Side, Span};

/// Every track's lane as the drag modes see it, before an edit.
type Lanes = Vec<(Id, Vec<Span>)>;

impl Session {
    /// Moves regions (a drag in the Tracks area) in the drag mode. `automation` says whether
    /// the automation under each region that stays on its track goes with it; None follows
    /// Settings › Editing, where Ask counts as Never (the frontend asks, and passes the
    /// answer).
    pub fn drag_move_regions(&mut self, moves: &[RegionMove], automation: Option<bool>) {
        let ids: Vec<Id> = moves.iter().map(|m| m.id).collect();
        let (lanes, was) = (self.lanes(), self.places(&ids));
        self.move_regions(moves);
        self.apply_drag_mode(&lanes, &ids);
        self.move_automation(&was, automation);
    }

    /// Nudges regions by `delta` samples in the drag mode, as a move.
    pub fn drag_nudge_regions(&mut self, ids: &[Id], delta: i64, automation: Option<bool>) {
        let (lanes, was) = (self.lanes(), self.places(ids));
        self.nudge_regions(ids, delta);
        self.apply_drag_mode(&lanes, ids);
        self.move_automation(&was, automation);
    }

    /// Trims a region's edges in the drag mode: No Overlap stops it at its neighbours, the
    /// Shuffle modes move them along.
    pub fn drag_resize_region(&mut self, id: Id, start: i64, length: i64) {
        let before = self.find_region(id).map(|(t, _)| (t, self.lane(t)));
        self.resize_region(id, start, length);
        match self.side() {
            Some(side) => {
                let (Some((track, lane)), Some(r)) = (before, self.region(id)) else { return };
                let starts = shuffle_resize(&lane, id, r.end() - r.start, side);
                self.place_regions(track, &starts);
            }
            None => self.trim_overlaps(&[id]),
        }
    }

    /// Deletes regions; in the Shuffle modes the regions after (Shuffle L) or before
    /// (Shuffle R) close the gap.
    pub fn drag_delete_regions(&mut self, ids: &[Id]) {
        let lanes = self.lanes();
        self.delete_regions(ids);
        self.apply_drag_mode(&lanes, ids);
    }

    /// Pastes (or Alt-drag copies) regions in the drag mode. Returns the new regions.
    pub fn drag_paste_regions(&mut self, items: &[(Id, Region)], offset: i64) -> Vec<Id> {
        let lanes = self.lanes();
        let made = self.paste_regions(items, offset);
        self.apply_drag_mode(&lanes, &made);
        made.into_iter().filter(|&id| self.find_region(id).is_some()).collect()
    }

    fn side(&self) -> Option<Side> {
        match self.project.drag_mode {
            DragMode::ShuffleL => Some(Side::Left),
            DragMode::ShuffleR => Some(Side::Right),
            _ => None,
        }
    }

    fn lane(&self, track: Id) -> Vec<Span> {
        self.project.track(track).map(|t| t.regions.iter().map(|r| Span { id: r.id, start: r.start, len: r.end() - r.start }).collect()).unwrap_or_default()
    }

    fn lanes(&self) -> Lanes {
        self.project.tracks.iter().map(|t| (t.id, self.lane(t.id))).collect()
    }

    /// Where each region is: (region, track, start, end).
    fn places(&self, ids: &[Id]) -> Vec<(Id, Id, i64, i64)> {
        ids.iter().filter_map(|&id| self.find_region(id).map(|(t, _)| (id, t, self.region(id).unwrap()))).map(|(id, t, r)| (id, t, r.start, r.end())).collect()
    }

    /// After `ids` were moved, deleted or pasted (`lanes` is how the tracks were before):
    /// No Overlap shortens what now overlaps, the Shuffle modes rearrange the lanes the
    /// regions left and arrived on.
    fn apply_drag_mode(&mut self, lanes: &Lanes, ids: &[Id]) {
        let Some(side) = self.side() else {
            if self.project.drag_mode == DragMode::NoOverlap {
                self.trim_overlaps(ids);
            }
            return;
        };
        for (track, before) in lanes {
            let leaving: Vec<Id> = before.iter().map(|s| s.id).filter(|id| ids.contains(id)).collect();
            let arriving: Vec<Span> = self.lane(*track).into_iter().filter(|s| ids.contains(&s.id)).collect();
            if leaving.is_empty() && arriving.is_empty() {
                continue;
            }
            let starts = shuffle(before, &leaving, &arriving, side);
            self.place_regions(*track, &starts);
        }
    }

    /// Moves regions of `track` to the given starts.
    fn place_regions(&mut self, track: Id, starts: &[(Id, i64)]) {
        let Some(t) = self.project.track_mut(track) else { return };
        let mut changed = false;
        for r in &mut t.regions {
            if let Some(&(_, s)) = starts.iter().find(|(id, _)| *id == r.id) {
                if s != r.start {
                    r.move_to(s.max(0));
                    changed = true;
                }
            }
        }
        if changed {
            self.push_regions(track);
            self.dirty = true;
        }
    }

    /// No Overlap: shortens the regions that overlap one of `ids`, removing those covered
    /// from their start.
    fn trim_overlaps(&mut self, ids: &[Id]) {
        let tracks: Vec<Id> = self.project.tracks.iter().filter(|t| t.regions.iter().any(|r| ids.contains(&r.id))).map(|t| t.id).collect();
        for track in tracks {
            let cuts = no_overlap(&self.lane(track), ids);
            if cuts.is_empty() {
                continue;
            }
            let t = self.project.track_mut(track).unwrap();
            t.regions.retain(|r| !cuts.iter().any(|&(id, len)| id == r.id && len <= 0));
            for r in &mut t.regions {
                if let Some(&(_, len)) = cuts.iter().find(|(id, _)| *id == r.id) {
                    shorten(r, len);
                }
            }
            self.push_regions(track);
            self.dirty = true;
        }
    }

    /// Move Automation with Regions: the automation under each region in `was` (where it
    /// was before the edit) moves to where the region is now, if it stayed on its track.
    fn move_automation(&mut self, was: &[(Id, Id, i64, i64)], automation: Option<bool>) {
        if !automation.unwrap_or(self.prefs.editing.move_automation == MoveAutomation::Always) {
            return;
        }
        let mut by_track: Vec<(Id, Vec<Shift>)> = Vec::new();
        for &(id, track, lo, hi) in was {
            let Some((now, idx)) = self.find_region(id) else { continue };
            let start = self.project.track(now).unwrap().regions[idx].start;
            if now != track || start == lo {
                continue;
            }
            let shift = Shift { lo, hi, delta: start - lo };
            match by_track.iter_mut().find(|(t, _)| *t == track) {
                Some((_, v)) => v.push(shift),
                None => by_track.push((track, vec![shift])),
            }
        }
        for (track, shifts) in by_track {
            let Some(t) = self.project.track_mut(track) else { continue };
            let mut changed = false;
            for lane in &mut t.automation {
                let moved = move_with_regions(&lane.points, &shifts);
                if moved != lane.points {
                    lane.points = moved;
                    changed = true;
                }
            }
            if changed {
                self.push_automation(track);
                self.dirty = true;
            }
        }
    }
}

/// Shortens a region to sound for `len` samples: its loop first, then the region itself.
fn shorten(r: &mut Region, len: i64) {
    let end = r.start + len;
    if end < r.start + r.length {
        r.length = len;
        r.loop_end = None;
        if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
            *fade_out = (*fade_out).min(len);
            *fade_in = (*fade_in).min(len - *fade_out);
        }
    } else {
        r.loop_end = Some(end);
        if !r.is_looped() {
            r.loop_end = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;

    fn session() -> (Session, Id) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("fermata-drag-modes-{}", std::process::id()));
        let t = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
        (s, t)
    }

    fn midi(s: &mut Session, track: Id, start: i64, length: i64) -> Id {
        let id = s.project.alloc_id();
        let content = RegionContent::Midi { notes: vec![Note { start: 0, length: 100, pitch: 60, velocity: 100, muted: false }], cc: Vec::new() };
        s.project.track_mut(track).unwrap().regions.push(Region { id, name: "M".into(), start, length, muted: false, color: None, content, loop_end: None, takes: None, params: Default::default() });
        s.push_regions(track);
        id
    }

    /// (id, start, length) by start.
    fn lane(s: &Session, track: Id) -> Vec<(Id, i64, i64)> {
        let mut v: Vec<(Id, i64, i64)> = s.project.track(track).unwrap().regions.iter().map(|r| (r.id, r.start, r.length)).collect();
        v.sort_by_key(|x| x.1);
        v
    }

    #[test]
    fn overlap_and_xfade_modes_are_the_plain_edits() {
        let (mut s, t) = session();
        let a = midi(&mut s, t, 0, 1000);
        let b = midi(&mut s, t, 1000, 1000);
        s.drag_move_regions(&[RegionMove { id: b, start: 500, track: None }], None);
        assert_eq!(lane(&s, t), vec![(a, 0, 1000), (b, 500, 1000)]);
        s.drag_delete_regions(&[a]);
        assert_eq!(lane(&s, t), vec![(b, 500, 1000)]);
    }

    #[test]
    fn no_overlap_trims_the_left_region() {
        let (mut s, t) = session();
        s.set_drag_mode(DragMode::NoOverlap);
        let a = midi(&mut s, t, 0, 1000);
        let b = midi(&mut s, t, 2000, 1000);
        s.drag_move_regions(&[RegionMove { id: b, start: 600, track: None }], None);
        assert_eq!(lane(&s, t), vec![(a, 0, 600), (b, 600, 1000)]);
        // Lengthening a's end is stopped by b.
        s.drag_resize_region(a, 0, 5000);
        assert_eq!(lane(&s, t), vec![(a, 0, 600), (b, 600, 1000)]);
        // A copy pasted exactly over b replaces it.
        let r = s.region(b).unwrap().clone();
        let made = s.drag_paste_regions(&[(t, r)], 0);
        assert_eq!(lane(&s, t), vec![(a, 0, 600), (made[0], 600, 1000)]);
    }

    #[test]
    fn shuffle_l_closes_and_opens_gaps() {
        let (mut s, t) = session();
        s.set_drag_mode(DragMode::ShuffleL);
        let a = midi(&mut s, t, 0, 1000);
        let b = midi(&mut s, t, 1000, 500);
        let c = midi(&mut s, t, 1500, 1000);
        // Deleting b pulls c left.
        s.drag_delete_regions(&[b]);
        assert_eq!(lane(&s, t), vec![(a, 0, 1000), (c, 1000, 1000)]);
        // Dragging a onto c's second half swaps them.
        s.drag_move_regions(&[RegionMove { id: a, start: 1700, track: None }], None);
        assert_eq!(lane(&s, t), vec![(c, 0, 1000), (a, 1000, 1000)]);
        // Trimming c's end shorter pulls a along.
        s.drag_resize_region(c, 0, 400);
        assert_eq!(lane(&s, t), vec![(c, 0, 400), (a, 400, 1000)]);
        // Trimming c's start keeps it at 0 and pulls a further.
        s.drag_resize_region(c, 100, 300);
        assert_eq!(lane(&s, t), vec![(c, 0, 300), (a, 300, 1000)]);
        // Pasting a copy of c onto a's first half inserts it before a.
        let r = s.region(c).unwrap().clone();
        let made = s.drag_paste_regions(&[(t, r)], 310);
        assert_eq!(lane(&s, t), vec![(c, 0, 300), (made[0], 300, 300), (a, 600, 1000)]);
    }

    #[test]
    fn automation_moves_with_the_region() {
        let (mut s, t) = session();
        let a = midi(&mut s, t, 1000, 1000);
        s.set_automation_lane(t, AutoParam::Volume, vec![AutoPoint { pos: 0, value: -10.0, curve: 0.0 }, AutoPoint { pos: 1200, value: 0.0, curve: 0.0 }, AutoPoint { pos: 1800, value: 0.0, curve: 0.0 }, AutoPoint { pos: 3000, value: -10.0, curve: 0.0 }]);
        let curve = |s: &Session, x: i64| crate::automation::value_at(&s.project.track(t).unwrap().automation[0].points, x).unwrap();
        // Never (the default): the curve stays.
        s.drag_move_regions(&[RegionMove { id: a, start: 5000, track: None }], None);
        assert_eq!(curve(&s, 1500), 0.0);
        // Back, then Always.
        s.drag_move_regions(&[RegionMove { id: a, start: 1000, track: None }], None);
        s.prefs.editing.move_automation = MoveAutomation::Always;
        s.drag_move_regions(&[RegionMove { id: a, start: 5000, track: None }], None);
        assert_eq!(curve(&s, 5500), 0.0);
        assert!((curve(&s, 1500) - curve(&s, 1000)).abs() < 0.5, "the old place is bridged");
        // An explicit answer (Ask) overrides the setting.
        s.drag_nudge_regions(&[a], 1000, Some(false));
        assert_eq!(curve(&s, 5500), 0.0);
        assert!(curve(&s, 6900) < -1.0);
    }
}
