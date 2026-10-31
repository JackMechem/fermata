//! Controller assignments on the session side (the pure parts are in `crate::controllers`):
//! learning an assignment for the last touched parameter, the global assignment list, and
//! applying controller moves.
//!
//! A controller move goes through the same session paths as the mouse: it makes an undo step
//! (coalesced per parameter, like a fader drag), and while the transport plays in Touch, Latch
//! or Write it is recorded as automation. The controller holds the parameter (an automation
//! touch) while it keeps moving and lets go [`RELEASE_SECS`] after it stops, which is when a
//! Touch pass ends.
//!
//! The input threads take assigned controllers off the live MIDI path (`midi_io.rs`); `idle`
//! applies them here, on the main thread, where plug-in controllers may be called.

use std::path::PathBuf;

use super::*;
use crate::controllers::{self as ctl, Assignment, AssignmentFile, Controller, CtlKind, CtlTarget, EqField, Follow, Takeover, TransportAction};

/// How long a controller holds its parameter after its last move (s).
const RELEASE_SECS: f64 = 0.5;

/// The Channel EQ's bands, as the editor names them.
const BAND_NAMES: [&str; 8] = ["Low Cut", "Low Shelf", "Bell 1", "Bell 2", "Bell 3", "Bell 4", "High Shelf", "High Cut"];

#[derive(Default)]
pub(super) struct Controls {
    list: Vec<Assignment>,
    /// Per assignment, in step with `list`.
    follow: Vec<Follow>,
    /// Where the list is saved; None keeps it in memory only.
    path: Option<PathBuf>,
    /// The target being learned and its name.
    learn: Option<(CtlTarget, String)>,
    /// What the last learn assigned, for the next tick.
    learned: Option<String>,
    /// The parameter the user last moved, for Learn Assignment.
    touched: Option<CtlTarget>,
    /// Automation touches held by controllers, and when each last moved.
    holding: Vec<(Id, AutoParam, Instant)>,
}

/// An assignment as the Controller Assignments window shows it.
#[derive(Debug, Clone, Serialize)]
pub struct AssignmentRow {
    #[serde(flatten)]
    pub assignment: Assignment,
    /// The target's name ("Bass › Volume"), or what it was when its track or plug-in isn't in
    /// this project.
    pub name: String,
    /// The target is in the open project.
    pub available: bool,
    /// The target is a switch or transport button (no mode or range).
    pub button: bool,
}

/// "CC 7, channel 1" or "Pitch Bend, channel 1".
pub fn controller_label(c: &Controller) -> String {
    match c.kind {
        CtlKind::Cc { number } => format!("CC {number}, channel {}", c.channel + 1),
        CtlKind::PitchBend => format!("Pitch Bend, channel {}", c.channel + 1),
    }
}

fn round_to(v: f32, step: f32) -> f32 {
    (v / step).round() * step
}

impl Session {
    // ---- the assignment list --------------------------------------------------------------

    /// Loads the global assignments (a missing or unreadable file is an empty list) and saves
    /// changes there from now on.
    pub fn load_controller_assignments(&mut self, path: PathBuf) {
        let file: AssignmentFile = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        self.controls.list = file.assignments;
        self.controls.path = Some(path);
        self.ctl_list_changed(false);
    }

    /// Every assignment, for the Controller Assignments window.
    pub fn controller_assignments(&self) -> Vec<AssignmentRow> {
        self.controls
            .list
            .iter()
            .map(|a| {
                let name = self.ctl_target_name(&a.target);
                AssignmentRow {
                    available: name.is_some(),
                    name: name.unwrap_or_else(|| missing_name(&a.target)),
                    button: a.target.is_button(),
                    assignment: a.clone(),
                }
            })
            .collect()
    }

    /// Changes an assignment's takeover mode and range.
    pub fn update_controller_assignment(&mut self, index: usize, mode: Takeover, min: f32, max: f32) -> Result<()> {
        let a = self.controls.list.get_mut(index).ok_or_else(|| anyhow!("no such assignment"))?;
        a.mode = mode;
        a.min = if min.is_finite() { min.clamp(0.0, 1.0) } else { 0.0 };
        a.max = if max.is_finite() { max.clamp(0.0, 1.0) } else { 1.0 };
        self.ctl_list_changed(true);
        Ok(())
    }

