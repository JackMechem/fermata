//! A project that uses every feature of the engine at once, built step by step, for the checks
//! that look at how the features get along (`restore_check`, `soak_check`, `rt_alloc_check`).
//! Each step is one undoable edit, so a check can undo and redo them one at a time.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use daw_engine::model::{AutoParam, AutoPoint, CompSegment, Id, InputSel, Note, Region, RegionContent, Take, TakeFolder, TrackKind, MASTER_ID};
use daw_engine::pattern::{Pattern, PatternRow, Step};
use daw_engine::region_params::RegionParams;
use daw_engine::session::{MidiContent, RegionParamsPatch, VcaPatch, BUILTIN_SYNTH};
use daw_engine::strip_silence::StripSettings;
use daw_engine::Session;

pub const RATE: u32 = 48_000;
/// A bar at 120 bpm in 4/4.
pub const BAR: i64 = 96_000;
/// How much of the project the checks render.
pub const LEN: i64 = 8 * BAR;

/// The tracks and regions the steps work on.
#[derive(Debug, Default, Clone)]
pub struct World {
    pub a1: Id,
    pub a2: Id,
    pub inst: Id,
    pub drums: Id,
    pub folder: Id,
    pub vca: Id,
    pub midi: Id,
    pub take_folder: Id,
    pub midi_folder: Id,
    pub pattern: Id,
    pub audio: PathBuf,
}

/// Writes the checks' audio: two seconds of tone bursts with silence between them (so Strip
/// Silence has something to strip and zero crossings are close together), stereo float.
pub fn write_bursts(path: &Path) {
    let spec = hound::WavSpec { channels: 2, sample_rate: RATE, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let n = 2 * RATE as usize;
    for i in 0..n {
        let t = i as f64 / RATE as f64;
        let (hz, on) = if t < 0.5 {
            (330.0, true)
        } else if t < 0.9 {
            (0.0, false)
        } else {
            (550.0, true)
        };
        let x = if on { 0.1 * (std::f64::consts::TAU * hz * t).sin() } else { 0.0 };
        w.write_sample(x as f32).unwrap();
        w.write_sample((x * 0.8) as f32).unwrap();
    }
    w.finalize().unwrap();
}

/// Every built-in model added after Tre, by id, with the preset that works it hardest.
#[allow(dead_code)]
pub const MODELS: [(&str, &str); 5] = [
    ("builtin:sei", "Unison Rave Stack"),
    ("builtin:cinque", "Poly Mod Pad"),
    ("builtin:uno", "Squelch Line"),
    ("builtin:sedici", "Long Tubular Chime"),
    ("builtin:onda", "Seven Saw Anthem"),
];

/// A track with one of those models and a part of chords and overlapping notes, for the real-time checks.
#[allow(dead_code)]
pub fn add_model(s: &mut Session, plugin: &str, preset: &str) -> Id {
    let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(plugin.into()), None).unwrap();
    let r = s.create_midi_region(t, 0, 8 * BAR).unwrap();
    let mut notes = Vec::new();
    for i in 0..32i64 {
        for k in 0..4i64 {
            notes.push(note(i * BAR / 4, BAR / 3, (40 + (i * 5 % 24) + k * 4) as u8, if i % 3 == 0 { 120 } else { 90 }));
        }
    }
    s.set_midi_content(vec![MidiContent { region: r, notes, cc: Vec::new(), start: None, length: None }]);
    s.load_preset(t, usize::MAX, plugin, preset, true).unwrap();
    t
}

/// A Fermata Tre track with a part and a lane moving its cutoff, for the real-time checks: a built-in
/// instrument with drift on, and audio-rate modulation so its costliest path runs.
#[allow(dead_code)]
pub fn add_tre(s: &mut Session) -> Id {
    use daw_engine::instrument::tre::params::param;
    let t = s.add_track(TrackKind::Instrument, InputSel::None, Some("builtin:tre".into()), None).unwrap();
    let r = s.create_midi_region(t, 0, 8 * BAR).unwrap();
    let notes = (0..64).map(|i| note(i * BAR / 8, BAR / 12, 36 + (i * 5 % 24) as u8, 100)).collect();
    s.set_midi_content(vec![MidiContent { region: r, notes, cc: Vec::new(), start: None, length: None }]);
    for (id, v) in [(param::DRIFT, 1.0), (param::ON3, 1.0), (param::NOISE_ON, 1.0), (param::FILTER_MOD, 1.0), (param::OSC_MOD, 1.0), (param::MOD_AMOUNT, 2.0)] {
        s.set_builtin_param(t, usize::MAX, id, v).unwrap();
    }
    let uid = s.project.track(t).unwrap().instrument.as_ref().unwrap().uid;
    let lane = AutoParam::Plugin { uid, param: param::CUTOFF, name: "Fermata Tre: Cutoff".into() };
    s.set_automation_lane(t, lane, vec![pt(0, 0.3, 0.0), pt(4 * BAR, 0.8, 0.0), pt(8 * BAR, 0.4, 0.0)]);
    t
}

