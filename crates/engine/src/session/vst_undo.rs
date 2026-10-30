//! Undo for parameter changes made in a VST3's own editor.
//!
//! The editor reports its edits through `IComponentHandler` (`Gesture`). One begin..end becomes
//! one undo step; edits without a begin and end (some plug-ins do that) are a step once they go
//! quiet, and bursts on one parameter share a step through the session's undo keys.
//!
//! A step needs the plug-in's state from before the edit, and asking for it takes `getState`,
//! which by then would be too late. So each plug-in with an open editor has a `Watch` holding
//! its last known state: read when the editor opens, again once each gesture has ended, and
//! when the host has changed the plug-in (automation following, a controller assignment), once
//! that goes quiet. A drag calls `getState` once, after the mouse button comes up, however
//! long it takes. Its answer is compared with the known state, so a click that changes nothing
//! makes no step, and no plug-in without an open editor is ever asked.
//!
//! Undo and redo load the state into the plug-in (`load_state`) for the entry that has one. What
//! the plug-in answers to that isn't an edit, so its gestures are dropped for a moment.
//!
//! A plug-in may also move its own parameters, like a meter or a modulator without a read-only
//! flag. Edits outside a gesture therefore have to earn their step (see `Tracker`).

use std::collections::{HashSet, VecDeque};

use vst3_host::Gesture;

use super::*;

/// How long after a gesture ends its step waits for the edits to reach the processor, whose
/// state is what `getState` reads. A gesture starting by then joins the step: nobody's that
/// fast, but a plug-in loading a preset as a run of gestures is.
const SETTLE: Duration = Duration::from_millis(60);
/// How long edits outside a gesture have to pause to be one burst.
const QUIET: Duration = Duration::from_millis(250);
/// How long a step waits for edits to reach a processor that isn't taking them.
const DRAIN_WAIT: Duration = Duration::from_millis(300);
/// An open gesture this long without an edit lost its end.
const STUCK: Duration = Duration::from_secs(10);
/// A burst without a pause this long is the plug-in itself: nobody turns a knob that long.
const BURST_MAX: Duration = Duration::from_secs(10);
/// At most this many steps outside gestures, per plug-in, in `CAP_WINDOW`.
const CAP_STEPS: usize = 8;
const CAP_WINDOW: Duration = Duration::from_secs(30);
/// Bursts on a parameter that left the state as it was, before the parameter is ignored.
const DUD_BURSTS: u32 = 3;
/// How long after a restore the plug-in's edits count as its answer to it.
const RESTORE_ECHO: Duration = Duration::from_millis(300);
/// The host's own changes to a plug-in have to pause this long before its state is read again...
const REFRESH_QUIET: Duration = Duration::from_millis(300);
/// ...at most this often, and spending at most 1 % of the time on a heavy plug-in.
const REFRESH_MIN: Duration = Duration::from_secs(1);
const REFRESH_SHARE: u32 = 100;

/// An edit on its way to becoming an undo step.
#[derive(Debug, Clone, PartialEq)]
struct Pending {
    /// In a gesture rather than a burst of bare edits.
    bracketed: bool,
    /// The parameters moved, first one first.
    params: Vec<u32>,
    /// `restartComponent(kParamValuesChanged)`: all of them, perhaps.
    restarted: bool,
    started: Instant,
    last: Instant,
}

/// Which edits become steps, and when: everything about them that doesn't need a plug-in.
#[derive(Debug, Default)]
struct Tracker {
    /// The parameters in open gestures.
    open: Vec<u32>,
    pending: Option<Pending>,
    /// Each parameter's last value, as edits outside gestures sometimes repeat it.
    values: HashMap<u32, f64>,
    read_only: HashSet<u32>,
    /// Parameters found to move by themselves: they make no steps.
    own: HashSet<u32>,
    duds: HashMap<u32, u32>,
    /// When the steps outside gestures were made, for the cap.
    steps: VecDeque<Instant>,
    /// Edits until this time are a plug-in answering a restore.
    muted_until: Option<Instant>,
    /// The plug-in changed in a way that made no step: its state needs reading again.
    drifted: bool,
}

impl Tracker {
    fn muted(&self, now: Instant) -> bool {
        self.muted_until.is_some_and(|t| now < t)
    }

