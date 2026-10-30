//! Exercises the Arrangement track against a running engine (silent ALSA null output): three
//! one-second sections with regions on two tracks (one across two section edges) and volume
//! automation. Moving section 3 before section 2, copying section 1, and deleting section 2
//! with its content must cut and move the regions and automation, and the offline render must
//! play the old audio in the new order; one undo puts each edit back.
//! usage: arrangement_check <scratch dir>
use std::path::{Path, PathBuf};

use daw_engine::audio_io::AudioConfig;
use daw_engine::automation::value_at;
use daw_engine::model::{AutoMode, AutoParam, AutoPoint, Id, InputSel, RegionContent, TrackKind};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Four seconds of noise, so every stretch of the file sounds different from every other.
fn write_noise(path: &Path, rate: u32) {
    let mut x: u32 = 0x1234_5678;
    let buf: Vec<f32> = (0..rate as usize * 4)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((x >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.6
        })
        .collect();
    daw_engine::decode::write_wav(path, rate, &[buf]).unwrap();
}

/// Two offline renders of `0..len`: the regions alone (automation off), to compare sample for
/// sample, and with the automation playing, to compare by level, since the engine applies
/// volume automation once per block (and smooths it), so a shifted ramp isn't bit-identical.
struct Render {
    plain: Vec<Vec<f32>>,
    auto: Vec<Vec<f32>>,
}

fn render(s: &mut Session, tracks: &[Id], len: i64) -> Render {
    for &t in tracks {
        s.set_auto_mode(t, AutoMode::Off);
    }
    let plain = s.render(0, len, 0, None, &mut |_| {}).unwrap();
    for &t in tracks {
        s.set_auto_mode(t, AutoMode::Read);
    }
    let auto = s.render(0, len, 0, None, &mut |_| {}).unwrap();
    Render { plain, auto }
}

/// The largest level difference in dB between `got[at..at + n]` and `want[from..from + n]`,
/// over 50 ms windows, leaving out 10 ms at each end of the stretch.
fn level_diff(got: &[Vec<f32>], at: i64, want: &[Vec<f32>], from: i64, n: i64, sr: i64) -> f32 {
    let (w, margin) = (sr / 20, sr / 100);
    let rms = |a: &[Vec<f32>], lo: i64, hi: i64| {
        let sum: f32 = a.iter().flat_map(|c| c[lo as usize..hi as usize].iter()).map(|x| x * x).sum();
        (sum / (2 * (hi - lo)) as f32).sqrt()
    };
    let db = |x: f32| 20.0 * x.max(1e-5).log10();
    let mut d = 0.0f32;
    let mut k = margin;
    while k < n - margin {
        let e = (k + w).min(n - margin);
        d = d.max((db(rms(got, at + k, at + e)) - db(rms(want, from + k, from + e))).abs());
        k = e;
    }
    d
}

/// How far a render is from the old one with its stretches `(at, from, n)` rearranged: the
/// largest sample difference without automation, and the largest level difference with it.
fn compare(got: &Render, want: &Render, map: &[(i64, i64, i64)], sr: i64) -> (f32, f32) {
    let exact = map.iter().map(|&(at, from, n)| diff(&got.plain, at, &want.plain, from, n, 0)).fold(0.0, f32::max);
    let level = map.iter().map(|&(at, from, n)| level_diff(&got.auto, at, &want.auto, from, n, sr)).fold(0.0, f32::max);
    (exact, level)
}

/// The largest difference between `got[at..at + n]` and `want[from..from + n]`, leaving out
/// `margin` samples at each end, over both channels.
fn diff(got: &[Vec<f32>], at: i64, want: &[Vec<f32>], from: i64, n: i64, margin: i64) -> f32 {
    let mut d = 0.0f32;
    for (g, w) in got.iter().zip(want) {
        for k in margin..n - margin {
            d = d.max((g[(at + k) as usize] - w[(from + k) as usize]).abs());
        }
    }
    d
}

fn peak(a: &[Vec<f32>], from: i64, to: i64) -> f32 {
    a.iter().flat_map(|c| c[from as usize..to as usize].iter()).fold(0.0f32, |m, s| m.max(s.abs()))
}

/// (start, end, clip offset) of a track's regions, by start.
fn spans(s: &Session, t: Id) -> Vec<(i64, i64, i64)> {
    let mut v: Vec<(i64, i64, i64)> = s.project.track(t).unwrap().regions.iter().map(|r| match r.content {
        RegionContent::Audio { offset, .. } => (r.start, r.start + r.length, offset),
        _ => (r.start, r.start + r.length, 0),
    }).collect();
    v.sort();
    v
}

