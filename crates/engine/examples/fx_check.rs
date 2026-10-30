//! Checks every built-in effect in the registry against the engine (silent ALSA null output):
//! it loads in an insert and processes audio, its parameters set, save, reload, undo and
//! duplicate, automation drives it, its latency reaches delay compensation, side-chains key it
//! from another track (in live renders and isolated ones), its meters reach the tick, and
//! freezing bakes it in. New registry entries are covered without changes here.
//! usage: fx_check <audio file> <scratch dir>
use std::path::PathBuf;
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::fx::{self, EffectInfo, ParamKind, ParamSpec};
use daw_engine::model::{AutoParam, AutoPoint, Id, InputSel, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) -> bool {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
    ok
}

/// Renders `[0, len)` of the full mix, or of one track through its strip.
fn render(s: &mut Session, len: i64, track: Option<Id>) -> Vec<Vec<f32>> {
    let iso = track.map(|t| s.isolate_track(t, true, true));
    s.render(0, len, 0, iso, &mut |_| {}).unwrap()
}

fn peak(a: &[Vec<f32>]) -> f32 {
    a.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

fn diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs())).fold(0.0, f32::max)
}

/// Like `diff`, after the first 0.1 s: effects glide to new settings, and envelopes settle.
fn diff_settled(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y).skip(4800).map(|(p, q)| (p - q).abs())).fold(0.0, f32::max)
}

fn finite(a: &[Vec<f32>]) -> bool {
    a.iter().flatten().all(|s| s.is_finite())
}

/// Lets the audio thread apply what was sent, then runs the main-thread housekeeping.
fn settle(s: &mut Session) {
    std::thread::sleep(Duration::from_millis(40));
    s.idle();
}

fn uid_in(s: &Session, track: Id, slot: usize) -> Option<Id> {
    s.project.track(track)?.inserts.get(slot)?.as_ref().map(|p| p.uid)
}

fn value(s: &Session, uid: Id, id: u32) -> f32 {
    s.builtin_params(uid).iter().find(|p| p.0 == id).map(|p| p.1).unwrap_or(f32::NAN)
}

/// A value well away from the default, inside the range.
fn far(p: &ParamSpec) -> f32 {
    match p.kind {
        ParamKind::Toggle => 1.0 - p.default.round(),
        ParamKind::Choice(labels) => {
            if p.default.round() as usize + 1 < labels.len() {
                p.default.round() + 1.0
            } else {
                0.0
            }
        }
        ParamKind::Continuous => {
            let (lo, hi) = (p.min, p.max);
            if (p.default - lo) < (hi - p.default) {
                hi - 0.1 * (hi - lo)
            } else {
                lo + 0.1 * (hi - lo)
            }
        }
    }
}

/// The normalized value automation would use for `far(p)`.
fn far_normalized(p: &ParamSpec) -> f32 {
    let v = far(p);
    match p.kind {
        ParamKind::Choice(labels) => v / (labels.len().max(2) - 1) as f32,
        ParamKind::Continuous if p.log && p.min > 0.0 => (v / p.min).ln() / (p.max / p.min).ln(),
        _ if p.max > p.min => (v - p.min) / (p.max - p.min),
        _ => 0.0,
    }
}

/// The parameters whose change audibly changes the test tone (once settled), most telling
/// first.
fn audible_params(s: &mut Session, track: Id, slot: usize, info: &EffectInfo, len: i64, base: &[Vec<f32>]) -> Vec<u32> {
    let mut out = Vec::new();
    for p in info.params {
        s.set_builtin_param(track, slot, p.id, far(p)).unwrap();
        let a = render(s, len, None);
        s.set_builtin_param(track, slot, p.id, p.default).unwrap();
        let d = diff_settled(&a, base);
        if finite(&a) && d > 1e-3 {
            out.push((p.id, d));
        }
    }
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out.into_iter().map(|x| x.0).collect()
}

