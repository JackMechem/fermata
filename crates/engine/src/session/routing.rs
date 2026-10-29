//! Sends, aux buses and track outputs.
//!
//! An aux is a track of kind `Aux`. Tracks reach it through send slots (`Track::sends`, each
//! addressed by track id + slot index) or by routing their output to it (`Track::output`).
//! Edits that would make a feedback loop are refused, so the engine can always play every
//! sender before its receivers (`Project::process_order`).

use anyhow::{anyhow, Result};

use super::Session;
use crate::dsp::Cmd;
use crate::model::*;

impl Session {
    /// The lowest bus number no aux uses yet.
    pub(super) fn free_bus(&self) -> u32 {
        (1..).find(|n| !self.project.tracks.iter().any(|t| t.kind == TrackKind::Aux && t.bus == *n)).unwrap()
    }

    /// A new aux at the end of the track list, fed by the next free bus. It's named after the
    /// bus ("Aux 3" on Bus 3) unless `name` is given.
    pub fn create_aux(&mut self, name: Option<String>) -> Result<Id> {
        let id = self.add_track(TrackKind::Aux, InputSel::None, None, Some(self.project.tracks.len()))?;
        if let (Some(n), Some(t)) = (name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()), self.project.track_mut(id)) {
            t.name = n;
        }
        Ok(id)
    }

    /// Checks that `track` may play into `target` (an aux) without making a feedback loop.
    fn check_route(&self, track: Id, target: Id) -> Result<()> {
        let p = &self.project;
        let t = p.track(track).ok_or_else(|| anyhow!("no such track"))?;
        if t.kind == TrackKind::Folder {
            return Err(anyhow!("a folder stack has no audio of its own to route"));
        }
        let aux = p.track(target).filter(|a| a.kind == TrackKind::Aux).ok_or_else(|| anyhow!("sends and outputs go to auxes"))?;
        if target == track {
            return Err(anyhow!("{} can't feed itself", t.name));
        }
        if p.depends(target, track) {
            return Err(anyhow!("{} already feeds {}, so this would make a feedback loop", aux.name, t.name));
        }
        Ok(())
    }

    /// Tells the engine the processing order and every track's routing.
    fn sync_routing(&mut self) {
        self.send(Cmd::Order(self.project.process_order()));
        self.push_all_params();
        self.dirty = true;
    }

    /// Fills or clears a send slot. Refuses sends to anything but an aux, and ones that would
    /// make a feedback loop.
    pub fn set_send(&mut self, track: Id, slot: usize, send: Option<TrackSend>) -> Result<()> {
        if slot >= SEND_SLOTS {
            return Err(anyhow!("no such send slot"));
        }
        if let Some(s) = &send {
            self.check_route(track, s.bus)?;
        }
        let t = self.project.track_mut(track).ok_or_else(|| anyhow!("no such track"))?;
        if t.sends.len() < SEND_SLOTS {
            t.sends.resize(SEND_SLOTS, None);
        }
        t.sends[slot] = send.map(|s| TrackSend { level_db: s.level_db.clamp(-96.0, 6.0), ..s });
        self.sync_routing();
        Ok(())
    }

    /// Changes a send's level (dB, -96 = off, up to +6).
    pub fn set_send_level(&mut self, track: Id, slot: usize, level_db: f32) {
        let Some(t) = self.project.track_mut(track) else { return };
        let Some(Some(s)) = t.sends.get_mut(slot) else { return };
        s.level_db = level_db.clamp(-96.0, 6.0);
        // Only this track's gains change; solo and order stay as they are.
        let params = Self::track_params(&self.project, self.project.track(track).unwrap());
        self.send(Cmd::Params(track, params));
        self.dirty = true;
    }

    /// Routes a track's output: to an aux, to Stereo Out (`MASTER_ID`, which takes a summing
    /// stack's subtrack past its stack), or back to the default (None).
    pub fn set_output(&mut self, track: Id, output: Option<Id>) -> Result<()> {
        match output {
            Some(MASTER_ID) => {
                if self.project.track(track).map(|t| t.kind == TrackKind::Folder).unwrap_or(true) {
                    return Err(anyhow!("a folder stack has no output of its own"));
                }
            }
            Some(o) => self.check_route(track, o)?,
            None => {}
        }
        let t = self.project.track_mut(track).ok_or_else(|| anyhow!("no such track"))?;
        t.output = output;
        self.sync_routing();
        Ok(())
    }
}
