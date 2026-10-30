//! Checks reordering Audio FX inserts (silent ALSA null output): a move changes what the
//! engine runs, not just the project; the instances keep running (same ids, nothing reloaded);
//! undo and redo put them back the same way; moving or copying onto another strip makes
//! room there; and a plug-in moved to another track takes its automation along.
//! usage: insert_order_check <audio file> <scratch dir>
use std::path::PathBuf;
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::fx;
use daw_engine::model::{AutoLane, AutoParam, AutoPoint, Id, InputSel, TrackKind, MASTER_ID};
use daw_engine::Session;

static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) -> bool {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
    ok
}

fn settle(s: &mut Session) {
    std::thread::sleep(Duration::from_millis(40));
    s.idle();
}

/// One track through its strip.
fn render(s: &mut Session, len: i64, track: Id) -> Vec<Vec<f32>> {
    let iso = s.isolate_track(track, true, true);
    s.render(0, len, 0, Some(iso), &mut |_| {}).unwrap()
}

fn diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs())).fold(0.0, f32::max)
}

fn uids(s: &Session, track: Id) -> Vec<Option<Id>> {
    let ins = if track == MASTER_ID { &s.project.master.inserts } else { &s.project.track(track).unwrap().inserts };
    ins.iter().map(|p| p.as_ref().map(|p| p.uid)).collect()
}

fn names(s: &Session, track: Id) -> Vec<String> {
    s.project.track(track).unwrap().inserts.iter().map(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "-".into())).collect()
}

