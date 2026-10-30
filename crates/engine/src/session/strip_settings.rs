//! Copying, pasting, saving and loading channel strip settings, and Reset Channel Strip (see
//! `crate::strip_settings` for what a setting holds and where saved ones live).
//!
//! Applying a setting replaces the target's Audio FX inserts and Channel EQ and, on an
//! instrument track, its instrument when the setting has one. Every plug-in comes in as a new
//! instance, as with Paste Plug-in. A VST3 that isn't installed here (or fails to load) stays
//! in its slot with its saved state, shown as missing, like one in a project opened on another
//! computer. Sends, routing, volume and pan are left alone, as in Logic.

use std::path::Path;

use super::*;
use crate::strip_settings::{self, StripSetting};

impl Session {
    /// A strip's setting (MASTER_ID for Stereo Out), with each VST3's state as it is right now.
    pub fn copy_strip(&self, track: Id) -> Result<StripSetting> {
        let capture = |s: &PluginSlot| {
            let mut c = s.clone();
            if let Some(ctrl) = self.controllers.get(&s.uid) {
                c.state = Some(encode_state(&ctrl.save_state()));
            }
            c
        };
        if track == MASTER_ID {
            let m = &self.project.master;
            return Ok(StripSetting { name: "Stereo Out".into(), version: 0, instrument: None, inserts: m.inserts.iter().map(|s| s.as_ref().map(capture)).collect(), eq: Some(m.eq.clone()) });
        }
        let t = self.project.track(track).ok_or_else(|| anyhow!("no such track"))?;
        if t.kind == TrackKind::Folder {
            return Err(anyhow!("A folder stack has no channel strip settings"));
        }
        Ok(StripSetting {
            name: t.name.clone(),
            version: 0,
            instrument: t.instrument.as_ref().filter(|_| t.kind == TrackKind::Instrument).map(capture),
            inserts: t.inserts.iter().map(|s| s.as_ref().map(capture)).collect(),
            eq: Some(t.eq.clone()),
        })
    }

    /// Applies a setting to each strip (MASTER_ID for Stereo Out).
    pub fn paste_strip(&mut self, tracks: &[Id], setting: &StripSetting) -> Result<()> {
        self.apply_strip(tracks, setting, false)
    }

    /// Logic's Reset Channel Strip: removes the instrument and every Audio FX plug-in, sets the
    /// Channel EQ flat and the fader and pan to their defaults.
    pub fn reset_strip(&mut self, tracks: &[Id]) -> Result<()> {
        let empty = StripSetting { eq: Some(ChannelEq::default()), ..Default::default() };
        self.apply_strip(tracks, &empty, true)?;
        for &t in tracks {
            if t == MASTER_ID {
                self.set_master_volume(0.0);
            } else {
                self.update_track(t, TrackPatch { volume_db: Some(0.0), pan: Some(0.0), ..Default::default() });
            }
        }
        Ok(())
    }

    /// Saves a strip's setting in `root` (see [`strip_settings::user_root`]). Returns the name
    /// it was saved under.
    pub fn save_strip_setting(&self, root: &Path, track: Id, name: &str, overwrite: bool) -> Result<String> {
        strip_settings::write(root, &self.copy_strip(track)?, name, overwrite)
    }

    /// Loads a saved setting onto each strip.
    pub fn load_strip_setting(&mut self, root: &Path, tracks: &[Id], name: &str) -> Result<()> {
        let s = strip_settings::read(root, name)?;
        self.paste_strip(tracks, &s)
    }

    /// `reset` also empties the instrument slot; otherwise a setting without an instrument
    /// leaves the target's.
    fn apply_strip(&mut self, tracks: &[Id], setting: &StripSetting, reset: bool) -> Result<()> {
        self.check_unfrozen(tracks)?;
        // Refuse before changing anything, so a paste never half happens.
        for &t in tracks {
            if t == MASTER_ID {
                continue;
            }
            match self.project.track(t) {
                None => return Err(anyhow!("no such track")),
                Some(tr) if tr.kind == TrackKind::Folder => return Err(anyhow!("A folder stack has no channel strip settings")),
                _ => {}
            }
        }
        for &track in tracks {
            let instrument_track = track != MASTER_ID && self.project.track(track).map(|t| t.kind == TrackKind::Instrument).unwrap_or(false);
            if instrument_track && (reset || setting.instrument.is_some()) {
                self.put_strip_slot(track, usize::MAX, setting.instrument.as_ref())?;
            }
            let slots = if track == MASTER_ID { self.project.master.inserts.len() } else { self.project.track(track).map(|t| t.inserts.len()).unwrap_or(0) };
            for k in 0..slots {
                self.put_strip_slot(track, k, setting.inserts.get(k).and_then(|s| s.as_ref()))?;
            }
            if let Some(eq) = &setting.eq {
                self.set_eq(track, eq.clone());
            }
        }
        self.send(Cmd::Order(self.project.process_order()));
        self.update_pdc();
        self.dirty = true;
        Ok(())
    }

