//! Exercises the tempo map and signature changes: the click rendered through the engine
//! across tempo and signature changes, plug-in transport positions, bar numbering, Move MIDI
//! Regions with Tempo, quantize, a MIDI file round trip of a tempo map, undo, and that a
//! project without changes keeps the old timing exactly. Runs against a session on the silent
//! ALSA null output. usage: tempo_check <scratch dir>
use std::sync::Arc;
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::dsp::{Cmd, Engine, EngineStatus};
use daw_engine::model::{InputSel, Note, Project, RegionContent, TrackKind};
use daw_engine::session::MidiContent;
use daw_engine::tempo::{Bbt, SigEvent, TempoEvent, TempoMap};
use daw_engine::Session;

fn check(name: &str, ok: bool) {
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn ev(pos: i64, bpm: f64, ramp: bool) -> TempoEvent {
    TempoEvent { pos, bpm, ramp }
}

fn sig(bar: u32, num: u32, den: u32) -> SigEvent {
    SigEvent { bar, num, den }
}

/// Plays `frames` from 0 through a bare engine with the metronome on and `map`, and returns
/// where each click starts and how loud it is.
fn render_clicks(map: TempoMap, frames: usize) -> Vec<(i64, f32)> {
    let sr = map.sample_rate();
    let (mut tx, rx) = rtrb::RingBuffer::new(64);
    let (gtx, _grx) = rtrb::RingBuffer::new(64);
    let (_live_tx, live_rx) = rtrb::RingBuffer::new(16);
    let (rec_tx, _rec_rx) = rtrb::RingBuffer::new(16);
    let (an_tx, _an_rx) = rtrb::RingBuffer::new(16);
    let mut engine = Engine::new(sr, rx, gtx, Arc::new(EngineStatus::default()), live_rx, rec_tx, an_tx);
    tx.push(Cmd::Tempo(Arc::new(map))).ok().unwrap();
    tx.push(Cmd::Metronome { on: true, count_in: false }).ok().unwrap();
    tx.push(Cmd::Play { from: 0 }).ok().unwrap();
    let mut out = vec![0.0f32; frames * 2];
    engine.process(&[], 0, &mut out, 2);
    // A click starts where the output wakes up after 5 ms of silence; its first sample is 0.
    let mut found: Vec<(i64, f32)> = Vec::new();
    let mut quiet = usize::MAX;
    for (i, f) in out.chunks(2).enumerate() {
        if f[0].abs() > 1e-4 {
            if quiet > 240 {
                found.push((i as i64 - 1, 0.0));
            }
            let last = found.last_mut().unwrap();
            last.1 = last.1.max(f[0].abs());
            quiet = 0;
        } else {
            quiet = quiet.saturating_add(1);
        }
    }
    found
}

fn midi_notes(s: &Session, id: u64) -> (i64, Vec<Note>) {
    for t in &s.project.tracks {
        for r in &t.regions {
            if r.id == id {
                if let RegionContent::Midi { notes, .. } = &r.content {
                    return (r.start, notes.clone());
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
    s.dir = scratch.join("TempoCheck.fermata");
    let sr = s.project.sample_rate as i64;
    let srf = sr as f64;
    let settle = || std::thread::sleep(Duration::from_millis(60));

    // ---- an empty map is the old timing, exactly ------------------------------------------
    let mut old = serde_json::to_value(Project::new("Old", 44100)).unwrap();
    let o = old.as_object_mut().unwrap();
    o.remove("tempo_map");
    o.remove("signatures");
    o.insert("tempo".into(), 97.0.into());
    o.insert("sig_num".into(), 7.into());
    o.insert("sig_den".into(), 8.into());
    let old: Project = serde_json::from_value(old).unwrap();
    let m = TempoMap::from_project(&old);
    let bar = old.samples_per_bar();
    check("a project saved before the tempo map loads with one tempo and signature", old.tempo_map.is_empty() && old.signatures.is_empty() && m.is_constant());
    check("its bar lines are where they always were", (0..500).all(|k| (m.bar_start(k + 1) - k as f64 * bar).abs() < 1e-6));
    let step = old.samples_per_beat() / 2.0;
    let mut g = m.grid_at_or_after(0.0, 2);
    let same_grid = (0..400i64).all(|k| {
        let ok = g.pos.round() as i64 == (k as f64 * step).round() as i64;
        g = m.grid_after(&g);
        ok
    });
    check("and its click grid (eighths) is the old k × step, rounded", same_grid);
    let clicks = render_clicks(TempoMap::constant(48000.0, 120.0, 4, 4), 4 * 48000);
    let want: Vec<i64> = (0..8).map(|k| k * 24000).collect();
    check(&format!("the engine clicks every beat at a constant 120 BPM ({} clicks)", clicks.len()), clicks.iter().map(|c| c.0).collect::<Vec<_>>() == want);

    // ---- tempo and signature changes in the session ----------------------------------------
    // 120 BPM for bar 1 (2 s), 60 BPM from there; 7/8 at bar 3 and 3/4 at bar 5.
    s.checkpoint("Tempo", None, false);
    s.set_tempo_map(vec![ev(0, 120.0, false), ev(2 * sr, 60.0, false)], false);
    s.set_signatures(vec![sig(3, 7, 8), sig(5, 3, 4)]);
    let map = s.tempo_map();
    check("the base tempo mirrors the first event", s.project.tempo == 120.0 && s.project.tempo_map.len() == 2);
    check("the base signature stays 4/4 with changes later", s.project.sig_num == 4 && s.project.sig_den == 4 && s.project.signatures.len() == 3);
    // Bars: 1 at 0 s, 2 at 2 s, 3 at 6 s (7/8 of eighths at 0.5 s), 4 at 9.5 s, 5 at 13 s.
    let secs = [0.0, 2.0, 6.0, 9.5, 13.0, 16.0];
    let bars_ok = secs.iter().enumerate().all(|(i, t)| (map.bar_start(i as i64 + 1) - t * srf).abs() < 1e-6);
    check("bars are numbered across the tempo and signature changes", bars_ok);
    check(
        "bar/beat positions count beats in the signature's denominator",
        map.bar_beat_at(7.0 * srf) == Bbt { bar: 3, beat: 3, div: 1, tick: 1 } && map.bar_beat_at(14.0 * srf) == Bbt { bar: 5, beat: 2, div: 1, tick: 1 },
    );

    // The click through the engine follows the same map: beats, then bars accented.
    let len = (16.0 * srf) as usize;
    let clicks = render_clicks(map.clone(), len);
    let want: Vec<i64> = map.grid(0.0, len as f64, 1).map(|g| g.pos.round() as i64).collect();
    let got: Vec<i64> = clicks.iter().map(|c| c.0).collect();
    check(&format!("clicks land on every beat across the changes ({} clicks)", got.len()), got == want);
    let downbeats: Vec<i64> = clicks.iter().filter(|c| c.1 > 0.4).map(|c| c.0).collect();
    let want_down: Vec<i64> = secs[..5].iter().map(|t| (t * srf).round() as i64).collect();
    check(&format!("accented clicks are the bar lines ({downbeats:?})"), downbeats == want_down);

    // Plug-ins hear the tempo, position and signature where the playhead is.
    s.locate((7.0 * srf) as i64);
    settle();
    let t = s.transport_info();
    check(
        &format!("plug-in transport at 7 s: {} BPM, {:.3} quarters, bar at {:.3}, {}/{}", t.tempo, t.position_quarters, t.bar_start_quarters, t.sig_num, t.sig_den),
        (t.tempo - 60.0).abs() < 1e-9 && (t.position_quarters - 9.0).abs() < 1e-9 && (t.bar_start_quarters - 8.0).abs() < 1e-9 && (t.sig_num, t.sig_den) == (7, 8),
    );

    // ---- undo brings the old map back to the engine ---------------------------------------
    s.checkpoint("Tempo", None, false);
    s.set_tempo_at((7.0 * srf) as i64, 90.0, false);
    settle();
    let changed = s.transport_info().tempo;
    s.undo();
    settle();
    let back = s.transport_info().tempo;
    check(&format!("editing the tempo at the playhead changes that event ({changed} BPM), undo restores it ({back})"), (changed - 90.0).abs() < 1e-9 && (back - 60.0).abs() < 1e-9 && s.project.tempo_map[1].bpm == 60.0);
    s.undo();
    settle();
    check("undoing the whole edit leaves one tempo again", s.project.tempo_map.is_empty() && s.project.signatures.is_empty() && s.transport_info().tempo == 120.0);
    s.redo();

    // ---- Move MIDI Regions with Tempo, and quantize ---------------------------------------
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let map = s.tempo_map();
    // A region on bar 3 with a note on its beat 2 (7/8: an eighth after the bar line).
    let r3 = map.bar_start(3).round() as i64;
    let rid = s.create_midi_region(inst, r3, (map.bar_start(4) - map.bar_start(3)).round() as i64).unwrap();
    let eighth = (map.sample_at(map.bar_start_q(3) + 0.5) - map.bar_start(3)).round() as i64;
    s.set_midi_content(vec![MidiContent { region: rid, notes: vec![Note { start: eighth, length: eighth, pitch: 60, velocity: 100, muted: false }], cc: vec![], start: None, length: None }]);
    let audio = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let _ = audio;
    // Twice as fast after bar 1: with Move MIDI the region follows its bar.
    s.checkpoint("Tempo", None, false);
    s.set_tempo_map(vec![ev(0, 120.0, false), ev(2 * sr, 120.0, false)], true);
    let map2 = s.tempo_map();
    let (start, notes) = midi_notes(&s, rid);
    let bb = map2.bar_beat_at((start + notes[0].start) as f64);
    check(&format!("Move MIDI keeps the region on bar 3 ({start} vs {:.0}) and its note on beat 2 ({bb:?})", map2.bar_start(3)), (start as f64 - map2.bar_start(3)).abs() <= 1.0 && bb.bar == 3 && bb.beat == 2 && bb.div == 1);
    s.undo();
    let (start, _) = midi_notes(&s, rid);
    s.checkpoint("Tempo", None, false);
    s.set_tempo_map(vec![ev(0, 120.0, false), ev(2 * sr, 120.0, false)], false);
    let (still, _) = midi_notes(&s, rid);
    check("without Move MIDI, regions keep their time", still == start);
    s.undo();

    // Quantize to sixteenths in the 60 BPM section: a sixteenth there is a quarter second.
    let map = s.tempo_map();
    let q16 = (srf * 60.0 / 120.0 / 4.0).round() as i64;
    let sixteenth = (srf / 4.0) as i64;
    let off = r3 + sixteenth * 3 + sixteenth / 3;
    s.set_midi_content(vec![MidiContent { region: rid, notes: vec![Note { start: off - r3, length: 100, pitch: 62, velocity: 100, muted: false }], cc: vec![], start: None, length: None }]);
    s.quantize_regions(&[rid], q16);
    let (start, notes) = midi_notes(&s, rid);
    check(&format!("quantize follows the tempo map ({} → {})", off, start + notes[0].start), start + notes[0].start == r3 + sixteenth * 3);
    let _ = map;

    // ---- a MIDI file carries the tempo map -------------------------------------------------
    // A ramp from 60 to 90 BPM over bar 2 too; files store it as steps.
    s.checkpoint("Tempo", None, false);
    s.set_tempo_map(vec![ev(0, 120.0, false), ev(2 * sr, 60.0, true), ev(6 * sr, 90.0, false)], false);
    let map = s.tempo_map();
    let late = map.bar_start(6).round() as i64;
    let rid2 = s.create_midi_region(inst, late, sr).unwrap();
    s.set_midi_content(vec![MidiContent { region: rid2, notes: vec![Note { start: 0, length: sr / 4, pitch: 64, velocity: 90, muted: false }], cc: vec![], start: None, length: None }]);
    let file = scratch.join("tempo_check.mid");
    s.export_midi(&[rid2], &file).unwrap();
    let parsed = daw_engine::smf::read(&std::fs::read(&file).unwrap()).unwrap();
    check(&format!("the exported tempo track has the changes ({} tempos, {} signatures)", parsed.tempos.len(), parsed.signatures.len()), parsed.tempos.len() > 10 && parsed.signatures.len() == 3);

    s.new_project();
    s.dir = scratch.join("TempoCheckImport.fermata");
    let fresh = &mut s;
    let made = fresh.import_midi(&file, None, 0, None).unwrap();
    let fm = fresh.tempo_map();
    let (start, notes) = midi_notes(fresh, made[0]);
    let at = start + notes[0].start;
    check(&format!("an empty project adopts the file's tempo map and signatures ({} events)", fm.events().len()), fm.signatures() == map.signatures() && (fm.bpm_at(srf) - 120.0).abs() < 0.01);
    check(&format!("bar positions survive the round trip ({at} vs {late})"), (at - late).abs() <= 2 && (fm.bar_start(6) - map.bar_start(6)).abs() <= 2.0);
    // The ramp comes back as steps whose durations add up.
    check("the ramp's duration survives as steps", (fm.bar_start(3) - map.bar_start(3)).abs() <= 2.0 && (fm.bpm_at(4.0 * srf) - map.bpm_at(4.0 * srf)).abs() < 2.0);

    // Into a project with music and another tempo, the file plays at its own times.
    let other = fresh.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    fresh.set_tempo_map(vec![ev(0, 100.0, false)], false);
    let made = fresh.import_midi(&file, Some(other), 0, None).unwrap();
    let (start, notes) = midi_notes(fresh, made[0]);
    check(&format!("into a project at another tempo, notes keep the file's timing ({} vs {late})", start + notes[0].start), (start + notes[0].start - late).abs() <= 2 && fresh.project.tempo == 100.0);
}
