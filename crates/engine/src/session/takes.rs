//! The session side of take folders and Autopunch: turning a cycle or punch recording into
//! takes, and the take folder edits (Quick Swipe Comping, Flatten, Pack, Unpack and the rest).
//!
//! The comp logic itself is in `crate::takes`. Playback needs nothing here: `regions_dsp`
//! expands each folder into its comp pieces, so bounces and exports hear the comp too.

use super::*;
use crate::takes::{flat_pieces, pass_pieces, RecPlan, COMP_XFADE_MS};

/// Which recorded MIDI is kept and where its region goes: `lo..hi` on the timeline. Notes
/// that start outside the punch range are dropped and none outlasts the punch-out; in cycle
/// mode a note held across the loop point ends at the cycle end.
pub(super) struct MidiWindow {
    pub lo: i64,
    pub hi: i64,
    wrap: i64,
    cut: i64,
    keep: Option<(i64, i64)>,
}

impl MidiWindow {
    pub fn keeps(&self, pos: i64) -> bool {
        self.keep.map(|(a, b)| pos >= a && pos < b).unwrap_or(true)
    }

    /// Where a note that went down at `on` and up at `off` ends.
    pub fn note_end(&self, on: i64, off: i64) -> i64 {
        let end = if off >= on {
            off
        } else if self.wrap < i64::MAX {
            self.wrap
        } else {
            on
        };
        end.min(self.cut)
    }
}

/// The MIDI window of a recording that started at `punch` and stopped at `stop`.
pub(super) fn midi_window(plan: Option<RecPlan>, punch: i64, stop: i64) -> MidiWindow {
    let plain = MidiWindow { lo: punch, hi: stop, wrap: i64::MAX, cut: i64::MAX, keep: None };
    let Some(plan) = plan else { return plain };
    match (plan.cycle, plan.punch) {
        (Some((_, ce)), p) => match plan.cycle_window() {
            Some((w0, w1, _)) => MidiWindow { lo: w0, hi: w1, wrap: ce, cut: if p.is_some() { w1 } else { i64::MAX }, keep: p.map(|_| (w0, w1)) },
            None => plain,
        },
        (None, Some((a, b))) => MidiWindow { lo: punch.max(a), hi: stop.min(b), wrap: i64::MAX, cut: b, keep: Some((a, b)) },
        (None, None) => plain,
    }
}

impl Session {
    /// How a recording starting at `punch` (transport from `from`) is set up: the plan for
    /// cutting it into takes afterwards, the range where record-enabled tracks fall silent
    /// (None: throughout) and the engine positions input is captured between.
    pub(super) fn record_plan(&self, from: i64, punch: i64, cycle: Option<(i64, i64)>) -> Result<(RecPlan, Option<(i64, i64)>, (i64, i64))> {
        let plan = RecPlan {
            from,
            cycle,
            punch: self.project.punch.range(),
            latency: self.record_latency_frames(),
            min_take: self.tempo_map().samples_per_beat_at(punch as f64).round() as i64,
        };
        let Some(capture) = plan.capture(punch) else {
            return Err(anyhow!(if cycle.is_some() { "The Autopunch range is outside the cycle" } else { "The playhead is past the punch-out point" }));
        };
        let quiet = match (plan.punch, cycle) {
            (Some(_), Some(_)) => plan.cycle_window().map(|(a, b, _)| (a, b)),
            (p, None) => p,
            (None, Some(_)) => None,
        };
        Ok((plan, quiet, capture))
    }

