//! Which strips must finish before which, for running them at the same time (see `pool.rs` and
//! `parallel.rs`).
//!
//! The session works a graph out from the project, off the audio thread, and sends it in a
//! `Cmd::Graph`. Tasks are the engine's tracks in processing order, so every edge points forward
//! in the list: a graph can't hold a cycle, and `TaskGraph::new` refuses an edge that would make
//! one. A strip's dependencies are kept in that same order, which is the order its inputs are
//! summed in, so the sums come out bit for bit as the serial loop's do.
//!
//! The graph is only a claim about what the engine does. `Engine` checks it against the routes it
//! resolved itself (`parallel::covers`) before trusting it, and plays serially whenever it
//! doesn't hold, as in the blocks between an edit's commands and its new graph.

use crate::model::{Id, Project};

/// The most tasks a graph can have; a larger project just plays serially.
pub const MAX_TASKS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphError {
    TooBig(usize),
    /// An edge names a track that isn't in the list, or the list names one twice.
    Unknown(Id),
    /// An edge points backwards in the list (or at its own track).
    Backwards { from: Id, to: Id },
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::TooBig(n) => write!(f, "{n} tracks are more than the {MAX_TASKS} a graph can hold"),
            GraphError::Unknown(id) => write!(f, "track {id} isn't in the processing order, or is in it twice"),
            GraphError::Backwards { from, to } => write!(f, "track {from} plays into {to}, which plays before it"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskGraph {
    ids: Vec<Id>,
    /// Each task's dependencies, ascending: `deps[dep_at[i]..dep_at[i + 1]]`.
    dep_at: Vec<u32>,
    deps: Vec<u16>,
    /// And what waits for it, likewise.
    succ_at: Vec<u32>,
    succs: Vec<u16>,
    roots: Vec<u16>,
    /// The most tasks with plug-ins that can run at once, by depth in the graph.
    heavy_width: usize,
}

impl TaskGraph {
    /// A graph over `ids` (the processing order), where each `(from, to)` makes `to` wait for
    /// `from`. `heavy` says which tracks run plug-ins, the only work worth a second thread.
    pub fn new(ids: Vec<Id>, edges: &[(Id, Id)], heavy: impl Fn(Id) -> bool) -> Result<TaskGraph, GraphError> {
        let n = ids.len();
        if n > MAX_TASKS {
            return Err(GraphError::TooBig(n));
        }
        let at: std::collections::HashMap<Id, usize> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        if at.len() != n {
            return Err(GraphError::Unknown(*ids.iter().find(|id| ids.iter().filter(|x| x == id).count() > 1).unwrap_or(&0)));
        }
        let mut dep: Vec<Vec<u16>> = vec![Vec::new(); n];
        let mut succ: Vec<Vec<u16>> = vec![Vec::new(); n];
        for &(from, to) in edges {
            let a = *at.get(&from).ok_or(GraphError::Unknown(from))?;
            let b = *at.get(&to).ok_or(GraphError::Unknown(to))?;
            if a >= b {
                return Err(GraphError::Backwards { from, to });
            }
            dep[b].push(a as u16);
            succ[a].push(b as u16);
        }
        for l in dep.iter_mut().chain(succ.iter_mut()) {
            l.sort_unstable();
            l.dedup();
        }
        let flatten = |lists: &[Vec<u16>]| {
            let mut starts = Vec::with_capacity(n + 1);
            let mut all = Vec::with_capacity(lists.iter().map(Vec::len).sum());
            for l in lists {
                starts.push(all.len() as u32);
                all.extend_from_slice(l);
            }
            starts.push(all.len() as u32);
            (starts, all)
        };
        // Depth in the graph: a task's level is one past its deepest dependency, so tasks of one
        // level can overlap whatever their cost.
        let mut level = vec![0usize; n];
        for i in 0..n {
            level[i] = dep[i].iter().map(|&d| level[d as usize] + 1).max().unwrap_or(0);
        }
        let mut per_level = vec![0usize; n + 1];
        for i in 0..n {
            if heavy(ids[i]) {
                per_level[level[i]] += 1;
            }
        }
        let (dep_at, deps) = flatten(&dep);
        let (succ_at, succs) = flatten(&succ);
        let roots = (0..n).filter(|&i| dep[i].is_empty()).map(|i| i as u16).collect();
        Ok(TaskGraph { ids, dep_at, deps, succ_at, succs, roots, heavy_width: per_level.into_iter().max().unwrap_or(0) })
    }

    /// The graph of a project: what `Project::process_order` puts each track after, except a
    /// folder's subtracks, which only read its automation (the engine does that before the strips
    /// start).
    pub fn of_project(p: &Project) -> Result<TaskGraph, GraphError> {
        let ids = p.process_order();
        let mut edges = Vec::new();
        for &id in &ids {
            for to in p.feeds(id).into_iter().chain(p.keyed_by(id)) {
                edges.push((id, to));
            }
        }
        let heavy = |id: Id| p.track(id).is_some_and(|t| t.frozen.is_none() && !p.is_off(id) && (t.instrument.is_some() || t.inserts.iter().any(Option::is_some)));
        TaskGraph::new(ids, &edges, heavy)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn id(&self, task: usize) -> Id {
        self.ids[task]
    }

    /// What `task` waits for, in processing order.
    pub fn deps(&self, task: usize) -> &[u16] {
        &self.deps[self.dep_at[task] as usize..self.dep_at[task + 1] as usize]
    }

    /// What waits for `task`.
    pub fn succs(&self, task: usize) -> &[u16] {
        &self.succs[self.succ_at[task] as usize..self.succ_at[task + 1] as usize]
    }

    /// The tasks that wait for nothing.
    pub fn roots(&self) -> &[u16] {
        &self.roots
    }

    /// Whether `task` waits for `dep` directly.
    pub fn has_dep(&self, task: usize, dep: usize) -> bool {
        self.deps(task).binary_search(&(dep as u16)).is_ok()
    }

    /// How many strips with plug-ins can run at once. Under 2, a second thread has nothing to
    /// gain, whatever the project's load.
    pub fn heavy_width(&self) -> usize {
        self.heavy_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AutoMode, ChannelEq, InputSel, InstrumentOutput, PluginSlot, Track, TrackKind, TrackSend, INSERT_SLOTS, SEND_SLOTS};

    fn graph(ids: &[Id], edges: &[(Id, Id)]) -> TaskGraph {
        TaskGraph::new(ids.to_vec(), edges, |_| true).unwrap()
    }

    #[test]
    fn dependencies_and_successors_are_sorted_and_unique() {
        let g = graph(&[10, 20, 30, 40], &[(30, 40), (10, 40), (10, 40), (20, 40), (10, 30)]);
        assert_eq!(g.deps(3), [0, 1, 2]);
        assert_eq!(g.succs(0), [2, 3]);
        assert_eq!(g.roots(), [0, 1]);
        assert!(g.has_dep(3, 1) && !g.has_dep(2, 1));
        assert_eq!(g.id(2), 30);
    }

    #[test]
    fn cycles_cannot_be_built() {
        let e = TaskGraph::new(vec![1, 2, 3], &[(1, 2), (2, 3), (3, 1)], |_| false);
        assert_eq!(e.unwrap_err(), GraphError::Backwards { from: 3, to: 1 });
        assert_eq!(TaskGraph::new(vec![1, 2], &[(2, 2)], |_| false).unwrap_err(), GraphError::Backwards { from: 2, to: 2 });
    }

    #[test]
    fn unknown_tracks_and_repeats_are_refused() {
        assert_eq!(TaskGraph::new(vec![1], &[(1, 9)], |_| false).unwrap_err(), GraphError::Unknown(9));
        assert_eq!(TaskGraph::new(vec![1, 1], &[], |_| false).unwrap_err(), GraphError::Unknown(1));
        assert!(matches!(TaskGraph::new((0..MAX_TASKS as Id + 1).collect(), &[], |_| false), Err(GraphError::TooBig(_))));
    }

    #[test]
    fn only_strips_with_plug_ins_at_one_depth_count_as_width() {
        let g = TaskGraph::new(vec![1, 2, 3, 4], &[(1, 3), (2, 3), (3, 4)], |id| id != 2).unwrap();
        assert_eq!(g.heavy_width(), 1, "1, 3 and 4 run one after the other");
        let g = TaskGraph::new(vec![1, 2, 3, 4], &[(1, 3), (2, 3), (3, 4)], |_| true).unwrap();
        assert_eq!(g.heavy_width(), 2);
    }

    fn track(id: Id, kind: TrackKind) -> Track {
        Track {
            id,
            kind,
            name: format!("t{id}"),
            color: String::new(),
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            armed: false,
            monitor: false,
            input: InputSel::None,
            instrument: None,
            inserts: vec![None; INSERT_SLOTS],
            regions: Vec::new(),
            eq: ChannelEq::default(),
            parent: None,
            collapsed: false,
            group: None,
            sends: vec![None; SEND_SLOTS],
            output: None,
            bus: 0,
            hidden: false,
            automation: Vec::new(),
            auto_mode: AutoMode::Read,
            frozen: None,
            vca: None,
            alternatives: Vec::new(),
            alternative: String::new(),
            auto_lanes: Vec::new(),
            instrument_output: None,
            off: false,
        }
    }

    fn project(tracks: Vec<Track>) -> Project {
        let mut p = Project::new("graph", 48000);
        p.tracks = tracks;
        p
    }

    fn insert(uid: Id, sidechain: Option<Id>) -> PluginSlot {
        PluginSlot { uid, plugin_id: "builtin:gain".into(), name: "Gain".into(), bypass: false, state: None, params: Vec::new(), sidechain, sidechain_pre: false }
    }

    #[test]
    fn every_kind_of_route_is_an_edge() {
        let mut send = track(1, TrackKind::Audio);
        send.sends[0] = Some(TrackSend { bus: 5, level_db: 0.0, pre_fader: false, muted: false });
        let mut sub = track(2, TrackKind::Audio);
        sub.parent = Some(3);
        let stack = track(3, TrackKind::Summing);
        let mut out = track(4, TrackKind::Audio);
        out.output = Some(6);
        let mut aux = track(5, TrackKind::Aux);
        aux.bus = 1;
        let mut out_aux = track(6, TrackKind::Aux);
        out_aux.bus = 2;
        let mut keyed = track(7, TrackKind::Audio);
        keyed.inserts[0] = Some(insert(70, Some(4)));
        let inst = track(8, TrackKind::Instrument);
        let mut multi = track(9, TrackKind::Aux);
        multi.bus = 3;
        multi.instrument_output = Some(InstrumentOutput { track: 8, bus: 1 });
        let p = project(vec![send, sub, stack, out, aux, out_aux, keyed, inst, multi]);
        let g = TaskGraph::of_project(&p).unwrap();
        let at = |id: Id| (0..g.len()).find(|&i| g.id(i) == id).unwrap();
        for (from, to, what) in [(1, 5, "send"), (2, 3, "summing stack"), (4, 6, "output"), (4, 7, "side-chain key"), (8, 9, "instrument output")] {
            assert!(g.has_dep(at(to), at(from)), "{what}: {from} before {to}");
        }
        assert!(!g.has_dep(at(5), at(4)), "no route, no edge");
    }

    #[test]
    fn folder_subtracks_stay_independent() {
        let folder = track(1, TrackKind::Folder);
        let mut a = track(2, TrackKind::Audio);
        a.parent = Some(1);
        let mut b = track(3, TrackKind::Audio);
        b.parent = Some(1);
        let g = TaskGraph::of_project(&project(vec![folder, a, b])).unwrap();
        let at = |id: Id| (0..g.len()).find(|&i| g.id(i) == id).unwrap();
        assert!(g.deps(at(2)).is_empty() && g.deps(at(3)).is_empty(), "they read the folder's lanes before the strips start");
    }

    #[test]
    fn frozen_and_off_tracks_are_not_heavy() {
        let mut a = track(1, TrackKind::Audio);
        a.inserts[0] = Some(insert(10, None));
        let mut b = track(2, TrackKind::Audio);
        b.inserts[0] = Some(insert(20, None));
        let g = TaskGraph::of_project(&project(vec![a.clone(), b.clone()])).unwrap();
        assert_eq!(g.heavy_width(), 2);
        b.off = true;
        assert_eq!(TaskGraph::of_project(&project(vec![a.clone(), b.clone()])).unwrap().heavy_width(), 1);
    }
}
