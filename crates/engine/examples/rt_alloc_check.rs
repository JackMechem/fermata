//! Real-time safety: plays the all-features project (`common/all_features.rs`: every new
//! built-in effect, VCAs, folder and Stereo Out automation with curves, pattern regions, take
//! folders, region parameters, the Linear Phase EQ) through `Engine::process` on this thread,
//! with a counting allocator, and checks that no block allocates or frees memory once warmed
//! up: while playing, cycling, locating (with note chase), and while edits arrive between
//! blocks, among them a latency change that makes delay compensation glide.
//! The silent ALSA null device runs, so edits queue their commands for the next block as they
//! do during playback; holding the engine's lock keeps its callback out.
//! usage: rt_alloc_check <scratch dir>
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use daw_engine::audio_io::AudioConfig;
use daw_engine::model::{AutoParam, TrackKind};
use daw_engine::session::{RegionParamsPatch, TrackPatch, VcaPatch};
use daw_engine::Session;

#[path = "common/all_features.rs"]
mod all_features;
use all_features::*;

/// Counts the allocations and frees made on a thread while its `COUNTING` flag is set.
struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
/// Backtraces printed so far (the first few allocations are shown).
static SHOWN: AtomicUsize = AtomicUsize::new(0);
const SHOW: usize = 6;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}

