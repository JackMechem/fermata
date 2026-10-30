//! Exercises controller assignments (MIDI Learn) against a running engine (silent ALSA null
//! output), with controller messages fed in through the hardware input path: learning, levels
//! and plug-in parameters following, Pickup, ranges, buttons, automation writing, consumed
//! messages and the saved list. usage: controller_check <audio file> <scratch dir>
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::controllers::{CtlKind, CtlTarget, Takeover, TransportAction};
use daw_engine::model::{AutoMode, AutoParam, InputSel, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// The port the messages pretend to come from, with the numbers ALSA appends.
const PORT: &str = "Test Controller:Test Controller MIDI 1 20:0";

/// Sends a control change on channel 1 and lets the session handle it, as `idle` does every
/// 30 ms in the app.
fn cc(s: &mut Session, number: u8, value: u8) {
    s.inject_midi_from(PORT, &[0xB0, number, value]);
    s.idle();
}

fn learn(s: &mut Session, target: CtlTarget, number: u8) -> Option<String> {
    s.start_learn(Some(target)).unwrap();
    cc(s, number, 64);
    s.tick().learned
}

fn index_of(s: &Session, number: u8) -> usize {
    s.controller_assignments().iter().position(|r| r.assignment.controller.kind == CtlKind::Cc { number }).expect("assignment")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = std::path::PathBuf::from(args.next().expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("ControllerCheck.fermata");
    let file = scratch.join("controller-assignments.json");
    s.load_controller_assignments(file.clone());

    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.select_track(Some(inst));
    let comp = s.set_plugin(a, 0, "builtin:compressor").unwrap();
    let vol = |s: &Session| s.project.track(a).unwrap().volume_db;

    // Learning the last touched parameter.
    check("learning needs a touched parameter first", s.start_learn(None).is_err());
    s.touched_patch(a, &TrackPatch { volume_db: Some(0.0), ..Default::default() });
    let name = s.start_learn(None).unwrap();
    check(&format!("learn names the parameter ({name})"), name.ends_with("› Volume"));
    check("the tick shows the learn", s.tick().learn.as_deref() == Some(name.as_str()));
    let before = vol(&s);
    cc(&mut s, 7, 30);
    let t = s.tick();
    check(&format!("a control change is learned ({:?})", t.learned), t.learned.is_some() && t.learn.is_none());
    check("the learning move doesn't act", vol(&s) == before);
    let rows = s.controller_assignments();
    let c = &rows[0].assignment.controller;
    check(&format!("assigned to the port without ALSA's numbers ({:?})", c), rows.len() == 1 && c.port == "Test Controller:Test Controller MIDI 1" && c.channel == 0 && c.kind == CtlKind::Cc { number: 7 });

    // The fader follows, on its taper.
    cc(&mut s, 7, 127);
    check(&format!("CC 127 is +6 dB ({})", vol(&s)), (vol(&s) - 6.0).abs() < 0.01);
    cc(&mut s, 7, 0);
    check(&format!("CC 0 is -inf ({})", vol(&s)), vol(&s) <= -96.0);
    cc(&mut s, 7, 79);
    check(&format!("CC 79 is near -12 dB, like the fader ({})", vol(&s)), (vol(&s) + 12.0).abs() < 0.5);
    check("a controller move is an undo step", s.undo_label().as_deref() == Some("Volume"));

    // Assigned messages don't reach the instrument; others do.
    std::thread::sleep(Duration::from_millis(50));
    let seen = s.tick().midi_activity;
    for v in 0..20 {
        cc(&mut s, 7, 40 + v);
    }
    std::thread::sleep(Duration::from_millis(50));
    let after = s.tick().midi_activity;
    check(&format!("assigned controllers are consumed ({seen} -> {after})"), after == seen);
    s.inject_midi_from(PORT, &[0xB0, 1, 64]);
    s.inject_midi_from(PORT, &[0x90, 60, 100]);
    std::thread::sleep(Duration::from_millis(50));
    let after = s.tick().midi_activity;
    check(&format!("other messages still play the instrument ({seen} -> {after})"), after == seen + 2);
    // Another channel or port is another controller.
    s.inject_midi_from(PORT, &[0xB1, 7, 127]);
    s.inject_midi_from("Other Port 24:0", &[0xB0, 7, 127]);
    s.idle();
    std::thread::sleep(Duration::from_millis(50));
    check("another channel or port isn't assigned", s.tick().midi_activity == seen + 4 && vol(&s) < 0.0);

    // A built-in effect's parameter.
    let threshold = |s: &Session| s.builtin_params(comp).iter().find(|p| p.0 == 1).map(|p| p.1).unwrap();
    let learned = learn(&mut s, CtlTarget::Plugin { track: a, uid: comp, plugin: String::new(), param: 1 }, 20);
    check(&format!("a plug-in parameter is learned ({learned:?})"), learned.is_some_and(|l| l.contains("Threshold")));
    cc(&mut s, 20, 0);
    let lo = threshold(&s);
    cc(&mut s, 20, 127);
    let hi = threshold(&s);
    check(&format!("the plug-in parameter follows ({lo} .. {hi})"), lo <= -59.9 && hi >= -0.1);

    // Pickup: ignored until the controller reaches the fader.
    let i = index_of(&s, 7);
    s.update_controller_assignment(i, Takeover::Pickup, 0.0, 1.0).unwrap();
    s.update_track_linked(a, TrackPatch { volume_db: Some(-12.0), ..Default::default() });
    cc(&mut s, 7, 10);
    cc(&mut s, 7, 50);
    check(&format!("pickup ignores the controller below the fader ({})", vol(&s)), vol(&s) == -12.0);
    cc(&mut s, 7, 100);
    let caught = vol(&s);
    check(&format!("pickup catches it on the way past ({caught})"), caught > -12.0);
    cc(&mut s, 7, 60);
    check(&format!("then it follows ({})", vol(&s)), vol(&s) < -12.0);
    s.update_track_linked(a, TrackPatch { volume_db: Some(0.0), ..Default::default() });
    cc(&mut s, 7, 61);
    check("moving the fader with the mouse lets go of it", vol(&s) == 0.0);

    // A range: the controller covers the middle of the pan knob's travel.
    learn(&mut s, CtlTarget::Pan { track: a }, 10);
    let i = index_of(&s, 10);
    s.update_controller_assignment(i, Takeover::Jump, 0.25, 0.75).unwrap();
    cc(&mut s, 10, 127);
    let pan = s.project.track(a).unwrap().pan;
    check(&format!("the range limits the travel ({pan})"), (pan - 0.5).abs() < 0.02);

    // Buttons: a press toggles, the release does nothing.
    learn(&mut s, CtlTarget::Mute { track: a }, 30);
    let muted = |s: &Session| s.project.track(a).unwrap().mute;
    cc(&mut s, 30, 127);
    let on = muted(&s);
    cc(&mut s, 30, 0);
    let held = muted(&s);
    cc(&mut s, 30, 127);
    check(&format!("a mute button toggles on press ({on} {held} {})", muted(&s)), on && held && !muted(&s));
    learn(&mut s, CtlTarget::Transport { action: TransportAction::Cycle }, 31);
    let was = s.project.cycle.enabled;
    cc(&mut s, 31, 127);
    cc(&mut s, 31, 0);
    check("a cycle button toggles Cycle", s.project.cycle.enabled != was);
    learn(&mut s, CtlTarget::Transport { action: TransportAction::Play }, 32);
    s.set_cycle(true, 0, 48000);
    cc(&mut s, 32, 127);
    cc(&mut s, 32, 0);
    std::thread::sleep(Duration::from_millis(100));
    let playing = s.is_playing();
    cc(&mut s, 32, 127);
    cc(&mut s, 32, 0);
    std::thread::sleep(Duration::from_millis(100));
    check(&format!("a play button starts and stops ({playing} {})", s.is_playing()), playing && !s.is_playing());

    // Automation: in Touch, controller moves are written like fader moves.
    let i = index_of(&s, 7);
    s.update_controller_assignment(i, Takeover::Jump, 0.0, 1.0).unwrap();
    s.set_auto_mode(a, AutoMode::Touch);
    s.locate(0);
    s.play();
    for v in [20u8, 40, 60, 80, 100] {
        cc(&mut s, 7, v);
        std::thread::sleep(Duration::from_millis(40));
        s.idle();
    }
    // Let go: the controller stops moving and its pass ends.
    std::thread::sleep(Duration::from_millis(700));
    s.idle();
    let lane = s.project.track(a).unwrap().automation.iter().find(|l| matches!(l.param, AutoParam::Volume)).map(|l| l.points.len()).unwrap_or(0);
    check(&format!("touch writes a volume lane after the controller stops ({lane} points)"), lane >= 2);
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    s.set_auto_mode(a, AutoMode::Read);

    // A target that isn't in the project is shown as missing and does nothing.
    learn(&mut s, CtlTarget::Volume { track: inst }, 40);
    s.delete_track(inst);
    let row = s.controller_assignments().into_iter().find(|r| r.assignment.controller.kind == CtlKind::Cc { number: 40 }).unwrap();
    check(&format!("a deleted track's assignment is unavailable ({})", row.name), !row.available);
    let before = vol(&s);
    cc(&mut s, 40, 127);
    check("and ignored", vol(&s) == before);

    // The list is saved, and loads back.
    let rows = s.controller_assignments();
    let saved = std::fs::read_to_string(&file).unwrap_or_default();
    s.load_controller_assignments(file.clone());
    let again = s.controller_assignments();
    check(
        &format!("assignments are saved and load back ({} in the file)", saved.matches("\"controller\"").count()),
        again.len() == rows.len() && again.iter().zip(&rows).all(|(x, y)| x.assignment == y.assignment),
    );
    let i = index_of(&s, 40);
    s.delete_controller_assignment(i).unwrap();
    check("delete removes one", s.controller_assignments().len() == rows.len() - 1);
    // A deleted assignment's controller plays the instrument again.
    let seen = s.tick().midi_activity;
    s.inject_midi_from(PORT, &[0xB0, 40, 1]);
    std::thread::sleep(Duration::from_millis(50));
    check("a deleted assignment's controller isn't consumed", s.tick().midi_activity == seen + 1);

    // Cancelling a learn assigns nothing and lets controllers through again.
    let n = s.controller_assignments().len();
    s.start_learn(Some(CtlTarget::MasterVolume)).unwrap();
    s.cancel_learn();
    cc(&mut s, 50, 100);
    check("cancel assigns nothing", s.controller_assignments().len() == n && s.tick().learn.is_none());

    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
