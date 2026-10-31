//! Built-in instruments through the real session (silent ALSA null output), with Fermata Tre:
//! loading it, a MIDI region's render and that it repeats bit for bit, parameter edits, automation
//! lanes, undo and redo, factory and user presets, saving and reopening, Freeze, Bounce in Place,
//! a bounce's tail, Track Off, copy and paste, strip settings, duplicating, MIDI learn, note chase
//! and live MIDI. And that Fermata Synth is still there, and an old project's slot still loads.
//! usage: instrument_check <scratch dir>
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use daw_engine::instrument::tre::params::param;
use daw_engine::model::{AutoParam, CcEvent, CcKind, Id, InputSel, Note, TrackKind};
use daw_engine::session::{BipOptions, BipSource, MidiContent, RenderOptions, TrackPatch, BUILTIN_SYNTH};
use daw_engine::{audio_io::AudioConfig, Session};

const TRE: &str = "builtin:tre";
const RATE: i64 = 48000;
/// The instrument slot, as the commands address it.
const SLOT: usize = usize::MAX;

static FAILED: AtomicBool = AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn session() -> Session {
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    Session::new(cfg, Vec::new())
}

fn note(start: i64, length: i64, pitch: u8) -> Note {
    Note { start, length, pitch, velocity: 100, muted: false }
}

/// An instrument track with a Tre and a phrase on it, ending at `RATE * 2` with one long note.
fn track(s: &mut Session, plugin: &str) -> Id {
    let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(plugin.into()), None).unwrap();
    let r = s.create_midi_region(t, 0, RATE * 2).unwrap();
    let notes = vec![note(0, RATE / 4, 36), note(RATE / 2, RATE / 4, 43), note(RATE, RATE / 2, 48), note(RATE * 3 / 2, RATE / 2, 36)];
    let cc = vec![CcEvent { start: RATE / 2, kind: CcKind::Controller(1), value: 90 }, CcEvent { start: RATE, kind: CcKind::PitchBend, value: 10000 }];
    s.set_midi_content(vec![MidiContent { region: r, notes, cc, start: None, length: None }]);
    t
}

fn render(s: &mut Session) -> Vec<Vec<f32>> {
    s.render(0, RATE * 2, RATE, None, &mut |_| {}).unwrap()
}

fn same(a: &[Vec<f32>], b: &[Vec<f32>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits()))
}

fn peak(a: &[Vec<f32>]) -> f32 {
    a.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()))
}

fn diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y)).fold(0.0f32, |m, (p, q)| m.max((p - q).abs()))
}

fn slot_uid(s: &Session, t: Id) -> Id {
    s.project.track(t).unwrap().instrument.as_ref().unwrap().uid
}

fn value(s: &Session, uid: Id, id: u32) -> f32 {
    s.builtin_params(uid).iter().find(|p| p.0 == id).map(|p| p.1).unwrap_or(f32::NAN)
}

fn undoable_set(s: &mut Session, t: Id, id: u32, v: f32) {
    let uid = slot_uid(s, t);
    s.checkpoint("Plug-in Parameter", Some(format!("fxparam:{uid}:{id}")), false);
    s.set_builtin_param(t, SLOT, id, v).unwrap();
}

