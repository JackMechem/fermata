//! Logic's arranging commands on the Tracks area: Edit › Section Between Locators (Insert
//! Silence, Cut Section, Repeat Section, Insert Cut Section), the Trim commands, Remove Gaps,
//! Delete and Shuffle, Paste Replace, Move Region to Playhead, Strip Silence, and naming and
//! colouring regions and tracks after each other. The arithmetic is in `crate::arrange_cmds`
//! and `crate::strip_silence`.
//!
//! Section edits move time the way the Arrangement track does (`session/arrangement.rs`):
//! regions across an edge are cut there, and automation, markers and arrangement sections go
//! with their time. The tempo and signature tracks stay as they are. With a marquee they act
//! on its tracks only, and the global tracks and Stereo Out's automation stay put.

use anyhow::{anyhow, Result};

use super::arrangement::{remap_regions, untouched};
use super::range_edit::sub_region;
use super::*;
use crate::arrange_cmds::{butt_together, end_at_next, overlap_trims, remap_sections, section_pieces, SectionEdit, Span};
use crate::arrangement::{map_pos, remap_points, Piece};
use crate::drag_modes::{shuffle, Side};
use crate::strip_silence::{sounding_parts, StripSettings};

/// What Cut Section Between Locators took out, laid out from 0, for Insert Cut Section.
#[derive(Clone)]
pub(super) struct SectionClip {
    length: i64,
    /// Per track: its regions and its automation in the section.
    tracks: Vec<(Id, Vec<Region>, Vec<AutoLane>)>,
    /// Stereo Out's automation and the markers, when the cut was across all tracks.
    master: Vec<AutoLane>,
    markers: Vec<Marker>,
    global: bool,
}

/// Every lane after the time map; true if any changed.
fn remap_lanes(lanes: &mut [AutoLane], pieces: &[Piece]) -> bool {
    let mut changed = false;
    for lane in lanes {
        let points = remap_points(&lane.points, pieces);
        if points != lane.points {
            lane.points = points;
            changed = true;
        }
    }
    changed
}

/// The lanes' automation between `a` and `b`, moved to start at 0 (empty lanes left out).
fn copy_lanes(lanes: &[AutoLane], a: i64, b: i64) -> Vec<AutoLane> {
    let piece = [Piece { start: a, end: b, dst: 0, copy: false }];
    lanes.iter().filter(|l| !l.points.is_empty()).map(|l| AutoLane { param: l.param.clone(), points: remap_points(&l.points, &piece) }).collect()
}

/// Puts `clip`'s lanes into `lanes` at `at`, over the stretch `at..at + len` that a silence
/// was just inserted into. Only lanes that are automated there already take the points:
/// adding a lane would change what plays outside the section.
fn paste_lanes(lanes: &mut [AutoLane], clip: &[AutoLane], at: i64, len: i64) -> bool {
    let mut changed = false;
    for c in clip {
        let Some(lane) = lanes.iter_mut().find(|l| l.param.same(&c.param) && !l.points.is_empty()) else { continue };
        let mut points: Vec<AutoPoint> = lane.points.iter().filter(|p| p.pos < at).copied().collect();
        points.extend(c.points.iter().map(|p| AutoPoint { pos: p.pos + at, ..*p }));
        points.extend(lane.points.iter().filter(|p| p.pos >= at + len).copied());
        lane.points = points;
        changed = true;
    }
    changed
}

/// A region as the drag-mode rules see it.
fn span(r: &Region) -> Span {
    Span { id: r.id, start: r.start, len: r.end() - r.start }
}

/// Shortens a region to sound for `len` samples: its loop first, then the region itself.
fn shorten(r: &mut Region, len: i64) {
    let end = r.start + len;
    if r.is_looped() && end > r.start + r.length {
        r.loop_end = Some(end);
        return;
    }
    r.loop_end = None;
    r.length = len.max(1);
    if let RegionContent::Audio { fade_in, fade_out, .. } = &mut r.content {
        *fade_out = (*fade_out).min(r.length);
        *fade_in = (*fade_in).min(r.length - *fade_out);
    }
}

impl Session {
    /// The selected regions grouped by track, in track order.
    fn by_track(&self, ids: &[Id]) -> Vec<(Id, Vec<Region>)> {
        self.project
            .tracks
            .iter()
            .map(|t| (t.id, t.regions.iter().filter(|r| ids.contains(&r.id)).cloned().collect::<Vec<_>>()))
            .filter(|(_, v)| !v.is_empty())
            .collect()
    }

