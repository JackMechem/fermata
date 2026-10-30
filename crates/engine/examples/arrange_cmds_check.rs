//! Exercises Logic's arranging commands against a running engine (silent ALSA null output).
//! With the locators at 1..1.5 s and regions and volume automation across them on two tracks,
//! Insert Silence, Cut Section and Repeat Section must move the content in time, and the
//! offline render must play the old audio with a gap, without the section, or with it twice;
//! Insert Cut Section after a cut must put the project back as it was. Remove Gaps, Remove
//! Overlaps and Paste Replace must lay out the regions as Logic does, and Strip Silence must
//! split a tone with gaps into one region per note, playing as before. One undo puts each
//! edit back.
//! usage: arrange_cmds_check <scratch dir>
use std::path::{Path, PathBuf};

use daw_engine::arrange_cmds::SectionEdit;
use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{AutoMode, AutoParam, AutoPoint, Id, InputSel, RegionContent, TrackKind};
use daw_engine::strip_silence::StripSettings;
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

/// A 440 Hz tone at half scale over each (start, end) in seconds, silence elsewhere, three
/// seconds long.
fn write_notes(path: &Path, rate: u32, notes: &[(f64, f64)]) {
    let sr = rate as f64;
    let buf: Vec<f32> = (0..rate as usize * 3)
        .map(|i| {
            let t = i as f64 / sr;
            if notes.iter().any(|&(a, b)| t >= a && t < b) { 0.5 * (std::f64::consts::TAU * 440.0 * t).sin() as f32 } else { 0.0 }
        })
        .collect();
    daw_engine::decode::write_wav(path, rate, &[buf]).unwrap();
}

/// Two offline renders of `0..len`: the regions alone (automation off), to compare sample for
/// sample, and with the automation playing, to compare by level (the engine applies volume
/// automation once per block, smoothed, so a shifted ramp isn't bit-identical).
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

/// The largest difference between `got[at..at + n]` and `want[from..from + n]`, both channels.
fn diff(got: &[Vec<f32>], at: i64, want: &[Vec<f32>], from: i64, n: i64) -> f32 {
    let mut d = 0.0f32;
    for (g, w) in got.iter().zip(want) {
        for k in 0..n {
            d = d.max((g[(at + k) as usize] - w[(from + k) as usize]).abs());
        }
    }
    d
}

