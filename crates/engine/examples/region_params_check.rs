//! Exercises region playback parameters (the Region inspector's Quantize, Transpose, Velocity,
//! Dynamics, Gate Time and Delay) against a running engine (silent ALSA null output), through
//! offline bounces of the built-in synth and a test tone: what plays matches the notes worked
//! out by hand, delays move notes and audio (also before a region's start), quantize follows
//! tempo changes, Apply Region Parameters plays the same and undoes in one step.
//! usage: region_params_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, Note, RegionContent, TrackKind};
use daw_engine::region_params::GATE_LEGATO;
use daw_engine::session::{BitDepth, MidiContent, RegionParamsPatch, RenderOptions, TrackPatch};
use daw_engine::tempo::TempoEvent;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Bounces `0..end` (no tail) and returns the left channel.
fn bounce(s: &mut Session, dir: &Path, end: i64) -> Vec<f32> {
    let path = dir.join("bounce.wav");
    let opts = RenderOptions { start: 0, end, tail_secs: 0.0, bit_depth: BitDepth::Float32, ..Default::default() };
    s.bounce_project(&path, &opts, &mut |_| {}).unwrap();
    let mut r = hound::WavReader::open(&path).unwrap();
    let ch = r.spec().channels as usize;
    r.samples::<f32>().step_by(ch).map(|x| x.unwrap()).collect()
}

/// The first sample that isn't silent.
fn onset(a: &[f32]) -> Option<i64> {
    a.iter().position(|x| x.abs() > 1e-7).map(|i| i as i64)
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(if a.len() == b.len() { 0.0 } else { 1.0 }, f32::max)
}

fn notes(s: &Session, id: Id) -> Vec<Note> {
    let r = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|r| r.id == id).unwrap();
    let RegionContent::Midi { notes, .. } = &r.content else { panic!("not MIDI") };
    notes.clone()
}

