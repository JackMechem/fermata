//! Exercises automation curves, Stereo Out automation and stack automation in offline renders
//! (silent ALSA null output): a curved volume ramp follows its curve sample by sample, a
//! straight one is unchanged by curves existing, Stereo Out's volume lane shapes the bounce,
//! summing and folder stacks' lanes shape their subtracks, and each edit undoes.
//! usage: autolanes_check <audio file> <scratch dir>
use daw_engine::audio_io::AudioConfig;
use daw_engine::automation::value_at;
use daw_engine::dsp::volume_gain;
use daw_engine::model::{AutoMode, AutoParam, AutoPoint, Id, InputSel, TrackKind, MASTER_ID};
use daw_engine::session::TrackPatch;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn db(g: f32) -> f32 {
    20.0 * g.max(1e-12).log10()
}

fn pt(pos: i64, value: f32, curve: f32) -> AutoPoint {
    AutoPoint { pos, value, curve }
}

/// The full mix of the region's length, left channel.
fn render(s: &mut Session, len: i64) -> Vec<f32> {
    s.render(0, len, 0, None, &mut |_| {}).unwrap().swap_remove(0)
}

const WIN: usize = 1024;

/// The level of `x` against `base`, in dB, over each window from `from` to `to` (fractions of
/// the length), with the level the lane `pts` should give there: the RMS of the reference
/// scaled sample by sample by the lane's gain.
fn windows(x: &[f32], base: &[f32], pts: &[AutoPoint], from: f64, to: f64) -> Vec<(usize, f32, f32)> {
    let n = x.len().min(base.len());
    let (a, b) = ((n as f64 * from) as usize, (n as f64 * to) as usize);
    let mut out = Vec::new();
    let mut k = a;
    while k + WIN <= b {
        let (mut e, mut got, mut want) = (0.0f64, 0.0f64, 0.0f64);
        for i in k..k + WIN {
            let g = volume_gain(value_at(pts, i as i64).unwrap()) as f64;
            e += (base[i] as f64).powi(2);
            got += (x[i] as f64).powi(2);
            want += (base[i] as f64 * g).powi(2);
        }
        if e > 1e-9 {
            out.push((k, db((got / e).sqrt() as f32), db((want / e).sqrt() as f32)));
        }
        k += WIN * 4;
    }
    out
}

/// The largest difference between measured and expected levels, in dB.
fn worst(w: &[(usize, f32, f32)]) -> f32 {
    w.iter().map(|(_, got, want)| (got - want).abs()).fold(0.0, f32::max)
}

