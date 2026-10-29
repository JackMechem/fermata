//! Exercises undo/redo through a sequence of edits (silent ALSA null output).
use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;

fn state(s: &Session) -> String {
    let t = s.project.tracks.iter().map(|t| {
        let ins: Vec<_> = t.inserts.iter().flatten().map(|p| p.name.clone()).collect();
        let regs: Vec<_> = t.regions.iter().map(|r| format!("{}@{:.1}s", r.name, r.start as f64 / 48000.0)).collect();
        format!("[{} vol={:.1} fx={:?} regions={:?}]", t.name, t.volume_db, ins, regs)
    });
    t.collect::<Vec<_>>().join(" ")
}

fn main() {
    let tone = std::env::args().nth(1).expect("audio file");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let plugins = daw_engine::plugin_db::load_cache().unwrap_or_default();
    let comp = plugins.iter().find(|p| p.name == "ZamCompX2").map(|p| p.id.clone());
    let mut s = Session::new(cfg, plugins);

    s.checkpoint("New Track", None, false);
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.checkpoint("Import Audio", None, false);
    s.import_audio(std::path::Path::new(&tone), t, 0).unwrap();
    if let Some(c) = &comp {
        s.checkpoint("Insert Plug-in", None, true);
        s.set_plugin(t, 0, c).unwrap();
    }
    s.checkpoint("Move Region", None, false);
    let rid = s.project.track(t).unwrap().regions[0].id;
    s.move_region(rid, 96000, None);
    for db in [-2.0, -4.0, -6.0] {
        s.checkpoint("Volume", Some(format!("track:{t}:volume")), false);
        s.update_track(t, TrackPatch { volume_db: Some(db), ..Default::default() });
    }
    s.checkpoint("Delete Track", None, true);
    s.delete_track(t);

    println!("after edits : {}", state(&s));
    while let Some(label) = s.undo() {
        println!("undo {label:<15}: {}", state(&s));
    }
    while let Some(label) = s.redo() {
        println!("redo {label:<15}: {}", state(&s));
    }
}