    /// Moves regions of `track` to the given starts (loop ends along).
    fn set_starts(&mut self, track: Id, starts: &[(Id, i64)]) {
        let Some(t) = self.project.track_mut(track) else { return };
        for r in &mut t.regions {
            if let Some(&(_, s)) = starts.iter().find(|(id, _)| *id == r.id) {
                r.move_to(s.max(0));
            }
        }
        self.push_regions(track);
        self.dirty = true;
    }

    /// Makes a region end at `end` (start fixed): a loop runs to it when it reaches past the
    /// region itself; otherwise the region is trimmed or lengthened, audio no further than its
    /// file goes.
    fn set_region_end(&mut self, id: Id, end: i64) {
        let Some(r) = self.region(id).cloned() else { return };
        if end <= r.start || end == r.end() {
            return;
        }
        if r.is_looped() && end > r.start + r.length {
            self.set_region_loop(id, Some(end));
            return;
        }
        if r.is_looped() {
            self.set_region_loop(id, None);
        }
        self.resize_region(id, r.start, end - r.start);
    }

    /// The regions on `scope`'s tracks (every track for None) that lie within `a..b`.
    fn regions_within(&self, a: i64, b: i64, scope: Option<&[Id]>) -> Vec<Id> {
        self.project.tracks.iter().filter(|t| scope.is_none_or(|s| s.contains(&t.id))).flat_map(|t| t.regions.iter()).filter(|r| r.start >= a && r.end() <= b).map(|r| r.id).collect()
    }

    /// Applies a time map to the regions and automation of `scope`'s tracks, or with None to
    /// every track, Stereo Out's automation, the markers and the arrangement sections. Refused
    /// (with nothing changed) when it would cut or move a frozen track's regions.
    fn apply_time_map(&mut self, pieces: &[Piece], scope: Option<&[Id]>) -> Result<()> {
        let in_scope = |id: Id| scope.is_none_or(|s| s.contains(&id));
        let touched: Vec<Id> = self.project.tracks.iter().filter(|t| in_scope(t.id) && t.regions.iter().any(|r| !untouched(r, pieces))).map(|t| t.id).collect();
        self.check_unfrozen(&touched)?;
        let mut next = self.project.next_id;
        let mut alloc = || {
            next += 1;
            next - 1
        };
        let mut lanes_changed = Vec::new();
        for t in self.project.tracks.iter_mut().filter(|t| in_scope(t.id)) {
            if touched.contains(&t.id) {
                t.regions = remap_regions(&t.regions, pieces, &mut alloc);
            }
            if remap_lanes(&mut t.automation, pieces) {
                lanes_changed.push(t.id);
            }
        }
        if scope.is_none() {
            if remap_lanes(&mut self.project.master.automation, pieces) {
                lanes_changed.push(MASTER_ID);
            }
            let mut markers = Vec::new();
            for m in &self.project.markers {
                for (k, pos) in map_pos(pieces, m.pos).into_iter().enumerate() {
                    markers.push(Marker { id: if k == 0 { m.id } else { alloc() }, pos, ..m.clone() });
                }
            }
            markers.sort_by_key(|m| (m.pos, m.id));
            self.project.markers = markers;
            self.project.arrangement = remap_sections(&self.project.arrangement, pieces);
        }
        self.project.next_id = next;
        for t in touched {
            self.push_regions(t);
        }
        for t in lanes_changed {
            self.push_automation(t);
        }
        self.dirty = true;
        Ok(())
    }

    /// What lies between `a` and `b` on `scope`'s tracks (every track for None), for Insert
    /// Cut Section.
    fn copy_section(&self, a: i64, b: i64, scope: Option<&[Id]>) -> SectionClip {
        let mut tracks = Vec::new();
        for t in self.project.tracks.iter().filter(|t| scope.is_none_or(|s| s.contains(&t.id))) {
            let mut regions = Vec::new();
            for r in &t.regions {
                let passes = if r.is_looped() { r.real_copies() } else { vec![r.clone()] };
                for p in passes.iter().filter(|p| p.start < b && p.start + p.length > a) {
                    let mut m = sub_region(p, a, b, p.id);
                    m.start -= a;
                    regions.push(m);
                }
            }
            tracks.push((t.id, regions, copy_lanes(&t.automation, a, b)));
        }
        let global = scope.is_none();
        SectionClip {
            length: b - a,
            tracks,
            master: if global { copy_lanes(&self.project.master.automation, a, b) } else { Vec::new() },
            markers: if global { self.project.markers.iter().filter(|m| m.pos >= a && m.pos < b).map(|m| Marker { pos: m.pos - a, ..m.clone() }).collect() } else { Vec::new() },
            global,
        }
    }

