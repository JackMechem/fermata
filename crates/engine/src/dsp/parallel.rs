//! Strips playing at the same time: what makes `Engine::process_block`'s strip loop safe to run on
//! several threads, with output bit-identical to the serial loop. See "Multi-core processing" in
//! docs/ARCHITECTURE.md for the design and the list of what a block touches.
//!
//! The serial loop lets a strip write straight into the strips after it (their `sum_l`/`sum_r`),
//! and into the Stereo Out bus. Floats summed in another order give other bits, so here nothing
//! writes across strips. A strip leaves what it plays with its own [`Outlets`], and a strip that
//! receives ([`Deferred::inputs`]) adds up its feeders' outlets, in the graph's order, which is the
//! order the serial loop's writes land in. Stereo Out's bus is summed the same way by the calling
//! thread once the strips are done (`Engine::process_block`). `run_strip` is the same code for both;
//! only its `Routes` differ.
//!
//! The rest of a block stays on the calling thread, before or after the strips: commands, garbage,
//! live MIDI and recorded MIDI's ring, folder automation (read in `Parallel::run`), the analyzer's
//! ring (the strip keeps a copy for it), Stereo Out and the click.

use std::cell::UnsafeCell;
use std::time::Duration;

use super::graph::{TaskGraph, MAX_TASKS};
use super::pool::{Job, Pool, PoolOptions};
use super::*;

/// Below this share of the buffer the callback plays alone: waking threads costs more than a
/// project this light can gain, and a sleeping worker can't add to the worst block.
const ENGAGE_LOAD: f32 = 0.15;
/// The least the callback waits for a block's last strips before it gives the block up. Only a stuck
/// plug-in or a worker kept off the CPU gets near it. The callback naps through most of this, so the
/// kernel's 200 ms real-time limit isn't at stake for it.
const GIVE_UP_SECS: f64 = 0.1;

/// How a block in the pool ended.
pub(super) enum Outcome {
    Done,
    /// This many strips panicked, and the others ran. The block is silenced.
    Panicked(u32),
    /// Some strip still isn't done (`GIVE_UP_SECS`). The block stays open.
    Stalled,
}

/// One block's work, as the pool's threads see it.
struct StripJob {
    /// Lives as long as the block is open: see `Parallel::run`.
    cx: BlockCtx<'static>,
    tracks: *const *mut TrackDsp,
    scratch: *mut Scratch,
    graph: *const TaskGraph,
    /// The track each thread is in (0: none).
    busy: *const AtomicU64,
}

// SAFETY: the pointers are what `run` says they are, and `BlockCtx` is Sync by itself.
unsafe impl Sync for StripJob {}

impl Job for StripJob {
    fn run(&self, task: usize, thread: usize) {
        // SAFETY: the pool gives each task to one thread and each thread runs one task at a time,
        // so this is the only reference to this strip and to this thread's scratch. No other task
        // looks at this strip until it's over: only tasks that wait for it do (`Deferred::earlier`).
        let (t, sc) = unsafe { (&mut **self.tracks.add(task), &mut *self.scratch.add(thread)) };
        // SAFETY: valid and unchanging while the block is open.
        let graph = unsafe { &*self.graph };
        // SAFETY: one per thread, and it lives as long as the pool.
        let busy = unsafe { &*self.busy.add(thread) };
        busy.store(t.id, Ordering::Relaxed);
        let at = std::time::Instant::now();
        t.outs.reset();
        let mut routes = Deferred { tracks: self.tracks, graph, ti: task, n: self.cx.n };
        run_strip(&self.cx, task, t, sc, &mut routes);
        t.spent = at.elapsed().as_secs_f32();
        busy.store(0, Ordering::Relaxed);
    }
}

/// The threads and what they use, built off the audio thread and sent in a `Cmd::Workers`.
pub struct Workers {
    pool: Pool,
    /// One per thread.
    scratch: Vec<Scratch>,
    busy: Vec<AtomicU64>,
    /// Stays in place while a block is open, whatever happens to the engine's own fields.
    job: Box<UnsafeCell<Option<StripJob>>>,
    /// The strips of the open block.
    tracks: Vec<*mut TrackDsp>,
}

// SAFETY: the raw pointers only point into what the engine owns, and are only used while a block is
// open, under the rules in `StripJob::run`.
unsafe impl Send for Workers {}

