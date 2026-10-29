//! Checks plug-in delay compensation with a real latent plug-in (ZaMaximX2, a look-ahead
//! maximizer that reports 480 frames at 48 kHz) against the engine (silent ALSA null output).
//! Timing is measured by cross-correlating offline renders of a noise burst.
//! usage: pdc_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, TrackKind, TrackSend};
use daw_engine::prefs::PdcMode;
use daw_engine::session::TrackPatch;
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) -> bool {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
    ok
}

/// The lag (in frames, -max..=max) at which `b` best matches `a`: positive when `b` is late.
fn lag(a: &[f32], b: &[f32], max: i64) -> i64 {
    let n = a.len().min(b.len()).min(24_000) as i64;
    let mut best = (0i64, f64::MIN);
    for d in -max..=max {
        let mut sum = 0.0f64;
        for i in max..n - max {
            sum += a[i as usize] as f64 * b[(i + d) as usize] as f64;
        }
        if sum > best.1 {
            best = (d, sum);
        }
    }
    best.0
}

fn rms(a: &[f32]) -> f32 {
    (a.iter().map(|s| s * s).sum::<f32>() / a.len().max(1) as f32).sqrt()
}

/// One second of white noise at -12 dBFS after a short silence, as a mono WAV.
fn noise_file(dir: &Path, sr: u32) -> PathBuf {
    let mut seed: u32 = 0x1234_5678;
    let mut x: Vec<f32> = vec![0.0; sr as usize / 10];
    x.extend((0..sr).map(|_| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.25
    }));
    let path = dir.join("noise.wav");
    daw_engine::decode::write_wav(&path, sr, &[x]).unwrap();
    path
}

fn mute(s: &mut Session, id: u64, on: bool) {
    s.update_track(id, TrackPatch { mute: Some(on), ..Default::default() });
}

