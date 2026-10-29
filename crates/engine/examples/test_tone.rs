//! Writes the test tone the headless checks play: 3 seconds of a 440 Hz sine at -21.5 dBFS,
//! stereo, 32-bit float, 48 kHz (the engine's default rate, so nothing is resampled). It
//! stands in for a real recording where none is at hand, as in CI.
//! usage: test_tone <output.wav>
use std::f64::consts::TAU;

const RATE: u32 = 48_000;
const SECONDS: u32 = 3;
const HZ: f64 = 440.0;
/// Quiet enough that sums of several tracks and +dB gains in the checks stay below clipping.
const AMPLITUDE: f64 = 0.084;

fn main() {
    let out = std::env::args().nth(1).expect("usage: test_tone <output.wav>");
    let spec = hound::WavSpec { channels: 2, sample_rate: RATE, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(&out, spec).expect("create the WAV file");
    for i in 0..RATE * SECONDS {
        let x = (AMPLITUDE * (TAU * HZ * i as f64 / RATE as f64).sin()) as f32;
        w.write_sample(x).unwrap();
        w.write_sample(x).unwrap();
    }
    w.finalize().expect("write the WAV file");
}
