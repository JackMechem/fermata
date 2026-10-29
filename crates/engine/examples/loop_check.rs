//! Exercises looping regions and hidden tracks against a running engine (silent ALSA null
//! output). usage: loop_check <audio file> <scratch dir>
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, Note, RegionContent, TrackKind};
use daw_engine::Session;

/// Plays the cycle `from..to` for a moment and returns the peak meter of each track id given,
/// plus Stereo Out.
fn levels(s: &mut Session, from: i64, to: i64, ids: &[u64]) -> (Vec<f32>, f32) {
    s.set_cycle(true, from, to);
    s.locate(from);
    s.play();
    std::thread::sleep(Duration::from_millis(150));
    s.tick();
    std::thread::sleep(Duration::from_millis(250));
    let t = s.tick();
    s.stop();
    // play() is ignored until the engine has really stopped.
    std::thread::sleep(Duration::from_millis(60));
    let lv = ids.iter().map(|id| t.meters.iter().find(|m| m.0 == *id).map(|m| m.1.max(m.2)).unwrap_or(-1.0)).collect();
    (lv, t.master.0.max(t.master.1))
}

fn check(name: &str, ok: bool) -> bool {
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
    ok
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = args.next().expect("scratch dir");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::path::PathBuf::from(&scratch).join("LoopCheck.fermata");
    std::fs::create_dir_all(s.dir.join("Audio Files")).unwrap();
    let sr = s.sample_rate() as i64;
    let mut ok = true;

    // An audio region trimmed to half a second; the cycle plays the second after it.
    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    let id = s.project.track(a).unwrap().regions[0].id;
    let full = s.project.track(a).unwrap().regions[0].length;
    let len = sr / 2;
    assert!(full > 5 * len, "the test file should be longer than 2.5 s");
    s.resize_region(id, 0, len);
    let (lv, _) = levels(&mut s, len + sr / 10, len + sr, &[a]);
    ok &= check(&format!("unlooped region is silent after its end ({:.4})", lv[0]), lv[0] < 1e-4);

    s.checkpoint("Loop Region", None, false);
    s.set_region_loop(id, Some(4 * len + len / 2));
    let (lv, _) = levels(&mut s, len + sr / 10, len + sr, &[a]);
    ok &= check(&format!("looped region plays in its repetitions ({:.3})", lv[0]), lv[0] > 0.01);
    let (lv, _) = levels(&mut s, 5 * len, 6 * len, &[a]);
    ok &= check(&format!("the loop stops at its loop end ({:.4})", lv[0]), lv[0] < 1e-4);

    s.undo();
    let gone = s.project.track(a).unwrap().regions[0].loop_end.is_none();
    let (lv, _) = levels(&mut s, len + sr / 10, len + sr, &[a]);
    ok &= check(&format!("undo removes the loop ({:.4})", lv[0]), gone && lv[0] < 1e-4);
    s.redo();
    let (lv, _) = levels(&mut s, len + sr / 10, len + sr, &[a]);
    ok &= check(&format!("redo brings it back ({:.3})", lv[0]), s.project.track(a).unwrap().regions[0].loop_end == Some(4 * len + len / 2) && lv[0] > 0.01);

    // Moving takes the loop along; trimming keeps its end, or drops it when overtaken.
    s.checkpoint("Move Region", None, false);
    s.move_region(id, sr, None);
    let r = s.project.track(a).unwrap().regions[0].clone();
    ok &= check(&format!("move shifts the loop end ({:?})", r.loop_end), r.loop_end == Some(sr + 4 * len + len / 2));
    s.undo();
    s.resize_region(id, 0, len / 2);
    let r = s.project.track(a).unwrap().regions[0].clone();
    ok &= check(&format!("trim keeps the loop end ({:?}, {} passes)", r.loop_end, r.passes().len()), r.loop_end == Some(4 * len + len / 2) && r.passes().len() == 9);
    s.resize_region(id, 0, len);
    s.checkpoint("Resize Region", None, false);
    s.resize_region(id, 0, full);
    ok &= check("trimming past the loop end drops the loop", s.project.track(a).unwrap().regions[0].loop_end.is_none());
    s.undo();

    // Repeat uses the looped extent.
    s.checkpoint("Repeat Regions", None, false);
    let made = s.repeat_regions(&[id], 1);
    let copy = s.project.track(a).unwrap().regions.iter().find(|r| r.id == made[0]).unwrap().clone();
    ok &= check(&format!("repeat starts after the loop ({} at {}, loop end {:?})", copy.id, copy.start, copy.loop_end), copy.start == 4 * len + len / 2 && copy.loop_end == Some(2 * (4 * len + len / 2)));
    s.undo();

    // Convert Loops to Real Copies.
    s.checkpoint("Convert Loops to Real Copies", None, false);
    let copies = s.convert_loops(&[id]);
    let mut starts: Vec<(i64, i64)> = s.project.track(a).unwrap().regions.iter().map(|r| (r.start, r.length)).collect();
    starts.sort();
    let want: Vec<(i64, i64)> = vec![(0, len), (len, len), (2 * len, len), (3 * len, len), (4 * len, len / 2)];
    ok &= check(&format!("convert makes one region per pass {starts:?}"), copies.len() == 4 && starts == want);
    ok &= check("no loops left", s.project.track(a).unwrap().regions.iter().all(|r| r.loop_end.is_none()));
    let (lv, _) = levels(&mut s, len + sr / 10, len + sr, &[a]);
    ok &= check(&format!("the copies play ({:.3})", lv[0]), lv[0] > 0.01);
    s.undo();
    ok &= check("undo restores the loop", s.project.track(a).unwrap().regions.len() == 1 && s.project.track(a).unwrap().regions[0].is_looped());

    // Split inside a repetition converts the loop first.
    s.checkpoint("Split Regions", None, false);
    s.split_at(2 * len + len / 2, &[id]);
    let mut starts: Vec<(i64, i64)> = s.project.track(a).unwrap().regions.iter().map(|r| (r.start, r.length)).collect();
    starts.sort();
    let want = vec![(0, len), (len, len), (2 * len, len / 2), (2 * len + len / 2, len / 2), (3 * len, len), (4 * len, len / 2)];
    ok &= check(&format!("split converts the loop, then splits {starts:?}"), starts == want);
    s.undo();

    // Join treats the loop as its copies.
    s.checkpoint("Join Regions", None, false);
    let other = s.paste_regions(&[(a, s.project.track(a).unwrap().regions[0].clone())], 6 * len);
    let mut ids = other.clone();
    ids.push(id);
    let joined = s.join_regions(&ids).unwrap();
    let t = s.project.track(a).unwrap();
    ok &= check(
        &format!("join covers the looped extent ({} regions, length {})", t.regions.len(), t.regions[0].length),
        joined.len() == 1 && t.regions.len() == 1 && t.regions[0].start == 0 && t.regions[0].length == 6 * len + 4 * len + len / 2,
    );
    s.undo();

    // Loop repetitions sit beneath region bodies: a quiet region under the loop still wins.
    s.checkpoint("Paste", None, false);
    let quiet = s.paste_regions(&[(a, { let mut r = s.project.track(a).unwrap().regions[0].clone(); r.loop_end = None; r })], 2 * len);
    s.set_region_gain(&quiet, Some(-40.0), None);
    s.move_region(id, 0, None); // the loop's region is now the newest
    let (lv, _) = levels(&mut s, 2 * len + sr / 20, 3 * len - sr / 20, &[a]);
    ok &= check(&format!("a region body covers the loop under it ({:.4})", lv[0]), lv[0] > 1e-5 && lv[0] < 0.05);
    s.undo();

    // Loop switch: to the next region on the track, else four times the length.
    s.set_region_loop(id, None);
    let later = s.paste_regions(&[(a, s.project.track(a).unwrap().regions[0].clone())], 3 * len);
    s.set_regions_looped(&[id], true);
    ok &= check("Loop runs up to the next region", s.project.track(a).unwrap().regions.iter().find(|r| r.id == id).unwrap().loop_end == Some(3 * len));
    s.delete_regions(&later);
    s.set_regions_looped(&[id], true);
    ok &= check("Loop with nothing after it runs four times the length", s.project.track(a).unwrap().regions[0].loop_end == Some(4 * len));
    s.set_regions_looped(&[id], false);
    ok &= check("Loop off removes it", s.project.track(a).unwrap().regions[0].loop_end.is_none());

    // MIDI: one beat with a short note, looped; the cycle plays a later beat.
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let beat = s.project.samples_per_beat().round() as i64;
    let mid = s.create_midi_region(inst, 0, beat).unwrap();
    if let Some(r) = s.project.track_mut(inst).unwrap().regions.iter_mut().find(|r| r.id == mid) {
        r.content = RegionContent::Midi { cc: Vec::new(), notes: vec![Note { start: 0, length: beat / 2, pitch: 60, velocity: 110, muted: false }] };
    }
    s.set_region_loop(mid, None); // pushes the edited notes
    s.update_track(a, daw_engine::session::TrackPatch { mute: Some(true), ..Default::default() });
    let (lv, _) = levels(&mut s, 2 * beat, 3 * beat, &[inst]);
    ok &= check(&format!("unlooped MIDI region is silent later ({:.4})", lv[0]), lv[0] < 1e-3);
    s.set_region_loop(mid, Some(4 * beat));
    let (lv, _) = levels(&mut s, 2 * beat, 3 * beat, &[inst]);
    ok &= check(&format!("looped MIDI region plays its notes again ({:.3})", lv[0]), lv[0] > 0.01);
    s.update_track(a, daw_engine::session::TrackPatch { mute: Some(false), ..Default::default() });

    // Hidden tracks still play; hiding a stack hides its subtracks; undo shows them again.
    let stack = s.create_stack(&[a, inst], false).unwrap();
    s.checkpoint("Hide Tracks", None, false);
    s.set_tracks_hidden(&[stack], true);
    let all_hidden = [stack, a, inst].iter().all(|id| s.project.track(*id).unwrap().hidden);
    ok &= check("hiding a stack hides its subtracks", all_hidden);
    let (lv, _) = levels(&mut s, 0, len, &[a]);
    ok &= check(&format!("a hidden track still plays ({:.3})", lv[0]), lv[0] > 0.01);
    s.undo();
    ok &= check("undo shows them again", s.project.tracks.iter().all(|t| !t.hidden));

    println!("{}", if ok { "ALL PASS" } else { "SOME FAILED" });
    std::process::exit(if ok { 0 } else { 1 });
}
