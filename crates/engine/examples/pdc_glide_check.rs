//! Checks that delay compensation changes while playing don't click (silent ALSA null
//! output). A sine plays on one track while a latent effect is inserted on another, mid-render
//! with the transport running: the sine's compensation delay grows, and its output must glide
//! to the new delay instead of jumping. Then the same for a larger jump (the Linear Phase
//! EQ), for taking the effects out again, and for a send into an aux.
//! usage: pdc_glide_check <scratch dir>
use std::path::PathBuf;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, TrackKind, TrackSend};
use daw_engine::session::TrackPatch;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

const FREQ: f32 = 440.0;
const AMP: f32 = 0.5;
/// Two seconds, and the edit halfway through.
const LEN: i64 = 96_000;
const AT: i64 = 48_000;

fn largest_step(a: &[Vec<f32>]) -> f32 {
    a.iter().flat_map(|c| c.windows(2).map(|w| (w[1] - w[0]).abs())).fold(0.0, f32::max)
}

/// How far `out` is from `reference` moved by `shift` frames (out[n] against
/// reference[n - shift]), over `from..`.
fn off_from(out: &[Vec<f32>], reference: &[Vec<f32>], shift: i64, from: usize) -> f32 {
    let mut worst = 0.0f32;
    for (o, r) in out.iter().zip(reference) {
        for n in from..o.len() {
            let m = n as i64 - shift;
            if m >= 0 && (m as usize) < r.len() {
                worst = worst.max((o[n] - r[m as usize]).abs());
            }
        }
    }
    worst
}

/// The track's compensation delay on its output.
fn delay_of(s: &Session, track: Id) -> i64 {
    s.pdc_report().strips.iter().find(|x| x.track == track).map(|x| x.delay as i64).unwrap_or(0)
}

/// Renders with `edit` halfway and checks the result against `reference`: no step larger than
/// the sine's own (a little over, for the glide), and afterwards exactly the reference moved by
/// how much the delay changed.
fn glide(s: &mut Session, what: &str, track: Id, reference: &[Vec<f32>], edit: &mut dyn FnMut(&mut Session)) {
    let before = delay_of(s, track);
    // A render drops the latency Stereo Out had when it started, so the edit lands that much
    // earlier in it.
    let at = (AT - s.pdc_total() as i64) as usize;
    let out = s.render_with_edit(0, LEN, AT, edit).unwrap();
    let after = delay_of(s, track);
    let change = after - before;
    let sr = s.sample_rate() as f32;
    // The sine's own largest step, and what an instant jump of the delay would have done at
    // the edit: the reference just before against the reference `change` frames earlier.
    let own = AMP * std::f32::consts::TAU * FREQ / sr;
    let worst_click = 2.0 * AMP * (std::f32::consts::PI * FREQ * change.unsigned_abs() as f32 / sr).sin().abs();
    let click = reference.iter().map(|c| (c[(at as i64 - change).max(0) as usize] - c[at - 1]).abs()).fold(0.0f32, f32::max);
    let step = largest_step(&out);
    check(
        &format!("{what}: the delay moves {before} → {after} frames without a click (largest step {step:.4}, the sine's own {own:.4}; jumping would step {click:.3} here, up to {worst_click:.3})"),
        change != 0 && step < own * 1.15,
    );
    let settled = at + (0.015 * sr) as usize + 1024;
    let off = off_from(&out, reference, change, settled);
    let before_edit: Vec<Vec<f32>> = out.iter().map(|c| c[..at].to_vec()).collect();
    let untouched = off_from(&before_edit, reference, 0, 0);
    check(&format!("{what}: after the glide the output is the sine exactly {change} frames later (diff {off:.2e}; before the edit {untouched:.2e})"), off < 1e-6 && untouched < 1e-6);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let config = scratch.join("config");
    let _ = std::fs::remove_dir_all(&config);
    std::env::set_var("XDG_CONFIG_HOME", &config);
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, daw_engine::plugin_db::load_cache().unwrap_or_default());
    s.dir = scratch.join("PdcGlideCheck.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();

    // Three seconds of a 440 Hz sine.
    let sr = s.sample_rate() as u32;
    let sine: Vec<f32> = (0..sr * 3).map(|i| AMP * (std::f32::consts::TAU * FREQ * i as f32 / sr as f32).sin()).collect();
    let path = scratch.join("sine.wav");
    daw_engine::decode::write_wav(&path, sr, &[sine]).unwrap();

    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(&path, a, 0).unwrap();
    let b = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    let reference = s.render(0, LEN, 0, None, &mut |_| {}).unwrap();
    check(&format!("the sine plays (peak {:.3})", reference[0].iter().fold(0.0f32, |m, x| m.max(x.abs()))), reference[0].iter().any(|x| x.abs() > 0.4));

    // A small change: the Overdrive's oversampling latency on the other track.
    glide(&mut s, "Overdrive inserted on another track", a, &reference, &mut |s| {
        s.set_plugin(b, 0, "builtin:overdrive").unwrap();
    });
    // A large one: the Linear Phase EQ (with the Overdrive still there).
    glide(&mut s, "Linear Phase EQ inserted on another track", a, &reference, &mut |s| {
        s.set_plugin(b, 1, "builtin:linear-phase-eq").unwrap();
    });
    // Back to none.
    glide(&mut s, "both removed", a, &reference, &mut |s| {
        s.remove_plugin(b, 1);
        s.remove_plugin(b, 0);
    });

    // A send: the sine reaches an aux only through a pre-fader send (its own fader is down).
    // Another track sending to the same aux turns latent, so the sine's send waits for it.
    let x = s.create_aux(None).unwrap();
    s.set_send(a, 0, Some(TrackSend { bus: x, level_db: 0.0, pre_fader: true, muted: false })).unwrap();
    s.set_send(b, 0, Some(TrackSend { bus: x, level_db: 0.0, pre_fader: true, muted: false })).unwrap();
    s.update_track(a, TrackPatch { volume_db: Some(-200.0), ..Default::default() });
    let via_send = s.render(0, LEN, 0, None, &mut |_| {}).unwrap();
    check("the sine plays through the aux", via_send[0].iter().any(|v| v.abs() > 0.4));
    // The aux's own output delay doesn't change; the sine's send delay does.
    let out = s
        .render_with_edit(0, LEN, AT, &mut |s| {
            s.set_plugin(b, 0, "builtin:overdrive").unwrap();
        })
        .unwrap();
    let change = s.pdc_total() as i64;
    let step = largest_step(&out);
    let own = AMP * std::f32::consts::TAU * FREQ / sr as f32;
    check(&format!("a pre-fader send glides to its new delay (largest step {step:.4}, the sine's own {own:.4})"), step < own * 1.15 && change > 0);
    let settled = AT as usize + (0.015 * sr as f32) as usize + 1024;
    let off = off_from(&out, &via_send, change, settled);
    check(&format!("after it the aux plays the sine exactly {change} frames later (diff {off:.2e})"), off < 1e-5);

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