    /// Puts a new instance of `from` (or nothing) into a slot (`usize::MAX`: the instrument).
    fn put_strip_slot(&mut self, track: Id, slot: usize, from: Option<&PluginSlot>) -> Result<()> {
        if from.is_none() && self.slot_ref(track, slot).map(|s| s.is_none()).unwrap_or(true) {
            return Ok(());
        }
        let new_slot = from.map(|s| {
            // A side-chain only comes along where its source exists and keys without a loop.
            let sidechain = s.sidechain.filter(|&src| src != track && self.project.track(src).map(|t| t.kind != TrackKind::Folder).unwrap_or(false) && (track == MASTER_ID || !self.project.depends(track, src)));
            PluginSlot { uid: self.project.alloc_id(), sidechain, ..s.clone() }
        });
        let ins = match &new_slot {
            Some(s) => match self.instantiate_slot(s) {
                Ok(i) => Some(i),
                Err(e) => {
                    // Kept with its state, as missing (see `missing_plugins`).
                    log::warn!("channel strip setting: {e}");
                    None
                }
            },
            None => None,
        };
        // A running VST3's state lives in its controller; the document keeps it only while the
        // plug-in is unloaded.
        let new_slot = new_slot.map(|s| PluginSlot { state: if self.controllers.contains_key(&s.uid) { None } else { s.state.clone() }, ..s });
        let old = std::mem::replace(self.slot_ref(track, slot).ok_or_else(|| anyhow!("no such slot"))?, new_slot);
        if let Some(o) = old {
            if let Some(mut c) = self.controllers.remove(&o.uid) {
                c.close_editor();
            }
        }
        if slot == usize::MAX {
            self.send(Cmd::Instrument { track, insert: ins });
        } else {
            self.send(Cmd::Insert { track, slot, insert: ins });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;

    fn session(tag: &str) -> Session {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        s.dir = std::env::temp_dir().join(format!("fermata-strip-settings-{tag}-{}", std::process::id()));
        s
    }

    fn ids(s: &Session, track: Id) -> Vec<Option<String>> {
        s.project.track(track).unwrap().inserts.iter().map(|x| x.as_ref().map(|x| x.plugin_id.clone())).collect()
    }

    fn uid(s: &Session, track: Id, slot: usize) -> Id {
        s.project.track(track).unwrap().inserts[slot].as_ref().unwrap().uid
    }

    /// The setting as it would come back from the UI's clipboard or a file.
    fn through_json(st: &StripSetting) -> StripSetting {
        serde_json::from_str(&serde_json::to_string(st).unwrap()).unwrap()
    }

    #[test]
    fn copy_and_paste_round_trip_through_json() {
        let mut s = session("rt");
        let src = s.add_track(TrackKind::Instrument, InputSel::Mono(0), Some(BUILTIN_SYNTH.into()), None).unwrap();
        let audio = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let inst = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
        let inst_before = s.project.track(inst).unwrap().instrument.as_ref().map(|i| i.uid);
        s.set_plugin(src, usize::MAX, BUILTIN_SYNTH).unwrap();
        s.set_plugin(src, 0, "builtin:compressor").unwrap();
        s.set_plugin(src, 2, "builtin:limiter").unwrap();
        s.set_builtin_param(src, 0, crate::fx::compressor::param::THRESHOLD, -30.0).unwrap();
        s.set_bypass(src, 2, true);
        let mut eq = ChannelEq::default();
        eq.bands[4].on = true;
        eq.bands[4].gain = 5.0;
        s.set_eq(src, eq);
        // Something already on the targets, which the paste replaces.
        s.set_plugin(audio, 1, "builtin:reverb").unwrap();
        s.set_plugin(audio, 5, "builtin:delay").unwrap();

        let st = through_json(&s.copy_strip(src).unwrap());
        assert_eq!(st.name, s.project.track(src).unwrap().name);
        s.checkpoint("Paste Channel Strip", None, true);
        s.paste_strip(&[audio, inst], &st).unwrap();

        for t in [audio, inst] {
            assert_eq!(ids(&s, t)[..3], [Some("builtin:compressor".to_string()), None, Some("builtin:limiter".to_string())]);
            assert!(ids(&s, t)[3..].iter().all(|x| x.is_none()), "the target's other plug-ins are gone");
            assert_ne!(uid(&s, t, 0), uid(&s, src, 0), "new instances");
            assert_eq!(s.builtin_params(uid(&s, t, 0)), s.builtin_params(uid(&s, src, 0)), "with the source's values");
            let tr = s.project.track(t).unwrap();
            assert!(tr.inserts[2].as_ref().unwrap().bypass, "bypass comes along");
            assert_eq!(tr.eq.bands[4].gain, 5.0);
        }
        assert_ne!(uid(&s, audio, 0), uid(&s, inst, 0));
        assert!(s.project.track(audio).unwrap().instrument.is_none(), "an audio track gets no instrument");
        assert_eq!(s.project.track(inst).unwrap().instrument.as_ref().map(|i| i.plugin_id.as_str()), Some(BUILTIN_SYNTH));
        assert_ne!(s.project.track(inst).unwrap().instrument.as_ref().map(|i| i.uid), inst_before, "the instrument is replaced");

        // One undo step brings both targets back.
        s.undo();
        assert_eq!(ids(&s, audio)[1].as_deref(), Some("builtin:reverb"));
        assert_eq!(ids(&s, audio)[5].as_deref(), Some("builtin:delay"));
        assert!(ids(&s, inst).iter().all(|x| x.is_none()));
        assert_eq!(s.project.track(inst).unwrap().instrument.as_ref().map(|i| i.uid), inst_before);
        assert_eq!(s.project.track(audio).unwrap().eq.bands[4].gain, ChannelEq::default().bands[4].gain);
    }

    #[test]
    fn an_audio_setting_keeps_the_targets_instrument_and_reset_clears_it() {
        let mut s = session("inst");
        let audio = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let inst = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
        s.set_plugin(inst, usize::MAX, BUILTIN_SYNTH).unwrap();
        s.set_plugin(inst, 0, "builtin:reverb").unwrap();
        s.set_plugin(audio, 0, "builtin:noise-gate").unwrap();
        let st = through_json(&s.copy_strip(audio).unwrap());
        assert!(st.instrument.is_none());
        s.paste_strip(&[inst], &st).unwrap();
        assert_eq!(ids(&s, inst)[0].as_deref(), Some("builtin:noise-gate"));
        assert!(s.project.track(inst).unwrap().instrument.is_some(), "no instrument in the setting: the target keeps its own");

        s.update_track(inst, TrackPatch { volume_db: Some(-8.0), pan: Some(0.5), ..Default::default() });
        s.reset_strip(&[inst, MASTER_ID]).unwrap();
        let t = s.project.track(inst).unwrap();
        assert!(t.instrument.is_none() && t.inserts.iter().all(|x| x.is_none()));
        assert_eq!((t.volume_db, t.pan), (0.0, 0.0));
        let folder = s.create_stack(&[audio], false).unwrap();
        assert!(s.paste_strip(&[inst, folder], &st).is_err(), "a folder stack is refused");
        assert!(s.project.track(inst).unwrap().inserts[0].is_none(), "and nothing was applied");
        assert!(s.copy_strip(folder).is_err());
    }

    #[test]
    fn missing_plug_ins_stay_as_missing_slots() {
        let mut s = session("missing");
        let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let gone = PluginSlot { uid: 1, plugin_id: "/nowhere/Gone.vst3|0123456789ABCDEF".into(), name: "Gone".into(), bypass: false, state: Some("AAAA:".into()), params: Vec::new(), sidechain: None, sidechain_pre: false };
        let st = StripSetting { inserts: vec![Some(gone), Some(PluginSlot { uid: 2, plugin_id: "builtin:limiter".into(), name: "Limiter".into(), bypass: false, state: None, params: Vec::new(), sidechain: Some(a), sidechain_pre: false })], ..Default::default() };
        s.paste_strip(&[a], &through_json(&st)).unwrap();
        let t = s.project.track(a).unwrap();
        let kept = t.inserts[0].as_ref().unwrap();
        assert_eq!((kept.name.as_str(), kept.state.as_deref()), ("Gone", Some("AAAA:")), "the slot and its state are kept");
        assert!(t.inserts[1].as_ref().unwrap().sidechain.is_none(), "a strip doesn't key itself");
        assert_eq!(t.eq.bands.len(), ChannelEq::default().bands.len(), "no EQ in the setting leaves the EQ alone");
        let missing: Vec<&str> = s.missing_plugins().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(missing, vec!["Gone"]);
    }

    #[test]
    fn saved_settings_load_onto_strips() {
        let mut s = session("files");
        let root = s.dir.join("channel-strips");
        let _ = std::fs::remove_dir_all(&root);
        let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.set_plugin(a, 0, "builtin:deesser").unwrap();
        let name = s.save_strip_setting(&root, a, "Vox", false).unwrap();
        assert!(s.save_strip_setting(&root, a, "Vox", false).is_err());
        s.load_strip_setting(&root, &[b, MASTER_ID], &name).unwrap();
        assert_eq!(ids(&s, b)[0].as_deref(), Some("builtin:deesser"));
        assert_eq!(s.project.master.inserts[0].as_ref().map(|x| x.plugin_id.as_str()), Some("builtin:deesser"));
        let _ = std::fs::remove_dir_all(&s.dir);
    }
}