    /// Whether this edit starts a gesture while a burst waits for its pause, which ends the
    /// burst: its step must be made before the edit is applied.
    fn ends_burst(&self, g: Gesture) -> bool {
        matches!(g, Gesture::Begin(id) if !self.read_only.contains(&id)) && self.pending.as_ref().is_some_and(|p| !p.bracketed)
    }

    /// `echo`: the value is one the host has just set.
    fn apply(&mut self, g: Gesture, echo: bool, now: Instant) {
        match g {
            Gesture::Begin(id) => {
                if self.read_only.contains(&id) {
                    return;
                }
                self.open.push(id);
                let p = self.pending.get_or_insert(Pending { bracketed: true, params: Vec::new(), restarted: false, started: now, last: now });
                p.last = now;
            }
            Gesture::Perform(id, v) => {
                if self.read_only.contains(&id) || echo {
                    return;
                }
                let same = self.values.insert(id, v) == Some(v);
                match &mut self.pending {
                    Some(p) if p.bracketed => {
                        if !p.params.contains(&id) {
                            p.params.push(id);
                        }
                        p.last = now;
                    }
                    _ if self.own.contains(&id) => self.drifted = true,
                    _ if same => {}
                    _ => self.burst(id, now),
                }
            }
            Gesture::End(id) => {
                let Some(i) = self.open.iter().position(|&o| o == id) else { return };
                self.open.remove(i);
                // Clicked, not moved: nothing happened.
                if self.open.is_empty() && self.pending.as_ref().is_some_and(|p| p.bracketed && p.params.is_empty()) {
                    self.pending = None;
                }
            }
        }
    }

    /// An edit outside a gesture.
    fn burst(&mut self, id: u32, now: Instant) {
        self.prune_steps(now);
        if self.pending.is_none() && self.capped() {
            self.own.insert(id);
            self.drifted = true;
            return;
        }
        let p = self.pending.get_or_insert(Pending { bracketed: false, params: Vec::new(), restarted: false, started: now, last: now });
        if !p.params.contains(&id) {
            p.params.push(id);
        }
        p.last = now;
        if now.duration_since(p.started) > BURST_MAX {
            let own = std::mem::take(&mut p.params);
            self.own.extend(own);
            self.pending = None;
            self.drifted = true;
        }
    }

    /// `restartComponent(kParamValuesChanged)`. A plug-in that keeps doing it gets the cap.
    fn restarted(&mut self, now: Instant) {
        self.prune_steps(now);
        if self.pending.is_none() && self.capped() {
            self.drifted = true;
            return;
        }
        let p = self.pending.get_or_insert(Pending { bracketed: false, params: Vec::new(), restarted: false, started: now, last: now });
        p.restarted = true;
        p.last = now;
    }

    fn prune_steps(&mut self, now: Instant) {
        while self.steps.front().is_some_and(|t| now.duration_since(*t) > CAP_WINDOW) {
            self.steps.pop_front();
        }
    }

    fn capped(&self) -> bool {
        self.steps.len() >= CAP_STEPS
    }

    /// Whether the pending edit should become a step now. `drained`: its edits have reached the
    /// processor.
    fn due(&self, now: Instant, drained: bool) -> bool {
        let Some(p) = &self.pending else { return false };
        let waited = now.duration_since(p.last);
        if !self.open.is_empty() {
            return waited > STUCK;
        }
        let pause = if p.bracketed { SETTLE } else { QUIET };
        waited >= pause && (drained || waited >= pause + DRAIN_WAIT)
    }

    /// The pending edit, to make its step. `None` if it never moved anything.
    fn take(&mut self) -> Option<Pending> {
        self.open.clear();
        self.pending.take().filter(|p| p.restarted || !p.params.is_empty())
    }

    /// How a pending edit turned out: `changed`, whether the plug-in's state did.
    fn finished(&mut self, p: &Pending, changed: bool) {
        if p.bracketed {
            return;
        }
        for id in &p.params {
            if changed {
                self.duds.remove(id);
            } else {
                let n = self.duds.entry(*id).or_default();
                *n += 1;
                if *n >= DUD_BURSTS {
                    self.own.insert(*id);
                }
            }
        }
    }

    /// A step was made for an edit outside a gesture.
    fn stepped(&mut self, now: Instant) {
        self.steps.push_back(now);
    }

    /// Everything in progress is over: the plug-in has just been put back.
    fn restored(&mut self, now: Instant) {
        self.open.clear();
        self.pending = None;
        self.values.clear();
        self.drifted = false;
        self.muted_until = Some(now + RESTORE_ECHO);
    }
}