    /// Edit › Section Between Locators: inserts silence between `start` and `end`, cuts that
    /// section out (keeping it for Insert Cut Section), or repeats it after itself, on the
    /// tracks of `scope` (a marquee's) or across the whole project. Returns the regions to
    /// select: the repeated copy's.
    pub fn section_edit(&mut self, edit: SectionEdit, start: i64, end: i64, scope: Option<&[Id]>) -> Result<Vec<Id>> {
        let (a, b) = (start.min(end).max(0), start.max(end));
        if b <= a {
            return Err(anyhow!("Set the locators around a section first."));
        }
        let clip = (edit == SectionEdit::Cut).then(|| self.copy_section(a, b, scope));
        self.apply_time_map(&section_pieces(edit, a, b), scope)?;
        if clip.is_some() {
            self.section_clip = clip;
        }
        Ok(if edit == SectionEdit::Repeat { self.regions_within(b, 2 * b - a, scope) } else { Vec::new() })
    }

    /// Insert Cut Section (Logic's Splice): makes room at `at` and puts the section that Cut
    /// Section Between Locators took out there, on the tracks it came from. Returns its
    /// regions.
    pub fn insert_cut_section(&mut self, at: i64) -> Result<Vec<Id>> {
        let clip = self.section_clip.clone().ok_or_else(|| anyhow!("Cut a section between the locators first."))?;
        let at = at.max(0);
        let len = clip.length;
        let receiving: Vec<Id> = clip.tracks.iter().filter(|(_, r, _)| !r.is_empty()).map(|(t, _, _)| *t).collect();
        self.check_unfrozen(&receiving)?;
        let scope: Option<Vec<Id>> = (!clip.global).then(|| clip.tracks.iter().map(|(t, _, _)| *t).collect());
        self.apply_time_map(&section_pieces(SectionEdit::InsertSilence, at, at + len), scope.as_deref())?;
        let mut made = Vec::new();
        for (track, regions, lanes) in &clip.tracks {
            let mut new = Vec::with_capacity(regions.len());
            // The clip outlives the project it was cut from: only what still fits goes in.
            let kind = self.project.track(*track).map(|t| t.kind);
            let fits = |r: &&Region| match (kind, &r.content) {
                (Some(TrackKind::Audio), RegionContent::Audio { clip, .. }) => self.clips.contains_key(clip),
                (Some(TrackKind::Instrument), RegionContent::Midi { .. }) => true,
                _ => false,
            };
            let fitting: Vec<Region> = regions.iter().filter(fits).cloned().collect();
            for r in &fitting {
                let id = self.project.alloc_id();
                new.push(Region { id, start: r.start + at, ..r.clone() });
                made.push(id);
            }
            let Some(t) = self.project.track_mut(*track) else { continue };
            t.regions.extend(new);
            let lanes_changed = paste_lanes(&mut t.automation, lanes, at, len);
            if !fitting.is_empty() {
                self.push_regions(*track);
            }
            if lanes_changed {
                self.push_automation(*track);
            }
        }
        if clip.global {
            if paste_lanes(&mut self.project.master.automation, &clip.master, at, len) {
                self.push_automation(MASTER_ID);
            }
            for m in &clip.markers {
                let id = self.project.alloc_id();
                self.project.markers.push(Marker { id, pos: m.pos + at, ..m.clone() });
            }
            self.project.markers.sort_by_key(|m| (m.pos, m.id));
        }
        self.dirty = true;
        Ok(made)
    }

    /// Remove Gaps: the selected regions on each track are butted together in time order,
    /// from where the first one is.
    pub fn remove_gaps(&mut self, ids: &[Id]) {
        for (track, regions) in self.by_track(ids) {
            let starts = butt_together(&regions.iter().map(span).collect::<Vec<_>>());
            if !starts.is_empty() {
                self.set_starts(track, &starts);
            }
        }
    }

    /// Trim › Remove Overlaps: of the selected regions on each track, one still sounding when
    /// the next starts is shortened to end there.
    pub fn remove_overlaps(&mut self, ids: &[Id]) {
        for (track, regions) in self.by_track(ids) {
            let cuts = overlap_trims(&regions.iter().map(span).collect::<Vec<_>>());
            if cuts.is_empty() {
                continue;
            }
            let t = self.project.track_mut(track).unwrap();
            for r in &mut t.regions {
                if let Some(&(_, len)) = cuts.iter().find(|(id, _)| *id == r.id) {
                    shorten(r, len);
                }
            }
            self.push_regions(track);
            self.dirty = true;
        }
    }

    /// The starts of the other regions on `id`'s track.
    fn other_starts(&self, id: Id) -> Vec<i64> {
        let Some((track, _)) = self.find_region(id) else { return Vec::new() };
        self.project.track(track).unwrap().regions.iter().filter(|r| r.id != id).map(|r| r.start).collect()
    }

