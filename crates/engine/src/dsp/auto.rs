//! Automation that reaches past a track's own strip: folder stacks, whose volume and mute
//! (automated or not) scale and silence their subtracks, and Stereo Out's lanes (its volume
//! and plug-in parameters). Track lanes themselves are played in `process_block`.

use super::*;

/// What a folder stack does to its subtracks this block. Worked out before the tracks play,
/// because a folder sits after its subtracks in the processing order.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct FoldState {
    /// The folder's gain, with the folders it is in.
    pub gain: f32,
    /// The folder, or a stack it is in, is muted.
    pub mute: bool,
    /// Automation drives the folder or a folder it is in: its subtracks take `gain` and
    /// `mute` instead of the stack settings the session worked out.
    pub live: bool,
}

/// Stereo Out's automation, as the engine plays it.
pub(super) struct MasterAuto {
    pub lanes: Arc<Vec<LaneDsp>>,
    pub mode: AutoMode,
    pub touched: [Option<AutoTarget>; MAX_TOUCHED],
    pub sent: [f32; MAX_AUTO_SENT],
    /// The volume the lane plays this block (linear), when it drives the fader.
    pub volume: Option<f32>,
}

impl Default for MasterAuto {
    fn default() -> Self {
        MasterAuto { lanes: Arc::new(Vec::new()), mode: AutoMode::Read, touched: [None; MAX_TOUCHED], sent: [f32::NAN; MAX_AUTO_SENT], volume: None }
    }
}

/// Whether lanes play in this mode (Write plays the live values being recorded over them).
pub(super) fn reads(mode: AutoMode) -> bool {
    matches!(mode, AutoMode::Read | AutoMode::Touch | AutoMode::Latch)
}

/// Puts a folder's state into a strip's settings: the static stack part the session worked out
/// gives way to the folder's gain and mute of this block.
pub(super) fn apply_fold(p: &mut TrackParams, f: FoldState) {
    if !f.live {
        return;
    }
    p.stack_gain = f.gain;
    p.stack_mute = f.mute || p.bus_mute;
    p.volume = p.own_gain * p.stack_gain;
    p.mute = p.own_mute || p.stack_mute;
}

impl Engine {
    /// Works out every folder stack's [`FoldState`] for the block at the playhead. Parents
    /// come after their subtracks in the list, so walking backwards meets them first.
    pub(super) fn update_folders(&mut self) {
        let playing = self.playing;
        for i in (0..self.tracks.len()).rev() {
            if self.tracks[i].kind != TrackKind::Folder {
                continue;
            }
            let parent = self.tracks[i].fold_idx.filter(|&j| j > i).map(|j| self.tracks[j].fold).unwrap_or_default();
            let t = &mut self.tracks[i];
            let mut p = t.params;
            apply_fold(&mut p, parent);
            let mut live = parent.live;
            if playing && reads(t.auto_mode) {
                let at = self.pos - t.pdc.output_time as i64;
                for lane in t.automation.iter() {
                    if t.touched.contains(&Some(lane.target)) {
                        continue;
                    }
                    let Some(v) = value_at(&lane.points, at) else { continue };
                    match lane.target {
                        AutoTarget::Volume => p.volume = volume_gain(v) * p.stack_gain,
                        AutoTarget::Mute => p.mute = v >= 0.5 || p.stack_mute,
                        // A folder has no audio: pan, sends and plug-ins don't apply.
                        _ => continue,
                    }
                    live = true;
                }
            }
            t.fold = FoldState { gain: p.volume, mute: p.mute, live };
        }
    }