/// A compressor squashing hard, so its place before or after the delay is clearly audible.
fn load_pair(s: &mut Session, track: Id, comp_slot: usize, delay_slot: usize) {
    s.set_plugin(track, comp_slot, "builtin:compressor").unwrap();
    s.set_plugin(track, delay_slot, "builtin:delay").unwrap();
    let info = fx::find("builtin:compressor").unwrap();
    for p in info.params {
        let v = match p.name {
            "Threshold" => p.min,
            "Ratio" => p.max,
            _ => continue,
        };
        s.set_builtin_param(track, comp_slot, p.id, v).unwrap();
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("InsertOrder.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();
    let len = 48000;

    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    load_pair(&mut s, a, 0, 1);
    settle(&mut s);
    let before = render(&mut s, len, a);
    let ids = uids(&s, a);

    // Reference: a second track loaded the other way round from the start.
    let b = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), b, 0).unwrap();
    load_pair(&mut s, b, 1, 0);
    settle(&mut s);
    let reversed = render(&mut s, len, b);
    check("the order of the compressor and the delay is audible", diff(&before, &reversed) > 1e-3);

    s.checkpoint("Move Plug-in", None, true);
    s.move_plugin(a, 1, a, 0, false).unwrap();
    settle(&mut s);
    check(&format!("moving slot 2 to slot 1 swaps them ({:?})", &names(&s, a)[..2]), uids(&s, a)[..2] == [ids[1], ids[0]]);
    let moved = render(&mut s, len, a);
    check(&format!("the engine plays the new order (diff {:.2e})", diff(&moved, &reversed)), diff(&moved, &reversed) < 1e-5);

    s.undo();
    settle(&mut s);
    check("undo puts the same instances back in their slots", uids(&s, a) == ids);
    check("after undo the engine plays the old order", diff(&render(&mut s, len, a), &before) < 1e-5);
    s.redo();
    settle(&mut s);
    check("redo moves them again", uids(&s, a)[..2] == [ids[1], ids[0]] && diff(&render(&mut s, len, a), &reversed) < 1e-5);
    s.undo();
    settle(&mut s);

    // Moving down past an empty slot: the ones between shift up.
    s.set_plugin(a, 3, "builtin:reverb").unwrap();
    let r = uids(&s, a)[3];
    s.move_plugin(a, 0, a, 3, false).unwrap();
    settle(&mut s);
    check(&format!("moving slot 1 to slot 4 shifts the rest up ({:?})", &names(&s, a)[..4]), uids(&s, a)[..4] == [ids[1], None, r, ids[0]]);
    s.move_plugin(a, 3, a, 0, false).unwrap();
    s.move_plugin(a, 3, a, 2, false).unwrap();
    s.remove_plugin(a, 2);
    settle(&mut s);
    check("and back", uids(&s, a) == ids);

    // Onto another strip: whatever is in the target slot shifts down.
    let b_ids = uids(&s, b);
    s.checkpoint("Move Plug-in", None, true);
    s.move_plugin(a, 0, b, 0, false).unwrap();
    settle(&mut s);
    let bn = names(&s, b);
    check(&format!("a move to another strip makes room there ({:?})", &bn[..3]), bn[..3] == ["Compressor", "Delay", "Compressor"] && uids(&s, b)[1..3] == b_ids[..2]);
    check("and leaves the source slot empty", uids(&s, a)[0].is_none() && uids(&s, a)[1] == ids[1]);
    let moved_comp = s.project.track(b).unwrap().inserts[0].clone().unwrap();
    let orig = s.project.track(b).unwrap().inserts[2].clone().unwrap();
    check("with its settings", moved_comp.params == orig.params);
    s.undo();
    settle(&mut s);
    check("undo brings both strips back", uids(&s, a) == ids && uids(&s, b) == b_ids);

    s.move_plugin(a, 1, b, 1, true).unwrap();
    settle(&mut s);
    check("a copy keeps the original", uids(&s, a) == ids && names(&s, b)[..3] == ["Delay", "Delay", "Compressor"]);

    // A copy onto the same strip.
    s.move_plugin(a, 0, a, 0, true).unwrap();
    settle(&mut s);
    check(&format!("a copy on the same strip goes in at the drop slot ({:?})", &names(&s, a)[..3]), names(&s, a)[..3] == ["Compressor", "Compressor", "Delay"] && uids(&s, a)[1] == ids[0]);

    // A full strip has no room.
    let c = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    for i in 0..8 {
        s.set_plugin(c, i, "builtin:tuner").unwrap();
    }
    let full = uids(&s, a);
    check("a full strip refuses a drop", s.move_plugin(a, 0, c, 0, false).is_err() && uids(&s, a) == full);

    // Stereo Out.
    s.set_plugin(MASTER_ID, 0, "builtin:limiter").unwrap();
    s.set_plugin(MASTER_ID, 1, "builtin:loudness").unwrap();
    let m = uids(&s, MASTER_ID);
    s.checkpoint("Move Plug-in", None, true);
    s.move_plugin(MASTER_ID, 0, MASTER_ID, 1, false).unwrap();
    check("Stereo Out reorders", uids(&s, MASTER_ID)[..2] == [m[1], m[0]]);
    s.undo();
    settle(&mut s);
    check("and undoes", uids(&s, MASTER_ID) == m);

    // A save and reload agrees with what the engine was playing.
    s.move_plugin(a, 2, a, 0, false).unwrap();
    settle(&mut s);
    let live = render(&mut s, len, a);
    let path = s.save(None).unwrap();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut t = Session::new(cfg, Vec::new());
    t.open(&path).unwrap();
    settle(&mut t);
    let d = diff(&render(&mut t, len, a), &live);
    let same_order = names(&t, a) == names(&s, a);
    check(&format!("a reloaded project plays the same as the reordered one ({:?}, diff {d:.2e})", &names(&t, a)[..3]), same_order && d < 1e-5);

    // One reloaded session at a time: each holds an ALSA sequencer queue.
    drop(t);
    automation_follows(&mut s, &tone, len, &scratch);

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}

/// The plug-in lanes of a track, as (instance, parameter, points).
fn plugin_lanes(s: &Session, track: Id) -> Vec<(Id, u32, Vec<AutoPoint>)> {
    let lanes: &[AutoLane] = &s.project.track(track).unwrap().automation;
    lanes.iter().filter_map(|l| match l.param {
        AutoParam::Plugin { uid, param, .. } => Some((uid, param, l.points.clone())),
        _ => None,
    }).collect()
}

