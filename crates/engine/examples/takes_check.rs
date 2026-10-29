//! Exercises take folders, comping, cycle and autopunch recording, and markers against a
//! running engine (silent ALSA null output, no input, so recordings are silent and only their
//! layout is checked). usage: takes_check <audio file> <scratch dir>
use std::time::{Duration, Instant};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{ChannelEq, Id, InputSel, Region, RegionContent, TrackKind};

const BALLAST: usize = 40;
use daw_engine::session::{BitDepth, RenderOptions, TrackPatch};
use daw_engine::Session;

fn check(name: &str, ok: bool) -> bool {
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
    ok
}

fn peak(a: &[f32]) -> f32 {
    a.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

/// Largest sample-to-sample step: a click shows up as a jump far above the tone's own slope.
fn max_step(a: &[f32]) -> f32 {
    a.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()))
}

fn render(s: &mut Session, from: i64, to: i64) -> Vec<f32> {
    s.render(from, to, 0, None, &mut |_| {}).unwrap().swap_remove(0)
}

fn regions(s: &Session, track: Id) -> Vec<Region> {
    s.project.track(track).unwrap().regions.clone()
}

/// Records with the transport as it's set up, stopping once `passes - 1` cycle wraps have
/// happened and the last pass is 80% through (or, without a cycle, past `until`). Reports
/// whether the recorder kept up (no ring overruns), without which the takes can't line up.
fn record(s: &mut Session, cycle: Option<(i64, i64)>, passes: usize, until: i64) -> bool {
    s.record().unwrap();
    let over0 = s.tick().overruns;
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut wraps = 0;
    let mut last = s.position();
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_micros(300));
        let pos = s.position();
        match cycle {
            Some((cs, ce)) => {
                if pos < last - (ce - cs) / 2 {
                    wraps += 1;
                }
                if wraps + 1 >= passes && pos > cs + (ce - cs) * 4 / 5 && pos < ce {
                    break;
                }
                assert!(wraps < passes, "missed the stop point; the engine runs too fast for this cycle");
            }
            None if pos > until => break,
            None => {}
        }
        last = pos;
    }
    let over = s.tick().overruns - over0;
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    check(&format!("the recorder kept up ({over} overruns)"), over == 0)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = args.next().expect("scratch dir");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::path::PathBuf::from(&scratch).join("TakesCheck.fermata");
    let _ = std::fs::remove_dir_all(&s.dir);
    std::fs::create_dir_all(s.dir.join("Audio Files")).unwrap();
    s.prefs.recording.pre_roll_secs = 0.0;
    let sr = s.sample_rate() as i64;
    let mut ok = true;

    // ---- comping: three takes of the tone at 0, -6 and -12 dB ------------------------------
    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let len = 2 * sr;
    let mut ids = Vec::new();
    for gain in [0.0, -6.0, -12.0] {
        let id = s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
        s.resize_region(id, 0, len);
        s.set_region_gain(&[id], Some(gain), None);
        ids.push(id);
    }
    s.checkpoint("Pack Take Folder", None, false);
    let folder = s.pack_take_folder(&ids).unwrap();
    let r = regions(&s, a);
    let f = r[0].takes.clone().unwrap();
    ok &= check(
        &format!("pack makes one folder with three takes ({} regions, {} takes)", r.len(), f.takes.len()),
        r.len() == 1 && r[0].id == folder && f.takes.len() == 3 && f.whole_take() == Some(f.takes[2].id),
    );
    let take: Vec<Id> = f.takes.iter().map(|t| t.id).collect();

    // Reference: take 1 alone, per third.
    s.select_take(folder, take[0]);
    let whole = render(&mut s, 0, len);
    let third = len / 3;
    let guard = sr / 50;
    let window = |k: i64| ((k * third + guard) as usize, ((k + 1) * third - guard) as usize);
    let refs: Vec<f32> = (0..3).map(|k| peak(&whole[window(k).0..window(k).1])).collect();

    s.checkpoint("Edit Comp", None, false);
    for (k, t) in take.iter().enumerate() {
        s.set_comp_range(folder, Some(*t), k as i64 * third, (k as i64 + 1) * third);
    }
    let comp = &regions(&s, a)[0].takes.clone().unwrap().comp;
    ok &= check(&format!("swipes build a three-part comp ({comp:?})"), comp.len() == 3 && comp.iter().zip(&take).all(|(c, t)| c.take == *t));
    let out = render(&mut s, 0, len);
    let levels: Vec<f32> = (0..3).map(|k| peak(&out[window(k).0..window(k).1]) / refs[k as usize]).collect();
    let want = [1.0, 0.501, 0.251];
    ok &= check(&format!("each comp segment plays its take ({levels:.3?})"), levels.iter().zip(want).all(|(l, w)| (l - w).abs() < 0.02));
    let slope = max_step(&whole);
    for b in [third, 2 * third] {
        let (x0, x1) = ((b - sr / 50) as usize, (b + sr / 50) as usize);
        let step = max_step(&out[x0..x1]);
        let dip = out[x0..x1].chunks(sr as usize / 500).map(peak).fold(f32::MAX, f32::min);
        let quieter = refs[(b / third) as usize] * want[(b / third) as usize];
        ok &= check(
            &format!("no click at the boundary at {b} (step {step:.4} vs tone {slope:.4}, lowest {dip:.3} vs {quieter:.3})"),
            step <= slope * 1.2 && dip > quieter * 0.6,
        );
    }

    // A bounce hears the comp.
    let wav = std::path::PathBuf::from(&scratch).join("takes_check_bounce.wav");
    let o = RenderOptions { start: 0, end: len, tail_secs: 0.0, bit_depth: BitDepth::Float32, ..Default::default() };
    s.bounce_project(&wav, &o, &mut |_| {}).unwrap();
    let bounced = daw_engine::decode::decode(&wav).unwrap().channels.swap_remove(0);
    let lb: Vec<f32> = (0..3).map(|k| peak(&bounced[window(k).0..window(k).1]) / refs[k as usize]).collect();
    ok &= check(&format!("the bounce plays the comp ({lb:.3?})"), lb.iter().zip(want).all(|(l, w)| (l - w).abs() < 0.02));

    s.set_comp_range(folder, None, third, 2 * third);
    let gap = render(&mut s, 0, len);
    ok &= check("a deselected part of the comp is silent", peak(&gap[window(1).0..window(1).1]) < 1e-6);
    s.undo();
    s.checkpoint("Edit Comp", None, false);
    for (k, t) in take.iter().enumerate() {
        s.set_comp_range(folder, Some(*t), k as i64 * third, (k as i64 + 1) * third);
    }

    // ---- flatten ---------------------------------------------------------------------------
    s.checkpoint("Flatten Take Folder", None, false);
    let flat = s.flatten_take_folder(folder);
    let r = regions(&s, a);
    let spans: Vec<(i64, i64, f32)> = r
        .iter()
        .map(|x| match x.content {
            RegionContent::Audio { gain_db, .. } => (x.start, x.length, gain_db),
            _ => (0, 0, 0.0),
        })
        .collect();
    let want_spans = vec![(0, third, 0.0), (third, third, -6.0), (2 * third, len - 2 * third, -12.0)];
    ok &= check(&format!("flatten makes a region per comp segment ({spans:?})"), flat.len() == 3 && r.len() == 3 && r.iter().all(|x| x.takes.is_none()) && spans == want_spans);
    let offsets_ok = r.iter().zip(&f.takes).all(|(x, t)| matches!(x.content, RegionContent::Audio { clip, offset, .. } if clip == t.clip && offset == t.offset + x.start));
    ok &= check("flattened regions play their takes' audio", offsets_ok);
    let out2 = render(&mut s, 0, len);
    let l2: Vec<f32> = (0..3).map(|k| peak(&out2[window(k).0..window(k).1]) / refs[k as usize]).collect();
    ok &= check(&format!("the flattened regions sound like the comp ({l2:.3?})"), l2.iter().zip(want).all(|(l, w)| (l - w).abs() < 0.02));
    s.undo();
    ok &= check("undo brings the folder back", regions(&s, a).len() == 1 && regions(&s, a)[0].takes.as_ref().map(|f| f.comp.len()) == Some(3));

    s.checkpoint("Flatten and Merge", None, false);
    let merged = s.merge_take_folder(folder).unwrap();
    let r = regions(&s, a);
    let out3 = render(&mut s, 0, len);
    let l3: Vec<f32> = (0..3).map(|k| peak(&out3[window(k).0..window(k).1]) / refs[k as usize]).collect();
    ok &= check(&format!("flatten and merge renders the comp to one region ({l3:.3?})"), merged.len() == 1 && r.len() == 1 && r[0].takes.is_none() && l3.iter().zip(want).all(|(l, w)| (l - w).abs() < 0.02));
    s.undo();

    s.checkpoint("Delete Unused Takes", None, false);
    s.select_take(folder, take[1]);
    s.delete_unused_takes(folder);
    ok &= check("delete unused takes keeps the one in use", regions(&s, a)[0].takes.as_ref().map(|f| f.takes.len()) == Some(1));
    s.undo();
    ok &= check("undo restores all takes and the comp", regions(&s, a)[0].takes.as_ref().map(|f| (f.takes.len(), f.comp.len())) == Some((3, 3)));

    // ---- markers -----------------------------------------------------------------------------
    s.checkpoint("Create Marker", None, false);
    let bar = s.project.samples_per_bar().round() as i64;
    let m1 = s.add_marker(4 * bar, None);
    s.checkpoint("Create Marker", None, false);
    let m2 = s.add_marker(bar, Some("Verse".into()));
    let names: Vec<(String, i64)> = s.project.markers.iter().map(|m| (m.name.clone(), m.pos)).collect();
    ok &= check(&format!("markers are kept in order ({names:?})"), names == vec![("Verse".into(), bar), ("Marker 1".into(), 4 * bar)]);
    s.checkpoint("Rename Marker", None, false);
    s.rename_marker(m1, "Chorus".into());
    s.checkpoint("Move Marker", None, false);
    s.move_marker(m2, 0);
    s.set_marker_color(m1, Some("#e5483a".into()));
    s.undo();
    s.undo();
    ok &= check("undo moves the marker back", s.project.markers.iter().find(|m| m.id == m2).map(|m| m.pos) == Some(bar));
    s.redo();
    s.undo();
    s.undo();
    ok &= check("undo removes a marker", s.project.markers.len() == 1);
    s.redo();
    s.redo();
    s.redo();
    s.set_punch(true, 2 * bar, 3 * bar);

    // ---- save and load -----------------------------------------------------------------------
    let dir = s.save(None).unwrap();
    s.open(&dir).unwrap();
    let r = regions(&s, a);
    let markers: Vec<(String, i64, Option<String>)> = s.project.markers.iter().map(|m| (m.name.clone(), m.pos, m.color.clone())).collect();
    ok &= check(&format!("markers load ({markers:?})"), markers == vec![("Verse".into(), 0, None), ("Chorus".into(), 4 * bar, Some("#e5483a".into()))]);
    ok &= check("the punch range loads", s.project.punch.range() == Some((2 * bar, 3 * bar)));
    ok &= check("the folder loads with its comp", r.len() == 1 && r[0].takes.as_ref().map(|f| (f.takes.len(), f.comp.len())) == Some((3, 3)));
    let out4 = render(&mut s, 0, len);
    let l4: Vec<f32> = (0..3).map(|k| peak(&out4[window(k).0..window(k).1]) / refs[k as usize]).collect();
    ok &= check(&format!("and plays the comp ({l4:.3?})"), l4.iter().zip(want).all(|(l, w)| (l - w).abs() < 0.02));
    let comp_saved = render(&mut s, 0, len);
    s.checkpoint("Delete Marker", None, false);
    s.delete_markers(&[m1, m2]);
    s.undo();
    ok &= check("deleting markers undoes", s.project.markers.len() == 2);

    // ---- cycle recording: three passes make a take folder ----------------------------------
    // The null device runs flat out, far faster than the recorder drains its rings, so give
    // the engine work to do (busy Channel EQs) until it runs nearer real time.
    let mut eq = ChannelEq::default();
    eq.bands.iter_mut().for_each(|b| b.gain = 3.0);
    let ballast: Vec<Id> = (0..BALLAST).map(|_| s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap()).collect();
    for &t in &ballast {
        s.set_eq(t, eq.clone());
    }
    s.set_punch(false, 2 * bar, 3 * bar);
    s.update_track(a, TrackPatch { armed: Some(false), ..Default::default() });
    let rec = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.update_track(rec, TrackPatch { armed: Some(true), ..Default::default() });
    let (cs, ce) = (2 * bar, 6 * bar);
    s.set_cycle(true, cs, ce);
    s.locate(0);
    let lat = (s.latency().compensation_ms as f64 * sr as f64 / 1000.0).round() as i64;
    ok &= record(&mut s, Some((cs, ce)), 3, 0);
    let r = regions(&s, rec);
    let f = r.first().and_then(|x| x.takes.clone());
    let takes = f.as_ref().map(|f| f.takes.len()).unwrap_or(0);
    ok &= check(&format!("three cycle passes make a folder with three takes ({} regions, {takes} takes)", r.len()), r.len() == 1 && takes == 3);
    if let (Some(f), true) = (f, takes == 3) {
        let l = ce - cs;
        ok &= check(&format!("the folder spans the cycle ({}..{})", r[0].start, r[0].start + r[0].length), r[0].start == cs && r[0].length == l);
        let t = &f.takes;
        ok &= check(
            &format!("takes are a cycle apart in the recording ({:?}, latency {lat})", t.iter().map(|t| t.offset).collect::<Vec<_>>()),
            t[1].offset - t[0].offset == l && t[2].offset - t[1].offset == l && (t[0].offset - lat).abs() <= 2 && t.iter().all(|x| x.clip == t[0].clip),
        );
        ok &= check(
            &format!("the last pass is kept part-way ({}..{}) and heard, the one before fills in ({:?})", t[2].start, t[2].end, f.comp),
            t[0].end == l && t[1].end == l && t[2].end >= l * 4 / 5 - sr / 10 && t[2].end < l && f.comp.len() == 2 && f.comp[0].take == t[2].id && f.comp[1].take == t[1].id,
        );
        ok &= check("takes are named in order", t.iter().map(|x| x.name.as_str()).collect::<Vec<_>>() == ["Take 1", "Take 2", "Take 3"]);
    }
    // Recording the same cycle again adds to the folder.
    s.locate(cs);
    ok &= record(&mut s, Some((cs, ce)), 2, 0);
    let r = regions(&s, rec);
    let names: Vec<String> = r[0].takes.as_ref().map(|f| f.takes.iter().map(|t| t.name.clone()).collect()).unwrap_or_default();
    ok &= check(&format!("another cycle recording adds takes to the folder ({names:?})"), r.len() == 1 && names.len() == 5 && names[4] == "Take 5");
    s.undo();
    ok &= check("undo removes them again", regions(&s, rec)[0].takes.as_ref().map(|f| f.takes.len()) == Some(3));

    // ---- autopunch -------------------------------------------------------------------------
    s.set_cycle(false, cs, ce);
    let punched = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.update_track(rec, TrackPatch { armed: Some(false), ..Default::default() });
    s.update_track(punched, TrackPatch { armed: Some(true), ..Default::default() });
    let (pin, pout) = (bar + bar / 3, 2 * bar + bar / 2);
    s.set_punch(true, pin, pout);
    s.locate(0);
    ok &= record(&mut s, None, 1, pout + bar);
    let r = regions(&s, punched);
    let got: Vec<(i64, i64, i64)> = r.iter().filter_map(|x| match x.content {
        RegionContent::Audio { offset, .. } => Some((x.start, x.length, offset)),
        _ => None,
    }).collect();
    ok &= check(&format!("autopunch trims the take to the punch range ({got:?}, want {pin}..{pout})"), got.len() == 1 && got[0].0 == pin && got[0].1 == pout - pin && (got[0].2 - lat).abs() <= 2);
    s.undo();

    // Autopunch inside the cycle: every pass is punched.
    let (pin, pout) = (cs + bar / 2, cs + 2 * bar);
    s.set_punch(true, pin, pout);
    s.set_cycle(true, cs, ce);
    s.locate(cs);
    ok &= record(&mut s, Some((cs, ce)), 2, 0);
    let r = regions(&s, punched);
    let f = r.first().and_then(|x| x.takes.clone());
    let offs: Vec<i64> = f.as_ref().map(|f| f.takes.iter().map(|t| t.offset).collect()).unwrap_or_default();
    ok &= check(
        &format!("punching in cycle mode makes a folder over the punch range ({:?}, offsets {offs:?})", r.iter().map(|x| (x.start, x.length)).collect::<Vec<_>>()),
        r.len() == 1 && r[0].start == pin && r[0].length == pout - pin && offs.len() == 2 && offs[1] - offs[0] == pout - pin + lat,
    );
    s.set_punch(false, pin, pout);

    // The comp still plays after all of this (undo and recording leave the folder alone).
    let again = render(&mut s, 0, len);
    ok &= check("the comped folder still plays as saved", again.iter().zip(&comp_saved).all(|(x, y)| (x - y).abs() < 1e-6));

    println!("{}", if ok { "ALL PASS" } else { "SOME FAILED" });
    std::process::exit(if ok { 0 } else { 1 });
}