    /// Trim › Trim Region End to Next Region: each region ends where the next region on its
    /// track starts (shorter or longer). Regions with nothing after them stay.
    pub fn trim_to_next_region(&mut self, ids: &[Id]) {
        for &id in ids {
            let Some(start) = self.region(id).map(|r| r.start) else { continue };
            if let Some(end) = end_at_next(start, &self.other_starts(id), None) {
                self.set_region_end(id, end);
            }
        }
    }

    /// Trim › Fill Within Locators: each region starting between `start` and `end` is
    /// lengthened up to the next region on its track, or to `end` when that comes first.
    pub fn fill_within_locators(&mut self, ids: &[Id], start: i64, end: i64) {
        let (a, b) = (start.min(end), start.max(end));
        for &id in ids {
            let Some(r) = self.region(id) else { continue };
            let (rs, re) = (r.start, r.end());
            if rs < a || rs >= b {
                continue;
            }
            if let Some(e) = end_at_next(rs, &self.other_starts(id), Some(b)).filter(|&e| e > re) {
                self.set_region_end(id, e);
            }
        }
    }

    /// Resize to Same Length: every region takes the length of the first one in `ids`.
    pub fn resize_to_same_length(&mut self, ids: &[Id]) {
        let Some(len) = ids.first().and_then(|&id| self.region(id)).map(|r| r.length) else { return };
        for &id in &ids[1..] {
            let Some(start) = self.region(id).map(|r| r.start) else { continue };
            self.resize_region(id, start, len);
        }
    }

    /// Trim Region Start (`end` false) or End to Playhead: the regions `pos` can trim (it lies
    /// before the end, or after the start) start or end there.
    pub fn trim_regions_to(&mut self, ids: &[Id], pos: i64, end: bool) {
        for &id in ids {
            let Some(r) = self.region(id).cloned() else { continue };
            if end && pos > r.start {
                self.set_region_end(id, pos);
            } else if !end && pos < r.start + r.length && pos != r.start {
                self.resize_region(id, pos.max(0), r.start + r.length - pos.max(0));
            }
        }
    }

    /// Crop Regions Outside Locators (or the marquee): the regions are trimmed to `start..end`
    /// and those wholly outside it deleted.
    pub fn crop_regions(&mut self, ids: &[Id], start: i64, end: i64) -> Result<Vec<Id>> {
        let (a, b) = (start.min(end), start.max(end));
        if b <= a {
            return Err(anyhow!("Set the locators around what to keep first."));
        }
        let ids = self.unloop(ids);
        let outside: Vec<Id> = ids.iter().copied().filter(|&id| self.region(id).is_some_and(|r| r.end() <= a || r.start >= b)).collect();
        self.delete_regions(&outside);
        let inside: Vec<Id> = ids.into_iter().filter(|id| !outside.contains(id)).collect();
        self.edit_range(&inside, a, b, RangeEdit::Trim)
    }

    /// Delete and Shuffle: deletes the regions, and the regions after each one on its track
    /// move left to close the gap (as Shuffle L does, whatever the drag mode).
    pub fn delete_and_shuffle(&mut self, ids: &[Id]) {
        let lanes: Vec<(Id, Vec<Span>, Vec<Id>)> = self
            .project
            .tracks
            .iter()
            .filter(|t| t.regions.iter().any(|r| ids.contains(&r.id)))
            .map(|t| (t.id, t.regions.iter().map(span).collect(), t.regions.iter().map(|r| r.id).filter(|id| ids.contains(id)).collect()))
            .collect();
        self.delete_regions(ids);
        for (track, before, leaving) in lanes {
            let starts = shuffle(&before, &leaving, &[], Side::Left);
            if !starts.is_empty() {
                self.set_starts(track, &starts);
            }
        }
    }

    /// Paste Replace: pastes as `paste_regions` does, after deleting what lies under the
    /// pasted range on each track that receives regions (cut at its edges).
    pub fn paste_replace(&mut self, items: &[(Id, Region)], offset: i64) -> Vec<Id> {
        let mut spans: Vec<(Id, i64, i64)> = Vec::new();
        for (track, r) in items {
            let fits = match (self.project.track(*track).map(|t| t.kind), &r.content) {
                (Some(TrackKind::Audio), RegionContent::Audio { clip, .. }) => self.clips.contains_key(clip),
                (Some(TrackKind::Instrument), RegionContent::Midi { .. }) => true,
                _ => false,
            };
            if !fits {
                continue;
            }
            let (a, b) = ((r.start + offset).max(0), (r.start + offset).max(0) + r.end() - r.start);
            match spans.iter_mut().find(|s| s.0 == *track) {
                Some(s) => (s.1, s.2) = (s.1.min(a), s.2.max(b)),
                None => spans.push((*track, a, b)),
            }
        }
        for (track, a, b) in spans {
            let under = self.marquee_regions(&[track], a, b);
            let _ = self.edit_range(&under, a, b, RangeEdit::Delete);
        }
        self.paste_regions(items, offset)
    }

