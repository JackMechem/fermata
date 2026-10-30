//! Plays, bounces and freezes the modulation, distortion and utility effects (silent ALSA null
//! output), and checks what is particular to them: a synced Tremolo locks to the song
//! position, the Distortion's oversampling latency is compensated, and Gain's polarity and
//! mono switches null a centred source. `fx_check` covers what every built-in effect shares.
//! usage: fx_play_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::fx;
use daw_engine::model::{Id, InputSel, TrackKind};
use daw_engine::session::{BitDepth, RenderOptions};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// The effects this check plays.
const EFFECTS: &[&str] = &["builtin:chorus", "builtin:flanger", "builtin:phaser", "builtin:tremolo", "builtin:overdrive", "builtin:distortion", "builtin:gain"];

/// Two seconds: the test tone lasts three.
const LEN: i64 = 96_000;

fn render(s: &mut Session, from: i64, to: i64) -> Vec<Vec<f32>> {
    s.render(from, to, 0, None, &mut |_| {}).unwrap()
}

fn peak(a: &[Vec<f32>]) -> f32 {
    a.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs())).fold(0.0, f32::max)
}

fn finite(a: &[Vec<f32>]) -> bool {
    a.iter().flatten().all(|s| s.is_finite())
}

/// Reads a float WAV back as channels.
fn read(path: &Path) -> Vec<Vec<f32>> {
    let mut r = hound::WavReader::open(path).unwrap();
    let ch = r.spec().channels as usize;
    let flat: Vec<f32> = r.samples::<f32>().map(|s| s.unwrap()).collect();
    (0..ch).map(|c| flat.iter().skip(c).step_by(ch).copied().collect()).collect()
}

/// Plays from the start for a moment and returns the loudest Stereo Out level seen.
fn play_level(s: &mut Session) -> f32 {
    s.locate(0);
    // Clears the peak held from before.
    std::thread::sleep(Duration::from_millis(60));
    s.tick();
    s.play();
    let mut level = 0.0f32;
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(100));
        let t = s.tick();
        level = level.max(t.master.0.max(t.master.1));
    }
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    level
}

/// A track with the test tone on it.
fn tone_track(s: &mut Session, tone: &str) -> Id {
    let a = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(Path::new(tone), a, 0).unwrap();
    a
}

/// Inserts the effect, plays it live, bounces it, freezes the track and bounces again.
fn play_bounce_freeze(s: &mut Session, id: &str, tone: &str, out: &Path) {
    let name = fx::find(id).map(|i| i.name).unwrap_or("?");
    let a = tone_track(s, tone);
    let dry = render(s, 0, LEN);
    s.set_plugin(a, 0, id).unwrap();
    let level = play_level(s);
    check(&format!("{name}: plays live (Stereo Out at {level:.3})"), level.is_finite() && level > 0.005);
    let live = render(s, 0, LEN);
    let (p, d) = (peak(&live), peak(&dry));
    check(&format!("{name}: the render is sane (peak {p:.3}, dry {d:.3})"), finite(&live) && p > 0.2 * d && p < 8.0 * d);

    let opts = RenderOptions { start: 0, end: LEN, tail_secs: 0.0, bit_depth: BitDepth::Float32, ..Default::default() };
    let before = out.join(format!("{name} live.wav"));
    let after = out.join(format!("{name} frozen.wav"));
    s.bounce_project(&before, &opts, &mut |_| {}).unwrap();
    s.freeze_tracks(&[a], &mut |_| {}).unwrap();
    s.bounce_project(&after, &opts, &mut |_| {}).unwrap();
    let (b, f) = (read(&before), read(&after));
    let off = diff(&b, &f);
    check(&format!("{name}: a bounce of the frozen track matches the live bounce (diff {off:.6})"), b[0].len() == f[0].len() && b[0].len() as i64 >= LEN && off < 1e-4);
    s.unfreeze_track(a).unwrap();
    s.delete_track(a);
}

