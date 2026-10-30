//! What edits during playback do to the notes an instrument is holding, counted at the
//! instrument: the note-ons and note-offs the engine hands it, block by block.

use super::*;
use crate::audio_io::NULL_OUTPUT;
use crate::dsp::Engine;

const LONG: i64 = 4_000_000;
const PITCH: u8 = 60;

fn session() -> Session {
    let cfg = crate::audio_io::AudioConfig { host: Some("ALSA".into()), output: Some(NULL_OUTPUT.into()), input_enabled: false, ..Default::default() };
    let s = Session::new(cfg, Vec::new());
    assert_eq!(s.audio_config.output.as_deref(), Some(NULL_OUTPUT));
    s
}

/// An instrument track holding one long note, and a second one, empty.
fn held(s: &mut Session) -> (Id, Id) {
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let other = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let r = s.create_midi_region(inst, 0, LONG).unwrap();
    let notes = vec![Note { start: 0, length: LONG, pitch: PITCH, velocity: 100, muted: false }];
    s.set_midi_content(vec![MidiContent { region: r, notes, cc: vec![], start: None, length: None }]);
    s.select_track(Some(inst));
    (inst, other)
}

/// Runs a few blocks and returns each note message the track's instrument got, as (on, pitch).
/// Holding the engine's lock keeps the device out, so these are the only blocks.
fn run(e: &mut Engine, track: Id) -> Vec<(bool, u8)> {
    let mut buf = vec![0.0f32; 512];
    let mut got = Vec::new();
    for _ in 0..4 {
        e.process(&[], 0, &mut buf, 2);
        got.extend(e.midi_sent(track).iter().filter(|m| m.data[0] & 0xE0 == 0x80).map(|m| (m.data[0] & 0xF0 == 0x90 && m.data[2] > 0, m.data[1])));
    }
    got
}

/// Starts playback and checks the note sounds once.
fn start(s: &mut Session, e: &mut Engine, inst: Id) {
    s.locate(0);
    s.play();
    assert_eq!(run(e, inst), vec![(true, PITCH)]);
    assert!(run(e, inst).is_empty());
}

#[test]
fn adding_a_track_leaves_held_notes_alone() {
    let mut s = session();
    let (inst, _) = held(&mut s);
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    start(&mut s, &mut e, inst);
    s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    assert_eq!(run(&mut e, inst), vec![]);
}

#[test]
fn edits_that_are_not_about_the_instrument_leave_held_notes_alone() {
    let mut s = session();
    let (inst, other) = held(&mut s);
    let aux = s.create_aux(None).unwrap();
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    start(&mut s, &mut e, inst);
    let edits: Vec<(&str, Box<dyn FnMut(&mut Session)>)> = vec![
        ("add an audio track", Box::new(|s: &mut Session| { s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap(); })),
        ("add an instrument track", Box::new(|s| { s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap(); })),
        ("add an aux", Box::new(|s| { s.create_aux(None).unwrap(); })),
        ("delete a track", Box::new(|s| { let last = s.project.tracks.last().unwrap().id; s.delete_track(last); })),
        ("reorder tracks", Box::new(|s| { let ids = s.project.tracks.iter().rev().map(|t| t.id).collect(); s.reorder_tracks(ids); })),
        ("add a send", Box::new(move |s| s.set_send(inst, 0, Some(TrackSend { bus: aux, level_db: -6.0, pre_fader: false, muted: false })).unwrap())),
        ("change a send's level", Box::new(move |s| s.set_send_level(inst, 0, -12.0))),
        ("remove a send", Box::new(move |s| s.set_send(inst, 0, None).unwrap())),
        ("move a fader", Box::new(move |s| s.update_track(inst, TrackPatch { volume_db: Some(-3.0), ..Default::default() }))),
        ("mute another track", Box::new(move |s| s.update_track(other, TrackPatch { mute: Some(true), ..Default::default() }))),
        ("switch the metronome", Box::new(|s| s.set_metronome(true))),
        ("move the cycle", Box::new(|s| s.set_cycle(false, 480_000, 960_000))),
        ("undo a track add", Box::new(|s| { s.checkpoint("Add", None, false); s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap(); s.undo(); })),
        // Live MIDI moves elsewhere, but no key is down: what the regions play has nothing to do with it.
        ("select another instrument track", Box::new(move |s| s.select_track(Some(other)))),
        ("select an aux", Box::new(move |s| s.select_track(Some(aux)))),
        ("select nothing", Box::new(|s| s.select_track(None))),
    ];
    for (what, mut edit) in edits {
        edit(&mut s);
        assert_eq!(run(&mut e, inst), vec![], "{what}");
    }
}

