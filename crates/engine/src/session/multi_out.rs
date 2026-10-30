//! Multi-output instruments, Logic's way: the strip of an instrument with extra output buses has
//! a "+" that adds an aux for the next output. The aux is an ordinary aux with its own fader,
//! pan, plug-ins and sends, whose input (`Track::instrument_output`) names the instrument track
//! and the output bus. The main output plays through the instrument's strip as ever.
//!
//! `Project::feeds` counts the output as a route, so the instrument plays first, a key from the
//! aux can't reach the instrument's own plug-ins, solo follows it (a soloed instrument keeps its
//! auxes; a soloed aux doesn't need the instrument's strip) and offline renders of the aux play
//! the instrument as a key source does. `update_pdc` lines the aux up with the main output and
//! switches the bus on while an aux uses it, off the audio thread (`sync_output_buses` and
//! `vst3_host`'s `multi_out.rs`); `dsp/multi_out.rs` is the audio side.
//!
//! The aux survives its instrument: it keeps its settings and just loses this input when the
//! track goes (`prune_routes`; an undo brings both back). A new instrument with fewer outputs, or
//! a plug-in that's missing, leaves it silent, and the strip says so. A copy of the instrument
//! starts with no auxes; a copy of the aux takes the same output, as a second strip on it.
//! Freeze and Bounce in Place would take only the main output (and could leave the auxes with
//! nothing), so they're refused until the auxes are gone.

use super::*;

/// An output of a multi-output instrument, for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct OutputInfo {
    /// The plug-in's bus (0 is the main output).
    pub index: u32,
    pub name: String,
    pub channels: u32,
    /// The plug-in plays it now: an aux uses it.
    pub active: bool,
}

/// The outputs of the instrument on a track.
#[derive(Debug, Clone, Serialize)]
pub struct InstrumentOutputs {
    pub track: Id,
    pub outputs: Vec<OutputInfo>,
}

