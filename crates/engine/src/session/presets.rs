//! Copying and pasting plug-ins and Channel EQ settings, and loading and saving presets (see
//! `crate::presets` for where they are stored).
//!
//! The clipboard lives in the UI, so it survives opening another project: copying hands the UI
//! the slot with its complete settings (a VST3's state captured from its controller), and
//! pasting makes a new instance from that.

use super::*;
use crate::presets::{self, Preset, CHANNEL_EQ};

/// The slot index that addresses a strip's Channel EQ in the preset commands.
pub const EQ_SLOT: usize = usize::MAX - 1;

impl Session {
    /// A slot with everything needed to paste it elsewhere: its settings, and a VST3's state
    /// as it is right now.
    pub fn copy_plugin(&self, track: Id, slot: usize) -> Result<PluginSlot> {
        let s = self.slot_get(track, slot).ok_or_else(|| anyhow!("no plug-in in that slot"))?;
        let mut copy = s.clone();
        if let Some(c) = self.controllers.get(&s.uid) {
            copy.state = Some(encode_state(&c.save_state()));
        }
        Ok(copy)
    }

    fn slot_get(&self, track: Id, slot: usize) -> Option<&PluginSlot> {
        if track == MASTER_ID {
            return self.project.master.inserts.get(slot)?.as_ref();
        }
        let t = self.project.track(track)?;
        if slot == usize::MAX {
            t.instrument.as_ref()
        } else {
            t.inserts.get(slot)?.as_ref()
        }
    }

    /// Pastes a copied slot into `slot` (the instrument slot when `usize::MAX`) of each track,
    /// as new instances with the copy's settings. A side-chain only comes along where its
    /// source exists in this project and keys without a loop.
    pub fn paste_plugin(&mut self, tracks: &[Id], slot: usize, copy: &PluginSlot) -> Result<()> {
        self.check_unfrozen(tracks)?;
        let builtin_fx = crate::fx::find(&copy.plugin_id).is_some();
        let instrument = if copy.plugin_id == BUILTIN_SYNTH {
            true
        } else if builtin_fx {
            false
        } else {
            self.find_plugin(&copy.plugin_id).map(|d| d.is_instrument).ok_or_else(|| anyhow!("{} isn't installed", copy.name))?
        };
        if instrument != (slot == usize::MAX) {
            return Err(anyhow!(if instrument { "An instrument goes in the instrument slot" } else { "An audio effect goes in an Audio FX slot" }));
        }
        for &track in tracks {
            if track != MASTER_ID {
                let t = self.project.track(track).ok_or_else(|| anyhow!("no such track"))?;
                if t.kind == TrackKind::Folder {
                    return Err(anyhow!("Folder stacks have no plug-ins"));
                }
                if instrument && t.kind != TrackKind::Instrument {
                    return Err(anyhow!("{} has no instrument slot", t.name));
                }
            } else if instrument {
                return Err(anyhow!("Stereo Out has no instrument slot"));
            }
            let sidechain = copy.sidechain.filter(|&src| {
                src != track && self.project.track(src).map(|t| t.kind != TrackKind::Folder).unwrap_or(false) && (track == MASTER_ID || !self.project.depends(track, src))
            });
            let new_slot = PluginSlot { uid: self.project.alloc_id(), sidechain, ..copy.clone() };
            let ins = self.instantiate_slot(&new_slot)?;
            // VST3 state is only kept in the document while the plug-in is unloaded.
            let new_slot = PluginSlot { state: if self.controllers.contains_key(&new_slot.uid) { None } else { new_slot.state }, ..new_slot };
            let old = self.slot_ref(track, slot).ok_or_else(|| anyhow!("no such slot"))?.replace(new_slot);
            if let Some(o) = old {
                if let Some(mut c) = self.controllers.remove(&o.uid) {
                    c.close_editor();
                }
            }
            if slot == usize::MAX {
                self.send(Cmd::Instrument { track, insert: Some(ins) });
            } else {
                self.send(Cmd::Insert { track, slot, insert: Some(ins) });
            }
        }
        self.send(Cmd::Order(self.project.process_order()));
        self.update_pdc();
        self.dirty = true;
        Ok(())
    }

