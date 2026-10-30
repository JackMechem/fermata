//! Checks Track On/Off with real VST3s, live on the silent ALSA null output: `suspend` and resume
//! while playing, delay compensation and the audio thread's load.
//! usage: onoff_vst_check <audio file> <scratch dir>
use std::path::PathBuf;
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn off(s: &mut Session, id: Id, v: bool) {
    s.update_track(id, TrackPatch { off: Some(v), ..Default::default() });
}

/// The loudest a track's meter reads over `ms`, and the mean load.
fn watch(s: &mut Session, id: Id, ms: u64) -> (f32, f32) {
    let (mut level, mut cpu, mut n) = (0.0f32, 0.0f32, 0);
    for _ in 0..ms / 20 {
        std::thread::sleep(Duration::from_millis(20));
        let t = s.tick();
        level = level.max(t.meters.iter().find(|m| m.0 == id).map(|m| m.1.max(m.2)).unwrap_or(0.0));
        cpu += t.cpu as f32;
        n += 1;
    }
    (level, cpu / n.max(1) as f32)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = PathBuf::from(args.next().expect("audio file"));
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let plugins = daw_engine::plugin_db::load_cache().unwrap_or_default();
    let find = |n: &str| plugins.iter().find(|d| d.name == n).cloned();
    let (Some(maxim), Some(amp)) = (find("ZaMaximX2"), find("TONE3000")) else {
        println!("FAIL ZaMaximX2 or TONE3000 isn't in the plug-in cache; scan plug-ins first");
        std::process::exit(1);
    };
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, plugins);
    s.dir = scratch.join("OnOffVstCheck.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();

    let step = daw_engine::decode::decode(&tone).unwrap().channels[0].len() as i64;
    println!("     the tone is {step} frames at {} Hz", s.sample_rate());
    let track = |s: &mut Session, id: &str| {
        let t = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        s.import_audio(&tone, t, 0).unwrap();
        if !id.is_empty() {
            s.set_plugin(t, 0, id).unwrap();
        }
        t
    };
    let plain = track(&mut s, "");
    let latent = track(&mut s, &maxim.id);
    let amps: Vec<Id> = (0..3).map(|_| track(&mut s, &amp.id)).collect();

    let total = s.pdc_report().total;
    check(&format!("the maximizer's latency counts while on ({total} frames)"), total > 0);
    off(&mut s, latent, true);
    let without = s.pdc_report().total;
    println!("     with it off, the total is {without} frames");
    off(&mut s, latent, false);

    // The null device has no clock and runs far faster than real time: a cycle keeps the signal coming.
    s.set_cycle(true, 0, step);
    s.play();
    let (on, _) = watch(&mut s, latent, 500);
    check(&format!("a VST3 track plays ({on:.3})"), on > 0.001);
    off(&mut s, latent, true);
    watch(&mut s, latent, 300);
    let (quiet, _) = watch(&mut s, latent, 200);
    check(&format!("off, its meter falls to nothing ({quiet:.4})"), quiet == 0.0);
    off(&mut s, plain, true);
    watch(&mut s, plain, 300);
    off(&mut s, plain, false);
    let (control, _) = watch(&mut s, plain, 500);
    check(&format!("back on, a plain track plays again ({control:.3}) at position {}", s.tick().pos), control > 0.001);
    off(&mut s, latent, false);
    let (back, _) = watch(&mut s, latent, 500);
    check(&format!("back on, the suspended plug-in plays again ({back:.3})"), back > 0.001);

    // Switching fast while playing, across every plug-in.
    let (_, cpu_on) = watch(&mut s, amps[0], 1000);
    for _ in 0..40 {
        for &a in &amps {
            off(&mut s, a, true);
        }
        std::thread::sleep(Duration::from_millis(15));
        for &a in &amps {
            off(&mut s, a, false);
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    let (level, _) = watch(&mut s, amps[0], 500);
    check(&format!("TONE3000 plays after 40 fast off/on rounds ({level:.3})"), level > 0.001);
    for &a in &amps {
        off(&mut s, a, true);
    }
    watch(&mut s, amps[0], 300);
    let (_, cpu_off) = watch(&mut s, amps[0], 1000);
    println!("     load with 3 TONE3000 (no model loaded) on {cpu_on:.3}, off {cpu_off:.3}");
    check("turning them off doesn't raise the load", cpu_off <= cpu_on + 0.01);
    s.stop();
    let warning = s.tick().warning;
    check(&format!("the engine raised no warning ({warning:?})"), warning.is_none());
    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