fn set(s: &mut Session, ids: &[Id], patch: RegionParamsPatch) {
    s.set_region_params(ids, &patch);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    std::fs::create_dir_all(&scratch).unwrap();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("RegionParams.fermata");
    let sr = s.sample_rate();
    s.set_tempo(120.0);
    let beat = (sr / 2.0).round() as i64;
    let bar = 4 * beat;
    let ms = |v: f64| (v * sr / 1000.0).round() as i64;

    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let r = s.create_midi_region(inst, bar, 2 * bar).unwrap();
    let note = |start: i64, length: i64, pitch: u8, velocity: u8| Note { start, length, pitch, velocity, muted: false };
    // Off the grid: a little after beat 1 and a little before beat 3 (counted in the region).
    let raw = vec![note(1000, beat / 2, 60, 100), note(2 * beat - 700, beat, 64, 40)];
    s.set_midi_content(vec![MidiContent { region: r, notes: raw.clone(), cc: Vec::new(), start: None, length: None }]);
    let end = 4 * bar;

    let plain = bounce(&mut s, &scratch, end);
    check(&format!("the region plays as written (onset {:?})", onset(&plain)), onset(&plain) == Some(bar + 1000));

    // ---- quantize, transpose, velocity, dynamics, gate: the same as the notes worked out ------
    s.checkpoint("Region Params", None, false);
    set(&mut s, &[r], RegionParamsPatch { quantize: Some(4), ..Default::default() });
    set(&mut s, &[r], RegionParamsPatch { transpose: Some(12), velocity: Some(-20), dynamics: Some(200), gate: Some(50), ..Default::default() });
    let with_params = bounce(&mut s, &scratch, end);
    // 2:1 around 64, then -20: 100 -> 82 -> 62, 40 -> 52 -> 32. Gate 50 % halves the lengths.
    let by_hand = vec![note(0, beat / 4, 72, 62), note(2 * beat, beat / 2, 76, 32)];
    s.checkpoint("By Hand", None, false);
    s.set_midi_content(vec![MidiContent { region: r, notes: by_hand.clone(), cc: Vec::new(), start: None, length: None }]);
    s.set_region_params(&[r], &RegionParamsPatch { quantize: Some(0), transpose: Some(0), velocity: Some(0), dynamics: Some(100), gate: Some(100), ..Default::default() });
    let written = bounce(&mut s, &scratch, end);
    let d = max_diff(&with_params, &written);
    check(&format!("parameters play the notes worked out by hand (max diff {d:.2e}, onset {:?})", onset(&with_params)), d < 1e-6 && onset(&with_params) == Some(bar));
    s.undo();
    let p = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|x| x.id == r).unwrap().params;
    check("undo puts the parameters and notes back", p.quantize == 4 && p.gate == 50 && notes(&s, r)[0].start == 1000);

    // Apply Region Parameters writes them in and plays the same; one undo takes it back.
    let undo_depth = s.undo_label();
    s.checkpoint("Apply Region Parameters", None, false);
    s.apply_region_params(&[r]);
    let applied = notes(&s, r);
    let region = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|x| x.id == r).unwrap().clone();
    check(
        &format!("Apply writes the notes and resets the parameters ({:?})", applied.iter().map(|n| (n.start, n.length, n.pitch, n.velocity)).collect::<Vec<_>>()),
        region.params.is_default() && applied.iter().zip(&by_hand).all(|(a, b)| (a.start, a.length, a.pitch, a.velocity) == (b.start, b.length, b.pitch, b.velocity)),
    );
    let baked = bounce(&mut s, &scratch, end);
    check(&format!("applied region plays the same (max diff {:.2e})", max_diff(&baked, &with_params)), max_diff(&baked, &with_params) < 1e-6);
    s.undo();
    let back = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|x| x.id == r).unwrap().clone();
    check("one undo brings the parameters and the notes back", back.params.quantize == 4 && back.params.transpose == 12 && notes(&s, r)[0].start == 1000 && s.undo_label() == undo_depth);
    let again = bounce(&mut s, &scratch, end);
    check("and the engine plays the parameters again", max_diff(&again, &with_params) < 1e-6);

    // Steps on one value with the same key are one undo step (as the Inspector's fields send).
    let key = Some(format!("region-params:Transpose:{:?}", [r]));
    for t in [13, 14, 15] {
        s.checkpoint("Transpose", key.clone(), false);
        set(&mut s, &[r], RegionParamsPatch { transpose: Some(t), ..Default::default() });
    }
    s.undo();
    let p = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|x| x.id == r).unwrap().params;
    check(&format!("a burst of steps undoes in one go (transpose {})", p.transpose), p.transpose == 12);

    // Legato: the first note reaches the second.
    s.checkpoint("Gate", None, false);
    set(&mut s, &[r], RegionParamsPatch { gate: Some(GATE_LEGATO), ..Default::default() });
    s.apply_region_params(&[r]);
    let n = notes(&s, r);
    check(&format!("Legato ties the first note to the second ({})", n[0].length), n[0].length == 2 * beat);
    s.undo();

    // ---- delay -----------------------------------------------------------------------------
    s.checkpoint("Delay", None, false);
    set(&mut s, &[r], RegionParamsPatch { quantize: Some(0), delay: Some(50.0), delay_ms: Some(true), ..Default::default() });
    let late = bounce(&mut s, &scratch, end);
    check(&format!("50 ms delay ({:?}, want {})", onset(&late), bar + 1000 + ms(50.0)), onset(&late) == Some(bar + 1000 + ms(50.0)));
    set(&mut s, &[r], RegionParamsPatch { delay: Some(-100.0), ..Default::default() });
    let early = bounce(&mut s, &scratch, end);
    check(&format!("a negative delay plays before the region start ({:?}, want {})", onset(&early), bar + 1000 - ms(100.0)), onset(&early) == Some(bar + 1000 - ms(100.0)));
    // Switching to ticks keeps the delay: -100 ms at 120 BPM = -192 ticks.
    set(&mut s, &[r], RegionParamsPatch { delay_ms: Some(false), ..Default::default() });
    let p = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|x| x.id == r).unwrap().params;
    check(&format!("ms to ticks ({} ticks)", p.delay), !p.delay_ms && p.delay == -192.0);
    // Ticks follow the tempo map: a quarter of delay at 60 BPM from bar 2 on is a second.
    set(&mut s, &[r], RegionParamsPatch { delay: Some(960.0), ..Default::default() });
    s.set_tempo_map(vec![TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: bar, bpm: 60.0, ramp: false }], false);
    let slow = bounce(&mut s, &scratch, end + 4 * bar);
    check(&format!("a delay in ticks follows the tempo ({:?}, want {})", onset(&slow), bar + 1000 + sr as i64), onset(&slow) == Some(bar + 1000 + sr as i64));
    s.undo();

    // ---- quantize follows tempo changes ------------------------------------------------------
    s.checkpoint("Quantize", None, false);
    set(&mut s, &[r], RegionParamsPatch { quantize: Some(4), transpose: Some(0), velocity: Some(0), dynamics: Some(100), gate: Some(100), ..Default::default() });
    let on = onset(&bounce(&mut s, &scratch, end));
    check(&format!("quantized at 120 BPM ({on:?})"), on == Some(bar));
    // At 100 BPM, beats fall every 0.6 s: the note at bar + 1000 goes to the nearest one.
    s.set_tempo(100.0);
    let b100 = (sr * 0.6).round() as i64;
    let want = ((bar + 1000) as f64 / b100 as f64).round() as i64 * b100;
    let on = onset(&bounce(&mut s, &scratch, end));
    check(&format!("quantize follows a tempo change ({on:?}, want {want})"), on == Some(want));
    s.set_tempo(120.0);
    s.undo();

    // ---- audio region delay ------------------------------------------------------------------
    s.update_track(inst, TrackPatch { mute: Some(true), ..Default::default() });
    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let ar = s.import_audio(Path::new(&tone), a, bar).unwrap();
    let dry = onset(&bounce(&mut s, &scratch, end)).unwrap();
    s.checkpoint("Delay", None, false);
    set(&mut s, &[ar], RegionParamsPatch { delay: Some(25.0), delay_ms: Some(true), transpose: Some(7), ..Default::default() });
    let p = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|x| x.id == ar).unwrap().params;
    check("MIDI-only values don't reach audio regions", p.transpose == 0 && p.delay == 25.0);
    let wet = onset(&bounce(&mut s, &scratch, end)).unwrap();
    check(&format!("an audio region's delay moves it ({} -> {wet}, want +{})", dry, ms(25.0)), wet - dry == ms(25.0));
    s.apply_region_params(&[ar]);
    let moved = s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|x| x.id == ar).unwrap().clone();
    check(&format!("Apply moves the audio region ({})", moved.start), moved.start == bar + ms(25.0) && moved.params.is_default());
    let wet2 = onset(&bounce(&mut s, &scratch, end)).unwrap();
    check("and it plays as before", wet2 == wet);

    // Saved and loaded back.
    s.undo();
    set(&mut s, &[ar], RegionParamsPatch { delay: Some(-10.0), delay_ms: Some(true), ..Default::default() });
    set(&mut s, &[r], RegionParamsPatch { quantize: Some(8), swing: Some(60), ..Default::default() });
    let json = serde_json::to_string(&s.project).unwrap();
    let back: daw_engine::model::Project = serde_json::from_str(&json).unwrap();
    let pr = back.tracks.iter().flat_map(|t| t.regions.iter()).find(|x| x.id == r).unwrap().params;
    check("parameters are saved", pr.quantize == 8 && pr.swing == 60 && json.contains("\"params\""));
    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
