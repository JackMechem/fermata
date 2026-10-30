//! Edits for the Audio Track editor: splitting regions at several points in one step, and the
//! actions on a selected time range (trim, silence, delete, normalize, reverse, fades). They
//! are all non-destructive: they split regions and change region settings, and only Reverse
//! writes a new file (as Reverse on a whole region does), leaving the original intact.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::Session;
use crate::clip::Clip;
use crate::model::*;

/// What `Session::edit_range` does with the selected range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeEdit {
    /// Split at both ends of the range.
    Split,
    /// Trim the regions to the range.
    Trim,
    /// Split, then mute the part inside the range. Unmute it to undo the silence later.
    Silence,
    /// Split, then delete the part inside the range, leaving a gap.
    Delete,
    /// Split, then set the inside part's gain so its peak reaches 0 dBFS.
    Normalize,
    /// Split, then reverse the inside part.
    Reverse,
    /// Fade in over the range: split at its start (if inside a region) and set that piece's
    /// fade-in to end where the range ends.
    FadeIn,
    /// Fade out over the range: split at its end and set the fade-out to start at its start.
    FadeOut,
}

impl RangeEdit {
    /// The undo step's name.
    pub fn label(self) -> &'static str {
        match self {
            RangeEdit::Split => "Split at Selection",
            RangeEdit::Trim => "Trim to Selection",
            RangeEdit::Silence => "Silence Selection",
            RangeEdit::Delete => "Delete Selection",
            RangeEdit::Normalize => "Normalize Selection",
            RangeEdit::Reverse => "Reverse Selection",
            RangeEdit::FadeIn => "Fade In to Selection",
            RangeEdit::FadeOut => "Fade Out to Selection",
        }
    }
}

/// What the editor's info readout shows about a clip.
#[derive(Debug, Clone, Serialize)]
pub struct ClipInfo {
    pub frames: i64,
    pub channels: usize,
    pub sample_rate: u32,
    /// As stored in the project: relative to the project folder when it's inside it.
    pub path: String,
}

/// The part of `r` between timeline positions `a` and `b` (clamped to the region), with id
/// `id`. Fades are kept only on the original's own ends.
pub(super) fn sub_region(r: &Region, a: i64, b: i64, id: Id) -> Region {
    let (rs, re) = (r.start, r.start + r.length);
    let (a, b) = (a.max(rs), b.min(re));
    let cut = a - rs;
    let length = (b - a).max(1);
    let mut out = Region { id, start: a, length, ..r.clone() };
    match &mut out.content {
        RegionContent::Audio { offset, fade_in, fade_out, fade_shape, .. } => {
            *offset += cut;
            // A cut end loses its fade, and its fade type (a speed fade or a crossfade).
            if a > rs {
                *fade_in = 0;
                fade_shape.in_type = Default::default();
            }
            if b < re {
                *fade_out = 0;
                fade_shape.out_type = Default::default();
            }
            *fade_in = (*fade_in).min(length);
            *fade_out = (*fade_out).min(length - *fade_in);
        }
        RegionContent::Midi { notes, cc } => {
            notes.retain(|n| n.start >= cut && n.start < cut + length);
            notes.iter_mut().for_each(|n| n.start -= cut);
            // Controllers start from the values in effect at the cut.
            *cc = crate::midi::cc_from(cc, cut).into_iter().filter(|c| c.start < length).collect();
        }
    }
    out
}

impl Session {
    /// A region's timeline span, start to end.
    fn region_span(&self, id: Id) -> Option<(i64, i64)> {
        let (t, i) = self.find_region(id)?;
        let r = &self.project.track(t)?.regions[i];
        Some((r.start, r.start + r.length))
    }