    /// Move Region to Playhead (Logic's Pickup Clock): the regions move together so the first
    /// one starts at `pos`, in the drag mode and with Move Automation with Regions.
    pub fn move_regions_to(&mut self, ids: &[Id], pos: i64) {
        let Some(first) = ids.iter().filter_map(|&id| self.region(id)).map(|r| r.start).min() else { return };
        if pos.max(0) != first {
            self.drag_nudge_regions(ids, pos.max(0) - first, None);
        }
    }

    /// The parts of an audio region that Strip Silence would keep, relative to its start.
    /// None for MIDI regions and take folders.
    fn strip_parts(&self, r: &Region, s: &StripSettings) -> Option<Vec<(i64, i64)>> {
        let RegionContent::Audio { clip, offset, gain_db, .. } = &r.content else { return None };
        if r.takes.is_some() {
            return None;
        }
        let c = self.clips.get(clip)?;
        let from = (*offset).max(0) as usize;
        Some(sounding_parts(&c.channels, from, from + r.length.max(0) as usize, 10f32.powf(gain_db / 20.0), c.sample_rate as f64, s))
    }

    /// Where Strip Silence would cut region `id`: the timeline ranges it keeps.
    pub fn strip_silence_preview(&self, id: Id, s: &StripSettings) -> Vec<(i64, i64)> {
        let Some(r) = self.region(id) else { return Vec::new() };
        self.strip_parts(r, s).unwrap_or_default().into_iter().map(|(a, b)| (r.start + a, r.start + b)).collect()
    }

