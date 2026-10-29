//! Exercises track stacks, groups and region edits against a running engine (silent ALSA
//! null output). usage: stack_check <audio file> <scratch dir>
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, RegionContent, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;

/// Plays for a moment and returns the peak meter of each track id given, plus Stereo Out.
fn levels(s: &mut Session, ids: &[u64]) -> (Vec<f32>, f32) {
    s.locate(0);
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

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = args.next().expect("scratch dir");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::path::PathBuf::from(&scratch).join("StackCheck.fermata");

    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    s.import_audio(std::path::Path::new(&tone), b, 0).unwrap();
    // The null device has no clock and runs flat out, so loop over the audio.
    s.set_cycle(true, 0, 48000);
    println!("audio: {:?}", s.audio_status);
    let (lv, master) = levels(&mut s, &[a, b]);
    check(&format!("both tracks play (a={:.3} b={:.3} out={master:.3})", lv[0], lv[1]), lv[0] > 0.01 && lv[1] > 0.01 && master > 0.01);

    // Summing stack: subtracks play through it.
    let sum = s.create_stack(&[a, b], true).unwrap();
    let order: Vec<_> = s.project.tracks.iter().map(|t| (t.name.clone(), t.parent)).collect();
    println!("order after stacking: {order:?}");
    let (lv, master) = levels(&mut s, &[a, b, sum]);
    check(&format!("summing stack carries its subtracks (stack={:.3} out={master:.3})", lv[2]), lv[2] > 0.01 && master > 0.01);
    s.update_track_linked(sum, TrackPatch { mute: Some(true), ..Default::default() });
    let (_, master) = levels(&mut s, &[a]);
    check(&format!("muting the stack silences it (out={master:.4})"), master < 1e-4);
    s.update_track_linked(sum, TrackPatch { mute: Some(false), ..Default::default() });
    s.update_track_linked(a, TrackPatch { solo: Some(true), ..Default::default() });
    let (lv, master) = levels(&mut s, &[a, b, sum]);
    check(&format!("soloing a subtrack keeps its stack audible (a={:.3} b={:.3} out={master:.3})", lv[0], lv[1]), lv[0] > 0.01 && lv[1] < 1e-4 && master > 0.01);
    s.update_track_linked(a, TrackPatch { solo: Some(false), ..Default::default() });

    // Folder stack: its fader scales the subtracks.
    s.flatten_stack(sum);
    check("flatten keeps the subtracks", s.project.track(a).map(|t| t.parent.is_none()).unwrap_or(false) && s.project.track(sum).is_none());
    let folder = s.create_stack(&[a, b], false).unwrap();
    let (before, _) = levels(&mut s, &[a]);
    s.update_track_linked(folder, TrackPatch { volume_db: Some(-12.0), ..Default::default() });
    let (after, _) = levels(&mut s, &[a]);
    let drop_db = 20.0 * (after[0] / before[0]).log10();
    check(&format!("folder fader moves subtracks ({drop_db:.1} dB)"), (drop_db + 12.0).abs() < 1.0);
    s.update_track_linked(folder, TrackPatch { volume_db: Some(0.0), ..Default::default() });

    // Groups link volume relatively.
    s.update_track_linked(b, TrackPatch { volume_db: Some(-6.0), ..Default::default() });
    let g = s.create_group(&[a, b]);
    s.update_track_linked(a, TrackPatch { volume_db: Some(-3.0), ..Default::default() });
    let vb = s.project.track(b).unwrap().volume_db;
    check(&format!("group moves the other member by the same amount (b={vb:.1})"), (vb + 9.0).abs() < 0.01);
    s.update_track_linked(a, TrackPatch { mute: Some(true), ..Default::default() });
    check("group links mute", s.project.track(b).unwrap().mute);
    s.update_track_linked(a, TrackPatch { mute: Some(false), ..Default::default() });
    let mut gs = s.project.group(g).unwrap().clone();
    gs.enabled = false;
    s.update_group(gs);
    s.update_track_linked(a, TrackPatch { mute: Some(true), ..Default::default() });
    check("disabled group doesn't link", !s.project.track(b).unwrap().mute);
    s.update_track_linked(a, TrackPatch { mute: Some(false), ..Default::default() });

    // Duplicate a track with content.
    let dup = s.duplicate_tracks(&[a], true);
    let d = s.project.track(dup[0]).unwrap();
    check(&format!("duplicate copies regions and stays in the stack ({}, parent {:?})", d.name, d.parent), d.regions.len() == 1 && d.parent == Some(folder));
    let (lv, _) = levels(&mut s, &[dup[0]]);
    check(&format!("duplicate plays ({:.3})", lv[0]), lv[0] > 0.01);

    // Region edits.
    let r = s.project.track(a).unwrap().regions[0].clone();
    let rep = s.repeat_regions(&[r.id], 2);
    let starts: Vec<i64> = s.project.track(a).unwrap().regions.iter().map(|x| x.start).collect();
    check(&format!("repeat x2 places copies end to end {starts:?}"), rep.len() == 2 && starts == vec![0, r.length, 2 * r.length]);
    let all: Vec<_> = s.project.track(a).unwrap().regions.iter().map(|x| x.id).collect();
    let joined = s.join_regions(&all).unwrap();
    let t = s.project.track(a).unwrap();
    check(&format!("join makes one region of 3x length ({} regions)", t.regions.len()), t.regions.len() == 1 && t.regions[0].length == 3 * r.length && joined.len() == 1);
    s.set_region_gain(&joined, Some(-20.0), None);
    s.normalize_regions(&joined);
    let gain = match &s.project.track(a).unwrap().regions[0].content {
        RegionContent::Audio { gain_db, .. } => *gain_db,
        _ => 0.0,
    };
    println!("normalized gain: {gain:.2} dB");
    s.set_region_fades(&joined, Some(4800), Some(r.length * 10));
    if let RegionContent::Audio { fade_in, fade_out, .. } = &s.project.track(a).unwrap().regions[0].content {
        check(&format!("fades are clamped to the region (in={fade_in} out={fade_out})"), *fade_in == 4800 && *fade_in + *fade_out == 3 * r.length);
    }
    s.reverse_regions(&joined).unwrap();
    check("reverse makes a new clip", matches!(&s.project.track(a).unwrap().regions[0].content, RegionContent::Audio { offset: 0, .. }));
    let moved = s.regions_to_new_track(&joined);
    check("move to new track", moved.len() == 1 && s.project.track(moved[0]).unwrap().regions.len() == 1 && s.project.track(a).unwrap().regions.is_empty());

    // Undo the lot.
    let tracks_before = s.project.tracks.len();
    s.checkpoint("Delete Tracks", None, true);
    s.delete_tracks(&[folder]);
    check(&format!("deleting a stack deletes its subtracks ({} -> {})", tracks_before, s.project.tracks.len()), s.project.tracks.len() == tracks_before - 5);
    s.undo();
    check("undo brings the stack back", s.project.tracks.len() == tracks_before && s.project.track(a).unwrap().parent == Some(folder));
    let (lv, _) = levels(&mut s, &[b]);
    check(&format!("restored subtrack plays ({:.3})", lv[0]), lv[0] > 0.01);
    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
