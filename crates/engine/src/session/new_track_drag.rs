//! Dragging regions past the last track (Logic's behaviour): each source lane that falls past
//! the end gets a new track of its own below the last one, and the regions move or copy there
//! with whatever else the same drag did, as one edit. The Tracks area works out which regions
//! go where (`src/newTrackDrag.ts`).

use super::region_moves::RegionMove;
use super::*;

impl Session {
    /// Moves regions as [`Session::drag_move_regions`] does, with `lanes` going onto new tracks:
    /// (source track, its regions' moves), in the order the new tracks are to have. A region on
    /// a frozen track, or heading onto one, stays where it is. Returns the regions moved to new
    /// tracks and selects the last new track, as Move to New Track does.
    pub fn drag_regions_to_new_tracks(&mut self, moves: &[RegionMove], lanes: &[(Id, Vec<RegionMove>)], automation: Option<bool>) -> Vec<Id> {
        let mut all: Vec<RegionMove> = moves.iter().filter(|m| self.movable(m.id, m.track)).cloned().collect();
        let mut made = Vec::new();
        let mut last = None;
        for (source, regions) in lanes {
            let on_lane: Vec<&RegionMove> = regions.iter().filter(|m| self.find_region(m.id).is_some_and(|(t, _)| t == *source) && self.movable(m.id, None)).collect();
            if on_lane.is_empty() {
                continue;
            }
            let Some(track) = self.new_last_track(*source) else { continue };
            all.extend(on_lane.iter().map(|m| RegionMove { id: m.id, start: m.start, track: Some(track) }));
            made.extend(on_lane.iter().map(|m| m.id));
            last = Some(track);
        }
        self.sync_tracks();
        // The new lanes are empty, so the drag mode only rearranges the lanes regions left.
        self.drag_move_regions(&all, automation);
        if last.is_some() {
            self.select_track(last);
        }
        made
    }

    /// Copies regions as [`Session::drag_paste_regions`] does (an Alt-drag), with `lanes` going
    /// onto new tracks: (source track, the copies in place). The originals stay. Returns every
    /// copy made and selects the last new track.
    pub fn drag_copy_regions_to_new_tracks(&mut self, items: &[(Id, Region)], lanes: &[(Id, Vec<Region>)]) -> Vec<Id> {
        let mut all: Vec<(Id, Region)> = items.iter().filter(|(t, _)| self.project.track(*t).is_some_and(|t| t.frozen.is_none())).cloned().collect();
        let mut last = None;
        for (source, regions) in lanes {
            let on_lane: Vec<&Region> = regions.iter().filter(|r| self.region_fits(*source, r)).collect();
            if on_lane.is_empty() {
                continue;
            }
            let Some(track) = self.new_last_track(*source) else { continue };
            all.extend(on_lane.into_iter().map(|r| (track, r.clone())));
            last = Some(track);
        }
        self.sync_tracks();
        let made = self.drag_paste_regions(&all, 0);
        if last.is_some() {
            self.select_track(last);
        }
        made
    }

    /// Whether a drag can take region `id` off its track and onto `to`: neither may be frozen.
    fn movable(&self, id: Id, to: Option<Id>) -> bool {
        let live = |t: Id| self.project.track(t).is_some_and(|t| t.frozen.is_none());
        self.find_region(id).is_some_and(|(from, _)| live(from)) && to.is_none_or(live)
    }

    /// An empty copy of `source` with its settings, as Move to New Track makes, but below every
    /// track and outside any stack: the drag showed it below the last track, whatever `source`
    /// sits in. The caller runs `sync_tracks`. (Leaving a summing stack, a track on the default
    /// output plays to Stereo Out.)
    fn new_last_track(&mut self, source: Id) -> Option<Id> {
        if !matches!(self.project.track(source)?.kind, TrackKind::Audio | TrackKind::Instrument) {
            return None;
        }
        let id = *self.duplicate_tracks(&[source], false).first()?;
        let at = self.project.tracks.iter().position(|t| t.id == id)?;
        let mut t = self.project.tracks.remove(at);
        t.parent = None;
        self.project.tracks.push(t);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;

    fn session() -> Session {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("fermata-new-track-drag-{}", std::process::id()));
        s
    }

    fn inst(s: &mut Session) -> Id {
        s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap()
    }

