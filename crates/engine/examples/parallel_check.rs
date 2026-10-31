//! Playing tracks on several threads (`dsp/parallel.rs`) must not change one bit of the output.
//! Plays a project with everything the engine has, through offline renders and through
//! `Engine::process` itself, at several buffer sizes and thread counts, and compares every sample
//! with the serial loop's. Edits arrive at fixed blocks during the live runs. Runs on any core count:
//! more threads than cores is slower, not different.
//!
//! The VST3s it adds where installed, all deterministic: LSP's Sidechain Compressor Stereo (a key
//! through a VST3's aux input) and LSP's Multi-Sampler x12 DirectOut (an aux on an extra output).
//! Not for this check: TONE3000 runs its model on threads of its own, so its output depends on
//! timing, and any plug-in with free-running randomness.
//!
//! Then real-time workers, where the system grants them: they must survive blocks coming back to
//! back, past the point RLIMIT_RTTIME would kill a thread that never sleeps.
//! usage: parallel_check <scratch dir>
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use base64::Engine as _;
use daw_engine::audio_io::{AudioConfig, NULL_OUTPUT};
use daw_engine::dsp::pool::PoolOptions;
use daw_engine::dsp::ParallelTest;
use daw_engine::model::{AutoParam, Id, InputSel, Note, TrackKind, TrackSend};
use daw_engine::session::{MidiContent, TrackPatch, BUILTIN_SYNTH};
use daw_engine::Session;

#[path = "common/all_features.rs"]
mod all_features;
use all_features::*;

static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

/// What the live runs change as they go.
struct Extras {
    sampler: Option<Id>,
    world: World,
    aux: Id,
    bus: Id,
    off: Id,
    monitored: Id,
    muted: Id,
    /// The VST3s used, for the report.
    vst: Vec<String>,
}

const PATIENT: ParallelTest = ParallelTest { always: true, renders: true, patient: true };

fn session(scratch: &Path) -> Session {
    let plugins = lsp_plugins();
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some(NULL_OUTPUT.into()), input_enabled: false, ..Default::default() };
    let s = Session::new(cfg, plugins);
    assert_eq!(s.audio_config.output.as_deref(), Some(NULL_OUTPUT), "these checks only ever play to the null device");
    assert!(!s.audio_status.running || s.audio_status.host == "ALSA" || s.audio_status.host == "Null", "not on the null device: {:?}", s.audio_status);
    std::fs::create_dir_all(scratch).unwrap();
    s
}

fn lsp_plugins() -> Vec<vst3_host::PluginDescriptor> {
    static FOUND: std::sync::OnceLock<Vec<vst3_host::PluginDescriptor>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            let bundles = vst3_host::scan::find_bundles(&vst3_host::scan::default_search_paths());
            let lsp = bundles.iter().filter(|b| b.to_string_lossy().contains("lsp-plugins")).flat_map(|b| vst3_host::scan::scan_bundle(b).unwrap_or_default());
            lsp.filter(|d| d.name == "Sidechain Compressor Stereo" || d.name == "Multi-Sampler x12 DirectOut").collect()
        })
        .clone()
}

/// LSP's state is a list of named ports (see `multiout_check`).
fn set_string(state: &mut Vec<u8>, name: &str, v: &str) {
    let mut key = vec![name.len() as u8];
    key.extend_from_slice(name.as_bytes());
    key.push(b's');
    let at = state.windows(key.len()).position(|w| w == key).unwrap_or_else(|| panic!("no port {name}")) + key.len();
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
    state.splice(at..at + 1, value);
}