#[test]
fn a_region_that_follows_the_tempo_only_restarts_when_the_tempo_moves() {
    let mut s = session();
    let (inst, _) = held(&mut s);
    s.project.track_mut(inst).unwrap().regions[0].params.quantize = crate::region_params::QUANTIZE_VALUES[1];
    s.push_regions(inst);
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    s.locate(0);
    s.play();
    run(&mut e, inst);
    s.set_metronome(true);
    s.checkpoint("Add", None, false);
    s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.undo();
    assert_eq!(run(&mut e, inst), vec![], "nothing about its regions changed");
    s.set_tempo(90.0);
    assert!(run(&mut e, inst).contains(&(false, PITCH)), "its playback list is new");
}

#[test]
fn a_new_instrument_gets_the_held_notes_afresh() {
    let mut s = session();
    let (inst, _) = held(&mut s);
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    start(&mut s, &mut e, inst);
    s.checkpoint("Instrument", None, false);
    s.set_plugin(inst, usize::MAX, BUILTIN_SYNTH).unwrap();
    assert_eq!(run(&mut e, inst), vec![(false, PITCH), (true, PITCH)]);
    assert_eq!(run(&mut e, inst), vec![]);
    s.undo();
    assert_eq!(run(&mut e, inst), vec![(false, PITCH), (true, PITCH)], "undo");
}

#[test]
fn with_chase_off_a_new_instrument_only_releases() {
    let mut s = session();
    let (inst, _) = held(&mut s);
    let mut prefs = s.prefs.clone();
    prefs.midi.chase_notes = false;
    s.set_prefs(prefs);
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    start(&mut s, &mut e, inst);
    s.set_plugin(inst, usize::MAX, BUILTIN_SYNTH).unwrap();
    assert_eq!(run(&mut e, inst), vec![(false, PITCH)]);
}

#[test]
fn an_edited_region_releases_and_chases() {
    let mut s = session();
    let (inst, _) = held(&mut s);
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    start(&mut s, &mut e, inst);
    let r = s.project.track(inst).unwrap().regions[0].id;
    let notes = vec![Note { start: 0, length: LONG, pitch: PITCH + 1, velocity: 100, muted: false }];
    s.set_midi_content(vec![MidiContent { region: r, notes, cc: vec![], start: None, length: None }]);
    assert_eq!(run(&mut e, inst), vec![(false, PITCH), (true, PITCH + 1)]);
}

#[test]
fn moving_the_target_releases_a_key_held_on_it() {
    let mut s = session();
    let (inst, other) = held(&mut s);
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    start(&mut s, &mut e, inst);
    s.send_midi([0x90, 72, 100]);
    assert_eq!(run(&mut e, inst), vec![(true, 72)]);
    s.select_track(Some(other));
    // Its note-off would go to the new target. The flush takes the region's note with it, and
    // the chase brings that one back.
    let mut got = run(&mut e, inst);
    got[..2].sort();
    assert_eq!(got, vec![(false, PITCH), (false, 72), (true, PITCH)]);
    // The key comes up on the new target, which never had it: nothing is left held.
    s.send_midi([0x80, 72, 0]);
    assert_eq!(run(&mut e, other), vec![(false, 72)]);
    assert_eq!(run(&mut e, inst), vec![]);
    s.select_track(Some(inst));
    assert_eq!(run(&mut e, inst), vec![]);
}

#[test]
fn turning_midi_thru_off_releases_a_held_key() {
    let mut s = session();
    let (inst, _) = held(&mut s);
    let engine = s.engine_for_checks();
    let mut e = engine.lock();
    start(&mut s, &mut e, inst);
    s.send_midi([0x90, 72, 100]);
    assert_eq!(run(&mut e, inst), vec![(true, 72)]);
    let mut prefs = s.prefs.clone();
    prefs.midi.thru = false;
    s.set_prefs(prefs);
    let got = run(&mut e, inst);
    assert!(got.contains(&(false, 72)), "{got:?}");
    assert_eq!(got.iter().filter(|g| g.0).count(), 1, "{got:?}");
}