    /// Strip Silence: each audio region becomes one region per part that sounds, the silence
    /// between them left out (a region that is silence throughout goes). The first part keeps
    /// the region's id. Returns the parts.
    pub fn strip_silence(&mut self, ids: &[Id], s: &StripSettings) -> Vec<Id> {
        let ids = self.unloop(ids);
        let mut made = Vec::new();
        let mut touched = Vec::new();
        for id in ids {
            let Some((track, idx)) = self.find_region(id) else { continue };
            let r = self.project.track(track).unwrap().regions[idx].clone();
            let Some(parts) = self.strip_parts(&r, s) else { continue };
            if parts == [(0, r.length)] {
                made.push(id);
                continue;
            }
            let mut pieces = Vec::with_capacity(parts.len());
            for (k, (a, b)) in parts.into_iter().enumerate() {
                let pid = if k == 0 { id } else { self.project.alloc_id() };
                pieces.push(sub_region(&r, r.start + a, r.start + b, pid));
            }
            made.extend(pieces.iter().map(|p| p.id));
            self.project.track_mut(track).unwrap().regions.splice(idx..=idx, pieces);
            if !touched.contains(&track) {
                touched.push(track);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
        made
    }

    /// Name Regions by Track: each region takes its track's name.
    pub fn name_regions_by_track(&mut self, ids: &[Id]) {
        for t in &mut self.project.tracks {
            let name = t.name.clone();
            t.regions.iter_mut().filter(|r| ids.contains(&r.id)).for_each(|r| r.name = name.clone());
        }
        self.dirty = true;
    }

    /// Color Regions by Track: the regions drop their own colour and show their track's.
    pub fn color_regions_by_track(&mut self, ids: &[Id]) {
        self.set_region_color(ids, None);
    }

    /// The first (earliest) of the selected regions on each track.
    fn first_per_track(&self, ids: &[Id]) -> Vec<(Id, Region)> {
        self.by_track(ids).into_iter().filter_map(|(t, v)| v.into_iter().min_by_key(|r| (r.start, r.id)).map(|r| (t, r))).collect()
    }

    /// Name Tracks by Region: each track takes the name of its first selected region.
    pub fn name_tracks_by_region(&mut self, ids: &[Id]) {
        for (track, r) in self.first_per_track(ids) {
            if let Some(t) = self.project.track_mut(track) {
                t.name = r.name;
            }
        }
        self.dirty = true;
    }

    /// Color Tracks by Region: each track takes the colour of its first selected region, if
    /// that has its own.
    pub fn color_tracks_by_region(&mut self, ids: &[Id]) {
        for (track, r) in self.first_per_track(ids) {
            if let (Some(c), Some(t)) = (r.color, self.project.track_mut(track)) {
                t.color = c;
            }
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::automation::value_at;

    /// A session on the silent ALSA null device with two audio tracks and a 200 000-frame
    /// clip: a 100 Hz tone with silence at 50 000..80 000 and from 150 000 on.
    fn session() -> (Session, Vec<Id>, Id) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("fermata-arrange-cmds-{}", std::process::id()));
        let path = s.dir.join("Audio Files").join("tone.wav");
        let tone: Vec<f32> = (0..200_000).map(|i| if (50_000..80_000).contains(&i) || i >= 150_000 { 0.0 } else { 0.5 * (i as f32 * std::f32::consts::TAU * 100.0 / 48_000.0).sin() }).collect();
        let clip = s.add_clip(&path, Clip::from_channels(path.clone(), 48_000, vec![tone]));
        let tracks: Vec<Id> = (0..2).map(|_| s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()).collect();
        (s, tracks, clip)
    }

    fn region(s: &mut Session, t: Id, clip: Id, start: i64, length: i64, offset: i64) -> Id {
        let id = s.project.alloc_id();
        let content = RegionContent::Audio { clip, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
        s.project.track_mut(t).unwrap().regions.push(Region { id, name: "R".into(), start, length, muted: false, color: None, content, loop_end: None, takes: None, params: Default::default(), pattern: None });
        s.push_regions(t);
        id
    }

    /// (start, end, clip offset) of a track's regions, by start.
    fn spans(s: &Session, t: Id) -> Vec<(i64, i64, i64)> {
        let mut v: Vec<(i64, i64, i64)> = s
            .project
            .track(t)
            .unwrap()
            .regions
            .iter()
            .map(|r| match r.content {
                RegionContent::Audio { offset, .. } => (r.start, r.end(), offset),
                _ => (r.start, r.end(), 0),
            })
            .collect();
        v.sort();
        v
    }

    fn pt(pos: i64, value: f32) -> AutoPoint {
        AutoPoint { pos, value, curve: 0.0 }
    }

    #[test]
    fn insert_silence_cut_and_repeat_move_everything_with_its_time() {
        let (mut s, t, clip) = session();
        region(&mut s, t[0], clip, 1000, 4000, 0);
        region(&mut s, t[1], clip, 6000, 1000, 0);
        s.set_automation_lane(t[0], AutoParam::Volume, vec![pt(0, -10.0), pt(4000, 0.0)]);
        s.set_automation_lane(MASTER_ID, AutoParam::Volume, vec![pt(2000, -6.0), pt(3000, 0.0)]);
        s.add_marker(2500, Some("Hit".into()));
        s.add_marker(6000, Some("Late".into()));
        let sec = s.add_arrangement_marker(None);
        s.resize_arrangement_marker(sec, 5000);

        // Silence at 2000..3000: the region is cut at 2000 and its tail moves on.
        s.section_edit(SectionEdit::InsertSilence, 2000, 3000, None).unwrap();
        assert_eq!(spans(&s, t[0]), vec![(1000, 2000, 0), (3000, 6000, 1000)]);
        assert_eq!(spans(&s, t[1]), vec![(7000, 8000, 0)]);
        assert_eq!(s.project.markers.iter().map(|m| m.pos).collect::<Vec<_>>(), vec![3500, 7000]);
        assert_eq!((s.project.arrangement[0].start, s.project.arrangement[0].length), (0, 6000), "the section takes in the silence");
        let vol = &s.project.track(t[0]).unwrap().automation[0].points;
        assert!((value_at(vol, 1999).unwrap() - -5.0).abs() < 0.01 && (value_at(vol, 3000).unwrap() - -5.0).abs() < 0.01, "{vol:?}");
        let master = &s.project.master.automation[0].points;
        assert!((value_at(master, 2500).unwrap() - -6.0).abs() < 0.01 && (value_at(master, 3500).unwrap() - -3.0).abs() < 0.01, "{master:?}");

        // Cut 1500..3500 back out: the silence and 500 frames each side go.
        s.section_edit(SectionEdit::Cut, 1500, 3500, None).unwrap();
        assert_eq!(spans(&s, t[0]), vec![(1000, 1500, 0), (1500, 4000, 1500)]);
        assert_eq!(spans(&s, t[1]), vec![(5000, 6000, 0)]);
        assert_eq!(s.project.markers.iter().map(|m| m.pos).collect::<Vec<_>>(), vec![1500, 5000]);

        // Repeat 1000..2000 (two regions): copies land at 2000..3000, the rest moves on.
        let made = s.section_edit(SectionEdit::Repeat, 1000, 2000, None).unwrap();
        assert_eq!(spans(&s, t[0]), vec![(1000, 1500, 0), (1500, 2000, 1500), (2000, 2500, 0), (2500, 3000, 1500), (3000, 5000, 2000)]);
        assert_eq!(made.len(), 2);
        assert_eq!(s.project.markers.iter().map(|m| m.pos).collect::<Vec<_>>(), vec![1500, 2500, 6000], "the marker in the section is repeated");
    }

    #[test]
    fn a_marquee_limits_a_section_edit_to_its_tracks() {
        let (mut s, t, clip) = session();
        region(&mut s, t[0], clip, 1000, 1000, 0);
        region(&mut s, t[1], clip, 1000, 1000, 0);
        s.add_marker(1500, None);
        s.section_edit(SectionEdit::InsertSilence, 500, 700, Some(&[t[1]])).unwrap();
        assert_eq!(spans(&s, t[0]), vec![(1000, 2000, 0)]);
        assert_eq!(spans(&s, t[1]), vec![(1200, 2200, 0)]);
        assert_eq!(s.project.markers[0].pos, 1500, "the global tracks stay");
        assert!(s.section_edit(SectionEdit::Cut, 700, 700, None).is_err());
    }

    #[test]
    fn insert_cut_section_puts_the_cut_back_elsewhere() {
        let (mut s, t, clip) = session();
        region(&mut s, t[0], clip, 1000, 2000, 0);
        s.set_automation_lane(t[0], AutoParam::Volume, vec![pt(1000, -20.0), pt(2000, 0.0), pt(10_000, 0.0)]);
        s.add_marker(1200, Some("In".into()));
        assert!(s.insert_cut_section(0).is_err(), "nothing cut yet");
        s.section_edit(SectionEdit::Cut, 1000, 1500, None).unwrap();
        assert_eq!(spans(&s, t[0]), vec![(1000, 2500, 500)]);
        // Paste it at 2000: room is made there, and the cut part comes back.
        let made = s.insert_cut_section(2000).unwrap();
        assert_eq!(spans(&s, t[0]), vec![(1000, 2000, 500), (2000, 2500, 0), (2500, 3000, 1500)]);
        assert_eq!(made.len(), 1);
        assert_eq!(s.project.markers.iter().map(|m| (m.pos, m.name.as_str())).collect::<Vec<_>>(), vec![(2200, "In")]);
        let vol = &s.project.track(t[0]).unwrap().automation[0].points;
        assert!((value_at(vol, 2000).unwrap() - -20.0).abs() < 0.05 && (value_at(vol, 2250).unwrap() - -15.0).abs() < 0.05, "{vol:?}");
    }

    #[test]
    fn gaps_overlaps_and_trims() {
        let (mut s, t, clip) = session();
        let a = region(&mut s, t[0], clip, 0, 1000, 0);
        let b = region(&mut s, t[0], clip, 3000, 1000, 0);
        let c = region(&mut s, t[0], clip, 6000, 1000, 0);
        s.remove_gaps(&[a, b, c]);
        assert_eq!(spans(&s, t[0]), vec![(0, 1000, 0), (1000, 2000, 0), (2000, 3000, 0)]);
        // Overlaps: b moved back over a.
        s.move_region(b, 500, None);
        s.remove_overlaps(&[a, b]);
        assert_eq!(spans(&s, t[0]), vec![(0, 500, 0), (500, 1500, 0), (2000, 3000, 0)]);
        // b's end to the next region (lengthened), then a's start and end to a playhead.
        s.trim_to_next_region(&[b]);
        assert_eq!(spans(&s, t[0])[1], (500, 2000, 0));
        s.trim_regions_to(&[a], 200, false);
        s.trim_regions_to(&[c], 2600, true);
        assert_eq!(spans(&s, t[0]), vec![(200, 500, 200), (500, 2000, 0), (2000, 2600, 0)]);
        // Same length as the first: c grows to b's 1500.
        s.resize_to_same_length(&[b, c]);
        assert_eq!(spans(&s, t[0])[2], (2000, 3500, 0));
    }

    #[test]
    fn fill_within_locators_lengthens_up_to_the_next_region_or_the_right_locator() {
        let (mut s, t, clip) = session();
        let a = region(&mut s, t[0], clip, 1000, 500, 0);
        let b = region(&mut s, t[0], clip, 3000, 500, 0);
        let out = region(&mut s, t[0], clip, 9000, 100, 0);
        s.fill_within_locators(&[a, b, out], 1000, 5000);
        assert_eq!(spans(&s, t[0]), vec![(1000, 3000, 0), (3000, 5000, 0), (9000, 9100, 0)]);
    }

    #[test]
    fn crop_and_delete_and_shuffle() {
        let (mut s, t, clip) = session();
        let a = region(&mut s, t[0], clip, 0, 1000, 0);
        let b = region(&mut s, t[0], clip, 1000, 1000, 0);
        let c = region(&mut s, t[0], clip, 5000, 1000, 0);
        s.crop_regions(&[a, b, c], 500, 1500).unwrap();
        assert_eq!(spans(&s, t[0]), vec![(500, 1000, 500), (1000, 1500, 0)]);
        let d = region(&mut s, t[0], clip, 1500, 500, 0);
        let e = region(&mut s, t[0], clip, 3000, 500, 0);
        s.delete_and_shuffle(&[d]);
        assert_eq!(spans(&s, t[0]), vec![(500, 1000, 500), (1000, 1500, 0), (2500, 3000, 0)]);
        assert!(s.region(e).is_some());
    }

    #[test]
    fn paste_replace_clears_the_pasted_range_first() {
        let (mut s, t, clip) = session();
        region(&mut s, t[0], clip, 0, 10_000, 0);
        let item = s.project.track(t[0]).unwrap().regions[0].clone();
        let small = Region { start: 0, length: 2000, ..item };
        let made = s.paste_replace(&[(t[1], small.clone()), (t[0], small)], 4000);
        assert_eq!(made.len(), 2);
        assert_eq!(spans(&s, t[0]), vec![(0, 4000, 0), (4000, 6000, 0), (6000, 10_000, 6000)]);
        assert_eq!(spans(&s, t[1]), vec![(4000, 6000, 0)]);
        assert_eq!(s.project.track(t[0]).unwrap().regions.len(), 3, "the part under the paste went");
    }

    #[test]
    fn move_to_playhead_keeps_the_spacing() {
        let (mut s, t, clip) = session();
        let a = region(&mut s, t[0], clip, 1000, 100, 0);
        let b = region(&mut s, t[1], clip, 1500, 100, 0);
        s.move_regions_to(&[a, b], 4000);
        assert_eq!((spans(&s, t[0]), spans(&s, t[1])), (vec![(4000, 4100, 0)], vec![(4500, 4600, 0)]));
    }

    #[test]
    fn strip_silence_cuts_out_the_quiet_parts() {
        let (mut s, t, clip) = session();
        let id = region(&mut s, t[0], clip, 10_000, 200_000, 0);
        let set = StripSettings { threshold: 0.04, min_time: 0.1, pre_attack: 0.0, post_release: 0.0 };
        let preview = s.strip_silence_preview(id, &set);
        assert_eq!(preview.len(), 2, "{preview:?}");
        let made = s.strip_silence(&[id], &set);
        assert_eq!(made.len(), 2);
        assert_eq!(made[0], id, "the first part keeps the id");
        let got = spans(&s, t[0]);
        assert_eq!(got.len(), 2);
        assert!((got[0].0 - 10_000).abs() < 10 && (got[0].1 - 60_000).abs() < 10, "{got:?}");
        assert!((got[1].0 - 90_000).abs() < 10 && (got[1].1 - 160_000).abs() < 10 && (got[1].2 - 80_000).abs() < 10, "{got:?}");
        assert_eq!(got.iter().map(|g| (g.0, g.1)).collect::<Vec<_>>(), preview, "the preview shows the cuts");
        // A silent region goes.
        let quiet = region(&mut s, t[1], clip, 0, 10_000, 160_000);
        assert!(s.strip_silence(&[quiet], &set).is_empty() && spans(&s, t[1]).is_empty());
    }

    #[test]
    fn names_and_colours_go_between_tracks_and_regions() {
        let (mut s, t, clip) = session();
        let a = region(&mut s, t[0], clip, 0, 100, 0);
        let b = region(&mut s, t[0], clip, 200, 100, 0);
        s.rename_region(a, "Lead".into());
        s.set_region_color(&[a], Some("#ff0000".into()));
        s.name_tracks_by_region(&[b, a]);
        s.color_tracks_by_region(&[a, b]);
        let tr = s.project.track(t[0]).unwrap();
        assert_eq!((tr.name.as_str(), tr.color.as_str()), ("Lead", "#ff0000"));
        s.update_track(t[0], TrackPatch { name: Some("Vox".into()), ..Default::default() });
        s.name_regions_by_track(&[a, b]);
        s.color_regions_by_track(&[a]);
        let tr = s.project.track(t[0]).unwrap();
        assert!(tr.regions.iter().all(|r| r.name == "Vox"));
        assert!(tr.regions.iter().all(|r| r.color.is_none()));
    }
}
