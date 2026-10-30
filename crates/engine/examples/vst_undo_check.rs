//! Undo for edits in a VST3's own editor, through the `IComponentHandler` the editor would call
//! (no editor can open here), with real plug-ins: LSP's Compressor, ZamCompX2 and TONE3000. Each
//! edit is read back from the controller and the saved state and measured in a render; prints
//! SKIP for a plug-in that isn't installed. Also prints what `getState` costs.
//! usage: vst_undo_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{AutoMode, AutoParam, AutoPoint, Id, InputSel, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;
use vst3_host::{Gesture, PluginDescriptor};

static FAILED: AtomicBool = AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Runs the idle loop for a while, as the app does every 30 ms.
fn pump(s: &mut Session, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        s.idle();
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// The mix's level once the plug-in has settled, in dB.
fn level(s: &mut Session, len: i64) -> f32 {
    let r = s.render(0, len, 0, None, &mut |_| {}).unwrap();
    let peak = r.iter().flat_map(|c| &c[(len / 2) as usize..]).fold(0.0f32, |m, x| m.max(x.abs()));
    20.0 * peak.max(1e-9).log10()
}

/// A drag in the editor from one value to another, one value per idle.
fn drag(s: &mut Session, uid: Id, param: u32, from: f64, to: f64) {
    s.drive_vst_handler(uid, Gesture::Begin(param));
    for i in 1..=10 {
        s.drive_vst_handler(uid, Gesture::Perform(param, from + (to - from) * i as f64 / 10.0));
        pump(s, 15);
    }
    s.drive_vst_handler(uid, Gesture::End(param));
}

fn saved(s: &Session, track: Id) -> String {
    s.copy_plugin(track, 0).unwrap().state.unwrap()
}

fn near(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-3)
}

struct Rig {
    s: Session,
    track: Id,
    uid: Id,
    param: u32,
    /// The parameter's value at the start and the one the edits move it to.
    v0: f64,
    v1: f64,
    len: i64,
    /// Whether a render hears it: TONE3000 without a model passes silence.
    audible: bool,
}

fn rig(desc: &PluginDescriptor, param: &str, v1: f64, tone: &Path, scratch: &Path, audible: bool) -> Rig {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, vec![desc.clone()]);
    s.dir = scratch.join(format!("{}.fermata", desc.name.replace(' ', "_")));
    std::fs::create_dir_all(&s.dir).unwrap();
    let track = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(tone, track, 0).unwrap();
    let uid = s.set_plugin(track, 0, &desc.id).unwrap();
    let id = s.plugin_params(uid).iter().find(|p| p.name == param).map(|p| p.id).unwrap_or_else(|| panic!("{} has no {param}", desc.name));
    let v0 = s.vst_param(uid, id).unwrap();
    let len = daw_engine::decode::decode(tone).unwrap().channels[0].len() as i64;
    s.set_cycle(true, 0, len);
    s.watch_vst_edits_headless(uid);
    Rig { s, track, uid, param: id, v0, v1, len, audible }
}