/// A compressor's Make Up automated on one track, then the compressor dragged to another.
fn automation_follows(s: &mut Session, tone: &str, len: i64, scratch: &std::path::Path) {
    let d = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    let e = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    for t in [d, e] {
        s.import_audio(std::path::Path::new(tone), t, 0).unwrap();
    }
    let comp = s.set_plugin(d, 0, "builtin:compressor").unwrap();
    let makeup = fx::find("builtin:compressor").unwrap().params.iter().find(|p| p.name == "Make Up").unwrap().id;
    // Make Up all the way down (-20 dB), well away from its 0 dB default.
    let points = vec![AutoPoint { pos: 0, value: 0.0, curve: 0.0 }, AutoPoint { pos: len, value: 0.0, curve: 0.0 }];
    s.set_automation_lane(d, AutoParam::Plugin { uid: comp, param: makeup, name: "Compressor: Make Up".into() }, points.clone());
    // A lane of the track's own, at pan's centre so the two tracks still sound alike.
    s.set_automation_lane(d, AutoParam::Pan, vec![AutoPoint { pos: 0, value: 0.0, curve: 0.0 }]);
    settle(s);
    let dry = render(s, len, e);
    let driven = render(s, len, d);
    check(&format!("the Make Up lane is audible (diff {:.2e})", diff(&driven, &dry)), diff(&driven, &dry) > 1e-2);

    s.checkpoint("Move Plug-in", None, true);
    s.move_plugin(d, 0, e, 0, false).unwrap();
    settle(s);
    let new_uid = uids(s, e)[0].unwrap();
    let on_e = plugin_lanes(s, e);
    check(&format!("a move to another track takes its lane along ({on_e:?})"), on_e == vec![(new_uid, makeup, points.clone())] && new_uid != comp);
    let left = &s.project.track(d).unwrap().automation;
    check("and leaves the track's own lanes behind", left.len() == 1 && left[0].param.same(&AutoParam::Pan));
    let moved = render(s, len, e);
    s.checkpoint("Delete Automation", None, false);
    s.clear_automation(e, None);
    settle(s);
    let unautomated = render(s, len, e);
    s.undo();
    settle(s);
    let ok = diff(&moved, &driven) < 1e-4 && diff(&unautomated, &driven) > 1e-2;
    check(&format!("the lane drives the compressor on its new track (diff {:.2e}, {:.2e} without it)", diff(&moved, &driven), diff(&unautomated, &driven)), ok);

    s.undo();
    settle(s);
    check("undo puts the lane back on the old track, on the old instance", plugin_lanes(s, d) == vec![(comp, makeup, points.clone())] && plugin_lanes(s, e).is_empty());
    check("where it drives the compressor again", diff(&render(s, len, d), &driven) < 1e-4 && diff(&render(s, len, e), &dry) < 1e-5);
    s.redo();
    settle(s);
    check("redo moves it again", plugin_lanes(s, e) == vec![(new_uid, makeup, points.clone())] && plugin_lanes(s, d).is_empty());
    check("and it drives the compressor there", diff(&render(s, len, e), &driven) < 1e-4);

    // A copy leaves the lanes with the original; the copy starts with none.
    s.move_plugin(e, 0, d, 0, true).unwrap();
    settle(s);
    check("a copy leaves the lane with the original", plugin_lanes(s, e) == vec![(new_uid, makeup, points.clone())] && plugin_lanes(s, d).is_empty());
    s.remove_plugin(d, 0);

    // Stereo Out has automation too: the lane goes along there.
    s.checkpoint("Move Plug-in", None, true);
    s.move_plugin(e, 0, MASTER_ID, 2, false).unwrap();
    let on_master: Vec<Id> = s.project.master.automation.iter().filter_map(|l| match l.param { AutoParam::Plugin { uid, .. } => Some(uid), _ => None }).collect();
    let master_uid = uids(s, MASTER_ID)[2];
    check("a move to Stereo Out takes the lane along", plugin_lanes(s, e).is_empty() && uids(s, e)[0].is_none() && master_uid.is_some() && on_master == vec![master_uid.unwrap()]);
    s.undo();
    settle(s);

    // A save and reload keeps the moved lane on its new instance.
    let path = s.save(None).unwrap();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut t = Session::new(cfg, Vec::new());
    t.dir = scratch.join("InsertOrder.fermata");
    t.open(&path).unwrap();
    settle(&mut t);
    let reloaded = render(&mut t, len, e);
    check(&format!("a reloaded project keeps the moved lane and plays it (diff {:.2e})", diff(&reloaded, &driven)), plugin_lanes(&t, e) == vec![(uids(&t, e)[0].unwrap(), makeup, points)] && diff(&reloaded, &driven) < 1e-4);
}
