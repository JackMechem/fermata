//! Edit › Snap Edits to Zero Crossings (Logic): with it on, splits and trims of audio regions
//! land on the nearest zero crossing of their audio, so a cut doesn't click. The search itself
//! is [`crate::clip::nearest_zero_crossing`]; this decides where it applies. MIDI regions and
//! take folders (whose audio is a comp of several takes) are cut where asked.

use super::*;
use crate::clip::nearest_zero_crossing;

/// How far either side of an edit point a zero crossing is looked for, in seconds. Past this
/// the edit stays where it was asked, as moving it would be audible as a timing change.
pub const ZERO_CROSSING_WINDOW: f64 = 0.005;

impl Session {
    pub fn set_snap_zero_crossings(&mut self, on: bool) {
        self.project.snap_zero_crossings = on;
        self.dirty = true;
    }

    /// Where an edit of `r` at timeline position `pos` lands: on the nearest zero crossing of
    /// its audio when Snap Edits to Zero Crossings is on and one is near, otherwise `pos`.
    pub(super) fn zero_snap(&self, r: &Region, pos: i64) -> i64 {
        if !self.project.snap_zero_crossings || r.takes.is_some() {
            return pos;
        }
        let RegionContent::Audio { clip, offset, .. } = &r.content else { return pos };
        let Some(c) = self.clips.get(clip) else { return pos };
        let frame = offset + (pos - r.start);
        if frame < 0 {
            return pos;
        }
        let window = (ZERO_CROSSING_WINDOW * c.sample_rate as f64).round() as usize;
        match nearest_zero_crossing(&c.channels, frame as usize, window) {
            Some(z) => pos + (z as i64 - frame),
            None => pos,
        }
    }

    /// Where a cut of `r` at `pos` lands: [`Session::zero_snap`], as long as that stays inside
    /// the region (a crossing just past either end would leave nothing to cut).
    pub(super) fn zero_cut(&self, r: &Region, pos: i64) -> i64 {
        let at = self.zero_snap(r, pos);
        if at > r.start && at < r.start + r.length {
            at
        } else {
            pos
        }
    }

    /// A trim of region `id` to `start`/`length` with each edge that moves snapped to a zero
    /// crossing.
    pub(super) fn zero_trim(&self, id: Id, start: i64, length: i64) -> (i64, i64) {
        let Some(r) = self.region(id) else { return (start, length) };
        let end = start + length;
        let s = if start != r.start { self.zero_snap(r, start) } else { start };
        let e = if end != r.start + r.length { self.zero_snap(r, end) } else { end };
        (s, (e - s).max(1))
    }

    /// The regions of a range edit from `a` to `b` grouped by where each is cut once its ends
    /// are snapped to zero crossings: one `((a, b), regions)` per distinct pair.
    pub(super) fn zero_range_groups(&self, ids: &[Id], a: i64, b: i64) -> Vec<((i64, i64), Vec<Id>)> {
        let mut groups: Vec<((i64, i64), Vec<Id>)> = Vec::new();
        for &id in ids {
            let Some(r) = self.region(id) else { continue };
            let key = (self.zero_cut(r, a), self.zero_cut(r, b));
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, g)) => g.push(id),
                None => groups.push((key, vec![id])),
            }
        }
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::session::RangeEdit;

    /// A session with one audio track holding a region of a 100-frame-period sine (crossings
    /// every 50 frames) at 1000, starting 10 frames into the clip.
    fn session() -> (Session, Id, Id) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("fermata-zero-{}", std::process::id()));
        let path = s.dir.join("Audio Files").join("sine.wav");
        let sine: Vec<f32> = (0..48_000).map(|i| (i as f32 * std::f32::consts::TAU / 100.0).sin() * 0.5).collect();
        let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48_000, vec![sine.clone(), sine]));
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let id = s.project.alloc_id();
        let content = RegionContent::Audio { clip, offset: 10, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
        s.project.track_mut(t).unwrap().regions.push(Region { id, name: "Sine".into(), start: 1000, length: 20_000, muted: false, color: None, content, loop_end: None, takes: None, params: Default::default(), pattern: None });
        s.push_regions(t);
        (s, t, id)
    }

    /// Timeline position `pos` of the region is a zero crossing: clip frame `pos - 1000 + 10`
    /// is a multiple of 50 give or take a frame.
    fn on_crossing(pos: i64) -> bool {
        let f = pos - 1000 + 10;
        let m = f.rem_euclid(50);
        m <= 1 || m >= 49
    }

    fn spans(s: &Session, t: Id) -> Vec<(i64, i64)> {
        let mut v: Vec<(i64, i64)> = s.project.track(t).unwrap().regions.iter().map(|r| (r.start, r.start + r.length)).collect();
        v.sort();
        v
    }

    #[test]
    fn splits_move_to_a_crossing_only_when_on() {
        let (mut s, t, id) = session();
        s.split_at(5_020, &[id]);
        assert_eq!(spans(&s, t), vec![(1000, 5020), (5020, 21_000)]);

        let (mut s, t, id) = session();
        s.set_snap_zero_crossings(true);
        s.split_at(5_020, &[id]);
        let v = spans(&s, t);
        assert_eq!(v.len(), 2);
        let cut = v[0].1;
        assert_eq!(v[1].0, cut);
        assert!(on_crossing(cut) && (cut - 5_020).abs() <= 25, "{cut}");
    }

    #[test]
    fn multi_splits_range_edits_and_trims_snap() {
        let (mut s, t, id) = session();
        s.set_snap_zero_crossings(true);
        s.split_regions_at(&[id], &[3_013, 9_037]);
        let v = spans(&s, t);
        assert_eq!(v.len(), 3);
        assert!(on_crossing(v[0].1) && on_crossing(v[1].1), "{v:?}");

        let (mut s, t, id) = session();
        s.set_snap_zero_crossings(true);
        let inside = s.edit_range(&[id], 4_005, 8_033, RangeEdit::Split).unwrap();
        assert_eq!(inside.len(), 1);
        let r = s.region(inside[0]).unwrap();
        assert!(on_crossing(r.start) && on_crossing(r.start + r.length), "{} {}", r.start, r.length);
        assert_eq!(spans(&s, t).len(), 3);

        let (mut s, _, id) = session();
        s.set_snap_zero_crossings(true);
        s.drag_resize_region(id, 1_217, 20_000 - 217 - 1_111);
        let r = s.region(id).unwrap();
        assert!(on_crossing(r.start) && on_crossing(r.start + r.length), "{} {}", r.start, r.length);
        assert!((r.start - 1_217).abs() <= 25);
        // The untouched edge stays.
        let (mut s, _, id) = session();
        s.set_snap_zero_crossings(true);
        s.drag_resize_region(id, 1_000, 15_013);
        let r = s.region(id).unwrap();
        assert_eq!(r.start, 1_000);
        assert!(on_crossing(r.start + r.length));
    }
}