/// A plug-in whose editor is open.
pub(super) struct Watch {
    /// Its last known state: what a step takes back to.
    known: Arc<PluginState>,
    /// `PluginController::host_changes` as of `known`.
    seen_host: u64,
    /// When the host (or the plug-in, unasked) last changed it after `known` was read.
    stale: Option<Instant>,
    read_at: Instant,
    read_took: Duration,
    /// Parameter names, for the steps' labels.
    names: HashMap<u32, String>,
    /// Watched without an editor: for headless checks.
    headless: bool,
    /// How many times `getState` has been called for it.
    reads: u32,
    tracker: Tracker,
}

impl Session {
    /// Starts watching a plug-in for edits in its editor, reading its state.
    pub(super) fn watch_vst_edits(&mut self, uid: Id, headless: bool) {
        let Some(c) = self.controllers.get_mut(&uid) else { return };
        if self.vst_watches.contains_key(&uid) {
            return;
        }
        c.take_gestures();
        c.take_params_restarted();
        let start = Instant::now();
        let known = Arc::new(c.save_state());
        let read_took = start.elapsed();
        let mut tracker = Tracker::default();
        let mut names = HashMap::new();
        for (id, name, read_only) in c.param_names() {
            if read_only {
                tracker.read_only.insert(id);
            }
            names.insert(id, name);
        }
        let watch = Watch { known, seen_host: c.host_changes(), stale: None, read_at: Instant::now(), read_took, names, headless, reads: 1, tracker };
        self.vst_watches.insert(uid, watch);
    }

    /// Watches a plug-in as if its editor were open, for headless checks.
    #[doc(hidden)]
    pub fn watch_vst_edits_headless(&mut self, uid: Id) {
        self.watch_vst_edits(uid, true);
    }

    /// Calls the host as a plug-in's editor would, for headless checks.
    #[doc(hidden)]
    pub fn drive_vst_handler(&self, uid: Id, g: Gesture) {
        if let Some(c) = self.controllers.get(&uid) {
            c.drive_handler(g);
        }
    }

    /// `restartComponent(kParamValuesChanged)` from a plug-in, for headless checks.
    #[doc(hidden)]
    pub fn drive_vst_params_restarted(&self, uid: Id) {
        if let Some(c) = self.controllers.get(&uid) {
            c.drive_params_restarted();
        }
    }

    /// How many times a watched plug-in's state has been read, for headless checks.
    #[doc(hidden)]
    pub fn vst_state_reads(&self, uid: Id) -> u32 {
        self.vst_watches.get(&uid).map_or(0, |w| w.reads)
    }

    /// How many undo and redo steps there are, for headless checks.
    #[doc(hidden)]
    pub fn undo_depths(&self) -> (usize, usize) {
        (self.undo_stack.len(), self.redo_stack.len())
    }

    /// Takes the edits an `idle` found. Returns true when they should go no further, being a
    /// plug-in's answer to a restore.
    pub(super) fn feed_vst_edits(&mut self, uid: Id, gestures: &[Gesture]) -> bool {
        if gestures.is_empty() || !self.vst_watches.contains_key(&uid) {
            return false;
        }
        let now = Instant::now();
        if self.vst_watches[&uid].tracker.muted(now) {
            return true;
        }
        // Automation being written takes the move, and has the undo step for it.
        let writing = self.is_playing() && self.track_of_plugin(uid).and_then(|t| self.auto_mode_of(t)).is_some_and(|m| m.writes());
        for &g in gestures {
            if writing && !matches!(g, Gesture::End(_)) {
                if let Some(w) = self.vst_watches.get_mut(&uid) {
                    w.tracker.drifted = true;
                }
                continue;
            }
            if self.vst_watches[&uid].tracker.ends_burst(g) {
                self.finish_vst_edit(uid);
            }
            let echo = match g {
                Gesture::Perform(id, v) => self.controllers.get(&uid).is_some_and(|c| c.host_echo(id, v)),
                _ => false,
            };
            if let Some(w) = self.vst_watches.get_mut(&uid) {
                w.tracker.apply(g, echo, now);
            }
        }
        false
    }

