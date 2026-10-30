//! Headless smoke test: lists devices, records two seconds from the default input through a
//! plug-in, and reports the resulting region.
use daw_engine::audio_io::{list_devices, AudioConfig};
use daw_engine::model::{InputSel, RegionContent, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;
use std::time::Duration;

fn main() {
    env_logger_init();
    let list = list_devices(None);
    println!("host {} (available: {:?})", list.host, list.hosts);
    for d in &list.inputs {
        println!("  in : {:<50} ch={} rates={:?} default={} id={}", d.name, d.channels, d.sample_rates, d.is_default, d.id);
    }
    for d in &list.outputs {
        println!("  out: {:<50} ch={} rates={:?} default={}", d.name, d.channels, d.sample_rates, d.is_default);
    }
    let plugins = vst3_host::scan::find_bundles(&vst3_host::scan::default_search_paths())
        .iter()
        .filter(|b| b.to_string_lossy().contains("ZamVerb"))
        .flat_map(|b| vst3_host::scan::scan_bundle(b).unwrap_or_default())
        .collect::<Vec<_>>();
    let cfg = AudioConfig { input: std::env::var("IN").ok(), output: std::env::var("OUT").ok(), host: std::env::var("HOST").ok(), ..Default::default() };
    let mut s = Session::new(cfg, plugins.clone());
    println!("audio: {:?}", s.audio_status);
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.update_track(t, TrackPatch { armed: Some(true), ..Default::default() });
    if let Some(p) = plugins.first() {
        s.set_plugin(t, 0, &p.id).unwrap();
        println!("inserted {}", p.name);
    }
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.set_metronome(true);
    s.record().unwrap();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(100));
        s.idle();
        let tick = s.tick();
        if tick.previews.iter().any(|p| !p.peaks.is_empty()) {
            print!(".");
        }
    }
    println!();
    let tick = s.tick();
    println!("pos={} playing={} rec={} cpu={:.3} inputs={:?}", tick.pos, tick.playing, tick.recording, tick.cpu, tick.inputs);
    println!("latency: {:?}", s.latency());
    s.stop();
    let tr = s.project.track(t).unwrap();
    for r in &tr.regions {
        if let RegionContent::Audio { clip, .. } = r.content {
            let max = s.clip_peak(clip).unwrap();
            println!("region '{}' start={} len={} ({:.2}s) peak={max:.4}", r.name, r.start, r.length, r.length as f64 / s.sample_rate());
        }
    }
    println!("inst regions: {}", s.project.track(inst).unwrap().regions.len());
    let saved = s.save(None).unwrap();
    println!("saved to {}", saved.display());
    s.open(&saved).unwrap();
    println!("reopened: {} tracks, clips={}", s.project.tracks.len(), s.project.clips.len());
}

fn env_logger_init() {}