/// The largest level difference in dB between the two stretches, over 50 ms windows, leaving
/// out 40 ms at each end: where a section edit joins two stretches the automation may jump
/// (a repeated section starts at its own first value), and the engine smooths gain changes
/// over about 5 ms.
fn level_diff(got: &[Vec<f32>], at: i64, want: &[Vec<f32>], from: i64, n: i64, sr: i64) -> f32 {
    let (w, margin) = (sr / 20, sr / 25);
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

/// How far a render is from the old one with its stretches `(at, from, n)` rearranged.
fn compare(got: &Render, want: &Render, map: &[(i64, i64, i64)], sr: i64) -> (f32, f32) {
    let exact = map.iter().map(|&(at, from, n)| diff(&got.plain, at, &want.plain, from, n)).fold(0.0, f32::max);
    let level = map.iter().map(|&(at, from, n)| level_diff(&got.auto, at, &want.auto, from, n, sr)).fold(0.0, f32::max);
    (exact, level)
}

fn peak(a: &[Vec<f32>], from: i64, to: i64) -> f32 {
    a.iter().flat_map(|c| c[from as usize..to as usize].iter()).fold(0.0f32, |m, s| m.max(s.abs()))
}

/// (start, end, clip offset) of a track's regions, by start.
fn spans(s: &Session, t: Id) -> Vec<(i64, i64, i64)> {
    let mut v: Vec<(i64, i64, i64)> = s
        .project
        .track(t)
        .unwrap()
        .regions
        .iter()
        .map(|r| match r.content {
            RegionContent::Audio { offset, .. } => (r.start, r.start + r.length, offset),
            _ => (r.start, r.start + r.length, 0),
        })
        .collect();
    v.sort();
    v
}

fn markers(s: &Session) -> Vec<i64> {
    s.project.markers.iter().map(|m| m.pos).collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("ArrangeCmdsCheck.fermata");
    let _ = std::fs::remove_dir_all(&s.dir);
    let sr = s.sample_rate() as i64;
    println!("audio: {:?}, {sr} Hz", s.audio_status);
    let noise = scratch.join("noise.wav");
    write_noise(&noise, sr as u32);

    // Track A: one region from 0.5 s to 2.5 s, across both locators. Track B: one inside the
    // locators and one after them. Volume ramps on both, a marker inside the section, and one
    // arrangement section over the first three seconds.
    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let ra = s.import_audio(&noise, a, sr / 2).unwrap();
    s.resize_region(ra, sr / 2, 2 * sr);
    let rb1 = s.import_audio(&noise, b, sr * 11 / 10).unwrap();
    s.resize_region(rb1, sr * 11 / 10, sr * 2 / 10);
    let rb2 = s.import_audio(&noise, b, sr * 22 / 10).unwrap();
    s.resize_region(rb2, sr * 22 / 10, sr * 6 / 10);
    let ramp = |p: &[(i64, f32)]| p.iter().map(|&(pos, value)| AutoPoint { pos, value, curve: 0.0 }).collect::<Vec<_>>();
    s.set_automation_lane(a, AutoParam::Volume, ramp(&[(sr * 8 / 10, 0.0), (sr * 13 / 10, -12.0), (sr * 2, -3.0)]));
    s.set_automation_lane(b, AutoParam::Volume, ramp(&[(0, -6.0), (sr * 3, 0.0)]));
    s.add_marker(sr * 12 / 10, Some("Inside".into()));
    s.add_marker(2 * sr, Some("After".into()));
    let sec = s.add_arrangement_marker(None);
    s.resize_arrangement_marker(sec, 3 * sr);
    let (l, r) = (sr, sr * 3 / 2);
    let n = r - l;
    s.set_cycle(true, l, r);

    let len = 4 * sr;
    let tracks = [a, b];
    let r0 = render(&mut s, &tracks, len);
    check(&format!("the project plays ({:.3})", peak(&r0.auto, 0, len)), peak(&r0.auto, 0, len) > 0.05);
    let ok = |(exact, level): (f32, f32)| exact < 1e-6 && level < 0.5;
    let a0 = spans(&s, a);
    let b0 = spans(&s, b);

    // ---- Insert Silence Between Locators.
    s.checkpoint(SectionEdit::InsertSilence.label(), None, false);
    s.section_edit(SectionEdit::InsertSilence, l, r, None).unwrap();
    let got = spans(&s, a);
    check(&format!("insert silence cuts the region at the left locator and moves its tail: {got:?}"), got == vec![(sr / 2, l, 0), (r, sr * 3, sr / 2)]);
    check(&format!("the markers and the arrangement section move with their time: {:?}", markers(&s)), markers(&s) == vec![sr * 12 / 10 + n, 2 * sr + n] && s.project.arrangement[0].length == 3 * sr + n);
    let r1 = render(&mut s, &tracks, len + n);
    let d = compare(&r1, &r0, &[(0, 0, l), (r, l, len - l)], sr);
    check(&format!("the render plays the old audio with half a second of silence at 1 s (diff {d:?}, gap peak {:.6})", peak(&r1.plain, l, r)), ok(d) && peak(&r1.plain, l, r) < 1e-6);
    s.undo();
    let back = render(&mut s, &tracks, len);
    check("undo puts it back", spans(&s, a) == a0 && compare(&back, &r0, &[(0, 0, len)], sr) == (0.0, 0.0));

    // ---- Cut Section Between Locators (Snip).
    s.checkpoint(SectionEdit::Cut.label(), None, false);
    s.section_edit(SectionEdit::Cut, l, r, None).unwrap();
    let got = (spans(&s, a), spans(&s, b));
    check(&format!("cut section takes the section out of the region, drops what was inside it, and closes the gap: {got:?}"), got == (vec![(sr / 2, l, 0), (l, 2 * sr, sr)], vec![(sr * 22 / 10 - n, sr * 28 / 10 - n, 0)]));
    check(&format!("the marker inside the section goes: {:?}", markers(&s)), markers(&s) == vec![2 * sr - n]);
    let r2 = render(&mut s, &tracks, len - n);
    let d = compare(&r2, &r0, &[(0, 0, l), (l, r, len - r)], sr);
    check(&format!("the render skips the section (diff {d:?})"), ok(d));

    // ---- Insert Cut Section at the same place: back to the start.
    s.checkpoint("Insert Cut Section", None, false);
    let made = s.insert_cut_section(l).unwrap();
    let got = (spans(&s, a), spans(&s, b));
    check(&format!("insert cut section puts the section's regions back ({} of them): {got:?}", made.len()), made.len() == 2 && got.1 == b0 && got.0 == vec![(sr / 2, l, 0), (l, r, sr / 2), (r, sr * 5 / 2, sr)]);
    let r3 = render(&mut s, &tracks, len);
    let d = compare(&r3, &r0, &[(0, 0, len)], sr);
    check(&format!("and the project plays as before the cut (diff {d:?}), markers {:?}", markers(&s)), ok(d) && markers(&s) == vec![sr * 12 / 10, 2 * sr]);
    s.undo();
    s.undo();
    check("undo takes back the insert, then the cut", spans(&s, a) == a0 && spans(&s, b) == b0 && markers(&s).len() == 2);

    // ---- Repeat Section Between Locators.
    s.checkpoint(SectionEdit::Repeat.label(), None, false);
    let made = s.section_edit(SectionEdit::Repeat, l, r, None).unwrap();
    let got = spans(&s, b);
    check(&format!("repeat section copies the region inside it after the right locator and moves the later one on: {got:?}"), got == vec![(sr * 11 / 10, sr * 13 / 10, 0), (sr * 11 / 10 + n, sr * 13 / 10 + n, 0), (sr * 22 / 10 + n, sr * 28 / 10 + n, 0)]);
    check(&format!("the copies are returned to select ({})", made.len()), made.len() == 2);
    let r4 = render(&mut s, &tracks, len + n);
    let d = compare(&r4, &r0, &[(0, 0, r), (r, l, n), (r + n, r, len - r)], sr);
    check(&format!("the render plays the section twice (diff {d:?})"), ok(d));
    s.undo();
    check("undo removes the repeat", spans(&s, b) == b0 && spans(&s, a) == a0);

    // ---- Remove Gaps and Remove Overlaps on track B.
    let c = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let ids: Vec<Id> = [(0, 3), (5, 3), (9, 3)].iter().map(|&(at, l)| {
        let id = s.import_audio(&noise, c, sr * at / 10).unwrap();
        s.resize_region(id, sr * at / 10, sr * l / 10);
        id
    }).collect();
    let c0 = spans(&s, c);
    s.checkpoint("Remove Gaps", None, false);
    s.remove_gaps(&ids);
    let got = spans(&s, c);
    check(&format!("remove gaps butts the regions together: {got:?}"), got.iter().map(|g| (g.0, g.1)).collect::<Vec<_>>() == vec![(0, sr * 3 / 10), (sr * 3 / 10, sr * 6 / 10), (sr * 6 / 10, sr * 9 / 10)]);
    s.undo();
    check("undo puts the gaps back", spans(&s, c) == c0);
    s.move_region(ids[1], sr * 2 / 10, None);
    s.checkpoint("Remove Overlaps", None, false);
    s.remove_overlaps(&ids);
    let got = spans(&s, c);
    check(&format!("remove overlaps trims the earlier region: {got:?}"), got[0].1 == sr * 2 / 10 && got[1].0 == sr * 2 / 10);
    s.undo();
    check("undo puts the overlap back", spans(&s, c)[0].1 == sr * 3 / 10);

    // ---- Paste Replace: track C's first region pasted at 1 s on track A.
    let item = s.project.track(c).unwrap().regions.iter().find(|x| x.id == ids[0]).unwrap().clone();
    s.checkpoint("Paste Replace", None, false);
    let made = s.paste_replace(&[(a, item)], sr);
    let got = spans(&s, a);
    check(&format!("paste replace cuts a hole for the pasted region: {got:?}"), made.len() == 1 && got == vec![(sr / 2, sr, 0), (sr, sr * 13 / 10, 0), (sr * 13 / 10, sr * 5 / 2, sr * 8 / 10)]);
    s.undo();
    check("undo restores the region underneath", spans(&s, a) == a0);

    // ---- Strip Silence: notes at 0..0.5, 1..1.25 and 2..2.5 s.
    let notes = scratch.join("notes.wav");
    write_notes(&notes, sr as u32, &[(0.0, 0.5), (1.0, 1.25), (2.0, 2.5)]);
    let d_t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let rn = s.import_audio(&notes, d_t, sr).unwrap();
    for t in [a, b, c] {
        s.update_track(t, daw_engine::session::TrackPatch { mute: Some(true), ..Default::default() });
    }
    let before = s.render(0, len, 0, None, &mut |_| {}).unwrap();
    // 10 ms either side keeps the quiet start and end of each note: only true silence goes.
    let set = StripSettings { threshold: 0.04, min_time: 0.1, pre_attack: 0.01, post_release: 0.01 };
    let preview = s.strip_silence_preview(rn, &set);
    s.checkpoint("Strip Silence", None, false);
    let made = s.strip_silence(&[rn], &set);
    let got = spans(&s, d_t);
    let near = |x: i64, want: i64| (x - want).abs() <= sr / 400;
    let ms10 = sr / 100;
    let want = [(sr, sr * 3 / 2 + ms10), (2 * sr - ms10, sr * 9 / 4 + ms10), (3 * sr - ms10, sr * 7 / 2 + ms10)];
    check(
        &format!("strip silence splits the tone into its three notes: {got:?}"),
        made.len() == 3 && got.len() == 3 && got.iter().zip(want).all(|(g, w)| near(g.0, w.0) && near(g.1, w.1) && near(g.2, g.0 - sr)),
    );
    check("the preview showed the same cuts", preview == got.iter().map(|g| (g.0, g.1)).collect::<Vec<_>>());
    let after = s.render(0, len, 0, None, &mut |_| {}).unwrap();
    let d = diff(&after, 0, &before, 0, len);
    check(&format!("and plays as before (diff {d})"), d < 1e-6);
    s.undo();
    check("undo makes it one region again", spans(&s, d_t) == vec![(sr, 4 * sr, 0)]);

    let _ = std::fs::remove_dir_all(&s.dir);
    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