    /// Periodic work from `idle`: edits that have finished become steps, and known states
    /// catch up. Returns true when a step was made, for a new snapshot.
    pub(super) fn vst_undo_idle(&mut self) -> bool {
        let now = Instant::now();
        let mut stepped = false;
        let uids: Vec<Id> = self.vst_watches.keys().copied().collect();
        for uid in uids {
            let Some(w) = self.vst_watches.get_mut(&uid) else { continue };
            let Some(c) = self.controllers.get_mut(&uid) else {
                self.vst_watches.remove(&uid);
                continue;
            };
            if !w.headless && !c.editor_open() {
                stepped |= self.finish_vst_edit(uid);
                self.vst_watches.remove(&uid);
                continue;
            }
            if c.take_params_restarted() && !w.tracker.muted(now) {
                w.tracker.restarted(now);
            }
            if c.host_changes() != w.seen_host {
                w.seen_host = c.host_changes();
                w.stale = Some(now);
            }
            if std::mem::take(&mut w.tracker.drifted) {
                w.stale = Some(now);
            }
            if w.tracker.due(now, !c.edits_pending()) {
                stepped |= self.finish_vst_edit(uid);
            } else if w.tracker.pending.is_none() && w.stale.is_some_and(|t| now.duration_since(t) >= REFRESH_QUIET) && now.duration_since(w.read_at) >= REFRESH_MIN.max(w.read_took * REFRESH_SHARE) {
                let start = Instant::now();
                w.known = Arc::new(c.save_state());
                w.reads += 1;
                w.read_took = start.elapsed();
                w.read_at = now;
                w.stale = None;
            }
        }
        stepped
    }

    /// Makes the step for a plug-in's pending edit, if it changed the plug-in. Returns whether
    /// it did.
    fn finish_vst_edit(&mut self, uid: Id) -> bool {
        let Some(w) = self.vst_watches.get_mut(&uid) else { return false };
        let Some(p) = w.tracker.take() else { return false };
        let Some(c) = self.controllers.get(&uid) else { return false };
        let start = Instant::now();
        let state = c.save_state();
        w.reads += 1;
        let now = Instant::now();
        w.read_took = now.duration_since(start);
        w.read_at = now;
        let changed = state.component != w.known.component || state.controller != w.known.controller;
        w.tracker.finished(&p, changed);
        if !changed {
            return false;
        }
        let before = std::mem::replace(&mut w.known, Arc::new(state));
        w.seen_host = c.host_changes();
        w.stale = None;
        let one = p.params.first().filter(|_| p.params.len() == 1 && !p.restarted);
        let label = match one.and_then(|id| w.names.get(id)) {
            Some(name) => format!("{}: {name}", c.desc.name),
            None => format!("{}: Parameters", c.desc.name),
        };
        let key = one.filter(|_| !p.bracketed).map(|id| format!("vst-edit:{uid}:{id}"));
        let pushed = self.push_checkpoint(&label, key, false);
        if pushed {
            if let Some(top) = self.undo_stack.last_mut() {
                top.plugin = Some((uid, before));
            }
            if !p.bracketed {
                if let Some(w) = self.vst_watches.get_mut(&uid) {
                    w.tracker.stepped(now);
                }
            }
        }
        self.dirty = true;
        true
    }

    /// Makes the steps for edits still pending, ahead of an undo, a redo or another edit: their
    /// order in the history must be the order they happened in.
    pub(super) fn flush_vst_edits(&mut self) {
        let uids: Vec<Id> = self.vst_watches.iter().filter(|(_, w)| w.tracker.pending.is_some()).map(|(&uid, _)| uid).collect();
        for uid in uids {
            // Edits still in the ring would land on top of what an undo puts back. Only ever
            // waits when a command comes in the moment after the mouse button came up.
            let deadline = Instant::now() + Duration::from_millis(50);
            while self.audio.is_some() && Instant::now() < deadline && self.controllers.get(&uid).is_some_and(|c| c.edits_pending()) {
                std::thread::sleep(Duration::from_millis(1));
            }
            if let Some(left) = self.vst_watches[&uid].tracker.pending.as_ref().and_then(|p| Duration::from_millis(20).checked_sub(p.last.elapsed())) {
                std::thread::sleep(left);
            }
            self.finish_vst_edit(uid);
        }
    }

    /// A plug-in's state as of now, for the step that comes back to it.
    pub(super) fn vst_state_now(&self, uid: Id) -> Arc<PluginState> {
        if let Some(c) = self.controllers.get(&uid) {
            return Arc::new(c.save_state());
        }
        let slot = self.slots_of_project().find(|s| s.uid == uid);
        Arc::new(slot.and_then(|s| s.state.as_deref()).map(decode_state).unwrap_or_default())
    }

