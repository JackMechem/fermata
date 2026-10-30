//! Track alternatives (Logic's Track Alternatives): a track can keep several sets of regions
//! and switch between them. The active set is `Track::regions`, named `Track::alternative`
//! ("A" until named); the others wait in `Track::alternatives`. Switching swaps them and
//! resends the regions. Each operation is an ordinary edit, so undo restores it.

use anyhow::{anyhow, Result};

use super::Session;
use crate::model::{Id, Region, Track, TrackAlternative};

/// The name of a track's first alternative.
const FIRST: &str = "A";

/// The active alternative's name.
pub fn active_name(t: &Track) -> &str {
    if t.alternative.is_empty() {
        FIRST
    } else {
        &t.alternative
    }
}

/// Every alternative of a track, the active one included, by name.
pub fn names(t: &Track) -> Vec<String> {
    let mut out: Vec<String> = t.alternatives.iter().map(|a| a.name.clone()).collect();
    out.push(active_name(t).to_string());
    out.sort_by_key(|n| n.to_lowercase());
    out
}

/// The next free name in Logic's sequence: A, B, C … Z, then A2, B2 …
fn next_name(taken: &[String]) -> String {
    let free = |n: &String| !taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    (1..)
        .flat_map(|round: u32| ('A'..='Z').map(move |c| if round == 1 { c.to_string() } else { format!("{c}{round}") }))
        .find(free)
        .unwrap()
}

impl Session {
    /// The track, if its alternatives can be edited (it exists and isn't frozen).
    fn alt_track(&mut self, track: Id) -> Result<&mut Track> {
        let t = self.project.track_mut(track).ok_or_else(|| anyhow!("No such track"))?;
        if t.frozen.is_some() {
            return Err(anyhow!("{} is frozen. Unfreeze it to change its track alternatives.", t.name));
        }
        Ok(t)
    }

    /// Track › Track Alternatives › New (empty) or Duplicate (a copy of the active one's
    /// regions): the new alternative becomes the active one. Returns its name.
    pub fn new_track_alternative(&mut self, track: Id, duplicate: bool) -> Result<String> {
        let t = self.alt_track(track)?;
        let name = next_name(&names(t));
        let copy: Vec<Region> = if duplicate { t.regions.clone() } else { Vec::new() };
        let copy: Vec<Region> = copy.into_iter().map(|r| Region { id: self.project.alloc_id(), ..r }).collect();
        let t = self.project.track_mut(track).unwrap();
        let old = TrackAlternative { name: active_name(t).to_string(), regions: std::mem::replace(&mut t.regions, copy) };
        t.alternatives.push(old);
        t.alternative = name.clone();
        self.push_regions(track);
        self.dirty = true;
        Ok(name)
    }

    /// Makes alternative `name` the track's active one.
    pub fn switch_track_alternative(&mut self, track: Id, name: &str) -> Result<()> {
        let t = self.alt_track(track)?;
        if name == active_name(t) {
            return Ok(());
        }
        let i = t.alternatives.iter().position(|a| a.name == name).ok_or_else(|| anyhow!("No alternative named “{name}”"))?;
        let next = t.alternatives.remove(i);
        let old = TrackAlternative { name: active_name(t).to_string(), regions: std::mem::replace(&mut t.regions, next.regions) };
        t.alternatives.insert(i, old);
        t.alternative = next.name;
        self.push_regions(track);
        self.dirty = true;
        Ok(())
    }

    /// Renames one of the track's alternatives.
    pub fn rename_track_alternative(&mut self, track: Id, name: &str, new_name: &str) -> Result<()> {
        let new_name = new_name.trim().to_string();
        let t = self.alt_track(track)?;
        if new_name.is_empty() {
            return Err(anyhow!("The name can't be empty."));
        }
        if names(t).iter().any(|n| n != name && n.to_lowercase() == new_name.to_lowercase()) {
            return Err(anyhow!("This track already has an alternative named “{new_name}”."));
        }
        if name == active_name(t) {
            t.alternative = new_name;
        } else {
            t.alternatives.iter_mut().find(|a| a.name == name).ok_or_else(|| anyhow!("No alternative named “{name}”"))?.name = new_name;
        }
        self.dirty = true;
        Ok(())
    }

