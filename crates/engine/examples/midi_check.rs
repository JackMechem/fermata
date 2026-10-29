//! Exercises MIDI region content against a running engine (silent ALSA null output): Piano
//! Roll edits, muted notes, controller playback and chasing, split/join/trim with controllers,
//! and MIDI file export and import. usage: midi_check <scratch dir>
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{CcEvent, CcKind, InputSel, Note, RegionContent, TrackKind};
use daw_engine::session::MidiContent;
use daw_engine::Session;

/// Plays the cycle for a moment and returns the track's peak level.
fn level(s: &mut Session, id: u64) -> f32 {
    s.locate(0);
    s.play();
    std::thread::sleep(Duration::from_millis(150));
    s.tick();
    std::thread::sleep(Duration::from_millis(250));
    let t = s.tick();
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    t.meters.iter().find(|m| m.0 == id).map(|m| m.1.max(m.2)).unwrap_or(-1.0)
}

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn midi(s: &Session, id: u64) -> (i64, i64, Vec<Note>, Vec<CcEvent>) {
    for t in &s.project.tracks {
        for r in &t.regions {
            if r.id == id {
                if let RegionContent::Midi { notes, cc } = &r.content {
                    return (r.start, r.length, notes.clone(), cc.clone());
                }
            }
        }
    }
    panic!("no region {id}");
}

