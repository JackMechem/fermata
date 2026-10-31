//! Fermata Sampler through the real session (silent ALSA null output): loading files and mapping
//! them, rendering, recording from a simulated input, converting a region, editing with undo and
//! redo, saving and reopening, moving the project, deleting originals and samples, saving and
//! loading a reusable instrument, Freeze, Track Off, copying, and an SFZ file made here.
//!
//! The null device has no input, so a recording here goes in through `feed_input_for_checks`,
//! which calls the engine as an audio device's callback would with those input samples. That shows
//! the path from the engine's input through the recorder to a playable zone. A real interface is
//! for a person to try.
//!
//! usage: sampler_check <scratch dir>
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use daw_engine::instrument::sampler::params::param;
use daw_engine::instrument::sampler::state::{SamplerState, Zone};
use daw_engine::instrument::sampler::ID as SAMPLER;
use daw_engine::model::{Id, InputSel, Note, RegionContent, TrackKind};
use daw_engine::session::{MidiContent, TrackPatch};
use daw_engine::{audio_io::AudioConfig, Session};

const RATE: i64 = 48000;
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

fn sine(hz: f64, rate: u32, secs: f64, level: f32) -> Vec<f32> {
    (0..(secs * rate as f64) as usize).map(|i| ((i as f64 / rate as f64 * hz * std::f64::consts::TAU).sin() as f32) * level).collect()
}

/// A 16, 24 or 32-bit integer file, or a float one with bits 0.
fn write(path: &Path, rate: u32, ch: &[Vec<f32>], bits: u16) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let float = bits == 0;
    let spec = hound::WavSpec { channels: ch.len() as u16, sample_rate: rate, bits_per_sample: if float { 32 } else { bits }, sample_format: if float { hound::SampleFormat::Float } else { hound::SampleFormat::Int } };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..ch[0].len() {
        for c in ch {
            if float {
                w.write_sample(c[i]).unwrap();
            } else {
                w.write_sample((c[i] as f64 * ((1i64 << (bits - 1)) - 1) as f64).round() as i32).unwrap();
            }
        }
    }
    w.finalize().unwrap();
}

fn play_notes(s: &mut Session, t: Id, notes: &[(i64, u8, u8)]) {
    let r = s.create_midi_region(t, 0, RATE * 2).unwrap();
    let notes = notes.iter().map(|&(start, pitch, velocity)| Note { start, length: RATE / 2, pitch, velocity, muted: false }).collect();
    s.set_midi_content(vec![MidiContent { region: r, notes, cc: Vec::new(), start: None, length: None }]);
}

fn render(s: &mut Session) -> Vec<Vec<f32>> {
    s.render(0, RATE * 2, RATE / 2, None, &mut |_| {}).unwrap()
}

fn same(a: &[Vec<f32>], b: &[Vec<f32>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits()))
}

fn peak(a: &[f32]) -> f32 {
    a.iter().fold(0.0, |m, v| m.max(v.abs()))
}

fn freq(x: &[f32]) -> f64 {
    let (mut first, mut last, mut count) = (None, 0.0f64, 0);
    for i in 1..x.len() {
        if x[i - 1] < 0.0 && x[i] >= 0.0 {
            let at = (i - 1) as f64 + (-x[i - 1] as f64) / (x[i] as f64 - x[i - 1] as f64);
            if first.is_none() {
                first = Some(at);
            } else {
                count += 1;
            }
            last = at;
        }
    }
    count as f64 / ((last - first.unwrap_or(0.0)) / RATE as f64)
}

fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

fn zones_of(s: &Session, t: Id) -> Vec<Zone> {
    s.sampler_view(t).unwrap().zones.into_iter().map(|z| z.zone).collect()
}