fn counted(what: &str) {
    let on = COUNTING.try_with(|c| c.get()).unwrap_or(false);
    if !on {
        return;
    }
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    if SHOWN.fetch_add(1, Ordering::Relaxed) < SHOW {
        // The backtrace allocates too: not counted.
        COUNTING.with(|c| c.set(false));
        eprintln!("{what} inside Engine::process:\n{}", std::backtrace::Backtrace::force_capture());
        COUNTING.with(|c| c.set(true));
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        counted("allocation");
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        counted("free");
        System.dealloc(p, l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        counted("allocation");
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        counted("reallocation");
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn check(name: &str, ok: bool) {
    FAILED.fetch_or(!ok, Ordering::Relaxed);
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}

const BLOCK: usize = 256;

fn lsp_compressor() -> Option<vst3_host::PluginDescriptor> {
    let bundles = vst3_host::scan::find_bundles(&vst3_host::scan::default_search_paths());
    let lsp = bundles.iter().filter(|b| b.to_string_lossy().contains("lsp-plugins")).flat_map(|b| vst3_host::scan::scan_bundle(b).unwrap_or_default());
    lsp.into_iter().find(|d| d.name == "Sidechain Compressor Stereo")
}

/// A VST3's key and its gate around a change of its buses, on the processor directly: the
/// `Cmd::VstGate`s arrive on the audio thread, the `setActive` in between runs off it.
fn vst_gate(d: &vst3_host::PluginDescriptor) {
    let (mut p, mut c) = vst3_host::instantiate_keyed(d, 48000.0, BLOCK, false).unwrap();
    let t = vst3_host::TransportInfo { sample_rate: 48000.0, tempo: 120.0, sig_num: 4, sig_den: 4, ..Default::default() };
    let (mut l, mut r) = (vec![0.1f32; BLOCK], vec![0.1f32; BLOCK]);
    let key = vec![0.5f32; BLOCK];
    let mut run = |p: &mut vst3_host::PluginProcessor, keyed: bool, blocks: usize| {
        for _ in 0..blocks {
            COUNTING.with(|c| c.set(true));
            p.process_keyed(&mut l, &mut r, keyed.then_some((&key[..], &key[..])), &[], &t);
            COUNTING.with(|c| c.set(false));
        }
    };
    run(&mut p, false, 100);
    let mark = ALLOCS.load(Ordering::Relaxed);
    for on in [true, false, true] {
        COUNTING.with(|c| c.set(true));
        p.close_gate();
        COUNTING.with(|c| c.set(false));
        run(&mut p, !on, 4);
        COUNTING.with(|c| c.set(true));
        p.shut_gate();
        COUNTING.with(|c| c.set(false));
        run(&mut p, !on, 1);
        c.set_key_active(on).unwrap();
        COUNTING.with(|c| c.set(true));
        p.open_gate(0);
        COUNTING.with(|c| c.set(false));
        run(&mut p, on, 50);
    }
    let n = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("a VST3's key and its gate allocate nothing ({n})"), n == 0);

    // Edits from its editor while it's bypassed, in a block of its own.
    let id = c.parameters()[0].id;
    let mark = ALLOCS.load(Ordering::Relaxed);
    for i in 0..50 {
        c.edit_param(id, i as f64 / 50.0);
        COUNTING.with(|c| c.set(true));
        p.pass_edits(&t);
        COUNTING.with(|c| c.set(false));
    }
    let n = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("a bypassed VST3 takes its editor's edits without allocating ({n})"), n == 0 && !p.has_edits());
}

fn main() {
    let scratch = PathBuf::from(std::env::args().nth(1).expect("scratch dir"));
    let cfg = AudioConfig { host: Some("ALSA".into()), output: Some("alsa:null".into()), input_enabled: false, ..Default::default() };
    let mut s = Session::new(cfg, Vec::new());
    s.dir = scratch.join("RtAlloc.fermata");
    let mut w = base(&mut s, &scratch);
    for (name, step) in steps() {
        s.checkpoint(name, None, true);
        step(&mut s, &mut w);
    }
    // A summing stack and an aux with a send, so buses run too.
    let sum = s.create_stack(&[w.a2], true).unwrap();
    let aux = s.create_aux(None).unwrap();
    s.set_send(w.inst, 0, Some(daw_engine::model::TrackSend { bus: aux, level_db: -6.0, pre_fader: false, muted: false })).unwrap();
    s.set_plugin(aux, 0, "builtin:reverb").unwrap();
    s.set_plugin(aux, 1, "builtin:delay").unwrap();
    s.idle();
    println!("audio: {:?}", s.audio_status.running);
    // The counter itself works (quietly: no backtrace for this one).
    SHOWN.store(SHOW, Ordering::Relaxed);
    COUNTING.with(|c| c.set(true));
    let v: Vec<u8> = Vec::with_capacity(SHOW);
    COUNTING.with(|c| c.set(false));
    check("the counter sees an allocation", ALLOCS.swap(0, Ordering::Relaxed) == 1 && v.capacity() == SHOW);
    drop(v);
    SHOWN.store(0, Ordering::Relaxed);
    // A real VST3 keyed by another track, where one is installed.
    let lsp = lsp_compressor();
    let vst_key = lsp.as_ref().map(|d| {
        s.set_plugins(vec![d.clone()]);
        let u = s.set_plugin(w.a2, 6, &d.id).unwrap();
        s.set_sidechain(w.a2, 6, Some(w.a1)).unwrap();
        check("the VST3 hears its key", s.vst_key_active(u) == Some(true));
        u
    });
    let latency = s.pdc_total();

    let engine = s.engine_for_checks();
    let mut buf = vec![0.0f32; BLOCK * 2];
    let mut peak = 0.0f32;
    let mut guard = engine.lock();
    // Runs `blocks` blocks, counting what they allocate when `count` is set.
    let mut run = |guard: &mut parking_lot::MutexGuard<daw_engine::dsp::Engine>, blocks: usize, count: bool| {
        for _ in 0..blocks {
            COUNTING.with(|c| c.set(count));
            guard.process(&[], 0, &mut buf, 2);
            COUNTING.with(|c| c.set(false));
            peak = buf.iter().fold(peak, |m, x| m.max(x.abs()));
        }
    };
    // Between blocks the session makes edits; their commands wait for the next block.
    s.locate(0);
    s.play();
    run(&mut guard, 400, false);
    let before = ALLOCS.load(Ordering::Relaxed);
    let bars = |b: f64| (b * BAR as f64 / BLOCK as f64) as usize;

    run(&mut guard, bars(1.0), true);
    let playing = ALLOCS.load(Ordering::Relaxed) - before;
    check(&format!("playing allocates nothing ({playing} allocations or frees)"), playing == 0);

    let mark = ALLOCS.load(Ordering::Relaxed);
    // A latency change: the Linear Phase EQ's quality, then the glide as compensation follows.
    s.set_builtin_param(w.drums, 0, daw_engine::fx::linear_phase_eq::QUALITY, 0.0).unwrap();
    for _ in 0..60 {
        run(&mut guard, 4, true);
        s.idle();
    }
    let changed = s.pdc_total() != latency;
    let glide = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("a latency change and its glide allocate nothing ({glide}; compensation {latency} -> {} frames)", s.pdc_total()), glide == 0 && changed);

    let mark = ALLOCS.load(Ordering::Relaxed);
    // Edits arriving while playing: new automation, VCA level and mute, region parameters,
    // a pattern change, a folder's fader, comps.
    s.set_automation_lane(w.folder, AutoParam::Volume, vec![pt(0, -3.0, 0.2), pt(8 * BAR, -20.0, 0.0)]);
    run(&mut guard, 50, true);
    s.update_vca(w.vca, VcaPatch { level_db: Some(-12.0), ..Default::default() });
    run(&mut guard, 50, true);
    s.update_vca(w.vca, VcaPatch { mute: Some(true), ..Default::default() });
    run(&mut guard, 50, true);
    s.update_vca(w.vca, VcaPatch { mute: Some(false), ..Default::default() });
    s.set_region_params(&[w.midi], &RegionParamsPatch { transpose: Some(-5), ..Default::default() });
    run(&mut guard, 50, true);
    s.update_track(w.folder, TrackPatch { volume_db: Some(-4.0), ..Default::default() });
    s.update_track(sum, TrackPatch { volume_db: Some(-2.0), ..Default::default() });
    run(&mut guard, 50, true);
    // Tracks and a whole stack going off, sleeping and waking while playing, an aux with effects
    // and an instrument among them.
    for id in [w.folder, aux, w.inst] {
        s.update_track(id, TrackPatch { off: Some(true), ..Default::default() });
        run(&mut guard, 50, true);
        s.update_track(id, TrackPatch { off: Some(false), ..Default::default() });
        run(&mut guard, 50, true);
    }
    if let Some(p) = s.project.track(w.drums).unwrap().regions.iter().find(|r| r.id == w.pattern).and_then(|r| r.pattern.clone()) {
        let mut p = p;
        p.swing = 0;
        s.set_pattern(w.pattern, p).unwrap();
    }
    s.set_comp_range(w.take_folder, None, 0, 20_000);
    s.set_builtin_param(w.a1, 0, 0, 0.7).unwrap();
    run(&mut guard, 50, true);
    // Plug-ins and tracks coming and going, and undo and redo, while playing.
    s.set_plugin(aux, 2, "builtin:chorus").unwrap();
    run(&mut guard, 50, true);
    let extra = s.add_track(TrackKind::Instrument, daw_engine::model::InputSel::None, Some(daw_engine::session::BUILTIN_SYNTH.into()), None).unwrap();
    run(&mut guard, 50, true);
    s.delete_track(extra);
    run(&mut guard, 50, true);
    for _ in 0..6 {
        s.undo();
        run(&mut guard, 30, true);
    }
    for _ in 0..6 {
        s.redo();
        run(&mut guard, 30, true);
    }
    for _ in 0..20 {
        run(&mut guard, 4, true);
        s.idle();
    }
    if vst_key.is_some() {
        // Between sources and from before to after the fader the bus stays as it is.
        s.set_sidechain_pre(w.a2, 6, true).unwrap();
        run(&mut guard, 50, true);
        s.set_sidechain(w.a2, 6, Some(w.inst)).unwrap();
        run(&mut guard, 50, true);
        s.set_sidechain(w.a2, 6, Some(w.a1)).unwrap();
        run(&mut guard, 50, true);
    }
    let edits = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("edits while playing allocate nothing inside process ({edits})"), edits == 0);

    let mark = ALLOCS.load(Ordering::Relaxed);
    // Locating into the middle of notes (chase) and cycling over the take folders and patterns.
    s.locate(BAR + BAR / 3);
    run(&mut guard, 100, true);
    s.set_cycle(true, 3 * BAR, 5 * BAR);
    s.locate(4 * BAR + 1000);
    run(&mut guard, bars(3.5), true);
    s.set_cycle(false, 3 * BAR, 5 * BAR);
    s.stop();
    run(&mut guard, 100, true);
    s.play();
    run(&mut guard, bars(4.0), true);
    let moves = ALLOCS.load(Ordering::Relaxed) - mark;
    check(&format!("locating, cycling and stopping allocate nothing ({moves})"), moves == 0);
    check(&format!("it played something (peak {peak:.3})"), peak > 0.01);
    drop(guard);
    s.stop();
    match &lsp {
        Some(d) => vst_gate(d),
        None => println!("SKIP the VST3 side-chain: LSP's Sidechain Compressor isn't installed"),
    }

    if FAILED.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}