fn volume(s: &Session, t: Id) -> Vec<AutoPoint> {
    s.project.track(t).unwrap().automation.iter().find(|l| matches!(l.param, AutoParam::Volume)).map(|l| l.points.clone()).unwrap_or_default()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("ArrangementCheck.fermata");
    let _ = std::fs::remove_dir_all(&s.dir);
    let sr = s.sample_rate() as i64;
    println!("audio: {:?}, {sr} Hz", s.audio_status);
    let noise = scratch.join("noise.wav");
    write_noise(&noise, sr as u32);

    // Sections of one second each: 0..sr, sr..2sr, 2sr..3sr.
    let sec: Vec<Id> = (0..3)
        .map(|_| {
            let id = s.add_arrangement_marker(None);
            s.resize_arrangement_marker(id, sr);
            id
        })
        .collect();
    let names: Vec<&str> = s.project.arrangement.iter().map(|x| x.name.as_str()).collect();
    check(&format!("three sections, named {names:?}"), names == ["Intro", "Verse", "Chorus"]);

    // Track A: one region from 0.5 s to 2.5 s, across both inner edges. Track B: one inside
    // section 1 and one inside section 3.
    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let ra = s.import_audio(&noise, a, sr / 2).unwrap();
    s.resize_region(ra, sr / 2, 2 * sr);
    let rb1 = s.import_audio(&noise, b, sr / 10).unwrap();
    s.resize_region(rb1, sr / 10, sr * 3 / 10);
    let rb3 = s.import_audio(&noise, b, sr * 22 / 10).unwrap();
    s.resize_region(rb3, sr * 22 / 10, sr * 6 / 10);
    // Volume ramps: on A across the edge of sections 1 and 2, on B through section 3.
    let ramp_a = vec![AutoPoint { pos: sr * 9 / 10, value: 0.0 }, AutoPoint { pos: sr * 11 / 10, value: -12.0 }];
    let ramp_b = vec![AutoPoint { pos: 0, value: -6.0 }, AutoPoint { pos: 2 * sr, value: -20.0 }, AutoPoint { pos: sr * 29 / 10, value: 0.0 }, AutoPoint { pos: 3 * sr, value: -6.0 }];
    s.set_automation_lane(a, AutoParam::Volume, ramp_a.clone());
    s.set_automation_lane(b, AutoParam::Volume, ramp_b.clone());

    let len = 4 * sr;
    let tracks = [a, b];
    let r0 = render(&mut s, &tracks, len);
    check(&format!("the project plays ({:.3})", peak(&r0.auto, 0, len)), peak(&r0.auto, 0, len) > 0.05);
    // Exact to the sample without automation; within half a dB with it.
    let ok = |(exact, level): (f32, f32)| exact < 1e-6 && level < 0.5;

    // ---- Move section 3 before section 2.
    s.checkpoint("Move Arrangement Marker", None, false);
    let picked = s.move_arrangement_marker(sec[2], Some(sec[1]), false).unwrap();
    let order: Vec<(Id, i64)> = s.project.arrangement.iter().map(|x| (x.id, x.start)).collect();
    check("section 3 now plays second", order == vec![(sec[0], 0), (sec[2], sr), (sec[1], 2 * sr)]);
    // A's region: 0.5..1 s stays, 1..2 s (offset 0.5 s) goes to 2..3 s, 2..2.5 s (offset 1.5 s)
    // to 1..1.5 s.
    let got = spans(&s, a);
    check(&format!("the region across both edges is cut into three, in their sections' new places: {got:?}"), got == vec![(sr / 2, sr, 0), (sr, sr * 3 / 2, sr * 3 / 2), (2 * sr, 3 * sr, sr / 2)]);
    let got = spans(&s, b);
    check(&format!("the regions inside sections move whole: {got:?}"), got == vec![(sr / 10, sr * 4 / 10, 0), (sr * 12 / 10, sr * 18 / 10, 0)]);
    check(&format!("the moved section's regions are returned to select ({})", picked.len()), picked.len() == 2 && picked.contains(&rb3));
    let va = volume(&s, a);
    let vb = volume(&s, b);
    let at = |p: &[AutoPoint], x: i64| value_at(p, x).unwrap();
    check(
        &format!("automation moves with its sections ({} and {} points)", va.len(), vb.len()),
        (at(&va, sr * 205 / 100) - at(&ramp_a, sr * 105 / 100)).abs() < 1e-3
            && (at(&va, sr * 21 / 10) - at(&ramp_a, sr * 11 / 10)).abs() < 1e-3
            && (at(&va, sr * 95 / 100) - at(&ramp_a, sr * 95 / 100)).abs() < 1e-3
            && (at(&vb, sr * 19 / 10) - at(&ramp_b, sr * 29 / 10)).abs() < 1e-3
            && (at(&vb, sr * 25 / 10) - at(&ramp_b, sr * 15 / 10)).abs() < 1e-3,
    );
    let r1 = render(&mut s, &tracks, len);
    let d = compare(&r1, &r0, &[(0, 0, sr), (sr, 2 * sr, sr), (2 * sr, sr, sr), (3 * sr, 3 * sr, sr)], sr);
    let wrong = compare(&r1, &r0, &[(sr, sr, sr)], sr);
    check(&format!("the render plays section 3's audio second and section 2's third (diff {d:?}; in the old order it would be {wrong:?})"), ok(d) && wrong.0 > 0.05);
    s.undo();
    let back = render(&mut s, &tracks, len);
    let d = compare(&back, &r0, &[(0, 0, len)], sr);
    check(&format!("undo puts the regions, automation and sound back (diff {d:?})"), spans(&s, a) == vec![(sr / 2, 5 * sr / 2, 0)] && volume(&s, a) == ramp_a && volume(&s, b) == ramp_b && s.project.arrangement[1].id == sec[1] && d == (0.0, 0.0));

    // ---- Alt-drag: a copy of section 1 between sections 1 and 2.
    s.checkpoint("Copy Arrangement Marker", None, false);
    s.move_arrangement_marker(sec[0], Some(sec[1]), true).unwrap();
    let starts: Vec<i64> = s.project.arrangement.iter().map(|x| x.start).collect();
    check(&format!("the copy makes four sections: {starts:?}"), starts == vec![0, sr, 2 * sr, 3 * sr] && s.project.arrangement[1].name == "Intro");
    let got = spans(&s, b);
    check(&format!("section 1's region is copied, the later ones move a second on: {got:?}"), got == vec![(sr / 10, sr * 4 / 10, 0), (sr * 11 / 10, sr * 14 / 10, 0), (sr * 32 / 10, sr * 38 / 10, 0)]);
    let r2 = render(&mut s, &tracks, len + sr);
    let d = compare(&r2, &r0, &[(0, 0, sr), (sr, 0, sr), (2 * sr, sr, sr), (3 * sr, 2 * sr, 2 * sr)], sr);
    check(&format!("the render plays section 1 twice, then the rest (diff {d:?})"), ok(d));
    s.undo();
    check("undo removes the copy", s.project.arrangement.len() == 3 && spans(&s, b).len() == 2);

    // ---- Delete section 2 with its content.
    s.checkpoint("Delete Arrangement Marker and Content", None, false);
    s.delete_arrangement_marker(sec[1], true).unwrap();
    let got = spans(&s, a);
    check(&format!("section 2's part of the region goes, the rest closes up: {got:?}"), got == vec![(sr / 2, sr, 0), (sr, sr * 3 / 2, sr * 3 / 2)]);
    let r3 = render(&mut s, &tracks, len - sr);
    let d = compare(&r3, &r0, &[(0, 0, sr), (sr, 2 * sr, 2 * sr)], sr);
    check(&format!("the render skips section 2 (diff {d:?})"), ok(d) && s.project.arrangement.len() == 2);
    s.undo();
    let back = render(&mut s, &tracks, len);
    let d = compare(&back, &r0, &[(0, 0, len)], sr);
    check(&format!("undo brings section 2 and its content back (diff {d:?})"), s.project.arrangement.len() == 3 && d == (0.0, 0.0));

    // ---- Delete Arrangement Marker only: the content stays.
    s.delete_arrangement_marker(sec[1], false).unwrap();
    let v: Vec<(i64, i64)> = s.project.arrangement.iter().map(|x| (x.start, x.length)).collect();
    check(&format!("deleting just the marker leaves the content and widens section 1: {v:?}"), v == vec![(0, 2 * sr), (2 * sr, sr)] && spans(&s, a) == vec![(sr / 2, 5 * sr / 2, 0)]);

    let _ = std::fs::remove_dir_all(&s.dir);
    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
