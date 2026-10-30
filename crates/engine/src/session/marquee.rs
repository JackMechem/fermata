//! The Tracks area's marquee selection (Logic's Marquee tool): a time range across some
//! tracks. Delete, Cut and a Pointer click inside it cut the regions at its edges, as the
//! Audio Track editor's range edits do; Copy takes the parts inside without changing anything.

use super::range_edit::sub_region;
use super::*;

impl Session {
    /// The regions on `tracks` that sound somewhere between `start` and `end`, loops included.
    pub fn marquee_regions(&self, tracks: &[Id], start: i64, end: i64) -> Vec<Id> {
        let (a, b) = (start.min(end), start.max(end));
        self.project.tracks.iter().filter(|t| tracks.contains(&t.id)).flat_map(|t| t.regions.iter()).filter(|r| r.start < b && r.end() > a).map(|r| r.id).collect()
    }

    /// Splits the regions under the marquee at its edges (looping ones become real copies
    /// first). Returns the pieces inside it, to select.
    pub fn marquee_split(&mut self, tracks: &[Id], start: i64, end: i64) -> Vec<Id> {
        let ids = self.marquee_regions(tracks, start, end);
        self.edit_range(&ids, start, end, RangeEdit::Split).unwrap_or_default()
    }

    /// Removes what is under the marquee: the regions are split at its edges and the parts
    /// inside deleted, in the drag mode (so Shuffle closes the gap, as a Delete does).
    pub fn marquee_delete(&mut self, tracks: &[Id], start: i64, end: i64) {
        let inside = self.marquee_split(tracks, start, end);
        if !inside.is_empty() {
            self.drag_delete_regions(&inside);
        }
    }

    /// Copies of the parts of the regions under the marquee, each with the index of its track
    /// (as the region clipboard holds them). A loop's repetitions count as regions of their
    /// own. The project is left as it is.
    pub fn marquee_copy(&self, tracks: &[Id], start: i64, end: i64) -> Vec<(usize, Region)> {
        let (a, b) = (start.min(end), start.max(end));
        let mut out = Vec::new();
        for (index, t) in self.project.tracks.iter().enumerate().filter(|(_, t)| tracks.contains(&t.id)) {
            for r in &t.regions {
                let passes = if r.is_looped() { r.real_copies() } else { vec![r.clone()] };
                for p in passes.iter().filter(|p| p.start < b && p.start + p.length > a) {
                    out.push((index, sub_region(p, a, b, p.id)));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::clip::Clip;

    /// A session on the silent ALSA null device with three audio tracks, each holding one
    /// region of a 48 000-frame clip at 10 000.
    fn session() -> (Session, Vec<Id>) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("fermata-marquee-{}", std::process::id()));
        let path = s.dir.join("Audio Files").join("tone.wav");
        let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48000, vec![vec![0.25; 48_000]]));
        let mut tracks = Vec::new();
        for _ in 0..3 {
            let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
            let id = s.project.alloc_id();
            let content = RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
            s.project.track_mut(t).unwrap().regions.push(Region { id, name: "Tone".into(), start: 10_000, length: 48_000, muted: false, color: None, content, loop_end: None, takes: None, params: Default::default(), pattern: None });
            s.push_regions(t);
            tracks.push(t);
        }
        (s, tracks)
    }

    fn spans(s: &Session, t: Id) -> Vec<(i64, i64)> {
        let mut v: Vec<(i64, i64)> = s.project.track(t).unwrap().regions.iter().map(|r| (r.start, r.start + r.length)).collect();
        v.sort();
        v
    }

    fn offset(r: &Region) -> i64 {
        match r.content {
            RegionContent::Audio { offset, .. } => offset,
            _ => panic!("not audio"),
        }
    }

    #[test]
    fn delete_cuts_a_hole_in_the_marked_tracks_only() {
        let (mut s, t) = session();
        s.marquee_delete(&t[..2], 30_000, 20_000);
        assert_eq!(spans(&s, t[0]), vec![(10_000, 20_000), (30_000, 58_000)]);
        assert_eq!(spans(&s, t[1]), vec![(10_000, 20_000), (30_000, 58_000)]);
        assert_eq!(spans(&s, t[2]), vec![(10_000, 58_000)], "the unmarked track is untouched");
        let right = s.project.track(t[0]).unwrap().regions.iter().find(|r| r.start == 30_000).unwrap().clone();
        assert_eq!(offset(&right), 20_000, "the right part still plays the same audio");
    }

    #[test]
    fn delete_in_shuffle_left_closes_the_gap() {
        let (mut s, t) = session();
        s.set_drag_mode(DragMode::ShuffleL);
        s.marquee_delete(&[t[0]], 20_000, 30_000);
        assert_eq!(spans(&s, t[0]), vec![(10_000, 20_000), (20_000, 48_000)]);
    }

    #[test]
    fn split_returns_the_inside_pieces() {
        let (mut s, t) = session();
        let inside = s.marquee_split(&t, 0, 20_000);
        assert_eq!(inside.len(), 3);
        for &tr in &t {
            assert_eq!(spans(&s, tr), vec![(10_000, 20_000), (20_000, 58_000)]);
        }
        for id in inside {
            let (tr, i) = s.find_region(id).unwrap();
            assert_eq!(s.project.track(tr).unwrap().regions[i].start, 10_000);
        }
        // A marquee over empty time splits nothing.
        assert!(s.marquee_split(&t, 60_000, 70_000).is_empty());
    }

    #[test]
    fn copy_takes_the_inside_and_changes_nothing() {
        let (mut s, t) = session();
        // A loop on the last track: one repetition after the region, 58 000 to 106 000.
        let id = s.project.track(t[2]).unwrap().regions[0].id;
        s.project.track_mut(t[2]).unwrap().regions[0].loop_end = Some(106_000);
        let clip = s.marquee_copy(&t[1..], 50_000, 70_000);
        assert_eq!(spans(&s, t[1]), vec![(10_000, 58_000)]);
        let got: Vec<(usize, i64, i64, i64)> = clip.iter().map(|(i, r)| (*i, r.start, r.length, offset(r))).collect();
        let (i1, i2) = (s.project.tracks.iter().position(|x| x.id == t[1]).unwrap(), s.project.tracks.iter().position(|x| x.id == t[2]).unwrap());
        assert_eq!(got, vec![(i1, 50_000, 8_000, 40_000), (i2, 50_000, 8_000, 40_000), (i2, 58_000, 12_000, 0)]);
        assert!(clip.iter().all(|(_, r)| r.loop_end.is_none()));
        assert_eq!(s.project.track(t[2]).unwrap().regions[0].id, id);
    }
}
