//! A small pool of real-time threads that run a [`TaskGraph`]'s tasks, with the thread that asked
//! (the audio callback) working too. Knows nothing about audio: see `parallel.rs` for what the
//! tasks do.
//!
//! The rules of the audio path hold here. A block allocates nothing, takes no lock and uses no
//! channel. Tasks are bits in a few words of atomics: finishing a task's last dependency sets its
//! bit, and whoever clears a bit owns the task. So there's no queue a preempted thread could hold
//! up: a ready task can always be seen and taken by the caller, however late any worker is.
//!
//! Waiting is bounded:
//!
//! - **Workers block between blocks.** A real-time thread that never blocks is killed at
//!   RLIMIT_RTTIME's hard limit (200 ms, `audio_io::survive_slow_blocks`). A worker with nothing to
//!   do spins for [`WORKER_SPIN`] at most, then parks in a futex. Blocks arriving back to back can
//!   keep finding it awake, so a worker that hasn't truly slept for [`RT_GUARD`] sleeps before its
//!   next task (the caller does the work meanwhile). Under real overload the engine sheds load
//!   first, at `OVERLOAD_SECS`, as it always did.
//! - **The caller never waits blindly.** It takes whatever is ready and only waits with nothing
//!   to take: spinning for [`CALLER_SPIN`], then in naps of [`CALLER_NAP`], which also let a worker
//!   that wasn't granted real-time priority run on this core. Past a time limit it gives up and
//!   reports the block unfinished ([`Pool::finish`]).
//! - **A panic is contained to its task.** It's counted, its dependants still run, and the pool
//!   carries on.

use std::cell::UnsafeCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use super::graph::{TaskGraph, MAX_TASKS};
use crate::denormals::NoDenormals;

/// The most threads a pool can have, the caller included.
pub const MAX_THREADS: usize = 32;
const WORDS: usize = MAX_TASKS.div_ceil(64);

/// How long a worker with nothing to do spins before it blocks. A task's dependants usually become
/// ready within this, and a wake from a futex costs much more than it (and a slow start from a deep
/// sleep state). Short enough to be small next to any buffer.
const WORKER_SPIN: Duration = Duration::from_micros(30);
/// How long the caller spins for a task a worker is running, before it naps between looks.
const CALLER_SPIN: Duration = Duration::from_micros(300);
const CALLER_NAP: Duration = Duration::from_micros(50);
/// The longest a worker keeps taking tasks without truly having slept: well inside RTTIME's 200 ms.
const RT_GUARD: Duration = Duration::from_millis(50);
const GUARD_NAP: Duration = Duration::from_micros(200);
/// A park that took at least this long blocked for real (one that finds a wake already waiting
/// returns in far less).
const TRUE_SLEEP: Duration = Duration::from_micros(10);
/// VST3 plug-ins expect an audio thread's roomy stack, and this much of it is touched before the
/// first block so that the first deep call doesn't take page faults on a real-time thread.
const STACK_SIZE: usize = 8 << 20;
const STACK_TOUCH: usize = 256 << 10;

/// Work the pool runs: `task` indexes the graph, `thread` is 0 for the caller and otherwise a
/// worker's number, so the job can keep per-thread scratch space.
pub trait Job: Sync {
    fn run(&self, task: usize, thread: usize);
}

/// How to start a pool.
#[derive(Debug, Clone, Default)]
pub struct PoolOptions {
    /// Threads including the caller, so it starts this many less one.
    pub threads: usize,
    /// Promote the workers to real time as cpal does its callback's thread, for this buffer size
    /// and sample rate. If it's refused they stay at normal priority.
    pub realtime: Option<(u32, u32)>,
    /// Logical CPUs the workers keep to (empty: anywhere). See `threads::Topology::fast`.
    pub cpus: Vec<usize>,
}

#[repr(align(64))]
struct Line<T>(T);

struct Slot {
    thread: OnceLock<Thread>,
    parked: AtomicBool,
}

