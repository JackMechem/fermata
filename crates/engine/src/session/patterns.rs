//! Pattern regions on the session side: creating them, the Step Sequencer's edits, converting
//! to and from MIDI regions, and the notes `regions_dsp` plays for them. The pattern maths is
//! in [`crate::pattern`].

use anyhow::{anyhow, Result};

use super::Session;
use crate::model::{Id, Region, RegionContent, TrackKind};
use crate::pattern::{self, Pattern};

impl Session {
    /// The regions with every pattern region turned into the plain MIDI region it plays, or
    /// None when there is no pattern region among them.
    pub(super) fn realize_patterns(&self, regions: &[Region]) -> Option<Vec<Region>> {
        if !regions.iter().any(|r| r.pattern.is_some()) {
            return None;
        }
        let map = self.tempo_map();
        Some(regions.iter().map(|r| if r.pattern.is_some() { pattern::realize(r, &map) } else { r.clone() }).collect())
    }

    /// Adds a pattern region with the default kit (Create Pattern Region) and returns its id.
    pub fn create_pattern_region(&mut self, track: Id, start: i64, length: i64) -> Result<Id> {
        self.check_unfrozen(&[track])?;
        match self.project.track(track) {
            Some(t) if t.kind == TrackKind::Instrument => {}
            _ => return Err(anyhow!("Pattern regions go on software instrument tracks")),
        }
        let id = self.project.alloc_id();
        let region = Region {
            id,
            name: "Pattern".into(),
            start: start.max(0),
            length: length.max(1),
            muted: false,
            color: None,
            content: RegionContent::Midi { notes: Vec::new(), cc: Vec::new() },
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: Some(Pattern::default()),
        };
        self.project.track_mut(track).unwrap().regions.push(region);
        self.push_regions(track);
        self.dirty = true;
        Ok(id)
    }

    /// Replaces a pattern region's pattern (the Step Sequencer's edits). Values are kept in
    /// range. When the pattern's length changes and the region was exactly as long as the
    /// pattern, the region follows, as a new pattern region's bar does.
    pub fn set_pattern(&mut self, id: Id, pattern: Pattern) -> Result<()> {
        let (track, _) = self.find_region(id).ok_or_else(|| anyhow!("No such region"))?;
        self.check_unfrozen(&[track])?;
        if self.region(id).is_some_and(|r| r.pattern.is_none()) {
            return Err(anyhow!("Not a pattern region"));
        }
        let map = self.tempo_map();
        let new = pattern.clamped();
        self.edit_regions(&[id], |r, _| {
            let Some(old) = &r.pattern else { return };
            let length_of = |p: &Pattern| (map.sample_at(map.beats_at(r.start as f64) + p.quarters()) - r.start as f64).round() as i64;
            if old.quarters() != new.quarters() && (r.length - length_of(old)).abs() <= 1 {
                r.length = length_of(&new).max(1);
                if !r.is_looped() {
                    r.loop_end = None;
                }
            }
            r.pattern = Some(new.clone());
        });
        self.dirty = true;
        Ok(())
    }

    /// Convert to MIDI Region: pattern regions become the MIDI regions they play.
    pub fn convert_patterns_to_midi(&mut self, ids: &[Id]) {
        let map = self.tempo_map();
        self.edit_regions(ids, |r, _| *r = pattern::realize(r, &map));
        self.dirty = true;
    }

    /// Convert to Pattern Region: MIDI regions become pattern regions playing their notes on
    /// a sixteenth-note grid (see [`pattern::from_notes`]), in place of their notes.
    /// Controllers stay.
    pub fn convert_midi_to_patterns(&mut self, ids: &[Id]) {
        let map = self.tempo_map();
        self.edit_regions(ids, |r, _| {
            if r.pattern.is_some() || r.takes.is_some() {
                return;
            }
            let RegionContent::Midi { notes, .. } = &mut r.content else { return };
            r.pattern = Some(pattern::from_notes(notes, r.start, r.length, &map));
            notes.clear();
        });
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::model::{InputSel, Note};
    use crate::pattern::Step;

    fn session() -> Session {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        Session::new(cfg, Vec::new())
    }

    fn notes_of(r: &Region) -> &[Note] {
        match &r.content {
            RegionContent::Midi { notes, .. } => notes,
            _ => &[],
        }
    }

    #[test]
    fn create_edit_undo_and_convert() {
        let mut s = session();
        let audio = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        assert!(s.create_pattern_region(audio, 0, 1000).is_err());
        let t = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
        let bar = s.tempo_map().samples_per_bar_at(0.0).round() as i64;
        s.checkpoint("Create Pattern Region", None, false);
        let id = s.create_pattern_region(t, 0, bar).unwrap();
        let r = s.region(id).unwrap().clone();
        assert_eq!(r.pattern.as_ref().unwrap().rows.len(), pattern::DEFAULT_KIT.len());

        // Two kicks: the pattern plays them, the stored notes stay empty.
        let mut p = r.pattern.clone().unwrap();
        p.rows[0].steps = vec![Step { on: true, ..Step::default() }; 1];
        p.rows[0].steps.resize(9, Step::default());
        p.rows[0].steps[8].on = true;
        s.checkpoint("Step", None, false);
        s.set_pattern(id, p.clone()).unwrap();
        assert!(notes_of(s.region(id).unwrap()).is_empty());
        let played = s.played_notes();
        let starts: Vec<i64> = played.iter().find(|p| p.region == id).unwrap().notes.iter().map(|n| n.start).collect();
        assert_eq!(starts, vec![0, bar / 2]);

        // Doubling the pattern's length takes the region along.
        let mut longer = p.clone();
        longer.length = 32;
        s.set_pattern(id, longer).unwrap();
        assert_eq!(s.region(id).unwrap().length, 2 * bar);

        s.undo();
        assert_eq!(s.region(id).unwrap().length, bar);
        assert_eq!(s.region(id).unwrap().pattern.as_ref().unwrap().rows[0].steps.len(), 0);

        s.set_pattern(id, p).unwrap();
        s.convert_patterns_to_midi(&[id]);
        let r = s.region(id).unwrap();
        assert!(r.pattern.is_none());
        assert_eq!(notes_of(r).iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>(), vec![(0, 36), (bar / 2, 36)]);

        s.convert_midi_to_patterns(&[id]);
        let r = s.region(id).unwrap();
        assert!(notes_of(r).is_empty());
        let p = r.pattern.as_ref().unwrap();
        assert_eq!(p.rows.len(), 1);
        assert!(p.rows[0].step(0).on && p.rows[0].step(8).on);
    }
}
