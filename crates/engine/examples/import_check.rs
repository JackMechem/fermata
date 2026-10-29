//! Imports the given files onto new tracks through the session (silent ALSA null output) and
//! reports what was created: `--example import_check -- <project dir> <files...>`.
use daw_engine::audio_io::AudioConfig;
use daw_engine::model::RegionContent;
use daw_engine::Session;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.save(Some(args[0].clone().into())).unwrap();
    let files: Vec<std::path::PathBuf> = args[1..].iter().map(Into::into).collect();
    let errors = s.import_audio_files(&files, None, 0, None);
    println!("errors: {errors:?}");
    let sr = s.sample_rate();
    for t in &s.project.tracks {
        for r in &t.regions {
            let RegionContent::Audio { clip, .. } = r.content else { continue };
            let path = &s.project.clips.iter().find(|c| c.id == clip).unwrap().path;
            let peak = s.clip_peak(clip).unwrap();
            println!("{:<8} input={:?} len={:.3}s peak={:.2} file={}", t.name, t.input, r.length as f64 / sr, peak, path);
        }
    }
}
