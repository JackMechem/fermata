//! Session edits for MIDI content: the Piano Roll's wholesale note/controller replacement and
//! Standard MIDI File import and export.

use super::*;
use crate::smf::{self, SmfCc, SmfFile, SmfNote, SmfTrack};

/// One region's new content, as the Piano Roll sends it after an edit.
#[derive(Debug, Clone, Deserialize)]
pub struct MidiContent {
    pub region: Id,
    pub notes: Vec<Note>,
    #[serde(default)]
    pub cc: Vec<CcEvent>,
    /// New timeline start, when the edit grew the region to the left. The notes and controllers
    /// are relative to it.
    #[serde(default)]
    pub start: Option<i64>,
    /// New length, when notes were drawn or moved past the region's end.
    #[serde(default)]
    pub length: Option<i64>,
}

impl Session {
    /// Samples per quarter note at the project tempo.
    fn samples_per_quarter(&self) -> f64 {
        self.project.sample_rate as f64 * 60.0 / self.project.tempo
    }

    /// Replaces the notes and controllers of MIDI regions (and optionally their bounds).
    /// Regions that aren't MIDI are skipped.
    pub fn set_midi_content(&mut self, edits: Vec<MidiContent>) {
        let mut touched = Vec::new();
        for e in edits {
            let Some((track, idx)) = self.find_region(e.region) else { continue };
            let r = &mut self.project.track_mut(track).unwrap().regions[idx];
            let RegionContent::Midi { notes, cc } = &mut r.content else { continue };
            let (mut n, mut c) = (e.notes, e.cc);
            crate::midi::normalize(&mut n, &mut c);
            *notes = n;
            *cc = c;
            if let Some(s) = e.start {
                r.start = s.max(0);
            }
            if let Some(l) = e.length {
                r.length = l.max(1);
            }
            if !touched.contains(&track) {
                touched.push(track);
            }
        }
        for t in touched {
            self.push_regions(t);
        }
        self.dirty = true;
    }

    /// Imports a Standard MIDI File with its tick 0 at `start`. With `track` set (an instrument
    /// track) the file's first part lands on it and the others on new instrument tracks below;
    /// otherwise every part gets a new track, inserted at `index` (the end when None). In an
    /// empty project the file's tempo and time signature are adopted. Returns the new regions.
    pub fn import_midi(&mut self, path: &Path, track: Option<Id>, start: i64, index: Option<usize>) -> Result<Vec<Id>> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let file = smf::read(&bytes)?;
        if file.tracks.is_empty() {
            return Err(anyhow!("{} has no notes", path.display()));
        }
        if self.project.tracks.iter().all(|t| t.regions.is_empty()) {
            if let Some(bpm) = file.tempo {
                // Keep the tempo tidy: files often store 119.99999 for 120.
                self.set_tempo((bpm * 100.0).round() / 100.0);
            }
            if let Some((num, den)) = file.signature {
                self.set_signature(num, den);
            }
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "MIDI".into());
        let spq = self.samples_per_quarter();
        let to_samples = |ticks: u64| (ticks as f64 / file.ppq as f64 * spq).round() as i64;
        let bar = self.project.samples_per_bar();
        let start = start.max(0);

        let target = track.filter(|id| self.project.track(*id).map(|t| t.kind == TrackKind::Instrument).unwrap_or(false));
        let mut next_index = match (target, index) {
            (Some(id), _) => self.project.tracks.iter().position(|t| t.id == id).map(|i| i + 1).unwrap_or(self.project.tracks.len()),
            (None, Some(i)) => i.min(self.project.tracks.len()),
            (None, None) => self.project.tracks.len(),
        };
        let mut made = Vec::new();
        let mut first_track = None;
        for (k, part) in file.tracks.iter().enumerate() {
            let name = if part.name.is_empty() { stem.clone() } else { part.name.clone() };
            let tid = match target {
                Some(id) if k == 0 => id,
                _ => {
                    let id = self.add_track(TrackKind::Instrument, InputSel::None, None, Some(next_index))?;
                    next_index += 1;
                    self.project.track_mut(id).unwrap().name = name.clone();
                    id
                }
            };
            first_track.get_or_insert(tid);

            // The region starts at the bar holding the first event and ends on a bar line.
            let first = part.notes.iter().map(|n| n.start).chain(part.cc.iter().map(|c| c.start)).min().unwrap_or(0);
            let last = part.notes.iter().map(|n| n.start + n.length).chain(part.cc.iter().map(|c| c.start + 1)).max().unwrap_or(0);
            let rstart = start + ((to_samples(first) as f64 / bar).floor() * bar) as i64;
            let end = start + to_samples(last);
            let length = (((end - rstart) as f64 / bar).ceil().max(1.0) * bar).round() as i64;
            let notes = part
                .notes
                .iter()
                .map(|n| Note { start: start + to_samples(n.start) - rstart, length: to_samples(n.length).max(1), pitch: n.pitch, velocity: n.velocity.max(1), muted: false })
                .collect();
            let cc = part.cc.iter().map(|c| CcEvent { start: start + to_samples(c.start) - rstart, kind: c.kind, value: c.value }).collect();
            let rid = self.project.alloc_id();
            self.project.track_mut(tid).unwrap().regions.push(Region { id: rid, name, start: rstart, length, muted: false, color: None, content: RegionContent::Midi { notes, cc }, loop_end: None, takes: None });
            self.push_regions(tid);
            made.push(rid);
        }
        if first_track.is_some() {
            self.select_track(first_track);
        }
        self.dirty = true;
        Ok(made)
    }

    /// Writes the given MIDI regions to a Standard MIDI File, one file track per project track,
    /// at their project positions. Muted notes, and anything a region's end cuts off, are left
    /// out, as they don't play.
    pub fn export_midi(&self, ids: &[Id], path: &Path) -> Result<()> {
        let spq = self.samples_per_quarter();
        let ticks = |s: i64| (s.max(0) as f64 / spq * smf::PPQ as f64).round() as u64;
        let mut parts = Vec::new();
        for t in &self.project.tracks {
            let mut part = SmfTrack { name: t.name.clone(), ..Default::default() };
            for r in t.regions.iter().filter(|r| ids.contains(&r.id)) {
                let RegionContent::Midi { notes, cc } = &r.content else { continue };
                for n in notes.iter().filter(|n| !n.muted && n.start >= 0 && n.start < r.length) {
                    let on = ticks(r.start + n.start);
                    let off = ticks(r.start + (n.start + n.length).min(r.length));
                    part.notes.push(SmfNote { start: on, length: off.saturating_sub(on).max(1), pitch: n.pitch, velocity: n.velocity });
                }
                for c in cc.iter().filter(|c| c.start >= 0 && c.start < r.length) {
                    part.cc.push(SmfCc { start: ticks(r.start + c.start), kind: c.kind, value: c.value });
                }
            }
            if part.notes.is_empty() && part.cc.is_empty() {
                continue;
            }
            part.notes.sort_by_key(|n| (n.start, n.pitch));
            part.cc.sort_by_key(|c| c.start);
            parts.push(part);
        }
        if parts.is_empty() {
            return Err(anyhow!("There are no notes to export"));
        }
        let file = SmfFile { ppq: smf::PPQ, tempo: Some(self.project.tempo), signature: Some((self.project.sig_num, self.project.sig_den)), tracks: parts };
        std::fs::write(path, smf::write(&file)).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}
