//! Exercises pattern regions (the Step Sequencer) against a running engine (silent ALSA null
//! output), through offline bounces of the built-in synth: a pattern plays exactly the notes
//! worked out by hand (straight, swung, rows of other lengths and rates, ties, a loop and a
//! tempo change), step edits undo, and Convert to MIDI Region renders identically.
//! usage: pattern_check <scratch dir>
use std::path::{Path, PathBuf};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, Note, RegionContent, TrackKind};
use daw_engine::pattern::{Pattern, PatternRow, Step};
use daw_engine::session::{BitDepth, MidiContent, RenderOptions};
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

fn region(s: &Session, id: Id) -> daw_engine::model::Region {
    s.project.tracks.iter().flat_map(|t| t.regions.iter()).find(|r| r.id == id).unwrap().clone()
}

/// A row with the given steps on, each at `gate` percent.
fn row(pitch: u8, on: &[usize], gate: u8) -> PatternRow {
    let mut r = PatternRow::new(pitch);
    for &k in on {
        if r.steps.len() <= k {
            r.steps.resize(k + 1, Step::default());
        }
        r.steps[k] = Step { on: true, gate, ..Step::default() };
    }
    r
}

/// Renders `notes` written into a plain MIDI region at `start` (the pattern region muted), then
/// puts everything back with undo.
fn by_hand(s: &mut Session, dir: &Path, track: Id, pattern: Id, start: i64, length: i64, notes: Vec<Note>, end: i64) -> Vec<f32> {
    s.checkpoint("By Hand", None, false);
    s.set_region_muted(pattern, true);
    let m = s.create_midi_region(track, start, length).unwrap();
    s.set_midi_content(vec![MidiContent { region: m, notes, cc: Vec::new(), start: None, length: None }]);
    let out = bounce(s, dir, end);
    s.undo();
    out
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    std::fs::create_dir_all(&scratch).unwrap();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("Pattern.fermata");
    let sr = s.sample_rate();
    s.set_tempo(120.0);
    let bar = (2.0 * sr).round() as i64;
    let end = 5 * bar;
    // A note from `on` to `off` quarter notes after the region start, at `bpm`, rounded to
    // samples the way the pattern's notes are.
    let at = |q: f64, bpm: f64| (q * sr * 60.0 / bpm).round() as i64;
    let note_at = |on: f64, off: f64, pitch: u8, bpm: f64| Note { start: at(on, bpm), length: at(off, bpm) - at(on, bpm), pitch, velocity: 100, muted: false };
    let note = |on: f64, off: f64, pitch: u8| note_at(on, off, pitch, 120.0);

    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.checkpoint("Create Pattern Region", None, false);
    let p = s.create_pattern_region(inst, bar, bar).unwrap();
    let made = region(&s, p);
    check("a new pattern region has the default kit and plays nothing", made.pattern.as_ref().map(|p| p.rows.len()) == Some(8) && onset(&bounce(&mut s, &scratch, end)).is_none());

    // ---- straight: kick on the beats, snare on 2 and 4, half-step gates ------------------------
    s.checkpoint("Steps", None, false);
    let straight = Pattern { rows: vec![row(36, &[0, 4, 8, 12], 50), row(38, &[4, 12], 50)], ..Pattern::default() };
    s.set_pattern(p, straight.clone()).unwrap();
    let played = bounce(&mut s, &scratch, end);
    let sixteenths = |k: usize| k as f64 / 4.0;
    let straight_notes = |bpm: f64| {
        let mut n: Vec<Note> = [0, 4, 8, 12].iter().map(|&k| note_at(sixteenths(k), sixteenths(k) + 0.125, 36, bpm)).chain([4, 12].iter().map(|&k| note_at(sixteenths(k), sixteenths(k) + 0.125, 38, bpm))).collect();
        n.sort_by_key(|n| (n.start, n.pitch));
        n
    };
    let want = by_hand(&mut s, &scratch, inst, p, bar, bar, straight_notes(120.0), end);
    let d = max_diff(&played, &want);
    check(&format!("straight steps play the notes worked out by hand (max diff {d:.2e}, onset {:?})", onset(&played)), d < 1e-6 && onset(&played) == Some(bar));

    // ---- swing, polymeter, another rate --------------------------------------------------------
    let mut hats = row(42, &(0..16).collect::<Vec<_>>(), 50);
    hats.steps[6].on = false;
    hats.steps[7] = Step { on: false, tie: true, ..Step::default() };
    let mut three = row(39, &[0], 25);
    three.length = Some(3);
    let mut eighths = row(49, &[0, 1], 100);
    eighths.rate = Some(8);
    eighths.length = Some(5);
    let swung = Pattern { swing: 50, rows: vec![hats, three, eighths], ..Pattern::default() };
    s.checkpoint("Swing", None, false);
    s.set_pattern(p, swung.clone()).unwrap();
    let played = bounce(&mut s, &scratch, end);
    // Swing 50 %: every odd step a quarter of a step late.
    let swing16 = |k: usize| sixteenths(k) + if k % 2 == 1 { 1.0 / 16.0 } else { 0.0 };
    let mut swung_notes = Vec::new();
    // Hats on every sixteenth but 6, and 7 (a tie after an off step plays nothing).
    for k in (0..16).filter(|&k| k != 6 && k != 7) {
        swung_notes.push(note(swing16(k), swing16(k) + 0.125, 42));
    }
    // A cycle of three sixteenths, quarter-step gates.
    for k in [0, 3, 6, 9, 12, 15] {
        swung_notes.push(note(swing16(k), swing16(k) + 0.0625, 39));
    }
    // Eighths in a cycle of five (steps 0 and 1 on): eighths 0, 1, 5 and 6, the odd ones a
    // quarter of an eighth late; a full gate ends a note where the next step starts.
    let swing8 = |k: usize| k as f64 / 2.0 + if k % 2 == 1 { 0.125 } else { 0.0 };
    for k in [0, 1, 5, 6] {
        swung_notes.push(note(swing8(k), (swing8(k) + 0.5).min(swing8(k + 1)), 49));
    }
    swung_notes.sort_by_key(|n| (n.start, n.pitch));
    let want = by_hand(&mut s, &scratch, inst, p, bar, bar, swung_notes.clone(), end);
    let d = max_diff(&played, &want);
    check(&format!("swing, a three-step row and a five-step eighth row play as worked out (max diff {d:.2e})"), d < 1e-6);
    // Just the hats, from step 1: the first sound is the swung step, a quarter step late.
    let mut solo = swung.clone();
    solo.rows[0].steps[0].on = false;
    solo.rows[0].solo = true;
    s.checkpoint("Solo", None, false);
    s.set_pattern(p, solo).unwrap();
    let only = bounce(&mut s, &scratch, end);
    s.undo();
    check(&format!("the first swung hat starts a quarter step late ({:?}, want {})", onset(&only), bar + at(swing16(1), 120.0)), onset(&only) == Some(bar + at(swing16(1), 120.0)));

    // A tie carries the note before it on: one note two steps long.
    s.checkpoint("Tie", None, false);
    let mut tie_row = row(42, &[0], 100);
    tie_row.steps.resize(2, Step::default());
    tie_row.steps[1].tie = true;
    s.set_pattern(p, Pattern { rows: vec![tie_row], ..Pattern::default() }).unwrap();
    let played = bounce(&mut s, &scratch, end);
    let want = by_hand(&mut s, &scratch, inst, p, bar, bar, vec![note(0.0, 0.5, 42)], end);
    check(&format!("a tie lengthens the note before it (max diff {:.2e})", max_diff(&played, &want)), max_diff(&played, &want) < 1e-6);
    s.undo();

    // ---- edits undo -----------------------------------------------------------------------------
    let before = bounce(&mut s, &scratch, end);
    let key = Some(format!("pattern:{p}:drag"));
    for k in [1, 2] {
        // A drag's steps share a key: one undo step.
        s.checkpoint("Toggle Steps", key.clone(), false);
        let mut next = region(&s, p).pattern.unwrap();
        next.rows[1].steps.resize(3, Step::default());
        next.rows[1].steps[k].on = true;
        s.set_pattern(p, next).unwrap();
    }
    let edited = bounce(&mut s, &scratch, end);
    s.undo();
    let undone = bounce(&mut s, &scratch, end);
    check(
        &format!("a painted run of steps changes the sound and undoes in one step (diff {:.2e}, after undo {:.2e})", max_diff(&edited, &before), max_diff(&undone, &before)),
        max_diff(&edited, &before) > 1e-3 && max_diff(&undone, &before) < 1e-6 && region(&s, p).pattern.as_ref() == Some(&swung),
    );

    // ---- a looping region repeats the pattern; a tempo change moves the steps -------------------
    s.checkpoint("Loop", None, false);
    s.set_pattern(p, straight.clone()).unwrap();
    s.set_region_loop(p, Some(3 * bar));
    let looped = bounce(&mut s, &scratch, end);
    let twice: Vec<Note> = straight_notes(120.0).into_iter().flat_map(|n| [n, Note { start: n.start + bar, ..n }]).collect();
    let want = by_hand(&mut s, &scratch, inst, p, bar, 2 * bar, twice, end);
    check(&format!("a looped pattern region plays the pattern again in its loop (max diff {:.2e})", max_diff(&looped, &want)), max_diff(&looped, &want) < 1e-6);
    s.set_region_loop(p, None);
    // The region keeps its place in samples (mid-bar at 100 BPM); its steps are 100 BPM
    // sixteenths from there.
    s.set_tempo(100.0);
    let slow = bounce(&mut s, &scratch, end);
    let want = by_hand(&mut s, &scratch, inst, p, bar, bar, straight_notes(100.0), end);
    check(&format!("steps follow a tempo change (max diff {:.2e}, onset {:?})", max_diff(&slow, &want), onset(&slow)), max_diff(&slow, &want) < 1e-6 && onset(&slow) == Some(bar));
    s.undo();

    // ---- Convert to MIDI Region renders identically ----------------------------------------------
    let pattern_render = bounce(&mut s, &scratch, end);
    s.checkpoint("Convert to MIDI Region", None, false);
    s.convert_patterns_to_midi(&[p]);
    let r = region(&s, p);
    let converted = bounce(&mut s, &scratch, end);
    let n = match &r.content {
        RegionContent::Midi { notes, .. } => notes.len(),
        _ => 0,
    };
    let d = max_diff(&converted, &pattern_render);
    check(&format!("Convert to MIDI Region writes the notes ({n}) and renders identically (max diff {d:.2e})"), r.pattern.is_none() && n == swung_notes.len() && d < 1e-6);
    s.undo();
    check("undo makes it a pattern region again", region(&s, p).pattern.as_ref() == Some(&swung));

    // And back: MIDI to pattern (the swung notes sit off the grid, so the straight ones).
    s.checkpoint("Straight", None, false);
    s.set_pattern(p, straight.clone()).unwrap();
    s.convert_patterns_to_midi(&[p]);
    let midi = bounce(&mut s, &scratch, end);
    s.convert_midi_to_patterns(&[p]);
    let again = bounce(&mut s, &scratch, end);
    check(&format!("Convert to Pattern Region plays the same notes (max diff {:.2e})", max_diff(&again, &midi)), region(&s, p).pattern.is_some() && max_diff(&again, &midi) < 1e-6);
    s.undo();

    // A saved project keeps its pattern.
    let json = serde_json::to_string(&s.project).unwrap();
    let back: daw_engine::model::Project = serde_json::from_str(&json).unwrap();
    let kept = back.tracks.iter().flat_map(|t| t.regions.iter()).find(|r| r.id == p).and_then(|r| r.pattern.clone());
    check("the pattern survives saving", kept.as_ref() == Some(&swung));

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
