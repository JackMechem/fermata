//! Track Freeze, as in Logic: a track's instrument, Channel EQ and plug-ins are rendered
//! (pre-fader) into a freeze file, which then plays in their place while the plug-ins are
//! unloaded. Fader, pan, sends and their automation stay live; plug-in automation is baked in.
//!
//! The plug-ins' settings are kept in the project (`PluginSlot::state`), so unfreezing loads
//! them again exactly as they were. While a track is frozen its regions, plug-ins and Channel
//! EQ can't change: the session refuses those edits (`check_unfrozen`, `frozen_guard`).
//!
//! Like Bounce in Place, freezing has two steps: the render (`render_freeze`, which may run on
//! a worker thread) and the edit (`place_freeze`, on the main thread, as one undo step).

use super::*;

/// Freeze files live here inside the project folder.
const FREEZE_DIR: &str = "Freeze Files";
/// How long a frozen track may ring on after its last region (silence is trimmed).
const FREEZE_TAIL_SECS: f64 = 4.0;
/// Below about -90 dBFS the tail counts as silent.
const SILENCE: f32 = 3.2e-5;

/// One track's freeze render, written to disk and waiting to be placed.
pub struct FreezeRender {
    track: Id,
    start: i64,
    path: PathBuf,
    clip: Clip,
}

impl Session {
    pub fn is_frozen(&self, track: Id) -> bool {
        self.project.track(track).map(|t| t.frozen.is_some()).unwrap_or(false)
    }

    /// Errors (naming the track) if any of `tracks` is frozen.
    pub fn check_unfrozen(&self, tracks: &[Id]) -> Result<()> {
        match tracks.iter().filter_map(|id| self.project.track(*id)).find(|t| t.frozen.is_some()) {
            Some(t) => Err(anyhow!("{} is frozen. Unfreeze it to edit its regions and plug-ins.", t.name)),
            None => Ok(()),
        }
    }

    /// How many plug-in instances are loaded (built-in synths aside).
    pub fn plugin_instances(&self) -> usize {
        self.controllers.len()
    }

    /// The Channel EQ the engine runs for a track: none while frozen, since the freeze file
    /// already has it.
    pub(super) fn eq_coeffs(&self, t: &Track) -> crate::eq::EqCoeffs {
        if t.frozen.is_some() {
            crate::eq::EqCoeffs::default()
        } else {
            crate::eq::coefficients(&t.eq, self.sample_rate())
        }
    }

    /// A frozen track's playback list: just its freeze file.
    pub(super) fn freeze_dsp(&self, t: &Track) -> Option<Arc<Vec<RegionDsp>>> {
        let f = t.frozen?;
        let clip = self.clips.get(&f.clip)?.clone();
        let length = clip.frames() as i64;
        Some(Arc::new(vec![RegionDsp::Audio { start: f.start, length, offset: 0, gain: 1.0, clip, fade_in: (f.start, 0), fade_out: (f.start + length, 0), laws: Default::default() }]))
    }

