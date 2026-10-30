//! Exercises the No Overlap, Shuffle L and Shuffle R drag modes and Move Automation with
//! Regions through the session API, as the Tracks area's commands call it, against a running
//! engine (silent ALSA null output): where regions end up, that moved automation is what
//! plays, and that each edit is one undo step.
//! usage: dragmode_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::fades::DragMode;
use daw_engine::model::{AutoParam, AutoPoint, Id, InputSel, TrackKind};
use daw_engine::prefs::MoveAutomation;
use daw_engine::session::RegionMove;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn session(dir: PathBuf) -> Session {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = dir;
    s
}

/// Peak level of Stereo Out while cycling over [a, b).
fn level(s: &mut Session, a: i64, b: i64) -> f32 {
    s.set_cycle(true, a, b);
    s.locate(a);
    s.play();
    std::thread::sleep(Duration::from_millis(150));
    s.tick();
    std::thread::sleep(Duration::from_millis(250));
    let t = s.tick();
    s.stop();
    // play() is ignored until the engine has really stopped.
    std::thread::sleep(Duration::from_millis(60));
    t.master.0.max(t.master.1)
}

/// The track's regions as (id, start, length), by start.
fn lane(s: &Session, track: Id) -> Vec<(Id, i64, i64)> {
    let mut v: Vec<(Id, i64, i64)> = s.project.track(track).unwrap().regions.iter().map(|r| (r.id, r.start, r.length)).collect();
    v.sort_by_key(|x| x.1);
    v
}

/// An audio region of the tone's first `len` frames, at `start`.
fn tone_region(s: &mut Session, tone: &Path, track: Id, start: i64, len: i64) -> Id {
    let id = s.import_audio(tone, track, 0).unwrap();
    s.resize_region(id, 0, len);
    s.move_region(id, start, None);
    id
}

fn modes(tone: &Path, scratch: &Path) {
    let mut s = session(scratch.join("DragModes.fermata"));
    let sec = s.sample_rate() as i64;
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let a = tone_region(&mut s, tone, t, 0, sec);
    let b = tone_region(&mut s, tone, t, sec, sec);
    let c = tone_region(&mut s, tone, t, 2 * sec, sec / 2);
    let start = lane(&s, t);

    // Shuffle L: deleting b pulls c left by b's length; one undo puts both back.
    s.set_drag_mode(DragMode::ShuffleL);
    s.checkpoint("Delete Region", None, false);
    s.drag_delete_regions(&[b]);
    check(&format!("Shuffle L: deleting closes the gap {:?}", lane(&s, t)), lane(&s, t) == vec![(a, 0, sec), (c, sec, sec / 2)]);
    s.undo();
    check("Shuffle L: one undo brings the region back and c to its place", lane(&s, t) == start);

    // Shuffle L: dragging a onto c's second half puts a after c, closing up behind it.
    s.checkpoint("Move Region", None, false);
    s.drag_move_regions(&[RegionMove { id: a, start: 2 * sec + sec / 3, track: None }], None);
    let want = vec![(b, 0, sec), (c, sec, sec / 2), (a, sec + sec / 2, sec)];
    check(&format!("Shuffle L: a drag reorders the lane {:?}", lane(&s, t)), lane(&s, t) == want);
    s.undo();
    check("Shuffle L: undo", lane(&s, t) == start);

    // Shuffle L: trimming b's end moves c along.
    s.checkpoint("Resize Region", None, false);
    s.drag_resize_region(b, sec, sec / 2);
    check(&format!("Shuffle L: a trim pulls the next region {:?}", lane(&s, t)), lane(&s, t) == vec![(a, 0, sec), (b, sec, sec / 2), (c, sec + sec / 2, sec / 2)]);
    s.undo();

    // Shuffle L: an Alt-drag copy of c onto a's first half goes in front of a.
    s.checkpoint("Copy Regions", None, false);
    let r = s.project.track(t).unwrap().regions.iter().find(|r| r.id == c).unwrap().clone();
    let made = s.drag_paste_regions(&[(t, r)], -2 * sec + sec / 10);
    let ok = made.len() == 1 && lane(&s, t) == vec![(made[0], 0, sec / 2), (a, sec / 2, sec), (b, sec + sec / 2, sec), (c, 2 * sec + sec / 2, sec / 2)];
    check(&format!("Shuffle L: a copy inserts and pushes the lane {:?}", lane(&s, t)), ok);
    s.undo();
    check("Shuffle L: undo removes the copy", lane(&s, t) == start);

    // Shuffle R: deleting b pulls a right; the lane still ends where it did.
    s.set_drag_mode(DragMode::ShuffleR);
    s.checkpoint("Delete Region", None, false);
    s.drag_delete_regions(&[b]);
    check(&format!("Shuffle R: deleting moves the regions before right {:?}", lane(&s, t)), lane(&s, t) == vec![(a, sec, sec), (c, 2 * sec, sec / 2)]);
    s.undo();

    // No Overlap: moving c over a's end shortens a; one undo restores both.
    s.set_drag_mode(DragMode::NoOverlap);
    s.checkpoint("Move Region", None, false);
    s.drag_move_regions(&[RegionMove { id: c, start: sec / 2, track: None }], None);
    let ok = lane(&s, t).first() == Some(&(a, 0, sec / 2)) && lane(&s, t).contains(&(c, sec / 2, sec / 2)) && lane(&s, t).contains(&(b, sec, sec));
    check(&format!("No Overlap: the region on the left is shortened {:?}", lane(&s, t)), ok);
    let heard = level(&mut s, 0, sec / 2);
    check(&format!("No Overlap: what's left of a still plays ({heard:.3})"), heard > 0.03);
    s.undo();
    check("No Overlap: undo", lane(&s, t) == start);

    // Moves onto another track: Shuffle L closes the source lane and opens the target one.
    s.set_drag_mode(DragMode::ShuffleL);
    let i = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
    let j = s.add_track(TrackKind::Instrument, InputSel::Mono(0), None, None).unwrap();
    let m1 = s.create_midi_region(i, 0, sec).unwrap();
    let m2 = s.create_midi_region(i, sec, sec).unwrap();
    let n1 = s.create_midi_region(j, 0, sec).unwrap();
    s.checkpoint("Move Region", None, false);
    s.drag_move_regions(&[RegionMove { id: m1, start: sec / 10, track: Some(j) }], None);
    let ok = lane(&s, i) == vec![(m2, 0, sec)] && lane(&s, j) == vec![(m1, 0, sec), (n1, sec, sec)];
    check(&format!("Shuffle L across tracks: {:?} / {:?}", lane(&s, i), lane(&s, j)), ok);
    s.undo();
    check("across tracks: undo", lane(&s, i) == vec![(m1, 0, sec), (m2, sec, sec)] && lane(&s, j) == vec![(n1, 0, sec)]);

    // Shuffle R: dragging m1's end onto m2's first half puts m1 after m2, and the lane still
    // ends where it did.
    s.set_drag_mode(DragMode::ShuffleR);
    s.checkpoint("Move Region", None, false);
    s.drag_move_regions(&[RegionMove { id: m1, start: sec * 8 / 10, track: None }], None);
    check(&format!("Shuffle R: a drag swaps neighbours {:?}", lane(&s, i)), lane(&s, i) == vec![(m2, 0, sec), (m1, sec, sec)]);
    s.undo();
    let _ = std::fs::remove_dir_all(&s.dir);
}

