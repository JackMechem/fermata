//! Automation on the session side: writing lanes from control moves while playing, as Logic's
//! modes do (Touch writes while a control is held and then returns to the existing curve,
//! Latch writes from the first touch until playback stops, Write overwrites every lane from
//! where playback started), keeping plug-in editors in step with the automation that plays,
//! and the lane editing commands.
//!
//! A pass collects points at the transport position as the control moves (and ~30 times a
//! second from `idle`), and goes into its lane in one undo step when it ends: on release in
//! Touch, at Stop otherwise.

use std::collections::HashMap;

use vst3_host::{Gesture, ParamInfo};

use super::{Session, TrackPatch};
use crate::automation::{lanes_dsp, splice, thin, tolerance, value_at, AutoTarget};
use crate::dsp::Cmd;
use crate::model::{AutoLane, AutoMode, AutoParam, AutoPoint, Id, TrackKind};

/// How long Touch takes to glide back to the existing curve after the control is let go (s).
const RETURN_GLIDE: f64 = 0.2;
/// A control that sat still for longer than this (s) gets a point where it starts moving
/// again, so the hold between doesn't turn into a slow ramp...
const HOLD_GAP: f64 = 0.05;
/// ...that far before the new value.
const HOLD_RAMP: f64 = 0.01;

#[derive(Default)]
pub(super) struct AutoWriter {
    passes: Vec<Pass>,
    /// What automation last moved each plug-in editor to, per (instance, parameter).
    shown: HashMap<(Id, u32), f32>,
    /// A pass went into its lane since `idle` last reported.
    written: bool,
}

/// One parameter being written.
struct Pass {
    track: Id,
    param: AutoParam,
    mode: AutoMode,
    /// The value before writing began, for a lane that was empty.
    before: f32,
    /// Finished stretches (a cycle jump back ends one): start, end, points.
    done: Vec<(i64, i64, Vec<AutoPoint>)>,
    start: i64,
    points: Vec<AutoPoint>,
    /// The latest live value.
    value: f32,
}

impl Session {
    /// The parameter's live value: the fader, pan knob, mute button or plug-in control.
    fn live_value(&self, track: Id, param: &AutoParam) -> Option<f32> {
        let t = self.project.track(track)?;
        match param {
            AutoParam::Volume => Some(t.volume_db),
            AutoParam::Pan => Some(t.pan),
            AutoParam::Mute => Some(t.mute as u8 as f32),
            AutoParam::Send(slot) => t.sends.get(*slot as usize).copied().flatten().map(|s| s.level_db),
            AutoParam::Plugin { uid, param, .. } => self.controllers.get(uid).and_then(|c| c.param_value(*param)).map(|v| v as f32),
        }
    }

    /// Moves the control itself (used to pick a control up where the automation has it).
    fn set_live(&mut self, track: Id, param: &AutoParam, v: f32) {
        let patch = match param {
            AutoParam::Volume => TrackPatch { volume_db: Some(v), ..Default::default() },
            AutoParam::Pan => TrackPatch { pan: Some(v), ..Default::default() },
            AutoParam::Mute => TrackPatch { mute: Some(v >= 0.5), ..Default::default() },
            AutoParam::Send(slot) => {
                self.set_send_level(track, *slot as usize, v);
                return;
            }
            AutoParam::Plugin { uid, param, .. } => {
                if let Some(c) = self.controllers.get(uid) {
                    c.set_param_value(*param, v as f64);
                }
                return;
            }
        };
        self.update_track(track, patch);
    }

    /// What the lane plays at `pos`, if it has points.
    fn curve_value(&self, track: Id, param: &AutoParam, pos: i64) -> Option<f32> {
        let lane = self.project.track(track)?.automation.iter().find(|l| l.param.same(param))?;
        value_at(&lane.points, pos)
    }

    fn push_automation(&mut self, track: Id) {
        if let Some(t) = self.project.track(track) {
            let lanes = lanes_dsp(&t.automation);
            self.send(Cmd::Automation(track, lanes));
        }
    }

    fn pass_index(&self, track: Id, param: &AutoParam) -> Option<usize> {
        self.auto.passes.iter().position(|p| p.track == track && p.param.same(param))
    }