    fn slots_of_project(&self) -> impl Iterator<Item = &PluginSlot> {
        let tracks = self.project.tracks.iter().flat_map(|t| t.instrument.iter().chain(t.inserts.iter().flatten()));
        tracks.chain(self.project.master.inserts.iter().flatten())
    }

    /// Puts a plug-in back: processor, controller and so an open editor. It makes no step,
    /// whatever the plug-in answers. A plug-in that isn't loaded (it's frozen or missing) gets
    /// the state when it next loads.
    pub(super) fn restore_vst_state(&mut self, uid: Id, st: Arc<PluginState>) {
        let Some(c) = self.controllers.get_mut(&uid) else {
            let encoded = encode_state(&st);
            let tracks = self.project.tracks.iter_mut().flat_map(|t| t.instrument.iter_mut().chain(t.inserts.iter_mut().flatten()));
            if let Some(slot) = tracks.chain(self.project.master.inserts.iter_mut().flatten()).find(|s| s.uid == uid) {
                slot.state = Some(encoded);
            }
            return;
        };
        c.load_state(&st);
        c.take_gestures();
        let seen_host = c.host_changes();
        if let Some(w) = self.vst_watches.get_mut(&uid) {
            w.known = st;
            w.seen_host = seen_host;
            w.stale = None;
            w.tracker.restored(Instant::now());
        }
        // Automation following has to show it again.
        self.auto.shown.retain(|(u, _), _| *u != uid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn a_gesture_is_one_edit_however_many_values_it_sends() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        t.apply(Gesture::Begin(1), false, s);
        for i in 0..200 {
            t.apply(Gesture::Perform(1, i as f64 / 200.0), false, at(s, i * 10));
            assert!(!t.due(at(s, i * 10 + 500), true), "not while it's open");
        }
        t.apply(Gesture::End(1), false, at(s, 2000));
        assert!(!t.due(at(s, 2000), true), "waits for the edits to reach the processor");
        assert!(t.due(at(s, 2000) + SETTLE, true));
        let p = t.take().unwrap();
        assert_eq!((p.bracketed, p.params), (true, vec![1]));
        assert!(t.pending.is_none() && t.open.is_empty());
    }

    #[test]
    fn a_click_that_moves_nothing_is_no_edit() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        t.apply(Gesture::Begin(1), false, s);
        t.apply(Gesture::End(1), false, s);
        assert!(t.pending.is_none());
    }

    #[test]
    fn a_run_of_gestures_at_once_is_one_edit() {
        // What a plug-in loading a preset by hand sends.
        let (mut t, s) = (Tracker::default(), Instant::now());
        for id in 0..50 {
            t.apply(Gesture::Begin(id), false, s);
            t.apply(Gesture::Perform(id, 0.5), false, s);
            t.apply(Gesture::End(id), false, s);
        }
        assert_eq!(t.take().unwrap().params.len(), 50);
    }

