//! What the sampler must hold: tuning, loops, crossfades, zones, round robin, no clicks, reset and
//! block size, against generated samples.

use std::sync::Arc;

use vst3_host::MidiEvent;

use super::map::{LoopBuf, SamplerMap};
use super::sample::{SampleBuf, SamplePool};
use super::state::{SamplerState, Zone};
use super::*;
use crate::instrument::{FxContext, Instrument};

const CTX: FxContext = FxContext { sample_rate: 48000.0, tempo: 120.0, playing: true, position: 0.0 };

fn temp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("fermata-sampler-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn sine(hz: f64, rate: u32, secs: f64) -> Vec<f32> {
    (0..(secs * rate as f64) as usize).map(|i| ((i as f64 / rate as f64) * hz * std::f64::consts::TAU).sin() as f32 * 0.5).collect()
}

/// A sampler at `rate` over these zones, with each file written to a temporary folder. Files are
/// float WAVs.
fn sampler(tag: &str, rate: f32, files: &[(&str, u32, Vec<Vec<f32>>)], zones: Vec<Zone>) -> Sampler {
    let dir = temp(tag);
    for (name, r, ch) in files {
        crate::decode::write_wav(&dir.join(name), *r, ch).unwrap();
    }
    let state = SamplerState { version: 1, next_id: 100, zones };
    let (map, issues) = SamplerMap::build(&state, &dir, rate, &SamplePool::default());
    assert!(issues.is_empty(), "{issues:?}");
    let mut s = Sampler::new(rate, 1);
    drop(s.swap_data(Arc::new(map)));
    s.reset();
    let _ = std::fs::remove_dir_all(&dir);
    s
}

fn zone(id: u64, file: &str, root: u8) -> Zone {
    Zone::new(id, file.into(), file.into(), root)
}



/// Plays `n` samples with events at absolute positions, in blocks of `block`.
fn play(s: &mut Sampler, n: usize, block: usize, events: &[(usize, [u8; 3])], ctx: &FxContext) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    let mut at = 0;
    while at < n {
        let m = block.min(n - at);
        let midi: Vec<MidiEvent> = events.iter().filter(|e| e.0 >= at && e.0 < at + m).map(|e| MidiEvent { offset: (e.0 - at) as u32, data: e.1 }).collect();
        let (a, b) = (&mut l[at..at + m], &mut r[at..at + m]);
        s.process(a, b, &midi, ctx);
        at += m;
    }
    (l, r)
}

/// Frequency by counting zero crossings with linear interpolation, over the middle of a block.
fn freq(x: &[f32], rate: f32) -> f64 {
    let (mut first, mut last, mut count) = (None, 0.0f64, 0);
    for i in 1..x.len() {
        if x[i - 1] < 0.0 && x[i] >= 0.0 {
            let at = (i - 1) as f64 + (-x[i - 1] as f64) / (x[i] as f64 - x[i - 1] as f64);
            if first.is_none() {
                first = Some(at);
            } else {
                count += 1;
            }
            last = at;
        }
    }
    count as f64 / ((last - first.unwrap_or(0.0)) / rate as f64)
}

fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

/// The largest step between neighbouring samples.
fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0, |m, w| m.max((w[1] - w[0]).abs()))
}

/// A Kaiser window (β = 16: side lobes near -120 dB, main lobe about 5 bins each side), so the
/// measurement doesn't hide what it looks for behind its own leakage.
fn window(n: usize) -> Vec<f64> {
    let i0 = |x: f64| {
        let (mut sum, mut term) = (1.0, 1.0);
        for k in 1..60 {
            term *= (x / 2.0 / k as f64) * (x / 2.0 / k as f64);
            sum += term;
        }
        sum
    };
    (0..n)
        .map(|i| {
            let t = 2.0 * i as f64 / (n - 1) as f64 - 1.0;
            i0(16.0 * (1.0 - t * t).sqrt()) / i0(16.0)
        })
        .collect()
}

/// The level of `f` in a signal, by correlating with the window.
fn tone_level(x: &[f32], w: &[f64], f: f64, rate: f64) -> f64 {
    let (mut re, mut im, mut sum) = (0.0f64, 0.0f64, 0.0f64);
    let step = std::f64::consts::TAU * f / rate;
    for (i, (&v, g)) in x.iter().zip(w).enumerate() {
        let p = step * i as f64;
        re += v as f64 * g * p.cos();
        im += v as f64 * g * p.sin();
        sum += g;
    }
    2.0 * (re * re + im * im).sqrt() / sum
}