    /// Starts writing `param` at `pos` if the track's mode writes (or finds the pass already
    /// going). `before` is the value the control had before it moved.
    fn begin_pass(&mut self, track: Id, param: &AutoParam, pos: i64, before: Option<f32>) -> Option<usize> {
        if let Some(i) = self.pass_index(track, param) {
            return Some(i);
        }
        let t = self.project.track(track)?;
        // A folder's own settings only shape its subtracks, which the engine doesn't automate.
        if !t.auto_mode.writes() || t.kind == TrackKind::Folder {
            return None;
        }
        let mode = t.auto_mode;
        let value = self.live_value(track, param)?;
        let before = before.or_else(|| self.curve_value(track, param, pos)).unwrap_or(value);
        // Write mode already plays every live value; Touch and Latch let go of this one.
        if mode != AutoMode::Write {
            self.send(Cmd::AutoTouch { track, target: AutoTarget::from(param), on: true });
        }
        self.auto.passes.push(Pass { track, param: param.clone(), mode, before, done: Vec::new(), start: pos, points: vec![AutoPoint { pos, value }], value });
        Some(self.auto.passes.len() - 1)
    }

    /// Adds the live value at the current position to a pass.
    fn sample_pass(&mut self, i: usize, value: f32) {
        let pos = self.position();
        let sr = self.sample_rate();
        let cycle = self.project.cycle.clone();
        let p = &mut self.auto.passes[i];
        p.value = value;
        let Some(last) = p.points.last().copied() else {
            p.points.push(AutoPoint { pos, value });
            return;
        };
        if pos < last.pos {
            // The cycle jumped back: this stretch ends at the cycle end, a new one starts.
            let (cs, ce) = if cycle.enabled { (cycle.start.min(pos), cycle.end.max(last.pos)) } else { (pos, last.pos) };
            let mut pts = std::mem::take(&mut p.points);
            if ce > last.pos {
                pts.push(AutoPoint { pos: ce, value: last.value });
            }
            p.done.push((p.start, ce, pts));
            p.start = cs;
            p.points.push(AutoPoint { pos: cs, value });
            if pos > cs {
                p.points.push(AutoPoint { pos, value });
            }
            return;
        }
        if pos == last.pos {
            p.points.last_mut().unwrap().value = value;
            return;
        }
        if last.value != value && (pos - last.pos) as f64 > HOLD_GAP * sr {
            p.points.push(AutoPoint { pos: pos - (HOLD_RAMP * sr) as i64, value: last.value });
        }
        p.points.push(AutoPoint { pos, value });
    }

    /// The playhead was moved while writing: what was written so far ends where it was, and
    /// writing carries on from the new place.
    pub(super) fn auto_jumped(&mut self, from: i64, to: i64) {
        for p in &mut self.auto.passes {
            let last = p.points.last().map(|x| x.pos).unwrap_or(p.start);
            let end = from.max(last);
            let mut pts = std::mem::take(&mut p.points);
            if end > last {
                pts.push(AutoPoint { pos: end, value: p.value });
            }
            p.done.push((p.start, end, pts));
            p.start = to;
            p.points.push(AutoPoint { pos: to, value: p.value });
        }
    }

    /// Volume, pan or mute moved through `update_track_linked`; `was` holds the values before
    /// the move of whichever of the three moved.
    pub(super) fn auto_moved(&mut self, track: Id, was: [Option<f32>; 3]) {
        if !self.is_playing() {
            return;
        }
        let pos = self.position();
        for (k, param) in [AutoParam::Volume, AutoParam::Pan, AutoParam::Mute].into_iter().enumerate() {
            let Some(before) = was[k] else { continue };
            let Some(i) = self.begin_pass(track, &param, pos, Some(before)) else { continue };
            if let Some(v) = self.live_value(track, &param) {
                self.sample_pass(i, v);
            }
        }
    }

    /// A send knob moved (from `before` dB); recorded like a fader move while writing.
    pub fn set_send_level_linked(&mut self, track: Id, slot: usize, level_db: f32) {
        let before = self.live_value(track, &AutoParam::Send(slot as u8));
        self.set_send_level(track, slot, level_db);
        if !self.is_playing() {
            return;
        }
        let param = AutoParam::Send(slot as u8);
        let pos = self.position();
        if let Some(i) = self.begin_pass(track, &param, pos, before) {
            if let Some(v) = self.live_value(track, &param) {
                self.sample_pass(i, v);
            }
        }
    }

