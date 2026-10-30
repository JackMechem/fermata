//! Writes a small demo project (synthesized drums, a plucked part and a MIDI region) for
//! trying the UI: `cargo run -p daw-engine --example demo_project -- <out.fermata>`.
use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, Note, Region, RegionContent, TrackKind};
use daw_engine::session::TrackPatch;
use daw_engine::Session;
use std::f32::consts::TAU;

const SR: u32 = 48000;

fn write_wav(path: &std::path::Path, ch: Vec<Vec<f32>>) {
    let spec = hound::WavSpec { channels: ch.len() as u16, sample_rate: SR, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..ch[0].len() {
        for c in &ch {
            w.write_sample(c[i]).unwrap();
        }
    }
    w.finalize().unwrap();
}

fn drums(bars: usize) -> Vec<Vec<f32>> {
    let beat = SR as usize / 2;
    let len = beat * 4 * bars;
    let mut l = vec![0.0f32; len];
    let mut seed = 12345u32;
    let mut noise = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 9) as f32 / (1 << 23) as f32 * 2.0 - 1.0
    };
    for b in 0..bars * 8 {
        let at = b * beat / 2;
        let step = b % 8;
        if step == 0 || step == 3 || step == 5 {
            for i in 0..(SR as usize / 4).min(len - at) {
                let t = i as f32 / SR as f32;
                let f = 50.0 + 90.0 * (-t * 30.0).exp();
                l[at + i] += (TAU * f * t).sin() * (-t * 9.0).exp() * 0.85;
            }
        }
        if step == 2 || step == 6 {
            for i in 0..(SR as usize / 5).min(len - at) {
                let t = i as f32 / SR as f32;
                l[at + i] += (noise() * 0.6 + (TAU * 190.0 * t).sin() * 0.4) * (-t * 18.0).exp() * 0.6;
            }
        }
        for i in 0..(SR as usize / 30).min(len - at) {
            let t = i as f32 / SR as f32;
            l[at + i] += noise() * (-t * 90.0).exp() * 0.18;
        }
    }
    vec![l.clone(), l]
}

fn pluck(notes: &[(f32, f32)], len_s: f32) -> Vec<Vec<f32>> {
    let len = (len_s * SR as f32) as usize;
    let mut out = vec![0.0f32; len];
    let mut seed = 99u32;
    for &(at, freq) in notes {
        let period = (SR as f32 / freq) as usize;
        let mut buf: Vec<f32> = (0..period)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 9) as f32 / (1 << 23) as f32 * 2.0 - 1.0
            })
            .collect();
        let start = (at * SR as f32) as usize;
        for i in 0..(SR as usize * 2) {
            if start + i >= len {
                break;
            }
            let k = i % period;
            let next = buf[(k + 1) % period];
            buf[k] = 0.996 * 0.5 * (buf[k] + next);
            out[start + i] += buf[k] * 0.35;
        }
    }
    vec![out]
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("output path"));
    let mut s = Session::new(AudioConfig { input_enabled: false, ..Default::default() }, daw_engine::plugin_db::load_cache().unwrap_or_default());
    let audio_dir = out.join("Audio Files");
    std::fs::create_dir_all(&audio_dir).unwrap();
    let dpath = audio_dir.join("Drum Loop.wav");
    write_wav(&dpath, drums(4));
    let gpath = audio_dir.join("Pluck.wav");
    let ns = [(0.0, 196.0), (0.5, 246.9), (1.0, 293.7), (1.5, 392.0), (2.0, 174.6), (2.5, 220.0), (3.0, 261.6), (3.5, 349.2), (4.0, 164.8), (4.5, 196.0), (5.0, 246.9), (5.5, 329.6), (6.0, 174.6), (6.5, 220.0), (7.0, 261.6), (7.5, 349.2)];
    write_wav(&gpath, pluck(&ns, 8.5));

    let bar = (SR * 2) as i64;
    let drums_t = s.add_track(TrackKind::Audio, InputSel::Stereo(0), None, None).unwrap();
    s.update_track(drums_t, TrackPatch { name: Some("Drums".into()), volume_db: Some(-3.0), ..Default::default() });
    s.import_audio(&dpath, drums_t, 0).unwrap();
    s.import_audio(&dpath, drums_t, 4 * bar).unwrap();
    let gtr = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.update_track(gtr, TrackPatch { name: Some("Pluck".into()), pan: Some(-0.3), color: Some("#f2a63b".into()), ..Default::default() });
    s.import_audio(&gpath, gtr, 2 * bar).unwrap();
    let keys = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.update_track(keys, TrackPatch { name: Some("Pad".into()), volume_db: Some(-8.0), ..Default::default() });
    let beat = SR as i64 / 2;
    let mut notes = Vec::new();
    for (i, chord) in [[55u8, 59, 62], [53, 57, 60], [52, 55, 59], [53, 57, 60]].iter().enumerate() {
        for &p in chord {
            notes.push(Note { start: i as i64 * 4 * beat, length: 4 * beat - beat / 4, pitch: p, velocity: 70 + (p % 5) * 8, muted: false });
        }
        notes.push(Note { start: i as i64 * 4 * beat + 2 * beat, length: beat, pitch: chord[2] + 12, velocity: 100, muted: false });
    }
    let id = s.project.alloc_id();
    s.project.track_mut(keys).unwrap().regions.push(Region { id, name: "Pad".into(), start: 0, length: 4 * bar, muted: false, color: None, content: RegionContent::Midi { notes, cc: Vec::new() }, loop_end: None, takes: None, params: Default::default() });
    if let Some(p) = s.plugins.iter().find(|p| p.name == "ZamCompX2").cloned() {
        s.set_plugin(drums_t, 0, &p.id).unwrap();
    }
    if let Some(p) = s.plugins.iter().find(|p| p.name == "Dragonfly Room Reverb").cloned() {
        s.set_plugin(gtr, 0, &p.id).unwrap();
    }
    s.set_cycle(true, 0, 4 * bar);
    s.set_cycle(false, 0, 4 * bar);
    s.select_track(Some(drums_t));
    let path = s.save(Some(out)).unwrap();
    println!("wrote {}", path.display());
}