/// What every plug-in has to do.
fn the_basics(r: &mut Rig, name: &str, param: &str) {
    let (track, uid, id, v0, v1, len) = (r.track, r.uid, r.param, r.v0, r.v1, r.len);
    let s = &mut r.s;
    let state0 = saved(s, track);
    let l0 = level(s, len);
    let steps = s.undo_depths().0;
    s.dirty = false;

    s.drive_vst_handler(uid, Gesture::Begin(id));
    s.drive_vst_handler(uid, Gesture::End(id));
    pump(s, 200);
    check(&format!("{name}: a click that moves nothing makes no step"), s.undo_depths().0 == steps && !s.dirty);

    let reads = s.vst_state_reads(uid);
    s.drive_vst_handler(uid, Gesture::Begin(id));
    for i in 1..=100 {
        s.drive_vst_handler(uid, Gesture::Perform(id, v0 + (v1 - v0) * i as f64 / 100.0));
        s.idle();
        std::thread::sleep(Duration::from_millis(1));
    }
    check(&format!("{name}: a drag of 100 values makes no step and no getState while it lasts"), s.vst_state_reads(uid) == reads && s.undo_depths().0 == steps);
    s.drive_vst_handler(uid, Gesture::End(id));
    pump(s, 200);
    let want = format!("{name}: {param}");
    check(&format!("{name}: it's one step, labelled {:?}, read once, project dirty", s.undo_label().unwrap_or_default()), s.undo_depths().0 == steps + 1 && s.undo_label().as_deref() == Some(want.as_str()) && s.vst_state_reads(uid) == reads + 1 && s.dirty);
    let state1 = saved(s, track);
    let l1 = level(s, len);
    check(&format!("{name}: the edit reached the plug-in ({l0:.2} -> {l1:.2} dB)"), near(s.vst_param(uid, id), v1) && state1 != state0 && (!r.audible || (l1 - l0).abs() > 1.0));

    let s = &mut r.s;
    s.undo();
    pump(s, 50);
    let l = level(s, len);
    check(&format!("{name}: undo puts back the parameter, the saved state and the sound ({l:.2} dB)"), near(s.vst_param(uid, id), v0) && saved(s, track) == state0 && (l - l0).abs() < 0.05);
    check(&format!("{name}: undo itself makes no step"), s.undo_depths() == (steps, 1));

    // Whatever an editor answers to the restore.
    s.drive_vst_handler(uid, Gesture::Begin(id));
    s.drive_vst_handler(uid, Gesture::Perform(id, v0));
    s.drive_vst_handler(uid, Gesture::End(id));
    s.drive_vst_params_restarted(uid);
    pump(s, 200);
    check(&format!("{name}: the plug-in's echoes during a restore make no step"), s.undo_depths() == (steps, 1) && s.redo_label().is_some());
    pump(s, 300);

    s.redo();
    pump(s, 50);
    let l = level(s, len);
    check(&format!("{name}: redo brings it all back ({l:.2} dB)"), near(s.vst_param(uid, id), v1) && saved(s, track) == state1 && (l - l1).abs() < 0.05);
    check(&format!("{name}: redo makes no extra step"), s.undo_depths() == (steps + 1, 0));
    pump(s, 400);

    // In order with the project's other edits.
    s.checkpoint("Rename", None, false);
    s.update_track(track, TrackPatch { name: Some("x".into()), ..Default::default() });
    s.undo();
    check(&format!("{name}: undoing another edit leaves the plug-in alone"), near(s.vst_param(uid, id), v1) && s.project.track(track).unwrap().name != "x");
    s.undo();
    check(&format!("{name}: and the step below it still works"), near(s.vst_param(uid, id), v0));
    s.redo();
    s.redo();
    check(&format!("{name}: redo goes back through both"), near(s.vst_param(uid, id), v1) && s.project.track(track).unwrap().name == "x");
    s.undo();
    s.undo();
    pump(s, 400);

    // Pressing undo the moment the mouse button comes up.
    drag(s, uid, id, v0, v1);
    let before = s.undo_depths().0;
    s.undo();
    check(&format!("{name}: undo straight after the release takes that edit back"), s.undo_depths().0 == before && near(s.vst_param(uid, id), v0));
    pump(s, 400);
    let l = level(s, len);
    check(&format!("{name}: and it stays back ({l:.2} dB)"), near(s.vst_param(uid, id), v0) && (l - l0).abs() < 0.05);
}

