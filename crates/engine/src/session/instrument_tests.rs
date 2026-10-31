//! Built-in instruments against the parts of the session the headless check can't see inside.

use super::*;
use crate::audio_io::NULL_OUTPUT;
use crate::dsp::TaskGraph;
use crate::instrument::tre::params::param;

const TRE: &str = "builtin:tre";

fn session() -> Session {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some(NULL_OUTPUT.into()), input_enabled: false, ..Default::default() };
    Session::new(cfg, Vec::new())
}

#[test]
fn tracks_with_built_in_instruments_count_as_work_for_a_second_thread() {
    let mut s = session();
    let width = |s: &Session| TaskGraph::of_project(&s.project).unwrap().heavy_width();
    let a = s.add_track(TrackKind::Instrument, InputSel::None, Some(TRE.into()), None).unwrap();
    assert_eq!(width(&s), 1);
    s.add_track(TrackKind::Instrument, InputSel::None, Some(TRE.into()), None).unwrap();
    assert_eq!(width(&s), 2, "two instruments overlap");
    s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    assert_eq!(width(&s), 3, "Fermata Synth too, as before");
    s.update_track(a, TrackPatch { off: Some(true), ..Default::default() });
    assert_eq!(width(&s), 2, "one that's Off doesn't");
}

#[test]
fn an_instrument_slot_answers_the_calls_an_effects_slot_does() {
    let mut s = session();
    let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(TRE.into()), None).unwrap();
    let uid = s.project.track(t).unwrap().instrument.as_ref().unwrap().uid;
    assert!(s.builtin_params(uid).len() > 40);
    assert!(s.plugin_params(uid).iter().any(|p| p.name == "Emphasis"));
    assert_eq!(s.builtin_param_name(uid, param::CUTOFF).as_deref(), Some("Cutoff"));
    // Normalized, as automation holds it.
    let n = s.builtin_normalized(uid, param::CUTOFF).unwrap();
    s.set_builtin_normalized(uid, param::CUTOFF, 1.0);
    assert_eq!(s.builtin_params(uid).iter().find(|p| p.0 == param::CUTOFF).unwrap().1, 20000.0);
    assert!(n > 0.0 && n < 1.0);
    // Its panel's watch and tail come through the same shared values.
    s.watch_builtin(uid, true);
    assert!(s.fx_ticks().iter().any(|t| t.uid == uid));
    s.watch_builtin(uid, false);
    s.set_builtin_param(t, usize::MAX, param::A_DECAY, 1000.0).unwrap();
    s.idle();
    // A running engine applies the command at its next block.
    let engine = s.engine_for_checks();
    let mut buf = vec![0.0f32; 128];
    engine.lock().process(&[], 0, &mut buf, 2);
    assert!(s.builtin_tail_secs(t) > 3.0, "{}", s.builtin_tail_secs(t));
}

#[test]
fn fermata_synth_stays_without_settings_or_a_panel_entry_in_the_project() {
    let mut s = session();
    let t = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let slot = s.project.track(t).unwrap().instrument.clone().unwrap();
    assert_eq!(slot.plugin_id, BUILTIN_SYNTH);
    assert!(slot.params.is_empty());
    assert_eq!(s.project.track(t).unwrap().name, "Inst 1");
    assert_eq!(s.builtin_tail_secs(t), 0.0, "a freeze's tail stays what it was");
}

#[test]
fn undo_reaches_a_running_instruments_bypass() {
    let mut s = session();
    let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(TRE.into()), None).unwrap();
    s.checkpoint("Bypass", None, false);
    s.set_bypass(t, usize::MAX, true);
    s.undo();
    assert!(!s.project.track(t).unwrap().instrument.as_ref().unwrap().bypass);
}