    /// The UI grabbed (`touching`) or let go of a control. Group members that share the
    /// parameter are touched with it.
    pub fn automation_touch(&mut self, track: Id, param: AutoParam, touching: bool) {
        let mut ids = vec![track];
        ids.extend(self.group_peers(track, |g| match param {
            AutoParam::Volume => g.volume,
            AutoParam::Pan => g.pan,
            AutoParam::Mute => g.mute,
            _ => false,
        }));
        for id in ids {
            if touching {
                self.touch_start(id, &param);
            } else {
                self.touch_end(id, &param);
            }
        }
    }

    fn touch_start(&mut self, track: Id, param: &AutoParam) {
        if !self.is_playing() {
            return;
        }
        let Some(mode) = self.project.track(track).map(|t| t.auto_mode) else { return };
        if !mode.writes() {
            return;
        }
        let pos = self.position();
        // Pick the control up where the automation has it, so the value doesn't jump.
        if mode != AutoMode::Write && self.pass_index(track, param).is_none() {
            if let Some(v) = self.curve_value(track, param, pos) {
                self.set_live(track, param, v);
            }
        }
        if let Some(i) = self.begin_pass(track, param, pos, None) {
            if let Some(v) = self.live_value(track, param) {
                self.sample_pass(i, v);
            }
        }
    }

    fn touch_end(&mut self, track: Id, param: &AutoParam) {
        let Some(i) = self.pass_index(track, param) else { return };
        // Latch and Write keep going (holding the value) until Stop.
        if self.auto.passes[i].mode == AutoMode::Touch {
            let pass = self.auto.passes.remove(i);
            let end = self.position();
            self.commit(vec![pass], end);
        }
    }

    /// Write mode starts writing every lane, and volume and pan, from where playback starts.
    fn start_write_passes(&mut self, track: Id, from: i64) {
        let Some(t) = self.project.track(track) else { return };
        if t.auto_mode != AutoMode::Write || t.kind == TrackKind::Folder {
            return;
        }
        let mut params = vec![AutoParam::Volume, AutoParam::Pan];
        for l in &t.automation {
            if !params.iter().any(|p| p.same(&l.param)) && !matches!(l.param, AutoParam::Send(_)) {
                params.push(l.param.clone());
            }
        }
        for p in params {
            self.begin_pass(track, &p, from, None);
        }
    }

    pub(super) fn auto_play_started(&mut self, from: i64) {
        // Editors may have been moved by hand while stopped.
        self.auto.shown.clear();
        let ids: Vec<Id> = self.project.tracks.iter().filter(|t| t.auto_mode == AutoMode::Write).map(|t| t.id).collect();
        for id in ids {
            self.start_write_passes(id, from);
        }
    }

    /// Playback stopped at `at`: every pass goes into its lane. As in Logic, a track that was
    /// in Write is left in Latch, so the next pass doesn't erase everything again.
    pub(super) fn auto_stopped(&mut self, at: i64) {
        let passes = std::mem::take(&mut self.auto.passes);
        let mut wrote: Vec<Id> = passes.iter().filter(|p| p.mode == AutoMode::Write).map(|p| p.track).collect();
        wrote.dedup();
        self.commit(passes, at);
        for id in wrote {
            if let Some(t) = self.project.track_mut(id) {
                if t.auto_mode == AutoMode::Write {
                    t.auto_mode = AutoMode::Latch;
                    self.send(Cmd::AutoMode(id, AutoMode::Latch));
                }
            }
        }
    }

    /// Drops passes in progress without writing them (undo while playing).
    pub(super) fn auto_abort(&mut self) {
        for p in std::mem::take(&mut self.auto.passes) {
            if p.mode != AutoMode::Write {
                self.send(Cmd::AutoTouch { track: p.track, target: AutoTarget::from(&p.param), on: false });
            }
        }
    }

