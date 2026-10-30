//! Times every built-in effect per block (the worst block and the average), against the
//! block's real-time budget. usage: fx_bench [block]
use daw_engine::fx::{self, FxContext};
use std::time::Instant;
fn main() {
    let block: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(512);
    let sr = 48000.0f32;
    let ctx = FxContext { sample_rate: sr, tempo: 120.0, playing: true, position: 0.0 };
    let budget = block as f64 / sr as f64 * 1e3;
    println!("block {block}: budget {budget:.2} ms");
    for info in fx::registry() {
        let mut e = (info.new)(sr);
        let (mut l, mut r) = (vec![0f32; block], vec![0f32; block]);
        let (mut worst, mut total, mut n) = (0f64, 0f64, 0usize);
        let mut ph = 0f64;
        for _ in 0..(5.0 * sr as f64 / block as f64) as usize {
            for i in 0..block {
                ph += 110.0 / sr as f64;
                l[i] = (0.3 * (std::f64::consts::TAU * ph).sin()) as f32;
                r[i] = l[i];
            }
            let t = Instant::now();
            e.process(&mut l, &mut r, None, &ctx);
            let d = t.elapsed().as_secs_f64() * 1e3;
            worst = worst.max(d);
            total += d;
            n += 1;
        }
        println!("{:16} worst {:7.3} ms ({:5.1}%)  avg {:7.3} ms ({:5.1}%)", info.name, worst, 100.0 * worst / budget, total / n as f64, 100.0 * total / n as f64 / budget);
    }
}
