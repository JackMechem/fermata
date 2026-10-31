//! Settings › Audio › Processing Threads on the session's side: starting and stopping the threads
//! that play tracks, off the audio thread, and keeping the engine's task graph up to date (see
//! `dsp/parallel.rs`).

use super::*;
use crate::dsp::graph::{GraphError, TaskGraph};
use crate::dsp::pool::PoolOptions;
use crate::dsp::{ParallelTest, Workers};
use crate::threads::{self as setting, ThreadChoices, Topology};

/// Whether a command can change which strips read which.
pub(super) fn changes_routes(cmd: &Cmd) -> bool {
    matches!(cmd, Cmd::AddTrack(..) | Cmd::RemoveTrack(_) | Cmd::Order(_) | Cmd::Params(..) | Cmd::Insert { .. } | Cmd::Instrument { .. } | Cmd::ReorderInserts { .. } | Cmd::Sidechain { .. })
}

/// What the session knows about the workers it started.
#[derive(Default)]
pub(super) struct ThreadState {
    pub topology: Topology,
    /// What the engine is running, as (threads, real time).
    pub running: Option<(usize, bool)>,
    /// The last graph it was sent.
    pub graph: Option<TaskGraph>,
    /// Checks route everything through the pool, so they want it whatever the project.
    pub test: ParallelTest,
    /// A measurement chose the workers itself.
    pub forced: bool,
}

impl Session {
    /// The threads the setting means on this machine.
    pub fn processing_threads(&self) -> usize {
        setting::resolve(self.audio_config.processing_threads, &self.threads.topology)
    }

    pub fn thread_choices(&self) -> ThreadChoices {
        setting::choices(&self.threads.topology)
    }

    /// Brings the workers and the graph in line with the setting and the project, after an edit.
    /// Allocates and may start or stop threads, as the audio thread never does; the engine takes
    /// the results between two blocks and sends back what it replaced, to be dropped in `idle`. Until
    /// a new graph reaches it, the engine notices that the old one doesn't fit the routes and plays
    /// the affected blocks serially, so this only needs to come soon after the edit.
    pub fn sync_parallel(&mut self) {
        if !std::mem::take(&mut self.graph_dirty) {
            return;
        }
        let threads = self.processing_threads();
        if threads <= 1 {
            self.stop_workers();
            return;
        }
        let graph = match TaskGraph::of_project(&self.project) {
            Ok(g) => Some(g),
            Err(e @ (GraphError::TooBig(_) | GraphError::Unknown(_) | GraphError::Backwards { .. })) => {
                log::warn!("playing tracks on one thread: {e}");
                None
            }
        };
        // Not for a project with nothing to gain, so a light session never has the threads at all.
        let worth = self.threads.test != ParallelTest::default() || graph.as_ref().is_some_and(|g| g.heavy_width() >= 2);
        if graph != self.threads.graph {
            self.threads.graph.clone_from(&graph);
            self.send(Cmd::Graph(graph.map(Box::new)));
        }
        let realtime = self.audio_status.running && self.audio_config.output.as_deref() != Some(audio_io::NULL_OUTPUT);
        let wanted = (threads, realtime);
        if worth && !self.threads.forced && self.threads.running != Some(wanted) {
            let opts = PoolOptions { threads, realtime: realtime.then_some((self.audio_status.buffer_size, self.audio_status.sample_rate)), cpus: self.threads.topology.fast.clone() };
            self.threads.running = Some(wanted);
            self.send(Cmd::Workers(Some(Workers::new(opts))));
        }
    }

    fn stop_workers(&mut self) {
        if self.threads.running.take().is_some() {
            self.send(Cmd::Workers(None));
        }
        if self.threads.graph.take().is_some() {
            self.send(Cmd::Graph(None));
        }
    }

    /// For measurements: starts workers exactly as given (real time on the null device, say).
    #[doc(hidden)]
    pub fn start_workers_for_checks(&mut self, opts: PoolOptions) {
        self.threads.forced = true;
        self.threads.running = Some((opts.threads, opts.realtime.is_some()));
        self.send(Cmd::Workers(Some(Workers::new(opts))));
    }

    /// For checks: whether the engine would trust its task graph right now.
    #[doc(hidden)]
    pub fn pool_usable_for_checks(&mut self) -> bool {
        self.sync_parallel();
        self.engine.lock().task_graph_fits()
    }

    /// For checks: sets the setting and routes blocks through the pool as `test` says (see
    /// [`ParallelTest`]), and takes effect by the next block.
    pub fn set_threads_for_checks(&mut self, threads: u32, test: ParallelTest) {
        self.audio_config.processing_threads = threads;
        self.threads.test = test;
        self.send(Cmd::ParallelTest(test));
        self.graph_dirty = true;
        self.sync_parallel();
    }
}