fn state_of(s: &Session, t: Id) -> Option<String> {
    s.project.track(t).unwrap().instrument.as_ref().unwrap().state.clone()
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    // Saved instruments go under HOME's Music folder, and presets under the config folder: keep
    // both in here whoever runs this, and never near anyone's real ones.
    std::env::set_var("HOME", scratch.join("home"));
    std::env::set_var("XDG_CONFIG_HOME", scratch.join("home/config"));
    std::env::set_var("XDG_DATA_HOME", scratch.join("home/data"));
    let src = scratch.join("originals");
    let mut s = session();
    let project = scratch.join("Song.fermata");
    s.save(Some(project.clone())).unwrap();

    println!("-- Getting sounds in: dropped files");
    write(&src.join("Vox.wav"), 44100, &[sine(440.0, 44100, 3.0, 0.5)], 16);
    let t = s.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.into()), None).unwrap();
    check("an empty sampler says so, with no zones", zones_of(&s, t).is_empty());
    let added = s.sampler_add_files(t, &[src.join("Vox.wav")], None).unwrap();
    check("one file: one zone", added.zones.len() == 1 && added.errors.is_empty());
    let z = &zones_of(&s, t)[0];
    check("over the whole keyboard, at C3", (z.key_lo, z.key_hi, z.root) == (0, 127, 60));
    check("copied into the project, relatively", z.file == "Samples/Vox.wav" && project.join("Samples/Vox.wav").is_file());
    check("the view knows its file", {
        let v = &s.sampler_view(t).unwrap().zones[0];
        v.rate == 44100 && v.frames == 132300 && v.channels == 1 && v.error.is_none()
    });
    play_notes(&mut s, t, &[(0, 60, 127), (RATE, 72, 127)]);
    let a = render(&mut s);
    check(&format!("it plays at its pitch from a 44.1 kHz, 16-bit file ({:.3} Hz)", freq(&a[0][2000..RATE as usize / 2 - 1000])), cents(freq(&a[0][2000..RATE as usize / 2 - 1000]), 440.0).abs() < 0.05);
    let up = freq(&a[0][RATE as usize + 2000..RATE as usize * 3 / 2 - 1000]);
    check(&format!("and an octave up on C4 ({up:.3} Hz)"), cents(up, 880.0).abs() < 0.05);
    check("a render repeats bit for bit", same(&a, &render(&mut s)));

    println!("-- Mapping several files");
    let m = s.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.into()), None).unwrap();
    for (name, hz, bits, rate) in [("Keys_C3", 261.63, 24u16, 48000u32), ("Keys_C4", 523.25, 32, 96000), ("Keys_C5", 1046.5, 0, 22050)] {
        write(&src.join(format!("{name}.wav")), rate, &[sine(hz, rate, 1.0, 0.4)], bits);
    }
    let paths: Vec<PathBuf> = ["Keys_C3", "Keys_C4", "Keys_C5"].iter().map(|n| src.join(format!("{n}.wav"))).collect();
    s.sampler_add_files(m, &paths, None).unwrap();
    let z = zones_of(&s, m);
    check("three named files split the keyboard by their notes", z.len() == 3 && (z[0].root, z[0].key_hi) == (60, 66) && (z[1].root, z[1].key_lo, z[1].key_hi) == (72, 67, 78) && (z[2].root, z[2].key_lo, z[2].key_hi) == (84, 79, 127));
    write(&src.join("Kick.wav"), 48000, &[sine(60.0, 48000, 0.3, 0.8)], 16);
    write(&src.join("Snare.wav"), 48000, &[sine(180.0, 48000, 0.2, 0.6)], 16);
    let d = s.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.into()), None).unwrap();
    s.sampler_add_files(d, &[src.join("Kick.wav"), src.join("Snare.wav")], Some(36)).unwrap();
    let z = zones_of(&s, d);
    check("files without notes go chromatically from where they're dropped", (z[0].key_lo, z[0].root, z[1].key_lo, z[1].key_hi) == (36, 36, 37, 37));
    let bad = s.sampler_add_files(d, &[src.join("Nothing.wav"), src.join("Kick.wav")], None).unwrap();
    check("a file that isn't there is reported, the rest still come", bad.errors.len() == 1 && bad.zones.len() == 1);
    s.delete_track(d);
    s.delete_track(m);

    println!("-- Zones, layers, round robin, editing, undo");
    s.checkpoint("Add", None, true);
    write(&src.join("Soft.wav"), 48000, &[vec![0.1; 48000]], 0);
    write(&src.join("Loud.wav"), 48000, &[vec![0.4; 48000]], 0);
    s.sampler_add_files(t, &[src.join("Soft.wav"), src.join("Loud.wav")], Some(90)).unwrap();
    let mut z = zones_of(&s, t);
    let soft = z.iter().position(|x| x.name == "Soft").unwrap();
    let loud = z.iter().position(|x| x.name == "Loud").unwrap();
    let vox_at = z.iter().position(|x| x.name == "Vox").unwrap();
    z[vox_at].key_hi = 80;
    let vox_only = z[vox_at].clone();
    z[soft].key_hi = 90;
    z[soft].vel_hi = 63;
    z[loud].key_lo = 90;
    z[loud].key_hi = 90;
    z[loud].root = 90;
    z[loud].vel_lo = 64;
    s.sampler_update_zones(t, vec![vox_only, z[soft].clone(), z[loud].clone()]).unwrap();
    let before_edit = state_of(&s, t);
    s.set_builtin_param(t, SLOT, param::VEL_AMP, 0.0).unwrap();
    let r2 = s.create_midi_region(t, RATE * 2, RATE).unwrap();
    s.set_midi_content(vec![MidiContent { region: r2, notes: vec![Note { start: 0, length: RATE / 4, pitch: 90, velocity: 40, muted: false }, Note { start: RATE / 2, length: RATE / 4, pitch: 90, velocity: 120, muted: false }], cc: Vec::new(), start: None, length: None }]);
    let layers = s.render(RATE * 2, RATE * 3, 0, None, &mut |_| {}).unwrap();
    let (quiet, full) = (layers[0][RATE as usize / 8], layers[0][RATE as usize * 5 / 8]);
    check(&format!("velocity layers: {quiet:.3} then {full:.3}"), (quiet - 0.1).abs() < 0.01 && (full - 0.4).abs() < 0.01);
    s.checkpoint("Move root", None, true);
    let base = render(&mut s);
    let mut z = zones_of(&s, t);
    z[0].root = 72;
    z[0].gain_db = -6.0;
    s.sampler_update_zones(t, vec![z[0].clone()]).unwrap();
    let moved = render(&mut s);
    check("an edit changes the sound", !same(&base, &moved));
    s.undo();
    check("undo puts the zones and the sound back, bit for bit", state_of(&s, t) == before_edit && same(&base, &render(&mut s)));
    s.redo();
    check("redo brings the edit back", same(&moved, &render(&mut s)));
    s.undo();
    for _ in 0..50 {
        s.idle();
    }
    check("maps replaced by those edits get freed once nothing plays them", s.sampler_retired() == 0);

    println!("-- Recording from a simulated input");
    let rec = s.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.into()), None).unwrap();
    s.sampler_record_start(rec, InputSel::Mono(1)).unwrap();
    let hz = 330.0;
    let mut sung = vec![0.0f32; RATE as usize];
    sung.extend(sine(hz, RATE as u32, 2.0, 0.6));
    sung.extend(vec![0.0f32; RATE as usize / 2]);
    // Two input channels, the sound on the second, a block at a time as a device would hand them.
    for block in sung.chunks(256) {
        let inter: Vec<f32> = block.iter().flat_map(|v| [0.9, *v]).collect();
        s.feed_input_for_checks(&inter, 2);
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    check("the recording in progress can be seen", s.sampler_recording().is_some_and(|r| r.frames > 0));
    let zone = s.sampler_record_stop(true, None).unwrap();
    let z = zones_of(&s, rec);
    let got = z.iter().find(|x| x.id == zone).unwrap();
    check(&format!("recorded into a zone: {} to {}", got.start, got.end), z.len() == 1 && got.file.starts_with("Samples/Recording"));
    check("the silence around it is trimmed", (got.start as i64 - RATE).abs() < 400 && (got.end as i64 - 3 * RATE).abs() < 400);
    let rel = got.file.clone();
    check("the file is the whole take", hound::WavReader::open(s.project_path().join(&rel)).unwrap().duration() == sung.len() as u32);
    play_notes(&mut s, rec, &[(0, 60, 127)]);
    s.update_track(t, TrackPatch { mute: Some(true), ..Default::default() });
    let out = s.render(0, RATE / 2, 0, None, &mut |_| {}).unwrap();
    let f = freq(&out[0][4000..RATE as usize / 2 - 1000]);
    check(&format!("it's playable straight away, from the right channel, in tune ({f:.3} Hz)"), cents(f, hz).abs() < 0.2);
    check("and its level is what came in", (peak(&out[0][4000..]) - 0.6).abs() < 0.03);
    s.sampler_record_start(rec, InputSel::Stereo(0)).unwrap();
    for _ in 0..10 {
        s.feed_input_for_checks(&vec![0.0f32; 512], 2);
    }
    check("a silent take is refused and says so", s.sampler_record_stop(false, None).is_err());
    s.sampler_record_start(rec, InputSel::Mono(0)).unwrap();
    s.feed_input_for_checks(&vec![0.5f32; 512], 1);
    let files = std::fs::read_dir(project.join("Samples")).unwrap().count();
    s.sampler_record_cancel();
    check("a cancelled take leaves no file", std::fs::read_dir(project.join("Samples")).unwrap().count() == files - 1);
    s.delete_track(rec);
    s.update_track(t, TrackPatch { mute: Some(false), ..Default::default() });

    println!("-- Converting a region");
    let mut drums = vec![0.0f32; RATE as usize * 2];
    let hits = [4800usize, 28800, 52800, 76800];
    for &h in &hits {
        for (i, v) in drums[h..h + 12000].iter_mut().enumerate() {
            *v += (i as f32 * 0.05).sin() * (-(i as f32) / 1500.0).exp() * 0.7;
        }
    }
    write(&src.join("Loop.wav"), RATE as u32, &[drums.clone()], 0);
    let at = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let region = s.import_audio(&src.join("Loop.wav"), at, 0).unwrap();
    s.update_track(t, TrackPatch { mute: Some(true), ..Default::default() });
    let audio_render = s.render(0, RATE * 2, 0, None, &mut |_| {}).unwrap();
    s.checkpoint("Convert to Sampler Track", None, true);
    let new = s.convert_region_to_sampler(region, None).unwrap();
    let nt = s.project.track(new).unwrap();
    check("a new instrument track with a sampler, below the original", nt.kind == TrackKind::Instrument && nt.instrument.as_ref().unwrap().plugin_id == SAMPLER && s.project.tracks.iter().position(|x| x.id == new) == s.project.tracks.iter().position(|x| x.id == at).map(|p| p + 1));
    check("the original region is muted", s.project.track(at).unwrap().regions[0].muted);
    check("a MIDI region stands in its place", matches!(&nt.regions[0].content, RegionContent::Midi { notes, .. } if notes.len() == 1 && notes[0].pitch == 60) && nt.regions[0].start == 0);
    let converted = s.render(0, RATE * 2, 0, None, &mut |_| {}).unwrap();
    let err = audio_render[0].iter().zip(&converted[0]).take(RATE as usize * 3 / 2).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    check(&format!("it plays what the region played (differs by {err:.4} at most)"), err < 0.05);
    s.undo();
    check("undo takes the track away and unmutes the region", s.project.track(new).is_none() && !s.project.track(at).unwrap().regions[0].muted);
    let new = s.convert_region_to_sampler(region, Some(0.5)).unwrap();
    let z = zones_of(&s, new);
    check(&format!("by transients: {} slices on keys from C1", z.len()), z.len() == 4 && z[0].root == 36 && z[3].key_lo == 39 && z.iter().all(|x| x.one_shot));
    check("slices begin at the hits", z.iter().zip(&hits).all(|(x, &h)| (x.start as i64 - h as i64).abs() < 100 || x.start == 0));
    let sliced = s.render(0, RATE * 2, 0, None, &mut |_| {}).unwrap();
    let hit = |x: &[f32], h: usize| peak(&x[h..h + 3000]);
    check("the slices play back in time", hits.iter().all(|&h| (hit(&sliced[0], h) - hit(&audio_render[0], h)).abs() < 0.1));
    let new_keys = zones_of(&s, new);
    s.sampler_slice(new, new_keys[0].id, 0.5).unwrap();
    check("slicing a slice with one hit leaves one", zones_of(&s, new).len() == 4);
    s.delete_track(new);
    s.delete_track(at);
    s.update_track(t, TrackPatch { mute: Some(false), ..Default::default() });

    println!("-- Saving, moving, deleting");
    let before = render(&mut s);
    let want_state = state_of(&s, t);
    s.save(None).unwrap();
    s.open(&project).unwrap();
    check("reopened, the zones are as they were", state_of(&s, t) == want_state);
    check("and the sound is the same, bit for bit", same(&before, &render(&mut s)));
    for f in std::fs::read_dir(&src).unwrap().flatten() {
        std::fs::remove_file(f.path()).unwrap();
    }
    s.open(&project).unwrap();
    check("with every original deleted it still plays, bit for bit", same(&before, &render(&mut s)));
    let moved_dir = scratch.join("Elsewhere/Moved.fermata");
    std::fs::create_dir_all(moved_dir.parent().unwrap()).unwrap();
    s.new_project();
    std::fs::rename(&project, &moved_dir).unwrap();
    s.open(&moved_dir).unwrap();
    check("moving the project folder changes nothing", same(&before, &render(&mut s)));
    let saved_as = scratch.join("Copy.fermata");
    s.save(Some(saved_as.clone())).unwrap();
    check("Save As brings the samples along", saved_as.join("Samples/Vox.wav").is_file());
    check("and the copy plays the same", same(&before, &render(&mut s)));

    println!("-- A missing sample");
    let z = zones_of(&s, t);
    let vox = z.iter().find(|x| x.name == "Vox").unwrap().clone();
    s.save(None).unwrap();
    std::fs::remove_file(saved_as.join("Samples/Soft.wav")).unwrap();
    s.open(&saved_as).unwrap();
    let warning = s.tick().warning.unwrap_or_default();
    check(&format!("opening says which: \"{warning}\""), warning.contains("Soft.wav") && warning.contains("missing"));
    let v = s.sampler_view(t).unwrap();
    let flagged: Vec<&str> = v.zones.iter().filter(|z| z.error.is_some()).map(|z| z.zone.name.as_str()).collect();
    check("the panel's view marks that zone", flagged == ["Soft"]);
    let out = render(&mut s);
    check("the rest aren't silenced", peak(&out[0][..RATE as usize / 2]) > 0.3);
    let soft = z.iter().find(|x| x.name == "Soft").unwrap();
    std::fs::create_dir_all(&src).unwrap();
    write(&src.join("Soft Again.wav"), 48000, &[vec![0.1; 48000]], 0);
    s.sampler_replace_file(t, soft.id, &src.join("Soft Again.wav")).unwrap();
    check("replacing the file mends it", s.sampler_view(t).unwrap().zones.iter().all(|z| z.error.is_none()));

    println!("-- A reusable instrument");
    let mut shaped = vox.clone();
    shaped.loop_on = true;
    shaped.loop_start = 44100;
    shaped.loop_end = 44100 + 44100;
    shaped.xfade = 2000;
    s.sampler_update_zones(t, vec![shaped]).unwrap();
    s.set_builtin_param(t, SLOT, param::A_RELEASE, 300.0).unwrap();
    let name = s.save_preset(t, SLOT, "My Keys", false).unwrap();
    let home_doc = scratch.join("home/Music/Fermata Instruments/My Keys/instrument.json");
    check("saved in the Music folder, the folder made on first save", name == "My Keys" && home_doc.is_file());
    check("with a copy of its samples", scratch.join("home/Music/Fermata Instruments/My Keys/Samples/Vox.wav").is_file());
    check("listed in the preset menu", daw_engine::presets::list(&daw_engine::presets::user_root(), SAMPLER).user == vec!["My Keys".to_string()]);
    check("an existing one isn't replaced without asking", s.save_preset(t, SLOT, "My Keys", false).is_err());
    s.save_preset(t, SLOT, "Spare", false).unwrap();
    let root = daw_engine::presets::user_root();
    daw_engine::presets::rename_user(&root, SAMPLER, "Spare", "Renamed").unwrap();
    check("renaming takes its samples along", scratch.join("home/Music/Fermata Instruments/Renamed/Samples/Vox.wav").is_file() && daw_engine::presets::list(&root, SAMPLER).user.contains(&"Renamed".to_string()));
    daw_engine::presets::delete_user(&root, SAMPLER, "Renamed").unwrap();
    check("deleting takes them too", !scratch.join("home/Music/Fermata Instruments/Renamed").exists());
    let with_it = render(&mut s);
    s.new_project();
    let other = s.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.into()), None).unwrap();
    let other_dir = scratch.join("Other.fermata");
    s.save(Some(other_dir.clone())).unwrap();
    s.load_preset(other, SLOT, SAMPLER, "My Keys", false).unwrap();
    check("another project loads it, with its zones", zones_of(&s, other).iter().any(|z| z.name == "Vox" && z.loop_on));
    let vox2 = zones_of(&s, other).into_iter().find(|z| z.name == "Vox").unwrap();
    check("copied into that project", other_dir.join(&vox2.file).is_file());
    check("and its parameters", s.builtin_params(s.project.track(other).unwrap().instrument.as_ref().unwrap().uid).iter().any(|p| p.0 == param::A_RELEASE && p.1 == 300.0));
    play_notes(&mut s, other, &[(0, 60, 127), (RATE, 72, 127)]);
    let there = render(&mut s);
    let err = with_it[0].iter().zip(&there[0]).take(RATE as usize / 4).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    check(&format!("and it sounds the same there ({err:.5})"), err < 1e-4);
    s.save(None).unwrap();
    std::fs::remove_dir_all(scratch.join("home/Music/Fermata Instruments/My Keys")).unwrap();
    s.open(&other_dir).unwrap();
    check("that project doesn't need the instrument's folder any more", peak(&render(&mut s)[0]) > 0.1 && s.sampler_view(other).unwrap().zones.iter().all(|z| z.error.is_none()));
    let t = other;

    println!("-- Factory presets keep the zones");
    let zones = state_of(&s, t);
    s.load_preset(t, SLOT, SAMPLER, "Slow Pad", true).unwrap();
    check("a factory preset changes the sound's shape, not what's mapped", state_of(&s, t) == zones);
    s.load_preset(t, SLOT, SAMPLER, "Default", true).unwrap();

    println!("-- Freeze, Track Off, copying");
    let live = render(&mut s);
    s.freeze_tracks(&[t], &mut |_| {}).unwrap();
    let frozen = render(&mut s);
    let off = frozen[0].iter().zip(&live[0]).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    check(&format!("a frozen sampler plays what it played live ({off:.5})"), off < 1e-3);
    check("zone edits are refused while frozen", s.sampler_delete_zones(t, &[1]).is_err());
    s.unfreeze_track(t).unwrap();
    check("unfrozen, it's back with its zones", state_of(&s, t) == zones && same(&live, &render(&mut s)));
    s.update_track(t, TrackPatch { off: Some(true), ..Default::default() });
    check("Off, it's silent", peak(&render(&mut s)[0]) == 0.0);
    s.update_track(t, TrackPatch { off: Some(false), ..Default::default() });
    check("back on, it plays the same", same(&live, &render(&mut s)));
    let copy = s.copy_plugin(t, SLOT).unwrap();
    let b = s.add_track(TrackKind::Instrument, InputSel::None, Some("builtin:synth".into()), None).unwrap();
    s.paste_plugin(&[b], SLOT, &copy).unwrap();
    check("pasting a sampler brings its zones", zones_of(&s, b).len() == zones_of(&s, t).len());
    let dup = s.duplicate_tracks(&[t], true)[0];
    check("so does duplicating its track", zones_of(&s, dup).len() == zones_of(&s, t).len());
    s.delete_track(dup);
    s.delete_track(b);

    println!("-- SFZ");
    let lib = scratch.join("library/Lib");
    write(&lib.join("wavs/low.wav"), 44100, &[sine(220.0, 44100, 2.0, 0.5)], 16);
    write(&lib.join("wavs/high one.wav"), 48000, &[sine(880.0, 48000, 2.0, 0.5)], 24);
    let lp = lib.join("wavs/loop.wav");
    write(&lp, 48000, &[(0..48000).map(|i| ((i % 480) as f32 / 480.0 * std::f32::consts::TAU).sin() * 0.5).collect()], 0);
    write(&lib.join("wavs/rr1.wav"), 48000, &[vec![0.3; 24000]], 0);
    write(&lib.join("wavs/rr2.wav"), 48000, &[vec![0.15; 24000]], 0);
    let sfz = lib.join("Inst.sfz");
    std::fs::write(
        &sfz,
        "<control> default_path=WAVS/\n<global> ampeg_release=0.05\n<group> volume=-6 cutoff=2000\n\
         <region> sample=low.wav lokey=c3 hikey=b3 pitch_keycenter=a3\n\
         <region> sample=high one.wav lokey=c4 hikey=b5 pitch_keycenter=a5 tune=50\n\
         <group> loop_mode=loop_continuous loop_start=4800 loop_end=9599\n<region> sample=loop.wav key=30 pitch_keycenter=30\n\
         <group> key=40 seq_length=2 ampeg_release=0.01\n<region> sample=rr1.wav seq_position=1\n<region> sample=rr2.wav seq_position=2\n\
         <region> sample=gone.wav key=41\n<region> sample=rr1.wav key=42 trigger=release\n",
    )
    .unwrap();
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, Some(SAMPLER.into()), None).unwrap();
    let report = s.sampler_load_sfz(inst, &sfz).unwrap();
    check(&format!("{} regions in; ignored {:?}; left out {:?}; missing {:?}", report.zones, report.ignored, report.left_out, report.missing), report.zones == 5);
    check("what it ignored is listed", report.ignored.iter().any(|(k, n)| k == "cutoff" && *n >= 1));
    check("a region it can't do is left out and said", report.left_out.len() == 1 && report.left_out[0].contains("trigger=release"));
    check("a missing sample in the file is said", report.missing == ["WAVS/gone.wav".to_string()]);
    let z = zones_of(&s, inst);
    check("keys, velocities, root, tune and volume read", z[0].key_lo == 48 && z[0].key_hi == 59 && z[0].root == 57 && z[0].gain_db == -6.0 && z[1].fine == 50.0);
    check("its samples are in the project", s.project_path().join(&z[0].file).is_file() && z[0].file.starts_with("Samples/Inst/"));
    check("its envelope is the zone's own", z[0].env == Some([0.0, 0.0, 1.0, 0.05]));
    s.update_track(t, TrackPatch { mute: Some(true), ..Default::default() });
    let r = s.create_midi_region(inst, 0, RATE * 2).unwrap();
    let n = |start: i64, pitch: u8| Note { start, length: RATE / 4, pitch, velocity: 127, muted: false };
    s.set_midi_content(vec![MidiContent { region: r, notes: vec![n(0, 57), n(RATE / 2, 69), n(RATE, 30), n(RATE * 3 / 2, 40), n(RATE * 7 / 4, 40)], cc: Vec::new(), start: None, length: None }]);
    let out = s.render(0, RATE * 2, 0, None, &mut |_| {}).unwrap();
    let f = freq(&out[0][2000..RATE as usize / 4 - 2000]);
    check(&format!("SFZ's C4 = 60 and a 44.1 kHz sample: A3 at {f:.3} Hz, at -6 dB"), cents(f, 220.0).abs() < 0.1 && (peak(&out[0][2000..RATE as usize / 4 - 2000]) - 0.25).abs() < 0.02);
    let f = freq(&out[0][RATE as usize / 2 + 2000..RATE as usize * 3 / 4 - 2000]);
    let want = 880.0 * 2f64.powf((-12.0 + 0.5) / 12.0);
    check(&format!("tune in cents: {f:.3} Hz against {want:.3}"), cents(f, want).abs() < 0.1);
    let f = freq(&out[0][RATE as usize + 2000..RATE as usize * 5 / 4 - 2000]);
    check(&format!("a loop past its file's start: {f:.3} Hz"), (f - 100.0).abs() < 0.05);
    let first = out[0][RATE as usize * 3 / 2 + 6000];
    let second = out[0][RATE as usize * 7 / 4 + 6000];
    check(&format!("sequence positions take turns: {first:.3} then {second:.3}"), (first - 0.3).abs() < 0.01 && (second - 0.15).abs() < 0.01);
    s.save(None).unwrap();
    std::fs::remove_dir_all(scratch.join("library")).unwrap();
    s.open(&other_dir).unwrap();
    check("with the library gone it still opens and plays", s.sampler_view(inst).unwrap().zones.iter().all(|z| z.error.is_none()) && peak(&s.render(0, RATE, 0, None, &mut |_| {}).unwrap()[0]) > 0.1);

    println!("-- Time a load takes");
    let big = scratch.join("big.wav");
    write(&big, 48000, &[sine(100.0, 48000, 120.0, 0.5), sine(101.0, 48000, 120.0, 0.5)], 24);
    let mb = std::fs::metadata(&big).unwrap().len() as f64 / 1e6;
    let time = std::time::Instant::now();
    s.sampler_add_files(inst, std::slice::from_ref(&big), Some(100)).unwrap();
    println!("     adding a 2 minute stereo 24-bit WAV ({mb:.0} MB): {:.0} ms (copy, decode and map)", time.elapsed().as_secs_f64() * 1000.0);
    let time = std::time::Instant::now();
    let z = zones_of(&s, inst);
    let mut edited = z[0].clone();
    edited.gain_db = -1.0;
    s.sampler_update_zones(inst, vec![edited]).unwrap();
    println!("     editing a zone with that loaded: {:.1} ms", time.elapsed().as_secs_f64() * 1000.0);
    let state: SamplerState = serde_json::from_str(&state_of(&s, inst).unwrap()).unwrap();
    check("memory used is reported", s.sampler_view(inst).unwrap().memory > 40_000_000 && state.zones.len() == 6);

    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
    println!("all passed");
}