/// The full mix of [0, len) with only `play` unmuted (out of `all`), as a bounce renders it.
fn mix_of(s: &mut Session, all: &[u64], play: &[u64], len: i64) -> Vec<f32> {
    for &id in all {
        mute(s, id, !play.contains(&id));
    }
    let audio = s.render(0, len, 0, None, &mut |_| {}).unwrap();
    for &id in all {
        mute(s, id, false);
    }
    audio[0].clone()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let _tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let plugins = daw_engine::plugin_db::load_cache().unwrap_or_default();
    let Some(maxim) = plugins.iter().find(|d| d.name == "ZaMaximX2").cloned() else {
        println!("FAIL ZaMaximX2 isn't in the plug-in cache; scan plug-ins first");
        std::process::exit(1);
    };
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, plugins);
    s.dir = scratch.join("PdcCheck.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();
    let sr = s.sample_rate() as u32;
    let noise = noise_file(&s.dir, sr);
    let len = (sr + sr / 10) as i64;
    let mut ok = true;

    // Two tracks with the same audio; B has the maximizer.
    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(&noise, a, 0).unwrap();
    s.import_audio(&noise, b, 0).unwrap();
    let src = daw_engine::decode::decode(&noise).unwrap().channels[0].clone();
    s.set_plugin(b, 0, &maxim.id).unwrap();
    let r = s.pdc_report();
    let lat_b = r.strips.iter().find(|x| x.track == b).map(|x| x.latency).unwrap_or(0);
    println!("report: total {} ({:.2} ms), strips {:?}", r.total, r.total_ms, r.strips);
    ok &= check(&format!("the maximizer reports its latency ({lat_b} frames) and Stereo Out is {} late", r.total), lat_b > 0 && r.total == lat_b);
    let delay_a = r.strips.iter().find(|x| x.track == a).map(|x| x.delay).unwrap_or(0);
    ok &= check(&format!("the plain track is delayed to match ({delay_a} frames)"), delay_a == lat_b);

    let all = [a, b];
    let ma = mix_of(&mut s, &all, &[a], len);
    let mb = mix_of(&mut s, &all, &[b], len);
    let l = lag(&ma, &mb, 1000);
    ok &= check(&format!("both tracks reach Stereo Out together (lag {l})"), l == 0);
    let l = lag(&src, &ma, 1000);
    ok &= check(&format!("the bounce lines up with the timeline (lag {l})"), l == 0);
    let both = mix_of(&mut s, &all, &all, len);
    let err = both.iter().zip(ma.iter().zip(&mb)).map(|(m, (x, y))| (m - x - y).abs()).fold(0.0f32, f32::max);
    ok &= check(&format!("the mix is the sum of the aligned tracks (max error {err:.2e})"), err < 1e-3);

    // Bypassed plug-ins still count; the delay line stands in for them.
    s.set_bypass(b, 0, true);
    let r = s.pdc_report();
    let mb_byp = mix_of(&mut s, &all, &[b], len);
    let l = lag(&ma, &mb_byp, 1000);
    ok &= check(&format!("a bypassed plug-in keeps its compensation (total {}, lag {l})", r.total), r.total == lat_b && l == 0);
    s.set_bypass(b, 0, false);

    // Off: B is late by the plug-in's latency.
    let mut prefs = s.prefs.clone();
    prefs.latency.compensation = PdcMode::Off;
    s.set_prefs(prefs.clone());
    let ma_off = mix_of(&mut s, &all, &[a], len);
    let mb_off = mix_of(&mut s, &all, &[b], len);
    let l = lag(&ma_off, &mb_off, 1000);
    ok &= check(&format!("with compensation off the latent track is late (lag {l}, total {})", s.pdc_total()), l == lat_b as i64 && s.pdc_total() == 0);
    prefs.latency.compensation = PdcMode::All;
    s.set_prefs(prefs.clone());

    // Recordings move earlier by the compensation too.
    let with = s.latency().compensation_ms;
    prefs.latency.compensation = PdcMode::Off;
    s.set_prefs(prefs.clone());
    let without = s.latency().compensation_ms;
    prefs.latency.compensation = PdcMode::All;
    s.set_prefs(prefs.clone());
    let want = lat_b as f32 * 1000.0 / sr as f32;
    ok &= check(&format!("recording compensation includes the plug-in delay ({with:.2} vs {without:.2} ms)"), (with - without - want).abs() < 0.05);

    // A send from a dry track into an aux with the maximizer: the aux path and the dry path
    // line up at Stereo Out (post- and pre-fader).
    let c = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(&noise, c, 0).unwrap();
    let x = s.create_aux(None).unwrap();
    s.set_plugin(x, 0, &maxim.id).unwrap();
    let all = [a, b, c];
    for pre in [false, true] {
        s.set_send(c, 0, Some(TrackSend { bus: x, level_db: 0.0, pre_fader: pre, muted: false })).unwrap();
        // Dry only: the send muted.
        s.set_send(c, 0, Some(TrackSend { bus: x, level_db: 0.0, pre_fader: pre, muted: true })).unwrap();
        let dry = mix_of(&mut s, &all, &[c], len);
        // Wet only: the send on, the track's own output silenced by routing it nowhere
        // audible is not possible, so take the difference from the full mix.
        s.set_send(c, 0, Some(TrackSend { bus: x, level_db: 0.0, pre_fader: pre, muted: false })).unwrap();
        let full = mix_of(&mut s, &all, &[c], len);
        let wet: Vec<f32> = full.iter().zip(&dry).map(|(f, d)| f - d).collect();
        let l = lag(&dry, &wet, 1000);
        let l_src = lag(&src, &dry, 1000);
        let kind = if pre { "pre" } else { "post" };
        ok &= check(&format!("{kind}-fader send through a latent aux lines up with the dry path (lag {l}, wet rms {:.3}, timeline lag {l_src})", rms(&wet)), l == 0 && l_src == 0 && rms(&wet) > 0.01);
    }
    let r = s.pdc_report();
    let send_delay = r.strips.iter().find(|x| x.track == c).map(|x| x.delay).unwrap_or(0);
    ok &= check(&format!("the dry track waits for the aux ({send_delay} frames on its output)"), send_delay == lat_b);

    // Summing stack: a subtrack with latency, its sibling and a track outside.
    s.set_send(c, 0, None).unwrap();
    let sum = s.create_stack(&[a, b], true).unwrap();
    let all = [a, b, c];
    let ma = mix_of(&mut s, &all, &[a], len);
    let mb = mix_of(&mut s, &all, &[b], len);
    let mc = mix_of(&mut s, &all, &[c], len);
    let (l1, l2) = (lag(&ma, &mb, 1000), lag(&mc, &mb, 1000));
    ok &= check(&format!("tracks in a summing stack and outside it stay together (lags {l1}, {l2})"), l1 == 0 && l2 == 0);
    s.set_plugin(sum, 0, &maxim.id).unwrap();
    let mb = mix_of(&mut s, &all, &[b], len);
    let mc = mix_of(&mut s, &all, &[c], len);
    let l = lag(&mc, &mb, 1000);
    let total = s.pdc_total();
    ok &= check(&format!("a latent stack delays the tracks outside it too (lag {l}, total {total})"), l == 0 && total == 2 * lat_b);

    // Undo and redo recompute it.
    s.checkpoint("Remove Plug-in", None, true);
    s.remove_plugin(sum, 0);
    let after_remove = s.pdc_total();
    s.undo();
    let after_undo = s.pdc_total();
    ok &= check(&format!("undo restores the compensation ({after_remove} -> {after_undo})"), after_remove == lat_b && after_undo == 2 * lat_b);

    // Project load computes it from scratch.
    let dir = s.save(Some(scratch.join("PdcCheckSaved.fermata"))).unwrap();
    let mut prefs2 = s.prefs.clone();
    prefs2.latency.compensation = PdcMode::Off;
    s.set_prefs(prefs2);
    s.new_project();
    s.set_prefs(prefs.clone());
    ok &= check("a new project has no compensation", s.pdc_total() == 0);
    s.open(&dir).unwrap();
    let total = s.pdc_total();
    let ma = mix_of(&mut s, &all, &[a], len);
    let mc = mix_of(&mut s, &all, &[c], len);
    let l = lag(&ma, &mc, 1000);
    ok &= check(&format!("a reopened project is compensated (total {total}, lag {l})"), total == 2 * lat_b && l == 0);

    // Low Latency Mode doesn't change playback timing, and renders ignore it.
    prefs.latency.low_latency = true;
    s.set_prefs(prefs.clone());
    let mb = mix_of(&mut s, &all, &[b], len);
    let l = lag(&ma, &mb, 1000);
    ok &= check(&format!("renders in Low Latency Mode stay aligned (lag {l}, low latency {})", s.pdc_report().low_latency), l == 0 && s.pdc_report().low_latency);

    println!("{}", if ok { "ALL PASS" } else { "SOME FAILED" });
    if !ok || FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
