//! Side-chain inputs of real VST3 plug-ins (LSP's Sidechain Compressor and Gate), on the silent
//! ALSA null device, measured from offline renders: a loud key changes the output and a silent
//! one doesn't, with no key source the key track makes no difference. Then what has to keep
//! working around it. Skips when LSP isn't installed.
//! usage: vst_sidechain_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, TrackKind, MASTER_ID};
use daw_engine::session::{BipOptions, TrackPatch};
use daw_engine::Session;
use vst3_host::PluginDescriptor;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

const LEN: i64 = 96_000;

fn render(s: &mut Session) -> Vec<Vec<f32>> {
    s.render(0, LEN, 0, None, &mut |_| {}).unwrap()
}

/// The level of the steady part of a render, in dB.
fn level(r: &[Vec<f32>]) -> f32 {
    let tail = &r[0][48_000..90_000];
    let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
    20.0 * rms.max(1e-9).log10()
}

fn level_of(s: &mut Session) -> f32 {
    level(&render(s))
}

fn tone_track(s: &mut Session, tone: &Path) -> Id {
    let t = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(tone, t, 0).unwrap();
    t
}

fn mute(s: &mut Session, id: Id, on: bool) {
    s.update_track(id, TrackPatch { mute: Some(on), ..Default::default() });
}

/// Finds a parameter by name and sets it, by normalized value.
fn set_param(s: &mut Session, uid: Id, name: &str, value: f64) {
    let params = s.plugin_params(uid);
    let p = params.iter().find(|p| p.name.to_lowercase().contains(&name.to_lowercase())).unwrap_or_else(|| panic!("no parameter {name}: {:?}", params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>()));
    s.edit_vst_param(uid, p.id, value);
}

