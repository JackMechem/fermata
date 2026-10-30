//! Checks Track On/Off (silent ALSA null output, offline renders and a short live run).
//! usage: onoff_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, TrackKind, TrackSend};
use daw_engine::session::{BipOptions, TrackPatch};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

const LEN: i64 = 96_000;

fn render(s: &mut Session) -> Vec<Vec<f32>> {
    s.render(0, LEN, 0, None, &mut |_| {}).unwrap()
}

fn peak(a: &[Vec<f32>]) -> f32 {
    a.iter().flatten().fold(0.0, |m, x| m.max(x.abs()))
}

fn largest_step(a: &[f32]) -> f32 {
    a.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

fn off(s: &mut Session, id: Id, v: bool) {
    s.update_track(id, TrackPatch { off: Some(v), ..Default::default() });
}

fn tone_track(s: &mut Session, tone: &Path) -> Id {
    let t = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(tone, t, 0).unwrap();
    t
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = PathBuf::from(args.next().expect("audio file"));
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("OnOffCheck.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();

    // Two tracks in the mix: one off leaves the other, both off leave silence.
    let (a, b) = (tone_track(&mut s, &tone), tone_track(&mut s, &tone));
    let both = render(&mut s);
    let one = peak(&both) / 2.0;
    check(&format!("both tracks play (peak {:.3})", peak(&both)), peak(&both) > 0.1);
    off(&mut s, a, true);
    let alone = render(&mut s);
    check(&format!("an Off track leaves the mix (peak {:.3})", peak(&alone)), (peak(&alone) - one).abs() < 0.01);
    off(&mut s, b, true);
    check("with both off the render is silent", peak(&render(&mut s)) == 0.0);
    off(&mut s, a, false);
    off(&mut s, b, false);
    check("back on, the mix is the same bit for bit", render(&mut s) == both);

    // Mute stays independent.
    s.update_track(a, TrackPatch { mute: Some(true), ..Default::default() });
    off(&mut s, a, true);
    off(&mut s, a, false);
    check("Mute is left as it was", s.project.track(a).unwrap().mute && (peak(&render(&mut s)) - one).abs() < 0.01);
    s.update_track(a, TrackPatch { mute: Some(false), ..Default::default() });

    // Stacks take their subtracks along; each keeps its own switch.
    for summing in [false, true] {
        let stack = s.create_stack(&[a, b], summing).unwrap();
        off(&mut s, stack, true);
        check(&format!("an Off {} silences its subtracks", if summing { "summing stack" } else { "folder stack" }), peak(&render(&mut s)) == 0.0 && s.export_targets().is_empty());
        off(&mut s, stack, false);
        check("back on, they play", (peak(&render(&mut s)) - peak(&both)).abs() < 0.01);
        s.flatten_stack(stack);
    }

    // An Off aux passes nothing; the track sending to it plays on.
    let x = s.create_aux(None).unwrap();
    s.set_send(a, 0, Some(TrackSend { bus: x, level_db: 0.0, pre_fader: false, muted: false })).unwrap();
    off(&mut s, b, true);
    let sent = peak(&render(&mut s));
    off(&mut s, x, true);
    let dry = peak(&render(&mut s));
    check(&format!("an Off aux drops only the send ({sent:.3} -> {dry:.3})"), sent > dry * 1.5 && (dry - one).abs() < 0.01);
    off(&mut s, x, false);
    off(&mut s, b, false);
    s.set_send(a, 0, None).unwrap();

    // A side-chain key from an Off track is silence: the compressor it keys stops compressing.
    s.set_plugin(b, 0, "builtin:compressor").unwrap();
    s.set_sidechain(b, 0, Some(a)).unwrap();
    let info = daw_engine::fx::find("builtin:compressor").unwrap();
    let threshold = info.params.iter().find(|p| p.name.contains("Threshold")).unwrap();
    s.set_builtin_param(b, 0, threshold.id, threshold.min).unwrap();
    // Before the fader, so only the key comes from the other track, not its sound.
    s.set_sidechain_pre(b, 0, true).unwrap();
    s.update_track(a, TrackPatch { volume_db: Some(-96.0), ..Default::default() });
    let keyed = render(&mut s);
    off(&mut s, a, true);
    let unkeyed = render(&mut s);
    let tail = |r: &[Vec<f32>]| r[0][48_000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    check(&format!("a key from an Off track is silence: no gain reduction ({:.3}, keyed {:.3})", tail(&unkeyed), tail(&keyed)), tail(&unkeyed) > 0.01 && tail(&keyed) < 0.8 * tail(&unkeyed));
    off(&mut s, a, false);
    s.update_track(a, TrackPatch { volume_db: Some(0.0), ..Default::default() });
    s.remove_plugin(b, 0);

    // Delay compensation: a latent track that's off doesn't count, also while playing.
    s.set_plugin(a, 0, "builtin:linear-phase-eq").unwrap();
    let total = s.pdc_total();
    check(&format!("the latent track counts while on ({total} frames)"), total > 0);
    off(&mut s, a, true);
    check("and not while off", s.pdc_total() == 0);
    let plain = render(&mut s);
    off(&mut s, a, false);
    // Off halfway through playback: the other track glides from its compensation to none.
    let glide = s.render_with_edit(0, LEN, LEN / 2, &mut |s| off(s, a, true)).unwrap();
    let step = largest_step(&glide[0]);
    let own = largest_step(&plain[0]);
    check(&format!("switched off while playing, no click (largest step {step:.4}; a track's own {own:.4}, the mix's {:.4})", 2.0 * own), step < 2.3 * own);
    check("and the delays are gone", s.pdc_total() == 0);
    let back = s.render_with_edit(0, LEN, LEN / 2, &mut |s| off(s, a, false)).unwrap();
    let step = largest_step(&back[0]);
    check(&format!("switched on while playing, no click (largest step {step:.4})"), step < 2.3 * own);
    check("and the compensation is back", s.pdc_total() == total);
    s.remove_plugin(a, 0);

    // A frozen track that's off plays nothing, and on plays its freeze file.
    s.freeze_tracks(&[b], &mut |_| {}).unwrap();
    off(&mut s, a, true);
    let frozen = peak(&render(&mut s));
    off(&mut s, b, true);
    check("a frozen track that's off plays nothing", frozen > 0.05 && peak(&render(&mut s)) == 0.0);
    off(&mut s, b, false);
    check("on again, it plays its freeze file", (peak(&render(&mut s)) - frozen).abs() < 0.01);
    s.unfreeze_track(b).unwrap();

    // Renders of a track that's off are refused, and exports leave it out.
    check("Freeze refuses a track that's off", s.render_freeze(&[a], &mut |_| {}).is_err());
    check("so does Bounce in Place", s.render_in_place(&[a], &[], &BipOptions::default(), &mut |_| {}).is_err());
    check("exports leave it out", !s.export_targets().contains(&a) && s.export_targets().contains(&b));

    // Saved with the project.
    let dir = s.save(None).unwrap();
    s.open(&dir).unwrap();
    check("saved and reopened, it is still off and silent", s.project.track(a).unwrap().off && !s.project.track(b).unwrap().off && (peak(&render(&mut s)) - one).abs() < 0.01);

    // Live: an Off track's meter falls to nothing rather than freezing. Tracks meter while
    // playing; the null device runs as fast as it can.
    off(&mut s, a, false);
    s.set_cycle(true, 0, 48000);
    s.locate(0);
    s.play();
    std::thread::sleep(Duration::from_millis(100));
    s.tick();
    std::thread::sleep(Duration::from_millis(100));
    let level = |s: &mut Session, id: Id| s.tick().meters.iter().find(|m| m.0 == id).map(|m| m.1.max(m.2)).unwrap_or(-1.0);
    let on = level(&mut s, a);
    off(&mut s, a, true);
    std::thread::sleep(Duration::from_millis(100));
    s.tick();
    std::thread::sleep(Duration::from_millis(100));
    let (quiet, other) = {
        let t = s.tick();
        let at = |id: Id| t.meters.iter().find(|m| m.0 == id).map(|m| m.1.max(m.2)).unwrap_or(-1.0);
        (at(a), at(b))
    };
    s.stop();
    check(&format!("live, the meter falls to nothing ({on:.3} -> {quiet:.3}) and the other track plays on ({other:.3})"), on > 0.01 && quiet == 0.0 && other > 0.01);

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
