//! Times each built-in instrument per block against the block's real-time budget, playing one
//! voice and then many. Build with --release; a debug build says little.
//! usage: instrument_bench [seconds per measurement]
use std::time::Instant;

use daw_engine::fx::FxContext;
use daw_engine::instrument::tre::params::param;
use daw_engine::instrument::{self, InstrumentInfo};
use vst3_host::MidiEvent;

const SR: f32 = 48000.0;

/// A name, the settings and how many notes to hold.
type Case = (&'static str, Vec<(u32, f32)>, usize);

/// Plays `notes` held notes at `block` frames and returns the mean and worst block time in µs.
fn measure(info: &InstrumentInfo, values: &[(u32, f32)], notes: usize, block: usize, seconds: f64) -> (f64, f64) {
    let mut i = (info.new)(SR, 1);
    for &(id, v) in values {
        i.set_param(id, v);
    }
    let ctx = FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: 0.0 };
    let (mut l, mut r) = (vec![0f32; block], vec![0f32; block]);
    let on: Vec<MidiEvent> = (0..notes).map(|k| MidiEvent { offset: 0, data: [0x90, 36 + (k as u8 * 5) % 48, 100] }).collect();
    i.process(&mut l, &mut r, &on, &ctx);
    // Past the attack, and into the steady state where a note spends its time.
    for _ in 0..(0.25 * SR as f64 / block as f64) as usize {
        i.process(&mut l, &mut r, &[], &ctx);
    }
    let blocks = (seconds * SR as f64 / block as f64) as usize;
    let (mut total, mut worst) = (0f64, 0f64);
    for _ in 0..blocks {
        let t = Instant::now();
        i.process(&mut l, &mut r, &[], &ctx);
        let d = t.elapsed().as_secs_f64() * 1e6;
        total += d;
        worst = worst.max(d);
    }
    (total / blocks as f64, worst)
}

fn main() {
    let seconds: f64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(4.0);
    let tre = instrument::find("builtin:tre").unwrap();
    let voices = |mode: f32, count: f32| vec![(param::VOICE_MODE, mode), (param::VOICES, count), (param::ON3, 1.0), (param::NOISE_ON, 1.0), (param::NOISE, 1.0), (param::EMPHASIS, 6.0), (param::CUTOFF, 900.0)];
    let cases: Vec<Case> = vec![
        ("one voice: all 3 oscillators, noise", voices(0.0, 0.0), 1),
        ("one voice, with filter and osc mod", [voices(0.0, 0.0), vec![(param::FILTER_MOD, 1.0), (param::OSC_MOD, 1.0), (param::MOD_AMOUNT, 3.0), (param::MOD_MIX, 2.0)]].concat(), 1),
        ("poly, 4 notes", voices(2.0, 2.0), 4),
        ("poly, 8 notes", voices(2.0, 4.0), 8),
    ];
    for block in [512usize, 128] {
        let budget = block as f64 / SR as f64 * 1e6;
        println!("{block} frames at {SR} Hz: budget {budget:.0} us");
        for (name, values, notes) in &cases {
            let (mean, worst) = measure(&tre, values, *notes, block, seconds);
            println!("  Fermata Tre, {name:38} mean {mean:7.1} us ({:5.2} %)  worst {worst:7.1} us ({:5.2} %)", 100.0 * mean / budget, 100.0 * worst / budget);
        }
        let (mean, worst) = measure(&tre, &[], 0, block, seconds.min(1.0));
        println!("  Fermata Tre, {:38} mean {mean:7.1} us ({:5.2} %)  worst {worst:7.1} us ({:5.2} %)", "at rest (no notes)", 100.0 * mean / budget, 100.0 * worst / budget);
        let basic = instrument::find(daw_engine::session::BUILTIN_SYNTH).unwrap();
        let (mean, worst) = measure(&basic, &[], 1, block, seconds);
        println!("  Fermata Synth, {:36} mean {mean:7.1} us ({:5.2} %)  worst {worst:7.1} us ({:5.2} %)", "one voice, for scale", 100.0 * mean / budget, 100.0 * worst / budget);
    }
}
