//! Take folder comps, MIDI take folders and crossfades for take folders and loops, against a
//! running engine (silent ALSA null output):
//! - several comps per folder: create, switch, rename and delete them, with undo, and each comp
//!   renders as it should;
//! - MIDI cycle recording with "Create take folders" (notes played in through the same path as
//!   Musical Typing) makes a folder with a take per pass, and choosing a take changes what
//!   plays;
//! - a take folder crossfaded with the next region renders without a click, and so do a loop's
//!   repetitions with Crossfade Loop Repetitions on;
//! - Settings › Editing › Crossfade Time sets the X-Fade drag mode's crossfades and the comp
//!   boundaries.
//!
//! usage: comping_check <scratch dir>
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::fades::{DragMode, FadeOutType};
use daw_engine::model::{Id, InputSel, Region, RegionContent, TrackKind};
use daw_engine::prefs::MidiOverlap;
use daw_engine::session::TrackPatch;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn write_wav(path: &Path, sr: u32, frames: usize, f: impl Fn(usize) -> f32) {
    let spec = hound::WavSpec { channels: 1, sample_rate: sr, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..frames {
        w.write_sample(f(i)).unwrap();
    }
    w.finalize().unwrap();
}

fn region(s: &Session, id: Id) -> Region {
    s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|r| r.id == id).cloned().expect("region")
}

/// The left channel of the full mix over `from..to`.
fn render(s: &mut Session, from: i64, to: i64) -> Vec<f32> {
    s.render(from, to, 0, None, &mut |_| {}).unwrap().swap_remove(0)
}