    /// Plays Stereo Out's lanes for the block: sets `master_auto.volume` and sends plug-in
    /// values to Stereo Out's plug-ins. Its audio lags the timeline by the whole graph's delay
    /// compensation, so the lanes are read that far back.
    pub(super) fn automate_master(&mut self, n: usize) {
        let a = &mut self.master_auto;
        a.volume = None;
        if !self.playing || !reads(a.mode) {
            return;
        }
        let at = self.pos - self.pdc.total as i64;
        for (k, lane) in a.lanes.iter().enumerate() {
            if a.touched.contains(&Some(lane.target)) {
                continue;
            }
            let Some(v) = value_at(&lane.points, at) else { continue };
            match lane.target {
                AutoTarget::Volume => a.volume = Some(volume_gain(v)),
                AutoTarget::Plugin { uid, param } => {
                    // Two points per block: the plug-in ramps between them.
                    let v1 = value_at(&lane.points, at + n as i64 - 1).unwrap_or(v);
                    let sent = a.sent.get(k).copied().unwrap_or(f32::NAN);
                    if v == sent && v1 == sent {
                        continue;
                    }
                    if let Some(ins) = self.master_inserts.iter_mut().flatten().find(|i| i.uid == uid) {
                        ins.plugin.set_param(param, 0, v as f64);
                        if v1 != v {
                            ins.plugin.set_param(param, n as i32 - 1, v1 as f64);
                        }
                    }
                    if let Some(s) = a.sent.get_mut(k) {
                        *s = v1;
                    }
                }
                // Stereo Out has no pan, mute or sends.
                _ => {}
            }
        }
    }

    pub(super) fn set_master_lanes(&mut self, lanes: Arc<Vec<LaneDsp>>) {
        let old = std::mem::replace(&mut self.master_auto.lanes, lanes);
        self.master_auto.sent = [f32::NAN; MAX_AUTO_SENT];
        self.discard(Garbage::Automation(old));
    }

    pub(super) fn set_master_touched(&mut self, target: AutoTarget, on: bool) {
        let a = &mut self.master_auto;
        let at = a.touched.iter().position(|t| *t == Some(target));
        match (on, at) {
            (true, None) => {
                if let Some(free) = a.touched.iter_mut().find(|t| t.is_none()) {
                    *free = Some(target);
                }
            }
            (false, Some(i)) => a.touched[i] = None,
            _ => {}
        }
        a.sent = [f32::NAN; MAX_AUTO_SENT];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> TrackParams {
        TrackParams {
            volume: 0.25,
            pan: 0.0,
            mute: true,
            solo: false,
            armed: false,
            monitor: false,
            input: InputSel::None,
            output: None,
            sends: [None; SEND_SLOTS],
            // The session's static values: a folder at -6 dB, muted.
            stack_gain: 0.5,
            stack_mute: true,
            folder: Some(7),
            own_gain: 0.5,
            own_mute: false,
            bus_mute: false,
        }
    }

    #[test]
    fn a_folder_without_automation_leaves_the_static_settings() {
        let mut p = params();
        apply_fold(&mut p, FoldState { gain: 0.1, mute: false, live: false });
        assert_eq!((p.volume, p.mute, p.stack_gain), (0.25, true, 0.5));
    }

    #[test]
    fn a_playing_folder_lane_replaces_the_stack_part() {
        let mut p = params();
        apply_fold(&mut p, FoldState { gain: 0.1, mute: false, live: true });
        assert!((p.volume - 0.05).abs() < 1e-7 && !p.mute && p.stack_gain == 0.1 && !p.stack_mute);
        // A muted summing stack above still mutes, and the track's own mute still counts.
        let mut p = TrackParams { bus_mute: true, ..params() };
        apply_fold(&mut p, FoldState { gain: 1.0, mute: false, live: true });
        assert!(p.mute && p.stack_mute);
        let mut p = TrackParams { own_mute: true, stack_mute: false, mute: true, ..params() };
        apply_fold(&mut p, FoldState { gain: 1.0, mute: false, live: true });
        assert!(p.mute && !p.stack_mute);
    }

    #[test]
    fn only_reading_modes_play_lanes() {
        assert!(reads(AutoMode::Read) && reads(AutoMode::Touch) && reads(AutoMode::Latch));
        assert!(!reads(AutoMode::Off) && !reads(AutoMode::Write));
    }
}
