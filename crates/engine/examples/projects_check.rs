//! Exercises project alternatives, backups and Revert to, user templates and track
//! alternatives against a running engine (silent ALSA null output), saving and reopening the
//! project along the way. usage: projects_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, Project, TrackKind};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn session() -> Session {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    Session::new(cfg, Vec::new())
}

/// Plays for a moment and returns the peak meter of `track`.
fn level(s: &mut Session, track: u64) -> f32 {
    s.locate(0);
    s.play();
    std::thread::sleep(Duration::from_millis(150));
    s.tick();
    std::thread::sleep(Duration::from_millis(250));
    let t = s.tick();
    s.stop();
    // play() is ignored until the engine has really stopped.
    std::thread::sleep(Duration::from_millis(60));
    t.meters.iter().find(|m| m.0 == track).map(|m| m.1.max(m.2)).unwrap_or(-1.0)
}

fn reopen(dir: &Path) -> Session {
    let mut s = session();
    s.open(dir).expect("reopen");
    s
}

fn tempo_of(dir: &Path, file: &str) -> f64 {
    serde_json::from_slice::<Project>(&std::fs::read(dir.join(file)).unwrap()).map(|p| p.tempo).unwrap_or(-1.0)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let dir = scratch.join("ProjectsCheck.fermata");
    let mut s = session();
    s.dir = dir.clone();
    s.prefs.general.backups = 3;

    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(Path::new(&tone), a, 0).unwrap();
    // The null device has no clock and runs flat out, so loop over the audio.
    s.set_cycle(true, 0, 48000);
    let lv = level(&mut s, a);
    check(&format!("the track plays ({lv:.3})"), lv > 0.01);

    // ---- track alternatives -----------------------------------------------------------------
    s.checkpoint("New Track Alternative", None, false);
    let b = s.new_track_alternative(a, false).unwrap();
    let lv = level(&mut s, a);
    check(&format!("a new track alternative ({b}) is empty and silent ({lv:.4})"), b == "B" && lv < 1e-4);
    s.checkpoint("Switch Track Alternative", None, false);
    s.switch_track_alternative(a, "A").unwrap();
    let lv = level(&mut s, a);
    check(&format!("switching back to A plays its regions ({lv:.3})"), lv > 0.01);
    s.undo();
    let lv = level(&mut s, a);
    check(&format!("undo switches back to B ({lv:.4})"), s.project.track(a).unwrap().alternative == "B" && lv < 1e-4);
    s.undo();
    let none = s.project.track(a).unwrap().alternatives.is_empty();
    let lv = level(&mut s, a);
    check(&format!("undo removes the new alternative ({lv:.3})"), none && lv > 0.01);
    s.redo();
    check("redo brings it back", s.project.track(a).unwrap().alternatives.len() == 1);
    s.switch_track_alternative(a, "A").unwrap();
    s.checkpoint("Duplicate Track Alternative", None, false);
    let c = s.new_track_alternative(a, true).unwrap();
    s.rename_track_alternative(a, &c, "Comp").unwrap();
    let lv = level(&mut s, a);
    check(&format!("a duplicate alternative plays the same regions ({lv:.3})"), lv > 0.01);
    s.save(None).unwrap();
    let mut r = reopen(&dir);
    let t = r.project.track(a).unwrap();
    let mut names: Vec<String> = t.alternatives.iter().map(|x| x.name.clone()).collect();
    names.sort();
    check(&format!("track alternatives survive save and reopen ({names:?}, active {})", t.alternative), names == ["A", "B"] && t.alternative == "Comp" && t.regions.len() == 1);
    r.switch_track_alternative(a, "B").unwrap();
    let lv = level(&mut r, a);
    check(&format!("after reopening, switching still swaps what plays ({lv:.4})"), lv < 1e-4);
    drop(r);

    // ---- backups -----------------------------------------------------------------------------
    for (i, bpm) in [100.0, 101.0, 102.0, 103.0, 104.0].iter().enumerate() {
        s.set_tempo(*bpm);
        s.save(None).unwrap();
        // Distinct modification times, for distinct backup times.
        if i < 4 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let backups = s.backups();
    let tempos: Vec<f64> = backups.iter().map(|b| tempo_of(&dir, &b.file)).collect();
    check(&format!("saves keep backups, pruned to 3, newest first ({tempos:?})"), tempos == [103.0, 102.0, 101.0]);
    check("no project.json.bak while backups are on", !dir.join(daw_engine::session::BACKUP_FILE).exists());
    s.set_tempo(110.0);
    s.dirty = true;
    let oldest = backups.last().unwrap().file.clone();
    s.revert_to_backup(&oldest).unwrap();
    let after: Vec<f64> = s.backups().iter().map(|b| tempo_of(&dir, &b.file)).collect();
    check(&format!("revert loads the backup ({}) and keeps the state before it ({after:?})", s.project.tempo), s.project.tempo == 101.0 && after.contains(&110.0) && !s.dirty);
    check("the reverted version is what's saved", reopen(&dir).project.tempo == 101.0);

    // ---- project alternatives ------------------------------------------------------------------
    let first = s.alternative_name();
    s.new_alternative("Mix 2").unwrap();
    s.set_tempo(90.0);
    s.save(None).unwrap();
    check(&format!("a new alternative copies the project ({:?})", s.alternatives()), s.alternatives() == ["Mix 2".to_string(), first.clone()] && s.project.track(a).is_some());
    s.set_tempo(95.0);
    s.dirty = true;
    s.switch_alternative(&first).unwrap();
    check(&format!("switching loads the other alternative ({})", s.project.tempo), s.alternative_name() == first && s.project.tempo == 101.0);
    let lv = level(&mut s, a);
    check(&format!("the switched-to alternative plays ({lv:.3})"), lv > 0.01);
    let r = reopen(&dir);
    check("reopening opens the active alternative", r.alternative_name() == first && r.alternatives().len() == 2);
    drop(r);
    s.switch_alternative("Mix 2").unwrap();
    check(&format!("switching saved the unsaved changes ({})", s.project.tempo), s.project.tempo == 95.0);
    check("each alternative has its own backups", s.backups().iter().all(|b| b.file.starts_with("Backups/Mix 2/")) && !s.backups().is_empty());
    s.rename_alternative("Mix 2", "Mix B").unwrap();
    s.rename_alternative(&first, "Demo").unwrap();
    let r = reopen(&dir);
    check(&format!("renames are on disk ({:?})", r.alternatives()), r.alternatives() == ["Demo".to_string(), "Mix B".to_string()] && r.alternative_name() == "Mix B");
    drop(r);
    s.delete_alternative("Mix B").unwrap();
    check(&format!("deleting the active alternative switches to the other ({}, {})", s.alternative_name(), s.project.tempo), s.alternative_name() == "Demo" && s.alternatives().len() == 1 && s.project.tempo == 101.0);
    check("its backups go too", !dir.join("Backups").join("Mix B").exists());
    check("the last alternative stays", s.delete_alternative("Demo").is_err());

    // ---- user templates ----------------------------------------------------------------------------
    let templates = scratch.join("templates");
    s.update_track(a, daw_engine::session::TrackPatch { name: Some("Vocals".into()), volume_db: Some(-4.0), ..Default::default() });
    s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.save_template(&templates, "Band", false).unwrap();
    let list = daw_engine::session::list_templates(&templates);
    check(&format!("the template is listed ({} tracks)", list.first().map(|t| t.tracks).unwrap_or(0)), list.len() == 1 && list[0].name == "Band" && list[0].tracks == 2);
    let mut t = session();
    t.new_from_template(&templates, "Band").unwrap();
    let v = t.project.track(a);
    check(
        "a project from the template has its tracks and settings, no regions",
        v.map(|v| v.name == "Vocals" && v.volume_db == -4.0 && v.regions.is_empty() && v.alternatives.is_empty()).unwrap_or(false) && t.project.tracks.len() == 2 && t.project.clips.is_empty() && !t.saved,
    );
    let new_dir = t.save(Some(scratch.join("FromTemplate.fermata"))).unwrap();
    let r = reopen(&new_dir);
    check("it saves and reopens as an ordinary project", r.project.tracks.len() == 2 && r.alternative_name() == "FromTemplate");
    daw_engine::session::rename_template(&templates, "Band", "Trio").unwrap();
    daw_engine::session::delete_template(&templates, "Trio").unwrap();
    check("templates can be renamed and deleted", daw_engine::session::list_templates(&templates).is_empty());

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
