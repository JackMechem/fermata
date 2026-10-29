//! Recording with a count-in from the start of the project, where the transport runs at
//! negative positions before the punch (this used to overflow and abort the app).
//! usage: countin_check <scratch dir>
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;

fn main() {
    let scratch = std::env::args().nth(1).expect("scratch dir");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::path::PathBuf::from(&scratch).join("CountIn.fermata");
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.update_track(t, TrackPatch { armed: Some(true), ..Default::default() });
    s.set_count_in(true);
    // A bar that isn't a whole number of blocks, so one block straddles position 0.
    s.set_tempo(123.0);
    let mut failed = false;
    for round in 0..3 {
        s.locate(0);
        s.record().unwrap();
        std::thread::sleep(Duration::from_millis(400));
        s.stop();
        std::thread::sleep(Duration::from_millis(100));
        let errors = s.status.engine_errors.load(std::sync::atomic::Ordering::Relaxed);
        let ok = errors == 0;
        failed |= !ok;
        println!("{} round {round}: recorded with count-in from bar 1 (engine errors: {errors})", if ok { "PASS" } else { "FAIL" });
    }
    std::process::exit(if failed { 1 } else { 0 });
}
