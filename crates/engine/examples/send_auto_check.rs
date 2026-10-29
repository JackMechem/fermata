//! Send-level automation: a lane on a send changes what reaches the aux. usage: send_auto_check <audio file> <scratch dir>
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{AutoMode, AutoParam, AutoPoint, InputSel, TrackKind, TrackSend};
use daw_engine::Session;


/// Plays for a moment and returns the peak meter of each track id given, plus Stereo Out.
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

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = args.next().expect("scratch dir");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::path::PathBuf::from(&scratch).join("SendAuto.fermata");
    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    s.set_cycle(true, 0, 48000);
    let aux = s.create_aux(None).unwrap();
    s.set_send(a, 0, Some(TrackSend { bus: aux, level_db: 0.0, pre_fader: false, muted: false })).unwrap();
    let (lv, _) = levels(&mut s, &[aux]);
    let before = lv[0];
    s.set_automation_lane(a, AutoParam::Send(0), vec![AutoPoint { pos: 0, value: -20.0 }, AutoPoint { pos: 48000, value: -20.0 }]);
    let (lv, _) = levels(&mut s, &[aux]);
    let drop = 20.0 * (lv[0] / before).log10();
    let ok = (drop + 20.0).abs() < 1.0;
    println!("{} send lane at -20 dB lowers the aux by {drop:.2} dB", if ok { "PASS" } else { "FAIL" });
    s.set_auto_mode(a, AutoMode::Off);
    let (lv, _) = levels(&mut s, &[aux]);
    let ok2 = (lv[0] - before).abs() / before < 0.05;
    println!("{} Off plays the knob's level again ({:.3} vs {before:.3})", if ok2 { "PASS" } else { "FAIL" }, lv[0]);
    FAILED.fetch_or(!(ok && ok2), std::sync::atomic::Ordering::Relaxed);
    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
