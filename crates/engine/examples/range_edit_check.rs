//! Exercises the Audio Track editor's range edits and Split at Transients against a running
//! engine (silent ALSA null output): what plays after each edit, and that one undo step
//! reverts it. usage: range_edit_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, RegionContent, TrackKind};
use daw_engine::session::RangeEdit;
use daw_engine::{transients, Session};

/// Peak level of Stereo Out while cycling over [a, b).
fn level(s: &mut Session, a: i64, b: i64) -> f32 {
    s.set_cycle(true, a, b);
    s.locate(a);
    s.play();
    std::thread::sleep(Duration::from_millis(150));
    s.tick();
    std::thread::sleep(Duration::from_millis(250));
    let t = s.tick();
    s.stop();
    // play() is ignored until the engine has really stopped.
    std::thread::sleep(Duration::from_millis(60));
    t.master.0.max(t.master.1)
}

fn check(name: &str, ok: bool) {
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// A drum-like test file: decaying sine bursts every half second.
fn write_hits(path: &Path, rate: u32, hits: usize) -> Vec<i64> {
    let gap = rate as usize / 2;
    let mut buf = vec![0.0f32; gap * (hits + 1)];
    let at: Vec<i64> = (0..hits).map(|i| (gap / 2 + i * gap) as i64).collect();
    for &a in &at {
        for i in 0..gap / 2 {
            let t = i as f32 / rate as f32;
            buf[a as usize + i] += 0.8 * (-t / 0.04).exp() * (std::f32::consts::TAU * 150.0 * t).sin();
        }
    }
    daw_engine::decode::write_wav(path, rate, &[buf]).unwrap();
    at
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("RangeEditCheck.fermata");
    let rate = s.sample_rate() as i64;

    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let id = s.import_audio(Path::new(&tone), t, 0).unwrap();
    let len = s.project.track(t).unwrap().regions[0].length;
    println!("audio: {:?}, region {len} frames", s.audio_status);
    let (a, b) = (rate / 2, rate);
    let before = level(&mut s, a, b);
    check(&format!("the tone plays ({before:.3})"), before > 0.01);

    // Silence: the range goes quiet, the rest still plays, one undo brings it back.
    s.checkpoint(RangeEdit::Silence.label(), None, false);
    let inside = s.edit_range(&[id], a, b, RangeEdit::Silence).unwrap();
    let regions = s.project.track(t).unwrap().regions.len();
    check(&format!("silence splits into three regions ({regions}) and mutes the middle"), regions == 3 && inside.len() == 1);
    let quiet = level(&mut s, a + 2400, b - 2400);
    let outside = level(&mut s, b + 2400, b + rate / 2);
    check(&format!("the silenced range is silent ({quiet:.5}) and after it plays ({outside:.3})"), quiet < 1e-4 && outside > 0.01);
    s.undo();
    let back = level(&mut s, a, b);
    check(&format!("undo restores one region that plays ({back:.3})"), s.project.track(t).unwrap().regions.len() == 1 && back > 0.01);

    // Delete leaves a gap.
    s.checkpoint(RangeEdit::Delete.label(), None, false);
    s.edit_range(&[id], a, b, RangeEdit::Delete).unwrap();
    let gap = level(&mut s, a + 2400, b - 2400);
    check(&format!("delete leaves a silent gap ({gap:.5}) with two regions"), gap < 1e-4 && s.project.track(t).unwrap().regions.len() == 2);
    s.undo();

    // Gain: normalizing a range changes only that range's region.
    s.checkpoint(RangeEdit::Normalize.label(), None, false);
    let inside = s.edit_range(&[id], a, b, RangeEdit::Normalize).unwrap();
    let louder = level(&mut s, a + 2400, b - 2400);
    let gain_of = |r: &daw_engine::model::Region| if let RegionContent::Audio { gain_db, .. } = r.content { gain_db } else { 0.0 };
    let regs = &s.project.track(t).unwrap().regions;
    let g = regs.iter().find(|r| r.id == inside[0]).map(gain_of).unwrap_or(0.0);
    let others_flat = regs.iter().filter(|r| r.id != inside[0]).all(|r| gain_of(r) == 0.0);
    let measured = 20.0 * (louder / before).log10();
    check(&format!("normalize sets only the range's gain ({g:+.2} dB, measured {measured:+.2} dB)"), others_flat && (measured - g).abs() < 1.0);
    s.undo();

    // Split at Transients on a synthetic drum file.
    let file = scratch.join("range-edit-hits.wav");
    let hits = write_hits(&file, rate as u32, 8);
    let d = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let drums = s.import_audio(&file, d, 0).unwrap();
    let RegionContent::Audio { clip, .. } = s.project.track(d).unwrap().regions[0].content else { unreachable!() };
    let audio = s.clip_audio(clip).unwrap();
    let found = transients::detect(&audio.channels, audio.sample_rate, 0.5);
    let close = found.len() == hits.len() && found.iter().zip(&hits).all(|(f, h)| (f - h).abs() <= rate / 1000);
    check(&format!("finds the {} hits within 1 ms ({found:?})", hits.len()), close);
    s.checkpoint("Split Regions", None, false);
    let pieces = s.split_regions_at(&[drums], &found);
    // The first hit isn't at the region start, so every hit starts a piece.
    let starts: Vec<i64> = s.project.track(d).unwrap().regions.iter().map(|r| r.start).collect();
    check(&format!("split at transients makes {} regions starting at the hits", pieces.len()), pieces.len() == hits.len() + 1 && starts[1..] == found[..]);
    let chopped = level(&mut s, 0, rate * 2);
    check(&format!("the chopped regions still play ({chopped:.3})"), chopped > 0.3);
    s.undo();
    check("one undo joins them back", s.project.track(d).unwrap().regions.len() == 1);

    let _ = std::fs::remove_dir_all(&s.dir);
    let _ = std::fs::remove_file(&file);
}