    #[test]
    fn overlapping_gestures_end_with_the_last() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        t.apply(Gesture::Begin(1), false, s);
        t.apply(Gesture::Begin(2), false, s);
        t.apply(Gesture::Perform(1, 0.1), false, s);
        t.apply(Gesture::Perform(2, 0.1), false, s);
        t.apply(Gesture::End(1), false, s);
        assert!(!t.due(at(s, 1000), true));
        t.apply(Gesture::End(2), false, at(s, 1000));
        assert!(t.due(at(s, 1100), true));
    }

    #[test]
    fn a_gesture_that_loses_its_end_still_finishes() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        t.apply(Gesture::Begin(1), false, s);
        t.apply(Gesture::Perform(1, 0.1), false, s);
        assert!(!t.due(s + STUCK, true));
        assert!(t.due(s + STUCK + Duration::from_millis(1), true));
    }

    #[test]
    fn bare_edits_are_a_burst_until_they_pause() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        for i in 0..20 {
            t.apply(Gesture::Perform(1, i as f64), false, at(s, i * 50));
            assert!(!t.due(at(s, i * 50 + 100), true));
        }
        assert!(t.due(at(s, 19 * 50) + QUIET, true));
        let p = t.take().unwrap();
        assert_eq!((p.bracketed, p.params), (false, vec![1]));
    }

    #[test]
    fn a_step_waits_for_a_processor_that_is_behind_but_not_forever() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        t.apply(Gesture::Begin(1), false, s);
        t.apply(Gesture::Perform(1, 0.1), false, s);
        t.apply(Gesture::End(1), false, s);
        assert!(!t.due(s + SETTLE, false));
        assert!(t.due(s + SETTLE + DRAIN_WAIT, false));
    }

    #[test]
    fn a_gesture_ends_a_burst() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        t.apply(Gesture::Perform(1, 0.5), false, s);
        assert!(t.ends_burst(Gesture::Begin(2)));
        assert!(!t.ends_burst(Gesture::Perform(2, 0.5)));
    }

    #[test]
    fn read_only_parameters_and_echoes_are_ignored() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        t.read_only.insert(9);
        t.apply(Gesture::Begin(9), false, s);
        t.apply(Gesture::Perform(9, 0.5), false, s);
        t.apply(Gesture::End(9), false, s);
        t.apply(Gesture::Perform(1, 0.5), true, s);
        assert!(t.pending.is_none());
        assert!(!t.ends_burst(Gesture::Begin(9)));
    }

    #[test]
    fn repeating_a_value_is_not_an_edit() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        t.apply(Gesture::Perform(1, 0.5), false, s);
        t.take();
        t.apply(Gesture::Perform(1, 0.5), false, at(s, 1000));
        assert!(t.pending.is_none());
    }

    #[test]
    fn a_parameter_that_never_stops_moving_makes_no_step_and_is_then_ignored() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        let mut ms = 0;
        while s + Duration::from_millis(ms) <= s + BURST_MAX + Duration::from_millis(100) {
            t.apply(Gesture::Perform(7, ms as f64), false, at(s, ms));
            ms += 33;
        }
        assert!(t.pending.is_none());
        assert!(t.own.contains(&7));
        t.apply(Gesture::Perform(7, 0.25), false, at(s, ms + 5000));
        assert!(t.pending.is_none() && t.drifted);
    }

    #[test]
    fn steps_outside_gestures_are_capped() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        let mut made = 0;
        // A plug-in that moves itself every two seconds, changing its state each time.
        for i in 0..40u64 {
            let now = at(s, i * 2000);
            t.apply(Gesture::Perform(3, i as f64), false, now);
            if let Some(p) = t.take() {
                t.finished(&p, true);
                t.stepped(now);
                made += 1;
            }
        }
        assert_eq!(made, CAP_STEPS, "then it's taken to be moving itself");
        assert!(t.own.contains(&3));
        // A knob the user moves in a gesture is another matter.
        t.apply(Gesture::Begin(3), false, at(s, 90_000));
        t.apply(Gesture::Perform(3, 0.125), false, at(s, 90_000));
        t.apply(Gesture::End(3), false, at(s, 90_000));
        assert!(t.take().is_some());
    }

    #[test]
    fn bursts_that_change_nothing_stop_being_read() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        for i in 0..DUD_BURSTS {
            t.apply(Gesture::Perform(4, i as f64), false, at(s, i as u64 * 1000));
            let p = t.take().unwrap();
            t.finished(&p, false);
        }
        t.apply(Gesture::Perform(4, 99.0), false, at(s, 9000));
        assert!(t.pending.is_none(), "an output parameter: no more calls to getState");
    }

    #[test]
    fn restarts_join_what_is_pending_and_get_the_cap() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        t.restarted(s);
        t.restarted(at(s, 10));
        assert!(t.due(at(s, 10) + QUIET, true));
        assert!(t.take().unwrap().restarted);
        for _ in 0..CAP_STEPS {
            t.stepped(at(s, 20));
        }
        t.restarted(at(s, 30));
        assert!(t.pending.is_none() && t.drifted);
    }

    #[test]
    fn a_restore_ends_everything_and_mutes_the_answers() {
        let (mut t, s) = (Tracker::default(), Instant::now());
        t.apply(Gesture::Begin(1), false, s);
        t.apply(Gesture::Perform(1, 0.2), false, s);
        t.restored(s);
        assert!(t.pending.is_none() && t.open.is_empty());
        assert!(t.muted(at(s, 100)));
        assert!(!t.muted(s + RESTORE_ECHO));
        // A knob still held goes on as bare edits.
        t.apply(Gesture::End(1), false, s + RESTORE_ECHO);
        assert!(t.pending.is_none());
    }
}