    /// Puts finished passes into their lanes, as one undo step.
    fn commit(&mut self, passes: Vec<Pass>, end: i64) {
        if passes.is_empty() {
            return;
        }
        self.checkpoint("Write Automation", None, false);
        let glide = (RETURN_GLIDE * self.sample_rate()) as i64;
        let mut tracks: Vec<Id> = Vec::new();
        let mut release: Vec<(Id, AutoTarget)> = Vec::new();
        for mut p in passes {
            if p.mode != AutoMode::Write {
                release.push((p.track, AutoTarget::from(&p.param)));
            }
            let last = p.points.last().map(|x| x.pos).unwrap_or(p.start);
            // Stop can land behind the last point when the cycle just jumped back.
            let end = end.max(last);
            if end > last {
                p.points.push(AutoPoint { pos: end, value: p.value });
            }
            p.done.push((p.start, end, std::mem::take(&mut p.points)));
            let Some(t) = self.project.track_mut(p.track) else { continue };
            let existing = t.automation.iter().position(|l| l.param.same(&p.param));
            let unchanged = p.done.iter().all(|(_, _, pts)| pts.iter().all(|x| x.value == p.before));
            // Untouched parameters (Write mode) don't get new lanes.
            if existing.is_none() && unchanged {
                continue;
            }
            let i = existing.unwrap_or_else(|| {
                t.automation.push(AutoLane { param: p.param.clone(), points: Vec::new() });
                t.automation.len() - 1
            });
            let lane = &mut t.automation[i];
            for (s, e, pts) in &p.done {
                let rec = thin(pts, tolerance(&p.param));
                lane.points = splice(&lane.points, &rec, *s, *e, p.before, p.mode == AutoMode::Touch, glide, tolerance(&p.param));
            }
            if !tracks.contains(&p.track) {
                tracks.push(p.track);
            }
        }
        for id in tracks {
            self.push_automation(id);
        }
        // After the new lane, so the engine goes straight from the live value to the new curve.
        for (track, target) in release {
            self.send(Cmd::AutoTouch { track, target, on: false });
        }
        self.auto.written = true;
        self.dirty = true;
    }

    /// The track a plug-in instance is on (Stereo Out plug-ins aren't automated).
    fn track_of_plugin(&self, uid: Id) -> Option<Id> {
        self.project.tracks.iter().find(|t| t.inserts.iter().flatten().chain(t.instrument.iter()).any(|s| s.uid == uid)).map(|t| t.id)
    }

    fn plugin_param(&self, track: Id, uid: Id, param: u32) -> AutoParam {
        let probe = AutoParam::Plugin { uid, param, name: String::new() };
        if let Some(l) = self.project.track(track).and_then(|t| t.automation.iter().find(|l| l.param.same(&probe))) {
            return l.param.clone();
        }
        let name = self.controllers.get(&uid).and_then(|c| c.parameters().into_iter().find(|p| p.id == param)).map(|p| p.name).unwrap_or_else(|| format!("Parameter {param}"));
        AutoParam::Plugin { uid, param, name }
    }

    /// Periodic automation work from `idle`: edits made in plug-in editors, sampling the
    /// passes in progress, and moving plug-in editors with the automation that plays.
    /// Returns true if a pass went into its lane since the last call.
    pub(super) fn auto_idle(&mut self) -> bool {
        let playing = self.is_playing();
        let uids: Vec<Id> = self.controllers.keys().copied().collect();
        for uid in uids {
            let gestures = self.controllers.get_mut(&uid).map(|c| c.take_gestures()).unwrap_or_default();
            if gestures.is_empty() || !playing {
                continue;
            }
            let Some(track) = self.track_of_plugin(uid) else { continue };
            for g in gestures {
                match g {
                    Gesture::Begin(id) => {
                        let p = self.plugin_param(track, uid, id);
                        self.touch_start(track, &p);
                    }
                    Gesture::Perform(id, v) => {
                        let p = self.plugin_param(track, uid, id);
                        // Some editors don't bracket their edits with begin/end.
                        let pos = self.position();
                        if let Some(i) = self.begin_pass(track, &p, pos, None) {
                            self.sample_pass(i, v as f32);
                        }
                    }
                    Gesture::End(id) => {
                        let p = self.plugin_param(track, uid, id);
                        self.touch_end(track, &p);
                    }
                }
            }
        }
        if playing {
            for i in 0..self.auto.passes.len() {
                let (track, param) = (self.auto.passes[i].track, self.auto.passes[i].param.clone());
                if let Some(v) = self.live_value(track, &param) {
                    self.sample_pass(i, v);
                }
            }
            self.follow_editors();
        }
        std::mem::take(&mut self.auto.written)
    }

