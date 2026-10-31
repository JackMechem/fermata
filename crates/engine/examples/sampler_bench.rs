//! Times Fermata Sampler per block against the block's real-time budget, up to full polyphony, and
//! how long a load takes. Build with --release; a debug build says little.
//! usage: sampler_bench <scratch dir> [seconds per measurement]
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use daw_engine::fx::FxContext;
use daw_engine::instrument::sampler::map::SamplerMap;
use daw_engine::instrument::sampler::params::param;
use daw_engine::instrument::sampler::sample::SamplePool;
use daw_engine::instrument::sampler::state::{SamplerState, Zone};
use daw_engine::instrument::sampler::Sampler;
use daw_engine::instrument::Instrument;
use vst3_host::MidiEvent;

const SR: f32 = 48000.0;

fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut x = seed;
    let mut low = 0.0f32;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let w = (x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            low += (w - low) * 0.3;
            low * 0.5
        })
        .collect()
}

struct Case {
    name: &'static str,
    /// Stereo or mono, its rate, and whether it loops.
    stereo: bool,
    file_rate: u32,
    filter: f32,
    notes: usize,
    /// Semitones above the root each note is held.
    up: u8,
}

fn measure(dir: &Path, pool: &SamplePool, c: &Case, block: usize, seconds: f64) -> (f64, f64) {
    let file = format!("{}-{}.wav", if c.stereo { "stereo" } else { "mono" }, c.file_rate);
    let path = dir.join(&file);
    if !path.exists() {
        let frames = c.file_rate as usize * 20;
        let ch: Vec<Vec<f32>> = if c.stereo { vec![noise(frames, 1), noise(frames, 2)] } else { vec![noise(frames, 1)] };
        daw_engine::decode::write_wav(&path, c.file_rate, &ch).unwrap();
    }
    // Across the keys around its root at full polyphony, or a run going up from `up` above it.
    let root = if c.up == 0 { 60 } else { 36 };
    let mut z = Zone::new(1, file.clone(), file, root);
    z.loop_on = true;
    z.loop_start = 48000;
    z.loop_end = 48000 * 6;
    z.xfade = 4800;
    let state = SamplerState { version: 1, next_id: 2, zones: vec![z] };
    let (map, issues) = SamplerMap::build(&state, dir, SR, pool);
    assert!(issues.is_empty());
    let mut s = Sampler::new(SR, 1);
    drop(s.swap_data(Arc::new(map)));
    s.set_param(param::POLYPHONY, 10.0);
    s.set_param(param::F_TYPE, c.filter);
    s.set_param(param::CUTOFF, 2000.0);
    s.reset();
    let ctx = FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: 0.0 };
    let (mut l, mut r) = (vec![0f32; block], vec![0f32; block]);
    let first = if c.up == 0 { 28 } else { root as usize + c.up as usize };
    let on: Vec<MidiEvent> = (0..c.notes).map(|k| MidiEvent { offset: 0, data: [0x90, (first + k).min(127) as u8, 127] }).collect();
    for chunk in on.chunks(32) {
        s.process(&mut l, &mut r, chunk, &ctx);
    }
    for _ in 0..(0.25 * SR as f64 / block as f64) as usize {
        s.process(&mut l, &mut r, &[], &ctx);
    }
    let blocks = (seconds * SR as f64 / block as f64) as usize;
    let (mut total, mut worst) = (0f64, 0f64);
    for _ in 0..blocks {
        let t = Instant::now();
        s.process(&mut l, &mut r, &[], &ctx);
        let d = t.elapsed().as_secs_f64() * 1e6;
        total += d;
        worst = worst.max(d);
    }
    let mut m = [0.0; 16];
    s.meters(&mut m);
    assert_eq!(m[0] as usize, c.notes.min(64), "{}", c.name);
    (total / blocks as f64, worst)
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    std::fs::create_dir_all(&dir).unwrap();
    let seconds: f64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let t = Instant::now();
    let _ = Sampler::new(SR, 1);
    println!("first instance, building the kernel tables ({} MB): {:.0} ms", daw_engine::instrument::sampler::kernel::kernels().bytes() >> 20, t.elapsed().as_secs_f64() * 1000.0);
    let pool = SamplePool::default();
    let cases = [
        Case { name: "1 stereo voice, pitched", stereo: true, file_rate: 44100, filter: 0.0, notes: 1, up: 1 },
        Case { name: "64 stereo voices, 44.1 kHz files", stereo: true, file_rate: 44100, filter: 0.0, notes: 64, up: 0 },
        Case { name: "64 mono voices, 44.1 kHz files", stereo: false, file_rate: 44100, filter: 0.0, notes: 64, up: 0 },
        Case { name: "64 stereo voices with 12 dB filter", stereo: true, file_rate: 44100, filter: 1.0, notes: 64, up: 0 },
        Case { name: "64 stereo voices with ladder", stereo: true, file_rate: 44100, filter: 4.0, notes: 64, up: 0 },
        Case { name: "64 stereo voices, 96 kHz files", stereo: true, file_rate: 96000, filter: 0.0, notes: 64, up: 0 },
        Case { name: "8 stereo voices, 4 octaves up", stereo: true, file_rate: 48000, filter: 0.0, notes: 8, up: 48 },
    ];
    for block in [512usize, 128] {
        let budget = block as f64 / SR as f64 * 1e6;
        println!("{block} frames at {SR} Hz: budget {budget:.0} us");
        for c in &cases {
            let (mean, worst) = measure(&dir, &pool, c, block, seconds);
            println!("  {:38} mean {mean:8.1} us ({:6.2} %)  worst {worst:8.1} us ({:6.2} %)", c.name, 100.0 * mean / budget, 100.0 * worst / budget);
        }
    }
    let big = dir.join("load.wav");
    let frames = 48000 * 300;
    let spec = hound::WavSpec { channels: 2, sample_rate: 48000, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&big, spec).unwrap();
    for v in noise(frames, 3) {
        let i = (v as f64 * 8388607.0) as i32;
        w.write_sample(i).unwrap();
        w.write_sample(i).unwrap();
    }
    w.finalize().unwrap();
    let mb = std::fs::metadata(&big).unwrap().len() as f64 / 1e6;
    let pool = SamplePool::default();
    let t = Instant::now();
    let buf = pool.get(&big).unwrap();
    println!("decoding a 5 minute stereo 24-bit WAV ({mb:.0} MB): {:.0} ms, {} MB in memory", t.elapsed().as_secs_f64() * 1000.0, buf.bytes() >> 20);
    let mut z = Zone::new(1, "z".into(), "load.wav".into(), 60);
    z.loop_on = true;
    z.loop_start = 48000;
    z.loop_end = 48000 * 120;
    z.xfade = 48000;
    let state = SamplerState { version: 1, next_id: 2, zones: vec![z] };
    let t = Instant::now();
    let _m = SamplerMap::build(&state, &dir, SR, &pool);
    println!("baking a 2 minute loop in it: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let zones: Vec<Zone> = (0..2000).map(|i| Zone::new(i, "z".into(), "load.wav".into(), 60)).collect();
    let state = SamplerState { version: 1, next_id: 2, zones };
    let t = Instant::now();
    let text = state.to_json();
    let parsed = SamplerState::parse(Some(&text));
    let _m = SamplerMap::build(&parsed, &dir, SR, &pool);
    println!("an edit to an instrument of 2000 zones: {:.1} ms ({} KB of state)", t.elapsed().as_secs_f64() * 1000.0, text.len() >> 10);
    let _ = std::fs::remove_file(&big);
}