    /// Freeze, step one: renders each track in `ids` through its instrument, Channel EQ and
    /// plug-ins (not its fader, pan or sends) into the project's Freeze Files folder.
    pub fn render_freeze(&mut self, ids: &[Id], progress: &mut dyn FnMut(f64)) -> Result<Vec<FreezeRender>> {
        self.check_on(ids)?;
        let sr = self.sample_rate();
        let tail = (FREEZE_TAIL_SECS * sr).round() as i64;
        let n = ids.len().max(1) as f64;
        let mut out = Vec::new();
        for (k, &id) in ids.iter().enumerate() {
            let t = self.project.track(id).ok_or_else(|| anyhow!("no such track"))?;
            if !matches!(t.kind, TrackKind::Audio | TrackKind::Instrument) {
                return Err(anyhow!("Only audio and software instrument tracks can be frozen"));
            }
            if t.frozen.is_some() {
                return Err(anyhow!("{} is already frozen", t.name));
            }
            let map = self.tempo_map();
            let span = t.regions.iter().filter(|r| !r.muted).map(|r| crate::region_params::bounds(r, &map)).fold(None, |acc: Option<(i64, i64)>, (s, e)| Some(acc.map_or((s, e), |(a, b)| (a.min(s), b.max(e)))));
            let Some((from, to)) = span else { return Err(anyhow!("{} has no regions to freeze", t.name)) };
            let name = t.name.clone();
            let iso = self.isolate_track(id, false, true);
            // Built-in reverbs and delays may ring longer than the usual tail.
            let tail = tail.max((self.builtin_tail_secs(id).min(60.0) * sr).round() as i64);
            let mut audio = self.render(from, to, tail, Some(iso), &mut |f| progress((k as f64 + f) / n))?;
            // Keep the ringing tail, not the silence after it.
            let keep = (to - from) as usize;
            let len = audio.first().map(|c| c.len()).unwrap_or(0);
            let last = (keep..len).rev().find(|&i| audio.iter().any(|c| c[i].abs() > SILENCE)).map(|i| i + 1).unwrap_or(keep);
            audio.iter_mut().for_each(|c| c.truncate(last.min(len)));
            let path = self.freeze_path(&name);
            crate::decode::write_wav(&path, sr as u32, &audio)?;
            out.push(FreezeRender { track: id, start: from, clip: Clip::from_channels(path.clone(), sr as u32, audio), path });
        }
        Ok(out)
    }

    /// An unused file name for a track's freeze file.
    fn freeze_path(&self, track_name: &str) -> PathBuf {
        let clean: String = track_name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
        let dir = self.dir.join(FREEZE_DIR);
        (1..).map(|i| if i == 1 { dir.join(format!("{clean} Freeze.wav")) } else { dir.join(format!("{clean} Freeze {i}.wav")) }).find(|p| !p.exists()).unwrap()
    }

    /// Freeze, step two: each track plays its render and unloads its plug-ins, keeping their
    /// settings for unfreezing. Record an undo step first, capturing plug-ins.
    pub fn place_freeze(&mut self, renders: Vec<FreezeRender>) -> Result<()> {
        for r in renders {
            let Some(t) = self.project.track(r.track).filter(|t| t.frozen.is_none()).cloned() else {
                let _ = std::fs::remove_file(&r.path);
                continue;
            };
            let states: Vec<(Id, String)> = t.inserts.iter().flatten().chain(t.instrument.iter()).filter_map(|s| self.controllers.get(&s.uid).map(|c| (s.uid, encode_state(&c.save_state())))).collect();
            let clip = self.add_clip(&r.path, r.clip);
            let tm = self.project.track_mut(r.track).unwrap();
            for slot in tm.inserts.iter_mut().flatten().chain(tm.instrument.iter_mut()) {
                if let Some((_, st)) = states.iter().find(|(uid, _)| *uid == slot.uid) {
                    slot.state = Some(st.clone());
                }
            }
            tm.frozen = Some(FreezeInfo { clip, start: r.start });
            tm.armed = false;
            self.unload_plugins(&t);
            self.push_regions(r.track);
            self.send(Cmd::Eq(r.track, crate::eq::EqCoeffs::default()));
        }
        // Arming changed, and the frozen tracks' latency went with their plug-ins.
        self.push_all_params();
        self.update_midi_target();
        self.dirty = true;
        self.request_autosave();
        Ok(())
    }

    /// Both freeze steps, for callers already on the main thread.
    pub fn freeze_tracks(&mut self, ids: &[Id], progress: &mut dyn FnMut(f64)) -> Result<()> {
        let renders = self.render_freeze(ids, progress)?;
        self.place_freeze(renders)
    }

    /// Loads a frozen track's plug-ins again (with the settings they had) and deletes its
    /// freeze file. The clip stays in memory, so undo can freeze the track again.
    pub fn unfreeze_track(&mut self, id: Id) -> Result<()> {
        let t = self.project.track(id).ok_or_else(|| anyhow!("no such track"))?;
        let f = t.frozen.ok_or_else(|| anyhow!("{} isn't frozen", t.name))?;
        self.project.track_mut(id).unwrap().frozen = None;
        if let Some(i) = self.project.clips.iter().position(|c| c.id == f.clip) {
            let c = self.project.clips.remove(i);
            let path = if Path::new(&c.path).is_absolute() { PathBuf::from(&c.path) } else { self.dir.join(&c.path) };
            let _ = std::fs::remove_file(path);
        }
        let t = self.project.track(id).unwrap().clone();
        self.load_plugins(&t);
        self.push_regions(id);
        let eq = self.eq_coeffs(&t);
        self.send(Cmd::Eq(id, eq));
        self.update_pdc();
        self.dirty = true;
        Ok(())
    }