fn check_effect(s: &mut Session, info: &EffectInfo, tone: &str, scratch: &PathBuf) {
    println!("== {} ({})", info.name, info.id);
    let len = 48000;
    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(std::path::Path::new(tone), a, 0).unwrap();
    let dry = render(s, len, None);

    // Loading it.
    s.checkpoint("Insert Plug-in", None, true);
    let uid = s.set_plugin(a, 0, info.id).unwrap();
    let slot = s.project.track(a).unwrap().inserts[0].clone().unwrap();
    check(&format!("{}: loads as \"{}\" with every value saved", info.name, slot.name), slot.name == info.name && slot.params.len() == info.params.len());
    let base = render(s, len, None);
    let metering = info.category == "Metering";
    check(&format!("{}: output is finite and sounds ({:.1} dBFS, dry {:.1})", info.name, db(peak(&base)), db(peak(&dry))), finite(&base) && peak(&base) > 1e-3);
    if metering {
        check(&format!("{}: a metering effect passes audio through", info.name), diff(&base, &dry) < 1e-4);
    }
    s.set_bypass(a, 0, true);
    let bypassed = render(s, len, None);
    check(&format!("{}: bypass plays the dry signal", info.name), diff(&bypassed, &dry) < 1e-5);
    s.set_bypass(a, 0, false);

    // Parameters: set, undo, redo, save and reload.
    let audible = audible_params(s, a, 0, info, len, &base);
    println!("   parameters that change the tone: {:?}", audible.iter().map(|id| info.param(*id).unwrap().name).collect::<Vec<_>>());
    if !metering {
        check(&format!("{}: some parameter changes the sound", info.name), !audible.is_empty());
    }
    let p = audible.first().and_then(|id| info.param(*id)).unwrap_or(&info.params[0]);
    let target = far(p);
    s.checkpoint("Plug-in Parameter", Some(format!("fxparam:{uid}:{}", p.id)), false);
    s.set_builtin_param(a, 0, p.id, target).unwrap();
    // A drag: more moves of the same parameter merge into the same undo step.
    s.checkpoint("Plug-in Parameter", Some(format!("fxparam:{uid}:{}", p.id)), false);
    s.set_builtin_param(a, 0, p.id, target).unwrap();
    let set = render(s, len, None);
    check(&format!("{}: {} set to {target}", info.name, p.name), (value(s, uid, p.id) - target).abs() < 1e-4);
    let clamped = s.set_builtin_param(a, 0, p.id, p.max + 1000.0).is_ok() && value(s, uid, p.id) <= p.max;
    s.set_builtin_param(a, 0, p.id, target).unwrap();
    check(&format!("{}: values are clamped to the range", info.name), clamped);
    s.undo();
    check(&format!("{}: undo puts {} back (and the sound)", info.name, p.name), (value(s, uid, p.id) - p.default).abs() < 1e-4 && diff(&render(s, len, None), &base) < 1e-4);
    s.redo();
    check(&format!("{}: redo sets it again", info.name), (value(s, uid, p.id) - target).abs() < 1e-4 && diff(&render(s, len, None), &set) < 1e-4);
    let dir = scratch.join(format!("FxCheck {}.fermata", info.name));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(Some(dir.clone())).unwrap();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap_or_default();
    s.open(&dir).unwrap();
    check(
        &format!("{}: saved and reloaded with its values", info.name),
        text.contains("\"params\"") && (value(s, uid, p.id) - target).abs() < 1e-4 && diff(&render(s, len, None), &set) < 1e-4,
    );
    let copies = s.duplicate_tracks(&[a], true);
    let copy = copies.first().and_then(|c| uid_in(s, *c, 0));
    check(&format!("{}: a duplicated track's copy keeps the values", info.name), copy.map(|c| c != uid && (value(s, c, p.id) - target).abs() < 1e-4).unwrap_or(false));
    s.delete_tracks(&copies);
    s.set_builtin_param(a, 0, p.id, p.default).unwrap();

    // Automation drives the parameter (normalized values, as for VST3).
    let lane = AutoParam::Plugin { uid, param: p.id, name: format!("{}: {}", info.name, p.name) };
    let n = far_normalized(p);
    s.set_automation_lane(a, lane.clone(), vec![AutoPoint { pos: 0, value: n }, AutoPoint { pos: len, value: n }]);
    let auto = render(s, len, None);
    if audible.is_empty() {
        check(&format!("{}: automation renders cleanly", info.name), finite(&auto));
    } else {
        let (moved, off) = (diff_settled(&auto, &base), diff_settled(&auto, &set));
        check(&format!("{}: automating {} changes the output like setting it ({moved:.4} from the default, {off:.5} from set)", info.name, p.name), moved > 1e-3 && off < 1e-3);
    }
    check(&format!("{}: its parameters are listed for automation lanes", info.name), s.plugin_params(uid).len() == info.params.len());
    s.clear_automation(a, None);

    // Meters and live values reach the tick while the panel watches.
    s.watch_builtin(uid, true);
    s.set_cycle(true, 0, len);
    s.locate(0);
    s.play();
    std::thread::sleep(Duration::from_millis(150));
    let t = s.tick();
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    s.watch_builtin(uid, false);
    let fx = t.fx.iter().find(|f| f.uid == uid);
    check(
        &format!("{}: meters reach the tick ({:?})", info.name, fx.map(|f| f.meters.clone()).unwrap_or_default()),
        fx.map(|f| f.meters.len() == info.meters.len() && f.meters.iter().all(|m| m.is_finite()) && f.values.len() == info.params.len()).unwrap_or(false),
    );
    check(&format!("{}: unwatched panels get no tick", info.name), !s.tick().fx.iter().any(|f| f.uid == uid));

    // Latency reaches delay compensation, whichever parameter sets it.
    let sr = s.sample_rate() as f32;
    let mut probe = (info.new)(sr);
    let mut latent = None;
    // An effect that is latent at its defaults (a limiter's lookahead) reports that latency;
    // finding "the latency parameter" below would only pick an unrelated one.
    let default_latency = probe.latency();
    for q in info.params.iter().filter(|_| default_latency == 0) {
        probe.set_param(q.id, q.max);
        if probe.latency() > 0 {
            latent = Some((q, probe.latency()));
            break;
        }
        probe.set_param(q.id, q.default);
    }
    let reported = |s: &Session| s.pdc_report().strips.iter().flat_map(|x| x.slots.clone()).find(|x| x.0 == uid).map(|x| x.1).unwrap_or(0);
    match latent {
        Some((q, lat)) => {
            s.set_builtin_param(a, 0, q.id, q.max).unwrap();
            settle(s);
            check(&format!("{}: {} = {} reports {lat} frames to PDC (got {})", info.name, q.name, q.max, reported(s)), reported(s) as usize == lat && s.pdc_total() as usize >= lat);
            let latent_render = render(s, len, None);
            let off = diff_settled(&latent_render, &base);
            check(&format!("{}: a latent render stays in line with the timeline (diff {off:.5})", info.name), finite(&latent_render) && off < 1e-3);
            s.set_builtin_param(a, 0, q.id, q.default).unwrap();
            settle(s);
            check(&format!("{}: back to no latency", info.name), reported(s) == 0);
        }
        None if default_latency > 0 => {
            check(
                &format!("{}: its default latency of {default_latency} frames reaches PDC (got {})", info.name, reported(s)),
                reported(s) as usize == default_latency && s.pdc_total() as usize >= default_latency,
            );
        }
        None => {
            check(&format!("{}: no latency, none reported", info.name), reported(s) == 0);
        }
    }

    // Side-chain: keyed by another track, which then plays first; loops are refused.
    if info.sidechain {
        let b = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        s.import_audio(std::path::Path::new(tone), b, 0).unwrap();
        // Place the key source after the receiver in the list: the engine must reorder.
        let ids: Vec<Id> = s.project.tracks.iter().map(|t| t.id).collect();
        s.reorder_tracks(ids);
        s.checkpoint("Side Chain", None, false);
        s.set_sidechain(a, 0, Some(b)).unwrap();
        let order = s.project.process_order();
        let at = |id| order.iter().position(|x| *x == id).unwrap();
        check(&format!("{}: the key source plays before the receiver", info.name), at(b) < at(a));
        s.set_plugin(b, 0, info.id).unwrap();
        check(&format!("{}: a side-chain loop is refused", info.name), s.set_sidechain(b, 0, Some(a)).is_err());
        s.remove_plugin(b, 0);
        let keyed = render(s, len, Some(a));
        check(&format!("{}: an isolated render with the key outside it is clean", info.name), finite(&keyed) && peak(&keyed) > 1e-3);
        if info.id == "builtin:noise-gate" {
            use fx::gate::THRESHOLD;
            // The receiver alone stays under the threshold; the key source is above it.
            let region = s.project.track(a).unwrap().regions[0].id;
            s.set_region_gain(&[region], Some(-12.0), None);
            let thr = db(peak(&dry)) - 6.0;
            s.set_builtin_param(a, 0, THRESHOLD, thr).unwrap();
            s.set_sidechain(a, 0, None).unwrap();
            let alone = peak(&render(s, len, Some(a)));
            s.set_sidechain(a, 0, Some(b)).unwrap();
            let opened = peak(&render(s, len, Some(a)));
            let mix = peak(&render(s, len, None));
            s.update_track_linked(b, TrackPatch { mute: Some(true), ..Default::default() });
            let muted = peak(&render(s, len, Some(a)));
            s.update_track_linked(b, TrackPatch { mute: Some(false), ..Default::default() });
            check(
                &format!("Noise Gate: opened by another track ({:.1} dB alone, {:.1} keyed, {:.1} in the mix, {:.1} with the key muted)", db(alone), db(opened), db(mix), db(muted)),
                alone < 1e-4 && (db(opened) - (db(peak(&dry)) - 12.0)).abs() < 1.0 && mix > opened && muted < 1e-4,
            );
            s.set_region_gain(&[region], Some(0.0), None);
            let default = info.param(THRESHOLD).unwrap().default;
            s.set_builtin_param(a, 0, THRESHOLD, default).unwrap();
        }
        s.undo();
        check(&format!("{}: undo takes the side-chain off", info.name), s.project.track(a).unwrap().inserts[0].as_ref().unwrap().sidechain.is_none());
        s.delete_track(b);
    }

    // Freeze bakes the effect in, at the settings it has.
    s.set_builtin_param(a, 0, p.id, target).unwrap();
    let live = render(s, len, None);
    // A lookahead effect at the very end of the render window sees the transport stop (silence
    // coming), while the freeze render plays the region on; compare up to that last stretch.
    let ahead = s.pdc_total() as usize;
    let cut = |x: &Vec<Vec<f32>>| x.iter().map(|c| c[..c.len().saturating_sub(ahead)].to_vec()).collect::<Vec<_>>();
    s.checkpoint("Freeze", None, true);
    s.freeze_tracks(&[a], &mut |_| {}).unwrap();
    let frozen = render(s, len, None);
    let unloaded = s.builtin_params(uid).len() == info.params.len();
    let off = diff(&cut(&frozen), &cut(&live));
    check(&format!("{}: freezing bakes it in (diff {off:.5})", info.name), off < 1e-3 && unloaded);
    s.unfreeze_track(a).unwrap();
    check(&format!("{}: unfreezing loads it with its values", info.name), (value(s, uid, p.id) - target).abs() < 1e-4 && diff(&render(s, len, None), &live) < 1e-4);

    s.delete_track(a);
}

