//! Moving several regions as one edit (a multi-region drag in the Tracks area).

use serde::Deserialize;

use super::Session;
use crate::model::Id;

/// Where one region goes: its new start and, optionally, another track.
#[derive(Debug, Clone, Deserialize)]
pub struct RegionMove {
    pub id: Id,
    pub start: i64,
    pub track: Option<Id>,
}

impl Session {
    /// Moves regions in time and across tracks, as [`Session::move_region`] does for one: a region
    /// only changes track onto one of its own kind, and moved regions come to the front so they
    /// win where they now overlap. Each touched track is pushed to the engine once.
    pub fn move_regions(&mut self, moves: &[RegionMove]) {
        let ids: Vec<Id> = moves.iter().map(|m| m.id).collect();
        let pairs = self.crossfade_pairs(&ids);
        // Take every region out first, so a region moving onto a track another one leaves
        // can't be confused with it.
        let mut taken = Vec::new();
        for m in moves {
            let Some((from, idx)) = self.find_region(m.id) else { continue };
            let to = m.track.unwrap_or(from);
            let kind_ok = matches!((self.project.track(from), self.project.track(to)), (Some(a), Some(b)) if a.kind == b.kind);
            let mut r = self.project.track_mut(from).unwrap().regions.remove(idx);
            r.move_to(m.start.max(0));
            taken.push((from, if kind_ok { to } else { from }, r));
        }
        let mut touched = Vec::new();
        for (from, to, r) in taken {
            self.project.track_mut(to).unwrap().regions.push(r);
            for t in [from, to] {
                if !touched.contains(&t) {
                    touched.push(t);
                }
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
        self.auto_crossfade(&ids, pairs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::model::{InputSel, Note, Region, RegionContent, TrackKind};

    fn midi(s: &mut Session, track: Id, start: i64) -> Id {
        let id = s.project.alloc_id();
        let content = RegionContent::Midi { notes: vec![Note { start: 0, length: 100, pitch: 60, velocity: 100, muted: false }], cc: Vec::new() };
        s.project.track_mut(track).unwrap().regions.push(Region { id, name: "M".into(), start, length: 1000, muted: false, color: None, content, loop_end: None, takes: None });
        id
    }

    fn starts(s: &Session, track: Id) -> Vec<(Id, i64)> {
        s.project.track(track).unwrap().regions.iter().map(|r| (r.id, r.start)).collect()
    }

    #[test]
    fn moves_many_regions_across_tracks_of_their_kind() {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("fermata-region-moves-{}", std::process::id()));
        let a = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
        let b = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
        let audio = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let r1 = midi(&mut s, a, 1000);
        let r2 = midi(&mut s, a, 5000);
        let r3 = midi(&mut s, b, 2000);
        s.dirty = false;

        // r1 and r3 swap tracks; r2 can't go onto the audio track and only moves in time; a
        // start before zero is clamped; an unknown id is ignored.
        s.move_regions(&[
            RegionMove { id: r1, start: 1500, track: Some(b) },
            RegionMove { id: r3, start: -10, track: Some(a) },
            RegionMove { id: r2, start: 6000, track: Some(audio) },
            RegionMove { id: 9999, start: 0, track: None },
        ]);
        assert_eq!(starts(&s, a), vec![(r3, 0), (r2, 6000)]);
        assert_eq!(starts(&s, b), vec![(r1, 1500)]);
        assert!(starts(&s, audio).is_empty());
        assert!(s.dirty);
    }
}
