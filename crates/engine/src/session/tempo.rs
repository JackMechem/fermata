//! Session edits for the Tempo and Signature global tracks.
//!
//! Regions stay locked to absolute time: a tempo change moves the bar grid, not the audio or
//! MIDI. "Move MIDI Regions with Tempo" (`move_midi` below) instead keeps MIDI regions and
//! their notes on their bars and beats; audio never moves.

use super::*;
use crate::tempo::{normalize_sigs, normalize_tempo, SigEvent, TempoEvent, TempoMap};

impl Session {
    /// The project's tempo map, built from its current tempo and signature changes.
    pub fn tempo_map(&self) -> TempoMap {
        TempoMap::from_project(&self.project)
    }

    /// What plug-ins are told about the transport at the playhead, from the engine itself
    /// (for checks: it shows which tempo map the engine has).
    pub fn transport_info(&self) -> vst3_host::TransportInfo {
        self.engine.lock().transport()
    }

    /// Replaces the Tempo track (an event at 0 is added when missing). With `move_midi`, MIDI
    /// regions and their notes move to keep their bar and beat positions.
    pub fn set_tempo_map(&mut self, events: Vec<TempoEvent>, move_midi: bool) {
        let old = self.tempo_map();
        let ev = normalize_tempo(&events, self.project.tempo);
        self.project.tempo = ev[0].bpm;
        // A single event is the plain project tempo, stored as projects always stored it.
        self.project.tempo_map = if ev.len() == 1 { Vec::new() } else { ev };
        if move_midi {
            let new = self.tempo_map();
            self.move_midi_with_tempo(&old, &new);
        }
        self.dirty = true;
        self.sync_globals();
    }

    /// Replaces the Signature track (a change at bar 1 is added when missing).
    pub fn set_signatures(&mut self, events: Vec<SigEvent>) {
        let ev = normalize_sigs(&events, (self.project.sig_num, self.project.sig_den));
        self.project.sig_num = ev[0].num;
        self.project.sig_den = ev[0].den;
        self.project.signatures = if ev.len() == 1 { Vec::new() } else { ev };
        self.dirty = true;
        self.sync_globals();
    }

    /// Sets the tempo of the event in effect at `pos` (the LCD's tempo field).
    pub fn set_tempo_at(&mut self, pos: i64, bpm: f64, move_midi: bool) {
        let mut ev = self.tempo_map().events().to_vec();
        let i = ev.iter().rposition(|e| e.pos <= pos).unwrap_or(0);
        ev[i].bpm = bpm;
        self.set_tempo_map(ev, move_midi);
    }

    /// Sets the signature in effect at 1-based `bar` (the LCD's signature menu).
    pub fn set_signature_at(&mut self, bar: u32, num: u32, den: u32) {
        let mut ev = self.tempo_map().signatures().to_vec();
        let i = ev.iter().rposition(|e| e.bar <= bar).unwrap_or(0);
        ev[i].num = num;
        ev[i].den = den;
        self.set_signatures(ev);
    }

    /// Moves MIDI regions, notes and controllers so that what sat at a bar and beat under
    /// `old` sits at the same bar and beat under `new`. Audio regions keep their time.
    pub fn move_midi_with_tempo(&mut self, old: &TempoMap, new: &TempoMap) {
        let to = |s: i64| old.remap(new, s as f64).round() as i64;
        let mut touched = Vec::new();
        // Frozen tracks can't change (their freeze file would no longer match).
        for t in self.project.tracks.iter_mut().filter(|t| t.frozen.is_none()) {
            let mut moved = false;
            for r in &mut t.regions {
                let rs = r.start;
                let RegionContent::Midi { notes, cc } = &mut r.content else { continue };
                let start = to(rs);
                for n in notes.iter_mut() {
                    let (a, b) = (to(rs + n.start), to(rs + n.start + n.length));
                    n.start = a - start;
                    n.length = (b - a).max(1);
                }
                for c in cc.iter_mut() {
                    c.start = to(rs + c.start) - start;
                }
                r.length = (to(rs + r.length) - start).max(1);
                r.loop_end = r.loop_end.map(to);
                r.start = start;
                moved = true;
            }
            if moved {
                touched.push(t.id);
            }
        }
        for id in touched {
            self.push_regions(id);
        }
        self.dirty = true;
    }

    /// `move_midi_with_tempo` between two Tempo tracks (the project's signatures apply).
    pub fn move_midi_between(&mut self, old: &[TempoEvent], new: &[TempoEvent]) {
        let p = &self.project;
        let sr = p.sample_rate as f64;
        let a = TempoMap::new(sr, old, &p.signatures, p.tempo, (p.sig_num, p.sig_den));
        let b = TempoMap::new(sr, new, &p.signatures, p.tempo, (p.sig_num, p.sig_den));
        self.move_midi_with_tempo(&a, &b);
    }
}