    /// A finished audio recording, as `finish_recording` made it (`plain`, from the file's
    /// `frames` captured from engine position `start`), cut to what the plan says: cycle passes
    /// become a take folder (or more takes in the folder already there), an autopunch take is
    /// trimmed to the punch range. None when there's nothing new to place.
    pub(super) fn shape_take(&mut self, plan: Option<RecPlan>, track: Id, plain: Region, start: i64, frames: i64) -> Option<Region> {
        let Some(plan) = plan else { return Some(plain) };
        let RegionContent::Audio { clip, .. } = plain.content else { return Some(plain) };
        if plan.cycle.is_none() {
            if plan.punch.is_none() {
                return Some(plain);
            }
            let (at, length, offset) = plan.punch_span(start, frames)?;
            return Some(self.trimmed(plain, at, length, offset));
        }
        let (fs, len, takes) = plan.cycle_takes(frames)?;
        // Recording the same cycle again adds takes to the folder there, as in Logic.
        let existing = self.project.track(track)?.regions.iter().position(|r| {
            r.takes.is_some() && r.start == fs && r.length == len && !r.is_looped() && matches!(r.content, RegionContent::Audio { offset: 0, .. })
        });
        if takes.is_empty() || (takes.len() == 1 && existing.is_none()) {
            let &(offset, a, b) = takes.first()?;
            return Some(self.trimmed(plain, fs + a, b - a, offset + a));
        }
        let mut folder = existing.and_then(|i| self.project.track(track).unwrap().regions[i].takes.clone()).unwrap_or(TakeFolder { takes: Vec::new(), comp: Vec::new() });
        let numbered = folder.takes.len();
        for (k, (offset, a, b)) in takes.into_iter().enumerate() {
            let id = self.project.alloc_id();
            folder.takes.push(Take { id, name: format!("Take {}", numbered + k + 1), clip, offset, start: a, end: b, gain_db: 0.0 });
        }
        folder.default_comp();
        if let Some(i) = existing {
            let r = &mut self.project.track_mut(track)?.regions[i];
            if let RegionContent::Audio { clip: c, .. } = &mut r.content {
                *c = clip;
            }
            r.takes = Some(folder);
            return None;
        }
        let fade = self.default_fade(len);
        Some(Region {
            start: fs,
            length: len,
            content: RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() },
            takes: Some(folder),
            ..plain
        })
    }

    /// `plain` moved to `start..start + length`, from clip frame `offset`, with fresh fades.
    fn trimmed(&self, plain: Region, start: i64, length: i64, offset: i64) -> Region {
        let fade = self.default_fade(length);
        let mut r = Region { start, length, ..plain };
        if let RegionContent::Audio { offset: o, fade_in, fade_out, .. } = &mut r.content {
            *o = offset;
            *fade_in = fade;
            *fade_out = fade;
        }
        r
    }

    fn comp_xfade(&self) -> i64 {
        (COMP_XFADE_MS * self.project.sample_rate as f64 / 1000.0).round() as i64
    }

    /// What a take folder's first pass plays (frames x channels), as `render_region` does for
    /// plain audio: the comp with its crossfades, the folder's gain and fades.
    pub(super) fn render_comp(&self, r: &Region, f: &TakeFolder, channels: usize) -> Vec<Vec<f32>> {
        let len = r.length.max(0);
        let mut out = vec![vec![0.0f32; len as usize]; channels];
        let RegionContent::Audio { gain_db, .. } = r.content else { return out };
        let curve = |x: f32| (x.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin();
        for p in pass_pieces(r, f, r.start, r.length, self.comp_xfade()) {
            let Some(c) = self.clips.get(&p.clip) else { continue };
            let g = volume_gain(gain_db + p.gain_db);
            for tl in p.start.max(r.start)..p.end.min(r.start + len) {
                let src = p.offset + (tl - p.start);
                if src < 0 || src as usize >= c.frames() {
                    continue;
                }
                let mut gain = g;
                if p.fade_in.1 > 0 && tl < p.fade_in.0 + p.fade_in.1 {
                    gain *= curve((tl - p.fade_in.0) as f32 / p.fade_in.1 as f32);
                }
                if p.fade_out.1 > 0 && tl >= p.fade_out.0 {
                    gain *= curve((p.fade_out.0 + p.fade_out.1 - tl) as f32 / p.fade_out.1 as f32);
                }
                for (ch, o) in out.iter_mut().enumerate() {
                    o[(tl - r.start) as usize] += c.channels[ch.min(c.channels.len() - 1)][src as usize] * gain;
                }
            }
        }
        out
    }

    /// The take folder `region` and the track it is on.
    fn folder_mut(&mut self, region: Id) -> Option<(Id, &mut Region)> {
        let (track, idx) = self.find_region(region)?;
        let r = &mut self.project.track_mut(track)?.regions[idx];
        r.takes.is_some().then_some((track, r))
    }

    /// Applies `f` to a folder and refreshes its track. `f` also gets what to add to a
    /// timeline position to get the folder's take time.
    fn edit_folder(&mut self, region: Id, f: impl FnOnce(&mut TakeFolder, i64)) {
        let Some((track, r)) = self.folder_mut(region) else { return };
        let to_take_time = match r.content {
            RegionContent::Audio { offset, .. } => offset - r.start,
            _ => 0,
        };
        let newest = {
            let folder = r.takes.as_mut().unwrap();
            f(folder, to_take_time);
            folder.takes.last().map(|t| t.clip)
        };
        // The folder's own clip follows its newest take (see `TakeFolder`).
        if let (Some(c), RegionContent::Audio { clip, .. }) = (newest, &mut r.content) {
            *clip = c;
        }
        self.push_regions(track);
        self.dirty = true;
    }

    /// Quick Swipe Comping: `take` is heard over timeline `start..end` of the folder (None
    /// silences that part of the comp).
    pub fn set_comp_range(&mut self, region: Id, take: Option<Id>, start: i64, end: i64) {
        self.edit_folder(region, |f, shift| {
            let (a, b) = (start.min(end) + shift, start.max(end) + shift);
            match take {
                Some(t) => f.select(t, a, b),
                None => f.clear(a, b),
            }
        });
    }

    /// Makes `take` the one heard throughout the folder.
    pub fn select_take(&mut self, region: Id, take: Id) {
        self.edit_folder(region, |f, _| f.select_whole(take));
    }

    pub fn rename_take(&mut self, region: Id, take: Id, name: String) {
        self.edit_folder(region, |f, _| {
            if let Some(t) = f.take_mut(take) {
                t.name = name;
            }
        });
    }

    /// Removes one take; the comp loses the parts it played.
    pub fn delete_take(&mut self, region: Id, take: Id) {
        self.edit_folder(region, |f, _| {
            if f.takes.len() > 1 {
                f.remove_take(take);
            }
        });
    }

    /// Delete Unused Takes: keeps only the takes the comp uses.
    pub fn delete_unused_takes(&mut self, region: Id) {
        self.edit_folder(region, |f, _| f.remove_unused());
    }

    /// Flatten: replaces the folder with plain regions, one per stretch of its comp (a looping
    /// folder flattens every pass). Returns the new regions.
    pub fn flatten_take_folder(&mut self, region: Id) -> Vec<Id> {
        let Some((track, idx)) = self.find_region(region) else { return Vec::new() };
        let r = self.project.track(track).unwrap().regions[idx].clone();
        let Some(folder) = r.takes.clone() else { return Vec::new() };
        let RegionContent::Audio { gain_db, .. } = r.content else { return Vec::new() };
        let mut made = Vec::new();
        for pass in r.real_copies() {
            let (pass_start, pass_end) = (pass.start, pass.start + pass.length);
            let (fi, fo, shape) = match pass.content {
                RegionContent::Audio { fade_in, fade_out, fade_shape, .. } => (fade_in, fade_out, fade_shape),
                _ => (0, 0, FadeShape::default()),
            };
            for (start, length, take, offset) in flat_pieces(&pass, &folder) {
                let t = folder.take(take).unwrap();
                let small = self.default_fade(length);
                let fade_in = if start == pass_start { fi.min(length) } else { small };
                let fade_out = if start + length == pass_end { fo.min(length - fade_in) } else { small };
                // The folder's fade shapes go with its own fades, on the outer pieces.
                let mut fade_shape = FadeShape::default();
                if start == pass_start {
                    (fade_shape.in_curve, fade_shape.in_type) = (shape.in_curve, shape.in_type);
                }
                if start + length == pass_end {
                    (fade_shape.out_curve, fade_shape.out_type) = (shape.out_curve, shape.out_type);
                }
                let id = self.project.alloc_id();
                made.push(Region {
                    id,
                    name: r.name.clone(),
                    start,
                    length,
                    muted: r.muted,
                    color: r.color.clone(),
                    content: RegionContent::Audio { clip: t.clip, offset, gain_db: gain_db + t.gain_db, fade_in, fade_out, fade_shape },
                    loop_end: None,
                    takes: None,
                    params: Default::default(),
                });
            }
        }
        let ids = made.iter().map(|r| r.id).collect();
        let t = self.project.track_mut(track).unwrap();
        t.regions.splice(idx..idx + 1, made);
        self.push_regions(track);
        self.dirty = true;
        ids
    }

    /// Flatten and Merge: renders the comp into a new audio file, one region in the folder's
    /// place (keeping its gain, fades and loop). Returns it.
    pub fn merge_take_folder(&mut self, region: Id) -> Result<Vec<Id>> {
        let Some((track, idx)) = self.find_region(region) else { return Ok(Vec::new()) };
        let r = self.project.track(track).unwrap().regions[idx].clone();
        let Some(folder) = r.takes.clone() else { return Ok(Vec::new()) };
        let RegionContent::Audio { offset, gain_db, fade_in, fade_out, fade_shape, .. } = r.content else { return Ok(Vec::new()) };
        // Rendered without the folder's gain and fades, which stay adjustable on the region.
        let plain = Region { content: RegionContent::Audio { clip: 0, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: FadeShape::default() }, loop_end: None, ..r.clone() };
        let channels = folder.takes.iter().filter_map(|t| self.clips.get(&t.clip)).map(|c| c.channels.len().min(2)).max().unwrap_or(1);
        let audio = self.render_comp(&plain, &folder, channels);
        let rate = self.sample_rate() as u32;
        let path = self.unique_audio_path(&format!("{} Comp", r.name));
        crate::decode::write_wav(&path, rate, &audio)?;
        let clip = self.add_clip(&path, Clip::from_channels(path.clone(), rate, audio));
        let id = self.project.alloc_id();
        let merged = Region { id, content: RegionContent::Audio { clip, offset: 0, gain_db, fade_in, fade_out, fade_shape }, takes: None, ..r };
        self.project.track_mut(track).unwrap().regions[idx] = merged;
        self.push_regions(track);
        self.dirty = true;
        Ok(vec![id])
    }

    /// Unpack to New Tracks: every take goes onto a new track below (with the same settings),
    /// muted so the song sounds the same, and the folder is flattened in place. Returns the
    /// new tracks.
    pub fn unpack_take_folder(&mut self, region: Id) -> Vec<Id> {
        let Some((track, idx)) = self.find_region(region) else { return Vec::new() };
        let r = self.project.track(track).unwrap().regions[idx].clone();
        let Some(folder) = r.takes.clone() else { return Vec::new() };
        let RegionContent::Audio { offset: tt0, gain_db, .. } = r.content else { return Vec::new() };
        let mut made = Vec::new();
        // Each new track lands right below the folder's, so go newest first to keep the order.
        for t in folder.takes.iter().rev() {
            let (a, b) = (t.start.max(tt0), t.end.min(tt0 + r.length));
            let Some(&dst) = self.duplicate_tracks(&[track], false).first() else { continue };
            if a < b {
                let id = self.project.alloc_id();
                let length = b - a;
                let fade = self.default_fade(length);
                let take = Region {
                    id,
                    name: t.name.clone(),
                    start: r.start + a - tt0,
                    length,
                    muted: true,
                    color: r.color.clone(),
                    content: RegionContent::Audio { clip: t.clip, offset: t.offset + a, gain_db: gain_db + t.gain_db, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() },
                    loop_end: None,
                    takes: None,
                    params: Default::default(),
                };
                if let Some(d) = self.project.track_mut(dst) {
                    d.name = t.name.clone();
                    d.regions.push(take);
                }
                self.push_regions(dst);
            }
            made.push(dst);
        }
        made.reverse();
        self.flatten_take_folder(region);
        made
    }

    /// Pack Take Folder: turns audio regions on one track into a take folder spanning them,
    /// one take each (the newest region, last on the track, starts out heard). Returns it.
    pub fn pack_take_folder(&mut self, ids: &[Id]) -> Result<Id> {
        let Some((track, _)) = ids.first().and_then(|id| self.find_region(*id)) else { return Err(anyhow!("No regions to pack")) };
        let t = self.project.track(track).unwrap();
        let picked: Vec<Region> = t.regions.iter().filter(|r| ids.contains(&r.id)).cloned().collect();
        if picked.len() != ids.len() {
            return Err(anyhow!("Take folders are packed from regions on one track"));
        }
        if picked.len() < 2 || picked.iter().any(|r| r.takes.is_some() || !matches!(r.content, RegionContent::Audio { .. })) {
            return Err(anyhow!("Select two or more audio regions to pack into a take folder"));
        }
        let fs = picked.iter().map(|r| r.start).min().unwrap();
        let fe = picked.iter().map(|r| r.start + r.length).max().unwrap();
        let mut folder = TakeFolder { takes: Vec::new(), comp: Vec::new() };
        for r in &picked {
            let RegionContent::Audio { clip, offset, gain_db, .. } = r.content else { continue };
            let at = r.start - fs;
            let id = self.project.alloc_id();
            folder.takes.push(Take { id, name: r.name.clone(), clip, offset: offset - at, start: at, end: at + r.length, gain_db });
        }
        folder.default_comp();
        let newest = picked.last().unwrap();
        let clip = folder.takes.last().map(|t| t.clip).unwrap_or(0);
        let id = self.project.alloc_id();
        let fade = self.default_fade(fe - fs);
        let packed = Region {
            id,
            name: newest.name.clone(),
            start: fs,
            length: fe - fs,
            muted: false,
            color: newest.color.clone(),
            content: RegionContent::Audio { clip, offset: 0, gain_db: 0.0, fade_in: fade, fade_out: fade, fade_shape: FadeShape::default() },
            loop_end: None,
            takes: Some(folder),
            params: Default::default(),
        };
        let t = self.project.track_mut(track).unwrap();
        let at = t.regions.iter().position(|r| ids.contains(&r.id)).unwrap_or(t.regions.len());
        t.regions.retain(|r| !ids.contains(&r.id));
        t.regions.insert(at.min(t.regions.len()), packed);
        self.push_regions(track);
        self.dirty = true;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_window_follows_cycle_and_punch() {
        let plan = RecPlan { from: 0, cycle: Some((1000, 2000)), punch: None, latency: 0, min_take: 1 };
        let w = midi_window(Some(plan), 1000, 1500);
        assert_eq!((w.lo, w.hi), (1000, 2000));
        // Held across the loop point: ends at the cycle end.
        assert_eq!(w.note_end(1900, 1100), 2000);
        assert_eq!(w.note_end(1200, 1300), 1300);
        let punched = midi_window(Some(RecPlan { cycle: None, punch: Some((1200, 1600)), ..plan }), 0, 3000);
        assert_eq!((punched.lo, punched.hi), (1200, 1600));
        assert!(!punched.keeps(1100) && punched.keeps(1200) && !punched.keeps(1600));
        assert_eq!(punched.note_end(1500, 1900), 1600);
        let plain = midi_window(None, 500, 900);
        assert_eq!((plain.lo, plain.hi, plain.note_end(600, 700)), (500, 900, 700));
    }
}