fn automation(tone: &Path, scratch: &Path) {
    let mut s = session(scratch.join("MoveAutomation.fermata"));
    let sec = s.sample_rate() as i64;
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    // A region at 0.5..1 s, and volume automation that is only open under it.
    let r = tone_region(&mut s, tone, t, sec / 2, sec / 2);
    let (lo, hi) = (sec / 2, sec);
    let points = vec![AutoPoint { pos: 0, value: -96.0, curve: 0.0 }, AutoPoint { pos: lo - 1, value: -96.0, curve: 0.0 }, AutoPoint { pos: lo, value: 0.0, curve: 0.0 }, AutoPoint { pos: hi - 1, value: 0.0, curve: 0.0 }, AutoPoint { pos: hi, value: -96.0, curve: 0.0 }];
    s.set_automation_lane(t, AutoParam::Volume, points);
    let open = level(&mut s, lo, hi);
    check(&format!("the region plays under its automation ({open:.3})"), open > 0.03);

    // Never: the region moves to 2 s, where the automation is closed.
    s.checkpoint("Move Region", None, false);
    s.drag_move_regions(&[RegionMove { id: r, start: 2 * sec, track: None }], None);
    let shut = level(&mut s, 2 * sec, 2 * sec + sec / 2);
    check(&format!("Never: the automation stays behind ({shut:.5})"), shut < 0.003);
    s.undo();

    // Always: the open stretch goes along.
    s.prefs.editing.move_automation = MoveAutomation::Always;
    s.checkpoint("Move Region", None, false);
    s.drag_move_regions(&[RegionMove { id: r, start: 2 * sec, track: None }], None);
    let moved = level(&mut s, 2 * sec, 2 * sec + sec / 2);
    check(&format!("Always: the automation moves with the region ({moved:.3})"), moved > 0.03);
    let lane = &s.project.track(t).unwrap().automation[0].points;
    let v = |x: i64| daw_engine::automation::value_at(lane, x).unwrap();
    check(&format!("the old place is closed again ({} dB)", v(sec * 3 / 4)), v(sec * 3 / 4) < -90.0);
    s.undo();
    let back = level(&mut s, lo, hi);
    let curve = &s.project.track(t).unwrap().automation[0].points;
    check(&format!("one undo puts region and automation back ({back:.3})"), back > 0.03 && curve.len() == 5);

    // An explicit answer (the Ask dialog's Don't Move) overrides Always.
    s.checkpoint("Nudge", None, false);
    s.drag_nudge_regions(&[r], 2 * sec, Some(false));
    let shut = level(&mut s, 2 * sec + sec / 2, 3 * sec);
    check(&format!("Don't Move leaves the automation ({shut:.5})"), shut < 0.003);
    s.undo();
    let _ = std::fs::remove_dir_all(&s.dir);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = PathBuf::from(args.next().expect("audio file"));
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    modes(&tone, &scratch);
    automation(&tone, &scratch);
    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