    /// Unloads a track's plug-ins from the engine (and their controllers and editors).
    fn unload_plugins(&mut self, t: &Track) {
        if let Some(s) = &t.instrument {
            if let Some(mut c) = self.controllers.remove(&s.uid) {
                c.close_editor();
            }
            self.send(Cmd::Instrument { track: t.id, insert: None });
        }
        for (slot, s) in t.inserts.iter().enumerate() {
            if let Some(s) = s {
                if let Some(mut c) = self.controllers.remove(&s.uid) {
                    c.close_editor();
                }
                self.fx.remove(&s.uid);
                self.send(Cmd::Insert { track: t.id, slot, insert: None });
            }
        }
    }

    /// Loads a track's plug-ins from their saved settings.
    fn load_plugins(&mut self, t: &Track) {
        if let Some(s) = &t.instrument {
            let ins = self.instantiate_slot(s).map_err(|e| log::error!("{}: {e}", t.name)).ok();
            self.send(Cmd::Instrument { track: t.id, insert: ins });
        }
        for (slot, s) in t.inserts.iter().enumerate() {
            if let Some(s) = s {
                let ins = self.instantiate_slot(s).map_err(|e| log::error!("{}: {e}", t.name)).ok();
                self.send(Cmd::Insert { track: t.id, slot, insert: ins });
            }
        }
    }

    /// Undo and redo across a freeze: loads or unloads the plug-ins of a track that `restore`
    /// moves between `cur` and `t`. Returns true when the track is frozen on either side, so
    /// the caller leaves its plug-ins alone.
    pub(super) fn restore_freeze(&mut self, cur: &Track, t: &Track) -> bool {
        if cur.frozen.is_none() && t.frozen.is_none() {
            return false;
        }
        let uids = |x: &Track| x.inserts.iter().map(|s| s.as_ref().map(|s| s.uid)).chain(std::iter::once(x.instrument.as_ref().map(|s| s.uid))).collect::<Vec<_>>();
        match (cur.frozen.is_some(), t.frozen.is_some()) {
            (true, false) => self.load_plugins(t),
            (false, true) => self.unload_plugins(cur),
            _ => {
                if uids(cur) != uids(t) {
                    self.unload_plugins(cur);
                }
            }
        }
        if t.frozen.is_some() {
            self.ensure_freeze_files();
        }
        if cur.frozen != t.frozen {
            self.push_regions(t.id);
            let eq = self.eq_coeffs(t);
            self.send(Cmd::Eq(t.id, eq));
        }
        true
    }

    /// Writes back any freeze file that is missing on disk (unfreezing deletes it; undo can
    /// bring the freeze back) from the copy still in memory.
    pub(super) fn ensure_freeze_files(&self) {
        for f in self.project.tracks.iter().filter_map(|t| t.frozen) {
            let (Some(c), Some(r)) = (self.clips.get(&f.clip), self.project.clips.iter().find(|c| c.id == f.clip)) else { continue };
            let path = if Path::new(&r.path).is_absolute() { PathBuf::from(&r.path) } else { self.dir.join(&r.path) };
            if !path.exists() {
                if let Err(e) = crate::decode::write_wav(&path, c.sample_rate, &c.channels) {
                    log::error!("rewriting freeze file {}: {e}", path.display());
                }
            }
        }
    }

