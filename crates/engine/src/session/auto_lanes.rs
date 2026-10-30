//! Automation beyond a track's own strip, and which lanes are shown: Stereo Out's lanes (the
//! Output track's), folder stacks' volume and mute, and the extra lanes shown under a track.
//!
//! Stereo Out is addressed as `MASTER_ID` by every automation command, as it is by the
//! channel strip commands.

use super::*;
use crate::model::AutoLane;

impl Session {
    /// A strip's automation lanes: a track's, or Stereo Out's for `MASTER_ID`.
    pub(super) fn strip_lanes(&self, track: Id) -> Option<&Vec<AutoLane>> {
        if track == MASTER_ID {
            return Some(&self.project.master.automation);
        }
        self.project.track(track).map(|t| &t.automation)
    }

    pub(super) fn strip_lanes_mut(&mut self, track: Id) -> Option<&mut Vec<AutoLane>> {
        if track == MASTER_ID {
            return Some(&mut self.project.master.automation);
        }
        self.project.track_mut(track).map(|t| &mut t.automation)
    }

    pub(super) fn auto_mode_of(&self, track: Id) -> Option<AutoMode> {
        if track == MASTER_ID {
            return Some(self.project.master.auto_mode);
        }
        self.project.track(track).map(|t| t.auto_mode)
    }

    /// Whether the strip has `param` to automate. A folder stack has no audio of its own, so
    /// only its volume and mute (which reach its subtracks); Stereo Out has no pan, mute or
    /// sends.
    pub(super) fn automatable(&self, track: Id, param: &AutoParam) -> bool {
        if track == MASTER_ID {
            return matches!(param, AutoParam::Volume | AutoParam::Plugin { .. });
        }
        match self.project.track(track) {
            Some(t) if t.kind == TrackKind::Folder => matches!(param, AutoParam::Volume | AutoParam::Mute),
            Some(_) => true,
            None => false,
        }
    }

    /// Sets the extra lanes shown below each listed track's main lane (`MASTER_ID` for the
    /// Output track). Parameters a strip can't automate, and repeats, are left out.
    pub fn set_auto_lanes(&mut self, lanes: Vec<(Id, Vec<AutoParam>)>) {
        for (track, params) in lanes {
            let mut keep: Vec<AutoParam> = Vec::new();
            for p in params {
                if self.automatable(track, &p) && !keep.iter().any(|k| k.same(&p)) {
                    keep.push(p);
                }
            }
            if track == MASTER_ID {
                self.project.master.auto_lanes = keep;
            } else if let Some(t) = self.project.track_mut(track) {
                t.auto_lanes = keep;
            }
        }
        self.dirty = true;
    }

    /// The Stereo Out fader as the user moves it: while playing in Touch, Latch or Write the
    /// move is recorded as automation, as a track's fader is.
    pub fn set_master_volume_linked(&mut self, db: f32) {
        let before = self.project.master.volume_db;
        self.set_master_volume(db);
        self.auto_moved(MASTER_ID, [Some(before), None, None]);
    }

    /// Sends Stereo Out's lanes and mode to the engine (project load and undo).
    pub(super) fn sync_master_automation(&mut self) {
        let lanes = crate::automation::lanes_dsp(&self.project.master.automation);
        self.send(Cmd::Automation(MASTER_ID, lanes));
        self.send(Cmd::AutoMode(MASTER_ID, self.project.master.auto_mode));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AutoPoint;

    fn session() -> Session {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        Session::new(cfg, Vec::new())
    }

    #[test]
    fn stereo_out_lanes_are_edited_like_a_tracks() {
        let mut s = session();
        let pts = vec![AutoPoint { pos: 0, value: -12.0, curve: 0.5 }, AutoPoint { pos: 4800, value: 0.0, curve: 0.0 }];
        s.set_automation_lane(MASTER_ID, AutoParam::Volume, pts.clone());
        assert_eq!(s.project.master.automation.len(), 1);
        assert_eq!(s.project.master.automation[0].points, pts);
        // Stereo Out has no pan: such a lane isn't kept, and can't be shown.
        s.set_auto_lanes(vec![(MASTER_ID, vec![AutoParam::Pan, AutoParam::Volume, AutoParam::Volume])]);
        assert_eq!(s.project.master.auto_lanes.len(), 1);
        s.set_auto_mode(MASTER_ID, AutoMode::Off);
        assert_eq!(s.project.master.auto_mode, AutoMode::Off);
        s.clear_automation(MASTER_ID, None);
        assert!(s.project.master.automation.is_empty());
    }

    #[test]
    fn folders_automate_volume_and_mute_only() {
        let mut s = session();
        let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        let f = s.create_stack(&[a], false).unwrap();
        assert!(s.automatable(f, &AutoParam::Volume) && s.automatable(f, &AutoParam::Mute));
        assert!(!s.automatable(f, &AutoParam::Pan) && !s.automatable(f, &AutoParam::Send(0)));
        assert!(s.automatable(a, &AutoParam::Pan));
        s.set_auto_lanes(vec![(f, vec![AutoParam::Mute, AutoParam::Pan])]);
        assert_eq!(s.project.track(f).unwrap().auto_lanes.len(), 1);
    }

    #[test]
    fn subtracks_know_their_folder() {
        let mut s = session();
        let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        let inner = s.create_stack(&[a], true).unwrap();
        let outer = s.create_stack(&[inner], false).unwrap();
        s.update_track(outer, TrackPatch { volume_db: Some(-6.0), mute: Some(true), ..Default::default() });
        s.update_track(inner, TrackPatch { mute: Some(true), ..Default::default() });
        let p = Session::track_params(&s.project, s.project.track(a).unwrap());
        assert_eq!(p.folder, Some(outer));
        assert_eq!(p.own_gain, 1.0);
        assert!(!p.own_mute && p.bus_mute && p.stack_mute);
        assert!((p.stack_gain - crate::dsp::volume_gain(-6.0)).abs() < 1e-6);
    }
}