    pub fn delete_controller_assignment(&mut self, index: usize) -> Result<()> {
        if index >= self.controls.list.len() {
            return Err(anyhow!("no such assignment"));
        }
        self.controls.list.remove(index);
        self.ctl_list_changed(true);
        Ok(())
    }

    /// The list changed: pickup states start over, the input threads learn which controllers
    /// to take, and the file is written.
    fn ctl_list_changed(&mut self, save: bool) {
        self.controls.follow = vec![Follow::default(); self.controls.list.len()];
        self.midi.set_assigned(self.controls.list.iter().map(|a| (a.controller.port.clone(), a.controller.channel, a.controller.kind)).collect());
        if save {
            self.save_controller_assignments();
        }
    }

    fn save_controller_assignments(&self) {
        let Some(path) = &self.controls.path else { return };
        let file = AssignmentFile { assignments: self.controls.list.clone() };
        let write = || -> Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_string_pretty(&file)?)?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        };
        if let Err(e) = write() {
            log::warn!("could not save {}: {e}", path.display());
        }
    }

    // ---- learning -------------------------------------------------------------------------

    /// Waits for a controller to assign to `target`, or to the last touched parameter. Returns
    /// the parameter's name.
    pub fn start_learn(&mut self, target: Option<CtlTarget>) -> Result<String> {
        let target = target.or_else(|| self.controls.touched.clone()).ok_or_else(|| anyhow!("Move a fader, knob or plug-in parameter first, then learn an assignment for it"))?;
        let target = self.ctl_complete(target);
        let name = self.ctl_target_name(&target).ok_or_else(|| anyhow!("That parameter isn't in this project any more"))?;
        self.controls.learn = Some((target, name.clone()));
        self.midi.set_learning(true);
        Ok(name)
    }

    pub fn cancel_learn(&mut self) {
        self.controls.learn = None;
        self.midi.set_learning(false);
    }

    /// The name of the parameter being learned, if any (for the tick).
    pub fn learning(&self) -> Option<String> {
        self.controls.learn.as_ref().map(|l| l.1.clone())
    }

    /// What the last learn assigned, once (for the tick).
    pub fn take_learned(&mut self) -> Option<String> {
        self.controls.learned.take()
    }

    fn ctl_learned(&mut self, controller: Controller, target: CtlTarget, name: String) {
        let label = controller_label(&controller);
        // A controller drives one parameter.
        self.controls.list.retain(|a| a.controller != controller);
        self.controls.list.push(Assignment::new(controller, target));
        self.midi.set_learning(false);
        self.ctl_list_changed(true);
        self.controls.learned = Some(format!("{label} assigned to {name}"));
    }

    /// Fills in a plug-in target's plug-in id (the UI may leave it empty).
    fn ctl_complete(&self, mut target: CtlTarget) -> CtlTarget {
        if let CtlTarget::Plugin { track, uid, plugin, .. } = &mut target {
            if let Some((_, s)) = self.ctl_slot(*track, *uid) {
                *plugin = s.plugin_id.clone();
            }
        }
        target
    }

    // ---- the last touched parameter -------------------------------------------------------

    /// The user moved a parameter; Learn Assignment without a target learns this one.
    pub fn touched(&mut self, target: CtlTarget) {
        self.controls.touched = Some(self.ctl_complete(target));
    }

    /// A track's volume, pan or a switch was changed from the interface.
    pub fn touched_patch(&mut self, track: Id, patch: &TrackPatch) {
        let t = if patch.volume_db.is_some() {
            CtlTarget::Volume { track }
        } else if patch.pan.is_some() {
            CtlTarget::Pan { track }
        } else if patch.mute.is_some() {
            CtlTarget::Mute { track }
        } else if patch.solo.is_some() {
            CtlTarget::Solo { track }
        } else if patch.armed.is_some() {
            CtlTarget::Arm { track }
        } else {
            return;
        };
        self.touched(t);
    }

    /// A built-in effect's or instrument's parameter was changed from its panel.
    pub fn touched_slot(&mut self, track: Id, slot: usize, param: u32) {
        if let Some(s) = self.slot_get(track, slot) {
            let t = CtlTarget::Plugin { track, uid: s.uid, plugin: s.plugin_id.clone(), param };
            self.touched(t);
        }
    }

    /// The Channel EQ editor changed `old` to `new`: the one setting that changed, if it was
    /// one (a reset or a paste isn't a touch).
    pub fn touched_eq(&mut self, track: Id, old: &ChannelEq, new: &ChannelEq) {
        let mut changed = Vec::new();
        if old.output_db != new.output_db {
            changed.push(CtlTarget::EqOutput { track });
        }
        for (i, (a, b)) in old.bands.iter().zip(&new.bands).enumerate() {
            let band = i as u8;
            for (field, differs) in [(EqField::On, a.on != b.on), (EqField::Freq, a.freq != b.freq), (EqField::Gain, a.gain != b.gain), (EqField::Q, a.q != b.q)] {
                if differs {
                    changed.push(CtlTarget::Eq { track, band, field });
                }
            }
        }
        // Setting a gain also switches its band on; dragging a band's point moves its
        // frequency and gain together, which counts as the frequency.
        if changed.len() > 1 {
            changed.retain(|t| !matches!(t, CtlTarget::Eq { field: EqField::On, .. }));
        }
        let t = match changed.as_slice() {
            [one] => one.clone(),
            [f @ CtlTarget::Eq { band: a, field: EqField::Freq, .. }, CtlTarget::Eq { band: b, field: EqField::Gain, .. }] if a == b => f.clone(),
            _ => return,
        };
        self.touched(t);
    }

    /// An automation parameter was grabbed (the controls that report touches).
    pub fn touched_auto(&mut self, track: Id, param: &AutoParam) {
        let t = match param {
            AutoParam::Volume => CtlTarget::Volume { track },
            AutoParam::Pan => CtlTarget::Pan { track },
            AutoParam::Mute => CtlTarget::Mute { track },
            AutoParam::Send(slot) => CtlTarget::Send { track, slot: *slot },
            AutoParam::Plugin { uid, param, .. } => CtlTarget::Plugin { track, uid: *uid, plugin: String::new(), param: *param },
        };
        self.touched(t);
    }

    /// A parameter moved in a plug-in's own editor.
    pub(super) fn touched_plugin(&mut self, uid: Id, param: u32) {
        let p = &self.project;
        let tracks = p.tracks.iter().map(|t| (t.id, t.inserts.iter().flatten().chain(t.instrument.iter()).find(|s| s.uid == uid)));
        let master = std::iter::once((MASTER_ID, p.master.inserts.iter().flatten().find(|s| s.uid == uid)));
        if let Some((track, s)) = tracks.chain(master).find_map(|(id, s)| s.map(|s| (id, s))) {
            let t = CtlTarget::Plugin { track, uid, plugin: s.plugin_id.clone(), param };
            self.controls.touched = Some(t);
        }
    }

    // ---- targets --------------------------------------------------------------------------

    /// A plug-in slot on a strip: its insert index (None for the instrument slot) and the slot.
    fn ctl_slot(&self, track: Id, uid: Id) -> Option<(Option<usize>, &PluginSlot)> {
        let (inserts, instrument) = if track == MASTER_ID {
            (&self.project.master.inserts, None)
        } else {
            let t = self.project.track(track)?;
            (&t.inserts, t.instrument.as_ref())
        };
        if let Some(i) = inserts.iter().position(|s| s.as_ref().is_some_and(|s| s.uid == uid)) {
            return inserts[i].as_ref().map(|s| (Some(i), s));
        }
        instrument.filter(|s| s.uid == uid).map(|s| (None, s))
    }

    /// The plug-in slot a target names, if it is there and still holds the same plug-in.
    fn ctl_plugin(&self, track: Id, uid: Id, plugin: &str) -> Option<(Option<usize>, &PluginSlot)> {
        self.ctl_slot(track, uid).filter(|(_, s)| s.plugin_id == plugin)
    }

    fn ctl_strip(&self, track: Id) -> Option<String> {
        if track == MASTER_ID {
            Some("Stereo Out".into())
        } else {
            self.project.track(track).map(|t| t.name.clone())
        }
    }

    /// A target's name, or None when it isn't in the open project. Calls plug-in controllers,
    /// so main thread only.
    fn ctl_target_name(&self, t: &CtlTarget) -> Option<String> {
        let track = |id: Id| self.project.track(id).map(|t| t.name.clone());
        Some(match t {
            CtlTarget::Volume { track: id } => format!("{} › Volume", track(*id)?),
            CtlTarget::Pan { track: id } => format!("{} › Pan", track(*id)?),
            CtlTarget::Mute { track: id } => format!("{} › Mute", track(*id)?),
            CtlTarget::Solo { track: id } => format!("{} › Solo", track(*id)?),
            CtlTarget::Arm { track: id } => format!("{} › Record Enable", track(*id)?),
            CtlTarget::Send { track: id, slot } => {
                self.project.track(*id)?.sends.get(*slot as usize)?.as_ref()?;
                format!("{} › Send {}", track(*id)?, slot + 1)
            }
            CtlTarget::Plugin { track: id, uid, plugin, param } => {
                let (_, slot) = self.ctl_plugin(*id, *uid, plugin)?;
                let p = self.plugin_params(*uid).into_iter().find(|p| p.id == *param)?;
                format!("{} › {} › {}", self.ctl_strip(*id)?, slot.name, p.name)
            }
            CtlTarget::Eq { track: id, band, field } => {
                self.eq_of(*id)?.bands.get(*band as usize)?;
                let what = match field {
                    EqField::On => "On",
                    EqField::Freq => "Frequency",
                    EqField::Gain => "Gain",
                    EqField::Q => "Q",
                };
                format!("{} › Channel EQ › {} {what}", self.ctl_strip(*id)?, BAND_NAMES.get(*band as usize).copied().unwrap_or("Band"))
            }
            CtlTarget::EqOutput { track: id } => format!("{} › Channel EQ › Output", self.ctl_strip(*id)?),
            CtlTarget::MasterVolume => "Stereo Out › Volume".into(),
            CtlTarget::Transport { action } => match action {
                TransportAction::Play => "Transport › Play / Stop".into(),
                TransportAction::Record => "Transport › Record".into(),
                TransportAction::Cycle => "Transport › Cycle".into(),
            },
        })
    }

    /// The target's value on its own 0..1 travel (the fader's taper for levels), or None
    /// when it isn't in the open project. Cheap enough for every controller message.
    fn ctl_value(&self, t: &CtlTarget) -> Option<f32> {
        let tr = |id: Id| self.project.track(id);
        Some(match t {
            CtlTarget::Volume { track } => ctl::db_to_fader(tr(*track)?.volume_db),
            CtlTarget::Pan { track } => (tr(*track)?.pan + 1.0) / 2.0,
            CtlTarget::Mute { track } => tr(*track)?.mute as u8 as f32,
            CtlTarget::Solo { track } => tr(*track)?.solo as u8 as f32,
            CtlTarget::Arm { track } => tr(*track)?.armed as u8 as f32,
            CtlTarget::Send { track, slot } => ctl::db_to_fader(tr(*track)?.sends.get(*slot as usize)?.as_ref()?.level_db),
            CtlTarget::Plugin { track, uid, plugin, param } => {
                self.ctl_plugin(*track, *uid, plugin)?;
                match self.controllers.get(uid) {
                    Some(c) => c.param_value(*param)? as f32,
                    None => self.builtin_normalized(*uid, *param)?,
                }
            }
            CtlTarget::Eq { track, band, field } => {
                let eq = self.eq_of(*track)?;
                let b = eq.bands.get(*band as usize)?;
                let v = match field {
                    EqField::On => b.on as u8 as f32,
                    EqField::Freq => b.freq,
                    EqField::Gain => b.gain,
                    EqField::Q => b.q,
                };
                ctl::eq_to_norm(*field, v)
            }
            CtlTarget::EqOutput { track } => ctl::eq_to_norm(EqField::Gain, self.eq_of(*track)?.output_db),
            CtlTarget::MasterVolume => ctl::db_to_fader(self.project.master.volume_db),
            CtlTarget::Transport { action } => match action {
                TransportAction::Play => self.is_playing() as u8 as f32,
                TransportAction::Record => self.is_recording() as u8 as f32,
                TransportAction::Cycle => self.project.cycle.enabled as u8 as f32,
            },
        })
    }

    /// An undo step for a controller move, coalesced like a drag of the same control, unless
    /// the move is being written as automation (then it belongs to the pass).
    fn ctl_checkpoint(&mut self, track: Id, label: &str, key: String) {
        if !self.automation_writing(track) {
            self.checkpoint(label, Some(key), false);
        }
    }

    /// While automation is being written, the controller holds the parameter as a hand on the
    /// fader would; `controller_idle` lets go once it stops moving.
    fn ctl_hold(&mut self, track: Id, param: AutoParam) {
        if !self.automation_writing(track) {
            return;
        }
        let now = Instant::now();
        if let Some(h) = self.controls.holding.iter_mut().find(|h| h.0 == track && h.1.same(&param)) {
            h.2 = now;
            return;
        }
        self.automation_touch(track, param.clone(), true);
        self.controls.holding.push((track, param, now));
    }

    /// Moves a continuous target to `n` on its 0..1 travel, as the user would.
    fn ctl_set(&mut self, t: &CtlTarget, n: f32) {
        match t {
            CtlTarget::Volume { track } => {
                self.ctl_checkpoint(*track, "Volume", format!("track:{track}:volume"));
                self.ctl_hold(*track, AutoParam::Volume);
                let db = round_to(ctl::fader_to_db(n), 0.1);
                self.update_track_linked(*track, TrackPatch { volume_db: Some(db), ..Default::default() });
            }
            CtlTarget::Pan { track } => {
                self.ctl_checkpoint(*track, "Pan", format!("track:{track}:pan"));
                self.ctl_hold(*track, AutoParam::Pan);
                let pan = round_to(n * 2.0 - 1.0, 1.0 / 64.0);
                self.update_track_linked(*track, TrackPatch { pan: Some(pan), ..Default::default() });
            }
            CtlTarget::Send { track, slot } => {
                self.ctl_checkpoint(*track, "Send Level", format!("send:{track}:{slot}"));
                self.ctl_hold(*track, AutoParam::Send(*slot));
                self.set_send_level_linked(*track, *slot as usize, round_to(ctl::fader_to_db(n), 0.1));
            }
            CtlTarget::Plugin { track, uid, plugin, param } => self.ctl_set_plugin(*track, *uid, plugin, *param, n),
            CtlTarget::Eq { track, band, field } => {
                let (Some(mut eq), false) = (self.eq_of(*track), self.is_frozen(*track)) else { return };
                let Some(b) = eq.bands.get_mut(*band as usize) else { return };
                let v = ctl::eq_to_plain(*field, n);
                match field {
                    EqField::On => b.on = v >= 0.5,
                    EqField::Freq => b.freq = round_to(v, 0.1),
                    EqField::Gain => {
                        b.gain = round_to(v, 0.1);
                        // As in the editor, setting a gain switches the band on.
                        b.on = true;
                    }
                    EqField::Q => b.q = round_to(v, 0.01),
                }
                self.checkpoint("Channel EQ", Some(format!("eq:{track}")), false);
                self.set_eq(*track, eq);
            }
            CtlTarget::EqOutput { track } => {
                let (Some(mut eq), false) = (self.eq_of(*track), self.is_frozen(*track)) else { return };
                eq.output_db = round_to(ctl::eq_to_plain(EqField::Gain, n), 0.1);
                self.checkpoint("Channel EQ", Some(format!("eq:{track}")), false);
                self.set_eq(*track, eq);
            }
            CtlTarget::MasterVolume => {
                self.ctl_checkpoint(MASTER_ID, "Stereo Out Volume", "master-volume".into());
                self.ctl_hold(MASTER_ID, AutoParam::Volume);
                self.set_master_volume_linked(round_to(ctl::fader_to_db(n), 0.1));
            }
            CtlTarget::Mute { .. } | CtlTarget::Solo { .. } | CtlTarget::Arm { .. } | CtlTarget::Transport { .. } => {}
        }
    }

    fn ctl_set_plugin(&mut self, track: Id, uid: Id, plugin: &str, param: u32, n: f32) {
        let Some((slot, s)) = self.ctl_plugin(track, uid, plugin) else { return };
        let name = format!("{}: {}", s.name, self.plugin_params(uid).into_iter().find(|p| p.id == param).map(|p| p.name).unwrap_or_else(|| format!("Parameter {param}")));
        let auto = AutoParam::Plugin { uid, param, name };
        // Stereo Out's plug-ins aren't automated.
        let automated = track != MASTER_ID;
        if let Some(spec) = builtin::builtin_spec(plugin).and_then(|(_, specs)| specs.iter().find(|p| p.id == param).copied()) {
            // None: the instrument slot.
            let slot = slot.unwrap_or(usize::MAX);
            let pushed = !self.automation_writing(track) && self.checkpoint("Plug-in Parameter", Some(format!("fxparam:{uid}:{param}")), false);
            if automated {
                self.ctl_hold(track, auto);
            }
            if let Err(e) = self.set_builtin_param(track, slot, param, to_plain(&spec, n)) {
                log::warn!("controller assignment: {e}");
                if pushed {
                    self.abandon_checkpoint();
                }
            }
        } else if let Some(before) = self.controllers.get(&uid).and_then(|c| c.param_value(param)) {
            // A VST3 parameter: as if moved in the plug-in's editor, but without an undo step:
            // one comes only from the editor itself (`vst_undo.rs`), as a control surface's
            // moves aren't edits of the project a step should take back.
            if automated {
                self.ctl_hold(track, auto);
            }
            if let Some(c) = self.controllers.get(&uid) {
                c.edit_param(param, n as f64);
            }
            if automated {
                self.auto_plugin_moved(track, uid, param, before as f32);
            }
            self.dirty = true;
        }
    }

    /// A button target was pressed.
    fn ctl_press(&mut self, t: &CtlTarget) {
        match t {
            CtlTarget::Mute { track } => {
                let on = self.project.track(*track).map(|t| !t.mute);
                self.update_track_linked(*track, TrackPatch { mute: on, ..Default::default() });
            }
            CtlTarget::Solo { track } => {
                let on = self.project.track(*track).map(|t| !t.solo);
                self.update_track_linked(*track, TrackPatch { solo: on, ..Default::default() });
            }
            CtlTarget::Arm { track } => {
                let on = self.project.track(*track).map(|t| !t.armed);
                self.update_track_linked(*track, TrackPatch { armed: on, ..Default::default() });
            }
            CtlTarget::Eq { track, band, field: EqField::On } => {
                let (Some(mut eq), false) = (self.eq_of(*track), self.is_frozen(*track)) else { return };
                let Some(b) = eq.bands.get_mut(*band as usize) else { return };
                b.on = !b.on;
                self.checkpoint("Channel EQ", Some(format!("eq:{track}")), false);
                self.set_eq(*track, eq);
            }
            CtlTarget::Transport { action: TransportAction::Play } => self.toggle_play(),
            CtlTarget::Transport { action: TransportAction::Record } => {
                if self.is_recording() {
                    self.stop();
                } else if let Err(e) = self.record() {
                    self.warning = Some(e.to_string());
                }
            }
            CtlTarget::Transport { action: TransportAction::Cycle } => {
                let c = self.project.cycle.clone();
                if c.end > c.start {
                    self.set_cycle(!c.enabled, c.start, c.end);
                } else {
                    // No locators yet: four bars from the playhead's bar, as the Cycle button.
                    let map = self.tempo_map();
                    let bar = map.bar_at(self.position() as f64);
                    let (start, end) = (map.bar_start(bar).round() as i64, map.bar_start(bar + 4).round() as i64);
                    self.set_cycle(true, start, end);
                }
            }
            _ => {}
        }
    }

    // ---- incoming controller messages -----------------------------------------------------

    /// Plays a message as if it came from the hardware port `port` (for tests without MIDI
    /// hardware). `idle` handles controller assignments it triggers.
    pub fn inject_midi_from(&self, port: &str, msg: &[u8]) {
        self.midi.inject_from(port, msg);
    }

    /// Handles the controller messages the input threads took since the last call, and lets
    /// go of parameters controllers stopped moving. Returns whether the project changed (the
    /// interface needs a new snapshot).
    pub(super) fn controller_idle(&mut self) -> bool {
        let mut changed = false;
        // Continuous controllers: only the latest value of each matters in one batch.
        let mut moves: Vec<(usize, f32)> = Vec::new();
        for m in self.midi.take_controls() {
            let Some(msg) = ctl::parse(&m.data) else { continue };
            if let Some((target, name)) = self.controls.learn.take() {
                // The move that was learned doesn't act yet.
                self.ctl_learned(Controller { port: m.port.to_string(), channel: msg.channel, kind: msg.kind }, target, name);
                continue;
            }
            let Some(i) = ctl::find(&self.controls.list, &m.port, &msg) else { continue };
            let target = self.controls.list[i].target.clone();
            if target.is_button() {
                if self.controls.follow[i].press(msg.pressed) && self.ctl_value(&target).is_some() {
                    self.ctl_press(&target);
                    changed = true;
                }
            } else {
                match moves.iter_mut().find(|(j, _)| *j == i) {
                    Some(mv) => mv.1 = msg.value,
                    None => moves.push((i, msg.value)),
                }
            }
        }
        for (i, value) in moves {
            changed |= self.ctl_move(i, value);
        }
        self.ctl_release();
        changed
    }

    fn ctl_move(&mut self, i: usize, value: f32) -> bool {
        let a = self.controls.list[i].clone();
        let Some(current) = self.ctl_value(&a.target) else { return false };
        let want = a.scale(value);
        if !self.controls.follow[i].take(a.mode, want, current) {
            return false;
        }
        self.ctl_set(&a.target, want);
        if let Some(v) = self.ctl_value(&a.target) {
            self.controls.follow[i].applied(v);
        }
        true
    }

    /// Lets go of parameters whose controllers have stopped (all of them once stopped).
    fn ctl_release(&mut self) {
        if self.controls.holding.is_empty() {
            return;
        }
        let playing = self.is_playing();
        let (done, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.controls.holding).into_iter().partition(|h| !playing || h.2.elapsed().as_secs_f64() > RELEASE_SECS);
        self.controls.holding = keep;
        for (track, param, _) in done {
            self.automation_touch(track, param, false);
        }
    }
}

/// How the window names a target whose track or plug-in isn't in the open project.
fn missing_name(t: &CtlTarget) -> String {
    let what = match t {
        CtlTarget::Volume { .. } => "Volume".to_string(),
        CtlTarget::Pan { .. } => "Pan".into(),
        CtlTarget::Mute { .. } => "Mute".into(),
        CtlTarget::Solo { .. } => "Solo".into(),
        CtlTarget::Arm { .. } => "Record Enable".into(),
        CtlTarget::Send { slot, .. } => format!("Send {}", slot + 1),
        CtlTarget::Plugin { plugin, param, .. } => format!("{} › Parameter {param}", plugin.strip_prefix("builtin:").unwrap_or(plugin)),
        CtlTarget::Eq { band, .. } => format!("Channel EQ › {}", BAND_NAMES.get(*band as usize).copied().unwrap_or("Band")),
        CtlTarget::EqOutput { .. } => "Channel EQ › Output".into(),
        CtlTarget::MasterVolume | CtlTarget::Transport { .. } => String::new(),
    };
    match t.track() {
        Some(MASTER_ID) => format!("Stereo Out › {what}"),
        Some(id) => format!("Track {id} › {what}"),
        None => what,
    }
}
