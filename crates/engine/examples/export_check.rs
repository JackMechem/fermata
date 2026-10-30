//! Bounces to AIFF and FLAC and exports the selected tracks, against a running engine (silent
//! ALSA null output). Every file is decoded back and compared with a WAV of the same render.
//! usage: export_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, TrackKind};
use daw_engine::session::{AudioFormat, BitDepth, RenderOptions, TrackPatch};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Decodes any bounce with the importer's decoder: (rate, channels).
fn read(path: &Path) -> (u32, Vec<Vec<f32>>) {
    let d = daw_engine::decode::decode(path).unwrap_or_else(|e| panic!("decoding {}: {e}", path.display()));
    (d.sample_rate, d.channels)
}

/// The largest sample difference between two renders, or infinity when their shapes differ.
fn max_diff(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    if a.len() != b.len() || a.iter().zip(b).any(|(x, y)| x.len() != y.len()) {
        return f32::INFINITY;
    }
    a.iter().zip(b).flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs())).fold(0.0, f32::max)
}

fn peak(a: &[Vec<f32>]) -> f32 {
    a.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn file_name(p: &str) -> String {
    Path::new(p).file_name().unwrap().to_string_lossy().to_string()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let out = scratch.join("exports");
    let _ = std::fs::remove_dir_all(&out);
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("ExportCheck.fermata");
    let _ = std::fs::remove_dir_all(&s.dir);
    let sr = s.sample_rate() as u32;
    println!("audio: {:?}", s.audio_status);

    // Three tone tracks at different levels, so each stem is told apart by its peak.
    let mut tracks = Vec::new();
    for (k, db) in [0.0, -6.0, -12.0].into_iter().enumerate() {
        let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
        s.import_audio(Path::new(&tone), t, (k as i64) * 4800).unwrap();
        s.update_track(t, TrackPatch { volume_db: Some(db), ..Default::default() });
        tracks.push(t);
    }
    let end = s.project_end();
    let opts = |format, bit_depth| RenderOptions { start: 0, end, tail_secs: 0.0, bit_depth, format, ..Default::default() };

    // ---- Bounce Project in every format ------------------------------------------------------
    let reference = out.join("mix-float.wav");
    s.bounce_project(&reference, &opts(AudioFormat::Wav, BitDepth::Float32), &mut |_| {}).unwrap();
    let (_, float) = read(&reference);
    check(&format!("float WAV mix renders ({} frames, peak {:.3})", float[0].len(), peak(&float)), float[0].len() as i64 == end && peak(&float) > 0.1);

    for (depth, bits) in [(BitDepth::Int16, 16), (BitDepth::Int24, 24)] {
        let wav = out.join(format!("mix{bits}.wav"));
        s.bounce_project(&wav, &opts(AudioFormat::Wav, depth), &mut |_| {}).unwrap();
        let (_, w) = read(&wav);
        // Rounding is half an LSB, and the writer's full scale (2^(n-1) - 1) is one LSB short of
        // the reader's; 16-bit adds up to ±1 LSB of dither.
        let lsb = 1.0 / (1u32 << (bits - 1)) as f32;
        let tolerance = if bits == 16 { 2.5 * lsb } else { 1.5 * lsb };
        for format in [AudioFormat::Aiff, AudioFormat::Flac] {
            let p = out.join(format!("mix{bits}.{}", format.extension()));
            let res = s.bounce_project(&p, &opts(format, depth), &mut |_| {}).unwrap();
            let (rate, got) = read(&p);
            check(&format!("{bits}-bit {format:?}: {} written and decodes at {rate} Hz", file_name(&res.paths[0])), rate == sr && p.exists());
            check(&format!("{bits}-bit {format:?} matches the {bits}-bit WAV sample for sample (max diff {:e})", max_diff(&got, &w)), max_diff(&got, &w) == 0.0);
            check(&format!("{bits}-bit {format:?} is within quantization of the float mix ({:.2} LSB)", max_diff(&got, &float) / lsb), max_diff(&got, &float) <= tolerance);
        }
        let flac = std::fs::metadata(out.join(format!("mix{bits}.flac"))).unwrap().len();
        let wav_len = std::fs::metadata(&wav).unwrap().len();
        check(&format!("{bits}-bit FLAC is compressed ({flac} bytes vs {wav_len} for WAV)"), flac < wav_len);
    }
    let p = out.join("mix-float.aif");
    s.bounce_project(&p, &opts(AudioFormat::Aiff, BitDepth::Float32), &mut |_| {}).unwrap();
    check("32-bit float AIFF-C is identical to the float WAV", max_diff(&read(&p).1, &float) == 0.0);
    let bad = out.join("float.flac");
    let refused = s.bounce_project(&bad, &opts(AudioFormat::Flac, BitDepth::Float32), &mut |_| {});
    check(&format!("FLAC refuses 32-bit float ({:?}) and writes nothing", refused.as_ref().err().map(|e| e.to_string())), refused.is_err() && !bad.exists());

    // Sample rate conversion and normalize go through the new writers too.
    let p = out.join("mix44.flac");
    let res = s.bounce_project(&p, &RenderOptions { sample_rate: Some(44100), normalize: true, ..opts(AudioFormat::Flac, BitDepth::Int24) }, &mut |_| {}).unwrap();
    let (rate, conv) = read(&p);
    let want = (end as f64 * 44100.0 / sr as f64).round() as usize;
    check(&format!("44.1 kHz normalized FLAC ({rate} Hz, {} frames, want {want}, peak {:.2} dB)", conv[0].len(), res.peak_db), rate == 44100 && conv[0].len() == want && (20.0 * peak(&conv).log10() + 0.1).abs() < 0.01);

    // ---- Export Selected Tracks --------------------------------------------------------------
    let all = s.export_tracks(&out.join("all"), &opts(AudioFormat::Wav, BitDepth::Float32), &mut |_| {}).unwrap();
    check(&format!("Export All Tracks writes three files ({:?})", all.paths.iter().map(|p| file_name(p)).collect::<Vec<_>>()), all.paths.len() == 3);
    let picked = [tracks[0], tracks[2]];
    check("targets of the selection: the two selected tracks", s.export_targets_of(&picked) == picked.to_vec());
    let res = s.export_selected_tracks(&out.join("selected"), &picked, &opts(AudioFormat::Flac, BitDepth::Int24), &mut |_| {}).unwrap();
    let names: Vec<String> = res.paths.iter().map(|p| file_name(p)).collect();
    println!("selected: {names:?}");
    check("two of three tracks exported as FLAC, numbered by track", names == vec!["01 Audio 1.flac".to_string(), "03 Audio 3.flac".to_string()]);
    let lsb = 1.0 / (1u32 << 23) as f32;
    for (got, want) in [(&res.paths[0], &all.paths[0]), (&res.paths[1], &all.paths[2])] {
        let (g, w) = (read(Path::new(got)).1, read(Path::new(want)).1);
        check(&format!("{} matches its Export All Tracks stem within 24-bit quantization ({:.2} LSB)", file_name(got), max_diff(&g, &w) / lsb), max_diff(&g, &w) <= 1.5 * lsb);
    }
    let aiff = s.export_selected_tracks(&out.join("selected-aiff"), &[tracks[1]], &opts(AudioFormat::Aiff, BitDepth::Int16), &mut |_| {}).unwrap();
    let (g, w) = (read(Path::new(&aiff.paths[0])).1, read(Path::new(&all.paths[1])).1);
    check(&format!("one selected track as 16-bit AIFF: {} ({:.2} LSB from the stem)", file_name(&aiff.paths[0]), max_diff(&g, &w) * 32768.0), aiff.paths.len() == 1 && aiff.paths[0].ends_with("02 Audio 2.aif") && max_diff(&g, &w) <= 2.5 / 32768.0);

    // A selected folder stack stands for its subtracks; a track without regions is left out.
    let folder = s.create_stack(&[tracks[1], tracks[2]], false).unwrap();
    let empty = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let targets = s.export_targets_of(&[folder, empty]);
    check(&format!("folder stack selected: its two subtracks are the targets ({targets:?})"), targets == vec![tracks[1], tracks[2]]);
    let res = s.export_selected_tracks(&out.join("folder"), &[folder], &opts(AudioFormat::Wav, BitDepth::Int24), &mut |_| {}).unwrap();
    check(&format!("folder export writes its subtracks ({:?})", res.paths.iter().map(|p| file_name(p)).collect::<Vec<_>>()), res.paths.len() == 2);
    let none = s.export_selected_tracks(&out.join("none"), &[empty], &opts(AudioFormat::Wav, BitDepth::Int24), &mut |_| {});
    check("a selection with nothing to play is refused", none.is_err());
    let refused = s.export_selected_tracks(&out.join("float-flac"), &picked, &opts(AudioFormat::Flac, BitDepth::Float32), &mut |_| {});
    check("float FLAC export refused before writing anything", refused.is_err() && !out.join("float-flac").exists());

    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
