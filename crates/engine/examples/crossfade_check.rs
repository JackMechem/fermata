//! Exercises fade curves, fade types and crossfades against a running engine (silent ALSA null
//! output): what each one sounds like in an offline render, that playback, bounce, export,
//! bounce in place, freeze and Join all agree, and that undo, save and load keep them.
//! usage: crossfade_check <audio file> <scratch dir>
//!
//! Gains are measured with synthetic files: a region on a constant (DC) file next to one on a
//! silent file shows one side of a crossfade on its own.
use std::path::{Path, PathBuf};
use std::time::Duration;

use daw_engine::audio_io::AudioConfig;
use daw_engine::fades::{curve, DragMode, FadeInType, FadeOutType};
use daw_engine::model::{Id, InputSel, RegionContent, TrackKind};
use daw_engine::session::{BipOptions, BipSource, BitDepth, RenderOptions};
use daw_engine::Session;

/// Set by any failed check: the example then exits with status 1, so scripts notice.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, std::sync::atomic::Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// Level of the DC files.
const DC: f32 = 0.5;

fn write_wav(path: &Path, sr: u32, frames: usize, f: impl Fn(usize) -> f32) {
    let spec = hound::WavSpec { channels: 1, sample_rate: sr, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..frames {
        w.write_sample(f(i)).unwrap();
    }
    w.finalize().unwrap();
}

fn read_wav(path: &Path) -> Vec<f32> {
    let mut r = hound::WavReader::open(path).unwrap();
    let ch = r.spec().channels as usize;
    r.samples::<f32>().step_by(ch).map(|s| s.unwrap()).collect()
}

/// The largest difference between two signals over their common length.
fn diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

/// Upward zero crossings in `a`.
fn crossings(a: &[f32]) -> usize {
    a.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
}

struct Files {
    dc: PathBuf,
    silence: PathBuf,
    sine: PathBuf,
}

/// Where the two regions meet, and how long the test crossfades are.
struct Layout {
    j: i64,
    len: i64,
}

/// A new audio track with region A on `a` ending at `l.j` and region B on `b` starting there.
/// A starts at 0 at the start of its file and B at frame `l.j` of its, so both have a second
/// of audio to spare beyond the junction.
fn pair(s: &mut Session, l: &Layout, a: &Path, b: &Path) -> (Id, Id, Id) {
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let ra = s.import_audio(a, t, 0).unwrap();
    s.resize_region(ra, 0, l.j);
    let rb = s.import_audio(b, t, 0).unwrap();
    s.resize_region(rb, l.j, 2 * l.j);
    (t, ra, rb)
}

/// The left channel of the full mix over `[0, 3 j)`.
fn render(s: &mut Session, l: &Layout) -> Vec<f32> {
    s.render(0, 3 * l.j, 0, None, &mut |_| {}).unwrap().swap_remove(0)
}

/// Gains of the outgoing and incoming sides of a crossfade of `kind` (curve `c`), measured
/// separately over the whole render.
fn sides(s: &mut Session, f: &Files, l: &Layout, kind: FadeOutType, c: f32) -> (Vec<f32>, Vec<f32>) {
    let mut out = Vec::new();
    for (a, b) in [(&f.dc, &f.silence), (&f.silence, &f.dc)] {
        let (t, ra, _) = pair(s, l, a, b);
        s.crossfade_regions(&[ra], l.len, Some(kind));
        s.set_region_fade_shape(&[ra], None, Some(c), None, None);
        out.push(render(s, l).iter().map(|v| v / DC).collect::<Vec<f32>>());
        s.delete_track(t);
    }
    let b = out.pop().unwrap();
    (out.pop().unwrap(), b)
}

/// The fade's sample at fraction `p` of the crossfade (centred on the junction).
fn at(l: &Layout, p: f64) -> usize {
    (l.j - l.len / 2 + (p * l.len as f64).round() as i64) as usize
}

/// Plays a cycle around the junction for a moment and returns the Stereo Out peak.
fn live_peak(s: &mut Session) -> f32 {
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
    let tone = PathBuf::from(args.next().expect("audio file"));
    let scratch = PathBuf::from(args.next().expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("CrossfadeCheck.fermata");
    let _ = std::fs::remove_dir_all(&s.dir);
    std::fs::create_dir_all(s.dir.join("Audio Files")).unwrap();
    let sr = s.sample_rate() as u32;
    println!("audio: {:?}", s.audio_status);

    // Four-second files: DC, silence and a 1 kHz sine.
    let gen = scratch.join("crossfade-sources");
    std::fs::create_dir_all(&gen).unwrap();
    let n = 4 * sr as usize;
    let f = Files { dc: gen.join("dc.wav"), silence: gen.join("silence.wav"), sine: gen.join("sine.wav") };
    write_wav(&f.dc, sr, n, |_| DC);
    write_wav(&f.silence, sr, n, |_| 0.0);
    write_wav(&f.sine, sr, n, |i| 0.5 * (std::f32::consts::TAU * 1000.0 * i as f32 / sr as f32).sin());
    let l = Layout { j: sr as i64, len: sr as i64 / 10 };
    let p = |x: f64| at(&l, x);

    // ---- Crossfade types ----------------------------------------------------------------------
    let (ga, gb) = sides(&mut s, &f, &l, FadeOutType::X, 0.0);
    let sum = (p(0.0)..=p(1.0)).map(|i| (ga[i] + gb[i] - 1.0).abs()).fold(0.0, f32::max);
    check(&format!("X: the two sides sum to unity across the crossfade (max error {sum:.1e})"), sum < 1e-4);
    check(&format!("X at curve 0 is linear ({:.4} at a quarter, {:.4} in the middle)", gb[p(0.25)], gb[p(0.5)]), (gb[p(0.25)] - 0.25).abs() < 1e-3 && (gb[p(0.5)] - 0.5).abs() < 1e-3);
    check(
        &format!("the crossfade is centred on the junction: A full before it, B full after ({:.3} {:.3} | {:.3} {:.3})", ga[p(0.0) - 10], gb[p(0.0) - 10], ga[p(1.0) + 10], gb[p(1.0) + 10]),
        (ga[p(0.0) - 10] - 1.0).abs() < 1e-5 && gb[p(0.0) - 10] == 0.0 && ga[p(1.0) + 10] == 0.0 && (gb[p(1.0) + 10] - 1.0).abs() < 1e-5,
    );
    let (ga, gb) = sides(&mut s, &f, &l, FadeOutType::X, 0.7);
    let sum = (p(0.0)..=p(1.0)).map(|i| (ga[i] + gb[i] - 1.0).abs()).fold(0.0, f32::max);
    check(&format!("X with a curve still sums to unity (max error {sum:.1e}; B at a quarter {:.3})", gb[p(0.25)]), sum < 1e-4 && (gb[p(0.25)] - 0.25).abs() > 0.02);
    let (ga, gb) = sides(&mut s, &f, &l, FadeOutType::EqP, 0.0);
    let pow = (p(0.0)..=p(1.0)).map(|i| (ga[i] * ga[i] + gb[i] * gb[i] - 1.0).abs()).fold(0.0, f32::max);
    check(&format!("EqP: constant power across the crossfade (max error {pow:.1e}; each side {:.4} in the middle)", gb[p(0.5)]), pow < 1e-3 && (gb[p(0.5)] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
    let (ga, gb) = sides(&mut s, &f, &l, FadeOutType::XS, 0.0);
    let sum = (p(0.0)..=p(1.0)).map(|i| (ga[i] + gb[i] - 1.0).abs()).fold(0.0, f32::max);
    check(&format!("X S: sums to unity, S-shaped ({sum:.1e}; B {:.3} at a quarter, {:.3} in the middle)", gb[p(0.25)], gb[p(0.5)]), sum < 1e-4 && (gb[p(0.25)] - 0.1464).abs() < 2e-3 && (gb[p(0.5)] - 0.5).abs() < 1e-3);

    // Correlated material (DC on both sides): EqP bumps +3 dB, X stays level.
    let (t, ra, _) = pair(&mut s, &l, &f.dc, &f.dc);
    s.crossfade_regions(&[ra], l.len, Some(FadeOutType::EqP));
    let eqp = render(&mut s, &l);
    let peak = eqp.iter().fold(0.0f32, |m, v| m.max(*v));
    check(&format!("EqP over identical material peaks at +3 dB in the middle ({peak:.4})"), (peak - DC * std::f32::consts::SQRT_2).abs() < 1e-3);
    s.set_cycle(true, l.j - l.len, l.j + l.len);
    let live = live_peak(&mut s);
    check(&format!("playback hears the same EqP bump ({live:.4})"), (live - peak).abs() < 2e-3);
    s.crossfade_regions(&[ra], l.len, Some(FadeOutType::X));
    s.set_region_fade_shape(&[ra], None, Some(0.0), None, None);
    let flat = render(&mut s, &l);
    let dev = flat[..(2 * l.j) as usize].iter().map(|v| (v - DC).abs()).fold(0.0, f32::max);
    let live = live_peak(&mut s);
    check(&format!("linear X over identical material is seamless in render ({dev:.1e}) and playback ({live:.4})"), dev < 1e-5 && (live - DC).abs() < 2e-3);
    s.set_cycle(false, 0, 0);
    s.delete_track(t);

    // ---- Where the files run out ----------------------------------------------------------------
    // Neither file has audio to spare: butt fades, with a dip but no click.
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let ra = s.import_audio(&f.dc, t, 0).unwrap();
    s.resize_region(ra, n as i64 - l.j, l.j);
    s.move_region(ra, 0, None);
    let a_end = s.project.track(t).unwrap().regions[0].clone();
    // A now plays the last second of its file, ending at the junction.
    let ok_layout = a_end.start == 0 && matches!(a_end.content, RegionContent::Audio { offset, .. } if offset == n as i64 - l.j);
    let rb = s.import_audio(&f.dc, t, l.j).unwrap();
    s.resize_region(rb, l.j, l.j);
    s.crossfade_regions(&[ra], l.len, Some(FadeOutType::EqP));
    let butt = render(&mut s, &l);
    let step = butt.windows(2).take(2 * l.j as usize - 1).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    let low = butt[l.j as usize - 1].min(butt[l.j as usize]);
    check(&format!("no audio to spare: butt fades meet at the junction without a jump (layout {ok_layout}, dip {low:.4}, largest step {step:.1e})"), ok_layout && low < 0.01 && step < 1e-3);
    let xs = s.crossfades();
    check(&format!("the butt fades are half the length each ({:?} / {:?})", xs[0].fade_out, xs[0].fade_in), xs[0].fade_out == (l.j - l.len / 2, l.len / 2) && xs[0].fade_in == (l.j, l.len / 2));
    // A gap: the crossfade type fades out like Out, and B starts cleanly.
    s.move_region(rb, l.j + 100, None);
    let gap = render(&mut s, &l);
    check(
        &format!("a gap turns the crossfade into a fade-out ({:.4} halfway through it, {:.4} in the gap)", gap[(l.j - l.len / 2) as usize] / DC, gap[l.j as usize + 50]),
        (gap[(l.j - l.len / 2) as usize] / DC - curve(0.5, 0.5)).abs() < 1e-3 && gap[l.j as usize + 50] == 0.0 && (gap[l.j as usize + 101] - DC).abs() < 1e-6,
    );
    s.delete_track(t);

    // ---- Fade curves ------------------------------------------------------------------------------
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let r = s.import_audio(&f.dc, t, 0).unwrap();
    s.resize_region(r, 0, l.j);
    s.set_region_fades(&[r], Some(l.len), Some(l.len));
    let plain = render(&mut s, &l);
    let legacy = (0..l.len).map(|i| (plain[i as usize] - DC * ((i as f32 / l.len as f32) * std::f32::consts::FRAC_PI_2).sin()).abs()).fold(0.0, f32::max);
    check(&format!("the default fade is the quarter sine fades always had ({legacy:.1e})"), legacy < 1e-5);
    let mid_out = (l.j - l.len / 2) as usize;
    for c in [0.0f32, -0.5, 0.9] {
        s.set_region_fade_shape(&[r], Some(c), Some(c), None, None);
        let a = render(&mut s, &l);
        let (gi, go) = (a[(l.len / 2) as usize] / DC, a[mid_out] / DC);
        check(&format!("curve {c}: fade-in and fade-out halfway at {gi:.4} / {go:.4} (want {:.4})", curve(0.5, c)), (gi - curve(0.5, c)).abs() < 1e-3 && (go - curve(0.5, c)).abs() < 1e-3);
    }
    s.delete_track(t);

    // ---- Speed Up and Slow Down ----------------------------------------------------------------
    let t = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let r = s.import_audio(&f.sine, t, 0).unwrap();
    s.resize_region(r, 0, 2 * l.j);
    let half = l.j / 2;
    let normal = render(&mut s, &l);
    s.set_region_fades(&[r], Some(half), Some(half));
    s.set_region_fade_shape(&[r], Some(0.0), Some(0.0), Some(FadeInType::SpeedUp), Some(FadeOutType::SlowDown));
    let speed = render(&mut s, &l);
    let (h, e) = (half as usize, 2 * l.j as usize);
    let (up, down, full) = (crossings(&speed[..h]), crossings(&speed[e - h..e]), crossings(&normal[..h]));
    check(&format!("Speed Up and Slow Down play at half speed on average over a linear fade ({up} and {down} cycles, {full} at full speed)"), (up as f32 / full as f32 - 0.5).abs() < 0.03 && (down as f32 / full as f32 - 0.5).abs() < 0.03);
    let between = diff(&speed[h..e - h], &normal[h..e - h]);
    check(&format!("between the speed fades the audio is where it always was ({between:.1e})"), between < 1e-6);
    let (first, last) = (speed[1..200].iter().fold(0.0f32, |m, v| m.max(v.abs())), speed[e - 200..e].iter().fold(0.0f32, |m, v| m.max(v.abs())));
    check(&format!("speed fades start and end silent ({first:.4}, {last:.4})"), first < 0.01 && last < 0.01);
    s.delete_track(t);

    // ---- Automatic crossfades on edits (X-Fade drag mode) ------------------------------------------
    // (Set up first: in X-Fade mode the butted pair would crossfade over the Crossfade Time.)
    let (t, _, rb) = pair(&mut s, &l, &f.dc, &f.silence);
    s.set_drag_mode(DragMode::XFade);
    s.checkpoint("Move Region", None, false);
    s.move_region(rb, l.j - l.len, None);
    let xs = s.crossfades();
    check(&format!("moving B over A's end crossfades the overlap ({:?})", xs.first().map(|x| (x.kind, x.fade_out))), xs.len() == 1 && xs[0].kind == FadeOutType::X && xs[0].fade_out == (l.j - l.len, l.len));
    let ga: Vec<f32> = render(&mut s, &l).iter().map(|v| v / DC).collect();
    let (mid, end) = (ga[(l.j - l.len / 2) as usize], ga[l.j as usize + 10]);
    check(&format!("A fades out across the overlap with its curve ({mid:.4} in the middle, {end:.4} after)"), (mid - curve(0.5, 0.5)).abs() < 1e-3 && end == 0.0);
    s.undo();
    check("undo puts B back without the crossfade", s.crossfades().is_empty() && s.project.track(t).unwrap().regions.iter().any(|r| r.id == rb && r.start == l.j));
    s.redo();
    check("redo brings both back", s.crossfades().len() == 1);
    s.set_drag_mode(DragMode::Overlap);
    s.delete_track(t);

    // ---- Every render path agrees ----------------------------------------------------------------
    let (t, ra, _) = pair(&mut s, &l, &f.sine, &f.dc);
    s.crossfade_regions(&[ra], l.len, Some(FadeOutType::XS));
    s.set_region_fades(&[ra], Some(l.len), None);
    s.set_region_fade_shape(&[ra], Some(-0.3), None, None, None);
    let mix = render(&mut s, &l);
    let range = RenderOptions { start: 0, end: 3 * l.j, tail_secs: 0.0, bit_depth: BitDepth::Float32, ..Default::default() };
    let out = scratch.join("crossfade-bounces");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    s.bounce_project(&out.join("mix.wav"), &range, &mut |_| {}).unwrap();
    let d = diff(&mix, &read_wav(&out.join("mix.wav")));
    check(&format!("Bounce Project matches the render ({d:.1e})"), d < 1e-6);
    let res = s.export_tracks(&out.join("stems"), &range, &mut |_| {}).unwrap();
    let d = diff(&mix, &read_wav(Path::new(&res.paths[0])));
    check(&format!("Export All Tracks matches ({d:.1e})"), d < 1e-6);
    s.checkpoint("Bounce in Place", None, false);
    let made = s.bounce_in_place(&[t], &[], &BipOptions { effects: true, fader: false, source: BipSource::Mute, tail: false }, &mut |_| {}).unwrap();
    let bip = s.project.track(made[0]).unwrap().regions[0].clone();
    let RegionContent::Audio { clip, .. } = bip.content else { unreachable!() };
    let audio = s.clip_audio(clip).unwrap();
    let d = diff(&mix[bip.start as usize..], &audio.channels[0]);
    check(&format!("Bounce in Place matches ({d:.1e}, {} frames)", audio.channels[0].len()), d < 1e-6 && audio.channels[0].len() >= 2 * l.j as usize);
    s.undo();
    s.checkpoint("Freeze", None, false);
    s.freeze_tracks(&[t], &mut |_| {}).unwrap();
    let d = diff(&mix, &render(&mut s, &l));
    check(&format!("a frozen track plays the same ({d:.1e})"), s.project.track(t).unwrap().frozen.is_some() && d < 1e-6);
    s.undo();
    let d = diff(&mix, &render(&mut s, &l));
    check(&format!("unfrozen again, unchanged ({d:.1e})"), d < 1e-6);
    // Live playback hears the crossfade too: the S-curve between sine and DC never exceeds
    // what the render reaches.
    let peak = mix.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    s.set_cycle(true, l.j - l.len, l.j + l.len);
    let live = live_peak(&mut s);
    let zone = mix[(l.j - l.len) as usize..(l.j + l.len) as usize].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    check(&format!("playback over the crossfade peaks where the render does ({live:.4} vs {zone:.4}; whole {peak:.4})"), (live - zone).abs() < 0.01);
    s.set_cycle(false, 0, 0);

    // Save and load.
    let saved = s.save(None).unwrap();
    let mut s2 = Session::new(AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() }, Vec::new());
    s2.open(&saved).unwrap();
    let d = diff(&mix, &render(&mut s2, &l));
    check(&format!("saved and reopened, it sounds the same ({d:.1e}, {} crossfade)", s2.crossfades().len()), d < 1e-6 && s2.crossfades().len() == 1);
    drop(s2);

    // Join mixes the crossfade into the new file.
    let ids: Vec<Id> = s.project.track(t).unwrap().regions.iter().map(|r| r.id).collect();
    s.checkpoint("Join", None, false);
    s.join_regions(&ids).unwrap();
    let d = diff(&mix, &render(&mut s, &l));
    check(&format!("Join keeps the crossfade ({d:.1e})"), s.project.track(t).unwrap().regions.len() == 1 && d < 1e-6);
    s.undo();
    check("undo restores the two regions and their crossfade", s.crossfades().len() == 1);

    // The supplied file plays through a crossfade with itself too.
    let tt = s.add_track(TrackKind::Audio, InputSel::Mono(0), None, None).unwrap();
    let a = s.import_audio(&tone, tt, 0).unwrap();
    let len = s.project.track(tt).unwrap().regions[0].length;
    let b = s.import_audio(&tone, tt, len / 2).unwrap();
    s.crossfade_regions(&[a], len / 10, None);
    let xs: Vec<_> = s.crossfades().into_iter().filter(|x| x.out_region == a).collect();
    check(&format!("the tone crossfades with a copy of itself as EqP ({:?})", xs.first().map(|x| (x.kind, x.fade_out))), xs.len() == 1 && xs[0].kind == FadeOutType::EqP && xs[0].in_region == b);

    std::process::exit(if FAILED.load(std::sync::atomic::Ordering::Relaxed) { 1 } else { 0 });
}
