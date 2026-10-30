//! Region playback parameters on the session side: the Region inspector's edits, Apply Region
//! Parameters, and the playback lists `regions_dsp` builds from them. The transforms
//! themselves are pure functions in [`crate::region_params`].

use serde::{Deserialize, Serialize};

use super::*;
use crate::region_params as rp;
use crate::tempo::TempoMap;

/// A change from the Region inspector. The fields given are set on every region; the MIDI ones
/// only on MIDI regions (audio regions have just the delay).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RegionParamsPatch {
    pub quantize: Option<u16>,
    pub swing: Option<u8>,
    pub transpose: Option<i16>,
    pub velocity: Option<i16>,
    pub dynamics: Option<u16>,
    pub gate: Option<u16>,
    pub delay: Option<f64>,
    /// Switches the delay's unit; without `delay`, the value is converted so the region plays
    /// as before at its start.
    pub delay_ms: Option<bool>,
}

impl RegionParamsPatch {
    /// The undo label: the Inspector's name for what changes.
    pub fn label(&self) -> &'static str {
        match self {
            p if p.quantize.is_some() => "Quantize",
            p if p.swing.is_some() => "Q-Swing",
            p if p.transpose.is_some() => "Transpose",
            p if p.velocity.is_some() => "Velocity",
            p if p.dynamics.is_some() => "Dynamics",
            p if p.gate.is_some() => "Gate Time",
            _ => "Delay",
        }
    }
}

/// The notes of a MIDI region with playback parameters as they play (relative to the region
/// start), so the Tracks area can draw them.
#[derive(Debug, Clone, Serialize)]
pub struct PlayedNotes {
    pub region: Id,
    pub notes: Vec<Note>,
}

impl Session {
    /// The tempo map, when any of the track's regions has playback parameters.
    pub(super) fn params_map(&self, t: &Track) -> Option<TempoMap> {
        t.regions.iter().any(|r| !r.params.is_default()).then(|| self.tempo_map())
    }

    /// The track's regions with each audio region's delay applied (moved), or None when no
    /// audio region has one.
    pub(super) fn delay_audio(t: &Track, map: &TempoMap) -> Option<Vec<Region>> {
        let delayed = |r: &Region| r.params.delay != 0.0 && matches!(r.content, RegionContent::Audio { .. });
        if !t.regions.iter().any(delayed) {
            return None;
        }
        Some(t.regions.iter().map(|r| if delayed(r) { rp::bake(r, map) } else { r.clone() }).collect())
    }

    /// The playback entries of a MIDI region with parameters, one per pass. Passes of the same
    /// length share their note and controller lists.
    pub(super) fn midi_passes(r: &Region, passes: &[(i64, i64)], map: &TempoMap) -> Vec<RegionDsp> {
        let mut made: Vec<(i64, RegionDsp)> = Vec::new();
        let mut out = Vec::with_capacity(passes.len());
        for &(ps, pl) in passes {
            if !made.iter().any(|m| m.0 == pl) {
                let Some(p) = rp::pass(r, pl, map) else { continue };
                made.push((pl, RegionDsp::Midi { start: -p.lead, length: p.length, notes: Arc::new(p.notes), cc: Arc::new(p.cc) }));
            }
            let Some((_, RegionDsp::Midi { start, length, notes, cc })) = made.iter().find(|m| m.0 == pl) else { continue };
            out.push(RegionDsp::Midi { start: ps + start, length: *length, notes: notes.clone(), cc: cc.clone() });
        }
        out
    }

    /// Sets playback parameters on regions (Region inspector). Values are kept in range.
    pub fn set_region_params(&mut self, ids: &[Id], patch: &RegionParamsPatch) {
        let map = self.tempo_map();
        self.edit_regions(ids, |r, _| {
            let midi = matches!(r.content, RegionContent::Midi { .. });
            let p = &mut r.params;
            if midi {
                p.quantize = patch.quantize.unwrap_or(p.quantize);
                p.swing = patch.swing.unwrap_or(p.swing);
                p.transpose = patch.transpose.unwrap_or(p.transpose);
                p.velocity = patch.velocity.unwrap_or(p.velocity);
                p.dynamics = patch.dynamics.unwrap_or(p.dynamics);
                p.gate = patch.gate.unwrap_or(p.gate);
            }
            if let Some(ms) = patch.delay_ms {
                p.delay = p.delay_in(ms, r.start as f64, &map);
                p.delay_ms = ms;
                // Whole ticks, tenths of a millisecond.
                p.delay = if ms { (p.delay * 10.0).round() / 10.0 } else { p.delay.round() };
            }
            p.delay = patch.delay.unwrap_or(p.delay);
            *p = p.clamped();
        });
    }

    /// Apply Region Parameters: writes each region's playback parameters into it (its notes,
    /// or an audio region's position) and resets them, so it plays as before.
    pub fn apply_region_params(&mut self, ids: &[Id]) {
        let map = self.tempo_map();
        self.edit_regions(ids, |r, _| *r = rp::bake(r, &map));
    }

    /// The notes of every MIDI region with playback parameters, as they play.
    pub fn played_notes(&self) -> Vec<PlayedNotes> {
        let mut out = Vec::new();
        let mut map = None;
        for r in self.project.tracks.iter().flat_map(|t| t.regions.iter()).filter(|r| !r.params.is_default()) {
            let RegionContent::Midi { notes, .. } = &r.content else { continue };
            let map = map.get_or_insert_with(|| self.tempo_map());
            let mut notes = rp::apply(&r.params, notes, &[], r.start, map).0;
            notes.sort_by_key(|n| (n.start, n.pitch));
            out.push(PlayedNotes { region: r.id, notes });
        }
        out
    }

    /// Rebuilds the playback lists of tracks whose regions' parameters follow the tempo map
    /// (quantize, delay in ticks), after the tempo or signature changed.
    pub(super) fn push_tempo_regions(&mut self) {
        let ids: Vec<Id> = self.project.tracks.iter().filter(|t| t.regions.iter().any(|r| r.params.follows_tempo())).map(|t| t.id).collect();
        for id in ids {
            self.push_regions(id);
        }
    }
}