/// Everything at once: the all-features project (folders, VCAs, automation of every kind, delay
/// compensation through the Linear Phase EQ, a pre-fader key, Stereo Out's inserts and lanes, take
/// folders, patterns) and, on top, a summing stack, auxes fed by outputs and by sends before and
/// after the fader and by each other, a post-fader key, Off, frozen, muted and monitored tracks.
fn build(s: &mut Session, scratch: &Path) -> Extras {
    s.dir = scratch.join("Parallel.fermata");
    let mut w = base(s, scratch);
    for (name, step) in steps() {
        s.checkpoint(name, None, true);
        step(s, &mut w);
    }
    let sum = s.create_stack(&[w.a2], true).unwrap();
    s.set_plugin(sum, 0, "builtin:overdrive").unwrap();
    let aux = s.create_aux(None).unwrap();
    s.set_plugin(aux, 0, "builtin:reverb").unwrap();
    let bus = s.create_aux(None).unwrap();
    s.set_plugin(bus, 0, "builtin:delay").unwrap();
    s.set_plugin(bus, 1, "builtin:compressor").unwrap();
    // Several feeders per bus, and one strip feeding the same bus twice from both sides of its fader.
    let send = |bus: Id, level_db: f32, pre_fader: bool| Some(TrackSend { bus, level_db, pre_fader, muted: false });
    s.set_send(w.inst, 0, send(aux, -6.0, false)).unwrap();
    s.set_send(w.inst, 1, send(aux, -9.0, true)).unwrap();
    s.set_send(w.a2, 0, send(aux, -3.0, true)).unwrap();
    s.set_send(w.a1, 0, send(aux, -12.0, false)).unwrap();
    s.set_send(w.drums, 0, send(bus, -4.0, false)).unwrap();
    s.set_send(w.a1, 1, send(bus, -8.0, true)).unwrap();
    s.set_send(aux, 0, send(bus, -10.0, false)).unwrap();
    s.set_sidechain(bus, 1, Some(w.inst)).unwrap();
    // A send's level follows a lane, too.
    s.set_automation_lane(w.inst, AutoParam::Send(0), vec![pt(0, -20.0, 0.0), pt(4 * BAR, -3.0, 0.2)]);

    let off = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(&w.audio, off, 0).unwrap();
    s.set_plugin(off, 0, "builtin:chorus").unwrap();
    s.set_send(off, 0, send(aux, -6.0, true)).unwrap();
    s.update_track(off, TrackPatch { off: Some(true), ..Default::default() });
    let monitored = s.add_track(TrackKind::Audio, InputSel::Stereo(0), None, None).unwrap();
    s.update_track(monitored, TrackPatch { monitor: Some(true), ..Default::default() });
    s.set_plugin(monitored, 0, "builtin:compressor").unwrap();
    s.set_sidechain(monitored, 0, Some(w.a1)).unwrap();
    let muted = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
    s.import_audio(&w.audio, muted, 0).unwrap();
    s.set_send(muted, 0, send(aux, -6.0, true)).unwrap();
    s.update_track(muted, TrackPatch { mute: Some(true), ..Default::default() });

    // Built-in instruments with drift on: their differences must come from their own seeds.
    let tre = add_tre(s);
    s.set_send(tre, 0, send(aux, -9.0, false)).unwrap();
    add_tre(s);

    // A sampler with loops, an LFO and round robin: its turns and random LFO come from itself.
    let sampler = s.add_track(TrackKind::Instrument, InputSel::None, Some(daw_engine::instrument::sampler::ID.into()), None).unwrap();
    s.sampler_add_files(sampler, &[w.audio.clone(), w.audio.clone()], Some(50)).unwrap();
    {
        use daw_engine::instrument::sampler::{params::param, state::SamplerState};
        let state = s.project.track(sampler).and_then(|t| t.instrument.as_ref()).and_then(|i| i.state.clone());
        let mut zones = SamplerState::parse(state.as_deref()).zones;
        for z in &mut zones {
            z.key_lo = 0;
            z.key_hi = 127;
            z.rr_group = 1;
            z.loop_on = true;
            z.loop_start = 2000;
            z.loop_end = 12000;
            z.xfade = 500;
        }
        zones[1].reverse = true;
        s.sampler_update_zones(sampler, zones).unwrap();
        s.set_builtin_param(sampler, usize::MAX, param::LFO_WAVE, 4.0).unwrap();
        s.set_builtin_param(sampler, usize::MAX, param::LFO_PITCH, 40.0).unwrap();
        s.set_builtin_param(sampler, usize::MAX, param::F_TYPE, 4.0).unwrap();
        s.set_builtin_param(sampler, usize::MAX, param::CUTOFF, 3000.0).unwrap();
        let r = s.create_midi_region(sampler, 0, 2 * BAR).unwrap();
        let notes = (0..12).map(|k| Note { start: k * BAR / 6, length: BAR / 4, pitch: 48 + (k as u8 * 5) % 24, velocity: 110, muted: false }).collect();
        s.set_midi_content(vec![daw_engine::session::MidiContent { region: r, notes, cc: Vec::new(), start: None, length: None }]);
    }

    let frozen = s.add_track(TrackKind::Instrument, InputSel::None, Some(BUILTIN_SYNTH.into()), None).unwrap();
    let r = s.create_midi_region(frozen, 0, 2 * BAR).unwrap();
    let notes = vec![Note { start: 0, length: BAR, pitch: 50, velocity: 100, muted: false }];
    s.set_midi_content(vec![MidiContent { region: r, notes, cc: Vec::new(), start: None, length: None }]);
    s.set_plugin(frozen, 0, "builtin:tremolo").unwrap();
    s.freeze_tracks(&[frozen], &mut |_| {}).unwrap();

    let mut vst = Vec::new();
    let mut sampler = None;
    let find = |n: &str| lsp_plugins().into_iter().find(|d| d.name == n);
    if let Some(d) = find("Sidechain Compressor Stereo") {
        let t = s.add_track(TrackKind::Audio, InputSel::None, None, None).unwrap();
        s.import_audio(&w.audio, t, BAR).unwrap();
        s.set_plugin(t, 0, &d.id).unwrap();
        s.set_sidechain(t, 0, Some(w.a1)).unwrap();
        vst.push(d.name);
    }
    if let Some(d) = find("Multi-Sampler x12 DirectOut") {
        let inst = s.add_track(TrackKind::Instrument, InputSel::None, None, None).unwrap();
        s.set_plugin(inst, usize::MAX, &d.id).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut copy = s.copy_plugin(inst, usize::MAX).unwrap();
        let saved = copy.state.clone().unwrap();
        let (component, controller) = saved.split_once(':').unwrap();
        let mut state = b64.decode(component).unwrap();
        let tone = scratch.join("sampler.wav");
        let x: Vec<f32> = (0..9600).map(|i| 0.5 * (i as f32 * 0.13).sin()).collect();
        daw_engine::decode::write_wav(&tone, RATE, &[x.clone(), x]).unwrap();
        set_string(&mut state, "sf_0_0", tone.to_str().unwrap());
        copy.state = Some(format!("{}:{controller}", b64.encode(&state)));
        s.paste_plugin(&[inst], usize::MAX, &copy).unwrap();
        let r = s.create_midi_region(inst, 0, 2 * BAR).unwrap();
        let notes = (0..8).map(|i| Note { start: i * BAR / 4, length: 6000, pitch: 57, velocity: 127, muted: false }).collect();
        s.set_midi_content(vec![MidiContent { region: r, notes, cc: Vec::new(), start: None, length: None }]);
        s.add_output_aux(inst).unwrap();
        sampler = Some(inst);
        vst.push(d.name);
        // It loads its sample on a thread of its own, which timing mustn't enter into.
        std::thread::sleep(std::time::Duration::from_millis(800));
    }
    s.set_metronome(true);
    s.idle();
    Extras { world: w, aux, bus, off, monitored, muted, vst, sampler }
}

