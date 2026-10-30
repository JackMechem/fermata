//! Reordering a strip's Audio FX inserts, and moving or copying a plug-in to another strip.
//!
//! Within a strip the running instances are moved, not reloaded, so a plug-in keeps its state,
//! its open editor and any tail it is ringing out. A move to another strip goes through copy
//! and paste, so it gets a new instance there, and its automation lanes go along onto it.

use super::*;

impl Session {
    fn inserts_mut(&mut self, track: Id) -> Option<&mut Vec<Option<PluginSlot>>> {
        if track == MASTER_ID {
            return Some(&mut self.project.master.inserts);
        }
        self.project.track_mut(track).filter(|t| t.kind != TrackKind::Folder).map(|t| &mut t.inserts)
    }

    fn strip_name(&self, track: Id) -> String {
        self.project.track(track).map(|t| t.name.clone()).unwrap_or_else(|| "Stereo Out".into())
    }

    /// Moves the insert in slot `from` to slot `to` of the same strip. The plug-ins in between
    /// shift over by one, as in Logic.
    pub fn move_insert(&mut self, track: Id, from: usize, to: usize) -> Result<()> {
        self.check_unfrozen(&[track])?;
        let slots = self.inserts_mut(track).ok_or_else(|| anyhow!("this strip has no Audio FX slots"))?;
        let n = slots.len().min(INSERT_SLOTS);
        if from >= n || to >= n {
            return Err(anyhow!("no such slot"));
        }
        if from == to {
            return Ok(());
        }
        let moved = slots.remove(from);
        slots.insert(to, moved);
        let mut order: [u8; INSERT_SLOTS] = std::array::from_fn(|i| i as u8);
        if from < to {
            order[from..=to].rotate_left(1);
        } else {
            order[to..=from].rotate_right(1);
        }
        self.send(Cmd::ReorderInserts { track, order });
        // Low Latency Mode names the plug-ins it turns off by slot.
        self.update_pdc();
        self.dirty = true;
        Ok(())
    }

    /// Puts the plug-in in `from_slot` of strip `from` into `to_slot` of strip `to`, moving it
    /// (or copying it when `copy`). Whatever is in `to_slot` shifts down a slot to make room.
    /// A moved plug-in takes its automation lanes along; a copy starts without any, as one
    /// made with Copy and Paste does.
    pub fn move_plugin(&mut self, from: Id, from_slot: usize, to: Id, to_slot: usize, copy: bool) -> Result<()> {
        if from == to && !copy {
            return self.move_insert(from, from_slot, to_slot);
        }
        self.check_unfrozen(&[from, to])?;
        let slot = self.copy_plugin(from, from_slot)?;
        self.make_room(to, to_slot)?;
        self.paste_plugin(&[to], to_slot, &slot)?;
        if !copy {
            // Making room on the same strip may have shifted the original down.
            let at = self.inserts_mut(from).and_then(|s| s.iter().position(|p| p.as_ref().map(|p| p.uid) == Some(slot.uid)));
            if let Some(at) = at {
                self.remove_plugin(from, at);
            }
            let new_uid = self.inserts_mut(to).and_then(|s| s.get(to_slot)?.as_ref().map(|p| p.uid));
            if let Some(new_uid) = new_uid {
                self.carry_automation(from, slot.uid, to, new_uid);
            }
        }
        Ok(())
    }

    /// Moves the lanes of plug-in `old` on strip `from` to strip `to` (either may be Stereo
    /// Out), where the plug-in is now instance `new`, so they keep driving the same parameters.
    fn carry_automation(&mut self, from: Id, old: Id, to: Id, new: Id) {
        if self.strip_lanes(to).is_none() {
            return;
        }
        let Some(src) = self.strip_lanes_mut(from) else { return };
        let lanes = crate::automation::take_plugin_lanes(src, old, new);
        if lanes.is_empty() {
            return;
        }
        if let Some(dst) = self.strip_lanes_mut(to) {
            dst.extend(lanes);
        }
        self.push_automation(from);
        self.push_automation(to);
    }

    /// Empties `slot` by moving the nearest free slot there, below it if there is one.
    fn make_room(&mut self, track: Id, slot: usize) -> Result<()> {
        let name = self.strip_name(track);
        let slots = self.inserts_mut(track).ok_or_else(|| anyhow!("{name} has no Audio FX slots"))?;
        let n = slots.len().min(INSERT_SLOTS);
        if slot >= n {
            return Err(anyhow!("no such slot"));
        }
        if slots[slot].is_none() {
            return Ok(());
        }
        let free = (slot + 1..n).find(|&i| slots[i].is_none()).or_else(|| (0..slot).rev().find(|&i| slots[i].is_none()));
        let free = free.ok_or_else(|| anyhow!("{name} has no free Audio FX slot"))?;
        self.move_insert(track, free, slot)
    }

    /// Undo across a reorder: when the snapshot has the same plug-ins on a strip in another
    /// order, the running instances are moved back into place instead of being reloaded.
    /// Returns `had` in its new order, for the rest of the restore to compare against.
    pub(super) fn restore_insert_order(&mut self, track: Id, had: &[Option<PluginSlot>], want: &[Option<PluginSlot>]) -> Option<Vec<Option<PluginSlot>>> {
        let uid = |s: &Option<PluginSlot>| s.as_ref().map(|s| s.uid);
        let n = had.len().min(want.len()).min(INSERT_SLOTS);
        if (0..n).all(|i| uid(&had[i]) == uid(&want[i])) {
            return None;
        }
        let mut order: [u8; INSERT_SLOTS] = std::array::from_fn(|i| i as u8);
        let mut used = [false; INSERT_SLOTS];
        let mut empty = (0..n).filter(|&j| had[j].is_none());
        for i in 0..n {
            let j = match uid(&want[i]) {
                Some(u) => (0..n).find(|&j| uid(&had[j]) == Some(u))?,
                None => empty.next()?,
            };
            order[i] = j as u8;
            used[j] = true;
        }
        // Not a pure reorder: some plug-ins were added or removed too.
        if (0..n).any(|j| !used[j]) {
            return None;
        }
        self.send(Cmd::ReorderInserts { track, order });
        let mut now = had.to_vec();
        for i in 0..n {
            now[i] = had[order[i] as usize].clone();
        }
        Some(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_latency_mode_follows_a_moved_plug_in() {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let mut prefs = s.prefs.clone();
        prefs.latency.low_latency = true;
        s.set_prefs(prefs);
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.set_plugin(t, 0, "builtin:linear-phase-eq").unwrap();
        s.set_plugin(t, 1, "builtin:gain").unwrap();
        let skip = |s: &Session| s.pdc_sent.get(&t).map(|p| p.0.low_latency_skip).unwrap_or(0);
        assert_eq!(skip(&s), 1 << 0, "the Linear Phase EQ is over the limit");
        s.move_insert(t, 0, 1).unwrap();
        assert_eq!(skip(&s), 1 << 1, "it is turned off in its new slot, not the Gain in its old one");
    }
}