/// The largest jump from one sample to the next: a click shows up as a big one.
fn max_step(a: &[f32]) -> f32 {
    a.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

/// Upward zero crossings in `a`.
fn crossings(a: &[f32]) -> usize {
    a.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
}

/// Upward zero crossings per 1000 samples over the part of `a` that sounds: the pitch,
/// whatever the note's length (live-recorded notes start anywhere within a beat).
fn crossing_rate(a: &[f32]) -> f32 {
    let on = |v: &f32| v.abs() > 1e-4;
    let (Some(first), Some(last)) = (a.iter().position(on), a.iter().rposition(on)) else { return 0.0 };
    crossings(&a[first..=last]) as f32 * 1000.0 / (last - first + 1) as f32
}

/// Samples that are neither of the two levels: the length of a crossfade between them.
fn between(a: &[f32], x: f32, y: f32) -> usize {
    a.iter().filter(|v| (**v - x).abs() > 1e-3 && (**v - y).abs() > 1e-3).count()
}

/// A region on `track` from `file`, trimmed to `start..start + length` at file frame `offset`.
fn place(s: &mut Session, track: Id, file: &Path, start: i64, length: i64, offset: i64) -> Id {
    let r = s.import_audio(file, track, start - offset).unwrap();
    s.resize_region(r, start, length);
    r
}

/// Waits until the playhead is in `lo..hi`, polling as the null device races along.
fn wait_for(s: &Session, lo: i64, hi: i64) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let p = s.position();
        if p >= lo && p < hi {
            return;
        }
        std::thread::sleep(Duration::from_micros(200));
    }
    panic!("the playhead never reached {lo}..{hi}");
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("CompingCheck.fermata");
    let _ = std::fs::remove_dir_all(&s.dir);
    std::fs::create_dir_all(s.dir.join("Audio Files")).unwrap();
    let sr = s.sample_rate() as u32;
    let j = sr as i64;
    let ms = |m: f64| (m * sr as f64 / 1000.0).round() as i64;

    // Four-second files: two DC levels, a 1 kHz sine and a ramp from -0.5 to 0.5.
    let gen = scratch.join("comping-sources");
    std::fs::create_dir_all(&gen).unwrap();
    let n = 4 * sr as usize;
    let (hi_dc, lo_dc, sine, ramp) = (gen.join("dc5.wav"), gen.join("dc25.wav"), gen.join("sine.wav"), gen.join("ramp.wav"));
    write_wav(&hi_dc, sr, n, |_| 0.5);
    write_wav(&lo_dc, sr, n, |_| 0.25);
    write_wav(&sine, sr, n, |i| 0.5 * (std::f32::consts::TAU * 1000.0 * i as f32 / sr as f32).sin());
    write_wav(&ramp, sr, n, |i| i as f32 / n as f32 - 0.5);

    // ---- Several comps per take folder ------------------------------------------------------
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let a = place(&mut s, t, &hi_dc, j, 2 * j, 0);
    let b = place(&mut s, t, &lo_dc, j, 2 * j, 0);
    let folder = s.pack_take_folder(&[a, b]).unwrap();
    let mid = |v: &[f32], at: i64| v[(at - j) as usize];
    let out = render(&mut s, j, 3 * j);
    check(&format!("a packed folder plays the newest take ({})", mid(&out, j + j / 2)), (mid(&out, j + j / 2) - 0.25).abs() < 1e-4);
    let takes: Vec<Id> = region(&s, folder).takes.unwrap().takes.iter().map(|k| k.id).collect();

    s.checkpoint("Create New Comp", None, false);
    s.create_comp(folder);
    // Swipe the first take over the first second of Comp 2.
    s.checkpoint("Edit Comp", None, false);
    s.set_comp_range(folder, Some(takes[0]), j, 2 * j);
    let f = region(&s, folder).takes.unwrap();
    check(&format!("Create New Comp makes Comp 2 the one heard ({:?})", f.comp_names()), f.comp_names() == ["Comp 1", "Comp 2"] && f.current == 1);
    let comp2 = render(&mut s, j, 3 * j);
    check(
        &format!("Comp 2 plays take 1 then take 2 ({}, {})", mid(&comp2, j + j / 2), mid(&comp2, 2 * j + j / 2)),
        (mid(&comp2, j + j / 2) - 0.5).abs() < 1e-4 && (mid(&comp2, 2 * j + j / 2) - 0.25).abs() < 1e-4,
    );
    s.checkpoint("Select Comp", None, false);
    s.select_comp(folder, 0);
    let comp1 = render(&mut s, j, 3 * j);
    check("switching back to Comp 1 plays take 2 throughout", comp1.iter().all(|v| (v - 0.25).abs() < 1e-4));
    s.undo();
    let f = region(&s, folder).takes.unwrap();
    check("undo switches back to Comp 2", f.current == 1 && render(&mut s, j, 3 * j) == comp2);
    s.checkpoint("Duplicate Comp", None, false);
    s.duplicate_comp(folder);
    s.checkpoint("Rename Comp", None, false);
    s.rename_comp(folder, 2, "Lead".into());
    let f = region(&s, folder).takes.unwrap();
    check(&format!("Duplicate Comp copies the comp heard and Rename names it ({:?})", f.comp_names()), f.comp_names() == ["Comp 1", "Comp 2", "Lead"] && f.comp_name() == "Lead" && render(&mut s, j, 3 * j) == comp2);
    s.checkpoint("Delete Comp", None, false);
    s.delete_comp(folder, 2);
    s.delete_comp(folder, 1);
    let f = region(&s, folder).takes.unwrap();
    check(&format!("deleting comps falls back to Comp 1 ({:?})", f.comp_names()), f.comp_names() == ["Comp 1"] && render(&mut s, j, 3 * j) == comp1);
    s.undo();
    check("undo brings the deleted comps back", region(&s, folder).takes.unwrap().comps.len() == 3);
    // Flatten uses the comp heard.
    s.checkpoint("Flatten", None, false);
    s.select_comp(folder, 1);
    let made = s.flatten_take_folder(folder);
    let flat = render(&mut s, j, 3 * j);
    check(&format!("Flatten keeps the comp heard ({} regions)", made.len()), made.len() == 2 && between(&flat, 0.5, 0.25) < ms(1.0) as usize);
    s.undo();

    // ---- Crossfade Time: comp boundaries and the X-Fade drag mode ---------------------------
    // (The few samples at the crossfade's ends that are still within a hair of a level don't
    // count, so the width comes out a little short.)
    let width = |s: &mut Session| between(&render(s, j, 3 * j), 0.5, 0.25) as i64;
    s.prefs.editing.crossfade_ms = 20.0;
    let w20 = width(&mut s);
    let mut p = s.prefs.clone();
    p.editing.crossfade_ms = 50.0;
    s.set_prefs(p);
    let w50 = width(&mut s);
    check(&format!("the comp boundary crossfades over the Crossfade Time ({w20} and {w50} frames)"), (w20 - ms(20.0)).abs() <= ms(20.0) / 30 && (w50 - ms(50.0)).abs() <= ms(50.0) / 30);
    s.delete_track(t);

    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let a = place(&mut s, t, &hi_dc, j, j, 0);
    let b = place(&mut s, t, &lo_dc, 3 * j, j, j);
    s.set_drag_mode(DragMode::XFade);
    let fade_out = |s: &Session, id: Id| match region(s, id).content {
        RegionContent::Audio { fade_out, .. } => fade_out,
        _ => 0,
    };
    s.move_region(b, 2 * j, None);
    let x50 = fade_out(&s, a);
    s.move_region(b, 3 * j, None);
    let mut p = s.prefs.clone();
    p.editing.crossfade_ms = 20.0;
    s.set_prefs(p);
    s.move_region(b, 2 * j, None);
    check(&format!("X-Fade crossfades butted regions over the Crossfade Time ({x50}, then {})", fade_out(&s, a)), x50 == ms(50.0) && fade_out(&s, a) == ms(20.0));
    s.set_drag_mode(DragMode::Overlap);
    s.delete_track(t);

    // ---- A take folder crossfaded with its neighbour -----------------------------------------
    // The folder plays the sine up to 2 s; the next region carries on with the same sine. A
    // linear X crossfade between two copies of one signal gives that signal back, so any
    // click or dip shows as a difference.
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let a = place(&mut s, t, &lo_dc, j, j, j);
    let b = place(&mut s, t, &sine, j, j, j);
    let folder = s.pack_take_folder(&[a, b]).unwrap();
    let next = place(&mut s, t, &sine, 2 * j, j, 2 * j);
    s.checkpoint("Crossfade", None, false);
    s.crossfade_regions(&[folder], ms(50.0), Some(FadeOutType::X));
    s.set_region_fade_shape(&[folder], None, Some(0.0), None, None);
    let xs = s.crossfades();
    check(&format!("the folder crossfades into the next region ({xs:?})"), xs.len() == 1 && xs[0].out_region == folder && xs[0].in_region == next);
    let out = render(&mut s, j, 3 * j);
    let expect = |i: usize| 0.5 * (std::f32::consts::TAU * 1000.0 * (i as i64 + j) as f32 / sr as f32).sin();
    let zone = ((j - ms(100.0)) as usize)..((j + ms(100.0)) as usize);
    let worst = zone.clone().map(|i| (out[i] - expect(i)).abs()).fold(0.0, f32::max);
    check(&format!("it renders the sine unbroken across the crossfade (off by {worst})"), worst < 1e-3);
    // Into a level of 0.5 instead: the crossfade smooths the step the butt joint has.
    s.undo();
    s.delete_regions(&[next]);
    place(&mut s, t, &hi_dc, 2 * j, j, 2 * j);
    let step_butt = max_step(&render(&mut s, j, 3 * j)[zone.clone()]);
    s.crossfade_regions(&[folder], ms(50.0), Some(FadeOutType::X));
    let step_x = max_step(&render(&mut s, j, 3 * j)[zone.clone()]);
    check(&format!("into a different signal it doesn't click (largest step {step_x} against {step_butt} butted)"), step_x < 0.1 && step_butt > 0.3);
    s.delete_track(t);

    // ---- A loop's repetitions and its neighbour at the loop end ----------------------------------
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    // Half a second of the ramp from 1 s into the file, looping four times from 1 s.
    let lp = place(&mut s, t, &ramp, j, j / 2, j);
    s.set_region_loop(lp, Some(3 * j));
    let seam = ((j / 2 - ms(30.0)) as usize)..((j / 2 + ms(30.0)) as usize);
    let plain = render(&mut s, j, 3 * j);
    s.checkpoint("Crossfade Loop Repetitions", None, false);
    s.set_loop_crossfade(&[lp], true);
    let smooth = render(&mut s, j, 3 * j);
    check(
        &format!("Crossfade Loop Repetitions smooths the seams ({} against {})", max_step(&smooth[seam.clone()]), max_step(&plain[seam.clone()])),
        max_step(&plain[seam.clone()]) > 0.1 && max_step(&smooth[seam.clone()]) < 0.01,
    );
    s.undo();
    check("undo takes it off again", render(&mut s, j, 3 * j) == plain);
    // A region right at the loop end, crossfaded from the last repetition.
    let after = place(&mut s, t, &lo_dc, 3 * j, j / 2, 0);
    s.crossfade_regions(&[lp], ms(20.0), Some(FadeOutType::X));
    let xs = s.crossfades();
    check(&format!("the loop crossfades into the region at its loop end ({xs:?})"), xs.iter().any(|x| x.out_region == lp && x.in_region == after));
    let end = ((2 * j - ms(40.0)) as usize)..((2 * j + ms(40.0)) as usize);
    let out = render(&mut s, j, 4 * j);
    check(&format!("and doesn't click there (largest step {})", max_step(&out[end.clone()])), max_step(&out[end]) < 0.05);
    s.delete_track(t);

    // ---- MIDI cycle recording into a take folder --------------------------------------------
    s.prefs.recording.pre_roll_secs = 0.0;
    s.set_count_in(false);
    let bar = s.project.samples_per_bar().round() as i64;
    let beat = s.project.samples_per_beat().round() as i64;
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.select_track(Some(inst));
    s.update_track(inst, TrackPatch { armed: Some(true), ..Default::default() });
    s.prefs.recording.midi_overlap = MidiOverlap::TakeFolders;
    s.set_cycle(true, bar, 2 * bar);
    s.record().unwrap();
    // One note per pass, a fifth and then an octave higher each time.
    for pitch in [60u8, 67, 72] {
        wait_for(&s, bar + beat, bar + 2 * beat);
        s.send_midi([0x90, pitch, 100]);
        wait_for(&s, bar + 3 * beat, 2 * bar);
        s.send_midi([0x80, pitch, 0]);
        wait_for(&s, bar, bar + beat);
    }
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    s.set_cycle(false, bar, 2 * bar);
    let regions = s.project.track(inst).unwrap().regions.clone();
    let folder = regions.iter().find(|r| r.takes.is_some());
    let count = folder.map(|f| f.takes.as_ref().unwrap().takes.len()).unwrap_or(0);
    check(&format!("three passes make a MIDI take folder with three takes ({} regions, {count} takes)", regions.len()), regions.len() == 1 && count == 3);
    let Some(folder) = folder.cloned() else {
        std::process::exit(1);
    };
    let pitches = |s: &Session| match region(s, folder.id).content {
        RegionContent::Midi { notes, .. } => notes.iter().map(|n| n.pitch).collect::<Vec<u8>>(),
        _ => Vec::new(),
    };
    check(&format!("the newest take plays ({:?})", pitches(&s)), pitches(&s) == [72]);
    let newest = render(&mut s, bar + beat, bar + 3 * beat);
    let takes: Vec<Id> = folder.takes.as_ref().unwrap().takes.iter().map(|k| k.id).collect();
    s.checkpoint("Select Take", None, false);
    s.select_take(folder.id, takes[0]);
    let first = render(&mut s, bar + beat, bar + 3 * beat);
    check(
        &format!(
            "choosing take 1 plays it, an octave lower ({:?}: {:.2} against {:.2} crossings per 1000 samples)",
            pitches(&s),
            crossing_rate(&first),
            crossing_rate(&newest)
        ),
        pitches(&s) == [60] && crossings(&first) > 10 && crossing_rate(&newest) > 1.6 * crossing_rate(&first),
    );
    s.undo();
    check("undo goes back to the newest take", pitches(&s) == [72]);
    s.checkpoint("Flatten", None, false);
    s.flatten_take_folder(folder.id);
    check("Flatten leaves a plain region of the take heard", region(&s, folder.id).takes.is_none() && pitches(&s) == [72]);
    s.undo();
    // The Replace button wins over take folders.
    s.undo();
    s.prefs.recording.replace = true;
    s.set_cycle(true, bar, 2 * bar);
    s.record().unwrap();
    for pitch in [62u8, 64] {
        wait_for(&s, bar + beat, bar + 2 * beat);
        s.send_midi([0x90, pitch, 100]);
        wait_for(&s, bar + 3 * beat, 2 * bar);
        s.send_midi([0x80, pitch, 0]);
        wait_for(&s, bar, bar + beat);
    }
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    let regions = s.project.track(inst).unwrap().regions.clone();
    check(&format!("with Replace on, cycle passes don't make a folder ({} regions)", regions.len()), regions.len() == 1 && regions[0].takes.is_none());

    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