/// The loudest component in `lo..hi` Hz further than the window's main lobe from every one of `skip`.
fn loudest_elsewhere(x: &[f32], w: &[f64], rate: f64, lo: f64, hi: f64, skip: &[f64]) -> f64 {
    let bin = rate / x.len() as f64;
    let mut m = 0.0f64;
    let mut f = lo;
    while f < hi {
        if skip.iter().all(|s| (f - s).abs() > bin * 8.0) {
            m = m.max(tone_level(x, w, f, rate));
        }
        f += bin * 2.0;
    }
    m
}

#[test]
fn plays_in_tune_across_the_range_at_every_rate() {
    // A sample at 440 Hz recorded at various rates, played on a project at each rate.
    let mut worst = 0.0f64;
    for out in [44100.0f32, 48000.0, 96000.0] {
        for file_rate in [44100u32, 48000, 96000] {
            let w = sine(440.0, file_rate, 6.0);
            let mut s = sampler(&format!("tune{out}{file_rate}"), out, &[("a.wav", file_rate, vec![w])], vec![zone(1, "a.wav", 69)]);
            let ctx = FxContext { sample_rate: out, ..CTX };
            for note in [33u8, 45, 57, 69, 72, 81, 93] {
                let n = (out * 0.4) as usize;
                let (l, _) = play(&mut s, n, 512, &[(0, [0x90, note, 127])], &ctx);
                let _ = play(&mut s, 64, 64, &[(0, [0x80, note, 0])], &ctx);
                s.reset();
                let want = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
                let got = freq(&l[n / 4..], out);
                let e = cents(got, want).abs();
                worst = worst.max(e);
                assert!(e < 0.05, "{out} from {file_rate}: note {note} is {e} cents off ({got} Hz)");
            }
        }
    }
    println!("worst tuning error {worst:.5} cents");
}

