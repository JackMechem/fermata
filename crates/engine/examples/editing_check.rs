//! Editing details against a running engine (silent ALSA null output): Snap Edits to Zero
//! Crossings on splits and range edits, a region gain drag as the Tracks area sends it (one
//! undo step, heard in playback), and a nudge in bars and beats across a tempo change.
//! usage: editing_check <audio file> <scratch dir>
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, Region, RegionContent, TrackKind};
use daw_engine::session::RangeEdit;
use daw_engine::tempo::TempoEvent;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Plays for a moment and returns Stereo Out's peak.
fn level(s: &mut Session) -> f32 {
    s.locate(0);
    s.play();
    std::thread::sleep(Duration::from_millis(150));
    s.tick();
    std::thread::sleep(Duration::from_millis(250));
    let t = s.tick();
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    t.master.0.max(t.master.1)
}

fn regions(s: &Session, track: u64) -> Vec<Region> {
    let mut v = s.project.track(track).unwrap().regions.clone();
    v.sort_by_key(|r| r.start);
    v
}

/// The audio at timeline position `pos` of region `r`, summed over its channels, and the
/// tone's peak sum.
fn sample_at(s: &Session, r: &Region, pos: i64) -> (f32, f32) {
    let RegionContent::Audio { clip, offset, .. } = r.content else { return (1.0, 1.0) };
    let c = s.clip_audio(clip).unwrap();
    let f = (offset + pos - r.start) as usize;
    let sum = |i: usize| c.channels.iter().map(|ch| ch[i]).sum::<f32>();
    let peak = (0..c.frames().min(4800)).map(|i| sum(i).abs()).fold(0.0, f32::max);
    (sum(f), peak)
}

fn gain(s: &Session, track: u64) -> Vec<f32> {
    regions(s, track).iter().map(|r| if let RegionContent::Audio { gain_db, .. } = r.content { gain_db } else { f32::NAN }).collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = args.next().expect("scratch dir");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::path::PathBuf::from(&scratch).join("EditingCheck.fermata");
    let sr = s.project.sample_rate as i64;
    let window = sr * 5 / 1000;

    // ---- Snap Edits to Zero Crossings ----
    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    // 440 Hz: a crossing every 54.5 samples, at 10 036 and 10 091 here; 10 060 is between.
    let pos = 10_060;
    s.checkpoint("Split", None, false);
    s.split_at(pos, &[]);
    let v = regions(&s, a);
    check(&format!("off: the split is where asked ({})", v[1].start), v.len() == 2 && v[1].start == pos);
    s.undo();
    s.set_snap_zero_crossings(true);
    s.checkpoint("Split", None, false);
    s.split_at(pos, &[]);
    let v = regions(&s, a);
    let cut = v[1].start;
    let (at, peak) = sample_at(&s, &v[1], cut);
    check(&format!("on: the split moves to a crossing within 5 ms ({cut}, |{at:.4}| of {peak:.3})"), v.len() == 2 && v[0].start + v[0].length == cut && (cut - pos).abs() <= window && (cut - pos).abs() > 10 && at.abs() < peak * 0.05);
    s.undo();
    let inside = s.edit_range(&regions(&s, a).iter().map(|r| r.id).collect::<Vec<_>>(), 30_011, 60_029, RangeEdit::Split).unwrap();
    let r = regions(&s, a).into_iter().find(|r| inside.contains(&r.id)).unwrap();
    let (x0, _) = sample_at(&s, &r, r.start);
    let (x1, _) = sample_at(&s, &r, r.start + r.length - 1);
    check(
        &format!("a range split cuts both ends at crossings ({} .. {}, |{x0:.4}| |{x1:.4}|)", r.start, r.start + r.length),
        (r.start - 30_011).abs() <= window && (r.start + r.length - 60_029).abs() <= window && x0.abs() < peak * 0.05 && x1.abs() < peak * 0.1,
    );
    let full = s.project.track(a).unwrap().regions.iter().map(|r| r.id).collect::<Vec<_>>();
    s.delete_regions(&full);
    s.set_snap_zero_crossings(false);

    // ---- Region gain drag ----
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    let ids: Vec<u64> = regions(&s, a).iter().map(|r| r.id).collect();
    s.set_cycle(true, 0, 48_000);
    let loud = level(&mut s);
    let undo_before = s.undo_label();
    // As `set_region_gains` sends a drag: the first update records the step, the rest join it.
    s.checkpoint("Region Gain", None, false);
    for db in [-2.0, -5.5, -9.0, -12.0] {
        s.set_region_gains(&[(ids[0], db)]);
    }
    let quiet = level(&mut s);
    let ratio = quiet / loud.max(1e-9);
    check(&format!("a -12 dB drag is heard ({loud:.4} -> {quiet:.4}, x{ratio:.3})"), gain(&s, a) == vec![-12.0] && (ratio - 0.251).abs() < 0.03);
    s.undo();
    check(&format!("one undo puts the gain back ({:?})", gain(&s, a)), gain(&s, a) == vec![0.0] && s.undo_label() == undo_before);
    s.set_cycle(false, 0, 48_000);

    // ---- A nudge in bars and beats across a tempo change ----
    // 120 BPM for four bars, then 90 BPM.
    let bar5 = 16 * sr / 2;
    s.set_tempo_map(vec![TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: bar5, bpm: 90.0, ramp: false }], false);
    let m = s.tempo_map();
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    // Regions at bar 1 and at bar 3 beat 2; a nudge of two bars takes the first to bar 3.
    s.import_audio(std::path::Path::new(&tone), b, 0).unwrap();
    let b3b2 = m.bar_start(3).round() as i64 + sr / 2;
    s.import_audio(std::path::Path::new(&tone), b, b3b2).unwrap();
    let ids: Vec<u64> = regions(&s, b).iter().map(|r| r.id).collect();
    s.checkpoint("Nudge", None, false);
    s.drag_nudge_musical(&ids, 0, m.bar_start(3).round() as i64, Some(false));
    let v = regions(&s, b);
    let bbt = m.bar_beat_at(v[1].start as f64);
    check(
        &format!("the second region lands on bar 5 beat 2 at 90 BPM ({} = {}.{}.{}.{})", v[1].start, bbt.bar, bbt.beat, bbt.div, bbt.tick),
        v[0].start == m.bar_start(3).round() as i64 && (v[1].start - (bar5 + sr * 60 / 90)).abs() <= 1 && (bbt.bar, bbt.beat) == (5, 2),
    );
    s.undo();
    check("undo puts both back", regions(&s, b).iter().map(|r| r.start).collect::<Vec<_>>() == vec![0, b3b2]);

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
