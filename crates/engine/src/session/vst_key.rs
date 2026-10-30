//! Side-chain inputs of VST3 plug-ins. A VST3 insert with an aux input bus is keyed like a
//! built-in effect (`PluginSlot::sidechain`, `set_sidechain`, the same routing, loop refusal and
//! renders; see `builtin.rs`); the difference is the bus.
//!
//! With no key source the aux bus stays inactive and the plug-in uses its internal key. Which
//! bus is active is set when the instance loads (`instantiate_slot`) and changes here, for every
//! way a slot's source can change: choosing one, undo, a source track deleted. VST3 only allows
//! `activateBus` on an inactive component, and `setActive` isn't for the audio thread, so:
//!
//! 1. `Cmd::VstGate`: the processor crossfades to its dry signal and stops running the plug-in.
//!    The main thread waits for that, without the audio thread waiting for anything.
//! 2. The main thread cycles `setActive` around `activateBus` (`set_key_active`).
//! 3. `Cmd::VstGate` again: the plug-in runs and, once its latency has passed, crossfades back.

use super::pdc::MAX_PLUGIN_LATENCY;
use super::*;

/// How long a plug-in gets to finish its fade: a few callbacks at the largest usual buffer. It
/// runs out only for one the engine isn't running (Low Latency Mode skips it, say).
const GATE_WAIT: Duration = Duration::from_millis(100);

impl Session {
    /// Instance ids of the loaded VST3 inserts that can be keyed, for the UI.
    pub fn vst_key_inputs(&self) -> Vec<Id> {
        let p = &self.project;
        let tracks = p.tracks.iter().flat_map(|t| t.inserts.iter());
        tracks.chain(p.master.inserts.iter()).flatten().filter(|s| self.controllers.get(&s.uid).is_some_and(|c| c.has_key_input())).map(|s| s.uid).collect()
    }

    /// Whether a loaded VST3's aux input is active (the plug-in hears its key).
    pub fn vst_key_active(&self, uid: Id) -> Option<bool> {
        self.controllers.get(&uid).map(|c| c.key_active())
    }

    /// Sets a loaded VST3's parameter as its editor would (normalized 0..1), without touching
    /// automation: for headless checks.
    pub fn edit_vst_param(&self, uid: Id, param: u32, value: f64) {
        if let Some(c) = self.controllers.get(&uid) {
            c.edit_param(param, value);
        }
    }

    /// A loaded VST3's parameter value (normalized 0..1).
    pub fn vst_param(&self, uid: Id, param: u32) -> Option<f64> {
        self.controllers.get(&uid).and_then(|c| c.param_value(param))
    }

    /// Whether the plug-in in a slot has a side-chain input. Instruments take none.
    pub(super) fn takes_key(&mut self, track: Id, slot: usize) -> bool {
        if slot == usize::MAX {
            return false;
        }
        let Some(Some(s)) = self.slot_ref(track, slot) else { return false };
        match crate::fx::find(&s.plugin_id) {
            Some(info) => info.sidechain,
            None => {
                let uid = s.uid;
                self.controllers.get(&uid).is_some_and(|c| c.has_key_input())
            }
        }
    }

    /// Brings every VST3's aux input in line with its slot's key source. Costs nothing unless
    /// one differs.
    pub(super) fn sync_vst_keys(&mut self) {
        let p = &self.project;
        let tracks = p.tracks.iter().map(|t| (t.id, &t.inserts)).chain(std::iter::once((MASTER_ID, &p.master.inserts)));
        let mut changes = Vec::new();
        for (track, inserts) in tracks {
            for s in inserts.iter().flatten() {
                if self.controllers.get(&s.uid).is_some_and(|c| c.has_key_input() && c.key_active() != s.sidechain.is_some()) {
                    changes.push((track, s.uid, s.sidechain.is_some()));
                }
            }
        }
        for (track, uid, on) in changes {
            self.switch_key(track, uid, on);
        }
    }

    /// Activates or deactivates a running plug-in's aux input. Leaves it as it was if the audio
    /// thread doesn't answer, which the next `sync_vst_keys` tries again.
    fn switch_key(&mut self, track: Id, uid: Id, on: bool) {
        self.send(Cmd::VstGate { track, uid, gate: VstGate::Close });
        let deadline = Instant::now() + GATE_WAIT;
        while self.audio.is_some() && Instant::now() < deadline && !self.controllers.get(&uid).is_some_and(|c| c.parked()) {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.send(Cmd::VstGate { track, uid, gate: VstGate::Shut });
        self.wait_for_engine();
        // Stopped is what makes the component safe to touch, whatever else the engine has done.
        let Some(c) = self.controllers.get_mut(&uid).filter(|c| c.parked()) else {
            log::warn!("side-chain: the audio thread didn't answer");
            self.send(Cmd::VstGate { track, uid, gate: VstGate::Open(0) });
            return;
        };
        let done = c.set_key_active(on);
        let settle = c.latency().min(MAX_PLUGIN_LATENCY);
        self.send(Cmd::VstGate { track, uid, gate: VstGate::Open(settle) });
        if let Err(e) = done {
            self.warning = Some(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        Session::new(cfg, Vec::new())
    }

    fn missing_slot(uid: Id, source: Option<Id>) -> PluginSlot {
        PluginSlot { uid, plugin_id: "/nowhere/Gone.vst3|0123456789ABCDEF".into(), name: "Gone".into(), bypass: false, state: None, params: Vec::new(), sidechain: source, sidechain_pre: false }
    }

    #[test]
    fn a_missing_plug_in_keeps_its_key_but_takes_no_new_one() {
        let mut s = session();
        let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        let b = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        let uid = s.project.alloc_id();
        s.project.track_mut(a).unwrap().inserts[0] = Some(missing_slot(uid, Some(b)));
        s.update_pdc();
        assert_eq!(s.project.track(a).unwrap().inserts[0].as_ref().unwrap().sidechain, Some(b), "syncing leaves it alone");
        assert!(s.vst_key_inputs().is_empty());
        assert!(s.set_sidechain(a, 0, Some(b)).is_err());
        s.set_sidechain(a, 0, None).unwrap();
        assert_eq!(s.project.track(a).unwrap().inserts[0].as_ref().unwrap().sidechain, None);
    }

    #[test]
    fn only_inserts_with_an_input_take_a_key() {
        let mut s = session();
        let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        let b = s.add_track(TrackKind::Instrument, InputSel::None, Some(BUILTIN_SYNTH.into()), None).unwrap();
        s.set_plugin(a, 0, "builtin:reverb").unwrap();
        assert!(s.set_sidechain(a, 0, Some(b)).is_err(), "a built-in effect without a side-chain");
        assert!(s.set_sidechain_pre(a, 0, true).is_err());
        assert!(s.set_sidechain(b, usize::MAX, Some(a)).is_err(), "an instrument");
        s.set_plugin(a, 0, "builtin:compressor").unwrap();
        s.set_sidechain(a, 0, Some(b)).unwrap();
        assert!(s.set_sidechain(a, 7, Some(b)).is_err(), "an empty slot");
    }

    #[test]
    fn a_slot_saved_before_side_chains_has_no_key() {
        let slot: PluginSlot = serde_json::from_str(r#"{"uid":3,"plugin_id":"/x.vst3|AB","name":"X"}"#).unwrap();
        assert_eq!((slot.sidechain, slot.sidechain_pre), (None, false));
    }
}