impl Workers {
    pub fn new(opts: PoolOptions) -> Box<Workers> {
        let pool = Pool::new(opts);
        let scratch = (0..pool.threads()).map(|_| Scratch::default()).collect();
        let busy = (0..pool.threads()).map(|_| AtomicU64::new(0)).collect();
        Box::new(Workers { pool, scratch, busy, job: Box::new(UnsafeCell::new(None)), tracks: vec![std::ptr::null_mut(); MAX_TASKS] })
    }

    pub fn threads(&self) -> usize {
        self.pool.threads()
    }

    pub fn realtime_workers(&self) -> u32 {
        self.pool.realtime_workers()
    }
}

/// Checks route the blocks through the pool whatever the load, and renders too, which otherwise
/// always play serially: their blocks come back to back, and real-time workers that never sleep
/// would be killed by the kernel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParallelTest {
    pub always: bool,
    pub renders: bool,
    /// Never give a block up, on a machine too loaded to promise anything about time.
    pub patient: bool,
}

#[derive(Default)]
pub(super) struct Parallel {
    workers: Option<Box<Workers>>,
    graph: Option<Box<TaskGraph>>,
    /// The graph holds for the engine's tracks as they are now (see [`covers`]).
    covered: bool,
    /// The routes or the graph changed, so that needs looking at again.
    stale: bool,
    /// A block was given up on and is still open: until it's over nothing may touch the strips.
    pub stalled: bool,
    /// The load is high enough to be worth the workers.
    engaged: bool,
    pub test: ParallelTest,
    /// Threads in the last block, for the meter's tooltip.
    last_threads: u32,
}

impl Parallel {
    pub fn routes_changed(&mut self) {
        self.stale = true;
    }

    pub fn set_graph(&mut self, g: Option<Box<TaskGraph>>) -> Option<Box<TaskGraph>> {
        self.stale = true;
        std::mem::replace(&mut self.graph, g)
    }

    pub fn set_workers(&mut self, w: Option<Box<Workers>>) -> Option<Box<Workers>> {
        std::mem::replace(&mut self.workers, w)
    }

    /// Whether this block goes through the pool.
    pub fn wanted(&mut self, offline: bool, tracks: &[Box<TrackDsp>]) -> bool {
        if self.workers.is_none() {
            return false;
        }
        let Some(g) = &self.graph else { return false };
        if self.stale {
            self.stale = false;
            self.covered = covers(g, tracks);
        }
        if !self.covered {
            return false;
        }
        if offline {
            return self.test.renders;
        }
        self.test.always || (self.engaged && g.heavy_width() >= 2)
    }

    pub fn fits(&self, tracks: &[Box<TrackDsp>]) -> bool {
        self.graph.as_ref().is_some_and(|g| covers(g, tracks))
    }

    /// Takes in a callback's load: `now` is its own, `average` the smoothed one. It goes back to
    /// playing alone once the load, with the workers helping, says that even alone it would stay
    /// under `ENGAGE_LOAD`: `threads` threads can't have done more than that many times the work.
    pub fn weigh(&mut self, now: f32, average: f32) {
        if self.engaged {
            let threads = self.workers.as_ref().map_or(1, |w| w.threads()) as f32;
            self.engaged = average * threads >= ENGAGE_LOAD;
        } else {
            self.engaged = now >= ENGAGE_LOAD;
        }
    }

    /// Threads the last callback used, when it changed: the callback alone unless `pooled`.
    pub fn threads_now(&mut self, pooled: bool) -> Option<u32> {
        let n = if pooled { self.workers.as_ref().map_or(1, |w| w.threads() as u32) } else { 1 };
        (n != self.last_threads).then(|| {
            self.last_threads = n;
            n
        })
    }

    /// A track a thread is in right now, after a block was given up on.
    pub fn stuck_track(&self) -> Option<Id> {
        let w = self.workers.as_ref()?;
        w.busy.iter().map(|b| b.load(Ordering::Relaxed)).find(|&id| id != 0)
    }

    /// Whether a block given up on is over.
    pub fn open_block_over(&self) -> bool {
        self.workers.as_ref().is_none_or(|w| w.pool.is_done())
    }