    fn midi(s: &mut Session, track: Id, start: i64) -> Id {
        let id = s.project.alloc_id();
        let content = RegionContent::Midi { notes: vec![Note { start: 0, length: 100, pitch: 60, velocity: 100, muted: false }], cc: Vec::new() };
        s.project.track_mut(track).unwrap().regions.push(Region { id, name: "M".into(), start, length: 1000, muted: false, color: None, content, loop_end: None, takes: None, params: Default::default(), pattern: None });
        s.push_regions(track);
        id
    }

    fn audio(s: &mut Session, track: Id, start: i64) -> Id {
        let id = s.project.alloc_id();
        let content = RegionContent::Audio { clip: 0, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
        s.project.track_mut(track).unwrap().regions.push(Region { id, name: "A".into(), start, length: 1000, muted: false, color: None, content, loop_end: None, takes: None, params: Default::default(), pattern: None });
        id
    }

    fn to(id: Id, start: i64) -> RegionMove {
        RegionMove { id, start, track: None }
    }

    fn starts(s: &Session, track: Id) -> Vec<(Id, i64)> {
        s.project.track(track).unwrap().regions.iter().map(|r| (r.id, r.start)).collect()
    }

    fn ids(s: &Session) -> Vec<Id> {
        s.project.tracks.iter().map(|t| t.id).collect()
    }

    #[test]
    fn moves_onto_a_new_track_of_the_same_kind_as_one_undo_step() {
        let mut s = session();
        let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let i = inst(&mut s);
        let last = inst(&mut s);
        s.project.track_mut(a).unwrap().volume_db = -6.0;
        s.project.track_mut(a).unwrap().color = "#123456".into();
        let ra = audio(&mut s, a, 1000);
        let ri = midi(&mut s, i, 2000);
        let before = ids(&s);

        s.checkpoint("Move Regions to New Tracks", None, false);
        let made = s.drag_regions_to_new_tracks(&[], &[(a, vec![to(ra, 1500)]), (i, vec![to(ri, 2500)])], None);
        assert_eq!(made, vec![ra, ri]);

        // Two new tracks below the last, in lane order, with their sources' kinds and settings.
        let order = ids(&s);
        assert_eq!(order.len(), 5);
        assert_eq!(&order[..3], &before[..]);
        let (na, ni) = (order[3], order[4]);
        assert_eq!(s.project.track(na).unwrap().kind, TrackKind::Audio);
        assert_eq!(s.project.track(ni).unwrap().kind, TrackKind::Instrument);
        assert_eq!(s.project.track(na).unwrap().volume_db, -6.0);
        assert_eq!(s.project.track(na).unwrap().color, "#123456");
        assert_eq!(starts(&s, na), vec![(ra, 1500)]);
        assert_eq!(starts(&s, ni), vec![(ri, 2500)]);
        assert!(starts(&s, a).is_empty() && starts(&s, i).is_empty() && starts(&s, last).is_empty());
        assert_eq!(s.selected, Some(ni));

        // One undo takes the tracks away and puts the regions back; redo does it again.
        s.undo();
        assert_eq!(ids(&s), before);
        assert_eq!(starts(&s, a), vec![(ra, 1000)]);
        assert_eq!(starts(&s, i), vec![(ri, 2000)]);
        assert!(s.undo_stack.is_empty());
        assert!(s.selected.is_none_or(|t| before.contains(&t)), "the selection isn't left on a removed track");
        s.redo();
        assert_eq!(ids(&s), order);
        assert_eq!(starts(&s, ni), vec![(ri, 2500)]);
    }

    #[test]
    fn regions_landing_on_tracks_move_in_the_same_edit() {
        let mut s = session();
        let (a, b) = (inst(&mut s), inst(&mut s));
        let (r1, r2, r3) = (midi(&mut s, a, 1000), midi(&mut s, a, 5000), midi(&mut s, b, 2000));
        s.drag_regions_to_new_tracks(&[RegionMove { id: r1, start: 1100, track: Some(b) }, RegionMove { id: r2, start: 5100, track: Some(b) }], &[(b, vec![to(r3, 2100)])], None);
        let order = ids(&s);
        assert_eq!(order.len(), 3);
        assert!(starts(&s, a).is_empty());
        assert_eq!(starts(&s, b), vec![(r1, 1100), (r2, 5100)]);
        assert_eq!(starts(&s, order[2]), vec![(r3, 2100)]);
    }

    #[test]
    fn a_new_track_sits_below_every_stack_and_outside_it() {
        let mut s = session();
        let a = inst(&mut s);
        let other = inst(&mut s);
        let stack = s.create_stack(&[a, other], true).unwrap();
        let r = midi(&mut s, a, 1000);
        s.drag_regions_to_new_tracks(&[], &[(a, vec![to(r, 1000)])], None);
        let new = *ids(&s).last().unwrap();
        assert_eq!(s.project.track(new).unwrap().parent, None);
        assert_eq!(s.project.track(a).unwrap().parent, Some(stack));
        assert_eq!(starts(&s, new), vec![(r, 1000)]);
    }

    #[test]
    fn shuffle_closes_the_lane_left_and_the_new_lane_keeps_the_drop_position() {
        let mut s = session();
        s.project.drag_mode = DragMode::ShuffleL;
        let a = inst(&mut s);
        let r1 = midi(&mut s, a, 0);
        let r2 = midi(&mut s, a, 1000);
        s.drag_regions_to_new_tracks(&[], &[(a, vec![to(r1, 7000)])], None);
        let new = *ids(&s).last().unwrap();
        assert_eq!(starts(&s, a), vec![(r2, 0)]);
        assert_eq!(starts(&s, new), vec![(r1, 7000)]);
    }

    #[test]
    fn automation_stays_on_the_source_track() {
        let mut s = session();
        s.prefs.editing.move_automation = MoveAutomation::Always;
        let a = inst(&mut s);
        let r = midi(&mut s, a, 1000);
        let points = vec![AutoPoint { pos: 1200, value: -3.0, curve: 0.0 }, AutoPoint { pos: 1800, value: -3.0, curve: 0.0 }];
        s.set_automation_lane(a, AutoParam::Volume, points.clone());
        s.drag_regions_to_new_tracks(&[], &[(a, vec![to(r, 9000)])], None);
        let new = *ids(&s).last().unwrap();
        assert_eq!(s.project.track(a).unwrap().automation[0].points, points);
        assert!(s.project.track(new).unwrap().automation.is_empty());
    }

    #[test]
    fn copies_go_onto_new_tracks_and_the_originals_stay() {
        let mut s = session();
        let (a, b) = (inst(&mut s), inst(&mut s));
        let (r1, r2) = (midi(&mut s, a, 1000), midi(&mut s, b, 2000));
        let (c1, c2) = (s.region(r1).unwrap().clone(), Region { start: 2500, ..s.region(r2).unwrap().clone() });
        s.checkpoint("Copy Regions to New Track", None, false);
        let made = s.drag_copy_regions_to_new_tracks(&[(a, c1)], &[(b, vec![c2])]);
        assert_eq!(made.len(), 2);
        let order = ids(&s);
        assert_eq!(order.len(), 3);
        assert_eq!(starts(&s, b), vec![(r2, 2000)]);
        assert_eq!(starts(&s, a), vec![(r1, 1000), (made[0], 1000)]);
        assert_eq!(starts(&s, order[2]), vec![(made[1], 2500)]);
        assert_eq!(s.selected, Some(order[2]));
        s.undo();
        assert_eq!(ids(&s), vec![a, b]);
        assert_eq!(starts(&s, a), vec![(r1, 1000)]);
    }

    #[test]
    fn nothing_to_place_makes_no_track() {
        let mut s = session();
        let (a, b) = (inst(&mut s), inst(&mut s));
        let r = midi(&mut s, a, 1000);
        // A frozen source keeps its region, and an unknown or foreign id makes no empty track.
        s.project.track_mut(a).unwrap().frozen = Some(FreezeInfo { clip: 0, start: 0 });
        s.drag_regions_to_new_tracks(&[], &[(a, vec![to(r, 2000)]), (b, vec![to(r, 2000), to(9999, 0)])], None);
        assert_eq!(ids(&s), vec![a, b]);
        assert_eq!(starts(&s, a), vec![(r, 1000)]);
        // A copy with content of the wrong kind or a clip that isn't loaded doesn't fit either.
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let wrong = s.region(r).unwrap().clone();
        assert!(s.drag_copy_regions_to_new_tracks(&[], &[(t, vec![wrong])]).is_empty());
        assert_eq!(ids(&s), vec![a, b, t]);
    }
}