fn lsp_plugins() -> Vec<PluginDescriptor> {
    let bundles = vst3_host::scan::find_bundles(&vst3_host::scan::default_search_paths());
    bundles.iter().filter(|b| b.to_string_lossy().contains("lsp-plugins")).flat_map(|b| vst3_host::scan::scan_bundle(b).unwrap_or_default()).filter(|d| d.name.contains("Compressor") || d.name.contains("Gate")).collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = PathBuf::from(args.next().expect("audio file"));
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let plugins = lsp_plugins();
    let find = |name: &str| plugins.iter().find(|d| d.name == name).cloned();
    let (Some(comp), Some(gate), Some(mono)) = (find("Sidechain Compressor Stereo"), find("Sidechain Gate Stereo"), find("Sidechain Compressor Mono")) else {
        println!("SKIP LSP's Sidechain Compressor and Gate aren't installed");
        return;
    };
    for d in [&comp, &gate, &mono] {
        let (_, c) = vst3_host::instantiate(d, 48000.0, 1024).unwrap();
        for b in c.audio_buses() {
            println!("{}: {} \"{}\": {} ch{}", d.name, if b.input { "in " } else { "out" }, b.name, b.channels, if b.aux { ", aux" } else { "" });
        }
    }
    gate_click_and_state(&comp);

    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg.clone(), plugins.clone());
    s.dir = scratch.join("VstSidechain.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();
    // The source comes after the plug-in's track, so the key has to reorder them. It's heard only
    // through the key: taken before its fader, which is all the way down.
    let main = tone_track(&mut s, &tone);
    let key = tone_track(&mut s, &tone);
    s.update_track(key, TrackPatch { volume_db: Some(-96.0), ..Default::default() });
    let dry = level_of(&mut s);

    // ---- the compressor: a loud key compresses, a silent one doesn't, no key changes nothing --
    let uid = s.set_plugin(main, 0, &comp.id).unwrap();
    // "Sidechain type" 2 of 0..3 is External, LSP's default, which is what the aux input feeds.
    set_param(&mut s, uid, "Sidechain type", 2.0 / 3.0);
    set_param(&mut s, uid, "Attack threshold", 0.1);
    set_param(&mut s, uid, "Ratio", 1.0);
    let inputs = s.vst_key_inputs();
    check("the plug-in reports a side-chain input", inputs.contains(&uid));
    check("with no key source its aux input is inactive", s.vst_key_active(uid) == Some(false));
    let none_loud = level_of(&mut s);
    mute(&mut s, key, true);
    let none_silent = level_of(&mut s);
    mute(&mut s, key, false);
    check(&format!("no source: the other track makes no difference ({none_loud:.2} and {none_silent:.2} dB, dry {dry:.2} dB)"), (none_loud - none_silent).abs() < 0.01 && (none_loud - dry).abs() < 0.5);
    s.set_sidechain(main, 0, Some(key)).unwrap();
    s.set_sidechain_pre(main, 0, true).unwrap();
    check("choosing a source activates it", s.vst_key_active(uid) == Some(true));
    let order = s.project.process_order();
    check("the source plays first", order.iter().position(|&i| i == key) < order.iter().position(|&i| i == main));
    let loud = level_of(&mut s);
    mute(&mut s, key, true);
    let silent = level_of(&mut s);
    mute(&mut s, key, false);
    check(&format!("Sidechain Compressor: a loud key compresses ({loud:.2} dB), a silent one doesn't ({silent:.2} dB)"), loud < silent - 12.0 && (silent - dry).abs() < 0.5);

    // ---- the same source through the fader, after it ------------------------------------------
    s.set_sidechain_pre(main, 0, false).unwrap();
    let post = level_of(&mut s);
    check(&format!("post-fader the key's fader counts: at -96 dB it doesn't compress ({post:.2} dB)"), (post - dry).abs() < 0.5);
    s.set_sidechain_pre(main, 0, true).unwrap();

    // ---- plug-ins off, on, bypassed ------------------------------------------------------------
    off(&mut s, key, true);
    let off_key = level_of(&mut s);
    check(&format!("a key from an Off track is silence ({off_key:.2} dB)"), (off_key - dry).abs() < 0.5);
    off(&mut s, key, false);
    off(&mut s, main, true);
    let off_main = level_of(&mut s);
    off(&mut s, main, false);
    check(&format!("an Off track with a keyed plug-in is silent ({off_main:.2} dB) and back on it's keyed ({:.2} dB)", level_of(&mut s)), off_main < -100.0 && (level_of(&mut s) - loud).abs() < 0.5);
    off(&mut s, main, true);
    s.set_sidechain(main, 0, None).unwrap();
    s.set_sidechain(main, 0, Some(key)).unwrap();
    off(&mut s, main, false);
    check(&format!("switched while its track is off it still works ({:.2} dB)", level_of(&mut s)), (level_of(&mut s) - loud).abs() < 0.5);
    s.set_bypass(main, 0, true);
    let t = Instant::now();
    s.set_sidechain(main, 0, None).unwrap();
    s.set_sidechain(main, 0, Some(key)).unwrap();
    let ms = t.elapsed().as_millis();
    s.set_bypass(main, 0, false);
    check(&format!("switched while bypassed, without waiting for a fade ({ms} ms for two switches)"), ms < 100 && (level_of(&mut s) - loud).abs() < 0.5);

    // ---- feedback, a plug-in without the input --------------------------------------------------
    let back = s.set_plugin(key, 0, &comp.id).unwrap();
    check("a side-chain loop is refused", s.set_sidechain(key, 0, Some(main)).is_err() && s.vst_key_active(back) == Some(false));
    s.remove_plugin(key, 0);
    let plain = plugins.iter().find(|d| d.name == "Compressor Stereo").cloned();
    if let Some(plain) = plain {
        let u = s.set_plugin(key, 0, &plain.id).unwrap();
        println!("{}: inputs {}", plain.name, s.vst_key_inputs().contains(&u));
        check("a plug-in with no aux input takes no key", !s.vst_key_inputs().contains(&u) && s.set_sidechain(key, 0, Some(main)).is_err());
        s.remove_plugin(key, 0);
    }

    // ---- switching while the engine plays ------------------------------------------------------
    s.play();
    std::thread::sleep(Duration::from_millis(200));
    let t = Instant::now();
    s.set_sidechain(main, 0, None).unwrap();
    let ms = t.elapsed().as_millis();
    check(&format!("while playing the aux input switches off ({ms} ms)"), s.vst_key_active(uid) == Some(false) && ms < 300);
    let t = Instant::now();
    s.set_sidechain(main, 0, Some(key)).unwrap();
    let ms = t.elapsed().as_millis();
    check(&format!("and on ({ms} ms)"), s.vst_key_active(uid) == Some(true) && ms < 300);
    std::thread::sleep(Duration::from_millis(100));
    s.stop();
    check("the engine reported no trouble", s.tick().warning.is_none());
    check(&format!("it plays keyed after that ({:.2} dB)", level_of(&mut s)), (level_of(&mut s) - loud).abs() < 0.5);

    // ---- undo, a deleted source ------------------------------------------------------------------
    s.checkpoint("Side Chain", None, false);
    s.set_sidechain(main, 0, None).unwrap();
    let unkeyed = s.vst_key_active(uid) == Some(false);
    s.undo();
    let back_on = s.vst_key_active(uid) == Some(true) && s.project.track(main).unwrap().inserts[0].as_ref().unwrap().sidechain == Some(key);
    check(&format!("undo brings the key back ({:.2} dB)", level_of(&mut s)), unkeyed && back_on && (level_of(&mut s) - loud).abs() < 0.5);
    s.redo();
    check("redo takes it off", s.vst_key_active(uid) == Some(false));
    s.undo();
    s.checkpoint("Delete", None, true);
    s.delete_track(key);
    let gone = s.project.track(main).unwrap().inserts[0].as_ref().unwrap().sidechain.is_none();
    check(&format!("deleting the source takes the key off ({:.2} dB)", level_of(&mut s)), gone && s.vst_key_active(uid) == Some(false) && (level_of(&mut s) - dry).abs() < 0.5);
    s.undo();
    let active = s.vst_key_active(uid) == Some(true) && s.project.track(main).unwrap().inserts[0].as_ref().is_some_and(|x| x.sidechain == Some(key));
    check(&format!("undo brings source and key back ({:.2} dB)", level_of(&mut s)), active && (level_of(&mut s) - loud).abs() < 0.5);

    // ---- freeze and bounce -----------------------------------------------------------------------
    s.freeze_tracks(&[main], &mut |_| {}).unwrap();
    let frozen = level_of(&mut s);
    check(&format!("a frozen track keeps the keyed sound ({frozen:.2} dB)"), (frozen - loud).abs() < 0.5);
    s.unfreeze_track(main).unwrap();
    let uid_now = s.project.track(main).unwrap().inserts[0].as_ref().unwrap().uid;
    check(&format!("unfrozen it's keyed again ({:.2} dB)", level_of(&mut s)), s.vst_key_active(uid_now) == Some(true) && (level_of(&mut s) - loud).abs() < 0.5);
    let before: Vec<Id> = s.project.tracks.iter().map(|t| t.id).collect();
    s.checkpoint("Bounce", None, true);
    let o = BipOptions { effects: true, fader: true, ..BipOptions::default() };
    s.bounce_in_place(&[main], &[], &o, &mut |_| {}).unwrap();
    let new: Vec<Id> = s.project.tracks.iter().map(|t| t.id).filter(|id| !before.contains(id)).collect();
    for &id in &before {
        mute(&mut s, id, true);
    }
    let bounced = level_of(&mut s);
    check(&format!("Bounce in Place hears the key ({bounced:.2} dB, {} new track)", new.len()), new.len() == 1 && (bounced - loud).abs() < 0.5);
    s.undo();
    mute(&mut s, main, false);
    mute(&mut s, key, false);
    let uid_now = s.project.track(main).unwrap().inserts[0].as_ref().unwrap().uid;
    check(&format!("undone, it plays keyed ({:.2} dB)", level_of(&mut s)), s.vst_key_active(uid_now) == Some(true) && (level_of(&mut s) - loud).abs() < 0.5);

    // ---- duplicating, channel strip settings -----------------------------------------------------
    mute(&mut s, main, true);
    let copy = s.duplicate_tracks(&[main], true)[0];
    mute(&mut s, copy, false);
    let dup = s.project.track(copy).unwrap().inserts[0].clone().unwrap();
    let dup_level = level_of(&mut s);
    check(&format!("a duplicate is keyed by the same source, with the same settings ({dup_level:.2} dB)"), dup.sidechain == Some(key) && dup.uid != uid_now && s.vst_key_active(dup.uid) == Some(true) && (dup_level - loud).abs() < 0.5);
    s.delete_track(copy);
    mute(&mut s, main, false);
    let strip = s.copy_strip(main).unwrap();
    let other = tone_track(&mut s, &tone);
    mute(&mut s, main, true);
    s.paste_strip(&[other], &strip).unwrap();
    let pasted = s.project.track(other).unwrap().inserts[0].clone().unwrap();
    let strip_level = level_of(&mut s);
    check(&format!("a pasted channel strip keeps the key ({strip_level:.2} dB)"), pasted.sidechain == Some(key) && s.vst_key_active(pasted.uid) == Some(true) && (strip_level - loud).abs() < 0.5);
    s.paste_strip(&[key], &strip).unwrap();
    let own = s.project.track(key).unwrap().inserts[0].clone().unwrap();
    check("pasted onto the source itself the key stays behind", own.sidechain.is_none() && s.vst_key_active(own.uid) == Some(false));
    s.remove_plugin(key, 0);
    s.delete_track(other);
    mute(&mut s, main, false);

    // ---- Stereo Out -------------------------------------------------------------------------------
    s.remove_plugin(main, 0);
    let m = s.set_plugin(MASTER_ID, 0, &comp.id).unwrap();
    set_param(&mut s, m, "Sidechain type", 2.0 / 3.0);
    set_param(&mut s, m, "Attack threshold", 0.1);
    set_param(&mut s, m, "Ratio", 1.0);
    s.set_sidechain(MASTER_ID, 0, Some(key)).unwrap();
    s.set_sidechain_pre(MASTER_ID, 0, true).unwrap();
    let master = level_of(&mut s);
    check(&format!("Stereo Out's plug-in can be keyed ({master:.2} dB)"), master < dry - 12.0);
    s.remove_plugin(MASTER_ID, 0);

    // ---- the gate: a loud key opens it, a silent one closes it ------------------------------------
    let g = s.set_plugin(main, 0, &gate.id).unwrap();
    set_param(&mut s, g, "Curve threshold", 0.5);
    set_param(&mut s, g, "Hysteresis threshold", 0.5);
    s.set_sidechain(main, 0, Some(key)).unwrap();
    s.set_sidechain_pre(main, 0, true).unwrap();
    let open = level_of(&mut s);
    mute(&mut s, key, true);
    let closed = level_of(&mut s);
    mute(&mut s, key, false);
    check(&format!("Sidechain Gate: a loud key opens it ({open:.2} dB), a silent one closes it ({closed:.2} dB)"), open > closed + 6.0 && (open - dry).abs() < 3.0);
    s.remove_plugin(main, 0);

    // ---- a mono aux input gets a mix-down ---------------------------------------------------------
    let u = s.set_plugin(main, 0, &mono.id).unwrap();
    set_param(&mut s, u, "Sidechain type", 2.0 / 3.0);
    set_param(&mut s, u, "Attack threshold", 0.1);
    set_param(&mut s, u, "Ratio", 1.0);
    s.set_sidechain(main, 0, Some(key)).unwrap();
    // Post-fader and panned hard right, so the left channel alone has nothing to key from.
    s.update_track(key, TrackPatch { volume_db: Some(0.0), pan: Some(1.0), ..Default::default() });
    let right_only = level_of(&mut s);
    mute(&mut s, key, true);
    let mono_silent = level_of(&mut s);
    mute(&mut s, key, false);
    check(&format!("a mono aux hears both of the key's channels ({right_only:.2} dB, silent key {mono_silent:.2} dB)"), right_only < mono_silent - 6.0);
    s.update_track(key, TrackPatch { volume_db: Some(-96.0), pan: Some(0.0), ..Default::default() });
    s.remove_plugin(main, 0);

    // ---- saving and reopening, a missing plug-in, an old project ----------------------------------
    let uid = s.set_plugin(main, 0, &comp.id).unwrap();
    set_param(&mut s, uid, "Sidechain type", 2.0 / 3.0);
    set_param(&mut s, uid, "Attack threshold", 0.1);
    set_param(&mut s, uid, "Ratio", 1.0);
    s.set_sidechain(main, 0, Some(key)).unwrap();
    s.set_sidechain_pre(main, 0, true).unwrap();
    let saved_level = level_of(&mut s);

    // A keyed plug-in's latency counts like any other, and the key's switch doesn't lose it.
    set_param(&mut s, uid, "Sidechain lookahead", 1.0);
    render(&mut s);
    s.set_sidechain(main, 0, None).unwrap();
    let unkeyed = s.pdc_total();
    s.set_sidechain(main, 0, Some(key)).unwrap();
    check(&format!("a keyed plug-in's latency reaches delay compensation ({} frames, {unkeyed} without the key)", s.pdc_total()), unkeyed > 0 && s.pdc_total() == unkeyed);
    set_param(&mut s, uid, "Sidechain lookahead", 0.0);
    render(&mut s);
    s.set_sidechain(main, 0, None).unwrap();
    s.set_sidechain(main, 0, Some(key)).unwrap();
    let dir = s.save(Some(scratch.join("Saved.fermata"))).unwrap();
    s.open(&dir).unwrap();
    let slot = s.project.track(main).unwrap().inserts[0].clone().unwrap();
    let reopened = level_of(&mut s);
    check(&format!("reopened it's keyed, plug-in settings included ({saved_level:.2} and {reopened:.2} dB)"), slot.sidechain == Some(key) && slot.sidechain_pre && s.vst_key_active(slot.uid) == Some(true) && (reopened - saved_level).abs() < 0.1);

    // Without the plug-in the key stays in the project; only clearing it is possible.
    let mut t = Session::new(cfg.clone(), Vec::new());
    t.open(&dir).unwrap();
    let held = t.project.track(main).unwrap().inserts[0].clone().unwrap();
    check("a missing plug-in keeps its key source", held.sidechain == Some(key) && t.missing_plugins().iter().any(|m| m.uid == held.uid) && t.vst_key_inputs().is_empty());
    check("but can't be given another", t.set_sidechain(main, 0, Some(key)).is_err());
    let moved = t.save(Some(scratch.join("Moved.fermata"))).unwrap();
    let json = std::fs::read_to_string(moved.join("project.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let kept = v["tracks"].as_array().unwrap().iter().flat_map(|t| t["inserts"].as_array().unwrap().iter()).any(|i| i["sidechain"].as_u64() == Some(key as u64));
    check("and saving there keeps it for the computer that has the plug-in", kept);
    drop(t);

    // An old project: the same one with the fields taken out plays with the aux input inactive.
    let mut v = v;
    for t in v["tracks"].as_array_mut().unwrap() {
        for i in t["inserts"].as_array_mut().unwrap() {
            if let Some(o) = i.as_object_mut() {
                o.remove("sidechain");
                o.remove("sidechain_pre");
            }
        }
    }
    std::fs::write(moved.join("project.json"), serde_json::to_string(&v).unwrap()).unwrap();
    s.open(&moved).unwrap();
    let slot = s.project.track(main).unwrap().inserts[0].clone().unwrap();
    let old = level_of(&mut s);
    check(&format!("a project from before side-chains loads with the aux input inactive ({old:.2} dB)"), slot.sidechain.is_none() && s.vst_key_active(slot.uid) == Some(false) && (old - dry).abs() < 0.5);

    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}

/// Switching the aux input while a steady signal plays, on the processor directly: no click, no
/// change to the plug-in's settings.
fn gate_click_and_state(comp: &PluginDescriptor) {
    const BLOCK: usize = 256;
    let (mut p, mut c) = vst3_host::instantiate(comp, 48000.0, BLOCK).unwrap();
    let id = |name: &str| c.parameters().iter().find(|x| x.name == name).map(|x| x.id).unwrap();
    let (ty, th, ra) = (id("Sidechain type"), id("Attack threshold"), id("Ratio"));
    c.edit_param(ty, 2.0 / 3.0);
    c.edit_param(th, 0.1);
    c.edit_param(ra, 1.0);
    let t = vst3_host::TransportInfo { sample_rate: 48000.0, tempo: 120.0, sig_num: 4, sig_den: 4, ..Default::default() };
    let mut phase = 0.0f32;
    let mut last = 0.0f32;
    let mut run = |p: &mut vst3_host::PluginProcessor, key: bool| {
        let mut l = [0.0f32; BLOCK];
        for x in l.iter_mut() {
            *x = 0.3 * phase.sin();
            phase += 0.0576;
        }
        let k = l;
        let mut r = l;
        p.process_keyed(&mut l, &mut r, key.then_some((&k[..], &k[..])), &[], &t);
        let mut step = 0.0f32;
        for x in l {
            step = step.max((x - last).abs());
            last = x;
        }
        (step, l.iter().fold(0.0f32, |m, x| m.max(x.abs())))
    };
    for _ in 0..200 {
        run(&mut p, false);
    }
    let own = (0..50).map(|_| run(&mut p, false).0).fold(0.0, f32::max);
    let before = (c.param_value(th), c.param_value(ra), c.param_value(ty));
    let mut worst = 0.0f32;
    p.close_gate();
    let mut blocks = 0;
    while !c.parked() && blocks < 100 {
        worst = worst.max(run(&mut p, false).0);
        blocks += 1;
    }
    check(&format!("the plug-in stops after a fade ({blocks} blocks)"), c.parked());
    // A block while it's stopped passes the dry signal, and the settings are safe to touch.
    worst = worst.max(run(&mut p, false).0);
    c.set_key_active(true).unwrap();
    let settle = c.latency();
    p.open_gate(settle);
    let mut peak = 0.0f32;
    for _ in 0..200 {
        let (step, pk) = run(&mut p, true);
        worst = worst.max(step);
        peak = pk;
    }
    check(&format!("switching on, the largest step is {worst:.4} (the signal's own {own:.4})"), worst < 1.5 * own);
    check(&format!("the key takes effect after the switch (peak {peak:.3} of 0.3)"), peak < 0.1);
    check("its settings survive setActive", before == (c.param_value(th), c.param_value(ra), c.param_value(ty)));
    let mut worst = 0.0f32;
    p.close_gate();
    while !c.parked() {
        worst = worst.max(run(&mut p, true).0);
    }
    c.set_key_active(false).unwrap();
    p.open_gate(c.latency());
    for _ in 0..200 {
        worst = worst.max(run(&mut p, false).0);
    }
    check(&format!("switching off, the largest step is {worst:.4}"), worst < 1.5 * own);
}

fn off(s: &mut Session, id: Id, v: bool) {
    s.update_track(id, TrackPatch { off: Some(v), ..Default::default() });
}