    /// Runs a block's strips on the pool, this thread working too.
    ///
    /// `wanted` said yes, so there are workers and a graph that matches `tracks`. If this returns
    /// `Stalled` the block stays open with threads still using `tracks` and what `cx` points at:
    /// until `open_block_over`, the caller must not touch any of it, and sets `stalled` to say so.
    pub fn run(&mut self, tracks: &mut [Box<TrackDsp>], cx: &BlockCtx<'_>) -> Outcome {
        let w = self.workers.as_deref_mut().expect("wanted() saw workers");
        let graph = self.graph.as_deref().expect("wanted() saw a graph");
        // Folders' automation is read now, while nothing is writing: a subtrack doesn't wait for
        // its folder, which runs (doing next to nothing) at the same time.
        for ti in 0..tracks.len() {
            if let Some(j) = tracks[ti].fold_idx.filter(|&j| j > ti) {
                let at = cx.pos - tracks[ti].pdc.output_time as i64;
                tracks[ti].fold_now = auto::fold_at(&tracks[ti + 1..], ti + 1, j, at, cx.playing);
            }
        }
        for (slot, t) in w.tracks.iter_mut().zip(tracks.iter_mut()) {
            *slot = &mut **t;
        }
        // SAFETY: only the lifetime changes. What `cx` borrows (the input buffers and live MIDI in
        // their own heap blocks, the tempo map and status behind their `Arc`s, the isolated render's
        // boxed settings) stays where it is and unchanged while the block is open, because the
        // caller applies no commands until `open_block_over`, and the job doesn't outlive that.
        let cx: BlockCtx<'static> = unsafe { std::mem::transmute(*cx) };
        let job = StripJob { cx, tracks: w.tracks.as_ptr(), scratch: w.scratch.as_mut_ptr(), graph, busy: w.busy.as_ptr() };
        // SAFETY: the last block is over (the caller checks `stalled`), so nobody reads the cell; and
        // the job, the graph and the strips stay put until the block is, as above.
        unsafe {
            *w.job.get() = Some(job);
            let ptr: *const StripJob = (*w.job.get()).as_ref().expect("just stored");
            w.pool.start(graph, ptr);
        }
        let limit = (!cx.wait).then(|| Duration::from_secs_f64(GIVE_UP_SECS.max(4.0 * cx.n as f64 / cx.sr)));
        if !w.pool.finish(limit) {
            return Outcome::Stalled;
        }
        match w.pool.take_panics() {
            0 => Outcome::Done,
            k => Outcome::Panicked(k),
        }
    }
}

/// Whether `g` allows for every way the engine's strips read each other, as `resolve_routes` left
/// them. A strip the graph doesn't make wait for its source would read it while it's being written.
pub(super) fn covers(g: &TaskGraph, tracks: &[Box<TrackDsp>]) -> bool {
    if g.len() != tracks.len() {
        return false;
    }
    tracks.iter().enumerate().all(|(i, t)| {
        g.id(i) == t.id
            && t.out_idx.is_none_or(|j| g.has_dep(j, i))
            && t.send_idx.iter().flatten().all(|&j| g.has_dep(j, i))
            && t.feed.source.is_none_or(|s| g.has_dep(i, s))
            && t.inserts.iter().flatten().filter_map(|ins| ins.key.idx).all(|s| g.has_dep(i, s))
    })
}

/// A strip's routes on the pool: it writes nothing of anyone else's.
struct Deferred<'a> {
    tracks: *const *mut TrackDsp,
    graph: &'a TaskGraph,
    ti: usize,
    n: usize,
}

impl Deferred<'_> {
    /// A strip this one waits for, which is finished and won't change while the block is open.
    fn dep(&self, idx: usize) -> &TrackDsp {
        // SAFETY: only called for this task's dependencies, whose tasks finished before this one
        // started (the pool's Acquire on `pending` and the bits sees all they wrote). Nobody holds a
        // `&mut` to them any more, and any number of receivers may read them at once.
        unsafe { &**self.tracks.add(idx) }
    }
}