struct Shared {
    /// Bit `i` is set while task `i` is ready and nobody has taken it.
    ready: Box<[Line<AtomicU64>]>,
    /// Dependencies left per task.
    pending: Box<[AtomicU32]>,
    /// Tasks not finished yet. Zero means nobody is in a task and no bit is set.
    remaining: Line<AtomicU32>,
    /// The job and its graph. Written by the caller only with `remaining` at zero, read only by a
    /// thread that has taken a task, whose bit was set after the write.
    job: UnsafeCell<Option<(*const (dyn Job + 'static), *const TaskGraph)>>,
    /// Which tasks each thread takes first, so that a strip tends to stay on one thread from block
    /// to block and find its data in that core's cache.
    home: Box<[[u64; WORDS]]>,
    workers: Box<[Slot]>,
    panics: AtomicU32,
    shutdown: AtomicBool,
    realtime: AtomicU32,
}

// SAFETY: `job` is the only field that isn't Sync. See its comment; the pointers it holds are only
// dereferenced by the thread owning a task, as `Pool::start`'s contract keeps them valid until the
// last task finishes.
unsafe impl Sync for Shared {}
unsafe impl Send for Shared {}

impl Shared {
    fn claim(&self, thread: usize) -> Option<usize> {
        for preferred in [true, false] {
            for w in 0..WORDS {
                let mut bits = self.ready[w].0.load(Ordering::Relaxed);
                if preferred {
                    bits &= self.home[thread][w];
                }
                while bits != 0 {
                    let bit = 1u64 << bits.trailing_zeros();
                    let was = self.ready[w].0.fetch_and(!bit, Ordering::AcqRel);
                    if was & bit != 0 {
                        return Some(w * 64 + bit.trailing_zeros() as usize);
                    }
                    bits &= !bit;
                }
            }
        }
        None
    }

    fn any_ready(&self) -> bool {
        self.ready.iter().any(|w| w.0.load(Ordering::SeqCst) != 0)
    }

    /// Wakes up to `count` parked workers. Never blocks: at worst it's a futex call per worker.
    fn wake(&self, mut count: usize) {
        for w in self.workers.iter() {
            if count == 0 {
                break;
            }
            if w.parked.load(Ordering::SeqCst) && w.parked.swap(false, Ordering::SeqCst) {
                if let Some(t) = w.thread.get() {
                    t.unpark();
                }
                count -= 1;
            }
        }
    }

    fn run_task(&self, task: usize, thread: usize) {
        // SAFETY: taking the task's bit synchronized with the `start` that wrote this, and the
        // caller won't write it again until this task has counted itself out below.
        let (job, graph) = unsafe { (*self.job.get()).expect("a task without a job") };
        // SAFETY: `Pool::start`'s contract keeps both alive until `remaining` reaches zero.
        let (job, graph) = unsafe { (&*job, &*graph) };
        if catch_unwind(AssertUnwindSafe(|| job.run(task, thread))).is_err() {
            self.panics.fetch_add(1, Ordering::Relaxed);
        }
        let mut freed = 0usize;
        for &s in graph.succs(task) {
            let s = s as usize;
            if self.pending[s].fetch_sub(1, Ordering::AcqRel) == 1 {
                self.ready[s / 64].0.fetch_or(1 << (s % 64), Ordering::SeqCst);
                freed += 1;
            }
        }
        // This thread takes one of them itself.
        if freed > 1 {
            self.wake(freed - 1);
        }
        // The last thing: after this the caller may start the next block.
        self.remaining.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct Pool {
    shared: Arc<Shared>,
    joins: Vec<JoinHandle<()>>,
    threads: usize,
}

/// Runs in each worker once it's set up and before its first task, for checks that need to know
/// the worker threads (`rt_alloc_check` counts what they allocate).
static WORKER_HOOK: OnceLock<fn()> = OnceLock::new();

pub fn set_worker_hook(f: fn()) {
    let _ = WORKER_HOOK.set(f);
}

impl Pool {
    /// Starts the workers. Off the audio thread: this allocates and spawns.
    pub fn new(opts: PoolOptions) -> Pool {
        let threads = opts.threads.clamp(1, MAX_THREADS);
        let home = (0..threads)
            .map(|t| {
                let mut m = [0u64; WORDS];
                for i in (t..MAX_TASKS).step_by(threads) {
                    m[i / 64] |= 1 << (i % 64);
                }
                m
            })
            .collect();
        let shared = Arc::new(Shared {
            ready: (0..WORDS).map(|_| Line(AtomicU64::new(0))).collect(),
            pending: (0..MAX_TASKS).map(|_| AtomicU32::new(0)).collect(),
            remaining: Line(AtomicU32::new(0)),
            job: UnsafeCell::new(None),
            home,
            workers: (1..threads).map(|_| Slot { thread: OnceLock::new(), parked: AtomicBool::new(false) }).collect(),
            panics: AtomicU32::new(0),
            shutdown: AtomicBool::new(false),
            realtime: AtomicU32::new(0),
        });
        let mut joins = Vec::with_capacity(threads - 1);
        for index in 1..threads {
            let sh = shared.clone();
            let o = opts.clone();
            match thread::Builder::new().name(format!("fermata-dsp-{index}")).stack_size(STACK_SIZE).spawn(move || worker(sh, index, o)) {
                Ok(j) => joins.push(j),
                // The caller can do every task alone, with whichever workers did start.
                Err(e) => log::error!("no processing thread {index}: {e}"),
            }
        }
        Pool { shared, joins, threads }
    }

    /// Threads including the caller.
    pub fn threads(&self) -> usize {
        self.threads
    }

    /// How many workers were granted real-time priority so far.
    pub fn realtime_workers(&self) -> u32 {
        self.shared.realtime.load(Ordering::Relaxed)
    }

    /// Opens a block: the graph's tasks can now be taken. Follow with [`Pool::finish`].
    ///
    /// # Safety
    ///
    /// `job` and `graph` must stay valid, and `job` must not be mutated, until [`Pool::is_done`]
    /// is true. The previous block must be done, and only one thread may be the caller at a time.
    pub unsafe fn start(&self, graph: &TaskGraph, job: *const (dyn Job + 'static)) {
        let sh = &*self.shared;
        debug_assert!(self.is_done());
        debug_assert!(graph.len() <= MAX_TASKS);
        // SAFETY: `remaining` is zero (the contract), so nobody holds a task, and no bit is set for
        // anyone to take one.
        unsafe { *sh.job.get() = Some((job, graph as *const TaskGraph)) };
        for i in 0..graph.len() {
            sh.pending[i].store(graph.deps(i).len() as u32, Ordering::Relaxed);
        }
        sh.remaining.0.store(graph.len() as u32, Ordering::Relaxed);
        let mut bits = [0u64; WORDS];
        for &r in graph.roots() {
            bits[r as usize / 64] |= 1 << (r % 64);
        }
        // Everything above is published by these (a thread takes a bit with Acquire).
        for (w, b) in bits.iter().enumerate().filter(|(_, b)| **b != 0) {
            sh.ready[w].0.fetch_or(*b, Ordering::SeqCst);
        }
        sh.wake(graph.roots().len().saturating_sub(1));
    }

    /// Works on the open block until it's done, and returns true; or false if it isn't done after
    /// `limit`, which can only be because a worker is stuck in a task or isn't getting the CPU. The
    /// block then stays open, and [`Pool::is_done`] says when it's over.
    pub fn finish(&self, limit: Option<Duration>) -> bool {
        let sh = &*self.shared;
        let started = Instant::now();
        let mut idle_since = started;
        let mut spins = 0u32;
        loop {
            if let Some(task) = sh.claim(0) {
                sh.run_task(task, 0);
                idle_since = Instant::now();
                continue;
            }
            if sh.remaining.0.load(Ordering::Acquire) == 0 {
                return true;
            }
            std::hint::spin_loop();
            spins += 1;
            // Looking at the clock costs more than a look at the atomics.
            if spins.is_multiple_of(32) {
                let now = Instant::now();
                if limit.is_some_and(|l| now - started > l) {
                    return false;
                }
                if now - idle_since > CALLER_SPIN {
                    thread::sleep(CALLER_NAP);
                }
            }
        }
    }

    pub fn is_done(&self) -> bool {
        self.shared.remaining.0.load(Ordering::Acquire) == 0
    }

    /// Tasks that panicked since the last call.
    pub fn take_panics(&self) -> u32 {
        if self.shared.panics.load(Ordering::Relaxed) == 0 {
            return 0;
        }
        self.shared.panics.swap(0, Ordering::Relaxed)
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        for w in self.shared.workers.iter() {
            w.parked.store(false, Ordering::SeqCst);
            if let Some(t) = w.thread.get() {
                t.unpark();
            }
        }
        for j in self.joins.drain(..) {
            let _ = j.join();
        }
    }
}

fn worker(sh: Arc<Shared>, index: usize, opts: PoolOptions) {
    let slot = &sh.workers[index - 1];
    let _ = slot.thread.set(thread::current());
    promote(&sh, index, &opts);
    keep_to(&opts.cpus);
    // Flush-to-zero is per thread. It holds for the worker's life, as the thread is ours alone.
    let _ftz = NoDenormals::new();
    touch_stack();
    if let Some(hook) = WORKER_HOOK.get() {
        hook();
    }
    let mut slept = Instant::now();
    loop {
        if sh.shutdown.load(Ordering::Acquire) {
            return;
        }
        if let Some(task) = sh.claim(index) {
            sh.run_task(task, index);
            if slept.elapsed() > RT_GUARD {
                thread::sleep(GUARD_NAP);
                slept = Instant::now();
            }
            continue;
        }
        if spin_for_work(&sh) {
            continue;
        }
        // A waker sets a ready bit and then looks at `parked`; this sets `parked` and then looks at
        // the ready bits. So one of the two always sees the other (all SeqCst), and a wake that
        // finds the thread not yet parked leaves its token for `park` to return at once.
        slot.parked.store(true, Ordering::SeqCst);
        if sh.any_ready() || sh.shutdown.load(Ordering::SeqCst) {
            slot.parked.store(false, Ordering::SeqCst);
            continue;
        }
        let before = Instant::now();
        thread::park();
        slot.parked.store(false, Ordering::SeqCst);
        if before.elapsed() >= TRUE_SLEEP {
            slept = Instant::now();
        }
    }
}

/// Spins for at most `WORKER_SPIN`; true if a task became ready meanwhile.
fn spin_for_work(sh: &Shared) -> bool {
    let until = Instant::now() + WORKER_SPIN;
    loop {
        for _ in 0..16 {
            std::hint::spin_loop();
            if sh.ready.iter().any(|w| w.0.load(Ordering::Relaxed) != 0) {
                return true;
            }
        }
        if sh.shutdown.load(Ordering::Relaxed) || Instant::now() >= until {
            return false;
        }
    }
}

#[inline(never)]
fn touch_stack() {
    let mut page = [0u8; STACK_TOUCH];
    for b in page.iter_mut().step_by(4096) {
        unsafe { std::ptr::write_volatile(b, 1) };
    }
    std::hint::black_box(&page);
}

/// Real-time priority, the way cpal gets it for its audio callback (rtkit over D-Bus, at
/// `audio_thread_priority`'s priority), so a worker and the callback preempt each other evenly.
/// When it's refused the worker runs at normal priority: the callback helps with what's ready, and
/// its bounded wait keeps a slow worker from hanging it.
fn promote(sh: &Shared, index: usize, opts: &PoolOptions) {
    let Some((frames, rate)) = opts.realtime else { return };
    #[cfg(target_os = "linux")]
    {
        crate::audio_io::survive_slow_blocks();
        match audio_thread_priority::promote_current_thread_to_real_time(frames, rate) {
            Ok(_) => {
                sh.realtime.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => log::warn!("processing thread {index} stays at normal priority: {e}"),
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (sh, index, frames, rate);
}

/// Real-time threads aren't placed by a core's speed: left alone they land on efficiency cores too,
/// where a heavy plug-in takes about twice as long.
fn keep_to(cpus: &[usize]) {
    #[cfg(target_os = "linux")]
    if !cpus.is_empty() {
        // SAFETY: a plain value struct, and a call about this thread only.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            for &c in cpus {
                libc::CPU_SET(c, &mut set);
            }
            if libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) != 0 {
                log::warn!("a processing thread can't keep to the fast cores: {}", std::io::Error::last_os_error());
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = cpus;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn pool(threads: usize) -> Pool {
        Pool::new(PoolOptions { threads, ..Default::default() })
    }

    /// Records the order tasks finished in, and which thread ran each.
    struct Log {
        graph: TaskGraph,
        done: Vec<AtomicU32>,
        clock: AtomicU32,
        thread: Vec<AtomicUsize>,
        early: AtomicU32,
    }

    impl Log {
        fn new(graph: TaskGraph) -> Log {
            let n = graph.len();
            Log { graph, done: (0..n).map(|_| AtomicU32::new(0)).collect(), clock: AtomicU32::new(1), thread: (0..n).map(|_| AtomicUsize::new(0)).collect(), early: AtomicU32::new(0) }
        }
    }

    impl Job for Log {
        fn run(&self, task: usize, thread: usize) {
            if self.graph.deps(task).iter().any(|&d| self.done[d as usize].load(Ordering::SeqCst) == 0) {
                self.early.fetch_add(1, Ordering::SeqCst);
            }
            self.thread[task].store(thread, Ordering::Relaxed);
            std::hint::black_box((0..200).sum::<u32>());
            self.done[task].store(self.clock.fetch_add(1, Ordering::SeqCst), Ordering::SeqCst);
        }
    }

    fn wide_graph(n: u64) -> TaskGraph {
        // Everything feeds a bus and the bus feeds the last one, with a chain on the side.
        let ids: Vec<u64> = (0..n).collect();
        let mut edges: Vec<(u64, u64)> = (0..n - 2).map(|i| (i, n - 2)).collect();
        edges.push((n - 2, n - 1));
        edges.extend((0..8).map(|i| (i, i + 1)));
        TaskGraph::new(ids, &edges, |_| true).unwrap()
    }

    fn run_blocks(p: &Pool, g: &TaskGraph, job: &Arc<Log>, blocks: usize) {
        for _ in 0..blocks {
            for d in &job.done {
                d.store(0, Ordering::SeqCst);
            }
            let ptr: *const Log = Arc::as_ptr(job);
            unsafe { p.start(g, ptr) };
            assert!(p.finish(None));
            assert!(job.done.iter().all(|d| d.load(Ordering::SeqCst) != 0), "every task ran");
        }
    }

    #[test]
    fn every_task_runs_after_its_dependencies_whatever_the_thread_count() {
        for threads in [1, 2, 3, 8, 16] {
            let g = wide_graph(40);
            let job = Arc::new(Log::new(g.clone()));
            let p = pool(threads);
            run_blocks(&p, &g, &job, 300);
            assert_eq!(job.early.load(Ordering::SeqCst), 0, "{threads} threads");
        }
    }

    #[test]
    fn a_pool_with_only_the_caller_does_it_all() {
        let g = wide_graph(20);
        let job = Arc::new(Log::new(g.clone()));
        let p = pool(1);
        run_blocks(&p, &g, &job, 5);
        assert!(job.thread.iter().all(|t| t.load(Ordering::Relaxed) == 0));
    }

    #[test]
    fn large_graphs_use_every_ready_word() {
        let g = TaskGraph::new((0..MAX_TASKS as u64).collect(), &[], |_| true).unwrap();
        let job = Arc::new(Log::new(g.clone()));
        let p = pool(4);
        run_blocks(&p, &g, &job, 3);
    }

    struct Panicky {
        at: usize,
        ran: AtomicUsize,
    }

    impl Job for Panicky {
        fn run(&self, task: usize, _thread: usize) {
            if task == self.at {
                panic!("expected: a plug-in blowing up in a pool test");
            }
            self.ran.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_panic_is_counted_and_the_pool_carries_on() {
        let g = wide_graph(12);
        for threads in [1, 4] {
            let p = pool(threads);
            for at in [0, 5, 11] {
                let job = Arc::new(Panicky { at, ran: AtomicUsize::new(0) });
                let ptr: *const Panicky = Arc::as_ptr(&job);
                unsafe { p.start(&g, ptr) };
                assert!(p.finish(None), "dependants of the panicked task still run");
                assert_eq!(job.ran.load(Ordering::SeqCst), 11);
                assert_eq!(p.take_panics(), 1);
                assert_eq!(p.take_panics(), 0);
            }
        }
    }

    struct Sleepy {
        slow: AtomicUsize,
        ran: AtomicUsize,
    }

    impl Job for Sleepy {
        fn run(&self, _task: usize, thread: usize) {
            // Whichever worker gets a task first holds on to it, as if preempted. The caller gives
            // it the chance, as on a single core it might otherwise never run.
            if thread == 0 {
                thread::sleep(Duration::from_micros(200));
            } else if self.slow.swap(0, Ordering::SeqCst) == 1 {
                thread::sleep(Duration::from_millis(300));
            }
            self.ran.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_stuck_worker_cannot_hang_the_caller_and_everything_else_gets_done() {
        let ids: Vec<u64> = (0..64).collect();
        let g = TaskGraph::new(ids, &[], |_| true).unwrap();
        let job = Arc::new(Sleepy { slow: AtomicUsize::new(1), ran: AtomicUsize::new(0) });
        let p = pool(2);
        let ptr: *const Sleepy = Arc::as_ptr(&job);
        // Until the worker gets started and takes a task, the caller could do them all; tries go on
        // until it does.
        for _ in 0..200 {
            unsafe { p.start(&g, ptr) };
            let t = Instant::now();
            let done = p.finish(Some(Duration::from_millis(20)));
            assert!(t.elapsed() < Duration::from_millis(280), "only the time limit and a nap past it");
            if !done {
                assert!(!p.is_done());
                assert!(job.ran.load(Ordering::SeqCst) >= 63, "the caller took everything that was ready");
                while !p.is_done() {
                    thread::sleep(Duration::from_millis(1));
                }
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("the worker never got a task in 200 blocks");
    }

    #[test]
    fn workers_really_block_between_blocks() {
        let g = wide_graph(10);
        let job = Arc::new(Log::new(g.clone()));
        let p = pool(3);
        run_blocks(&p, &g, &job, 10);
        thread::sleep(Duration::from_millis(50));
        assert!(p.shared.workers.iter().all(|w| w.parked.load(Ordering::SeqCst)), "both parked");
        // And they wake for the next one.
        for _ in 0..100 {
            run_blocks(&p, &g, &job, 1);
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(job.early.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn workers_flush_denormals() {
        struct Denormal(AtomicU32);
        impl Job for Denormal {
            fn run(&self, _task: usize, thread: usize) {
                if thread != 0 && std::hint::black_box(f32::MIN_POSITIVE) * std::hint::black_box(0.5f32) > 0.0 {
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        let g = TaskGraph::new((0..64).collect(), &[], |_| true).unwrap();
        let job = Arc::new(Denormal(AtomicU32::new(0)));
        let p = pool(4);
        let ptr: *const Denormal = Arc::as_ptr(&job);
        for _ in 0..200 {
            unsafe { p.start(&g, ptr) };
            assert!(p.finish(None));
        }
        assert_eq!(job.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stopping_a_pool_returns_at_once() {
        let t = Instant::now();
        drop(pool(8));
        assert!(t.elapsed() < Duration::from_secs(5));
    }
}