/// Edits without a begin and end; the other things that mustn't make steps.
fn no_gestures(r: &mut Rig, name: &str) {
    let (uid, id, v0, v1) = (r.uid, r.param, r.v0, r.v1);
    let s = &mut r.s;
    let steps = s.undo_depths().0;
    for i in 1..=10 {
        s.drive_vst_handler(uid, Gesture::Perform(id, v0 + (v1 - v0) * i as f64 / 20.0));
        pump(s, 30);
    }
    pump(s, 400);
    check(&format!("{name}: a burst without begin and end is one step"), s.undo_depths().0 == steps + 1);
    for i in 11..=20 {
        s.drive_vst_handler(uid, Gesture::Perform(id, v0 + (v1 - v0) * i as f64 / 20.0));
        pump(s, 30);
    }
    pump(s, 400);
    check(&format!("{name}: another straight after joins it, through the undo key"), s.undo_depths().0 == steps + 1 && near(s.vst_param(uid, id), v1));
    s.undo();
    check(&format!("{name}: undo takes both back"), near(s.vst_param(uid, id), v0) && s.undo_depths().0 == steps);
    pump(s, 400);

    // A preset loaded inside the plug-in: its values change and it says so.
    s.edit_vst_param(uid, id, v1);
    s.drive_vst_params_restarted(uid);
    pump(s, 500);
    check(&format!("{name}: restartComponent(kParamValuesChanged) is one step, {:?}", s.undo_label().unwrap_or_default()), s.undo_depths().0 == steps + 1 && s.undo_label().is_some_and(|l| l.ends_with("Parameters")));
    s.undo();
    check(&format!("{name}: undo takes the preset back"), near(s.vst_param(uid, id), v0));
    pump(s, 400);

    // MIDI Learn and the side-chain panel set parameters this way. It makes no step, but the
    // next step goes back to it.
    s.edit_vst_param(uid, id, v1);
    pump(s, 200);
    check(&format!("{name}: a parameter the host sets makes no step"), s.undo_depths().0 == steps);
    s.edit_vst_param(uid, id, v0);
    s.drive_vst_handler(uid, Gesture::Perform(id, v0));
    pump(s, 500);
    check(&format!("{name}: nor the plug-in's echo of it"), s.undo_depths().0 == steps);
    let held = v0 + (v1 - v0) * 0.5;
    s.edit_vst_param(uid, id, held);
    pump(s, 1500);
    drag(s, uid, id, held, v1);
    pump(s, 200);
    s.undo();
    check(&format!("{name}: a step goes back to what the host set before it"), near(s.vst_param(uid, id), held));
    s.edit_vst_param(uid, id, v0);
    pump(s, 1500);
}

/// A plug-in's own meter: read-only, and one that doesn't say it is.
fn meters(s: &mut Session, name: &str, uid: Id, read_only: u32, own: u32) {
    let steps = s.undo_depths().0;
    let reads = s.vst_state_reads(uid);
    for i in 0..500 {
        s.drive_vst_handler(uid, Gesture::Perform(read_only, i as f64 / 500.0));
        s.idle();
    }
    pump(s, 400);
    check(&format!("{name}: 500 values on a read-only parameter: no step, no getState"), s.undo_depths().0 == steps && s.vst_state_reads(uid) == reads);
    let start = Instant::now();
    let mut i = 0;
    while start.elapsed() < Duration::from_millis(10_500) {
        i += 1;
        s.drive_vst_handler(uid, Gesture::Perform(own, (i % 1000) as f64 / 1000.0));
        pump(s, 30);
    }
    pump(s, 400);
    check(&format!("{name}: a parameter moving itself for 10 s makes no step ({} getStates)", s.vst_state_reads(uid) - reads), s.undo_depths().0 == steps);
    s.drive_vst_handler(uid, Gesture::Perform(own, 0.123));
    pump(s, 400);
    check(&format!("{name}: and it's ignored from then on"), s.undo_depths().0 == steps);
    drag(s, uid, own, 0.123, 0.5);
    pump(s, 200);
    check(&format!("{name}: but a real gesture on it still makes a step"), s.undo_depths().0 == steps + 1);
    s.undo();
    pump(s, 400);
}

