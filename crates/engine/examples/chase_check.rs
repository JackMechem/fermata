//! Chase: Notes and Controllers against a running engine (silent ALSA null output) through the
//! built-in synth: playback started in the middle of a long note sounds at once, a jump into one
//! does too, and neither does with Chase: Notes off; a controller set before the playhead is in
//! effect from the start, unless Chase: Controllers is off. usage: chase_check <scratch dir>
use std::time::Duration;

use daw_engine::model::{CcEvent, CcKind, InputSel, Note, TrackKind};
use daw_engine::session::MidiContent;
use daw_engine::{audio_io::AudioConfig, Session};

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// The track's peak level over a moment of playback, read off the meters.
fn peak(s: &mut Session, id: u64) -> f32 {
    s.tick();
    std::thread::sleep(Duration::from_millis(250));
    let t = s.tick();
    t.meters.iter().find(|m| m.0 == id).map(|m| m.1.max(m.2)).unwrap_or(-1.0)
}

/// Plays from `from` (with the cycle kept inside the note, so no note starts on its own) and
/// returns the level.
fn play_from(s: &mut Session, id: u64, from: i64) -> f32 {
    s.locate(from);
    s.play();
    std::thread::sleep(Duration::from_millis(60));
    let lv = peak(s, id);
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    lv
}

/// Plays from `away` (past every note: silent), then jumps to `to` while playing. Returns the
/// level before and after the jump. The cycle is off meanwhile: the engine would wrap a playhead
/// past its end straight back to its start.
fn jump(s: &mut Session, id: u64, away: i64, to: i64) -> (f32, f32) {
    let c = s.project.cycle.clone();
    s.set_cycle(false, c.start, c.end);
    s.locate(away);
    s.play();
    std::thread::sleep(Duration::from_millis(60));
    let before = peak(s, id);
    // The meters were just read: what they hold next is from the jump on (and silence before).
    s.locate(to);
    std::thread::sleep(Duration::from_millis(250));
    let t = s.tick();
    let after = t.meters.iter().find(|m| m.0 == id).map(|m| m.1.max(m.2)).unwrap_or(-1.0);
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    s.set_cycle(c.enabled, c.start, c.end);
    (before, after)
}

fn set_chase(s: &mut Session, notes: bool, controllers: bool) {
    let mut p = s.prefs.clone();
    p.midi.chase_notes = notes;
    p.midi.chase_controllers = controllers;
    s.set_prefs(p);
}

fn main() {
    let scratch = std::path::PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    std::fs::create_dir_all(&scratch).unwrap();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("ChaseCheck.fermata");
    let bar = s.project.samples_per_bar().round() as i64;
    let beat = s.project.samples_per_beat().round() as i64;

    // One note held through four bars; the cycle loops bars 2 and 3, inside it.
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.select_track(Some(inst));
    let r = s.create_midi_region(inst, 0, 4 * bar).unwrap();
    let pad = Note { start: 0, length: 4 * bar, pitch: 48, velocity: 100, muted: false };
    s.set_midi_content(vec![MidiContent { region: r, notes: vec![pad], cc: vec![], start: None, length: None }]);
    s.set_cycle(true, bar, 3 * bar);
    let mid = bar + beat;

    check("chase is on by default", s.prefs.midi.chase_notes && s.prefs.midi.chase_controllers);
    let lv = play_from(&mut s, inst, mid);
    check(&format!("starting inside a long note sounds ({lv:.3})"), lv > 0.01);
    let (before, after) = jump(&mut s, inst, 6 * bar, mid);
    check(&format!("past the note it is silent ({before:.4})"), before < 1e-4);
    check(&format!("jumping into the note sounds ({after:.3})"), after > 0.01);

    set_chase(&mut s, false, true);
    let lv = play_from(&mut s, inst, mid);
    check(&format!("with Chase: Notes off it is silent ({lv:.4})"), lv < 1e-4);
    let (_, after) = jump(&mut s, inst, 6 * bar, mid);
    check(&format!("and a jump into the note is silent too ({after:.4})"), after < 1e-4);
    set_chase(&mut s, true, true);

    // Expression 0 set at the region start silences notes that start inside the cycle, but
    // only when it is chased.
    let beats: Vec<Note> = (4..12).map(|i| Note { start: i * beat, length: beat / 2, pitch: 60, velocity: 100, muted: false }).collect();
    let quiet = vec![CcEvent { start: 0, kind: CcKind::Controller(11), value: 0 }];
    s.set_midi_content(vec![MidiContent { region: r, notes: beats, cc: quiet, start: None, length: None }]);
    // Off first: the synth keeps a controller value it was sent until something changes it.
    set_chase(&mut s, true, false);
    let lv = play_from(&mut s, inst, bar);
    check(&format!("with Chase: Controllers off the notes play ({lv:.3})"), lv > 0.01);
    set_chase(&mut s, true, true);
    let lv = play_from(&mut s, inst, bar);
    check(&format!("a chased expression of 0 silences ({lv:.4})"), lv < 1e-4);

    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
