//! Checks the Linear Phase EQ against the engine (silent ALSA null output): its latency is
//! compensated exactly, each Quality reports its latency, and with the same settings its
//! magnitude response matches the Channel EQ's while adding no phase shift. `fx_check` covers
//! what every built-in effect shares (presets, undo, automation, freezing).
//! usage: linphase_check <scratch dir>
use std::path::PathBuf;

use daw_engine::audio_io::AudioConfig;
use daw_engine::fx::linear_phase_eq::{self as lp, freq, gain, on, q, slope, OUTPUT, QUALITY};
use daw_engine::model::{ChannelEq, Id, InputSel, TrackKind};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// The test frequencies: a sine burst of each, one after another.
const FREQS: [f32; 11] = [100.0, 200.0, 400.0, 700.0, 1000.0, 1500.0, 2500.0, 4000.0, 7000.0, 10000.0, 14000.0];
/// Each burst's length, and the stretch of it that is measured (past the filters' settling).
const BURST: usize = 19_200;
const MEASURE: std::ops::Range<usize> = 7_200..16_800;

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs())).fold(0.0, f32::max)
}

fn render(s: &mut Session, len: i64) -> Vec<Vec<f32>> {
    s.render(0, len, 0, None, &mut |_| {}).unwrap()
}

/// The measured stretch of burst `k` (left channel).
fn burst(x: &[Vec<f32>], k: usize) -> &[f32] {
    &x[0][k * BURST + MEASURE.start..k * BURST + MEASURE.end]
}

fn latency_of(s: &Session, uid: Id) -> u32 {
    s.pdc_report().strips.iter().flat_map(|x| x.slots.clone()).find(|x| x.0 == uid).map(|x| x.1).unwrap_or(0)
}

fn settle(s: &mut Session) {
    std::thread::sleep(std::time::Duration::from_millis(40));
    s.idle();
}

/// The Channel EQ settings both EQs get.
fn settings() -> ChannelEq {
    let mut eq = ChannelEq::default();
    let b = &mut eq.bands;
    b[0].on = true;
    b[0].freq = 40.0;
    b[0].slope = 24;
    b[1].freq = 150.0;
    b[1].gain = -4.0;
    b[3].freq = 600.0;
    b[3].gain = -3.0;
    b[3].q = 2.0;
    b[4].freq = 1000.0;
    b[4].gain = 6.0;
    b[5].freq = 3000.0;
    b[5].gain = 2.0;
    b[5].q = 0.8;
    b[6].freq = 6000.0;
    b[6].gain = 3.0;
    eq.output_db = -1.0;
    eq
}

fn main() {
    let mut args = std::env::args().skip(1);
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let config = scratch.join("config");
    let _ = std::fs::remove_dir_all(&config);
    std::env::set_var("XDG_CONFIG_HOME", &config);
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, daw_engine::plugin_db::load_cache().unwrap_or_default());
    s.dir = scratch.join("LinPhaseCheck.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();

    let sr = s.sample_rate() as u32;
    let mut x = Vec::new();
    for f in FREQS {
        x.extend((0..BURST).map(|i| 0.25 * (std::f32::consts::TAU * f * i as f32 / sr as f32).sin()));
    }
    let path = scratch.join("bursts.wav");
    daw_engine::decode::write_wav(&path, sr, &[x]).unwrap();
    let len = (FREQS.len() * BURST) as i64;

    let t = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(&path, t, 0).unwrap();
    let dry = render(&mut s, len);

    // Flat, at each quality: only a delay, which compensation takes out exactly.
    let uid = s.set_plugin(t, 0, "builtin:linear-phase-eq").unwrap();
    for (k, name) in ["Low", "Medium", "High"].iter().enumerate() {
        s.set_builtin_param(t, 0, QUALITY, k as f32).unwrap();
        settle(&mut s);
        let want = (lp::taps(k, sr as f32) - 1) / 2;
        let flat = render(&mut s, len);
        let off = diff(&flat, &dry);
        check(&format!("{name} quality: reports {want} frames of latency (got {}) and, flat, plays the dry signal in time (diff {off:.2e})", latency_of(&s, uid)), latency_of(&s, uid) as usize == want && off < 1e-5);
    }
    s.set_builtin_param(t, 0, QUALITY, 1.0).unwrap();
    s.remove_plugin(t, 0);

    // The Channel EQ with the settings, then the Linear Phase EQ with the same.
    let eq = settings();
    s.set_eq(t, eq.clone());
    let channel = render(&mut s, len);
    s.set_eq(t, ChannelEq::default());
    s.set_plugin(t, 0, "builtin:linear-phase-eq").unwrap();
    for (bi, band) in eq.bands.iter().enumerate() {
        s.set_builtin_param(t, 0, on(bi), if band.on { 1.0 } else { 0.0 }).unwrap();
        s.set_builtin_param(t, 0, freq(bi), band.freq).unwrap();
        s.set_builtin_param(t, 0, q(bi), band.q).unwrap();
        if lp::is_cut(bi) {
            s.set_builtin_param(t, 0, slope(bi), match band.slope { 12 => 0.0, 24 => 1.0, _ => 2.0 }).unwrap();
        } else {
            s.set_builtin_param(t, 0, gain(bi), band.gain).unwrap();
        }
    }
    s.set_builtin_param(t, 0, OUTPUT, eq.output_db).unwrap();
    let linear = render(&mut s, len);

    let mut worst = 0.0f32;
    let mut rows = Vec::new();
    for (k, f) in FREQS.iter().enumerate() {
        let d = rms(burst(&dry, k));
        let (gc, gl) = (20.0 * (rms(burst(&channel, k)) / d).log10(), 20.0 * (rms(burst(&linear, k)) / d).log10());
        rows.push(format!("{f} Hz {gc:+.2}/{gl:+.2}"));
        worst = worst.max((gc - gl).abs());
    }
    check(&format!("the magnitude matches the Channel EQ's within 0.25 dB (worst {worst:.3} dB; Channel/Linear: {})", rows.join(", ")), worst < 0.25);

    // No phase shift: each burst comes out as the dry burst scaled, in time. The Channel EQ
    // shifts the phase around its bands.
    let shape = |out: &[Vec<f32>], k: usize| {
        let (o, d) = (burst(out, k), burst(&dry, k));
        let g = rms(o) / rms(d);
        let res: Vec<f32> = o.iter().zip(d).map(|(o, d)| o - g * d).collect();
        rms(&res) / rms(o)
    };
    let worst_lin = (0..FREQS.len()).map(|k| shape(&linear, k)).fold(0.0f32, f32::max);
    let worst_ch = (0..FREQS.len()).map(|k| shape(&channel, k)).fold(0.0f32, f32::max);
    check(&format!("no phase shift: every burst is the dry one scaled (residual {:.2} %, the Channel EQ's {:.0} %)", worst_lin * 100.0, worst_ch * 100.0), worst_lin < 0.005 && worst_ch > 0.05);

    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
