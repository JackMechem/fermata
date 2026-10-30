//! Multi-output instruments, measured with a real plug-in: LSP's Multi-Sampler x12 DirectOut, its
//! first two instruments loaded with a 1 kHz and a 2 kHz sine through the plug-in's state (it has
//! no parameter for a sample's file). A note on each goes out through the main output and that
//! instrument's direct output. Prints SKIP when the plug-in isn't installed.
//! usage: multiout_check <scratch dir>
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{Id, InputSel, InstrumentOutput, Note, TrackKind, TrackSend};
use daw_engine::session::{MidiContent, RenderOptions, TrackPatch};
use daw_engine::Session;
use vst3_host::PluginDescriptor;

const PLUGIN: &str = "Multi-Sampler x12 DirectOut";
const SYNTH: &str = "builtin:synth";
/// When the two instruments play (each has its sample's 0.2 s at 48 kHz in here).
const A: (i64, i64) = (0, 12_000);
const B: (i64, i64) = (24_000, 36_000);
const LEN: i64 = 48_000;
/// The first instrument's default note in the plug-in: octave 4, A, which it counts as 57.
const NOTE_ONE: u8 = 57;

/// Counts what the thread running the engine allocates, as `rt_alloc_check` does.
struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}
fn counted() {
    if COUNTING.try_with(|c| c.get()).unwrap_or(false) {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
    }
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        counted();
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        counted();
        System.dealloc(p, l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        counted();
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        counted();
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

static FAILED: AtomicBool = AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

fn peak(a: &[Vec<f32>], range: (i64, i64)) -> f32 {
    a.iter().flat_map(|c| &c[range.0 as usize..(range.1 as usize).min(c.len())]).fold(0.0, |m, x| m.max(x.abs()))
}

fn largest_step(a: &[f32]) -> f32 {
    a.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

/// The whole mix.
fn mix(s: &mut Session) -> Vec<Vec<f32>> {
    s.render(0, LEN, 0, None, &mut |_| {}).unwrap()
}

/// One strip by itself, through its fader.
fn strip(s: &mut Session, id: Id) -> Vec<Vec<f32>> {
    let iso = s.isolate_track(id, true, true);
    s.render(0, LEN, 0, Some(iso), &mut |_| {}).unwrap()
}

fn find_plugin() -> Option<PluginDescriptor> {
    for b in vst3_host::scan::find_bundles(&vst3_host::scan::default_search_paths()) {
        if b.to_string_lossy().contains("lsp-plugins") {
            if let Some(d) = vst3_host::scan::scan_bundle(&b).ok()?.into_iter().find(|d| d.name == PLUGIN) {
                return Some(d);
            }
        }
    }
    None
}

/// LSP's state is a list of named ports: a length, the name, a type letter, the value. Replaces
/// a port's value, keeping the rest.
fn set_port(state: &mut Vec<u8>, name: &str, kind: u8, old_len: impl Fn(&[u8]) -> usize, value: &[u8]) {
    let mut key = vec![name.len() as u8];
    key.extend_from_slice(name.as_bytes());
    key.push(kind);
    let at = state.windows(key.len()).position(|w| w == key).unwrap_or_else(|| panic!("no port {name} in the plug-in's state")) + key.len();
    let n = old_len(&state[at..]);
    state.splice(at..at + n, value.iter().copied());
}

fn set_float(state: &mut Vec<u8>, name: &str, v: f32) {
    set_port(state, name, b'f', |_| 4, &v.to_le_bytes());
}

/// A string port: its length as a variable-length integer, then the bytes.
fn set_string(state: &mut Vec<u8>, name: &str, v: &str) {
    let mut value = Vec::new();
    let mut n = v.len();
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        value.push(if n > 0 { b | 0x80 } else { b });
        if n == 0 {
            break;
        }
    }
    value.extend_from_slice(v.as_bytes());
    // An empty string is one zero byte.
    set_port(state, name, b's', |_| 1, &value);
}

fn sine(path: &Path, hz: f32, rate: u32) {
    // Shorter than the time between the notes: the sampler plays the whole of it.
    let x: Vec<f32> = (0..rate / 5).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin()).collect();
    daw_engine::decode::write_wav(path, rate, &[x.clone(), x]).unwrap();
}

/// Loads the sines into instruments 1 and 2, the first on its default note and the second two
/// semitones up, without touching the rest.
fn load_samples(s: &mut Session, inst: Id, one: &Path, two: &Path) {
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut copy = s.copy_plugin(inst, usize::MAX).unwrap();
    let saved = copy.state.clone().unwrap();
    let (component, controller) = saved.split_once(':').unwrap();
    let mut state = b64.decode(component).unwrap();
    set_string(&mut state, "sf_0_0", one.to_str().unwrap());
    set_string(&mut state, "sf_1_0", two.to_str().unwrap());
    set_float(&mut state, "note_1", 11.0);
    copy.state = Some(format!("{}:{controller}", b64.encode(&state)));
    s.paste_plugin(&[inst], usize::MAX, &copy).unwrap();
}

/// The plug-in loads samples on a thread of its own, with the audio thread running. Waits until
/// a render has both notes in it.
fn wait_for_samples(s: &mut Session, id: Id, window: (i64, i64)) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if peak(&strip(s, id), window) > 0.01 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn active(s: &Session, inst: Id) -> Vec<u32> {
    s.instrument_outputs().iter().filter(|o| o.track == inst).flat_map(|o| o.outputs.iter()).filter(|b| b.active && b.index > 0).map(|b| b.index).collect()
}

fn set_db(s: &mut Session, id: Id, db: f32) {
    s.update_track(id, TrackPatch { volume_db: Some(db), ..Default::default() });
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    let Some(desc) = find_plugin() else {
        println!("SKIP {PLUGIN} (lsp-plugins) isn't installed: nothing measured");
        return;
    };
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg.clone(), vec![desc.clone()]);
    s.dir = scratch.join("MultiOut.fermata");
    std::fs::create_dir_all(&s.dir).unwrap();
    let rate = s.project.sample_rate;
    let (one, two) = (scratch.join("one.wav"), scratch.join("two.wav"));
    sine(&one, 1000.0, rate);
    sine(&two, 2000.0, rate);

    let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.update_track(inst, TrackPatch { name: Some("Drums".into()), ..Default::default() });
    s.set_plugin(inst, usize::MAX, &desc.id).unwrap();
    let outputs = s.instrument_outputs();
    check(&format!("the plug-in reports its outputs ({} buses, first extra one {:?})", outputs[0].outputs.len(), outputs[0].outputs[1].name), outputs.len() == 1 && outputs[0].outputs.len() == 13 && outputs[0].outputs[1].channels == 2);
    check("no extra bus is active yet", active(&s, inst).is_empty());
    load_samples(&mut s, inst, &one, &two);
    let r = s.create_midi_region(inst, 0, LEN).unwrap();
    let note = |start: i64, pitch: u8| Note { start, length: 6000, pitch, velocity: 127, muted: false };
    s.set_midi_content(vec![MidiContent { region: r, notes: vec![note(A.0, NOTE_ONE), note(B.0, NOTE_ONE + 2)], cc: vec![], start: None, length: None }]);
    check("the sampler loads its samples", wait_for_samples(&mut s, inst, A) && wait_for_samples(&mut s, inst, B));

    let main0 = strip(&mut s, inst);
    let (ma, mb) = (peak(&main0, A), peak(&main0, B));
    println!("     main output: note A {:.1} dB, note B {:.1} dB", db(ma), db(mb));
    check("both notes play on the main output", ma > 0.01 && mb > 0.01);

    // The first output. One undo step, and the second click takes the next one.
    s.checkpoint("New Aux", None, false);
    let a1 = s.add_output_aux(inst).unwrap();
    check(&format!("the aux is named after both ({:?})", s.project.track(a1).unwrap().name), s.project.track(a1).unwrap().name == "Drums Direct out 0");
    check("the aux's input is the instrument's output 1", s.project.track(a1).unwrap().instrument_output == Some(InstrumentOutput { track: inst, bus: 1 }));
    check("bus 1 is active now, and only it", active(&s, inst) == [1]);
    check("the sampler still plays after the bus change", wait_for_samples(&mut s, a1, A));
    let d1 = strip(&mut s, a1);
    println!("     aux on output 1: note A {:.1} dB, note B {:.1} dB", db(peak(&d1, A)), db(peak(&d1, B)));
    check("note A reaches its aux at full level", (db(peak(&d1, A)) - db(ma)).abs() < 0.1);
    check("note B doesn't", peak(&d1, B) < 1e-4);
    let main1 = strip(&mut s, inst);
    check("the main output is as it was", (db(peak(&main1, A)) - db(ma)).abs() < 0.1 && (db(peak(&main1, B)) - db(mb)).abs() < 0.1);
    let off_by = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
    let gap = off_by(&d1[0][..A.1 as usize], &main1[0][..A.1 as usize]);
    println!("     aux minus main over note A, largest sample: {gap:.2e}");
    check("the aux has the same samples as the main output: no extra block of latency", gap < 1e-3);

    let a2 = s.add_output_aux(inst).unwrap();
    check("the next click takes the next output", s.project.track(a2).unwrap().instrument_output == Some(InstrumentOutput { track: inst, bus: 2 }) && active(&s, inst) == [1, 2]);
    wait_for_samples(&mut s, a2, B);
    let d2 = strip(&mut s, a2);
    println!("     aux on output 2: note A {:.1} dB, note B {:.1} dB", db(peak(&d2, A)), db(peak(&d2, B)));
    check("note B reaches the second aux, and note A doesn't", (db(peak(&d2, B)) - db(mb)).abs() < 0.1 && peak(&d2, A) < 1e-4);

    // The aux's own fader, pan and mute, in the whole mix.
    let both = mix(&mut s);
    println!("     mix: note A {:.1} dB (main + aux)", db(peak(&both, A)));
    check("in the mix main and aux add up, in phase", (db(peak(&both, A)) - db(2.0 * ma)).abs() < 0.1);
    set_db(&mut s, a1, -12.0);
    let d = strip(&mut s, a1);
    println!("     aux fader at -12 dB: {:.2} dB", db(peak(&d, A)) - db(ma));
    check("the aux's fader sets the aux's level", (db(peak(&d, A)) - db(ma) + 12.0).abs() < 0.05);
    let m = strip(&mut s, inst);
    check("and not the main output's", (db(peak(&m, A)) - db(ma)).abs() < 0.05);
    let lower = mix(&mut s);
    check("the mix follows it", (peak(&lower, A) - ma * 1.2512).abs() < 0.01);
    set_db(&mut s, a1, 0.0);
    set_db(&mut s, inst, -96.0);
    let d = strip(&mut s, a1);
    check("the instrument's fader doesn't touch its output", (db(peak(&d, A)) - db(ma)).abs() < 0.05);
    set_db(&mut s, inst, 0.0);
    s.update_track(inst, TrackPatch { mute: Some(true), ..Default::default() });
    let d = mix(&mut s);
    check("nor does its mute, the aux plays on", (db(peak(&d, A)) - db(ma)).abs() < 0.05);
    s.update_track(inst, TrackPatch { mute: Some(false), ..Default::default() });
    s.update_track(a1, TrackPatch { solo: Some(true), ..Default::default() });
    let d = mix(&mut s);
    check("a soloed aux plays alone", (db(peak(&d, A)) - db(ma)).abs() < 0.05 && peak(&d, B) < 1e-4);
    s.update_track(a1, TrackPatch { solo: Some(false), ..Default::default() });

    // Delay compensation: a latent effect on the instrument's strip, on the aux, and a send
    // arriving late at the aux. In each the two paths still add up in phase.
    let in_step = |s: &mut Session, what: &str| {
        let d = mix(s);
        let got = peak(&d, A);
        println!("     {what}: total latency {} frames, note A {:.2} dB", s.pdc_total(), db(got));
        check(&format!("{what}: main and aux in step"), s.pdc_total() > 0 && (db(got) - db(2.0 * ma)).abs() < 0.2);
    };
    s.set_plugin(inst, 0, "builtin:linear-phase-eq").unwrap();
    in_step(&mut s, "a latent plug-in on the instrument's strip");
    s.remove_plugin(inst, 0);
    s.set_plugin(a1, 0, "builtin:linear-phase-eq").unwrap();
    in_step(&mut s, "a latent plug-in on the aux");
    s.remove_plugin(a1, 0);
    let late = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.set_plugin(late, 0, "builtin:linear-phase-eq").unwrap();
    s.set_send(late, 0, Some(TrackSend { bus: a1, level_db: 0.0, pre_fader: false, muted: false })).unwrap();
    in_step(&mut s, "a later send into the aux (the output waits on its own line)");
    s.delete_track(late);

    // Track Off.
    let plain_mix = mix(&mut s);
    let off = |s: &mut Session, id: Id, v: bool| s.update_track(id, TrackPatch { off: Some(v), ..Default::default() });
    off(&mut s, inst, true);
    check("an Off instrument leaves its auxes silent", peak(&mix(&mut s), (0, LEN)) == 0.0);
    off(&mut s, inst, false);
    check("back on, the sampler plays", wait_for_samples(&mut s, a1, A));
    off(&mut s, a1, true);
    let d = mix(&mut s);
    check("an Off aux stops being fed; the rest plays on", (db(peak(&d, A)) - db(ma)).abs() < 0.05 && (db(peak(&d, B)) - db(2.0 * mb)).abs() < 0.1);
    off(&mut s, a1, false);
    let back = mix(&mut s);
    check("all back on, the mix is the same", (peak(&back, A) - peak(&plain_mix, A)).abs() < 1e-3);
    // Switched off while it sounds, the aux fades.
    let own = 0.5 * std::f32::consts::TAU * 1000.0 / rate as f32;
    let edited = {
        let iso_off = |s: &mut Session| s.update_track(inst, TrackPatch { off: Some(true), ..Default::default() });
        s.render_with_edit(0, LEN, 3000, &mut |s| iso_off(s)).unwrap()
    };
    let step = largest_step(&edited[0][2800..LEN as usize]);
    println!("     an instrument switched off mid-note: largest step {step:.4} (the 2 x sine's own: {:.4})", 2.0 * own * (2.0 * ma));
    check("switching the instrument off mid-note doesn't click", step < 2.0 * own * (2.0 * ma));
    off(&mut s, inst, false);
    wait_for_samples(&mut s, a1, A);

    // Bypass.
    s.set_bypass(inst, usize::MAX, true);
    check("a bypassed instrument leaves its aux silent", peak(&strip(&mut s, a1), (0, LEN)) == 0.0);
    s.set_bypass(inst, usize::MAX, false);
    wait_for_samples(&mut s, a1, A);

    // Bounce and export include the auxes.
    let path = scratch.join("Bounce.wav");
    let o = RenderOptions { end: LEN, tail_secs: 0.0, bit_depth: daw_engine::session::BitDepth::Float32, ..Default::default() };
    s.bounce_project(&path, &o, &mut |_| {}).unwrap();
    let bounced = daw_engine::decode::decode(&path).unwrap();
    check("a bounce has the main output and the auxes", (peak(&bounced.channels, A) - peak(&plain_mix, A)).abs() < 1e-3);
    let dir = scratch.join("Export");
    let done = s.export_tracks(&dir, &o, &mut |_| {}).unwrap();
    check(&format!("Export All Tracks writes the instrument and both auxes ({} files)", done.paths.len()), done.paths.len() == 3);
    let aux_file = done.paths.iter().find(|p| p.contains("Direct out 0")).map(|p| daw_engine::decode::decode(Path::new(p)).unwrap());
    check("the exported aux has its output", aux_file.is_some_and(|f| (db(peak(&f.channels, A)) - db(ma)).abs() < 0.1 && peak(&f.channels, B) < 1e-4));

    // Refusals.
    let msg = s.render_freeze(&[inst], &mut |_| {}).err().map(|e| e.to_string()).unwrap_or_default();
    println!("     freeze says: {msg}");
    check("Freeze refuses with a clear message", msg.contains("extra outputs"));
    check("Bounce in Place refuses too", s.render_in_place(&[inst], &[], &Default::default(), &mut |_| {}).is_err());

    // Deleting and undoing.
    s.checkpoint("Delete", None, true);
    s.delete_track(a2);
    check("deleting an aux switches its bus off", active(&s, inst) == [1]);
    s.undo();
    check("undo brings the aux and its bus back", s.project.track(a2).is_some() && active(&s, inst) == [1, 2]);
    wait_for_samples(&mut s, a2, B);
    check("and it plays", (db(peak(&strip(&mut s, a2), B)) - db(mb)).abs() < 0.1);
    s.checkpoint("Delete", None, true);
    s.delete_track(inst);
    check("deleting the instrument leaves the aux, without that input", s.project.track(a1).is_some_and(|t| t.instrument_output.is_none()));
    check("the aux is silent, nothing breaks", peak(&mix(&mut s), (0, LEN)) == 0.0);
    s.undo();
    check("undo rewires it and activates its buses on the new instance", s.project.track(a1).is_some_and(|t| t.instrument_output.is_some()) && active(&s, inst) == [1, 2]);
    check("and it plays again", wait_for_samples(&mut s, a1, A) && (db(peak(&strip(&mut s, a1), A)) - db(ma)).abs() < 0.1);

    // Another instrument in its slot, with one output.
    s.checkpoint("Instrument", None, true);
    s.set_plugin(inst, usize::MAX, SYNTH).unwrap();
    check("with a one-output instrument the aux has nothing, and the UI is told", s.instrument_outputs().is_empty() && peak(&strip(&mut s, a1), (0, LEN)) == 0.0);
    let d = strip(&mut s, inst);
    check("the new instrument plays on the main output", peak(&d, A) > 0.01);
    s.undo();
    check("undo brings the sampler back", wait_for_samples(&mut s, a1, A) && active(&s, inst) == [1, 2]);

    // Channel strip settings: the input isn't part of a strip's setting.
    let setting = s.copy_strip(a2).unwrap();
    s.checkpoint("Paste", None, true);
    s.paste_strip(&[a1], &setting).unwrap();
    check("a strip setting pasted on the aux leaves its input", s.project.track(a1).unwrap().instrument_output == Some(InstrumentOutput { track: inst, bus: 1 }));
    s.undo();
    let setting = s.copy_strip(inst).unwrap();
    let copy = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
    s.checkpoint("Paste", None, true);
    s.paste_strip(&[copy], &setting).unwrap();
    check("pasted on another instrument, its auxes stay with the first", active(&s, inst) == [1, 2] && active(&s, copy).is_empty());
    s.delete_track(copy);

    // Duplicates.
    let dup = s.duplicate_tracks(&[inst], true)[0];
    check("a copy of the instrument starts with no auxes", s.project.feeds(dup).is_empty() && active(&s, dup).is_empty());
    s.delete_track(dup);

    // Saving and reopening, and project alternatives.
    let saved = s.save(None).unwrap();
    let reference = strip(&mut s, a1);
    let mut again = Session::new(cfg.clone(), vec![desc.clone()]);
    again.open(&saved).unwrap();
    check("reopened, the buses are active", active(&again, inst) == [1, 2]);
    check("and the aux plays", wait_for_samples(&mut again, a1, A) && (db(peak(&strip(&mut again, a1), A)) - db(peak(&reference, A))).abs() < 0.1);
    s.new_alternative("Second").unwrap();
    check("a project alternative keeps it", s.project.track(a1).unwrap().instrument_output.is_some() && active(&s, inst) == [1, 2]);
    drop(again);
    // Missing plug-in.
    let mut bare = Session::new(cfg, Vec::new());
    bare.open(&saved).unwrap();
    check("with the plug-in missing, the project opens", bare.project.track(a1).is_some_and(|t| t.instrument_output.is_some()) && bare.instrument_outputs().is_empty());
    check("and the aux is silent", peak(&strip(&mut bare, a1), (0, LEN)) == 0.0);
    drop(bare);

    // Live: adding and removing an aux while the engine plays, on its own thread, the way a
    // device callback does. A steady sine on another track goes on through it; the sampler's
    // own sound fades. Nothing clicks and the engine's thread allocates nothing.
    let tone_path = scratch.join("steady.wav");
    let steady: Vec<f32> = (0..rate * 10).map(|i| 0.2 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin()).collect();
    daw_engine::decode::write_wav(&tone_path, rate, &[steady.clone(), steady]).unwrap();
    let tone = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(&tone_path, tone, 0).unwrap();
    // Sampler notes through the whole of it, one after another.
    s.delete_track(a2);
    let r2 = s.create_midi_region(inst, LEN, 9 * LEN).unwrap();
    let notes: Vec<Note> = (0..45).map(|k| Note { start: k * 9600, length: 4800, pitch: NOTE_ONE, velocity: 127, muted: false }).collect();
    s.set_midi_content(vec![MidiContent { region: r2, notes, cc: vec![], start: None, length: None }]);
    s.idle();
    s.locate(0);
    s.play();
    let engine = s.engine_for_checks();
    let stop = Arc::new(AtomicBool::new(false));
    let worker = {
        let (engine, stop) = (engine.clone(), stop.clone());
        std::thread::spawn(move || {
            const BLOCK: usize = 256;
            let mut guard = engine.lock();
            let mut buf = vec![0.0f32; BLOCK * 2];
            let mut out = Vec::with_capacity(48000 * 20);
            let started = Instant::now();
            let mut warm = false;
            while !stop.load(Ordering::Relaxed) && out.len() + BLOCK < out.capacity() {
                warm |= started.elapsed() > Duration::from_millis(50);
                COUNTING.with(|c| c.set(warm));
                guard.process(&[], 0, &mut buf, 2);
                COUNTING.with(|c| c.set(false));
                out.extend(buf.chunks_exact(2).map(|f| f[0]));
                // About real time, so a block is under way when the session changes something.
                std::thread::sleep(Duration::from_micros(800));
            }
            out
        })
    };
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    let a3 = s.add_output_aux(inst).unwrap();
    let took = t.elapsed();
    check("live: the aux takes the next free output", s.project.track(a3).unwrap().instrument_output == Some(InstrumentOutput { track: inst, bus: 2 }));
    std::thread::sleep(Duration::from_millis(300));
    s.delete_track(a3);
    std::thread::sleep(Duration::from_millis(300));
    stop.store(true, Ordering::Relaxed);
    let out = worker.join().unwrap();
    s.stop();
    let step = largest_step(&out[512..]);
    // The sine's largest step, plus a note's (which starts at a sample's first peak or so).
    let tone_step = 0.2 * std::f32::consts::TAU * 440.0 / rate as f32;
    let note_step = (ma * 2.0) * std::f32::consts::TAU * 1000.0 / rate as f32;
    println!("     live: the switch took {took:?}; largest step in {} frames {step:.4} (sine {tone_step:.4}, sine + note {:.4})", out.len(), tone_step + note_step);
    check("live: no click when a bus switches on and off", step < 1.5 * (tone_step + note_step));
    let quietest = out[512..].chunks(480).map(|c| c.iter().fold(0.0f32, |m, x| m.max(x.abs()))).fold(1.0f32, f32::min);
    check(&format!("live: the other track plays through it (quietest 10 ms has peak {quietest:.3})"), quietest > 0.15);
    let allocs = ALLOCS.load(Ordering::Relaxed);
    check(&format!("live: the engine's thread allocates nothing ({allocs} allocations or frees)"), allocs == 0);

    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}