/// A synced Tremolo follows the song position: a render starting mid-beat carries on where a
/// render from the start is at that point.
fn tremolo_locks_to_the_beat(s: &mut Session, tone: &str) {
    use fx::tremolo::{DEPTH, NOTE, RATE, SYNC};
    let a = tone_track(s, tone);
    s.set_plugin(a, 0, "builtin:tremolo").unwrap();
    s.set_builtin_param(a, 0, NOTE, 6.0).unwrap(); // 1/4
    s.set_builtin_param(a, 0, DEPTH, 1.0).unwrap();
    let beat = (s.sample_rate() * 60.0 / s.project.tempo).round() as i64;
    let from = beat + beat / 2;
    let whole = render(s, 0, LEN);
    let part = render(s, from, LEN);
    let tail: Vec<Vec<f32>> = whole.iter().map(|c| c[from as usize..].to_vec()).collect();
    let off = diff(&part, &tail);
    check(&format!("Tremolo: synced, a render from mid-beat lines up with one from the start (diff {off:.6})"), off < 1e-4);
    // At the start of the render the level is at the bottom of its cycle, half a beat in.
    let first = peak(&part.iter().map(|c| c[..64].to_vec()).collect::<Vec<_>>());
    check(&format!("Tremolo: half a beat in it is at the bottom ({first:.5})"), first < 1e-3);
    // Free-running, the LFO starts afresh with each render, so the two differ: the lock is
    // what lines them up. (At 3 Hz the render starts 2.25 cycles in, not on a whole cycle.)
    s.set_builtin_param(a, 0, SYNC, 0.0).unwrap();
    s.set_builtin_param(a, 0, RATE, 3.0).unwrap();
    let whole = render(s, 0, LEN);
    let part = render(s, from, LEN);
    let tail: Vec<Vec<f32>> = whole.iter().map(|c| c[from as usize..].to_vec()).collect();
    check("Tremolo: free-running, it starts with the render instead", diff(&part, &tail) > 1e-2);
    s.delete_track(a);
}

/// The Distortion's oversampling delays it; delay compensation puts it back in time.
fn distortion_is_compensated(s: &mut Session, tone: &str) {
    use fx::distortion::{DRIVE, MIX};
    let a = tone_track(s, tone);
    let dry = render(s, 0, LEN);
    let uid = s.set_plugin(a, 0, "builtin:distortion").unwrap();
    std::thread::sleep(Duration::from_millis(40));
    s.idle();
    let reported = s.pdc_report().strips.iter().flat_map(|x| x.slots.clone()).find(|x| x.0 == uid).map(|x| x.1).unwrap_or(0);
    let latency = (fx::find("builtin:distortion").unwrap().new)(48_000.0).latency();
    check(&format!("Distortion: reports its {latency}-sample latency ({reported})"), latency > 0 && reported as usize == latency);
    // Mix at 0 is the dry signal, delayed inside by the latency and moved back by compensation.
    s.set_builtin_param(a, 0, MIX, 0.0).unwrap();
    s.set_builtin_param(a, 0, DRIVE, 60.0).unwrap();
    let parallel = render(s, 0, LEN);
    let off = diff(&parallel, &dry);
    check(&format!("Distortion: at 0 % mix the compensated output is the dry signal, in time (diff {off:.7})"), off < 1e-6);
    s.delete_track(a);
}

/// Gain's switches, on a source that is the same on both sides.
fn gain_nulls(s: &mut Session, tone: &str) {
    use fx::gain::{GAIN, INVERT_R, MONO};
    let a = tone_track(s, tone);
    let dry = render(s, 0, LEN);
    s.set_plugin(a, 0, "builtin:gain").unwrap();
    let unity = render(s, 0, LEN);
    check("Gain: at its defaults the signal passes bit for bit", diff(&unity, &dry) == 0.0);
    s.set_builtin_param(a, 0, INVERT_R, 1.0).unwrap();
    s.set_builtin_param(a, 0, MONO, 1.0).unwrap();
    let nulled = peak(&render(s, 0, LEN));
    check(&format!("Gain: inverting one side and summing to mono cancels a centred source ({nulled:.2e})"), nulled < 1e-6);
    s.set_builtin_param(a, 0, INVERT_R, 0.0).unwrap();
    s.set_builtin_param(a, 0, GAIN, -96.0).unwrap();
    let quiet = peak(&render(s, 0, LEN));
    check(&format!("Gain: -96 dB is near silence ({quiet:.2e})"), quiet < peak(&dry) * 2e-5);
    s.delete_track(a);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let config = scratch.join("config");
    let _ = std::fs::remove_dir_all(&config);
    std::env::set_var("XDG_CONFIG_HOME", &config);
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, daw_engine::plugin_db::load_cache().unwrap_or_default());
    s.dir = scratch.join("FxPlayCheck.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();
    let out = scratch.join("bounces");
    std::fs::create_dir_all(&out).unwrap();

    for id in EFFECTS {
        check(&format!("{id} is in the registry"), fx::find(id).is_some());
        play_bounce_freeze(&mut s, id, &tone, &out);
    }
    tremolo_locks_to_the_beat(&mut s, &tone);
    distortion_is_compensated(&mut s, &tone);
    gain_nulls(&mut s, &tone);

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
