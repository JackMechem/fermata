//! Exercises sends, aux buses and output routing against a running engine (silent ALSA null
//! output). usage: sends_check <audio file> <scratch dir>
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, TrackKind, TrackSend, MASTER_ID};
use daw_engine::session::TrackPatch;
use daw_engine::Session;

/// Plays for a moment and returns the peak meter of each track id given, plus Stereo Out.
fn levels(s: &mut Session, ids: &[u64]) -> (Vec<f32>, f32) {
    s.locate(0);
    s.play();
    std::thread::sleep(Duration::from_millis(150));
    s.tick();
    std::thread::sleep(Duration::from_millis(250));
    let t = s.tick();
    s.stop();
    // play() is ignored until the engine has really stopped.
    std::thread::sleep(Duration::from_millis(60));
    let lv = ids.iter().map(|id| t.meters.iter().find(|m| m.0 == *id).map(|m| m.1.max(m.2)).unwrap_or(-1.0)).collect();
    (lv, t.master.0.max(t.master.1))
}

fn check(name: &str, ok: bool) -> bool {
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
    ok
}

fn patch(s: &mut Session, id: u64, p: TrackPatch) {
    s.update_track_linked(id, p);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = args.next().expect("scratch dir");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::path::PathBuf::from(&scratch).join("SendsCheck.fermata");
    let mut ok = true;

    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    s.import_audio(std::path::Path::new(&tone), b, 0).unwrap();
    s.set_cycle(true, 0, 48000);
    let x = s.create_aux(None).unwrap();
    let xt = s.project.track(x).unwrap();
    ok &= check(&format!("new aux is named after its bus ({}, bus {})", xt.name, xt.bus), xt.name == "Aux 1" && xt.bus == 1 && s.project.tracks.last().map(|t| t.id) == Some(x));

    let (lv, _) = levels(&mut s, &[a, x]);
    ok &= check(&format!("an aux without sends is silent (aux={:.4})", lv[1]), lv[1] < 1e-4);

    // Post-fader send at 0 dB: the aux gets the track's own (post-fader) signal.
    let send = |pre_fader| Some(TrackSend { bus: x, level_db: 0.0, pre_fader, muted: false });
    s.set_send(a, 0, send(false)).unwrap();
    let (lv, _) = levels(&mut s, &[a, x]);
    let diff = 20.0 * (lv[1] / lv[0]).log10();
    ok &= check(&format!("a 0 dB send delivers to the aux (track={:.3} aux={:.3}, {diff:+.2} dB)", lv[0], lv[1]), lv[1] > 0.01 && diff.abs() < 0.5);
    s.set_send_level(a, 0, -12.0);
    let (lv, _) = levels(&mut s, &[a, x]);
    let diff = 20.0 * (lv[1] / lv[0]).log10();
    ok &= check(&format!("send level -12 dB ({diff:+.2} dB)"), (diff + 12.0).abs() < 0.5);
    s.set_send_level(a, 0, 0.0);

    // Fader at -inf: the post-fader send goes quiet, a pre-fader one carries on.
    patch(&mut s, a, TrackPatch { volume_db: Some(-96.0), ..Default::default() });
    let (lv, _) = levels(&mut s, &[a, x]);
    ok &= check(&format!("post-fader send follows the fader down (aux={:.4})", lv[1]), lv[1] < 1e-4);
    s.set_send(a, 0, send(true)).unwrap();
    let (lv, master) = levels(&mut s, &[a, x]);
    ok &= check(&format!("pre-fader send delivers with the fader at -inf (track={:.4} aux={:.3} out={master:.3})", lv[0], lv[1]), lv[0] < 1e-4 && lv[1] > 0.01 && master > 0.01);

    // Muting the aux silences what arrives there (b is muted so only the aux could sound).
    patch(&mut s, b, TrackPatch { mute: Some(true), ..Default::default() });
    patch(&mut s, x, TrackPatch { mute: Some(true), ..Default::default() });
    let (lv, master) = levels(&mut s, &[x]);
    ok &= check(&format!("muting the aux silences it (aux={:.4} out={master:.4})", lv[0]), lv[0] < 1e-4 && master < 1e-4);
    patch(&mut s, x, TrackPatch { mute: Some(false), ..Default::default() });
    patch(&mut s, b, TrackPatch { mute: Some(false), ..Default::default() });
    patch(&mut s, a, TrackPatch { volume_db: Some(0.0), ..Default::default() });

    // Solo: soloing the source keeps its aux audible; soloing the aux keeps its source.
    patch(&mut s, a, TrackPatch { solo: Some(true), ..Default::default() });
    let (lv, _) = levels(&mut s, &[a, b, x]);
    ok &= check(&format!("soloing a track keeps its aux audible (a={:.3} b={:.4} aux={:.3})", lv[0], lv[1], lv[2]), lv[0] > 0.01 && lv[1] < 1e-4 && lv[2] > 0.01);
    patch(&mut s, a, TrackPatch { solo: Some(false), ..Default::default() });
    patch(&mut s, x, TrackPatch { solo: Some(true), ..Default::default() });
    let (lv, _) = levels(&mut s, &[a, b, x]);
    ok &= check(&format!("soloing an aux keeps its sources audible (a={:.3} b={:.4} aux={:.3})", lv[0], lv[1], lv[2]), lv[0] > 0.01 && lv[1] < 1e-4 && lv[2] > 0.01);
    patch(&mut s, x, TrackPatch { solo: Some(false), ..Default::default() });

    // Output routing: b plays only through the aux.
    s.set_send(a, 0, None).unwrap();
    s.set_output(b, Some(x)).unwrap();
    patch(&mut s, a, TrackPatch { mute: Some(true), ..Default::default() });
    let (lv, master) = levels(&mut s, &[b, x]);
    ok &= check(&format!("output to an aux (b={:.3} aux={:.3} out={master:.3})", lv[0], lv[1]), lv[1] > 0.01 && master > 0.01);
    patch(&mut s, x, TrackPatch { mute: Some(true), ..Default::default() });
    let (_, master) = levels(&mut s, &[b]);
    ok &= check(&format!("muting the aux silences tracks routed to it (out={master:.4})"), master < 1e-4);
    patch(&mut s, x, TrackPatch { mute: Some(false), ..Default::default() });
    patch(&mut s, a, TrackPatch { mute: Some(false), ..Default::default() });

    // An aux placed above its senders still plays after them.
    let mut ids: Vec<u64> = s.project.tracks.iter().map(|t| t.id).collect();
    ids.retain(|&i| i != x);
    ids.insert(0, x);
    s.reorder_tracks(ids);
    let (lv, _) = levels(&mut s, &[x]);
    ok &= check(&format!("an aux above its senders still receives (aux={:.3})", lv[0]), lv[0] > 0.01);

    // Feedback loops are refused.
    let y = s.create_aux(None).unwrap();
    s.set_send(x, 0, Some(TrackSend { bus: y, level_db: 0.0, pre_fader: false, muted: false })).unwrap();
    let back = s.set_send(y, 0, Some(TrackSend { bus: x, level_db: 0.0, pre_fader: false, muted: false }));
    ok &= check(&format!("a send that closes a loop is refused ({:?})", back.as_ref().err().map(|e| e.to_string())), back.is_err());
    ok &= check("an output that closes a loop is refused", s.set_output(y, Some(x)).is_err());
    ok &= check("an aux can't feed itself", s.set_output(x, Some(x)).is_err() && s.set_send(x, 1, Some(TrackSend { bus: x, level_db: 0.0, pre_fader: false, muted: false })).is_err());
    ok &= check("sends only go to auxes", s.set_send(a, 0, Some(TrackSend { bus: b, level_db: 0.0, pre_fader: false, muted: false })).is_err());
    let order = s.project.process_order();
    let pos = |id| order.iter().position(|&i| i == id).unwrap();
    ok &= check("process order puts receivers after senders", pos(b) < pos(x) && pos(x) < pos(y));

    // Duplicating copies sends; a duplicated aux gets its own bus.
    s.set_send(a, 2, Some(TrackSend { bus: x, level_db: -6.0, pre_fader: true, muted: false })).unwrap();
    let dup = s.duplicate_tracks(&[a], false)[0];
    ok &= check("duplicate copies the sends", s.project.track(dup).unwrap().sends[2] == s.project.track(a).unwrap().sends[2]);
    let xdup = s.duplicate_tracks(&[x], false)[0];
    ok &= check(&format!("a duplicated aux gets a new bus (bus {})", s.project.track(xdup).unwrap().bus), s.project.track(xdup).unwrap().bus == 3);
    s.delete_tracks(&[dup, xdup]);

    // Delete Unused Tracks keeps auxes that something feeds.
    let lone = s.create_aux(None).unwrap();
    let n = s.delete_unused_tracks();
    ok &= check(&format!("delete unused keeps fed auxes ({n} deleted)"), n == 1 && s.project.track(lone).is_none() && s.project.track(x).is_some() && s.project.track(y).is_some());

    // Save and reopen: the graph is rebuilt with its routing.
    let saved = s.save(Some(std::path::PathBuf::from(&scratch).join("SendsReload.fermata"))).unwrap();
    s.open(&saved).unwrap();
    s.set_cycle(true, 0, 48000);
    let (lv, _) = levels(&mut s, &[x, y]);
    ok &= check(&format!("routing survives save and open (aux={:.3} chained aux={:.3})", lv[0], lv[1]), lv[0] > 0.01 && lv[1] > 0.01);

    // Deleting the aux clears what pointed at it; undo puts it all back.
    s.checkpoint("Delete Track", None, true);
    s.delete_tracks(&[x]);
    let ta = s.project.track(a).unwrap();
    let tb = s.project.track(b).unwrap();
    ok &= check("deleting the aux clears sends and outputs to it", ta.sends.iter().all(|x| x.is_none()) && tb.output.is_none());
    let (lv, _) = levels(&mut s, &[y]);
    ok &= check(&format!("the aux it fed goes quiet (aux={:.4})", lv[0]), lv[0] < 1e-4);
    s.undo();
    let ta = s.project.track(a).unwrap();
    let tb = s.project.track(b).unwrap();
    ok &= check("undo restores sends and outputs", ta.sends[2].map(|s| s.bus) == Some(x) && tb.output == Some(x) && s.project.track(x).unwrap().sends[0].map(|s| s.bus) == Some(y));
    let (lv, _) = levels(&mut s, &[x, y]);
    ok &= check(&format!("restored routing plays (aux={:.3} chained aux={:.3})", lv[0], lv[1]), lv[0] > 0.01 && lv[1] > 0.01);

    // Undo of a routing edit.
    s.checkpoint("Track Output", None, false);
    s.set_output(b, Some(MASTER_ID)).unwrap();
    s.undo();
    ok &= check("undo of an output change", s.project.track(b).unwrap().output == Some(x));

    println!("{}", if ok { "ALL PASS" } else { "SOME FAILED" });
    std::process::exit(if ok { 0 } else { 1 });
}