fn same(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

fn level(x: &[f32], base: &[f32], from: f64, to: f64) -> f32 {
    let n = x.len().min(base.len());
    let (a, b) = ((n as f64 * from) as usize, (n as f64 * to) as usize);
    let e: f64 = base[a..b].iter().map(|s| (*s as f64).powi(2)).sum();
    let g: f64 = x[a..b].iter().map(|s| (*s as f64).powi(2)).sum();
    db((g / e.max(1e-12)).sqrt() as f32)
}

fn lane(s: &mut Session, track: Id, param: AutoParam, pts: Vec<AutoPoint>) {
    s.checkpoint("Edit Automation", None, false);
    s.set_automation_lane(track, param, pts);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = args.next().expect("scratch dir");
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = std::path::PathBuf::from(&scratch).join("AutoLanesCheck.fermata");
    println!("audio: {:?}", s.audio_status);

    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), a, 0).unwrap();
    let len = s.project.track(a).unwrap().regions[0].length;
    let base = render(&mut s, len);
    check(&format!("the tone renders ({} frames)", base.len()), base.len() as i64 == len && base.iter().any(|x| x.abs() > 0.01));

    // A straight ramp: an old project's lane (no curve saved) and a curve-0 lane are the same.
    let old: Vec<AutoPoint> = serde_json::from_str(&format!(r#"[{{"pos":0,"value":-30.0}},{{"pos":{len},"value":0.0}}]"#)).unwrap();
    lane(&mut s, a, AutoParam::Volume, old.clone());
    let straight = render(&mut s, len);
    let w = windows(&straight, &base, &old, 0.02, 0.98);
    check(&format!("a straight ramp renders its line (worst {:.3} dB over {} windows)", worst(&w), w.len()), worst(&w) < 0.3 && w.len() > 20);
    let flat = vec![pt(0, -30.0, 0.0), pt(len, 0.0, 0.0)];
    lane(&mut s, a, AutoParam::Volume, flat);
    check("curve 0 renders bit-identically to a lane saved without curves", same(&render(&mut s, len), &straight));

    // The same ramp bent: the change comes early.
    let bent = vec![pt(0, -30.0, 0.6), pt(len, 0.0, 0.0)];
    lane(&mut s, a, AutoParam::Volume, bent.clone());
    let curved = render(&mut s, len);
    let w = windows(&curved, &base, &bent, 0.1, 0.98);
    let mid = level(&curved, &base, 0.49, 0.51);
    for (k, got, want) in w.iter().step_by(4) {
        println!("  {:>6}: {got:7.2} dB, expected {want:7.2} dB", k);
    }
    check(&format!("a curved ramp follows its curve (worst {:.3} dB over {} windows)", worst(&w), w.len()), worst(&w) < 0.3 && w.len() > 20);
    check(&format!("the curve bends the ramp (middle {mid:.1} dB, the line gives -15 dB)"), mid > -8.0);
    let inv = vec![pt(0, -30.0, -0.6), pt(len, 0.0, 0.0)];
    lane(&mut s, a, AutoParam::Volume, inv.clone());
    let late = render(&mut s, len);
    let w = windows(&late, &base, &inv, 0.02, 0.98);
    // Lanes are read once per audio block and the fader is smoothed, so the steep end of this
    // curve lags a little more.
    check(&format!("a negative curve follows its curve too (worst {:.3} dB, middle {:.1} dB)", worst(&w), level(&late, &base, 0.49, 0.51)), worst(&w) < 0.6 && level(&late, &base, 0.49, 0.51) < -22.0);
    s.undo();
    check("undo takes the curve back", same(&render(&mut s, len), &curved));
    s.undo();
    s.undo();
    check("and back to the straight ramp", same(&render(&mut s, len), &straight));
    s.undo();
    check("and to no automation", same(&render(&mut s, len), &base) && s.project.track(a).unwrap().automation.is_empty());

    // Stereo Out: its volume lane shapes the whole bounce.
    lane(&mut s, MASTER_ID, AutoParam::Volume, vec![pt(0, -20.0, 0.0), pt(len / 2, -20.0, 0.0), pt(len, -2.0, -0.5)]);
    let out = render(&mut s, len);
    let (early, end) = (level(&out, &base, 0.1, 0.45), level(&out, &base, 0.97, 0.99));
    let w = windows(&out, &base, &s.project.master.automation[0].points.clone(), 0.02, 0.98);
    check(&format!("Stereo Out's volume lane shapes the bounce ({early:.2} dB, then {end:.2} dB; worst {:.3} dB)", worst(&w)), (early + 20.0).abs() < 0.1 && worst(&w) < 0.3);
    s.set_auto_mode(MASTER_ID, AutoMode::Off);
    check("Off leaves Stereo Out at its fader", same(&render(&mut s, len), &base));
    s.set_auto_mode(MASTER_ID, AutoMode::Read);
    s.set_master_volume(-6.0);
    check("in Read the lane overrides the fader", same(&render(&mut s, len), &out));
    s.set_master_volume(0.0);
    s.undo();
    check("undo removes Stereo Out's lane", s.project.master.automation.is_empty() && same(&render(&mut s, len), &base));

    // A summing stack: its volume lane scales what its subtracks sum to.
    s.checkpoint("Create Track Stack", None, false);
    let sum = s.create_stack(&[a], true).unwrap();
    let stacked = render(&mut s, len);
    lane(&mut s, sum, AutoParam::Volume, vec![pt(0, -12.0, 0.0)]);
    let g = level(&render(&mut s, len), &stacked, 0.1, 0.9);
    check(&format!("a summing stack's volume lane scales its subtracks' sum ({g:.2} dB)"), (g + 12.0).abs() < 0.1);
    s.undo();
    check("undo takes it back", same(&render(&mut s, len), &stacked));
    s.undo();

    // A folder stack has no audio: its lanes scale and mute its subtracks.
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(std::path::Path::new(&tone), b, 0).unwrap();
    s.update_track(a, TrackPatch { mute: Some(true), ..Default::default() });
    let one = render(&mut s, len);
    s.checkpoint("Create Track Stack", None, false);
    let folder = s.create_stack(&[b], false).unwrap();
    s.update_track(folder, TrackPatch { volume_db: Some(-6.0), ..Default::default() });
    let foldered = render(&mut s, len);
    check(&format!("the folder's fader scales its subtrack ({:.2} dB)", level(&foldered, &one, 0.1, 0.9)), (level(&foldered, &one, 0.1, 0.9) + 6.0).abs() < 0.1);
    let ramp = vec![pt(0, -24.0, 0.4), pt(len, 0.0, 0.0)];
    lane(&mut s, folder, AutoParam::Volume, ramp.clone());
    let r = render(&mut s, len);
    let w = windows(&r, &one, &ramp, 0.05, 0.98);
    check(&format!("a folder's volume lane drives its subtrack (worst {:.3} dB)", worst(&w)), worst(&w) < 0.4 && w.len() > 20);
    // The subtrack's own lane still applies on top.
    lane(&mut s, b, AutoParam::Volume, vec![pt(0, -10.0, 0.0)]);
    let both = render(&mut s, len);
    let shifted: Vec<AutoPoint> = ramp.iter().map(|p| pt(p.pos, p.value - 10.0, p.curve)).collect();
    let w = windows(&both, &one, &shifted, 0.05, 0.98);
    check(&format!("with the subtrack's own lane on top (worst {:.3} dB)", worst(&w)), worst(&w) < 0.4);
    s.undo();
    let half = len / 2;
    lane(&mut s, folder, AutoParam::Mute, vec![pt(0, 0.0, 0.0), pt(half, 0.0, 0.0), pt(half, 1.0, 0.0)]);
    let m = render(&mut s, len);
    let (on, off) = (level(&m, &one, 0.1, 0.45), level(&m, &r, 0.1, 0.45));
    check(&format!("a folder's mute lane mutes its subtrack (unchanged by {off:.2} dB, then {:.1} dB)", level(&m, &one, 0.55, 0.95)), off.abs() < 0.01 && on < -6.0 && level(&m, &one, 0.55, 0.95) < -80.0);
    // A nested folder follows the outer folder's lane.
    s.clear_automation(folder, Some(AutoParam::Mute));
    s.checkpoint("Create Track Stack", None, false);
    let outer = s.create_stack(&[folder], false).unwrap();
    lane(&mut s, outer, AutoParam::Volume, vec![pt(0, -10.0, 0.0)]);
    let nested = render(&mut s, len);
    let shifted: Vec<AutoPoint> = ramp.iter().map(|p| pt(p.pos, p.value - 10.0, p.curve)).collect();
    let w = windows(&nested, &one, &shifted, 0.05, 0.98);
    check(&format!("an outer folder's lane reaches through an inner one (worst {:.3} dB)", worst(&w)), worst(&w) < 0.4);
    for _ in 0..4 {
        s.undo();
    }
    check("undo of the folder lanes", s.project.track(folder).unwrap().automation.is_empty() && same(&render(&mut s, len), &foldered));
    s.set_automation_lane(folder, AutoParam::Volume, ramp);
    s.set_auto_mode(folder, AutoMode::Off);
    check("Off leaves the folder at its fader", same(&render(&mut s, len), &foldered));

    // Saving keeps the curves, Stereo Out's lanes and the shown lanes.
    s.set_auto_mode(folder, AutoMode::Read);
    s.set_automation_lane(MASTER_ID, AutoParam::Volume, vec![pt(0, -3.0, 0.25), pt(len, 0.0, 0.0)]);
    s.set_automation_lane(b, AutoParam::Pan, vec![pt(0, -0.5, -0.75), pt(len, 0.5, 0.0)]);
    s.set_auto_lanes(vec![(b, vec![AutoParam::Pan, AutoParam::Mute]), (MASTER_ID, vec![AutoParam::Volume])]);
    let path = s.save(None).unwrap();
    let mut t = Session::new(AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() }, Vec::new());
    t.open(&path).unwrap();
    let tb = t.project.track(b).unwrap();
    check(
        "a saved project keeps curves, Stereo Out's lanes and the shown lanes",
        t.project.master.automation[0].points[0].curve == 0.25 && tb.automation.iter().any(|l| l.points[0].curve == -0.75) && tb.auto_lanes.len() == 2 && t.project.master.auto_lanes.len() == 1,
    );
    check("and plays them as before", same(&render(&mut t, len), &render(&mut s, len)));

    // A folder's lane lines up with its subtrack under delay compensation: with a latent
    // plug-in elsewhere (the Linear Phase EQ, High), its mute still lands where it is drawn.
    s.clear_automation(MASTER_ID, None);
    s.clear_automation(b, None);
    s.clear_automation(folder, None);
    s.set_automation_lane(folder, AutoParam::Mute, vec![pt(0, 0.0, 0.0), pt(half, 0.0, 0.0), pt(half, 1.0, 0.0)]);
    // Where the subtrack falls silent: past the last sample above -60 dB of its level.
    let silent_from = |x: &[f32]| x.iter().rposition(|v| v.abs() > 1e-3 * 0.084).map(|i| i as i64 + 1).unwrap_or(0);
    let plain = silent_from(&render(&mut s, len));
    s.set_plugin(a, 0, "builtin:linear-phase-eq").unwrap();
    s.set_builtin_param(a, 0, daw_engine::fx::linear_phase_eq::QUALITY, 2.0).unwrap();
    s.idle();
    let latency = s.pdc_total();
    let compensated = silent_from(&render(&mut s, len));
    check(
        &format!("a folder's mute lands at its point under delay compensation ({latency} frames): at {compensated}, without it {plain} (drawn at {half})"),
        latency > 1000 && (compensated - plain).abs() < 64 && (0..3000).contains(&(plain - half)),
    );
    // And a bounce's last frames, still in the compensation delay when the range ends, follow
    // Stereo Out's lane too: a fade to silence stays silent to the end.
    s.clear_automation(folder, None);
    s.set_automation_lane(MASTER_ID, AutoParam::Volume, vec![pt(0, 0.0, 0.0), pt(half, 0.0, 0.0), pt(half, -96.0, 0.0)]);
    let faded = silent_from(&render(&mut s, len));
    check(&format!("Stereo Out's lane reaches the end of a bounce under delay compensation (silent from {faded})"), (faded - plain).abs() < 64);

    // An automated plug-in parameter moved by hand: in Read the lane still decides a bounce,
    // on Stereo Out and on a track alike (each render sends the lanes afresh).
    s.clear_automation(MASTER_ID, None);
    s.remove_plugin(a, 0);
    for track in [MASTER_ID, b] {
        use daw_engine::fx::gain::GAIN;
        s.set_plugin(track, 1, "builtin:gain").unwrap();
        let uid = if track == MASTER_ID { s.project.master.inserts[1].as_ref().unwrap().uid } else { s.project.track(b).unwrap().inserts[1].as_ref().unwrap().uid };
        // -6 dB: the Gain runs from -96 to +24 dB.
        s.set_automation_lane(track, AutoParam::Plugin { uid, param: GAIN, name: "Gain".into() }, vec![pt(0, 0.75, 0.0)]);
        let by_lane = render(&mut s, len);
        s.set_builtin_param(track, 1, GAIN, 0.0).unwrap();
        let after = render(&mut s, len);
        let name = if track == MASTER_ID { "Stereo Out" } else { "a track" };
        check(&format!("on {name}, a lane overrides a hand-moved parameter in a bounce ({:.2} dB)", level(&after, &by_lane, 0.1, 0.4)), same(&after, &by_lane));
        // In Off the lane is ignored: the parameter plays its own value (0 dB), as it will
        // when the project is opened again, not the lane's last.
        s.set_auto_mode(track, AutoMode::Off);
        let off = render(&mut s, len);
        s.set_auto_mode(track, AutoMode::Read);
        s.clear_automation(track, None);
        let own = render(&mut s, len);
        check(&format!("on {name}, Off plays the parameter's own value ({:.2} dB)", level(&off, &own, 0.1, 0.4)), same(&off, &own));
        s.remove_plugin(track, 1);
    }

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