fn same(a: &[Vec<f32>], b: &[Vec<f32>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits()))
}

fn first_diff(a: &[f32], b: &[f32]) -> String {
    match a.iter().zip(b).position(|(p, q)| p.to_bits() != q.to_bits()) {
        Some(i) => format!("sample {i}: {} vs {}", a[i], b[i]),
        None => format!("lengths {} and {}", a.len(), b.len()),
    }
}

fn peak(a: &[f32]) -> f32 {
    a.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Plays `frames` through `Engine::process` a buffer at a time, with the same input and the same
/// edits at the same blocks whatever the threads. Returns the output and how many blocks of it the
/// pool played.
fn play_live(s: &mut Session, e: &Extras, buffer: usize, frames: usize) -> (Vec<f32>, u64, u64) {
    let engine = s.engine_for_checks();
    let mut guard = engine.lock();
    let status = guard.status.clone();
    // The null device's callback played an unknown number of blocks while the project was built,
    // and effects that run free (a chorus's LFO) moved on meanwhile. A render's end leaves them all
    // reset, and the lock keeps the callback out from here on.
    guard.render(0, 1, 0, None, &mut |_| {});
    let before = status.parallel_blocks.load(Ordering::Relaxed);
    s.set_cycle(true, BAR / 2, BAR / 2 + (frames as i64 * 3 / 4));
    s.locate(0);
    s.play();
    let (mut out, mut all) = (vec![0.0f32; buffer * 2], Vec::with_capacity(frames * 2));
    // A stereo input, for the monitored track.
    let mut input = vec![0.0f32; buffer * 2];
    let mut at = 0usize;
    let blocks = frames / buffer;
    for b in 0..blocks {
        for (i, f) in input.chunks_exact_mut(2).enumerate() {
            let t = (at + i) as f32 / RATE as f32;
            f[0] = 0.2 * (t * 700.0).sin();
            f[1] = 0.1 * (t * 1100.0).sin();
        }
        // The same moments in the music, whatever the buffer size, as near as blocks allow.
        let moment = |num: usize, den: usize| b == blocks * num / den;
        if moment(1, 8) {
            s.update_track(e.world.a1, TrackPatch { volume_db: Some(-5.0), ..Default::default() });
            s.update_track(e.muted, TrackPatch { mute: Some(false), ..Default::default() });
        }
        if moment(2, 8) {
            s.update_track(e.off, TrackPatch { off: Some(false), ..Default::default() });
            s.update_track(e.world.a2, TrackPatch { off: Some(true), ..Default::default() });
            s.sync_parallel();
        }
        if moment(3, 8) {
            s.update_track(e.world.a2, TrackPatch { off: Some(false), ..Default::default() });
            s.set_send(e.world.a1, 2, Some(TrackSend { bus: e.aux, level_db: -6.0, pre_fader: true, muted: false })).unwrap();
            s.sync_parallel();
        }
        if moment(4, 8) {
            s.update_track(e.world.inst, TrackPatch { solo: Some(true), ..Default::default() });
            s.sync_parallel();
        }
        if moment(5, 8) {
            s.update_track(e.world.inst, TrackPatch { solo: Some(false), ..Default::default() });
            s.update_track(e.bus, TrackPatch { pan: Some(0.3), ..Default::default() });
            s.update_track(e.monitored, TrackPatch { mute: Some(true), ..Default::default() });
            s.sync_parallel();
        }
        if moment(6, 8) {
            // A route changes: the blocks up to the new graph play serially, inaudibly.
            s.set_send(e.world.a1, 2, None).unwrap();
            s.update_track(e.monitored, TrackPatch { mute: Some(false), ..Default::default() });
            s.sync_parallel();
        }
        guard.process(&input, 2, &mut out, 2);
        all.extend_from_slice(&out);
        at += buffer;
    }
    let pooled = status.parallel_blocks.load(Ordering::Relaxed) - before;
    let errors = status.engine_errors.load(Ordering::Relaxed) as u64;
    drop(guard);
    s.stop();
    (all, pooled, errors)
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    let frames = 2 * BAR as usize;

    // Offline renders.
    let mut s = session(&scratch);
    let e = build(&mut s, &scratch.join("render"));
    println!("     VST3s in this run: {:?} (LSP is deterministic; none would mean only built-in effects)", e.vst);
    println!("     {} tracks, {} frames compensated", s.project.tracks.len(), s.pdc_total());
    check("delay compensation is in play", s.pdc_total() > 0);
    s.set_threads_for_checks(1, ParallelTest::default());
    let serial = s.render(0, 2 * BAR, BAR / 2, None, &mut |_| {}).unwrap();
    let again = s.render(0, 2 * BAR, BAR / 2, None, &mut |_| {}).unwrap();
    check("two serial renders are the same (the baseline is sound)", same(&serial, &again));
    check(&format!("the render has sound (peak {:.3})", peak(&serial[0])), peak(&serial[0]) > 0.01);
    let isolated = |s: &mut Session, id: Id| s.render(0, BAR, 0, Some(daw_engine::dsp::Isolate { tracks: vec![(id, true)], tap: id, fader: true, effects: true, keys: Vec::new() }), &mut |_| {}).unwrap();
    let iso_serial = isolated(&mut s, e.bus);
    let status = s.status.clone();
    for threads in [2u32, 3, 4, 8] {
        s.set_threads_for_checks(threads, PATIENT);
        let before = status.parallel_blocks.load(Ordering::Relaxed);
        let r = s.render(0, 2 * BAR, BAR / 2, None, &mut |_| {}).unwrap();
        let pooled = status.parallel_blocks.load(Ordering::Relaxed) - before;
        check(&format!("a render on {threads} threads is bit-identical ({pooled} blocks on the pool)"), same(&r, &serial) && pooled > 0);
        if !same(&r, &serial) {
            println!("     {}", first_diff(&r[0], &serial[0]));
        }
        let iso = isolated(&mut s, e.bus);
        check(&format!("an isolated render on {threads} threads too"), same(&iso, &iso_serial));
    }
    check("no strip panicked", status.engine_errors.load(Ordering::Relaxed) == 0);
    drop(s);

    // `Engine::process` itself, through fresh sessions, as the state playing leaves behind counts.
    let live = |buffer: usize, threads: u32, name: &str| -> (Vec<f32>, u64, u64) {
        for attempt in 0.. {
            let mut s = session(&scratch);
            let e = build(&mut s, &scratch.join(name));
            // LSP's Multi-Sampler now and then loses its sample for good while loading (see TODO.md),
            // which would read as a difference between two runs: a fresh instance brings it back.
            if let Some(id) = e.sampler {
                let solo = daw_engine::dsp::Isolate { tracks: vec![(id, true)], tap: id, fader: true, effects: true, keys: Vec::new() };
                let heard = s.render(0, BAR / 4, 0, Some(solo), &mut |_| {}).unwrap();
                if peak(&heard[0]) == 0.0 {
                    assert!(attempt < 4, "the Multi-Sampler lost its sample five times running");
                    println!("     the plug-in lost a sample while loading");
                    continue;
                }
            }
            // The Linear Phase EQ designs its filters on a thread of its own, and playing live takes them
            // when they're ready (a render waits for them): the same latency from a compressor's
            // lookahead instead.
            s.remove_plugin(e.world.drums, 0);
            s.set_plugin(e.world.drums, 0, "builtin:compressor").unwrap();
            s.set_builtin_param(e.world.drums, 0, daw_engine::fx::compressor::param::LOOKAHEAD, 5.0).unwrap();
            s.set_threads_for_checks(threads, if threads > 1 { PATIENT } else { ParallelTest::default() });
            return play_live(&mut s, &e, buffer, frames);
        }
        unreachable!()
    };
    for buffer in [64usize, 128, 512, 1500] {
        let (base, pooled, _) = live(buffer, 1, "serial");
        check(&format!("{buffer} frames: the serial loop stays off the pool, with sound (peak {:.3})", peak(&base)), pooled == 0 && peak(&base) > 0.01);
        if buffer == 512 {
            let (twice, _, _) = live(buffer, 1, "serial2");
            check("two live serial runs are the same", twice == base || twice.iter().zip(&base).all(|(a, b)| a.to_bits() == b.to_bits()));
        }
        for threads in [2u32, 3, 8] {
            let (out, pooled, errors) = live(buffer, threads, "pool");
            let blocks = (frames / buffer) as u64;
            let same_bits = out.len() == base.len() && out.iter().zip(&base).all(|(a, b)| a.to_bits() == b.to_bits());
            // Blocks of over MAX_BLOCK frames count a pool run per sub-block.
            check(&format!("{buffer} frames on {threads} threads: bit-identical ({pooled} blocks on the pool, of about {blocks})"), same_bits && pooled + 12 >= blocks && errors == 0);
            if !same_bits {
                println!("     {}", first_diff(&out, &base));
            }
        }
    }

    realtime_workers(&scratch);

    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}

/// Real-time workers, where the system grants it (rtkit): blocks back to back with nothing else
/// letting them sleep. The kernel's 200 ms limit would SIGKILL this process if they never did.
fn realtime_workers(scratch: &Path) {
    let mut s = session(scratch);
    let _e = build(&mut s, &scratch.join("rt"));
    s.set_threads_for_checks(3, ParallelTest { always: true, renders: false, patient: true });
    s.start_workers_for_checks(PoolOptions { threads: 3, realtime: Some((512, RATE)), cpus: Vec::new() });
    s.idle();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let granted = rt_threads();
    let engine = s.engine_for_checks();
    let mut guard = engine.lock();
    s.locate(0);
    s.play();
    let mut out = vec![0.0f32; 128 * 2];
    let started = std::time::Instant::now();
    let mut blocks = 0u64;
    while started.elapsed().as_millis() < 800 {
        guard.process(&[], 0, &mut out, 2);
        blocks += 1;
    }
    let pooled = guard.status.parallel_blocks.load(Ordering::Relaxed);
    drop(guard);
    s.stop();
    if granted == 0 {
        println!("SKIP real-time workers: this system didn't grant real-time priority (they fell back to normal, and played {pooled} of {blocks} blocks)");
    } else {
        check(&format!("{granted} real-time workers lived through 800 ms of blocks back to back ({pooled} of {blocks} blocks on the pool)"), pooled > 0);
    }
}

fn rt_threads() -> usize {
    let Ok(dir) = std::fs::read_dir("/proc/self/task") else { return 0 };
    dir.flatten()
        .filter(|d| std::fs::read_to_string(d.path().join("comm")).is_ok_and(|c| c.starts_with("fermata-dsp")))
        .filter(|d| {
            // The policy is the 41st field of `stat`, after the command in parentheses.
            let stat = std::fs::read_to_string(d.path().join("stat")).unwrap_or_default();
            let rest = stat.rsplit(')').next().unwrap_or("");
            rest.split_whitespace().nth(38).is_some_and(|p| p != "0")
        })
        .count()
}