    /// Refuses an edit that changed a frozen track's regions, plug-ins or Channel EQ: puts
    /// them back and returns why. Call straight after an edit recorded with `checkpoint`
    /// (the command wrappers do). If nothing else changed, the edit's undo step goes too.
    pub fn frozen_guard(&mut self) -> Result<()> {
        let Some(before) = self.undo_stack.last().map(|e| &e.project) else { return Ok(()) };
        let mut hit: Vec<(Track, String)> = Vec::new();
        for t in self.project.tracks.iter().filter(|t| t.frozen.is_some()) {
            let Some(b) = before.track(t.id).filter(|b| b.frozen == t.frozen) else { continue };
            if !same(&t.regions, &b.regions) || !same(&t.instrument, &b.instrument) || !same(&t.inserts, &b.inserts) || !same(&t.eq, &b.eq) {
                hit.push((b.clone(), t.name.clone()));
            }
        }
        let Some(name) = hit.first().map(|h| h.1.clone()) else { return Ok(()) };
        let mut target = self.project.clone();
        // A region moved across is now on no track or two once the frozen track is put back, so
        // it goes back to where it was as well.
        let mut moved_in: Vec<Id> = Vec::new();
        let mut moved_off: Vec<Id> = Vec::new();
        for (b, _) in &hit {
            let now = self.project.track(b.id).map_or(&[][..], |t| &t.regions[..]);
            moved_in.extend(now.iter().filter(|r| !b.regions.iter().any(|o| o.id == r.id)).map(|r| r.id));
            moved_off.extend(b.regions.iter().filter(|r| !now.iter().any(|o| o.id == r.id)).map(|r| r.id));
        }
        for t in target.tracks.iter_mut().filter(|t| !hit.iter().any(|(b, _)| b.id == t.id)) {
            t.regions.retain(|r| !moved_off.contains(&r.id));
            let back = before.track(t.id).map_or(&[][..], |b| &b.regions[..]);
            t.regions.extend(back.iter().filter(|r| moved_in.contains(&r.id)).cloned());
        }
        for (b, _) in hit {
            if let Some(t) = target.track_mut(b.id) {
                t.regions = b.regions;
                t.instrument = b.instrument;
                t.inserts = b.inserts;
                t.eq = b.eq;
            }
        }
        let nothing_else = same(&target.tracks, &before.tracks) && same(&target.groups, &before.groups);
        self.restore(target);
        if nothing_else {
            self.undo_stack.pop();
        }
        Err(anyhow!("{name} is frozen. Unfreeze it to edit its regions and plug-ins."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::AudioConfig;
    use crate::session::RegionMove;

    fn session_with_two_tracks() -> (Session, Id, Id, Id) {
        let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        let mut s = Session::new(cfg, Vec::new());
        let a = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
        let b = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
        let id = s.project.alloc_id();
        let content = RegionContent::Midi { notes: Vec::new(), cc: Vec::new() };
        s.project.track_mut(a).unwrap().regions.push(Region { id, name: "M".into(), start: 0, length: 1000, muted: false, color: None, content, loop_end: None, takes: None, params: Default::default(), pattern: None });
        (s, a, b, id)
    }

    fn place(s: &Session, id: Id) -> Vec<Id> {
        s.project.tracks.iter().filter(|t| t.regions.iter().any(|r| r.id == id)).map(|t| t.id).collect()
    }

    #[test]
    fn a_region_moved_onto_a_frozen_track_stays_where_it_was() {
        let (mut s, a, b, id) = session_with_two_tracks();
        s.project.track_mut(b).unwrap().frozen = Some(FreezeInfo { clip: 0, start: 0 });
        s.checkpoint("Move Region", None, false);
        s.move_regions(&[RegionMove { id, start: 0, track: Some(b) }]);
        assert!(s.frozen_guard().is_err());
        assert_eq!(place(&s, id), vec![a]);
        assert!(s.undo_stack.is_empty(), "nothing changed, so no undo step");
    }

    #[test]
    fn a_region_moved_off_a_frozen_track_isnt_doubled() {
        let (mut s, a, b, id) = session_with_two_tracks();
        s.project.track_mut(a).unwrap().frozen = Some(FreezeInfo { clip: 0, start: 0 });
        s.checkpoint("Move Region", None, false);
        s.move_regions(&[RegionMove { id, start: 0, track: Some(b) }]);
        assert!(s.frozen_guard().is_err());
        assert_eq!(place(&s, id), vec![a]);
    }
}