    /// Splits the given regions at every position that falls inside them, as one edit. Each
    /// region is replaced where it is in its track's list (so it keeps its place in what hides
    /// what) by its pieces, the first keeping its id. Unlike `split_at`, only the first piece
    /// keeps the fade-in and only the last the fade-out. Returns every piece's id, the regions
    /// not cut included.
    pub fn split_regions_at(&mut self, ids: &[Id], positions: &[i64]) -> Vec<Id> {
        let ids = &self.unloop(ids);
        let mut cuts = positions.to_vec();
        cuts.sort_unstable();
        cuts.dedup();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let mut touched = Vec::new();
        for &id in ids.iter().filter(|id| seen.insert(**id)) {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let r = self.project.track(track).unwrap().regions[idx].clone();
            let (rs, re) = (r.start, r.start + r.length);
            let mut edges = vec![rs];
            edges.extend(cuts.iter().copied().filter(|&p| p > rs && p < re));
            edges.push(re);
            if edges.len() == 2 {
                out.push(id);
                continue;
            }
            let mut pieces = Vec::with_capacity(edges.len() - 1);
            for (k, w) in edges.windows(2).enumerate() {
                let pid = if k == 0 { id } else { self.project.alloc_id() };
                pieces.push(sub_region(&r, w[0], w[1], pid));
            }
            out.extend(pieces.iter().map(|p| p.id));
            self.project.track_mut(track).unwrap().regions.splice(idx..=idx, pieces);
            if !touched.contains(&track) {
                touched.push(track);
            }
        }
        // One engine update per track, however many cuts.
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
        out
    }

    /// Splits the regions at `a` and `b` and returns the pieces inside the range.
    fn split_range(&mut self, ids: &[Id], a: i64, b: i64) -> Vec<Id> {
        let pieces = self.split_regions_at(ids, &[a, b]);
        pieces.into_iter().filter(|&id| self.region_span(id).is_some_and(|(s, e)| s >= a && e <= b)).collect()
    }