fn note(start: i64, length: i64, pitch: u8, velocity: u8) -> Note {
    Note { start, length, pitch, velocity, muted: false }
}

pub fn pt(pos: i64, value: f32, curve: f32) -> AutoPoint {
    AutoPoint { pos, value, curve }
}

/// The regions on a track, by id.
pub fn region_ids(s: &Session, track: Id) -> Vec<Id> {
    s.project.track(track).map(|t| t.regions.iter().map(|r| r.id).collect()).unwrap_or_default()
}

/// The project the steps start from: two audio tracks and two instrument tracks (the built-in
/// synth), the first audio track and the first instrument in a folder stack, and a MIDI take
/// folder (written into the document and reopened, since only MIDI cycle recording makes one).
pub fn base(s: &mut Session, dir: &Path) -> World {
    std::fs::create_dir_all(dir).unwrap();
    let audio = dir.join("bursts.wav");
    write_bursts(&audio);
    s.set_tempo(120.0);
    let mut w = World { audio: audio.clone(), ..Default::default() };
    w.a1 = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    w.a2 = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    w.inst = s.add_track(TrackKind::Instrument, InputSel::None, Some(BUILTIN_SYNTH.into()), None).unwrap();
    w.drums = s.add_track(TrackKind::Instrument, InputSel::None, Some(BUILTIN_SYNTH.into()), None).unwrap();
    s.import_audio(&audio, w.a1, 0).unwrap();
    s.import_audio(&audio, w.a1, 2 * BAR).unwrap();
    s.import_audio(&audio, w.a2, 0).unwrap();
    s.import_audio(&audio, w.a2, RATE as i64).unwrap();
    s.import_audio(&audio, w.a2, 4 * BAR).unwrap();
    w.midi = s.create_midi_region(w.inst, 0, 2 * BAR).unwrap();
    let q = BAR / 4;
    let notes = vec![note(0, q, 60, 100), note(q + 1234, q / 2, 64, 90), note(2 * q - 700, q, 67, 80), note(5 * q, 3 * q, 72, 110)];
    s.set_midi_content(vec![MidiContent { region: w.midi, notes, cc: Vec::new(), start: None, length: None }]);
    w.folder = s.create_stack(&[w.a1, w.inst], false).unwrap();

    // A MIDI take folder on the second instrument at bar 5: two takes, the second heard.
    s.save(Some(dir.join("Base.fermata"))).unwrap();
    let file = dir.join("Base.fermata/project.json");
    let mut doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let mut next = doc["next_id"].as_u64().unwrap();
    let mut id = || {
        next += 1;
        next - 1
    };
    let (t1, t2, rid) = (id(), id(), id());
    let take = |id: Id, name: &str, pitch: u8| Take { id, name: name.into(), clip: 0, offset: 0, start: 0, end: 2 * BAR, gain_db: 0.0, notes: vec![note(0, q, pitch, 100), note(2 * q, q, pitch + 4, 100), note(BAR + q, q, pitch + 7, 100)], cc: Vec::new() };
    let mut folder = TakeFolder::new();
    folder.takes = vec![take(t1, "Take 1", 48), take(t2, "Take 2", 55)];
    folder.comp = vec![CompSegment { start: 0, end: 2 * BAR, take: t2 }];
    let heard = folder.takes[1].notes.clone();
    let region = Region {
        id: rid,
        name: "Takes".into(),
        start: 4 * BAR,
        length: 2 * BAR,
        muted: false,
        color: None,
        content: RegionContent::Midi { notes: heard, cc: Vec::new() },
        loop_end: None,
        takes: Some(folder),
        params: RegionParams::default(),
        pattern: None,
    };
    doc["next_id"] = serde_json::json!(next);
    let tracks = doc["tracks"].as_array_mut().unwrap();
    let t = tracks.iter_mut().find(|t| t["id"].as_u64() == Some(w.drums)).unwrap();
    t["regions"].as_array_mut().unwrap().push(serde_json::to_value(&region).unwrap());
    std::fs::write(&file, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    s.open(&dir.join("Base.fermata")).unwrap();
    w.midi_folder = rid;
    w
}

/// One undoable edit of the all-features project.
pub type StepFn = fn(&mut Session, &mut World);

fn set_fx(s: &mut Session, track: Id, slot: usize, id: &str, frac: f32) {
    s.set_plugin(track, slot, id).unwrap();
    let info = daw_engine::fx::find(id).unwrap();
    let p = &info.params[0];
    s.set_builtin_param(track, slot, p.id, p.min + (p.max - p.min) * frac).unwrap();
}

fn slot_uid(s: &Session, track: Id, slot: usize) -> Id {
    let t = if track == MASTER_ID { s.project.master.inserts[slot].as_ref() } else { s.project.track(track).unwrap().inserts[slot].as_ref() };
    t.unwrap().uid
}

/// Every feature, one undo step each, in an order where later ones meet the earlier ones.
pub fn steps() -> Vec<(&'static str, StepFn)> {
    vec![
        ("Chorus", |s, w| set_fx(s, w.a1, 0, "builtin:chorus", 0.4)),
        ("Flanger", |s, w| set_fx(s, w.a1, 1, "builtin:flanger", 0.3)),
        ("Tremolo", |s, w| set_fx(s, w.a1, 2, "builtin:tremolo", 0.6)),
        ("Phaser", |s, w| set_fx(s, w.a2, 0, "builtin:phaser", 0.5)),
        ("Overdrive", |s, w| set_fx(s, w.a2, 1, "builtin:overdrive", 0.3)),
        ("Distortion", |s, w| set_fx(s, w.inst, 0, "builtin:distortion", 0.2)),
        ("Gain", |s, w| set_fx(s, w.inst, 1, "builtin:gain", 0.45)),
        ("Linear Phase EQ", |s, w| {
            use daw_engine::fx::linear_phase_eq::{gain, on, QUALITY};
            s.set_plugin(w.drums, 0, "builtin:linear-phase-eq").unwrap();
            s.set_builtin_param(w.drums, 0, on(3), 1.0).unwrap();
            s.set_builtin_param(w.drums, 0, gain(3), 9.0).unwrap();
            s.set_builtin_param(w.drums, 0, QUALITY, 0.0).unwrap();
        }),
        ("Linear Phase EQ Quality (latency change)", |s, w| {
            s.set_builtin_param(w.drums, 0, daw_engine::fx::linear_phase_eq::QUALITY, 2.0).unwrap();
        }),
        ("Pre-fader side-chain", |s, w| {
            set_fx(s, w.a2, 2, "builtin:compressor", 0.1);
            s.set_sidechain(w.a2, 2, Some(w.a1)).unwrap();
            s.set_sidechain_pre(w.a2, 2, true).unwrap();
        }),
        ("Create VCA", |s, w| {
            w.vca = s.create_vca(&[w.a1, w.inst]);
            s.update_vca(w.vca, VcaPatch { level_db: Some(-5.0), ..Default::default() });
        }),
        ("Assign VCA", |s, w| s.assign_vca(&[w.drums], Some(w.vca)).unwrap()),
        ("Folder automation", |s, w| s.set_automation_lane(w.folder, AutoParam::Volume, vec![pt(0, -12.0, 0.5), pt(3 * BAR, 0.0, 0.0), pt(6 * BAR, -6.0, -0.4)])),
        ("Folder mute automation", |s, w| s.set_automation_lane(w.folder, AutoParam::Mute, vec![pt(0, 0.0, 0.0), pt(7 * BAR, 0.0, 0.0), pt(7 * BAR, 1.0, 0.0)])),
        ("Curved pan", |s, w| s.set_automation_lane(w.a2, AutoParam::Pan, vec![pt(0, -0.8, 0.7), pt(4 * BAR, 0.8, -0.7), pt(8 * BAR, 0.0, 0.0)])),
        ("Effect automation", |s, w| {
            let uid = slot_uid(s, w.a1, 0);
            s.set_automation_lane(w.a1, AutoParam::Plugin { uid, param: 0, name: "Rate".into() }, vec![pt(0, 0.1, 0.0), pt(4 * BAR, 0.9, 0.3)]);
        }),
        ("Stereo Out automation", |s, _| s.set_automation_lane(MASTER_ID, AutoParam::Volume, vec![pt(0, -3.0, 0.0), pt(5 * BAR, -1.0, 0.6), pt(8 * BAR, -9.0, 0.0)])),
        ("Stereo Out effect automation", |s, _| {
            // Gain runs from -96 to +24 dB: 0.8 is 0 dB.
            set_fx(s, MASTER_ID, 0, "builtin:gain", 0.8);
            let uid = slot_uid(s, MASTER_ID, 0);
            s.set_automation_lane(MASTER_ID, AutoParam::Plugin { uid, param: 0, name: "Gain".into() }, vec![pt(0, 0.75, 0.0), pt(8 * BAR, 0.82, 0.0)]);
        }),
        ("Automation lanes", |s, w| s.set_auto_lanes(vec![(w.a1, vec![AutoParam::Pan, AutoParam::Mute]), (MASTER_ID, vec![AutoParam::Volume])])),
        ("Pattern region", |s, w| {
            w.pattern = s.create_pattern_region(w.drums, BAR, 2 * BAR).unwrap();
            let mut p = Pattern { swing: 30, ..Pattern::default() };
            p.rows = vec![PatternRow::new(36), PatternRow::new(42)];
            for k in [0usize, 4, 8, 12] {
                p.rows[0].steps.resize(16, Step::default());
                p.rows[0].steps[k] = Step { on: true, ..Step::default() };
            }
            p.rows[1].steps = (0..16).map(|k| Step { on: k % 2 == 0, ..Step::default() }).collect();
            s.set_pattern(w.pattern, p).unwrap();
        }),
        ("Pattern region parameters", |s, w| s.set_region_params(&[w.pattern], &RegionParamsPatch { transpose: Some(-2), velocity: Some(-20), ..Default::default() })),
        ("Region parameters", |s, w| s.set_region_params(&[w.midi], &RegionParamsPatch { quantize: Some(16), swing: Some(60), transpose: Some(3), dynamics: Some(150), gate: Some(80), delay: Some(30.0), ..Default::default() })),
        ("Audio region delay", |s, w| {
            let r = region_ids(s, w.a2)[2];
            s.set_region_params(&[r], &RegionParamsPatch { delay: Some(7.5), delay_ms: Some(true), ..Default::default() });
        }),
        ("MIDI take folder parameters", |s, w| s.set_region_params(&[w.midi_folder], &RegionParamsPatch { transpose: Some(5), ..Default::default() })),
        ("Pack take folder", |s, w| {
            let ids = region_ids(s, w.a2);
            w.take_folder = s.pack_take_folder(&ids[..2]).unwrap();
        }),
        ("New comp", |s, w| {
            s.create_comp(w.take_folder);
            let f = s.project.track(w.a2).unwrap().regions.iter().find(|r| r.id == w.take_folder).unwrap().clone();
            let first = f.takes.as_ref().unwrap().takes[0].id;
            s.set_comp_range(w.take_folder, Some(first), f.start + 10_000, f.start + 40_000);
        }),
        ("MIDI take", |s, w| {
            let take = s.project.track(w.drums).unwrap().regions.iter().find(|r| r.id == w.midi_folder).unwrap().takes.as_ref().unwrap().takes[0].id;
            s.select_take(w.midi_folder, take);
        }),
        ("Loop with crossfades", |s, w| {
            let r = region_ids(s, w.a1)[1];
            let end = s.project.track(w.a1).unwrap().regions[1].end() + BAR;
            s.set_region_loop(r, Some(end));
            s.set_loop_crossfade(&[r], true);
        }),
        ("Region gain", |s, w| {
            let r = region_ids(s, w.a1)[0];
            s.set_region_gains(&[(r, -4.5)]);
        }),
        ("Arrangement", |s, _| {
            let ids: Vec<Id> = (0..4).map(|_| s.add_arrangement_marker(None)).collect();
            for id in &ids {
                s.resize_arrangement_marker(*id, 2 * BAR);
            }
            s.move_arrangement_marker(ids[2], Some(ids[0]), false).unwrap();
        }),
        ("Marquee delete", |s, w| s.marquee_delete(&[w.a1, w.a2, w.inst], BAR / 2, BAR / 2 + BAR / 4)),
        ("Strip Silence", |s, w| {
            let ids = region_ids(s, w.a1);
            s.strip_silence(&ids, &StripSettings { threshold: 0.01, min_time: 0.1, pre_attack: 0.01, post_release: 0.02 });
        }),
        ("Zero-crossing split", |s, w| {
            s.set_snap_zero_crossings(true);
            let ids = region_ids(s, w.a2);
            s.split_at(4 * BAR + 30_001, &ids);
        }),
        ("Track alternative", |s, w| {
            s.new_track_alternative(w.inst, true).unwrap();
            let r = region_ids(s, w.inst)[0];
            s.transpose_regions(&[r], 2);
        }),
        ("Section cut", |s, _| {
            s.section_edit(daw_engine::arrange_cmds::SectionEdit::Cut, 7 * BAR, 7 * BAR + BAR / 3, None).unwrap();
        }),
    ]
}
