//! Exercises offline bounces and exports against a running engine (silent ALSA null output).
//! usage: bounce_check <audio file> <scratch dir>
use std::path::{Path, PathBuf};
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{InputSel, Note, Region, RegionContent, TrackKind};
use daw_engine::session::{BipOptions, BipSource, BitDepth, RenderOptions, TrackPatch};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Reads a WAV back as float channels, with its spec.
fn read(path: &Path) -> (hound::WavSpec, Vec<Vec<f32>>) {
    let mut r = hound::WavReader::open(path).unwrap();
    let spec = r.spec();
    let ch = spec.channels as usize;
    let flat: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.unwrap() as f32 * scale).collect()
        }
    };
    let mut out = vec![Vec::new(); ch];
    for f in flat.chunks_exact(ch) {
        for (c, o) in out.iter_mut().enumerate() {
            o.push(f[c]);
        }
    }
    (spec, out)
}

fn peak(a: &[Vec<f32>]) -> f32 {
    a.iter().flatten().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn rms(a: &[f32]) -> f32 {
    (a.iter().map(|s| s * s).sum::<f32>() / a.len().max(1) as f32).sqrt()
}

fn db(g: f32) -> f32 {
    20.0 * g.max(1e-12).log10()
}

/// Plays a moment and returns the peak on Stereo Out, to show live playback still works.
fn live_level(s: &mut Session) -> f32 {
    s.locate(0);
    s.play();
    std::thread::sleep(Duration::from_millis(150));
    s.tick();
    std::thread::sleep(Duration::from_millis(250));
    let t = s.tick();
    s.stop();
    std::thread::sleep(Duration::from_millis(60));
    t.master.0.max(t.master.1)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tone = args.next().expect("audio file");
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let out = scratch.join("bounces");
    let _ = std::fs::remove_dir_all(&out);
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let plugins = daw_engine::plugin_db::load_cache().unwrap_or_default();
    let comp = plugins.iter().find(|p| p.name == "ZamCompX2").map(|p| p.id.clone());
    let mut s = Session::new(cfg, plugins);
    s.dir = scratch.join("BounceCheck.fermata");
    let _ = std::fs::remove_dir_all(&s.dir);
    let sr = s.sample_rate();
    println!("audio: {:?}", s.audio_status);

    let a = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(Path::new(&tone), a, 0).unwrap();
    let region = s.project.track(a).unwrap().regions[0].clone();
    let clip = match &region.content {
        RegionContent::Audio { clip, .. } => *clip,
        _ => unreachable!(),
    };
    let tone_peak = s.clip_peak(clip).unwrap();
    // Cycle on, so the null device's flat-out playback loops; bounces must ignore it.
    s.set_cycle(true, 0, 48000);
    let end = s.project_end();
    check(&format!("project end is the region end ({end})"), end == region.length);

    // ---- Bounce Project ---------------------------------------------------------------------
    s.locate(12345);
    std::thread::sleep(Duration::from_millis(30));
    let opts = RenderOptions { start: 0, end, tail_secs: 2.0, bit_depth: BitDepth::Int24, ..Default::default() };
    let p24 = out.join("mix24.wav");
    let res = s.bounce_project(&p24, &opts, &mut |_| {}).unwrap();
    let (spec, mix) = read(&p24);
    println!("bounce: {res:?}");
    check(&format!("24-bit stereo WAV at the project rate ({spec:?})"), spec.bits_per_sample == 24 && spec.channels == 2 && spec.sample_rate == sr as u32);
    check(&format!("silent tail trimmed: length = range ({} vs {end})", mix[0].len()), mix[0].len() as i64 == end);
    check(&format!("peak matches the tone ({:.4} vs {tone_peak:.4})", peak(&mix)), (peak(&mix) - tone_peak).abs() < 1e-3);
    check(&format!("transport restored to where it was ({})", s.position()), s.position() == 12345 && !s.is_playing() && s.project.cycle.enabled);
    // Mono source panned centre with the -3 dB-compensated law: unity on both sides.
    check("both channels carry the tone", rms(&mix[0]) > 0.01 && (rms(&mix[0]) - rms(&mix[1])).abs() < 1e-3);

    // A section, at a different rate, in 16-bit.
    let sec = RenderOptions { start: 24000, end: 72000, tail_secs: 0.0, bit_depth: BitDepth::Int16, sample_rate: Some(44100), normalize: false, ..Default::default() };
    let p16 = out.join("section16.wav");
    s.bounce_project(&p16, &sec, &mut |_| {}).unwrap();
    let (spec, part) = read(&p16);
    let want = (48000.0 * 44100.0 / sr).round() as usize;
    check(&format!("16-bit 44.1 kHz section is valid ({spec:?}, {} frames, want {want})", part[0].len()), spec.bits_per_sample == 16 && spec.sample_rate == 44100 && part[0].len() == want);
    check(&format!("16-bit section has the tone ({:.4})", peak(&part)), (peak(&part) - tone_peak).abs() < 0.01);

    // Normalize to -0.1 dBFS, also with the master fader down (normalize undoes it).
    s.set_master_volume(-10.0);
    let pn = out.join("norm.wav");
    let res = s.bounce_project(&pn, &RenderOptions { normalize: true, bit_depth: BitDepth::Float32, ..opts.clone() }, &mut |_| {}).unwrap();
    let (spec, norm) = read(&pn);
    check(&format!("normalize hits -0.1 dBFS ({:.3} dB, reported {:.3})", db(peak(&norm)), res.peak_db), (db(peak(&norm)) + 0.1).abs() < 0.01 && spec.sample_format == hound::SampleFormat::Float);
    let pq = out.join("quiet.wav");
    s.bounce_project(&pq, &opts, &mut |_| {}).unwrap();
    let (_, quiet) = read(&pq);
    check(&format!("full mix goes through the Stereo Out fader ({:.2} dB)", db(peak(&quiet) / tone_peak)), (db(peak(&quiet) / tone_peak) + 10.0).abs() < 0.1);
    s.set_master_volume(0.0);

    // ---- An instrument: MIDI renders, notes end, the release becomes the tail -------------------
    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    let bar = s.project.samples_per_bar().round() as i64;
    let notes = vec![Note { start: 0, length: bar / 2, pitch: 60, velocity: 100, muted: false }, Note { start: bar / 2, length: bar * 4, pitch: 64, velocity: 100, muted: false }];
    let midi = Region { id: 0, name: "Chord".into(), start: 0, length: bar, muted: false, color: None, loop_end: None, takes: None, content: RegionContent::Midi { notes, cc: Vec::new() } };
    s.paste_regions(&[(inst, midi)], 0);
    s.update_track(a, TrackPatch { mute: Some(true), ..Default::default() });
    let pi = out.join("inst.wav");
    s.bounce_project(&pi, &RenderOptions { start: 0, end: bar, tail_secs: 2.0, bit_depth: BitDepth::Float32, ..Default::default() }, &mut |_| {}).unwrap();
    let (_, synth) = read(&pi);
    let len = synth[0].len() as i64;
    check(&format!("instrument renders ({:.3}) and the tone track stays muted", peak(&synth)), peak(&synth) > 0.01);
    check(&format!("the held note is released at the end: tail of {:.0} ms, under 2 s", (len - bar) as f64 * 1000.0 / sr), len > bar && len < bar + (2.0 * sr) as i64);
    s.update_track(a, TrackPatch { mute: Some(false), ..Default::default() });

    // ---- Export All Tracks -------------------------------------------------------------------
    s.update_track(a, TrackPatch { volume_db: Some(-6.0), ..Default::default() });
    let stems = out.join("stems");
    let res = s.export_tracks(&stems, &RenderOptions { start: 0, end, tail_secs: 1.0, bit_depth: BitDepth::Int24, ..Default::default() }, &mut |_| {}).unwrap();
    let names: Vec<String> = res.paths.iter().map(|p| Path::new(p).file_name().unwrap().to_string_lossy().to_string()).collect();
    println!("stems: {names:?}");
    check("one file per track, numbered", names == vec!["01 Audio 1.wav".to_string(), "02 Inst 1.wav".to_string()]);
    let lens: Vec<usize> = res.paths.iter().map(|p| read(Path::new(p)).1[0].len()).collect();
    check(&format!("stems line up: same length, range + tail ({lens:?})"), lens.iter().all(|&l| l as i64 == end + sr as i64));
    let (_, stem) = read(Path::new(&res.paths[0]));
    check(&format!("stem is post-fader ({:.2} dB)", db(peak(&stem) / tone_peak)), (db(peak(&stem) / tone_peak) + 6.0).abs() < 0.1);
    let (_, stem2) = read(Path::new(&res.paths[1]));
    // The tone (at -6 dB) would still be playing at the end of the range; the synth's release
    // has nearly died away by then, and is silent by the end of the tail.
    let (late, last) = (rms(&stem2[0][end as usize - 1000..end as usize]), rms(&stem2[0][stem2[0].len() - 1000..]));
    check(&format!("instrument stem has only the instrument ({late:.5} at the range end, {last:.6} at the file end)"), peak(&stem2) > 0.01 && late < 0.005 && last < 1e-4);

    // ---- Export Region ------------------------------------------------------------------------
    let pr = out.join("region.wav");
    s.export_region(region.id, &pr, &RenderOptions { tail_secs: 0.5, ..Default::default() }, &mut |_| {}).unwrap();
    let (_, reg) = read(&pr);
    check(&format!("region export is pre-fader and region-length ({} frames, {:.2} dB)", reg[0].len(), db(peak(&reg) / tone_peak)), reg[0].len() as i64 == region.length && db(peak(&reg) / tone_peak).abs() < 0.1);

    // ---- Bounce in Place ----------------------------------------------------------------------
    let tracks_before = s.project.tracks.len();
    s.checkpoint("Bounce in Place", None, false);
    let made = s.bounce_in_place(&[a], &[], &BipOptions { effects: true, fader: false, source: BipSource::Mute, tail: true }, &mut |_| {}).unwrap();
    let src = s.project.track(a).unwrap().clone();
    let idx = s.project.tracks.iter().position(|t| t.id == made[0]).unwrap();
    let nt = s.project.tracks[idx].clone();
    check(&format!("bounce track \"{}\" sits below the source with its colour", nt.name), nt.name == "Audio 1 (bounced)" && idx == 1 && nt.color == src.color);
    check("new track keeps the source's level (volume not printed)", (nt.volume_db + 6.0).abs() < 1e-3);
    check("source muted", src.mute);
    let r = &nt.regions[0];
    let RegionContent::Audio { clip: bclip, .. } = r.content else { unreachable!() };
    let bpeak = s.clip_peak(bclip).unwrap();
    check(&format!("one region at the start, unity level ({} frames at {}, {:.2} dB)", r.length, r.start, db(bpeak / tone_peak)), nt.regions.len() == 1 && r.start == 0 && r.length == end && db(bpeak / tone_peak).abs() < 0.1);
    let file = s.dir.join("Audio Files").join("Audio 1 (bounced).wav");
    check("written to the project's Audio Files", file.exists());
    let lvl = live_level(&mut s);
    check(&format!("bounce plays live in place of the muted source ({lvl:.3})"), lvl > 0.01);
    s.undo();
    check("undo removes the bounce and unmutes the source", s.project.tracks.len() == tracks_before && !s.project.track(a).unwrap().mute);

    // With the fader printed, and the source muted beforehand (it still renders).
    s.update_track(a, TrackPatch { mute: Some(true), ..Default::default() });
    let made = s.bounce_in_place(&[a], &[], &BipOptions { fader: true, source: BipSource::Leave, ..Default::default() }, &mut |_| {}).unwrap();
    let nt = s.project.track(made[0]).unwrap().clone();
    let RegionContent::Audio { clip: bclip, .. } = nt.regions[0].content else { unreachable!() };
    let gain = db(s.clip_peak(bclip).unwrap() / tone_peak);
    check(&format!("printed fader: -6 dB in the file, new track at 0 dB ({gain:.2} dB)"), (gain + 6.0).abs() < 0.1 && nt.volume_db == 0.0);
    s.delete_track(made[0]);
    s.update_track(a, TrackPatch { mute: Some(false), volume_db: Some(0.0), ..Default::default() });

    // Instrument track: MIDI to audio, source deleted, and undo brings it back.
    s.checkpoint("Bounce in Place", None, true);
    let made = s.bounce_in_place(&[inst], &[], &BipOptions { source: BipSource::Delete, ..Default::default() }, &mut |_| {}).unwrap();
    let nt = s.project.track(made[0]).unwrap().clone();
    let RegionContent::Audio { clip: bclip, .. } = nt.regions[0].content else { unreachable!() };
    check(&format!("instrument bounces to audio ({:.3}), tail kept ({} > {bar})", s.clip_peak(bclip).unwrap(), nt.regions[0].length), s.clip_peak(bclip).unwrap() > 0.01 && nt.regions[0].length > bar && nt.kind == TrackKind::Audio);
    check("source instrument track deleted", s.project.track(inst).is_none());
    s.undo();
    check("undo restores the instrument track and drops the bounce", s.project.track(inst).map(|t| t.instrument.is_some()).unwrap_or(false) && s.project.track(made[0]).is_none());

    // Summing stack: its output, through the stack.
    let b = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    s.import_audio(Path::new(&tone), b, 0).unwrap();
    let sum = s.create_stack(&[a, b], true).unwrap();
    s.update_track(sum, TrackPatch { volume_db: Some(-12.0), ..Default::default() });
    let made = s.bounce_in_place(&[sum, a], &[], &BipOptions { fader: true, source: BipSource::Leave, ..Default::default() }, &mut |_| {}).unwrap();
    let nt = s.project.track(made[0]).unwrap().clone();
    let RegionContent::Audio { clip: bclip, .. } = nt.regions[0].content else { unreachable!() };
    // Two copies of the tone (+6 dB) through the stack fader (-12 dB).
    let gain = db(s.clip_peak(bclip).unwrap() / tone_peak);
    check(&format!("summing stack bounces its subtracks through itself ({gain:.2} dB), one bounce for the stack"), made.len() == 1 && (gain + 6.0).abs() < 0.2);
    let family = s.project.descendants(sum);
    check("the stack's bounce lands below its subtracks, outside the stack", s.project.tracks.iter().position(|t| t.id == made[0]).unwrap() == s.project.tracks.iter().rposition(|t| family.contains(&t.id) || t.id == sum).unwrap() + 1 && nt.parent.is_none());

    // From the region menu: just one region.
    let rid = s.project.track(b).unwrap().regions[0].id;
    let made = s.bounce_in_place(&[], &[rid], &BipOptions { source: BipSource::Mute, ..Default::default() }, &mut |_| {}).unwrap();
    let nt = s.project.track(made[0]).unwrap().clone();
    check("region bounce: new track inside the stack, region muted", nt.parent == Some(sum) && s.project.track(b).unwrap().regions[0].muted && !s.project.track(b).unwrap().mute);

    // Folder stack: its fader is baked into the subtracks, so it is always printed.
    s.set_region_muted(rid, false);
    s.flatten_stack(sum);
    let folder = s.create_stack(&[a, b], false).unwrap();
    s.update_track(folder, TrackPatch { volume_db: Some(-6.0), ..Default::default() });
    let made = s.bounce_in_place(&[folder], &[], &BipOptions { fader: false, source: BipSource::Leave, ..Default::default() }, &mut |_| {}).unwrap();
    let nt = s.project.track(made[0]).unwrap().clone();
    let RegionContent::Audio { clip: bclip, .. } = nt.regions[0].content else { unreachable!() };
    let gain = db(s.clip_peak(bclip).unwrap() / tone_peak);
    check(&format!("folder stack bounces its subtracks' mix, fader printed once ({gain:.2} dB, new track {:.1} dB)", nt.volume_db), (gain - 0.0).abs() < 0.2 && nt.volume_db == 0.0);
    s.delete_track(made[0]);
    s.flatten_stack(folder);

    // Stems ignore solos elsewhere; the full mix honours them.
    s.update_track(b, TrackPatch { solo: Some(true), ..Default::default() });
    let res = s.export_tracks(&out.join("stems-solo"), &RenderOptions { start: 0, end, tail_secs: 0.0, bit_depth: BitDepth::Float32, ..Default::default() }, &mut |_| {}).unwrap();
    let first = read(Path::new(&res.paths[0])).1;
    check(&format!("stem of an un-soloed track still renders ({:.3})", peak(&first)), (peak(&first) - tone_peak).abs() < 1e-3);
    let ps = out.join("solo.wav");
    s.bounce_project(&ps, &RenderOptions { start: 0, end, tail_secs: 0.0, bit_depth: BitDepth::Float32, ..Default::default() }, &mut |_| {}).unwrap();
    let solo_mix = read(&ps).1;
    check(&format!("full mix honours solo: one tone, not two ({:.2} dB)", db(peak(&solo_mix) / tone_peak)), db(peak(&solo_mix) / tone_peak).abs() < 0.1);
    s.update_track(b, TrackPatch { solo: Some(false), ..Default::default() });

    // Bouncing while playing: playback carries on afterwards.
    s.locate(0);
    s.play();
    std::thread::sleep(Duration::from_millis(100));
    s.bounce_project(&out.join("playing.wav"), &RenderOptions { start: 0, end: 48000, tail_secs: 0.0, ..Default::default() }, &mut |_| {}).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    s.tick();
    std::thread::sleep(Duration::from_millis(200));
    let t = s.tick();
    check(&format!("live playback resumes after a bounce (playing={}, out={:.3})", t.playing, t.master.0), t.playing && t.master.0 > 0.01);
    s.stop();
    std::thread::sleep(Duration::from_millis(60));

    // A real VST3 insert renders offline too.
    match comp {
        Some(c) => {
            s.set_plugin(a, 0, &c).unwrap();
            s.update_track(b, TrackPatch { mute: Some(true), ..Default::default() });
            let pv = out.join("vst.wav");
            s.bounce_project(&pv, &RenderOptions { start: 0, end, tail_secs: 0.0, bit_depth: BitDepth::Float32, ..Default::default() }, &mut |_| {}).unwrap();
            let v = read(&pv).1;
            check(&format!("VST3 insert (ZamCompX2) renders offline ({:.3})", peak(&v)), peak(&v) > 0.01 && v[0].len() as i64 == end);
            let made = s.bounce_in_place(&[a], &[], &BipOptions { effects: false, source: BipSource::Leave, ..Default::default() }, &mut |_| {}).unwrap();
            let RegionContent::Audio { clip: bclip, .. } = s.project.track(made[0]).unwrap().regions[0].content else { unreachable!() };
            check("Bounce in Place without Audio FX skips the plug-in (unity peak)", (db(s.clip_peak(bclip).unwrap() / tone_peak)).abs() < 0.1);
        }
        None => println!("SKIP no ZamCompX2 in the plug-in cache"),
    }
    std::process::exit(FAILED.load(std::sync::atomic::Ordering::Relaxed) as i32);
}
