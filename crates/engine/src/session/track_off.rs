//! Logic's Track On/Off, the session's side. `Track::off` is switched with `update_track`, like
//! Mute (a performance switch: no undo step of its own, and an undo or redo puts back the state
//! its snapshot had). `Project::is_off` applies the stacks above a track, `track_params` tells the
//! engine, and `update_pdc` leaves the track out of delay compensation. See `dsp/track_off.rs`.
//!
//! An Off track can't record. Record skips it and keeps its R button for later, and refuses when
//! no other track is enabled, so nothing is armed that won't be taken. Switching any track while
//! recording is refused: the track might be in the take, and the delay compensation the take is
//! placed with must stay as it is. Renders of a single track that's off (Freeze, Bounce in Place,
//! Export Region) are refused rather than writing silence.

use super::*;

impl Session {
    /// Errors (naming the track) if any of `tracks` is off.
    pub(super) fn check_on(&self, tracks: &[Id]) -> Result<()> {
        match tracks.iter().find(|id| self.project.is_off(**id)).and_then(|id| self.project.track(*id)) {
            Some(t) => Err(anyhow!("{} is off. Turn it on first.", t.name)),
            None => Ok(()),
        }
    }

    /// Why a track can't be switched on or off right now, if so.
    pub fn off_refused(&self) -> Option<String> {
        self.rec_punch.is_some().then(|| "Stop recording before turning a track on or off.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
        Session::new(cfg, Vec::new())
    }

    fn audio(s: &mut Session) -> Id {
        s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap()
    }

    fn set_off(s: &mut Session, id: Id, off: bool) {
        s.update_track(id, TrackPatch { off: Some(off), ..Default::default() });
    }

    #[test]
    fn latency_leaves_delay_compensation_while_off() {
        let mut s = session();
        let (a, b) = (audio(&mut s), audio(&mut s));
        s.set_plugin(a, 0, "builtin:linear-phase-eq").unwrap();
        let total = s.pdc_total();
        assert!(total > 0);
        assert!(s.pdc_report().strips.iter().any(|x| x.track == b && x.delay == total), "the other track waits for it");
        set_off(&mut s, a, true);
        assert_eq!(s.pdc_total(), 0);
        assert!(s.pdc_report().strips.is_empty(), "nobody waits, nothing is reported");
        set_off(&mut s, a, false);
        assert_eq!(s.pdc_total(), total);
    }

    #[test]
    fn a_stacks_off_reaches_its_subtracks() {
        for summing in [false, true] {
            let mut s = session();
            let (a, b) = (audio(&mut s), audio(&mut s));
            let stack = s.create_stack(&[a, b], summing).unwrap();
            let params = |s: &Session, id: Id| Session::track_params(&s.project, s.project.track(id).unwrap());
            set_off(&mut s, stack, true);
            assert!(s.project.is_off(a) && params(&s, a).off && params(&s, stack).off);
            assert!(!s.project.track(a).unwrap().off, "the subtrack's own switch is untouched");
            set_off(&mut s, a, true);
            set_off(&mut s, stack, false);
            assert!(s.project.is_off(a) && !s.project.is_off(b));
            assert!(params(&s, a).off && !params(&s, b).off);
        }
    }

    #[test]
    fn undo_and_redo_put_back_the_state_in_their_snapshot() {
        let mut s = session();
        let a = audio(&mut s);
        s.set_plugin(a, 0, "builtin:linear-phase-eq").unwrap();
        let total = s.pdc_total();
        // No undo step of its own, as for Mute.
        let steps = s.undo_stack.len();
        set_off(&mut s, a, true);
        assert_eq!(s.undo_stack.len(), steps);
        s.checkpoint("Rename", None, false);
        s.update_track(a, TrackPatch { name: Some("x".into()), ..Default::default() });
        set_off(&mut s, a, false);
        s.undo();
        assert!(s.project.track(a).unwrap().off);
        assert_eq!(s.pdc_total(), 0, "the engine follows");
        s.redo();
        assert!(!s.project.track(a).unwrap().off);
        assert_eq!(s.pdc_total(), total);
    }

    #[test]
    fn duplicates_and_saved_projects_keep_it() {
        let mut s = session();
        let a = audio(&mut s);
        set_off(&mut s, a, true);
        let copy = s.duplicate_tracks(&[a], true)[0];
        assert!(s.project.track(copy).unwrap().off);
        let json = serde_json::to_string(&s.project).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert!(back.track(a).unwrap().off);
    }

    #[test]
    fn off_tracks_dont_record_and_arent_switched_during_a_take() {
        let mut s = session();
        let dir = std::env::temp_dir().join(format!("fermata-off-{}", std::process::id()));
        s.dir = dir.join("Off.fermata");
        let (a, b) = (audio(&mut s), audio(&mut s));
        s.update_track(a, TrackPatch { armed: Some(true), ..Default::default() });
        set_off(&mut s, a, true);
        assert!(s.project.track(a).unwrap().armed, "it keeps its R button");
        assert!(s.record().unwrap_err().to_string().contains("off"));
        assert!(!s.is_recording());
        assert!(s.off_refused().is_none());

        s.update_track(b, TrackPatch { armed: Some(true), ..Default::default() });
        s.record().unwrap();
        assert!(s.is_recording());
        assert!(s.off_refused().is_some());
        set_off(&mut s, b, true);
        set_off(&mut s, a, false);
        assert!(!s.project.track(b).unwrap().off && s.project.track(a).unwrap().off, "unchanged during a take");
        s.stop();
        assert!(s.off_refused().is_none());
        set_off(&mut s, b, true);
        assert!(s.project.track(b).unwrap().off);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn single_track_renders_refuse_an_off_track_and_export_skips_it() {
        let mut s = session();
        let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        let r = s.create_midi_region(t, 0, 4800).unwrap();
        assert_eq!(s.export_targets(), vec![t]);
        set_off(&mut s, t, true);
        assert!(s.export_targets().is_empty());
        assert!(s.render_freeze(&[t], &mut |_| {}).is_err());
        assert!(s.render_in_place(&[t], &[], &BipOptions::default(), &mut |_| {}).is_err());
        assert!(s.export_region(r, Path::new("/nonexistent/x.wav"), &RenderOptions::default(), &mut |_| {}).unwrap_err().to_string().contains("off"));
    }
}