    /// Moves plug-in controllers (and so their editors) to the automation the engine is
    /// playing to their processors.
    fn follow_editors(&mut self) {
        let pos = self.position();
        let mut moves = Vec::new();
        for t in &self.project.tracks {
            if !matches!(t.auto_mode, AutoMode::Read | AutoMode::Touch | AutoMode::Latch) {
                continue;
            }
            for l in &t.automation {
                let AutoParam::Plugin { uid, param, .. } = &l.param else { continue };
                if self.auto.passes.iter().any(|p| p.track == t.id && p.param.same(&l.param)) {
                    continue;
                }
                let Some(v) = value_at(&l.points, pos) else { continue };
                if self.auto.shown.get(&(*uid, *param)) != Some(&v) {
                    moves.push((*uid, *param, v));
                }
            }
        }
        for (uid, param, v) in moves {
            if let Some(c) = self.controllers.get(&uid) {
                c.set_param_value(param, v as f64);
            }
            self.auto.shown.insert((uid, param), v);
        }
    }

    // ---- commands ---------------------------------------------------------------------------

    /// Replaces a lane's points (an empty list removes the lane). Values are clamped to the
    /// parameter's range.
    pub fn set_automation_lane(&mut self, track: Id, param: AutoParam, mut points: Vec<AutoPoint>) {
        let (lo, hi) = param.range();
        for p in &mut points {
            p.pos = p.pos.max(0);
            p.value = if p.value.is_finite() { p.value.clamp(lo, hi) } else { lo };
            if matches!(param, AutoParam::Mute) {
                p.value = p.value.round();
            }
        }
        points.sort_by_key(|p| p.pos);
        let Some(t) = self.project.track_mut(track) else { return };
        match t.automation.iter().position(|l| l.param.same(&param)) {
            Some(i) if points.is_empty() => {
                t.automation.remove(i);
            }
            Some(i) => t.automation[i] = AutoLane { param, points },
            None if points.is_empty() => {}
            None => t.automation.push(AutoLane { param, points }),
        }
        self.push_automation(track);
        self.dirty = true;
    }

    pub fn set_auto_mode(&mut self, track: Id, mode: AutoMode) {
        if self.project.track(track).is_none() {
            return;
        }
        // Passes in progress on the track end here.
        let (mine, rest): (Vec<Pass>, Vec<Pass>) = std::mem::take(&mut self.auto.passes).into_iter().partition(|p| p.track == track);
        self.auto.passes = rest;
        let pos = self.position();
        self.commit(mine, pos);
        if let Some(t) = self.project.track_mut(track) {
            t.auto_mode = mode;
        }
        self.send(Cmd::AutoMode(track, mode));
        if mode == AutoMode::Write && self.is_playing() {
            self.start_write_passes(track, pos);
        }
        self.dirty = true;
    }

    /// Deletes one lane, or all of the track's automation with `param` None.
    pub fn clear_automation(&mut self, track: Id, param: Option<AutoParam>) {
        let Some(t) = self.project.track_mut(track) else { return };
        match &param {
            Some(p) => t.automation.retain(|l| !l.param.same(p)),
            None => t.automation.clear(),
        }
        self.push_automation(track);
        self.dirty = true;
    }

    /// A plug-in instance's automatable parameters (none for the built-in synth).
    pub fn plugin_params(&self, uid: Id) -> Vec<ParamInfo> {
        self.controllers.get(&uid).map(|c| c.parameters()).unwrap_or_default()
    }

    /// Whether moving the track's controls now records automation (playing in Touch, Latch or
    /// Write). Those moves belong to the pass's undo step rather than getting their own.
    pub fn automation_writing(&self, track: Id) -> bool {
        self.is_playing() && self.project.track(track).map(|t| t.auto_mode.writes()).unwrap_or(false)
    }
}