/// "direct_out_0" as a person would write it.
fn tidy(name: &str) -> String {
    let spaced = name.replace('_', " ");
    let mut c = spaced.trim().chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

impl Session {
    /// Instruments with extra outputs (those with just a main output aren't listed, nor are
    /// plug-ins that aren't loaded).
    pub fn instrument_outputs(&self) -> Vec<InstrumentOutputs> {
        let mut out = Vec::new();
        for t in self.project.tracks.iter().filter(|t| t.kind == TrackKind::Instrument) {
            let Some(c) = t.instrument.as_ref().and_then(|s| self.controllers.get(&s.uid)) else { continue };
            if c.output_buses().len() > 1 {
                let outputs = c.output_buses().iter().map(|b| OutputInfo { index: b.index, name: tidy(&b.name), channels: b.channels, active: b.active }).collect();
                out.push(InstrumentOutputs { track: t.id, outputs });
            }
        }
        out
    }

    /// A new aux on the instrument's next output no aux is on, named after both. Record an undo
    /// step first.
    pub fn add_output_aux(&mut self, track: Id) -> Result<Id> {
        let t = self.project.track(track).ok_or_else(|| anyhow!("no such track"))?;
        if t.kind != TrackKind::Instrument {
            return Err(anyhow!("{} isn't a software instrument track", t.name));
        }
        if t.frozen.is_some() {
            return Err(anyhow!("{} is frozen. Unfreeze it first.", t.name));
        }
        let name = t.name.clone();
        let color = t.color.clone();
        let c = t.instrument.as_ref().and_then(|s| self.controllers.get(&s.uid)).ok_or_else(|| anyhow!("{name} has no instrument with extra outputs"))?;
        if c.output_buses().len() < 2 {
            return Err(anyhow!("{name}'s instrument has no extra outputs"));
        }
        let used = |bus: u32| self.project.tracks.iter().any(|a| a.instrument_output == Some(InstrumentOutput { track, bus }));
        let next = c.output_buses().iter().skip(1).find(|b| !used(b.index)).ok_or_else(|| anyhow!("All of {name}'s outputs already have an aux"))?;
        let (bus, bus_name) = (next.index, tidy(&next.name));
        let id = self.create_aux(Some(format!("{name} {bus_name}")))?;
        if let Some(a) = self.project.track_mut(id) {
            a.color = color;
            a.instrument_output = Some(InstrumentOutput { track, bus });
        }
        self.sync_tracks();
        self.dirty = true;
        Ok(id)
    }

    /// Switches each loaded instrument's extra outputs to the ones auxes use now. Does nothing
    /// unless that changed.
    pub(super) fn sync_output_buses(&mut self) {
        let tracks = &self.project.tracks;
        for t in tracks.iter().filter(|t| t.kind == TrackKind::Instrument) {
            let Some(c) = t.instrument.as_ref().and_then(|s| self.controllers.get_mut(&s.uid)) else { continue };
            let want: Vec<u32> = tracks.iter().filter_map(|a| a.instrument_output.filter(|o| o.track == t.id).map(|o| o.bus)).collect();
            c.set_active_outputs(&want);
        }
    }

    /// Errors (naming the track) if any of `tracks` has auxes on its extra outputs, for renders
    /// that can't carry those along.
    pub(super) fn check_no_output_auxes(&self, tracks: &[Id], what: &str) -> Result<()> {
        for &id in tracks {
            if let Some(a) = self.project.tracks.iter().find(|a| a.instrument_output.is_some_and(|o| o.track == id)) {
                let name = self.project.track(id).map(|t| t.name.as_str()).unwrap_or_default();
                return Err(anyhow!("{name} has auxes on its extra outputs ({} and maybe others). {what} would only take its main output: delete those auxes first.", a.name));
            }
        }
        Ok(())
    }

    /// The instrument that feeds aux `id`, if it plays.
    pub(super) fn output_source(&self, id: Id) -> Option<Id> {
        let source = self.project.track(id)?.instrument_output?.track;
        (!self.project.is_off(source)).then_some(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        Session::new(cfg, Vec::new())
    }

    /// An instrument track and an aux that says it's on the instrument's output 3.
    fn wired(s: &mut Session) -> (Id, Id) {
        let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let aux = s.create_aux(None).unwrap();
        s.project.track_mut(aux).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 3 });
        s.sync_tracks();
        (inst, aux)
    }

    fn params(s: &Session, id: Id) -> TrackParams {
        Session::track_params(&s.project, s.project.track(id).unwrap())
    }

    #[test]
    fn the_instrument_plays_before_the_aux_and_it_is_a_route() {
        let mut s = session();
        // The aux comes first in the list, the instrument after it.
        let aux = s.create_aux(None).unwrap();
        let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        s.project.track_mut(aux).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 1 });
        s.sync_tracks();
        let order = s.project.process_order();
        let at = |id: Id| order.iter().position(|&x| x == id).unwrap();
        assert!(at(inst) < at(aux));
        assert_eq!(s.project.feeds(inst), vec![aux]);
        assert!(s.project.reaches(inst, aux) && !s.project.reaches(aux, inst));
        assert_eq!(params(&s, aux).instrument_output, Some(InstrumentOutput { track: inst, bus: 1 }));
    }

    #[test]
    fn a_key_from_the_aux_cant_reach_its_own_instrument() {
        let mut s = session();
        let (inst, aux) = wired(&mut s);
        s.set_plugin(inst, 0, "builtin:compressor").unwrap();
        assert!(s.set_sidechain(inst, 0, Some(aux)).is_err(), "that would be a feedback loop");
    }

    #[test]
    fn solo_follows_the_output_one_way() {
        let mut s = session();
        let (inst, aux) = wired(&mut s);
        let other = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.update_track(inst, TrackPatch { solo: Some(true), ..Default::default() });
        assert!(params(&s, aux).solo, "a soloed instrument keeps its auxes");
        assert!(!params(&s, other).solo);
        s.update_track(inst, TrackPatch { solo: Some(false), ..Default::default() });
        s.update_track(aux, TrackPatch { solo: Some(true), ..Default::default() });
        assert!(!params(&s, inst).solo, "a soloed aux doesn't bring the instrument's strip with it");
    }

    #[test]
    fn deleting_the_instrument_keeps_the_aux_and_undo_rewires_it() {
        let mut s = session();
        let (inst, aux) = wired(&mut s);
        s.set_plugin(aux, 0, "builtin:gain").unwrap();
        s.checkpoint("Delete", None, true);
        s.delete_track(inst);
        let a = s.project.track(aux).unwrap();
        assert_eq!(a.instrument_output, None);
        assert!(a.inserts[0].is_some(), "its plug-ins stay");
        assert_eq!(params(&s, aux).instrument_output, None);
        s.undo();
        assert_eq!(s.project.track(aux).unwrap().instrument_output, Some(InstrumentOutput { track: inst, bus: 3 }));
        assert_eq!(params(&s, aux).instrument_output, Some(InstrumentOutput { track: inst, bus: 3 }));
        assert_eq!(s.project.feeds(inst), vec![aux]);
        s.redo();
        assert_eq!(s.project.track(aux).unwrap().instrument_output, None);
    }

    #[test]
    fn deleting_the_aux_leaves_the_instrument_and_undo_brings_it_back() {
        let mut s = session();
        let (inst, aux) = wired(&mut s);
        s.checkpoint("Delete", None, true);
        s.delete_track(aux);
        assert!(s.project.feeds(inst).is_empty());
        s.undo();
        assert_eq!(s.project.feeds(inst), vec![aux]);
        assert_eq!(s.project.track(aux).unwrap().kind, TrackKind::Aux);
    }

    #[test]
    fn unused_tracks_go_together() {
        let mut s = session();
        let (inst, aux) = wired(&mut s);
        // The instrument has content: its aux is in use.
        s.create_midi_region(inst, 0, 4800).unwrap();
        assert_eq!(s.delete_unused_tracks(), 0);
        let (inst2, aux2) = wired(&mut s);
        assert_eq!(s.delete_unused_tracks(), 2);
        assert!(s.project.track(inst2).is_none() && s.project.track(aux2).is_none());
        assert!(s.project.track(inst).is_some() && s.project.track(aux).is_some());
    }

    #[test]
    fn copies_of_the_instrument_start_with_no_auxes_and_copies_of_the_aux_share_the_output() {
        let mut s = session();
        let (inst, aux) = wired(&mut s);
        let copy = s.duplicate_tracks(&[inst], true)[0];
        assert!(s.project.feeds(copy).is_empty());
        assert_eq!(s.project.feeds(inst), vec![aux]);
        let aux2 = s.duplicate_tracks(&[aux], true)[0];
        assert_eq!(s.project.track(aux2).unwrap().instrument_output, s.project.track(aux).unwrap().instrument_output);
        assert_ne!(s.project.track(aux2).unwrap().bus, s.project.track(aux).unwrap().bus);
    }

    #[test]
    fn only_an_aux_takes_an_instrument_output_and_only_from_an_instrument() {
        let mut p = Project::new("x", 48000);
        let mk = |p: &mut Project, kind: TrackKind| {
            let id = p.alloc_id();
            p.tracks.push(serde_json::from_value(serde_json::json!({
                "id": id, "kind": kind, "name": "t", "color": "", "volume_db": 0.0, "pan": 0.0, "mute": false,
                "solo": false, "armed": false, "monitor": false, "input": { "type": "none" },
            })).unwrap());
            id
        };
        let (inst, audio, aux, aux2, aux3) = (mk(&mut p, TrackKind::Instrument), mk(&mut p, TrackKind::Audio), mk(&mut p, TrackKind::Aux), mk(&mut p, TrackKind::Aux), mk(&mut p, TrackKind::Aux));
        p.track_mut(aux).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 2 });
        p.track_mut(aux2).unwrap().instrument_output = Some(InstrumentOutput { track: audio, bus: 2 });
        p.track_mut(aux3).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 0 });
        p.track_mut(audio).unwrap().instrument_output = Some(InstrumentOutput { track: inst, bus: 1 });
        p.prune_routes();
        assert!(p.track(aux).unwrap().instrument_output.is_some());
        assert!(p.track(aux2).unwrap().instrument_output.is_none() && p.track(aux3).unwrap().instrument_output.is_none());
        assert!(p.track(audio).unwrap().instrument_output.is_none());
    }

    #[test]
    fn an_old_project_loads_without_it_and_a_new_one_keeps_it() {
        let mut s = session();
        let (_, aux) = wired(&mut s);
        let json = serde_json::to_string(&s.project).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.track(aux).unwrap().instrument_output, s.project.track(aux).unwrap().instrument_output);
        let mut old = serde_json::to_value(&s.project).unwrap();
        for t in old["tracks"].as_array_mut().unwrap() {
            t.as_object_mut().unwrap().remove("instrument_output");
        }
        let back: Project = serde_json::from_value(old).unwrap();
        assert!(back.tracks.iter().all(|t| t.instrument_output.is_none()));
        // Nothing is written for tracks that don't use it.
        let plain = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        assert!(!serde_json::to_string(s.project.track(plain).unwrap()).unwrap().contains("instrument_output"));
    }

    #[test]
    fn a_new_aux_needs_a_loaded_instrument_with_extra_outputs() {
        let mut s = session();
        let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let audio = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        // The built-in synth has one output.
        let steps = s.project.tracks.len();
        assert!(s.add_output_aux(inst).unwrap_err().to_string().contains("extra outputs"));
        assert!(s.add_output_aux(audio).is_err());
        assert_eq!(s.project.tracks.len(), steps, "nothing was added");
        assert!(s.instrument_outputs().is_empty());
    }

    #[test]
    fn freeze_and_bounce_in_place_refuse_and_say_why() {
        let mut s = session();
        let (inst, aux) = wired(&mut s);
        s.create_midi_region(inst, 0, 4800).unwrap();
        for e in [s.render_freeze(&[inst], &mut |_| {}).err(), s.render_in_place(&[inst], &[], &BipOptions::default(), &mut |_| {}).err()] {
            let e = e.expect("refused").to_string();
            assert!(e.contains("extra outputs") && e.contains("delete those auxes"), "{e}");
        }
        s.delete_track(aux);
        assert!(s.check_no_output_auxes(&[inst], "Freeze").is_ok());
    }

    #[test]
    fn an_exported_aux_covers_its_instrument_and_plays_it() {
        let mut s = session();
        let (inst, aux) = wired(&mut s);
        assert!(s.export_targets().is_empty());
        s.create_midi_region(inst, 0, 4800).unwrap();
        assert_eq!(s.export_targets(), vec![inst, aux]);
        let iso = s.isolate_track(aux, true, true);
        assert_eq!(iso.keys, vec![inst], "the instrument plays, unheard, for the aux's input");
        s.update_track(inst, TrackPatch { off: Some(true), ..Default::default() });
        assert!(s.export_targets().is_empty(), "an Off instrument leaves its aux with nothing");
    }

    #[test]
    fn an_off_instrument_leaves_timing_to_the_others() {
        let mut s = session();
        let (inst, aux) = wired(&mut s);
        s.set_plugin(inst, 0, "builtin:linear-phase-eq").unwrap();
        let total = s.pdc_total();
        assert!(total > 0);
        assert!(s.pdc_report().strips.iter().any(|x| x.track == aux && x.delay == total), "the aux waits for the main output's inserts");
        s.update_track(inst, TrackPatch { off: Some(true), ..Default::default() });
        assert_eq!(s.pdc_total(), 0);
    }
}