    /// Deletes one of the track's alternatives with its regions. Deleting the active one makes
    /// the first of the others (by name) active. The last one can't go.
    pub fn delete_track_alternative(&mut self, track: Id, name: &str) -> Result<()> {
        let t = self.alt_track(track)?;
        if t.alternatives.is_empty() {
            return Err(anyhow!("A track keeps at least one alternative."));
        }
        if name == active_name(t) {
            let first = names(t).into_iter().find(|n| n != name).unwrap();
            let i = t.alternatives.iter().position(|a| a.name == first).unwrap();
            let next = t.alternatives.remove(i);
            t.regions = next.regions;
            t.alternative = next.name;
            self.push_regions(track);
        } else {
            let i = t.alternatives.iter().position(|a| a.name == name).ok_or_else(|| anyhow!("No alternative named “{name}”"))?;
            t.alternatives.remove(i);
        }
        self.dirty = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::model::{InputSel, RegionContent, TrackKind};

    fn session() -> Session {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        Session::new(cfg, Vec::new())
    }

    fn region(id: Id, start: i64) -> Region {
        Region {
            id,
            name: "r".into(),
            start,
            length: 100,
            muted: false,
            color: None,
            content: RegionContent::Midi { notes: Vec::new(), cc: Vec::new() },
            loop_end: None,
            takes: None,
            params: Default::default(),
        }
    }

    fn starts(s: &Session, t: Id) -> Vec<i64> {
        s.project.track(t).unwrap().regions.iter().map(|r| r.start).collect()
    }

    #[test]
    fn names_follow_logic() {
        assert_eq!(next_name(&["A".into()]), "B");
        assert_eq!(next_name(&["A".into(), "c".into(), "B".into()]), "D");
        let all: Vec<String> = ('A'..='Z').map(|c| c.to_string()).collect();
        assert_eq!(next_name(&all), "A2");
    }

    #[test]
    fn new_duplicate_switch_rename_delete_undo() {
        let mut s = session();
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let id = s.project.alloc_id();
        s.project.track_mut(t).unwrap().regions.push(region(id, 0));

        s.checkpoint("New Track Alternative", None, false);
        assert_eq!(s.new_track_alternative(t, false).unwrap(), "B");
        assert!(starts(&s, t).is_empty());
        assert_eq!(names(s.project.track(t).unwrap()), vec!["A", "B"]);

        s.checkpoint("Duplicate Track Alternative", None, false);
        let id = s.project.alloc_id();
        s.project.track_mut(t).unwrap().regions.push(region(id, 500));
        assert_eq!(s.new_track_alternative(t, true).unwrap(), "C");
        assert_eq!(starts(&s, t), vec![500]);
        // A copy has its own ids.
        let tr = s.project.track(t).unwrap();
        assert_ne!(tr.regions[0].id, tr.alternatives[1].regions[0].id);

        s.switch_track_alternative(t, "A").unwrap();
        assert_eq!(starts(&s, t), vec![0]);
        assert_eq!(active_name(s.project.track(t).unwrap()), "A");
        assert!(s.switch_track_alternative(t, "Z").is_err());

        s.rename_track_alternative(t, "A", "Verse").unwrap();
        s.rename_track_alternative(t, "C", "Chorus").unwrap();
        assert!(s.rename_track_alternative(t, "B", "verse").is_err());
        assert_eq!(names(s.project.track(t).unwrap()), vec!["B", "Chorus", "Verse"]);

        // Deleting the active one activates the first other.
        s.delete_track_alternative(t, "Verse").unwrap();
        assert_eq!(active_name(s.project.track(t).unwrap()), "B");
        s.delete_track_alternative(t, "Chorus").unwrap();
        assert!(s.delete_track_alternative(t, "B").is_err());
        assert!(s.project.track(t).unwrap().alternatives.is_empty());

        // Undo goes back through the steps, regions included: to before Duplicate (only the
        // checkpointed steps above are undo steps here)…
        s.undo();
        let tr = s.project.track(t).unwrap();
        assert_eq!((active_name(tr), tr.alternatives.len()), ("B", 1));
        assert!(starts(&s, t).is_empty());
        // …and before New.
        s.undo();
        let tr = s.project.track(t).unwrap();
        assert_eq!((active_name(tr), tr.alternatives.len()), ("A", 0));
        assert_eq!(starts(&s, t), vec![0]);
    }

    #[test]
    fn frozen_tracks_refuse() {
        let mut s = session();
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        s.project.track_mut(t).unwrap().frozen = Some(crate::model::FreezeInfo { clip: 1, start: 0 });
        assert!(s.new_track_alternative(t, false).is_err());
    }
}