/// The track's level over a moment of live playback, off the meters.
fn level(s: &mut Session, id: Id, ms: u64) -> f32 {
    s.tick();
    std::thread::sleep(Duration::from_millis(ms));
    let t = s.tick();
    t.meters.iter().find(|m| m.0 == id).map(|m| m.1.max(m.2)).unwrap_or(-1.0)
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    std::fs::create_dir_all(&scratch).unwrap();
    let mut s = session();

    // The registry.
    let listed = daw_engine::session::builtin_instruments();
    check("the registry lists Fermata Synth and Fermata Tre", listed.iter().any(|i| i.id == BUILTIN_SYNTH && i.name == "Fermata Synth") && listed.iter().any(|i| i.id == TRE && !i.sections.is_empty()));
    check("instruments aren't in the effects' list", daw_engine::session::builtin_effects().iter().all(|e| e.id != TRE && e.id != BUILTIN_SYNTH));

    // Loading.
    let a = track(&mut s, TRE);
    let slot = s.project.track(a).unwrap().instrument.clone().unwrap();
    check(&format!("a new track takes it, named for it ({})", s.project.track(a).unwrap().name), slot.plugin_id == TRE && slot.name == "Fermata Tre" && s.project.track(a).unwrap().name == "Fermata Tre");
    check("its values are written out in the project", slot.params.len() == daw_engine::instrument::find(TRE).unwrap().params.len());
    check("it isn't reported missing", s.missing_plugins().is_empty());
    check("an instrument doesn't go in an Audio FX slot", s.set_plugin(a, 0, TRE).is_err());
    check("an effect doesn't go in the instrument slot", s.set_plugin(a, SLOT, "builtin:reverb").is_err());
    let plain = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    check("a new instrument track without a choice is still Fermata Synth", s.project.track(plain).unwrap().instrument.as_ref().unwrap().plugin_id == BUILTIN_SYNTH);
    s.delete_track(plain);

    // Rendering.
    let base = render(&mut s);
    check(&format!("a MIDI region renders (peak {:.3})", peak(&base)), peak(&base) > 0.02 && peak(&base) < 1.5);
    check("it is finite", base.iter().flatten().all(|v| v.is_finite()));
    check("two renders are bit-identical", same(&base, &render(&mut s)));
    let quiet_start = base[0][..200].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    check(&format!("a render starts clean, not with a tail ({quiet_start:.4})"), quiet_start < 0.2);

    // Parameter edits, undo and redo.
    let uid = slot_uid(&s, a);
    undoable_set(&mut s, a, param::CUTOFF, 200.0);
    let edited = render(&mut s);
    check("a parameter edit changes the sound", diff(&base, &edited) > 0.01);
    check("its value is in the project", value(&s, uid, param::CUTOFF) == 200.0);
    s.undo();
    check("undo puts the value and the sound back, bit for bit", value(&s, uid, param::CUTOFF) == 800.0 && same(&base, &render(&mut s)));
    s.redo();
    check("redo sets it again", value(&s, uid, param::CUTOFF) == 200.0 && same(&edited, &render(&mut s)));
    s.undo();
    s.set_builtin_param(a, SLOT, param::EMPHASIS, 1e6).unwrap();
    check("values are clamped to the range", value(&s, uid, param::EMPHASIS) == 10.0);
    s.set_builtin_param(a, SLOT, param::EMPHASIS, 2.0).unwrap();
    check("an unknown parameter is refused", s.set_builtin_param(a, SLOT, 9999, 1.0).is_err());

    // Automation.
    let infos = s.plugin_params(uid);
    let cutoff_norm = |hz: f32| (hz / 20.0).ln() / 1000f32.ln();
    check(&format!("the lane picker lists its {} parameters", infos.len()), infos.len() > 40 && infos.iter().any(|p| p.name == "Cutoff" && p.id == param::CUTOFF));
    let lane = AutoParam::Plugin { uid, param: param::CUTOFF, name: "Fermata Tre: Cutoff".into() };
    let pt = |pos: i64, value: f32| daw_engine::model::AutoPoint { pos, value, curve: 0.0 };
    s.set_automation_lane(a, lane.clone(), vec![pt(0, cutoff_norm(100.0)), pt(RATE * 2, cutoff_norm(3000.0))]);
    let automated = render(&mut s);
    check("an automation lane changes the sound", diff(&base, &automated) > 0.01);
    check("and repeats exactly", same(&automated, &render(&mut s)));
    let first = automated[0][..RATE as usize / 8].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let unautomated = base[0][..RATE as usize / 8].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    check(&format!("the lane's start plays from the first block ({first:.3} against {unautomated:.3})"), first < unautomated);
    s.clear_automation(a, Some(lane.clone()));
    check("clearing the lane puts the sound back", same(&base, &render(&mut s)));

    // Presets.
    let root = daw_engine::presets::user_root();
    let list = daw_engine::presets::list(&root, TRE);
    check(&format!("{} factory presets, Default first", list.factory.len()), list.factory.len() >= 11 && list.factory[0] == "Default");
    s.checkpoint("Load Preset", None, false);
    s.load_preset(a, SLOT, TRE, "Funk Pluck", true).unwrap();
    let funk = render(&mut s);
    check("a factory preset changes the sound and the values", diff(&base, &funk) > 0.01 && value(&s, uid, param::F_SUSTAIN) == 0.0);
    s.undo();
    check("undo takes a preset back in one step", same(&base, &render(&mut s)));
    s.load_preset(a, SLOT, TRE, "Funk Pluck", true).unwrap();
    s.set_builtin_param(a, SLOT, param::EMPHASIS, 6.5).unwrap();
    let mine = render(&mut s);
    check("a user preset saves", s.save_preset(a, SLOT, "Instrument Check Bass", true).is_ok());
    check("and lists", daw_engine::presets::list(&root, TRE).user.iter().any(|n| n == "Instrument Check Bass"));
    check("saving over it asks first", s.save_preset(a, SLOT, "Instrument Check Bass", false).is_err());
    s.load_preset(a, SLOT, TRE, "Default", true).unwrap();
    check("Default puts every value back", same(&base, &render(&mut s)));
    s.load_preset(a, SLOT, TRE, "Instrument Check Bass", false).unwrap();
    check("loading the user preset brings the sound back, bit for bit", same(&mine, &render(&mut s)) && value(&s, uid, param::EMPHASIS) == 6.5);
    let other_slot = daw_engine::presets::get(&root, TRE, "Instrument Check Bass", false).unwrap();
    check("a user preset is a file with values by id", other_slot.values.iter().any(|v| v.0 == param::EMPHASIS && v.1 == 6.5));
    let wrong = daw_engine::presets::Preset { effect: "builtin:reverb".into(), ..other_slot };
    check("a preset for another plug-in is refused", s.apply_preset(a, SLOT, &wrong).is_err());
    let renamed = daw_engine::presets::rename_user(&root, TRE, "Instrument Check Bass", "Instrument Check Bass 2");
    check("it renames and deletes", renamed.is_ok() && daw_engine::presets::delete_user(&root, TRE, "Instrument Check Bass 2").is_ok());

    // Saving and reopening.
    s.load_preset(a, SLOT, TRE, "Singing Lead", true).unwrap();
    s.set_builtin_param(a, SLOT, param::DRIFT, 1.0).unwrap();
    s.set_automation_lane(a, lane.clone(), vec![pt(0, cutoff_norm(800.0)), pt(RATE, cutoff_norm(2000.0))]);
    let lead = render(&mut s);
    let dir = scratch.join("Tre.fermata");
    s.save(Some(dir.clone())).unwrap();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap_or_default();
    check("the project file holds the slot's values", text.contains(TRE) && text.contains("\"params\""));
    drop(s);
    let mut s = session();
    s.open(&dir).unwrap();
    check("reopened: the values are back", value(&s, uid, param::DRIFT) == 1.0 && s.project.track(a).unwrap().automation.iter().any(|l| !l.points.is_empty()));
    check("reopened: the render is bit-identical, drift included", same(&lead, &render(&mut s)));

    // Duplicates are their own instrument: same settings, their own analog differences.
    let copy = s.duplicate_tracks(&[a], true)[0];
    let copy_uid = slot_uid(&s, copy);
    check("a duplicated track has new instance with the values", copy_uid != uid && value(&s, copy_uid, param::DRIFT) == 1.0);
    let solo_copy = |s: &mut Session, on: bool| {
        s.update_track(a, TrackPatch { mute: Some(on), ..Default::default() });
        render(s)
    };
    let dup = solo_copy(&mut s, true);
    s.update_track(a, TrackPatch { mute: Some(false), ..Default::default() });
    s.update_track(copy, TrackPatch { mute: Some(true), ..Default::default() });
    let orig = render(&mut s);
    check("the duplicate plays the same part, with differences of its own", peak(&dup) > 0.02 && !same(&dup, &orig) && diff(&dup, &orig) < 1.5);
    s.delete_track(copy);

    // Freeze.
    s.set_builtin_param(a, SLOT, param::DECAY_ON, 1.0).unwrap();
    s.set_builtin_param(a, SLOT, param::A_DECAY, 600.0).unwrap();
    let live = render(&mut s);
    // Past the region's end, as a frozen track's tail only plays with the transport running.
    let long = |s: &mut Session| s.render(0, RATE * 4, 0, None, &mut |_| {}).unwrap();
    let live_long = long(&mut s);
    s.freeze_tracks(&[a], &mut |_| {}).unwrap();
    check("freezing unloads it", s.project.track(a).unwrap().frozen.is_some());
    check("a frozen track refuses a parameter edit", s.set_builtin_param(a, SLOT, param::CUTOFF, 300.0).is_err());
    let frozen = long(&mut s);
    // The range itself is the same; the tail is what a render's stop leaves ringing (the engine lets
    // go of the bend there, as it ends the notes, so its pitch isn't the running transport's).
    let upto = (RATE * 2) as usize;
    let cut = |x: &[Vec<f32>]| x.iter().map(|c| c[..upto].to_vec()).collect::<Vec<_>>();
    let off = diff(&cut(&frozen), &cut(&live_long));
    let ringing = frozen[0][upto + RATE as usize / 4..upto + RATE as usize / 2].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let wanted = live_long[0][upto + RATE as usize / 4..upto + RATE as usize / 2].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    check(&format!("a frozen track plays what it played live, with its tail (off by {off:.5}; tail {ringing:.3}, live {wanted:.3})"), off < 2e-3 && ringing > 0.25 * wanted && ringing < 4.0 * wanted);
    s.unfreeze_track(a).unwrap();
    check("unfrozen, it renders live again, bit for bit", same(&live, &render(&mut s)));

    // Bounce in Place and a bounce's tail.
    let made = s.bounce_in_place(&[a], &[], &BipOptions { effects: true, fader: false, source: BipSource::Leave, tail: true }, &mut |_| {}).unwrap();
    check("Bounce in Place makes a track from it", made.len() == 1 && !s.project.track(made[0]).unwrap().regions.is_empty());
    s.delete_track(made[0]);
    let wav = scratch.join("bounce.wav");
    let bounced = s.bounce_project(&wav, &RenderOptions { start: 0, end: RATE * 2, ..Default::default() }, &mut |_| {}).unwrap();
    check(&format!("a bounce keeps the tail ({:.2} s written)", bounced.seconds), bounced.seconds > 2.05 && wav.exists());

    // Track Off.
    s.update_track(a, TrackPatch { off: Some(true), ..Default::default() });
    let off = render(&mut s);
    check("Off, it is silent", peak(&off) == 0.0);
    s.update_track(a, TrackPatch { off: Some(false), ..Default::default() });
    check("on again, it renders as before, bit for bit", same(&live, &render(&mut s)));

    // Copy and paste, strip settings.
    let b = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let clip = s.copy_plugin(a, SLOT).unwrap();
    s.paste_plugin(&[b], SLOT, &clip).unwrap();
    let pasted = s.project.track(b).unwrap().instrument.clone().unwrap();
    check("pasting a slot makes a new instance with its values", pasted.plugin_id == TRE && pasted.uid != uid && pasted.params == clip.params);
    check("an instrument doesn't paste into an Audio FX slot", s.paste_plugin(&[b], 0, &clip).is_err());
    s.checkpoint("Paste", None, true);
    let setting = s.copy_strip(a).unwrap();
    s.remove_plugin(b, SLOT);
    s.paste_strip(&[b], &setting).unwrap();
    let pasted = s.project.track(b).unwrap().instrument.clone().unwrap();
    check("a strip setting carries it", pasted.plugin_id == TRE && pasted.params == clip.params);
    let setting_uid = pasted.uid;
    s.undo();
    check("undo of the strip setting takes it out", s.project.track(b).unwrap().instrument.as_ref().map(|i| i.uid) != Some(setting_uid));
    s.delete_track(b);

    // MIDI learn: moves the parameter, as a built-in effect's does.
    s.touched_slot(a, SLOT, param::CUTOFF);
    let name = s.start_learn(None).unwrap();
    check(&format!("learning names the parameter ({name})"), name.contains("tre") || name.contains("Tre"));
    const PORT: &str = "Test Controller:Test Controller MIDI 1 20:0";
    let cc = |s: &mut Session, value: u8| {
        s.inject_midi_from(PORT, &[0xB0, 74, value]);
        s.idle();
    };
    cc(&mut s, 64);
    check("a control change is learned", s.tick().learned.is_some());
    s.clear_automation(a, Some(lane.clone()));
    let before = value(&s, uid, param::CUTOFF);
    cc(&mut s, 127);
    std::thread::sleep(Duration::from_millis(20));
    cc(&mut s, 20);
    check(&format!("the controller moves it ({before} -> {})", value(&s, uid, param::CUTOFF)), value(&s, uid, param::CUTOFF) != before);
    s.undo();
    check("its move undoes", value(&s, uid, param::CUTOFF) != 0.0);

    // Live: chase, thru and stopping.
    s.set_builtin_param(a, SLOT, param::DECAY_ON, 0.0).unwrap();
    s.set_builtin_param(a, SLOT, param::A_SUSTAIN, 10.0).unwrap();
    s.set_builtin_param(a, SLOT, param::VOLUME, 8.0).unwrap();
    s.set_builtin_param(a, SLOT, param::CUTOFF, 2000.0).unwrap();
    let c = s.project.cycle.clone();
    s.set_cycle(true, RATE + RATE / 8, RATE + RATE * 3 / 8);
    s.locate(RATE + RATE / 8);
    s.play();
    std::thread::sleep(Duration::from_millis(60));
    let chased = level(&mut s, a, 200);
    s.stop();
    std::thread::sleep(Duration::from_millis(100));
    let stopped = level(&mut s, a, 200);
    check(&format!("played from inside a note it sounds at once, and stops (meter {chased:.3} then {stopped:.5})"), chased > 0.02 && stopped < 1e-3);
    s.set_cycle(c.enabled, c.start, c.end);
    s.locate(RATE * 10);
    s.select_track(Some(a));
    s.send_midi([0x90, 60, 100]);
    std::thread::sleep(Duration::from_millis(50));
    let held = level(&mut s, a, 150);
    s.send_midi([0x80, 60, 0]);
    std::thread::sleep(Duration::from_millis(100));
    let released = level(&mut s, a, 200);
    check(&format!("live MIDI plays it, and a note-off ends it (meter {held:.3} then {released:.5})"), held > 0.02 && released < 1e-3);
    s.send_midi([0x90, 60, 100]);
    std::thread::sleep(Duration::from_millis(50));
    s.select_track(None);
    std::thread::sleep(Duration::from_millis(100));
    let moved_off = level(&mut s, a, 200);
    check(&format!("a held key lets go when the track stops being the target (meter {moved_off:.5})"), moved_off < 1e-3);

    s.select_track(Some(a));
    s.send_midi([0x90, 60, 100]);
    std::thread::sleep(Duration::from_millis(50));
    s.set_bypass(a, SLOT, true);
    s.send_midi([0x80, 60, 0]);
    std::thread::sleep(Duration::from_millis(50));
    s.set_bypass(a, SLOT, false);
    std::thread::sleep(Duration::from_millis(100));
    let unbypassed = level(&mut s, a, 200);
    check(&format!("a key let go while it's bypassed doesn't stay down (meter {unbypassed:.5})"), unbypassed < 1e-3);

    // A slot saved before this existed still opens as what it was.
    let synth = track(&mut s, BUILTIN_SYNTH);
    check("Fermata Synth still loads with no values", s.project.track(synth).unwrap().instrument.as_ref().unwrap().params.is_empty());
    s.update_track(a, TrackPatch { mute: Some(true), ..Default::default() });
    check("and plays", peak(&render(&mut s)) > 0.01);
    check("no audio was contained", s.status.bad_audio.load(Ordering::Relaxed) == 0);

    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}