/// Off, bypassed, frozen and removed plug-ins.
fn states(r: &mut Rig, name: &str) {
    let (track, uid, id, v0, v1, len) = (r.track, r.uid, r.param, r.v0, r.v1, r.len);
    let s = &mut r.s;
    let l0 = level(s, len);
    let steps = s.undo_depths().0;

    s.update_track(track, TrackPatch { off: Some(true), ..Default::default() });
    pump(s, 100);
    drag(s, uid, id, v0, v1);
    pump(s, 200);
    check(&format!("{name}: an edit on an Off track makes a step"), s.undo_depths().0 == steps + 1);
    s.update_track(track, TrackPatch { off: Some(false), ..Default::default() });
    let on = level(s, len);
    check(&format!("{name}: back on, it plays the edit ({on:.2} dB)"), (on - l0).abs() > 1.0);
    s.update_track(track, TrackPatch { off: Some(true), ..Default::default() });
    pump(s, 100);
    s.undo();
    pump(s, 100);
    check(&format!("{name}: undo while Off"), near(s.vst_param(uid, id), v0));
    s.update_track(track, TrackPatch { off: Some(false), ..Default::default() });
    let l = level(s, len);
    check(&format!("{name}: back on, it plays the undo ({l:.2} dB)"), (l - l0).abs() < 0.05);
    pump(s, 400);

    s.set_bypass(track, 0, true);
    pump(s, 50);
    drag(s, uid, id, v0, v1);
    pump(s, 200);
    check(&format!("{name}: an edit while bypassed makes a step: the state has it"), s.undo_depths().0 == steps + 1);
    s.undo();
    s.set_bypass(track, 0, false);
    pump(s, 50);
    let l = level(s, len);
    check(&format!("{name}: undone while bypassed, it plays as before ({l:.2} dB)"), near(s.vst_param(uid, id), v0) && (l - l0).abs() < 0.05);
    pump(s, 400);

    drag(s, uid, id, v0, v1);
    pump(s, 200);
    s.checkpoint("Freeze Track", None, true);
    s.freeze_tracks(&[track], &mut |_| {}).unwrap();
    pump(s, 50);
    check(&format!("{name}: frozen, the plug-in is unloaded"), s.vst_param(uid, id).is_none());
    s.undo();
    check(&format!("{name}: unfrozen by undo, it has the edit"), near(s.vst_param(uid, id), v1));
    s.undo();
    pump(s, 50);
    check(&format!("{name}: and the edit undoes on the instance loaded since"), near(s.vst_param(uid, id), v0));
    s.redo();
    s.redo();
    pump(s, 50);
    check(&format!("{name}: redo through both freezes it again"), s.vst_param(uid, id).is_none() && s.project.track(track).unwrap().frozen.is_some());
    s.undo();
    s.undo();
    pump(s, 50);
    check(&format!("{name}: undo through both again"), near(s.vst_param(uid, id), v0));

    s.watch_vst_edits_headless(uid);
    drag(s, uid, id, v0, v1);
    pump(s, 200);
    s.checkpoint("Remove Plug-in", None, true);
    s.remove_plugin(track, 0);
    pump(s, 50);
    s.undo();
    check(&format!("{name}: a removed plug-in comes back with the edit"), near(s.vst_param(uid, id), v1));
    s.undo();
    let l = level(s, len);
    check(&format!("{name}: and the edit undoes ({l:.2} dB)"), near(s.vst_param(uid, id), v0) && (l - l0).abs() < 0.05);
    s.watch_vst_edits_headless(uid);
    pump(s, 400);
}