impl Routes for Deferred<'_> {
    fn earlier(&self, idx: usize) -> Option<&TrackDsp> {
        // `covers` checked this, but reading a strip that may be running would be undefined
        // behaviour where a missed key is merely silence, so it's checked every time.
        self.graph.has_dep(self.ti, idx).then(|| self.dep(idx))
    }

    fn id_of(&self, j: usize) -> Id {
        self.graph.id(j)
    }

    fn fold(&self, read_ahead: auto::FoldState, _j: usize, _at: i64, _playing: bool) -> auto::FoldState {
        read_ahead
    }

    fn inputs(&mut self, _sum: (&mut [f32], &mut [f32]), l: &mut [f32], r: &mut [f32]) {
        let add = |dst: &mut [f32], src: &[f32]| {
            for (d, s) in dst.iter_mut().zip(src) {
                *d += *s;
            }
        };
        // In the order the serial loop's strips write: by processing order, and within a strip the
        // pre-fader sends, then the output, then the post-fader sends.
        for &src in self.graph.deps(self.ti) {
            let s = self.dep(src as usize);
            let sends = |pre: bool, l: &mut [f32], r: &mut [f32]| {
                for k in 0..SEND_SLOTS {
                    if s.outs.sends[k] == Some(SendOut { slot: k, to: self.ti, pre }) {
                        let (a, b) = s.outs.send_buf(k, self.n);
                        add(l, a);
                        add(r, b);
                    }
                }
            };
            sends(true, l, r);
            if s.outs.dest == Dest::Track(self.ti) {
                add(l, &s.l[..self.n]);
                add(r, &s.r[..self.n]);
            }
            sends(false, l, r);
        }
    }

    fn send(&mut self, outs: &mut Outlets, send: SendOut, from: (&[f32], &[f32]), gain: &mut f32, target: f32, smooth: f32) {
        let (a, b) = outs.send_buf_mut(send.slot, from.0.len());
        let live = render_send((a, b), from, gain, target, smooth);
        outs.sends[send.slot] = live.then_some(send);
    }

    fn output(&mut self, outs: &mut Outlets, dest: Dest, _l: &[f32], _r: &[f32]) {
        // It stays in the strip's own buffers.
        outs.dest = dest;
    }

    fn analyze(&mut self, outs: &mut Outlets, signal: Option<(&[f32], &[f32])>, n: usize) {
        outs.analyzed = true;
        match signal {
            Some((l, r)) => {
                for i in 0..n {
                    outs.analysis[i] = 0.5 * (l[i] + r[i]);
                }
            }
            None => outs.analysis[..n].fill(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{Effect, EffectInfo, FxContext, Key};

    const SR: f64 = 48000.0;
    const BLOCK: usize = 256;

    struct Rig {
        engine: Engine,
        tx: rtrb::Producer<Cmd>,
        status: Arc<EngineStatus>,
        _keep: (rtrb::Consumer<Garbage>, rtrb::Producer<[u8; 3]>, rtrb::Consumer<(i64, [u8; 3])>, rtrb::Consumer<f32>),
    }

    fn rig() -> Rig {
        let (tx, rx) = rtrb::RingBuffer::new(256);
        let (gtx, grx) = rtrb::RingBuffer::new(256);
        let (live_tx, live_rx) = rtrb::RingBuffer::new(16);
        let (rec_tx, rec_rx) = rtrb::RingBuffer::new(16);
        let (an_tx, an_rx) = rtrb::RingBuffer::new(1 << 16);
        let status = Arc::new(EngineStatus::default());
        let engine = Engine::new(SR, rx, gtx, status.clone(), live_rx, rec_tx, an_tx);
        Rig { engine, tx, status, _keep: (grx, live_tx, rec_rx, an_rx) }
    }

    fn params(output: Option<Id>, sends: &[(Id, f32, bool)]) -> TrackParams {
        let mut s = [None; SEND_SLOTS];
        for (k, &(bus, gain, pre_fader)) in sends.iter().enumerate() {
            s[k] = Some(SendParams { bus, gain, pre_fader });
        }
        TrackParams {
            volume: 0.7,
            pan: 0.25,
            mute: false,
            solo: false,
            armed: false,
            monitor: false,
            input: InputSel::None,
            output,
            sends: s,
            stack_gain: 1.0,
            stack_mute: false,
            folder: None,
            own_gain: 0.7,
            own_mute: false,
            bus_mute: false,
            vca_gain: 1.0,
            vca_mute: false,
            off: false,
            instrument_output: None,
        }
    }

    /// An effect that makes its own signal (a sine at a pitch of its own) on top of what comes in,
    /// adds the key into it, and takes a little time, so that strips overlap.
    struct Tone {
        phase: f32,
        hz: f32,
    }

    impl Effect for Tone {
        fn set_param(&mut self, _: u32, _: f32) {}
        fn param(&self, _: u32) -> f32 {
            0.0
        }
        fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key, _: &FxContext) {
            for i in 0..l.len() {
                self.phase = (self.phase + self.hz / SR as f32) % 1.0;
                let s = (self.phase * std::f32::consts::TAU).sin() * 0.1;
                let k = key.map_or(0.0, |(a, b)| 0.25 * (a[i] + b[i]));
                l[i] += s + k;
                r[i] += s * 0.5 - k;
            }
        }
        fn reset(&mut self) {}
    }

    fn tone(uid: Id, hz: f32, key: Option<Id>, pre: bool) -> InsertDsp {
        // `EffectInfo::new` is a plain function, so the pitch comes through the uid's slot below.
        let info = EffectInfo { id: "test:tone", name: "Tone", category: "Utility", params: &[], meters: &[], sidechain: true, new: |_| Box::new(Tone { phase: 0.0, hz: 0.0 }) };
        let mut b = BuiltinDsp::new(&info, SR as f32, &[]);
        b.replace_effect_for_tests(Box::new(Tone { phase: uid as f32 * 0.01, hz }));
        InsertDsp { uid, bypass: false, plugin: Plugin::Builtin(Box::new(b)), key: KeyRoute::new(key, pre) }
    }

    fn strip(id: Id, kind: TrackKind, p: TrackParams, fx: Vec<InsertDsp>) -> Box<TrackDsp> {
        let mut t = Box::new(TrackDsp::new(id, kind, p, Arc::new(Meter::default())));
        for (k, f) in fx.into_iter().enumerate() {
            t.set_insert(k, Some(f));
        }
        t
    }

    /// Sends, an output into an aux, a summing stack, keys taken before and after the fader, a
    /// send with a gain changing under it, and several feeders per bus: order matters everywhere.
    fn project(e: &mut Rig) -> TaskGraph {
        let keyed = strip(4, TrackKind::Audio, params(None, &[(6, 0.3, false)]), vec![tone(40, 310.0, Some(1), false), tone(41, 390.0, Some(2), true)]);
        let tracks = vec![
            strip(1, TrackKind::Audio, params(Some(5), &[(6, 0.5, true), (7, 0.25, false), (6, 0.125, false)]), vec![tone(10, 110.0, None, false)]),
            strip(2, TrackKind::Audio, params(Some(5), &[(6, 0.4, false)]), vec![tone(20, 170.0, None, false)]),
            strip(3, TrackKind::Audio, params(None, &[(7, 0.6, true)]), vec![tone(30, 230.0, None, false)]),
            keyed,
            strip(5, TrackKind::Summing, params(None, &[(7, 0.2, false)]), vec![tone(50, 470.0, None, false)]),
            strip(6, TrackKind::Aux, params(None, &[]), vec![tone(60, 530.0, None, false)]),
            strip(7, TrackKind::Aux, params(None, &[]), vec![tone(70, 610.0, None, false)]),
        ];
        let ids = tracks.iter().map(|t| t.id).collect();
        e.engine.replace_graph(SR, tracks, (0..INSERT_SLOTS).map(|_| None).collect());
        let edges = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4)];
        TaskGraph::new(ids, &edges, |_| true).unwrap()
    }

    fn play(e: &mut Rig, blocks: usize) -> Vec<f32> {
        e.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
        let mut all = Vec::new();
        let mut out = vec![0.0f32; BLOCK * 2];
        for _ in 0..blocks {
            e.engine.process(&[], 0, &mut out, 2);
            all.extend_from_slice(&out);
        }
        all
    }

    fn bits(v: &[f32]) -> Vec<u32> {
        v.iter().map(|s| s.to_bits()).collect()
    }

    fn pooled(threads: usize, blocks: usize) -> (Vec<f32>, u64) {
        let mut e = rig();
        let g = project(&mut e);
        e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        let out = play(&mut e, blocks);
        (out, e.status.parallel_blocks.load(Ordering::Relaxed))
    }

    #[test]
    fn the_pool_plays_what_the_serial_loop_plays_bit_for_bit() {
        let mut e = rig();
        project(&mut e);
        let serial = play(&mut e, 200);
        assert!(serial.iter().any(|s| s.abs() > 0.01));
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 0);
        for threads in [1, 2, 3, 4, 8] {
            let (out, blocks) = pooled(threads, 200);
            assert_eq!(blocks, 200, "{threads} threads: every block went through the pool");
            assert!(bits(&out) == bits(&serial), "{threads} threads differ from the serial loop");
        }
    }

    #[test]
    fn without_a_graph_that_fits_it_plays_serially() {
        let mut e = rig();
        project(&mut e);
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        play(&mut e, 2);
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 0, "no graph");
        // One missing route of each kind.
        for missing in [(1, 5), (1, 7), (2, 6), (1, 4), (2, 4)] {
            let mut r = rig();
            let full = project(&mut r);
            let edges: Vec<(Id, Id)> = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4)].into_iter().filter(|x| *x != missing).collect();
            let ids = (0..full.len()).map(|i| full.id(i)).collect();
            r.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(ids, &edges, |_| true).unwrap())))).ok().unwrap();
            r.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
            r.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
            play(&mut r, 2);
            assert_eq!(r.status.parallel_blocks.load(Ordering::Relaxed), 0, "without the route {missing:?}");
        }
    }

    #[test]
    fn a_route_the_graph_lacks_goes_serial_at_once_and_a_new_graph_brings_it_back() {
        let mut e = rig();
        let g = project(&mut e);
        e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        play(&mut e, 2);
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 2);
        // Track 3 starts sending to aux 6.
        e.tx.push(Cmd::Params(3, params(None, &[(7, 0.6, true), (6, 0.5, false)]))).ok().unwrap();
        play(&mut e, 2);
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 2, "those two blocks played serially");
        let ids = vec![1, 2, 3, 4, 5, 6, 7];
        let edges = [(1, 5), (2, 5), (1, 6), (1, 7), (2, 6), (3, 7), (4, 6), (5, 7), (1, 4), (2, 4), (3, 6)];
        e.tx.push(Cmd::Graph(Some(Box::new(TaskGraph::new(ids, &edges, |_| true).unwrap())))).ok().unwrap();
        play(&mut e, 2);
        assert_eq!(e.status.parallel_blocks.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn a_light_project_stays_on_the_callback_until_the_load_climbs() {
        let mut p = Parallel { engaged: false, ..Default::default() };
        p.weigh(0.05, 0.05);
        assert!(!p.engaged);
        p.weigh(0.2, 0.06);
        assert!(p.engaged, "one heavy block is enough");
        p.weigh(0.01, 0.2);
        assert!(p.engaged, "the average is still up");
        p.weigh(0.01, 0.1);
        assert!(!p.engaged, "no workers: under the mark means under it");
    }

    /// A panic in a strip, once it's told to, on whichever thread has it.
    struct Bomb(Arc<std::sync::atomic::AtomicBool>);

    impl Effect for Bomb {
        fn set_param(&mut self, _: u32, _: f32) {}
        fn param(&self, _: u32) -> f32 {
            0.0
        }
        fn process(&mut self, l: &mut [f32], _: &mut [f32], _: Key, _: &FxContext) {
            if self.0.swap(false, Ordering::SeqCst) {
                panic!("expected: a plug-in blowing up in an engine test");
            }
            l.fill(0.5);
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn a_panicking_strip_silences_its_block_and_only_that() {
        let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut e = rig();
        let g = project(&mut e);
        let info = EffectInfo { id: "test:bomb", name: "Bomb", category: "Utility", params: &[], meters: &[], sidechain: false, new: |_| Box::new(Tone { phase: 0.0, hz: 0.0 }) };
        let mut b = BuiltinDsp::new(&info, SR as f32, &[]);
        b.replace_effect_for_tests(Box::new(Bomb(armed.clone())));
        e.tx.push(Cmd::Insert { track: 3, slot: 1, insert: Some(InsertDsp { uid: 99, bypass: false, plugin: Plugin::Builtin(Box::new(b)), key: KeyRoute::default() }) }).ok().unwrap();
        e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 4, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: true })).ok().unwrap();
        let mut out = vec![0.0f32; BLOCK * 2];
        for _ in 0..5 {
            e.engine.process(&[], 0, &mut out, 2);
        }
        assert!(out.iter().any(|s| s.abs() > 0.01));
        armed.store(true, Ordering::SeqCst);
        e.engine.process(&[], 0, &mut out, 2);
        assert!(out.iter().all(|&s| s == 0.0), "silence for the block");
        assert_eq!(e.status.engine_errors.load(Ordering::Relaxed), 1, "and it's counted");
        // Reporting a panic can take long enough to count as an overload (with a backtrace on
        // Windows it does), and the engine then stays quiet for `SHED_SECS`.
        let blocks = (2.0 * SHED_SECS * SR / BLOCK as f64) as usize;
        let heard = (0..blocks).any(|_| {
            e.engine.process(&[], 0, &mut out, 2);
            out.iter().any(|s| s.abs() > 0.01)
        });
        assert!(heard, "the pool isn't poisoned");
        assert_eq!(e.status.engine_errors.load(Ordering::Relaxed), 1);
    }

    /// Holds up the strip it's on, as a stuck plug-in or a starved worker would.
    struct Tar(Arc<AtomicU32>);

    impl Effect for Tar {
        fn set_param(&mut self, _: u32, _: f32) {}
        fn param(&self, _: u32) -> f32 {
            0.0
        }
        fn process(&mut self, _: &mut [f32], _: &mut [f32], _: Key, _: &FxContext) {
            while self.0.load(Ordering::SeqCst) == 1 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn a_stuck_strip_costs_silence_not_a_hung_callback() {
        // Only a worker may hold it up: a task on the calling thread isn't given up on (as a plug-in
        // that never returns can't be). So everything is made to wait for a worker: a two-thread pool
        // whose caller finds nothing left to take is the sure way to get there, and the test tries
        // until the worker has the strip.
        let hold = Arc::new(AtomicU32::new(0));
        let mut e = rig();
        let g = project(&mut e);
        let info = EffectInfo { id: "test:tar", name: "Tar", category: "Utility", params: &[], meters: &[], sidechain: false, new: |_| Box::new(Tone { phase: 0.0, hz: 0.0 }) };
        let mut b = BuiltinDsp::new(&info, SR as f32, &[]);
        b.replace_effect_for_tests(Box::new(TarOnWorker(Tar(hold.clone()))));
        e.tx.push(Cmd::Insert { track: 2, slot: 1, insert: Some(InsertDsp { uid: 99, bypass: false, plugin: Plugin::Builtin(Box::new(b)), key: KeyRoute::default() }) }).ok().unwrap();
        e.tx.push(Cmd::Graph(Some(Box::new(g)))).ok().unwrap();
        e.tx.push(Cmd::Workers(Some(Workers::new(PoolOptions { threads: 2, ..Default::default() })))).ok().unwrap();
        e.tx.push(Cmd::ParallelTest(ParallelTest { always: true, renders: false, patient: false })).ok().unwrap();
        hold.store(1, Ordering::SeqCst);
        let mut out = vec![1.0f32; BLOCK * 2];
        let started = Instant::now();
        while e.status.overloads.load(Ordering::Relaxed) == 0 {
            assert!(started.elapsed().as_secs() < 20, "the worker never got the strip");
            e.engine.process(&[], 0, &mut out, 2);
        }
        assert!(out.iter().all(|&s| s == 0.0), "the block given up on is silent");
        assert!(e.engine.parallel.stalled);
        let t = Instant::now();
        out.fill(1.0);
        e.engine.process(&[], 0, &mut out, 2);
        assert!(t.elapsed() < Duration::from_millis(50), "the next callback doesn't wait");
        assert!(out.iter().all(|&s| s == 0.0));
        // Commands wait for it, since they could change the strips under the stuck thread.
        let done = e.status.cmds_done.load(Ordering::Relaxed);
        e.tx.push(Cmd::Stop).ok().unwrap();
        e.engine.process(&[], 0, &mut [], 0);
        assert_eq!(e.status.cmds_done.load(Ordering::Relaxed), done);
        // It comes unstuck: the engine catches up and, past the overload's silence, plays again.
        hold.store(0, Ordering::SeqCst);
        let started = Instant::now();
        while e.engine.parallel.stalled || e.engine.shedding > 0 {
            assert!(started.elapsed().as_secs() < 20, "it never recovered");
            e.engine.process(&[], 0, &mut out, 2);
        }
        e.tx.push(Cmd::Play { from: 0 }).ok().unwrap();
        e.engine.process(&[], 0, &mut out, 2);
        assert!(out.iter().any(|s| s.abs() > 0.01), "and plays again");
    }

    /// Tar, on worker threads only (the test thread's name tells them apart).
    struct TarOnWorker(Tar);

    impl Effect for TarOnWorker {
        fn set_param(&mut self, _: u32, _: f32) {}
        fn param(&self, _: u32) -> f32 {
            0.0
        }
        fn process(&mut self, l: &mut [f32], r: &mut [f32], k: Key, c: &FxContext) {
            if std::thread::current().name().is_some_and(|n| n.starts_with("fermata-dsp")) {
                self.0.process(l, r, k, c);
            }
        }
        fn reset(&mut self) {}
    }
}
