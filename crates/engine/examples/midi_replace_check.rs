//! Records MIDI over a MIDI region against a running engine (silent ALSA null output), playing
//! the notes in through the same path as Musical Typing, and checks where they land in each
//! overlap mode: Replace (the setting, and the control bar's button) clears the recorded range,
//! Merge keeps what was there, and a new region is made otherwise.
//! usage: midi_replace_check <scratch dir>
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{CcEvent, CcKind, Id, InputSel, Note, Region, RegionContent, TrackKind};
use daw_engine::prefs::MidiOverlap;
use daw_engine::session::{MidiContent, TrackPatch};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

const MOD: CcKind = CcKind::Controller(1);

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

/// Records, playing note 72 (and the mod wheel at 33) once the playhead is inside `play`, and
/// stops once it reaches `stop_after` (or right after the note with a cycle).
fn record(s: &mut Session, play: (i64, i64), stop_after: Option<i64>) {
    s.record().unwrap();
    wait_for(s, play.0, play.1);
    s.send_midi([0xB0, 1, 33]);
    s.send_midi([0x90, 72, 90]);
    std::thread::sleep(Duration::from_millis(5));
    s.send_midi([0x80, 72, 0]);
    std::thread::sleep(Duration::from_millis(20));
    if let Some(at) = stop_after {
        wait_for(s, at, i64::MAX);
    }
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
}

fn midi_regions(s: &Session, track: Id) -> Vec<Region> {
    s.project.track(track).unwrap().regions.iter().filter(|r| matches!(r.content, RegionContent::Midi { .. })).cloned().collect()
}

/// Every note and controller event of the track's regions, on the timeline.
fn content(s: &Session, track: Id) -> (Vec<Note>, Vec<CcEvent>) {
    let mut notes = Vec::new();
    let mut cc = Vec::new();
    for r in midi_regions(s, track) {
        if let RegionContent::Midi { notes: n, cc: c } = &r.content {
            notes.extend(n.iter().map(|x| Note { start: x.start + r.start, ..*x }));
            cc.extend(c.iter().map(|x| CcEvent { start: x.start + r.start, ..*x }));
        }
    }
    notes.sort_by_key(|n| n.start);
    cc.sort_by_key(|c| c.start);
    (notes, cc)
}

fn main() {
    let scratch = std::path::PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    std::fs::create_dir_all(&scratch).unwrap();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("ReplaceCheck.fermata");
    s.prefs.recording.pre_roll_secs = 0.0;
    s.set_count_in(false);
    let bar = s.project.samples_per_bar().round() as i64;
    let beat = s.project.samples_per_beat().round() as i64;

    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.select_track(Some(inst));
    s.update_track(inst, TrackPatch { armed: Some(true), ..Default::default() });
    // Four bars of C3 on every beat; the mod wheel at 10, 90 from bar 2 beat 2, 70 from bar 4.
    let r = s.create_midi_region(inst, 0, 4 * bar).unwrap();
    let old: Vec<Note> = (0..16).map(|i| Note { start: i * beat, length: beat / 2, pitch: 60, velocity: 100, muted: false }).collect();
    let old_cc = vec![CcEvent { start: 0, kind: MOD, value: 10 }, CcEvent { start: bar + beat, kind: MOD, value: 90 }, CcEvent { start: 3 * bar, kind: MOD, value: 70 }];
    s.set_midi_content(vec![MidiContent { region: r, notes: old.clone(), cc: old_cc, start: None, length: None }]);
    let in_bar2 = |n: &Note| n.start >= bar && n.start < 2 * bar;
    let ours = |n: &Note| n.pitch == 72;

    // ---- Replace, in a cycle over bar 2: the whole cycle is the recorded range ------------------
    s.prefs.recording.midi_overlap = MidiOverlap::Replace;
    s.set_cycle(true, bar, 2 * bar);
    record(&mut s, (bar + beat, bar + 3 * beat), None);
    let (notes, cc) = content(&s, inst);
    check(&format!("replace keeps one region ({})", midi_regions(&s, inst).len()), midi_regions(&s, inst).len() == 1);
    check(
        &format!("the old notes in the cycle are gone ({} left)", notes.iter().filter(|n| n.pitch == 60).count()),
        notes.iter().filter(|n| n.pitch == 60).count() == 12 && !notes.iter().any(|n| n.pitch == 60 && in_bar2(n)),
    );
    check("the played note is in the cycle", notes.iter().filter(|n| ours(n)).count() == 1 && notes.iter().any(|n| ours(n) && in_bar2(n)));
    check(
        &format!("controllers: the old change in the range is replaced and its value comes back after it ({cc:?})"),
        !cc.iter().any(|c| c.value == 90 && c.start == bar + beat)
            && cc.iter().any(|c| c.value == 33 && c.start >= bar && c.start < 2 * bar)
            && cc.iter().any(|c| c.value == 90 && c.start == 2 * bar)
            && cc.iter().any(|c| c.value == 70 && c.start == 3 * bar),
    );
    s.undo();
    let (notes, _) = content(&s, inst);
    check(&format!("undo brings the old notes back ({})", notes.len()), notes.len() == 16 && !notes.iter().any(ours));

    // ---- Merge keeps what was there ---------------------------------------------------------
    s.prefs.recording.midi_overlap = MidiOverlap::Merge;
    record(&mut s, (bar + beat, bar + 3 * beat), None);
    let (notes, _) = content(&s, inst);
    check(&format!("merge adds to the region ({} notes)", notes.len()), notes.len() == 17 && midi_regions(&s, inst).len() == 1);
    s.undo();

    // ---- The control bar's Replace button wins over "Create a new region" ------------------------
    s.prefs.recording.midi_overlap = MidiOverlap::NewRegion;
    s.prefs.recording.replace = true;
    record(&mut s, (bar + beat, bar + 3 * beat), None);
    let (notes, _) = content(&s, inst);
    check(
        &format!("the Replace button replaces ({} regions, {} notes)", midi_regions(&s, inst).len(), notes.len()),
        midi_regions(&s, inst).len() == 1 && notes.len() == 13 && !notes.iter().any(|n| n.pitch == 60 && in_bar2(n)),
    );
    s.undo();
    s.prefs.recording.replace = false;
    record(&mut s, (bar + beat, bar + 3 * beat), None);
    check("without it a new region is made", midi_regions(&s, inst).len() == 2);
    s.undo();

    // ---- Replace without a cycle: from where recording started to where it stopped -------------
    s.prefs.recording.midi_overlap = MidiOverlap::Replace;
    s.set_cycle(false, bar, 2 * bar);
    s.locate(2 * bar);
    std::thread::sleep(Duration::from_millis(30));
    record(&mut s, (2 * bar + beat, 3 * bar), Some(3 * bar + beat + beat / 2));
    let (notes, _) = content(&s, inst);
    let kept_before = notes.iter().filter(|n| n.pitch == 60 && n.start < 2 * bar).count();
    let gone = !notes.iter().any(|n| n.pitch == 60 && n.start >= 2 * bar && n.start <= 3 * bar + beat);
    check(&format!("notes before the record start stay ({kept_before})"), kept_before == 8);
    check("notes from the record start to the stop are replaced", gone && notes.iter().filter(|n| ours(n)).count() == 1);

    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