    /// Applies `action` to the parts of the given regions between timeline positions `start`
    /// and `end`. Returns the regions that now hold the range (to select them); empty for
    /// Delete, or when no region overlaps the range.
    pub fn edit_range(&mut self, ids: &[Id], start: i64, end: i64, action: RangeEdit) -> Result<Vec<Id>> {
        let ids = &self.unloop(ids);
        let (a, b) = (start.min(end).max(0), start.max(end));
        let picked: Vec<Id> = ids.iter().copied().filter(|&id| self.region_span(id).is_some_and(|(s, e)| s < b && e > a)).collect();
        if b <= a || picked.is_empty() {
            return Ok(Vec::new());
        }
        Ok(match action {
            RangeEdit::Split => self.split_range(&picked, a, b),
            RangeEdit::Trim => {
                self.edit_regions(&picked, |r, _| *r = sub_region(r, a, b, r.id));
                picked
            }
            RangeEdit::Silence => {
                let inside = self.split_range(&picked, a, b);
                self.edit_regions(&inside, |r, _| r.muted = true);
                inside
            }
            RangeEdit::Delete => {
                let inside = self.split_range(&picked, a, b);
                self.delete_regions(&inside);
                Vec::new()
            }
            RangeEdit::Normalize => {
                let inside = self.split_range(&picked, a, b);
                self.normalize_regions(&inside);
                inside
            }
            RangeEdit::Reverse => {
                let inside = self.split_range(&picked, a, b);
                self.reverse_regions(&inside)?;
                inside
            }
            RangeEdit::FadeIn => {
                let pieces = self.split_regions_at(&picked, &[a]);
                let faded: Vec<Id> = pieces.into_iter().filter(|&id| self.region_span(id).is_some_and(|(s, _)| s >= a && s < b)).collect();
                self.edit_regions(&faded, |r, _| {
                    let len = (b - r.start).min(r.length);
                    if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
                        *fade_in = len;
                        *fade_out = (*fade_out).min(r.length - len);
                    }
                });
                faded
            }
            RangeEdit::FadeOut => {
                let pieces = self.split_regions_at(&picked, &[b]);
                let faded: Vec<Id> = pieces.into_iter().filter(|&id| self.region_span(id).is_some_and(|(_, e)| e <= b && e > a)).collect();
                self.edit_regions(&faded, |r, _| {
                    let len = (r.start + r.length - a).min(r.length);
                    if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
                        *fade_out = len;
                        *fade_in = (*fade_in).min(r.length - len);
                    }
                });
                faded
            }
        })
    }

    /// The clip's audio, shared, so it can be analysed without holding the session.
    /// Range edits work on real regions: looping ones among `ids` become real copies first (as
    /// Split does). Returns `ids` plus the copies.
    fn unloop(&mut self, ids: &[Id]) -> Vec<Id> {
        let looped: Vec<Id> = ids.iter().copied().filter(|id| self.find_region(*id).is_some_and(|(t, i)| self.project.track(t).unwrap().regions[i].is_looped())).collect();
        let mut all = ids.to_vec();
        if !looped.is_empty() {
            all.extend(self.convert_loops(&looped));
        }
        all
    }

    pub fn clip_audio(&self, id: Id) -> Option<Arc<Clip>> {
        self.clips.get(&id).cloned()
    }

    pub fn clip_info(&self, id: Id) -> Option<ClipInfo> {
        let c = self.clips.get(&id)?;
        let path = self.project.clips.iter().find(|r| r.id == id).map(|r| r.path.clone()).unwrap_or_else(|| c.path.to_string_lossy().to_string());
        Some(ClipInfo { frames: c.frames() as i64, channels: c.channels.len(), sample_rate: c.sample_rate, path })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;

    /// A session on the silent ALSA null device, with one audio track holding one region of a
    /// synthetic clip (frame i has the value i / frames, so offsets are easy to check).
    fn session(frames: usize) -> (Session, Id, Id) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("fermata-range-edit-{}", std::process::id()));
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let ramp: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
        let path = s.dir.join("Audio Files").join("ramp.wav");
        let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48000, vec![ramp]));
        let id = s.project.alloc_id();
        let content = RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: 1000, fade_out: 2000, fade_shape: Default::default() };
        s.project.track_mut(t).unwrap().regions.push(Region { id, name: "Ramp".into(), start: 10_000, length: frames as i64, muted: false, color: None, content, loop_end: None, takes: None, params: Default::default(), pattern: None });
        s.push_regions(t);
        (s, t, id)
    }

    fn regions(s: &Session, t: Id) -> Vec<Region> {
        let mut v = s.project.track(t).unwrap().regions.clone();
        v.sort_by_key(|r| r.start);
        v
    }

    fn audio(r: &Region) -> (i64, i64, i64, f32) {
        match r.content {
            RegionContent::Audio { offset, fade_in, fade_out, gain_db, .. } => (offset, fade_in, fade_out, gain_db),
            _ => panic!("not audio"),
        }
    }

    #[test]
    fn split_at_many_positions_keeps_order_and_outer_fades() {
        let (mut s, t, id) = session(48_000);
        // Positions outside the region are ignored; duplicates count once.
        let pieces = s.split_regions_at(&[id, id], &[30_000, 20_000, 5_000, 20_000, 70_000]);
        let rs = regions(&s, t);
        assert_eq!(pieces.len(), 3);
        assert_eq!(rs.iter().map(|r| (r.start, r.length)).collect::<Vec<_>>(), vec![(10_000, 10_000), (20_000, 10_000), (30_000, 28_000)]);
        assert_eq!(rs[0].id, id, "the first piece keeps the id");
        assert_eq!(rs.iter().map(|r| audio(r).0).collect::<Vec<_>>(), vec![0, 10_000, 20_000]);
        assert_eq!(rs.iter().map(|r| (audio(r).1, audio(r).2)).collect::<Vec<_>>(), vec![(1000, 0), (0, 0), (0, 2000)]);
        // Nothing to cut: the region comes back unchanged.
        assert_eq!(s.split_regions_at(&[id], &[10_000, 1]), vec![id]);
    }

    #[test]
    fn silence_and_delete_cut_out_the_range() {
        let (mut s, t, id) = session(48_000);
        let inside = s.edit_range(&[id], 20_000, 30_000, RangeEdit::Silence).unwrap();
        let rs = regions(&s, t);
        assert_eq!(rs.len(), 3);
        assert_eq!(inside, vec![rs[1].id]);
        assert!(rs[1].muted && !rs[0].muted && !rs[2].muted);
        assert_eq!((rs[1].start, rs[1].length), (20_000, 10_000));

        let (mut s, t, id) = session(48_000);
        assert!(s.edit_range(&[id], 30_000, 20_000, RangeEdit::Delete).unwrap().is_empty());
        let rs = regions(&s, t);
        assert_eq!(rs.iter().map(|r| (r.start, r.start + r.length)).collect::<Vec<_>>(), vec![(10_000, 20_000), (30_000, 58_000)]);
        assert_eq!(audio(&rs[1]).0, 20_000, "the right part still plays the same audio");
    }

    #[test]
    fn trim_and_split_at_selection() {
        let (mut s, t, id) = session(48_000);
        assert_eq!(s.edit_range(&[id], 15_000, 25_000, RangeEdit::Trim).unwrap(), vec![id]);
        let rs = regions(&s, t);
        assert_eq!((rs.len(), rs[0].start, rs[0].length, audio(&rs[0]).0), (1, 15_000, 10_000, 5_000));
        assert_eq!((audio(&rs[0]).1, audio(&rs[0]).2), (0, 0), "trimmed-off ends lose their fades");

        let (mut s, t, id) = session(48_000);
        // A range over the start: only one cut falls inside.
        let inside = s.edit_range(&[id], 0, 12_000, RangeEdit::Split).unwrap();
        let rs = regions(&s, t);
        assert_eq!(rs.len(), 2);
        assert_eq!(inside, vec![id]);
        assert_eq!((rs[0].start, rs[0].length), (10_000, 2_000));
    }

    #[test]
    fn fades_to_selection() {
        let (mut s, t, id) = session(48_000);
        // From the region start: just a longer fade-in, no split.
        s.edit_range(&[id], 10_000, 16_000, RangeEdit::FadeIn).unwrap();
        let rs = regions(&s, t);
        assert_eq!((rs.len(), audio(&rs[0]).1), (1, 6_000));
        // From inside: split at the start and fade the rest in over the range.
        let faded = s.edit_range(&[id], 30_000, 34_000, RangeEdit::FadeIn).unwrap();
        let rs = regions(&s, t);
        assert_eq!(rs.len(), 2);
        assert_eq!(faded, vec![rs[1].id]);
        assert_eq!((rs[1].start, audio(&rs[1]).1, audio(&rs[1]).2), (30_000, 4_000, 2_000));
        // Fade out over a range in the middle of the last piece.
        let faded = s.edit_range(&[rs[1].id], 40_000, 50_000, RangeEdit::FadeOut).unwrap();
        let rs = regions(&s, t);
        assert_eq!(rs.len(), 3);
        assert_eq!(faded, vec![rs[1].id]);
        assert_eq!((rs[1].start + rs[1].length, audio(&rs[1]).2, audio(&rs[1]).1), (50_000, 10_000, 4_000));
        assert_eq!(audio(&rs[2]).2, 2_000, "the original fade-out stays on the last piece");
    }

    #[test]
    fn normalize_selection_uses_the_peak_inside() {
        let (mut s, t, id) = session(48_000);
        // The ramp peaks at the range's end: (30_000 - 10_000) / 48_000.
        let inside = s.edit_range(&[id], 20_000, 30_000, RangeEdit::Normalize).unwrap();
        let rs = regions(&s, t);
        let r = rs.iter().find(|r| r.id == inside[0]).unwrap();
        let want = -20.0 * (19_999.0f32 / 48_000.0).log10();
        assert!((audio(r).3 - want).abs() < 0.01, "{} vs {want}", audio(r).3);
        assert_eq!(audio(&rs[0]).3, 0.0);
    }

    #[test]
    fn reverse_selection_writes_only_the_inside() {
        let (mut s, t, id) = session(4_800);
        let inside = s.edit_range(&[id], 11_000, 12_000, RangeEdit::Reverse).unwrap();
        let rs = regions(&s, t);
        assert_eq!(rs.len(), 3);
        let RegionContent::Audio { clip, offset, .. } = rs[1].content else { panic!() };
        assert_eq!((rs[1].id, offset), (inside[0], 0));
        let c = s.clip_audio(clip).unwrap();
        assert_eq!(c.frames(), 1_000);
        // Reversed: starts with the ramp value at the range's end.
        assert!((c.channels[0][0] - 1_999.0 / 4_800.0).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn ranges_outside_every_region_change_nothing() {
        let (mut s, t, id) = session(48_000);
        assert!(s.edit_range(&[id], 0, 5_000, RangeEdit::Delete).unwrap().is_empty());
        assert!(s.edit_range(&[id], 20_000, 20_000, RangeEdit::Silence).unwrap().is_empty());
        assert_eq!(regions(&s, t).len(), 1);
    }
}
