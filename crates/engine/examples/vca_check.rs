//! VCA faders and pre-fader side-chain keys, against a running engine (silent ALSA null
//! output): two tracks on a VCA at -6 dB render 6 dB lower, muting the VCA silences them, undo
//! restores the level, automated member volume still combines with the VCA, a frozen track
//! and Bounce still follow it; and a compressor keyed pre-fader from a track whose fader is at
//! -inf still compresses.
//! usage: vca_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};

use daw_engine::audio_io::AudioConfig;
use daw_engine::fx::compressor::param;
use daw_engine::model::{AutoParam, AutoPoint, Id, InputSel, TrackKind};
use daw_engine::session::{RenderOptions, TrackPatch, VcaPatch};
use daw_engine::Session;

static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

const LEN: i64 = 48000;

/// The full mix's RMS over the second half of `[0, LEN)` (after any compressor has settled),
/// in dBFS.
fn mix_db(s: &mut Session) -> f32 {
    let a = s.render(0, LEN, 0, None, &mut |_| {}).unwrap();
    let half = (LEN / 2) as usize;
    let (mut sum, mut n) = (0.0f64, 0usize);
    for c in &a {
        for x in &c[half.min(c.len())..] {
            sum += (*x as f64) * (*x as f64);
            n += 1;
        }
    }
    let rms = (sum / n.max(1) as f64).sqrt();
    if rms <= 1e-9 {
        -200.0
    } else {
        20.0 * rms.log10() as f32
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.1
}

fn level(s: &mut Session, vca: Id, db: f32) {
    s.update_vca(vca, VcaPatch { level_db: Some(db), ..Default::default() });
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("VcaCheck.fermata");
    let _ = std::fs::remove_dir_all(&s.dir);

    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    for t in [a, b] {
        s.import_audio(Path::new(&tone), t, 0).unwrap();
    }

    // ---- level, mute and undo ------------------------------------------------------------------
    let base = mix_db(&mut s);
    s.checkpoint("Create New VCA", None, false);
    let v = s.create_vca(&[a, b]);
    check(&format!("a VCA at 0 dB changes nothing ({base:.2} dBFS)"), near(mix_db(&mut s), base));
    s.checkpoint("VCA Level", None, false);
    level(&mut s, v, -6.0);
    let down = mix_db(&mut s);
    check(&format!("two tracks on a VCA at -6 dB render 6 dB lower ({:.2} dB)", down - base), near(down - base, -6.0));
    check("their own faders don't move", [a, b].iter().all(|t| s.project.track(*t).unwrap().volume_db == 0.0));

    s.update_vca(v, VcaPatch { mute: Some(true), ..Default::default() });
    let muted = mix_db(&mut s);
    check(&format!("muting the VCA silences them ({muted:.1} dBFS)"), muted < -150.0);
    check("without muting the tracks themselves", [a, b].iter().all(|t| !s.project.track(*t).unwrap().mute));
    s.update_vca(v, VcaPatch { mute: Some(false), ..Default::default() });
    check("unmuting brings them back", near(mix_db(&mut s), down));

    let label = s.undo();
    check(&format!("undo ({label:?}) restores the VCA's level"), s.vca(v).map(|x| x.level_db) == Some(0.0) && near(mix_db(&mut s), base));
    s.redo();
    check("redo puts it back to -6 dB", near(mix_db(&mut s), down));

    // ---- nested VCAs ---------------------------------------------------------------------------
    let top = s.create_vca(&[]);
    s.set_vca_parent(v, Some(top)).unwrap();
    level(&mut s, top, -4.0);
    check("a VCA's parent offsets its strips too (-6 + -4 dB)", near(mix_db(&mut s) - base, -10.0));
    check("a loop between VCAs is refused", s.set_vca_parent(top, Some(v)).is_err());
    level(&mut s, top, 0.0);

    // ---- automation of a member's own volume ---------------------------------------------------
    // Track b alone, its volume automated to -10 dB, on the VCA at -6 dB: -16 dB.
    level(&mut s, v, 0.0);
    s.update_track(a, TrackPatch { mute: Some(true), ..Default::default() });
    let solo_b = mix_db(&mut s);
    s.set_automation_lane(b, AutoParam::Volume, vec![AutoPoint { pos: 0, value: -10.0, curve: 0.0 }]);
    check(&format!("automation alone takes b to -10 dB ({:.2})", mix_db(&mut s) - solo_b), near(mix_db(&mut s) - solo_b, -10.0));
    level(&mut s, v, -6.0);
    let both = mix_db(&mut s) - solo_b;
    check(&format!("automated volume combines with the VCA ({both:.2} dB)"), near(both, -16.0));
    s.set_automation_lane(b, AutoParam::Volume, Vec::new());
    s.update_track(a, TrackPatch { mute: Some(false), ..Default::default() });

    // ---- freeze and bounce ---------------------------------------------------------------------
    let before = mix_db(&mut s);
    s.checkpoint("Freeze Track", None, true);
    let frozen = s.freeze_tracks(&[a], &mut |_| {});
    check(&format!("a member freezes ({:?})", frozen.as_ref().err()), frozen.is_ok() && s.project.track(a).unwrap().frozen.is_some());
    check(&format!("the frozen member still follows the VCA ({:.2} vs {before:.2} dBFS)", mix_db(&mut s)), near(mix_db(&mut s), before));
    level(&mut s, v, 0.0);
    check("and moves with it after freezing", near(mix_db(&mut s), base));
    level(&mut s, v, -6.0);

    let opts = RenderOptions { start: 0, end: LEN, tail_secs: 0.0, ..Default::default() };
    let low = s.bounce_project(&scratch.join("vca-low.wav"), &opts, &mut |_| {}).unwrap().peak_db;
    level(&mut s, v, 0.0);
    let high = s.bounce_project(&scratch.join("vca-high.wav"), &opts, &mut |_| {}).unwrap().peak_db;
    check(&format!("Bounce follows the VCA ({:.2} dB)", low - high), (low - high + 6.0).abs() < 0.1);

    // ---- a pre-fader side-chain key ------------------------------------------------------------
    s.delete_vca(v);
    s.update_track(b, TrackPatch { mute: Some(true), ..Default::default() });
    let c = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(Path::new(&tone), c, 0).unwrap();
    // Track a keys the compressor on c from a fader at -inf: post-fader it hears nothing.
    s.update_track(a, TrackPatch { volume_db: Some(-96.0), ..Default::default() });
    let open = mix_db(&mut s);
    s.set_plugin(c, 0, "builtin:compressor").unwrap();
    s.set_builtin_param(c, 0, param::THRESHOLD, -45.0).unwrap();
    s.set_builtin_param(c, 0, param::RATIO, 8.0).unwrap();
    s.set_sidechain(c, 0, Some(a)).unwrap();
    let post = mix_db(&mut s);
    check(&format!("keyed post-fader from a silent fader, the compressor stays open ({:.2} dB)", post - open), (post - open).abs() < 0.5);
    s.checkpoint("Side Chain", None, false);
    s.set_sidechain_pre(c, 0, true).unwrap();
    let pre = mix_db(&mut s);
    check(&format!("keyed pre-fader, it compresses ({:.2} dB)", pre - open), pre - open < -6.0);
    s.update_track(a, TrackPatch { mute: Some(true), ..Default::default() });
    check("the source's mute still silences a pre-fader key", (mix_db(&mut s) - open).abs() < 0.5);
    s.update_track(a, TrackPatch { mute: Some(false), ..Default::default() });
    s.undo();
    let slot = |s: &Session| s.project.track(c).unwrap().inserts[0].as_ref().map(|x| (x.sidechain, x.sidechain_pre));
    check("undo takes the key back to post-fader", slot(&s) == Some((Some(a), false)) && (mix_db(&mut s) - open).abs() < 0.5);
    s.redo();
    check("redo makes it pre-fader again", slot(&s) == Some((Some(a), true)) && near(mix_db(&mut s), pre));

    // ---- a VCA inside a folder stack whose volume is automated ---------------------------------
    // Track c alone, in a folder whose volume lane holds -10 dB, on a VCA at -6 dB: -16 dB.
    s.remove_plugin(c, 0);
    s.update_track(a, TrackPatch { mute: Some(true), ..Default::default() });
    let alone = mix_db(&mut s);
    let folder = s.create_stack(&[c], false).unwrap();
    s.set_automation_lane(folder, AutoParam::Volume, vec![AutoPoint { pos: 0, value: -10.0, curve: 0.0 }]);
    let lane = mix_db(&mut s) - alone;
    check(&format!("the folder's lane alone takes c to -10 dB ({lane:.2})"), near(lane, -10.0));
    let v = s.create_vca(&[c]);
    level(&mut s, v, -6.0);
    let both = mix_db(&mut s) - alone;
    check(&format!("the VCA still applies while the folder's lane plays ({both:.2} dB)"), near(both, -16.0));
    s.update_vca(v, VcaPatch { mute: Some(true), ..Default::default() });
    check("and its mute still mutes", mix_db(&mut s) < -150.0);

    let _ = std::fs::remove_dir_all(&s.dir);
    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