    /// A strip's Channel EQ (MASTER_ID for Stereo Out).
    pub fn eq_of(&self, track: Id) -> Option<ChannelEq> {
        if track == MASTER_ID {
            Some(self.project.master.eq.clone())
        } else {
            self.project.track(track).map(|t| t.eq.clone())
        }
    }

    /// Pastes Channel EQ settings onto each strip.
    pub fn paste_eq(&mut self, tracks: &[Id], eq: &ChannelEq) -> Result<()> {
        self.check_unfrozen(tracks)?;
        for &t in tracks {
            if t != MASTER_ID && self.project.track(t).is_none() {
                return Err(anyhow!("no such track"));
            }
            self.set_eq(t, eq.clone());
        }
        Ok(())
    }

    /// Applies a preset to a built-in effect's slot, or to the strip's Channel EQ when `slot`
    /// is [`EQ_SLOT`]. A built-in effect's parameters the preset doesn't list go to their
    /// defaults.
    pub fn load_preset(&mut self, track: Id, slot: usize, effect: &str, name: &str, factory: bool) -> Result<()> {
        let p = presets::get(&presets::user_root(), effect, name, factory)?;
        self.apply_preset(track, slot, &p)
    }

    pub fn apply_preset(&mut self, track: Id, slot: usize, p: &Preset) -> Result<()> {
        self.check_unfrozen(&[track])?;
        if slot == EQ_SLOT {
            let eq = p.eq.clone().filter(|_| p.effect == CHANNEL_EQ).ok_or_else(|| anyhow!("\"{}\" isn't a Channel EQ preset", p.name))?;
            if track != MASTER_ID && self.project.track(track).is_none() {
                return Err(anyhow!("no such track"));
            }
            self.set_eq(track, eq);
            return Ok(());
        }
        let s = self.slot_ref(track, slot).and_then(|s| s.as_mut()).ok_or_else(|| anyhow!("no plug-in in that slot"))?;
        if s.plugin_id != p.effect {
            return Err(anyhow!("\"{}\" is a preset for another plug-in", p.name));
        }
        let info = crate::fx::find(&s.plugin_id).ok_or_else(|| anyhow!("{} has no presets here", s.name))?;
        s.params = info.params.iter().map(|spec| (spec.id, p.values.iter().find(|v| v.0 == spec.id).map(|v| crate::dsp::builtin_clamp(spec, v.1)).unwrap_or(spec.default))).collect();
        let (uid, values) = (s.uid, s.params.clone());
        for (id, value) in values {
            self.send(Cmd::FxParam { track, uid, id, value });
        }
        if self.builtin_latency_changed() {
            self.update_pdc();
        }
        self.dirty = true;
        Ok(())
    }

    /// Saves the settings of a built-in effect's slot (or the strip's Channel EQ, with
    /// [`EQ_SLOT`]) as a user preset. Returns the name it was saved under.
    pub fn save_preset(&self, track: Id, slot: usize, name: &str, overwrite: bool) -> Result<String> {
        let p = if slot == EQ_SLOT {
            let eq = self.eq_of(track).ok_or_else(|| anyhow!("no such track"))?;
            Preset { name: name.into(), effect: CHANNEL_EQ.into(), version: 0, values: Vec::new(), eq: Some(eq) }
        } else {
            let s = self.slot_get(track, slot).ok_or_else(|| anyhow!("no plug-in in that slot"))?;
            if crate::fx::find(&s.plugin_id).is_none() {
                return Err(anyhow!("Presets are saved in {}'s own window", s.name));
            }
            Preset { name: name.into(), effect: s.plugin_id.clone(), version: 0, values: self.builtin_params(s.uid), eq: None }
        };
        presets::write_user(&presets::user_root(), &p, overwrite)
    }
}