/// Copy and paste of built-in effects, VST3 plug-ins and the Channel EQ, and saving presets.
fn clipboard_and_presets(s: &mut Session, tone: &str) {
    use daw_engine::model::{ChannelEq, PluginSlot, MASTER_ID};
    use daw_engine::presets::CHANNEL_EQ;
    use daw_engine::session::EQ_SLOT;
    println!("== copy, paste and presets");
    let len = 48000;
    let tracks: Vec<Id> = (0..3).map(|_| s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap()).collect();
    let (a, b, c) = (tracks[0], tracks[1], tracks[2]);
    for &t in &tracks {
        s.import_audio(std::path::Path::new(tone), t, 0).unwrap();
    }
    let gate = s.set_plugin(a, 0, "builtin:noise-gate").unwrap();
    s.set_builtin_param(a, 0, fx::gate::THRESHOLD, -22.0).unwrap();
    s.set_builtin_param(a, 0, fx::gate::LOW_CUT, 4000.0).unwrap();
    // The clipboard goes through the UI as JSON.
    let copied: PluginSlot = serde_json::from_str(&serde_json::to_string(&s.copy_plugin(a, 0).unwrap()).unwrap()).unwrap();
    s.checkpoint("Paste Plug-in", None, true);
    s.paste_plugin(&[b, c], 2, &copied).unwrap();
    let pasted: Vec<Option<Id>> = [b, c].iter().map(|t| uid_in(s, *t, 2)).collect();
    let same = pasted.iter().all(|u| u.map(|u| u != gate && s.builtin_params(u) == s.builtin_params(gate)).unwrap_or(false));
    let (ra, rb) = (render(s, len, Some(a)), render(s, len, Some(b)));
    check("a built-in effect pastes onto two tracks as new instances with its settings", same && pasted[0] != pasted[1] && diff(&ra, &rb) < 1e-5);
    s.undo();
    check("the paste is one undo step", uid_in(s, b, 2).is_none() && uid_in(s, c, 2).is_none());
    check("a built-in effect doesn't paste into an instrument slot", s.paste_plugin(&[b], usize::MAX, &copied).is_err());

    match s.plugins.iter().find(|d| d.name == "ZaMaximX2").cloned() {
        Some(maxim) => {
            s.set_plugin(a, 1, &maxim.id).unwrap();
            let copied = s.copy_plugin(a, 1).unwrap();
            s.checkpoint("Paste Plug-in", None, true);
            s.paste_plugin(&[b], 1, &copied).unwrap();
            let again = s.copy_plugin(b, 1).unwrap();
            check(
                "a VST3 pastes with its state captured from the controller",
                copied.state.is_some() && again.state == copied.state && uid_in(s, b, 1) != uid_in(s, a, 1) && s.plugin_instances() >= 2,
            );
            s.set_bypass(a, 0, true);
            let (ra, rb) = (render(s, len, Some(a)), render(s, len, Some(b)));
            s.set_bypass(a, 0, false);
            check(&format!("the pasted VST3 sounds like the original (diff {:.6})", diff(&ra, &rb)), diff(&ra, &rb) < 1e-4);
        }
        // CI has no third-party plug-ins: everything else here still runs.
        None => println!("SKIP the VST3 paste checks: ZaMaximX2 isn't in the plug-in cache (scan plug-ins first)"),
    }

    // Channel EQ: copy from one strip, paste onto another and Stereo Out in one step.
    s.checkpoint("Load Preset", None, false);
    s.load_preset(a, EQ_SLOT, CHANNEL_EQ, "Telephone", true).unwrap();
    let eq = s.eq_of(a).unwrap();
    let json = |e: &ChannelEq| serde_json::to_string(e).unwrap();
    check("a Channel EQ factory preset loads", json(&eq) == json(&daw_engine::presets::eq_factory().into_iter().find(|p| p.0 == "Telephone").unwrap().1));
    let before = s.eq_of(b).unwrap();
    s.checkpoint("Paste Channel EQ", None, false);
    s.paste_eq(&[b, MASTER_ID], &eq).unwrap();
    check("the Channel EQ pastes onto a track and Stereo Out", json(&s.eq_of(b).unwrap()) == json(&eq) && json(&s.eq_of(MASTER_ID).unwrap()) == json(&eq));
    s.undo();
    check("pasting the EQ is one undo step", json(&s.eq_of(b).unwrap()) == json(&before) && json(&s.eq_of(MASTER_ID).unwrap()) == json(&ChannelEq::default()));

    // Factory presets of every effect load; user presets save, refusing silent overwrites.
    for info in fx::registry() {
        let t = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        let uid = s.set_plugin(t, 0, info.id).unwrap();
        let mut ok = true;
        for p in fx::factory_presets(info.id) {
            ok &= s.load_preset(t, 0, info.id, p.name, true).is_ok();
            ok &= p.values.iter().all(|(id, v)| (value(s, uid, *id) - v).abs() < 1e-4);
        }
        check(&format!("{}: its {} factory presets load", info.name, fx::factory_presets(info.id).len()), ok);
        s.delete_track(t);
    }
    let saved = s.save_preset(a, 0, " Fx Check Kick ", false);
    let twice = s.save_preset(a, 0, "Fx Check Kick", false);
    let replaced = s.save_preset(a, 0, "Fx Check Kick", true);
    let eq_saved = s.save_preset(a, EQ_SLOT, "Fx Check Phone", false);
    check(
        "user presets save (and refuse to overwrite unasked)",
        saved.as_deref().ok() == Some("Fx Check Kick") && twice.is_err() && replaced.is_ok() && eq_saved.is_ok(),
    );
    for t in tracks {
        s.delete_track(t);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let plugins = daw_engine::plugin_db::load_cache().unwrap_or_default();
    // User presets go to the scratch folder, not the real config folder.
    let config = scratch.join("config");
    let _ = std::fs::remove_dir_all(&config);
    std::env::set_var("XDG_CONFIG_HOME", &config);
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg.clone(), plugins.clone());
    s.dir = scratch.join("FxCheck.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();

    let list = daw_engine::session::builtin_effects();
    let json = serde_json::to_value(&list).unwrap();
    let kinds_ok = json.as_array().unwrap().iter().flat_map(|e| e["params"].as_array().unwrap().iter()).all(|p| {
        let unit = p["unit"].as_str().unwrap_or("?");
        let kind = &p["kind"];
        ["none", "db", "ms", "hz", "ratio", "percent", "semitones", "note", "deg"].contains(&unit)
            && match kind["type"].as_str() {
                Some("continuous") | Some("toggle") => true,
                Some("choice") => kind["labels"].as_array().map(|l| !l.is_empty()).unwrap_or(false),
                _ => false,
            }
    });
    check(&format!("the registry lists {} effects in the UI's shape", list.len()), !list.is_empty() && kinds_ok);
    let reg = fx::registry();
    let mut ids: Vec<&str> = reg.iter().map(|e| e.id).collect();
    ids.sort();
    ids.dedup();
    check("registry ids are unique and built-in", ids.len() == reg.len() && reg.iter().all(|e| e.id.starts_with("builtin:")));
    for info in &reg {
        let mut ok = true;
        for p in info.params {
            ok &= p.min <= p.default && p.default <= p.max && (!p.log || p.min > 0.0);
            ok &= info.params.iter().filter(|q| q.id == p.id).count() == 1;
        }
        check(&format!("{}: parameter specs are sane", info.name), ok && info.meters.len() <= fx::MAX_METERS);
    }

    for info in &reg {
        check_effect(&mut s, info, &tone, &scratch);
    }
    clipboard_and_presets(&mut s, &tone);
    drop(s);
    // User presets are global: a new session (any project) sees them.
    let mut s = Session::new(cfg, plugins);
    s.dir = scratch.join("FxCheck2.fermata");
    let list = daw_engine::presets::list(&daw_engine::presets::user_root(), "builtin:noise-gate");
    check("user presets outlive the session", list.user.contains(&"Fx Check Kick".to_string()));
    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    let uid = s.set_plugin(a, 0, "builtin:noise-gate").unwrap();
    s.checkpoint("Load Preset", None, false);
    s.load_preset(a, 0, "builtin:noise-gate", "Fx Check Kick", false).unwrap();
    check("a user preset loads into another project", (value(&s, uid, fx::gate::THRESHOLD) + 22.0).abs() < 1e-4);
    s.undo();
    check("loading a preset is undoable", (value(&s, uid, fx::gate::THRESHOLD) + 40.0).abs() < 1e-4);
    let root = daw_engine::presets::user_root();
    let renamed = daw_engine::presets::rename_user(&root, "builtin:noise-gate", "Fx Check Kick", "Kick 2").is_ok();
    let listed = daw_engine::presets::list_user(&root, "builtin:noise-gate");
    let deleted = daw_engine::presets::delete_user(&root, "builtin:noise-gate", "Kick 2").is_ok();
    check("user presets rename and delete", renamed && listed == vec!["Kick 2".to_string()] && deleted && daw_engine::presets::list_user(&root, "builtin:noise-gate").is_empty());

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