fn main() {
    let scratch = std::path::PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    std::fs::create_dir_all(&scratch).unwrap();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("MidiCheck.fermata");
    let bar = s.project.samples_per_bar().round() as i64;
    let beat = s.project.samples_per_beat().round() as i64;

    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.select_track(Some(inst));
    let r = s.create_midi_region(inst, 0, 2 * bar).unwrap();
    s.set_cycle(true, 0, 2 * bar);
    let note = |start: i64, pitch: u8, muted: bool| Note { start, length: beat, pitch, velocity: 100, muted };
    let mod_ = CcKind::Controller(1);
    let expr = CcKind::Controller(11);
    let content = |notes: Vec<Note>, cc: Vec<CcEvent>| vec![MidiContent { region: r, notes, cc, start: None, length: None }];
    let chord: Vec<Note> = (0..8).flat_map(|i| [note(i * beat, 60, false), note(i * beat, 64, false)]).collect();

    s.checkpoint("Create Notes", None, false);
    s.set_midi_content(content(chord.clone(), vec![]));
    let lv = level(&mut s, inst);
    check(&format!("notes play ({lv:.3})"), lv > 0.01);

    s.set_midi_content(content(chord.iter().map(|n| Note { muted: true, ..*n }).collect(), vec![]));
    let lv = level(&mut s, inst);
    check(&format!("muted notes are silent ({lv:.4})"), lv < 1e-4);

    // Expression at 0 from the start silences the synth; chased on every play.
    s.set_midi_content(content(chord.clone(), vec![CcEvent { start: 0, kind: expr, value: 0 }]));
    let lv = level(&mut s, inst);
    check(&format!("expression 0 silences ({lv:.4})"), lv < 1e-4);
    s.set_midi_content(content(chord.clone(), vec![CcEvent { start: 0, kind: expr, value: 127 }]));
    let lv = level(&mut s, inst);
    check(&format!("expression 127 plays ({lv:.3})"), lv > 0.01);

    // Unsorted, out-of-range input comes back normalised.
    let cc = vec![
        CcEvent { start: bar + beat, kind: mod_, value: 90 },
        CcEvent { start: 0, kind: mod_, value: 10 },
        CcEvent { start: beat, kind: CcKind::PitchBend, value: 20000 },
        CcEvent { start: bar / 2, kind: CcKind::Controller(64), value: 127 },
    ];
    s.set_midi_content(vec![MidiContent { region: r, notes: chord.iter().rev().copied().collect(), cc, start: None, length: Some(3 * bar) }]);
    let (_, len, notes, cc) = midi(&s, r);
    check("region grew to the given length", len == 3 * bar);
    check("notes sorted", notes.windows(2).all(|w| (w[0].start, w[0].pitch) <= (w[1].start, w[1].pitch)));
    check("cc sorted and clamped", cc.windows(2).all(|w| w[0].start <= w[1].start) && cc.iter().any(|c| c.kind == CcKind::PitchBend && c.value == 16383));

    // Undo puts the previous content back.
    let before = midi(&s, r).2.len();
    s.checkpoint("Delete Notes", None, false);
    s.set_midi_content(content(vec![], vec![]));
    s.undo();
    check("undo restores notes", midi(&s, r).2.len() == before);

    // Split at bar 2: the right part starts with the mod value in effect there.
    s.split_at(bar, &[r]);
    let right = s.project.track(inst).unwrap().regions.iter().find(|x| x.start == bar).unwrap().id;
    let (_, _, _, lc) = midi(&s, r);
    let (_, _, rn, rc) = midi(&s, right);
    check("left keeps the earlier events", lc.iter().all(|c| c.start < bar) && lc.len() == 3);
    check(
        &format!("right starts with chased mod, bend and sustain ({rc:?})"),
        rc.iter().any(|c| c.start == 0 && c.kind == mod_ && c.value == 10)
            && rc.iter().any(|c| c.start == 0 && c.kind == CcKind::PitchBend)
            && rc.iter().any(|c| c.start == beat && c.kind == mod_ && c.value == 90),
    );
    check("right notes are relative", rn.iter().all(|n| n.start >= 0) && rn.len() == 8);

    // Join them back.
    let joined = s.join_regions(&[r, right]).unwrap()[0];
    let (_, _, jn, jc) = midi(&s, joined);
    check("join merges notes", jn.len() == 16);
    check("join merges controllers in order", jc.windows(2).all(|w| w[0].start <= w[1].start) && jc.iter().any(|c| c.start == bar + beat && c.value == 90));

    // Trim the front by half a bar: the mod value set before carries to the new start.
    s.resize_region(joined, bar / 2 + beat / 2, 3 * bar - bar / 2 - beat / 2);
    let (st, _, _, tc) = midi(&s, joined);
    check(&format!("trim chases controllers ({tc:?})"), st == bar / 2 + beat / 2 && tc.first().map(|c| c.start == 0).unwrap_or(false));

    // Export and import round trip.
    let file = scratch.join("check.mid");
    s.export_midi(&[joined], &file).unwrap();
    let (st, _, en, ec) = midi(&s, joined);
    let made = s.import_midi(&file, None, 0, None).unwrap();
    check("import made one region", made.len() == 1);
    let (ist, _, inn, icc) = midi(&s, made[0]);
    let abs = |start: i64, ns: &[Note]| ns.iter().filter(|n| !n.muted && n.start >= 0).map(|n| (start + n.start, n.pitch)).collect::<Vec<_>>();
    let close = |a: &[(i64, u8)], b: &[(i64, u8)]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x.0 - y.0).abs() <= 2 && x.1 == y.1);
    check(&format!("imported notes land where they were ({} notes)", inn.len()), close(&abs(st, &en), &abs(ist, &inn)));
    check("imported controllers", icc.len() == ec.len());

    // A looped region exports every pass, as it plays.
    let (st, len, en, _) = midi(&s, joined);
    s.set_region_loop(joined, Some(st + 2 * len));
    let looped = scratch.join("looped.mid");
    s.export_midi(&[joined], &looped).unwrap();
    let made = s.import_midi(&looped, None, 0, None).unwrap();
    let (_, _, ln, _) = midi(&s, made[0]);
    let once = en.iter().filter(|n| !n.muted && n.start >= 0 && n.start < len).count();
    check(&format!("a loop exports both passes ({} of {} notes)", ln.len(), 2 * once), once > 0 && ln.len() == 2 * once);
    println!("tracks: {:?}", s.project.tracks.iter().map(|t| (t.name.clone(), t.regions.len())).collect::<Vec<_>>());
    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