#[test]
fn depths_play_in_tune() {
    // 16, 24 and 32-bit integers and 32-bit float, as decoded from real WAV files.
    let dir = temp("depths");
    let wave = sine(440.0, 44100, 3.0);
    for (bits, float) in [(16, false), (24, false), (32, false), (32, true)] {
        let path = dir.join(format!("{bits}{float}.wav"));
        let spec = hound::WavSpec { channels: 1, sample_rate: 44100, bits_per_sample: bits, sample_format: if float { hound::SampleFormat::Float } else { hound::SampleFormat::Int } };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for &v in &wave {
            if float {
                w.write_sample(v).unwrap();
            } else {
                w.write_sample((v as f64 * ((1i64 << (bits - 1)) - 1) as f64).round() as i32).unwrap();
            }
        }
        w.finalize().unwrap();
        let state = SamplerState { version: 1, next_id: 2, zones: vec![zone(1, path.file_name().unwrap().to_str().unwrap(), 69)] };
        let (map, issues) = SamplerMap::build(&state, &dir, 48000.0, &SamplePool::default());
        assert!(issues.is_empty());
        let mut s = Sampler::new(48000.0, 1);
        drop(s.swap_data(Arc::new(map)));
        s.reset();
        let (l, _) = play(&mut s, 48000, 512, &[(0, [0x90, 69, 127])], &CTX);
        let e = cents(freq(&l[12000..], 48000.0), 440.0).abs();
        assert!(e < 0.02, "{bits} bits, float {float}: {e} cents");
        assert!((peak(&l[12000..]) - 0.5).abs() < 0.01);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn at_its_root_a_sample_comes_out_unchanged() {
    let w = sine(997.0, 48000, 0.5);
    let mut s = sampler("root", 48000.0, &[("a.wav", 48000, vec![w.clone()])], vec![zone(1, "a.wav", 60)]);
    let (l, r) = play(&mut s, 12000, 512, &[(0, [0x90, 60, 127])], &CTX);
    // The attack is instant, and the sine starts at zero so nothing ramps it.
    for i in 0..12000 {
        assert_eq!(l[i], w[i], "{i}");
        assert_eq!(r[i], w[i], "{i}");
    }
}

#[test]
fn aliasing_playing_up_is_measured() {
    // A sine played up so that it stays in band, or goes above Nyquist where it mustn't fold back:
    // everything below 19 kHz that isn't the tone must be far below it.
    let n = 8192;
    let w = window(n);
    let mut worst = -200.0f64;
    for (file_hz, note_up) in [(5000.0, 12u8), (5000.0, 7), (4100.0, 11), (9000.0, 12), (3000.0, 24), (1300.0, 35), (1500.0, 36), (900.0, 47), (900.0, 48), (15000.0, 2), (18000.0, 7), (8000.0, 19), (12000.0, 12), (6000.0, 31)] {
        let wave = sine(file_hz, 48000, 5.0);
        let mut s = sampler(&format!("alias{file_hz}{note_up}"), 48000.0, &[("a.wav", 48000, vec![wave])], vec![zone(1, "a.wav", 36)]);
        let (l, _) = play(&mut s, n + 2048, 512, &[(0, [0x90, 36 + note_up, 127])], &CTX);
        let want = file_hz * 2f64.powf(note_up as f64 / 12.0);
        let x = &l[2048..];
        let others = loudest_elsewhere(x, &w, 48000.0, 20.0, 19000.0, &[want]);
        // Against what the sample holds: -6 dB.
        let db = 20.0 * (others / 0.5).log10();
        println!("{file_hz} Hz up {note_up} st ({want:.0} Hz): the rest {db:.1} dB");
        assert!(db < -100.0, "{file_hz} up {note_up}: {db}");
        worst = worst.max(db);
    }
    println!("worst sine case {worst:.1} dB");
}

#[test]
fn aliasing_of_a_bright_sound_played_up_is_measured() {
    // A band-limited saw full of harmonics near the source's Nyquist, at fundamental 750 Hz.
    let wave: Vec<f32> = (0..240000)
        .map(|i| {
            let t = i as f64 / 48000.0;
            let mut v = 0.0;
            for h in 1..=31 {
                v += (std::f64::consts::TAU * 750.0 * h as f64 * t).sin() / h as f64;
            }
            (v * 0.4) as f32
        })
        .collect();
    let n = 8192;
    let w = window(n);
    for note_up in [1u8, 7, 12, 19, 24, 31, 36] {
        let mut s = sampler(&format!("saw{note_up}"), 48000.0, &[("a.wav", 48000, vec![wave.clone()])], vec![zone(1, "a.wav", 36)]);
        let (l, _) = play(&mut s, n + 2048, 512, &[(0, [0x90, 36 + note_up, 127])], &CTX);
        let f0 = 750.0 * 2f64.powf(note_up as f64 / 12.0);
        let harmonics: Vec<f64> = (1..64).map(|h| h as f64 * f0).collect();
        let x = &l[2048..];
        let fundamental = tone_level(x, &w, f0, 48000.0);
        let others = loudest_elsewhere(x, &w, 48000.0, 20.0, 18000.0, &harmonics);
        let db = 20.0 * (others / fundamental).log10();
        println!("saw up {note_up} st: aliases {db:.1} dB against its fundamental, below 18 kHz");
        assert!(db < -75.0, "up {note_up}: {db}");
    }
}

#[test]
fn far_up_the_longest_kernels_keep_aliases_out() {
    // A low, harmonically rich sample four, five and six octaves up. The kernels stop growing at four.
    let f = 93.75 / 2.0;
    let wave: Vec<f32> = (0..48000 * 16)
        .map(|i| {
            let t = i as f64 / 48000.0;
            let mut v = 0.0;
            for h in 1..=500 {
                v += (std::f64::consts::TAU * f * h as f64 * t).sin() / h as f64;
            }
            (v * 0.4) as f32
        })
        .collect();
    let n = 8192;
    let w = window(n);
    let mut level = Vec::new();
    for up in [47u8, 59, 71] {
        let root = 24 - up.saturating_sub(59) / 2;
        let mut s = sampler(&format!("far{up}"), 48000.0, &[("a.wav", 48000, vec![wave.clone()])], vec![zone(1, "a.wav", root)]);
        let (l, _) = play(&mut s, n + 2048, 512, &[(0, [0x90, root + up, 127])], &CTX);
        let f0 = f * 2f64.powf(up as f64 / 12.0);
        let harmonics: Vec<f64> = (1..64).map(|h| h as f64 * f0).collect();
        let x = &l[2048..];
        let db = 20.0 * (loudest_elsewhere(x, &w, 48000.0, 20.0, 18000.0, &harmonics) / tone_level(x, &w, f0, 48000.0)).log10();
        println!("low saw up {up} st: aliases {db:.1} dB against its fundamental");
        level.push(db);
    }
    assert!(level[0] < -90.0, "{level:?}");
    assert!(level[1] < -70.0, "{level:?}");
}

#[test]
fn a_loop_goes_round_at_the_right_pitch_without_a_click() {
    // A sine with a whole number of cycles in the loop: no crossfade needed, and no seam.
    let period = 100usize;
    let w: Vec<f32> = (0..48000).map(|i| ((i % period) as f64 / period as f64 * std::f64::consts::TAU).sin() as f32 * 0.5).collect();
    let mut z = zone(1, "a.wav", 69);
    z.loop_on = true;
    z.loop_start = 1000;
    z.loop_end = 1000 + period as u64 * 7;
    let mut s = sampler("loop", 48000.0, &[("a.wav", 48000, vec![w])], vec![z]);
    let n = 48000 * 2;
    let (l, _) = play(&mut s, n, 512, &[(0, [0x90, 69, 127])], &CTX);
    // It plays past the file's length by looping.
    assert!(peak(&l[n - 4800..]) > 0.49, "it stopped");
    assert!((freq(&l[n / 2..], 48000.0) - 480.0).abs() < 0.01);
    let steady = max_step(&l[100..1000]);
    assert!(max_step(&l[1000..]) <= steady * 1.01, "{} against {steady}", max_step(&l[1000..]));
    // And the same a fifth up, through the fractional kernels.
    s.reset();
    let (l, _) = play(&mut s, n, 512, &[(0, [0x90, 76, 127])], &CTX);
    assert!((cents(freq(&l[n / 2..], 48000.0), 480.0 * 2f64.powf(7.0 / 12.0))).abs() < 0.02);
    let steady = max_step(&l[100..1000]);
    assert!(max_step(&l[1000..]) <= steady * 1.02);
}

#[test]
fn a_single_cycle_loops_even_far_up() {
    let period = 64usize;
    let w: Vec<f32> = (0..period).map(|i| (i as f64 / period as f64 * std::f64::consts::TAU).sin() as f32 * 0.5).collect();
    let mut z = zone(1, "a.wav", 60);
    z.loop_on = true;
    let mut s = sampler("cycle", 48000.0, &[("a.wav", 48000, vec![w])], vec![z]);
    for (note, up) in [(60u8, 0.0f64), (72, 12.0), (96, 36.0)] {
        s.reset();
        let (l, _) = play(&mut s, 24000, 128, &[(0, [0x90, note, 127])], &CTX);
        let want = 750.0 * 2f64.powf(up / 12.0);
        let got = freq(&l[6000..], 48000.0);
        assert!(cents(got, want).abs() < 0.5, "note {note}: {got} against {want}");
        assert!(l.iter().all(|v| v.is_finite()));
        assert!(peak(&l[6000..]) > 0.45);
    }
}

#[test]
fn a_crossfade_closes_a_loop_whose_ends_dont_meet() {
    // A loop whose ends can't meet: a rising ramp, from a sound that keeps rising.
    let w: Vec<f32> = (0..8000).map(|i| i as f32 / 8000.0 * 0.8).collect();
    let buf = SampleBuf::new(48000, std::slice::from_ref(&w));
    let (start, end) = (4000, 6000);
    for xfade in [0usize, 500] {
        let b = LoopBuf::bake(&buf, start, end, xfade, false);
        let body: Vec<f32> = b.ch[0][super::kernel::GUARD..super::kernel::GUARD + b.len].to_vec();
        let wrap = (body[0] - body[b.len - 1]).abs();
        if xfade == 0 {
            assert!(wrap > 0.19, "{wrap}");
            continue;
        }
        // Its last frame leads into its first as the frame before the start does.
        assert!(wrap < 0.001, "wrap {wrap}");
        let step = body.windows(2).fold(0.0f32, |m, p| m.max((p[1] - p[0]).abs()));
        // The crossfade runs from the ramp's height down to where the loop starts over 500 frames:
        // nothing sudden.
        assert!(step < 0.001, "step {step}");
        // Untouched before the fade, and a mix inside it.
        assert_eq!(body[0], w[start]);
        assert_eq!(body[1000], w[start + 1000]);
        let mid = b.len - 250;
        let t = 251.0 / 501.0f32;
        assert!((body[mid] - (w[start + mid] * (1.0 - t) + w[start + mid - b.len] * t)).abs() < 1e-5);
    }
    // Equal power keeps uncorrelated audio's level through the fade; equal gain dips.
    let noise: Vec<f32> = {
        let mut r = crate::synth_dsp::analog::Rng::new(9);
        (0..48000).map(|_| r.bipolar() * 0.5).collect()
    };
    let buf = SampleBuf::new(48000, &[noise]);
    let rms = |x: &[f32]| (x.iter().map(|v| (v * v) as f64).sum::<f64>() / x.len() as f64).sqrt();
    let gain = LoopBuf::bake(&buf, 20000, 30000, 8000, false);
    let power = LoopBuf::bake(&buf, 20000, 30000, 8000, true);
    let mid = super::kernel::GUARD + 10000 - 4400;
    let g = rms(&gain.ch[0][mid..mid + 800]);
    let p = rms(&power.ch[0][mid..mid + 800]);
    assert!(p > g * 1.3, "equal power {p}, equal gain {g}");
    assert!((p / rms(&buf.frames_of(0)[..8000]) - 1.0).abs() < 0.1, "{p}");
    // A loop at the very start has nothing to fade in from: the crossfade shrinks to fit.
    assert_eq!(LoopBuf::bake(&buf, 100, 1000, 500, false).xfade, 100);
}

#[test]
fn zones_answer_their_keys_and_velocities() {
    let a = vec![vec![0.5f32; 4800]];
    let b = vec![vec![0.25f32; 4800]];
    let c = vec![vec![0.125f32; 4800]];
    let mut za = zone(1, "a.wav", 60);
    za.key_lo = 60;
    za.key_hi = 62;
    za.vel_hi = 63;
    let mut zb = zone(2, "b.wav", 60);
    zb.key_lo = 60;
    zb.key_hi = 62;
    zb.vel_lo = 64;
    let mut zc = zone(3, "c.wav", 70);
    zc.key_lo = 70;
    zc.key_hi = 70;
    let mut s = sampler("zones", 48000.0, &[("a.wav", 48000, a), ("b.wav", 48000, b), ("c.wav", 48000, c)], vec![za, zb, zc]);
    s.set_param(params::param::VEL_AMP, 0.0);
    let mut level = |note: u8, vel: u8| {
        s.reset();
        let (l, _) = play(&mut s, 1000, 128, &[(0, [0x90, note, vel])], &CTX);
        l[900]
    };
    assert!((level(60, 40) - 0.5).abs() < 1e-3);
    assert!((level(62, 63) - 0.5).abs() < 1e-3);
    assert!((level(60, 64) - 0.25).abs() < 1e-3);
    assert!((level(61, 127) - 0.25).abs() < 1e-3);
    assert!((level(70, 100) - 0.125).abs() < 1e-3);
    assert_eq!(level(63, 100), 0.0);
    assert_eq!(level(10, 100), 0.0);
}

#[test]
fn overlapping_zones_layer() {
    let mut z2 = zone(2, "b.wav", 60);
    z2.pan = 1.0;
    let mut z1 = zone(1, "a.wav", 60);
    z1.pan = -1.0;
    let mut s = sampler("layers", 48000.0, &[("a.wav", 48000, vec![vec![0.5; 4800]]), ("b.wav", 48000, vec![vec![0.25; 4800]])], vec![z1, z2]);
    let (l, r) = play(&mut s, 1000, 128, &[(0, [0x90, 60, 127])], &CTX);
    assert!((l[900] - 0.5).abs() < 1e-3 && (r[900] - 0.25).abs() < 1e-3, "{} {}", l[900], r[900]);
}

#[test]
fn round_robin_takes_turns_and_starts_over_on_reset() {
    let files = [("a.wav", 48000, vec![vec![0.5f32; 2400]]), ("b.wav", 48000, vec![vec![0.25f32; 2400]]), ("c.wav", 48000, vec![vec![0.125f32; 2400]])];
    let mut zs = vec![zone(1, "a.wav", 60), zone(2, "b.wav", 60), zone(3, "c.wav", 60)];
    zs.iter_mut().for_each(|z| z.rr_group = 1);
    let mut s = sampler("rr", 48000.0, &files, zs);
    let heard = |s: &mut Sampler| {
        let (l, _) = play(s, 800, 128, &[(0, [0x90, 60, 127])], &CTX);
        let _ = play(s, 8000, 512, &[(0, [0x80, 60, 0])], &CTX);
        l[700]
    };
    let turn = |want: f32, got: f32| assert!((want - got).abs() < 1e-3, "{want} {got}");
    for want in [0.5, 0.25, 0.125, 0.5, 0.25] {
        turn(want, heard(&mut s));
    }
    s.reset();
    turn(0.5, heard(&mut s));
}

#[test]
fn nothing_goes_nan_whatever_the_file_holds() {
    let mut w = sine(440.0, 48000, 0.2);
    w[100] = f32::NAN;
    w[200] = f32::INFINITY;
    w[300] = 1e30;
    let dir = temp("nan");
    crate::decode::write_wav(&dir.join("a.wav"), 48000, &[w]).unwrap();
    let state = SamplerState { version: 1, next_id: 2, zones: vec![zone(1, "a.wav", 60)] };
    let (map, _) = SamplerMap::build(&state, &dir, 48000.0, &SamplePool::default());
    let mut s = Sampler::new(48000.0, 1);
    drop(s.swap_data(Arc::new(map)));
    s.reset();
    for f in [0.0, 1.0, 2.0, 3.0, 4.0] {
        s.set_param(params::param::F_TYPE, f);
        s.set_param(params::param::RESONANCE, 100.0);
        s.set_param(params::param::CUTOFF, 200.0);
        s.process(&mut [0.0; 8], &mut [0.0; 8], &[], &CTX);
        s.reset();
        let (l, r) = play(&mut s, 4800, 128, &[(0, [0x90, 48, 127]), (10, [0xE0, 0, 127]), (400, [0x90, 84, 127])], &CTX);
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 1e4), "filter {f}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stealing_and_note_off_dont_click() {
    // A full-level sine, hard-panned nowhere, in a loop so voices sound for as long as we like.
    let period = 96usize;
    let w: Vec<f32> = (0..48000).map(|i| ((i % period) as f64 / period as f64 * std::f64::consts::TAU).sin() as f32 * 0.9).collect();
    let mut z = zone(1, "a.wav", 60);
    z.loop_on = true;
    z.loop_start = 960;
    z.loop_end = 960 + 480 * period as u64;
    z.xfade = 96;
    let mut s = sampler("steal", 48000.0, &[("a.wav", 48000, vec![w])], vec![z]);
    s.set_param(params::param::POLYPHONY, 0.0);
    // Note on, then at an arbitrary phase another note steals its only voice.
    let (l, _) = play(&mut s, 12000, 128, &[(0, [0x90, 60, 127]), (6007, [0x90, 60, 127])], &CTX);
    let steady = max_step(&l[1000..5900]);
    let at_steal = max_step(&l[5990..6400]);
    println!("steal: step {at_steal:.5} against steady {steady:.5}, {:.2} times", at_steal / steady);
    assert!(at_steal < steady * 2.5, "{at_steal} against {steady}");
    // While it fades there are two voices, so the voice count says so.
    s.reset();
    let _ = play(&mut s, 2000, 128, &[(0, [0x90, 60, 127]), (1990, [0x90, 64, 127])], &CTX);
    let mut m = [0.0; MAX_METERS];
    s.meters(&mut m);
    assert_eq!(m[0], 2.0, "the stolen one fades on its own voice");
    // Note-off at an arbitrary phase: the release is shorter than a period of it would click at
    // only if it were abrupt.
    s.reset();
    s.set_param(params::param::POLYPHONY, 8.0);
    s.set_param(params::param::A_RELEASE, 1.0);
    let (l, _) = play(&mut s, 12000, 128, &[(0, [0x90, 60, 127]), (6011, [0x80, 60, 0])], &CTX);
    let steady = max_step(&l[1000..6000]);
    let at_off = max_step(&l[6000..7000]);
    println!("note off at the shortest release: step {at_off:.5} against steady {steady:.5}");
    assert!(at_off < steady * 1.5, "{at_off} against {steady}");
    assert!(peak(&l[8000..]) < 1e-3, "it should have ended");
    let mut m = [0.0; MAX_METERS];
    s.meters(&mut m);
    assert_eq!(m[0], 0.0, "and freed its voice");
    // A sample cut mid-wave at its end fades rather than stops.
    let w = vec![0.8f32; 4800];
    let mut s = sampler("cut", 48000.0, &[("a.wav", 48000, vec![w])], vec![zone(1, "a.wav", 60)]);
    let (l, _) = play(&mut s, 6000, 128, &[(0, [0x90, 60, 127])], &CTX);
    assert_eq!(l[5000], 0.0);
    assert!(max_step(&l[1000..]) < 0.8 / 40.0, "{}", max_step(&l[1000..]));
    // And a start cut mid-wave fades in.
    assert!(max_step(&l[..100]) < 0.8 / 20.0, "{}", max_step(&l[..100]));
}

#[test]
fn reset_repeats_bit_for_bit_and_block_size_doesnt_matter() {
    let w = sine(220.0, 44100, 2.0);
    let mut z = zone(1, "a.wav", 57);
    z.loop_on = true;
    z.loop_start = 4410;
    z.loop_end = 60000;
    z.xfade = 2000;
    let mut other = zone(2, "b.wav", 60);
    other.rr_group = 3;
    let mut third = zone(3, "a.wav", 60);
    third.rr_group = 3;
    let mut s = sampler("repeat", 48000.0, &[("a.wav", 44100, vec![w.clone()]), ("b.wav", 44100, vec![w.iter().map(|v| v * 0.5).collect(), w.clone()])], vec![z, other, third]);
    use params::param::*;
    s.set_param(F_TYPE, 4.0);
    s.set_param(CUTOFF, 1500.0);
    s.set_param(RESONANCE, 40.0);
    s.set_param(F_ENV, 24.0);
    s.set_param(LFO_PITCH, 30.0);
    s.set_param(LFO_FILTER, 12.0);
    s.set_param(LFO_WAVE, 4.0);
    s.set_param(GLIDE, 50.0);
    let events: Vec<(usize, [u8; 3])> = vec![
        (0, [0x90, 60, 100]),
        (1000, [0x90, 64, 80]),
        (3333, [0x80, 60, 0]),
        (5000, [0xE0, 0, 80]),
        (7000, [0xB0, 1, 90]),
        (9001, [0x90, 60, 127]),
        (15000, [0x80, 64, 0]),
        (20000, [0x80, 60, 0]),
    ];
    let n = 48000;
    s.reset();
    let a = play(&mut s, n, 512, &events, &CTX);
    s.reset();
    let b = play(&mut s, n, 512, &events, &CTX);
    assert!(a.0.iter().zip(&b.0).all(|(x, y)| x.to_bits() == y.to_bits()), "reset doesn't repeat");
    for block in [1, 37, 128, 1024] {
        s.reset();
        let c = play(&mut s, n, block, &events, &CTX);
        let at = a.0.iter().zip(&c.0).position(|(x, y)| x.to_bits() != y.to_bits());
        assert_eq!(at, None, "block {block} differs from 512");
    }
    assert!(peak(&a.0) > 0.01);
}

#[test]
fn replacing_the_map_leaves_ringing_notes_alone() {
    let dir = temp("swap");
    let pool = SamplePool::default();
    crate::decode::write_wav(&dir.join("a.wav"), 48000, &[vec![0.5; 96000]]).unwrap();
    crate::decode::write_wav(&dir.join("b.wav"), 48000, &[vec![0.25; 96000]]).unwrap();
    let build = |file: &str| Arc::new(SamplerMap::build(&SamplerState { version: 1, next_id: 2, zones: vec![zone(1, file, 60)] }, &dir, 48000.0, &pool).0);
    let mut s = Sampler::new(48000.0, 1);
    let first = build("a.wav");
    assert!(s.swap_data(first.clone()).is_some());
    s.reset();
    let _ = play(&mut s, 1000, 128, &[(0, [0x90, 60, 127])], &CTX);
    // The instance, the voice and this test hold it.
    assert_eq!(Arc::strong_count(&first), 3);
    let old = s.swap_data(build("b.wav")).unwrap();
    assert_eq!(Arc::as_ptr(&old) as *const u8, Arc::as_ptr(&first) as *const u8);
    // The held note plays on from the old one; a new note gets the new one.
    let (l, _) = play(&mut s, 500, 128, &[], &CTX);
    assert!((l[400] - 0.5).abs() < 1e-3);
    let (l, _) = play(&mut s, 500, 128, &[(0, [0x90, 64, 127])], &CTX);
    assert!((l[400] - (0.5 + 0.25)).abs() < 1e-3, "{}", l[400]);
    drop(old);
    // Let go of the note: the voice's hold goes, without freeing anything here since `first` holds.
    let _ = play(&mut s, 48000, 512, &[(0, [0x80, 60, 0]), (0, [0x80, 64, 0])], &CTX);
    assert_eq!(Arc::strong_count(&first), 1, "the voice let go");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn vibrato_across_a_kernel_change_doesnt_step() {
    // A bright tone with vibrato going back and forth over a bank's edge, 3 semitones up.
    let wave = sine(5000.0, 48000, 4.0);
    let mut s = sampler("bank", 48000.0, &[("a.wav", 48000, vec![wave])], vec![zone(1, "a.wav", 60)]);
    let (steady, _) = play(&mut s, 24000, 512, &[(0, [0x90, 63, 127])], &CTX);
    s.set_param(params::param::LFO_PITCH, 30.0);
    s.set_param(params::param::LFO_RATE, 8.0);
    s.reset();
    let (moving, _) = play(&mut s, 48000, 512, &[(0, [0x90, 63, 127])], &CTX);
    // Its slope grows with its pitch and no more: a step would show on top of that.
    let (a, b) = (max_step(&steady[2000..]), max_step(&moving[2000..]));
    println!("step across kernels {b:.5} against steady {a:.5}, {:.3} times", b / a);
    assert!(b < a * 1.05, "{b} against {a}");
}

#[test]
fn glide_slides_between_notes() {
    let w = sine(440.0, 48000, 2.0);
    let mut s = sampler("glide", 48000.0, &[("a.wav", 48000, vec![w])], vec![zone(1, "a.wav", 69)]);
    s.set_param(params::param::MODE, 2.0);
    s.set_param(params::param::GLIDE, 100.0);
    let (l, _) = play(&mut s, 48000, 512, &[(0, [0x90, 69, 127]), (24000, [0x90, 81, 127])], &CTX);
    assert!(cents(freq(&l[12000..23000], 48000.0), 440.0).abs() < 0.1);
    // Partway through the slide, between the notes.
    let mid = freq(&l[24000..24000 + 1500], 48000.0);
    assert!(mid > 480.0 && mid < 800.0, "{mid}");
    assert!(cents(freq(&l[40000..], 48000.0), 880.0).abs() < 0.5);
}

#[test]
fn one_shots_ignore_the_key_but_not_a_stop() {
    let mut z = zone(1, "a.wav", 60);
    z.one_shot = true;
    let mut s = sampler("oneshot", 48000.0, &[("a.wav", 48000, vec![vec![0.5; 24000]])], vec![z]);
    let (l, _) = play(&mut s, 12000, 128, &[(0, [0x90, 60, 127]), (10, [0x80, 60, 0])], &CTX);
    assert!(peak(&l[11000..]) > 0.49, "it stopped with the key");
    let tail = s.tail_secs();
    assert!(tail > 0.5 * 0.99, "{tail}");
    s.all_notes_off();
    let (l, _) = play(&mut s, 48000, 512, &[], &CTX);
    assert_eq!(l[47000], 0.0, "a stop ends it");
}

#[test]
fn a_map_that_cant_load_part_of_itself_says_which() {
    let dir = temp("missing");
    crate::decode::write_wav(&dir.join("a.wav"), 48000, &[vec![0.5; 4800]]).unwrap();
    std::fs::write(dir.join("junk.wav"), b"not audio at all").unwrap();
    let state = SamplerState { version: 1, next_id: 4, zones: vec![zone(1, "a.wav", 60), zone(2, "gone.wav", 60), zone(3, "junk.wav", 60)] };
    let (map, issues) = SamplerMap::build(&state, &dir, 48000.0, &SamplePool::default());
    assert_eq!(map.zones.len(), 1);
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].error, super::sample::LoadError::Missing);
    assert!(matches!(issues[1].error, super::sample::LoadError::Unreadable(_)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_memory_limit_refuses_what_wont_fit() {
    let dir = temp("limit");
    crate::decode::write_wav(&dir.join("a.wav"), 48000, &[vec![0.5; 4800]]).unwrap();
    crate::decode::write_wav(&dir.join("b.wav"), 48000, &[vec![0.5; 4800]]).unwrap();
    let pool = SamplePool::with_limit(30000);
    let a = pool.get(&dir.join("a.wav")).unwrap();
    assert_eq!(pool.get(&dir.join("b.wav")).err(), Some(super::sample::LoadError::OverMemoryLimit));
    // Once nothing plays the first, it makes way.
    drop(a);
    assert!(pool.get(&dir.join("b.wav")).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reverse_plays_backwards() {
    let w: Vec<f32> = (0..4800).map(|i| i as f32 / 4800.0).collect();
    let mut z = zone(1, "a.wav", 60);
    z.reverse = true;
    z.start = 1000;
    z.end = 3000;
    let mut s = sampler("reverse", 48000.0, &[("a.wav", 48000, vec![w])], vec![z]);
    let (l, _) = play(&mut s, 2100, 128, &[(0, [0x90, 60, 127])], &CTX);
    // From the zone's end down to its start.
    assert!((l[100] - (2899.0 / 4800.0)).abs() < 1e-3, "{}", l[100]);
    assert!((l[1500] - (1499.0 / 4800.0)).abs() < 1e-3, "{}", l[1500]);
    assert_eq!(l[2050], 0.0);
}

#[test]
fn smpl_chunks_are_read() {
    let dir = temp("smpl");
    let path = dir.join("a.wav");
    crate::decode::write_wav(&path, 48000, &[vec![0.1; 100]]).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let mut body = vec![0u8; 36 + 24];
    body[12..16].copy_from_slice(&64u32.to_le_bytes());
    body[28..32].copy_from_slice(&1u32.to_le_bytes());
    body[36 + 8..36 + 12].copy_from_slice(&10u32.to_le_bytes());
    body[36 + 12..36 + 16].copy_from_slice(&89u32.to_le_bytes());
    bytes.extend_from_slice(b"smpl");
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&body);
    let riff = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    let i = super::sample::wav_info(&path);
    assert_eq!((i.root, i.loop_points), (Some(64), Some((10, 90))));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn state_round_trips_and_tidies() {
    let mut z = zone(5, "a.wav", 200);
    z.key_lo = 100;
    z.key_hi = 50;
    z.pan = 9.0;
    let s = SamplerState { version: 1, next_id: 0, zones: vec![z] };
    let back = SamplerState::parse(Some(&s.to_json()));
    assert_eq!(back.next_id, 6);
    let z = &back.zones[0];
    assert_eq!((z.root, z.key_lo, z.key_hi, z.pan), (127, 50, 100, 1.0));
    assert!(SamplerState::parse(Some("not json")).zones.is_empty());
    assert!(SamplerState::parse(None).zones.is_empty());
}
