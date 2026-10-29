//! Checks Track Freeze against the engine (silent ALSA null output): a frozen instrument track
//! with a latent plug-in sounds as it did live, its plug-ins are unloaded, edits are refused,
//! and undo, redo, unfreeze and reopening bring the right state back.
//! usage: freeze_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::time::Instant;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{AutoParam, AutoPoint, InputSel, Note, TrackKind};
use daw_engine::session::{MidiContent, TrackPatch};
use daw_engine::Session;

fn check(name: &str, ok: bool) -> bool {
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
    ok
}

/// The full mix of `[0, len)` and how long it took to render.
fn mix(s: &mut Session, len: i64) -> (Vec<Vec<f32>>, f64) {
    let t = Instant::now();
    let a = s.render(0, len, 0, None, &mut |_| {}).unwrap();
    (a, t.elapsed().as_secs_f64())
}

/// The largest sample difference between two renders.
fn diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs())).fold(0.0, f32::max)
}

fn peak(a: &[Vec<f32>]) -> f32 {
    a.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn frozen_file(s: &Session, id: u64) -> Option<PathBuf> {
    let f = s.project.track(id)?.frozen?;
    let c = s.project.clips.iter().find(|c| c.id == f.clip)?;
    let p = Path::new(&c.path);
    Some(if p.is_absolute() { p.to_path_buf() } else { s.dir.join(p) })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let plugins = daw_engine::plugin_db::load_cache().unwrap_or_default();
    let Some(maxim) = plugins.iter().find(|d| d.name == "ZaMaximX2").cloned() else {
        println!("FAIL ZaMaximX2 isn't in the plug-in cache; scan plug-ins first");
        std::process::exit(1);
    };
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, plugins);
    s.dir = scratch.join("FreezeCheck.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();
    let bar = s.project.samples_per_bar().round() as i64;
    let beat = s.project.samples_per_beat().round() as i64;
    let len = 3 * bar;
    let mut ok = true;

    // An instrument track (built-in synth) through the maximizer, with automation on it, next
    // to a plain audio track.
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let audio = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(Path::new(&tone), audio, 0).unwrap();
    let r = s.create_midi_region(inst, 0, 2 * bar).unwrap();
    let notes: Vec<Note> = (1..8).flat_map(|i| [60u8, 64, 67].map(|p| Note { start: i * beat, length: beat / 2, pitch: p, velocity: 110, muted: false })).collect();
    s.set_midi_content(vec![MidiContent { region: r, notes, cc: vec![], start: None, length: None }]);
    let uid = s.set_plugin(inst, 0, &maxim.id).unwrap();
    let (plain, _) = mix(&mut s, len);
    let gain = s.plugin_params(uid).into_iter().find(|p| p.name == "Input Gain").expect("ZaMaximX2 has an Input Gain");
    let pts = vec![AutoPoint { pos: 0, value: gain.default as f32 }, AutoPoint { pos: 2 * bar, value: 0.0 }];
    s.set_automation_lane(inst, AutoParam::Plugin { uid, param: gain.id, name: gain.name.clone() }, pts);
    let (live, live_secs) = mix(&mut s, len);
    ok &= check(&format!("the maximizer's Input Gain automation changes the sound (max diff {:.3})", diff(&plain, &live)), diff(&plain, &live) > 0.01);
    let instances = s.plugin_instances();
    ok &= check(&format!("the live mix sounds (peak {:.3}, {instances} plug-in loaded)", peak(&live)), peak(&live) > 0.05 && instances == 1);

    // Freeze.
    s.checkpoint("Freeze Track", None, true);
    s.freeze_tracks(&[inst], &mut |_| {}).unwrap();
    let file = frozen_file(&s, inst);
    ok &= check(&format!("the track is frozen into {file:?}"), file.as_ref().map(|f| f.exists()).unwrap_or(false));
    ok &= check(&format!("its plug-ins are unloaded ({} loaded)", s.plugin_instances()), s.plugin_instances() == 0);
    let (frozen, frozen_secs) = mix(&mut s, len);
    let d = diff(&live, &frozen);
    ok &= check(&format!("frozen plays the same as live, plug-in automation baked in (max diff {d:.2e})"), d < 1e-3);
    println!("render time: live {:.1} ms, frozen {:.1} ms", live_secs * 1e3, frozen_secs * 1e3);
    ok &= check("the total latency went with the plug-ins", s.pdc_total() == 0);

    // Edits are refused, and leave no undo step behind.
    let start = s.project.track(inst).unwrap().regions[0].start;
    s.checkpoint("Move Region", None, false);
    s.move_region(r, start + beat, None);
    let refused = s.frozen_guard();
    let now = s.project.track(inst).unwrap().regions[0].start;
    ok &= check(&format!("moving a frozen region is refused ({:?})", refused.as_ref().err().map(|e| e.to_string())), refused.is_err() && now == start && s.undo_label().as_deref() == Some("Freeze Track"));
    ok &= check("loading a plug-in onto a frozen track is refused", s.set_plugin(inst, 1, &maxim.id).is_err() && s.plugin_instances() == 0);
    s.set_bypass(inst, 0, true);
    s.remove_plugin(inst, 0);
    let slot = s.project.track(inst).unwrap().inserts[0].clone();
    ok &= check("its plug-ins can't be bypassed or removed", slot.map(|p| p.uid == uid && !p.bypass).unwrap_or(false));
    s.update_track(inst, TrackPatch { armed: Some(true), ..Default::default() });
    ok &= check("a frozen track can't be record-enabled", !s.project.track(inst).unwrap().armed);

    // The fader stays live.
    s.update_track(inst, TrackPatch { volume_db: Some(-6.0), ..Default::default() });
    s.update_track(audio, TrackPatch { mute: Some(true), ..Default::default() });
    let (quiet, _) = mix(&mut s, len);
    s.update_track(inst, TrackPatch { volume_db: Some(0.0), ..Default::default() });
    let (loud, _) = mix(&mut s, len);
    s.update_track(audio, TrackPatch { mute: Some(false), ..Default::default() });
    let db = 20.0 * (peak(&quiet) / peak(&loud)).log10();
    ok &= check(&format!("the fader still works on a frozen track ({db:+.2} dB)"), (db + 6.0).abs() < 0.1);

    // Undo and redo.
    s.undo();
    ok &= check(&format!("undo unfreezes and reloads the plug-in ({} loaded)", s.plugin_instances()), s.project.track(inst).unwrap().frozen.is_none() && s.plugin_instances() == 1);
    let (back, _) = mix(&mut s, len);
    let d = diff(&live, &back);
    ok &= check(&format!("after undo it plays live again (max diff {d:.2e}, latency {})", s.pdc_total()), d < 1e-3 && s.pdc_total() > 0);
    s.redo();
    let (again, _) = mix(&mut s, len);
    ok &= check(&format!("redo freezes it again (max diff {:.2e})", diff(&frozen, &again)), s.project.track(inst).unwrap().frozen.is_some() && s.plugin_instances() == 0 && diff(&frozen, &again) < 1e-6);

    // Unfreeze, and undo that.
    let file = frozen_file(&s, inst).unwrap();
    s.checkpoint("Unfreeze Track", None, true);
    s.unfreeze_track(inst).unwrap();
    let (unfrozen, _) = mix(&mut s, len);
    let d = diff(&live, &unfrozen);
    ok &= check(&format!("unfreeze deletes the file, reloads the plug-in and plays live (max diff {d:.2e})"), !file.exists() && s.plugin_instances() == 1 && d < 1e-3);
    s.undo();
    ok &= check("undoing the unfreeze brings the freeze file back", file.exists() && s.plugin_instances() == 0 && s.project.track(inst).unwrap().frozen.is_some());

    // Save and reopen a frozen project.
    let dir = s.save(Some(scratch.join("FreezeCheckSaved.fermata"))).unwrap();
    s.new_project();
    s.open(&dir).unwrap();
    let (reopened, _) = mix(&mut s, len);
    ok &= check(
        &format!("a reopened project is still frozen (max diff {:.2e}, {} loaded)", diff(&frozen, &reopened), s.plugin_instances()),
        s.project.track(inst).map(|t| t.frozen.is_some()).unwrap_or(false) && s.plugin_instances() == 0 && diff(&frozen, &reopened) < 1e-6,
    );
    s.checkpoint("Unfreeze Track", None, true);
    s.unfreeze_track(inst).unwrap();
    let (reloaded, _) = mix(&mut s, len);
    let d = diff(&live, &reloaded);
    ok &= check(&format!("its saved plug-in settings come back on unfreeze (max diff {d:.2e})"), s.plugin_instances() == 1 && d < 1e-3);

    // An audio track freezes too; an aux doesn't.
    s.checkpoint("Freeze Track", None, true);
    let res = s.freeze_tracks(&[audio], &mut |_| {});
    let (af, _) = mix(&mut s, len);
    ok &= check(&format!("an audio track freezes and sounds the same (max diff {:.2e})", diff(&reloaded, &af)), res.is_ok() && diff(&reloaded, &af) < 1e-3);
    let aux = s.create_aux(None).unwrap();
    ok &= check("an aux can't be frozen", s.freeze_tracks(&[aux], &mut |_| {}).is_err());

    println!("{}", if ok { "ALL PASS" } else { "SOME FAILED" });
    if !ok {
        std::process::exit(1);
    }
}