/// Automation: a move while writing belongs to the pass, and playing it back makes no steps.
fn automation(r: &mut Rig, name: &str) {
    let (track, uid, id, v0, v1) = (r.track, r.uid, r.param, r.v0, r.v1);
    let s = &mut r.s;
    let lane = |s: &Session| s.project.track(track).unwrap().automation.iter().find(|l| matches!(l.param, AutoParam::Plugin { param, .. } if param == id)).map_or(0, |l| l.points.len());
    let steps = s.undo_depths().0;
    s.set_auto_mode(track, AutoMode::Touch);
    s.locate(0);
    s.play();
    pump(s, 100);
    drag(s, uid, id, v0, v1);
    pump(s, 300);
    // The pass's own step comes as the gesture ends.
    check(&format!("{name}: in Touch, playing, a drag makes no step of its own, only the pass's {:?}", s.undo_label().unwrap_or_default()), s.undo_depths().0 <= steps + 1 && !s.undo_label().is_some_and(|l| l.starts_with(name)));
    s.stop();
    pump(s, 200);
    let written = lane(s);
    check(&format!("{name}: it's written to the lane as before ({written} points), one step {:?}", s.undo_label().unwrap_or_default()), written >= 2 && s.undo_depths().0 == steps + 1 && !s.undo_label().is_some_and(|l| l.starts_with(name)));
    s.undo();
    check(&format!("{name}: undo removes the pass"), lane(s) == 0);
    s.set_auto_mode(track, AutoMode::Read);
    s.edit_vst_param(uid, id, v0);
    pump(s, 1500);

    let steps = s.undo_depths().0;
    s.set_auto_mode(track, AutoMode::Latch);
    s.drive_vst_handler(uid, Gesture::Begin(id));
    s.drive_vst_handler(uid, Gesture::Perform(id, v1));
    s.drive_vst_handler(uid, Gesture::End(id));
    pump(s, 200);
    check(&format!("{name}: stopped, the same drag is a step, in any mode"), s.undo_depths().0 == steps + 1 && lane(s) == 0);
    s.undo();
    pump(s, 400);
    s.set_auto_mode(track, AutoMode::Read);

    let pts = vec![AutoPoint { pos: 0, value: v0 as f32, curve: 0.0 }, AutoPoint { pos: r.len, value: v1 as f32, curve: 0.0 }];
    s.set_automation_lane(track, AutoParam::Plugin { uid, param: id, name: String::new() }, pts);
    let steps = s.undo_depths().0;
    let reads = s.vst_state_reads(uid);
    s.locate(0);
    s.play();
    pump(s, 1000);
    s.stop();
    pump(s, 200);
    check(&format!("{name}: automation playing makes no step ({} getStates)", s.vst_state_reads(uid) - reads), s.undo_depths().0 == steps);
    s.set_automation_lane(track, AutoParam::Plugin { uid, param: id, name: String::new() }, Vec::new());
    s.edit_vst_param(uid, id, v0);
    pump(s, 1500);
}

/// Undo and redo with the audio thread running the plug-in, again and again.
fn live(r: &mut Rig, name: &str) {
    let (track, uid, id, v0, v1) = (r.track, r.uid, r.param, r.v0, r.v1);
    let s = &mut r.s;
    let meter = |s: &mut Session, ms: u64| {
        let mut m = 0.0f32;
        pump(s, 100);
        s.tick();
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            pump(s, 20);
            m = m.max(s.tick().meters.iter().find(|x| x.0 == track).map_or(0.0, |x| x.1.max(x.2)));
        }
        20.0 * m.max(1e-9).log10()
    };
    s.locate(0);
    s.play();
    let a = meter(s, 300);
    drag(s, uid, id, v0, v1);
    pump(s, 200);
    let b = meter(s, 300);
    check(&format!("{name}: live, the drag changes what plays ({a:.1} -> {b:.1} dB)"), (a - b).abs() > 1.0);
    s.undo();
    let c = meter(s, 300);
    check(&format!("{name}: live, undo puts it back ({c:.1} dB)"), (a - c).abs() < 0.5);
    s.redo();
    let d = meter(s, 300);
    check(&format!("{name}: live, redo again ({d:.1} dB)"), (b - d).abs() < 0.5);
    let depths = s.undo_depths();
    for _ in 0..50 {
        s.undo();
        pump(s, 5);
        s.redo();
        pump(s, 5);
    }
    let e = meter(s, 300);
    check(&format!("{name}: 50 undos and redos while playing: still right ({e:.1} dB), no steps"), (b - e).abs() < 0.5 && s.undo_depths() == depths);
    s.undo();
    s.stop();
    pump(s, 400);
}

/// What `getState` costs, the only call into the plug-in that's added to a gesture.
fn costs(all: &[PluginDescriptor]) {
    for name in ["TONE3000", "ZamCompX2", "Compressor Stereo", "Sidechain Compressor Stereo", "Parametric Equalizer x32 Stereo", "Dragonfly Hall Reverb", "CHOWTapeModel", "Multi-Sampler x12 DirectOut", "ZamGrains"] {
        let Some(d) = all.iter().find(|d| d.name == name) else {
            println!("SKIP the cost of {name}: not installed");
            continue;
        };
        let (_p, c) = vst3_host::instantiate(d, 48000.0, 512).unwrap();
        let mut times: Vec<Duration> = Vec::new();
        let mut size = 0;
        for _ in 0..200 {
            let t = Instant::now();
            let st = c.save_state();
            times.push(t.elapsed());
            size = st.component.len() + st.controller.len();
        }
        times.sort();
        let st = c.save_state();
        let t = Instant::now();
        for _ in 0..20 {
            c.load_state(&st);
        }
        let load = t.elapsed() / 20;
        println!("     {name}: getState {size} bytes, median {:?}, worst {:?}; load_state {load:?}; {} parameters", times[100], times[199], c.param_names().len());
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = PathBuf::from(args.next().expect("audio file"));
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let all: Vec<PluginDescriptor> = vst3_host::scan::find_bundles(&vst3_host::scan::default_search_paths()).iter().flat_map(|b| vst3_host::scan::scan_bundle(b).unwrap_or_default()).collect();
    let find = |n: &str| all.iter().find(|d| d.name == n).cloned();
    costs(&all);

    match find("Compressor Stereo") {
        Some(d) => {
            let mut r = rig(&d, "Output gain", 0.0, &tone, &scratch, true);
            r.v1 = r.v0 * 0.5;
            the_basics(&mut r, &d.name, "Output gain");
            no_gestures(&mut r, &d.name);
            states(&mut r, &d.name);
            automation(&mut r, &d.name);
            live(&mut r, &d.name);
        }
        None => println!("SKIP LSP's Compressor Stereo isn't installed"),
    }
    match find("ZamCompX2") {
        Some(d) => {
            let mut r = rig(&d, "Makeup", 0.8, &tone, &scratch, true);
            the_basics(&mut r, &d.name, "Makeup");
            states(&mut r, &d.name);
            live(&mut r, &d.name);
            let names: Vec<(u32, String, bool)> = {
                let (_p, c) = vst3_host::instantiate(&d, 48000.0, 512).unwrap();
                c.param_names()
            };
            let id = |n: &str| names.iter().find(|p| p.1 == n).map(|p| p.0).unwrap();
            let (uid, ro, own) = (r.uid, id("Gain Reduction"), id("Slew"));
            meters(&mut r.s, &d.name, uid, ro, own);
        }
        None => println!("SKIP ZamCompX2 isn't installed"),
    }
    match find("TONE3000") {
        Some(d) => {
            let mut r = rig(&d, "outputLevel", 0.0, &tone, &scratch, false);
            r.v1 = if r.v0 > 0.5 { r.v0 - 0.3 } else { r.v0 + 0.3 };
            the_basics(&mut r, &d.name, "outputLevel");
        }
        None => println!("SKIP TONE3000 isn't installed"),
    }
    std::process::exit(FAILED.load(Ordering::Relaxed) as i32);
}
