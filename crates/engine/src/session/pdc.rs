//! Plug-in delay compensation (PDC): how late each strip's plug-ins make its audio, and the
//! delays that bring every path back in line, as Logic's "All" mode does.
//!
//! Each strip has a latency: its instrument plus its insert plug-ins, as they report it
//! (`getLatencySamples`, re-read when a plug-in says it changed). Bypassed plug-ins still
//! count, as in Logic: the engine doesn't run them, so the strip's delay line makes up their
//! latency instead. That keeps the timing of everything else steady while a plug-in is
//! switched on and off.
//!
//! Walking the graph in processing order (every feeder before its bus):
//! - a source track's audio enters its strip on time; a bus (summing stack or aux) receives
//!   its inputs at the latest time any of them leaves its strip;
//! - each strip's output leaves at its input time plus its latency, then waits in the strip's
//!   delay line until the bus it plays into (or Stereo Out) is due, and so does each send.
//!   Sends tap the same line at their own delay, pre- or post-fader alike, so a send into a
//!   latent aux leaves earlier than the dry signal and both meet again at Stereo Out;
//! - Stereo Out is due when its latest input arrives. Its own plug-ins delay everything
//!   equally, so they need no compensation, but they count in the total, which delays the
//!   click, shifts recordings and is skipped at the start of a bounce.
//!
//! Live playing is the exception: an armed audio track that is recording, and in Low Latency
//! Mode any monitored track, plays without its compensation delay (and in Low Latency Mode
//! without plug-ins over the limit), so performers hear themselves as soon as possible.
//! Recordings are still lined up with what was heard, which reached the performer `total`
//! late.

use super::*;

/// The latest latency a plug-in may report before it is treated as broken (about ten seconds
/// at 48 kHz).
const MAX_PLUGIN_LATENCY: u32 = 1 << 19;
/// Delay lines are allocated with room for at least this many frames, so small latency
/// changes don't need a new one.
const MIN_LINE: usize = 8192 - MAX_BLOCK;

/// One strip's plug-in latency and compensation, for the channel strip readout.
#[derive(Debug, Clone, Serialize)]
pub struct StripLatency {
    /// The track (or `MASTER_ID` for Stereo Out).
    pub track: Id,
    /// The latency of its instrument and inserts, bypassed ones included (frames).
    pub latency: u32,
    /// The compensation delay on its output (frames).
    pub delay: u32,
    /// Each loaded plug-in's own latency: instance id, frames.
    pub slots: Vec<(Id, u32)>,
}

/// Latency figures for the UI.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PdcReport {
    /// Compensation is on (Settings › Audio).
    pub enabled: bool,
    pub low_latency: bool,
    /// How far Stereo Out lags the timeline (frames and ms).
    pub total: u32,
    pub total_ms: f32,
    /// Strips with latent plug-ins or a compensation delay.
    pub strips: Vec<StripLatency>,
}

/// A strip as the solver sees it.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub id: Id,
    /// Its plug-ins' latency (frames).
    pub latency: u32,
    /// The bus its output plays into; None = Stereo Out.
    pub output: Option<Id>,
    /// The aux each send slot feeds.
    pub sends: [Option<Id>; SEND_SLOTS],
}

/// When a strip's audio passes each point, in frames after the timeline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Timing {
    /// At the strip's input.
    pub input: u32,
    /// Leaving its plug-ins.
    pub post: u32,
    /// Compensation delay before its output and before each send.
    pub output_delay: u32,
    pub send_delays: [u32; SEND_SLOTS],
}

/// Works out every strip's timing so that all paths line up at Stereo Out. `nodes` are in
/// processing order (feeders before the buses they feed). Returns the timings and when
/// Stereo Out's input is due.
pub(crate) fn solve(nodes: &[Node]) -> (HashMap<Id, Timing>, u32) {
    let mut due: HashMap<Id, u32> = HashMap::new();
    let mut timing: HashMap<Id, Timing> = HashMap::new();
    let mut master = 0u32;
    for n in nodes {
        let input = due.get(&n.id).copied().unwrap_or(0);
        let post = input.saturating_add(n.latency);
        timing.insert(n.id, Timing { input, post, ..Default::default() });
        match n.output {
            Some(b) => {
                let d = due.entry(b).or_insert(0);
                *d = (*d).max(post);
            }
            None => master = master.max(post),
        }
        for b in n.sends.iter().flatten() {
            let d = due.entry(*b).or_insert(0);
            *d = (*d).max(post);
        }
    }
    for n in nodes {
        let t = timing.get_mut(&n.id).unwrap();
        let at = |b: Option<Id>| b.map(|b| due.get(&b).copied().unwrap_or(0)).unwrap_or(master);
        t.output_delay = at(n.output).saturating_sub(t.post);
        for (k, b) in n.sends.iter().enumerate() {
            if b.is_some() {
                t.send_delays[k] = at(*b).saturating_sub(t.post);
            }
        }
    }
    (timing, master)
}

/// A strip's plug-in latency: running, bypassed, and the slots Low Latency Mode turns off.
struct StripPlugins {
    active: u32,
    bypassed: u32,
    skip: u16,
    slots: Vec<(Id, u32)>,
}

impl Session {
    /// The latency of a loaded plug-in instance (0 for the built-in synth and for plug-ins a
    /// frozen track has unloaded). Built-in effects report theirs from the audio thread.
    fn slot_latency(&self, slot: &PluginSlot) -> u32 {
        let builtin = || self.builtin_latency(slot.uid);
        self.controllers.get(&slot.uid).map(|c| c.latency()).or_else(builtin).map(|l| l.min(MAX_PLUGIN_LATENCY)).unwrap_or(0)
    }

    fn strip_plugins(&self, instrument: Option<&PluginSlot>, inserts: &[Option<PluginSlot>], limit: u32) -> StripPlugins {
        let mut s = StripPlugins { active: 0, bypassed: 0, skip: 0, slots: Vec::new() };
        let all = instrument.map(|i| (INSTRUMENT_BIT, i)).into_iter().chain(inserts.iter().enumerate().filter_map(|(k, i)| i.as_ref().map(|i| (1u16 << k.min(14), i))));
        for (bit, slot) in all {
            let lat = self.slot_latency(slot);
            if lat == 0 {
                continue;
            }
            s.slots.push((slot.uid, lat));
            if slot.bypass {
                s.bypassed += lat;
            } else {
                s.active += lat;
            }
            if lat > limit {
                s.skip |= bit;
            }
        }
        s
    }

    /// Re-reads every plug-in's latency and sends the engine the delays that line the graph
    /// up again. Cheap when nothing changed: only tracks whose compensation moved get a
    /// command. Called whenever routing, plug-ins or the settings change.
    pub(super) fn update_pdc(&mut self) {
        let on = self.prefs.latency.compensation == PdcMode::All;
        let low_latency = self.prefs.latency.low_latency;
        let sr = self.sample_rate();
        let limit = if low_latency { (self.prefs.latency.low_latency_limit_ms.max(0.0) as f64 * sr / 1000.0) as u32 } else { u32::MAX };

        let p = &self.project;
        let mut nodes = Vec::new();
        let mut plugins: HashMap<Id, StripPlugins> = HashMap::new();
        for id in p.process_order() {
            let Some(t) = p.track(id).filter(|t| t.kind != TrackKind::Folder) else { continue };
            let sp = self.strip_plugins(t.instrument.as_ref(), &t.inserts, limit);
            let mut sends = [None; SEND_SLOTS];
            for (k, s) in t.sends.iter().take(SEND_SLOTS).enumerate() {
                sends[k] = s.filter(|s| s.bus != t.id && p.is_aux(s.bus)).map(|s| s.bus);
            }
            nodes.push(Node { id, latency: sp.active + sp.bypassed, output: p.output_of(id), sends });
            plugins.insert(id, sp);
        }
        let (timing, master_due) = solve(&nodes);
        let master = self.strip_plugins(None, &self.project.master.inserts, u32::MAX);
        let total = if on { master_due + master.active + master.bypassed } else { 0 };

        let mut report = PdcReport { enabled: on, low_latency, total, total_ms: (total as f64 * 1000.0 / sr.max(1.0)) as f32, strips: Vec::new() };
        let mut cmds = Vec::new();
        for n in &nodes {
            let t = timing[&n.id];
            let sp = &plugins[&n.id];
            let params = if on {
                // The line also stands in for bypassed plug-ins, which don't run.
                let mut sends = [0; SEND_SLOTS];
                for k in 0..SEND_SLOTS {
                    sends[k] = t.send_delays[k] + sp.bypassed;
                }
                PdcParams {
                    output: t.output_delay + sp.bypassed,
                    sends,
                    input_time: t.input,
                    output_time: t.post + t.output_delay,
                    active_latency: sp.active,
                    low_latency_skip: if low_latency { sp.skip } else { 0 },
                }
            } else {
                // Uncompensated, a strip is only as late as its own running plug-ins. Renders
                // of a single track still skip that, so a freeze or bounce lines up.
                PdcParams { output_time: sp.active, active_latency: sp.active, low_latency_skip: if low_latency { sp.skip } else { 0 }, ..Default::default() }
            };
            if n.latency > 0 || params.output > 0 {
                report.strips.push(StripLatency { track: n.id, latency: n.latency, delay: params.output, slots: sp.slots.clone() });
            }
            let (sent, cap) = self.pdc_sent.get(&n.id).copied().unwrap_or_default();
            let need = params.max_delay() as usize;
            let line = (need > cap).then(|| Box::new(DelayLine::new(need.max(MIN_LINE))));
            if line.is_none() && sent == params {
                continue;
            }
            let cap = line.as_ref().map(|l| l.max_delay()).unwrap_or(cap);
            self.pdc_sent.insert(n.id, (params, cap));
            cmds.push(Cmd::Pdc { track: n.id, params, line });
        }
        // Tracks that are gone keep no entry (a rebuilt track starts afresh in `build_track`).
        self.pdc_sent.retain(|id, _| timing.contains_key(id));
        if !master.slots.is_empty() {
            report.strips.push(StripLatency { track: MASTER_ID, latency: master.active + master.bypassed, delay: 0, slots: master.slots });
        }
        for c in cmds {
            self.send(c);
        }
        let global = PdcGlobal { total, low_latency };
        if global != self.pdc_global {
            self.pdc_global = global;
            self.send(Cmd::PdcGlobal(global));
        }
        self.pdc_report = report;
    }

    /// Plug-in latency and compensation, for the channel strips and the status line.
    pub fn pdc_report(&self) -> PdcReport {
        self.pdc_report.clone()
    }

    /// How far Stereo Out lags the timeline, in frames (0 with compensation off).
    pub fn pdc_total(&self) -> u32 {
        self.pdc_global.total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: Id, latency: u32, output: Option<Id>, sends: &[(usize, Id)]) -> Node {
        let mut s = [None; SEND_SLOTS];
        for &(k, b) in sends {
            s[k] = Some(b);
        }
        Node { id, latency, output, sends: s }
    }

    #[test]
    fn tracks_line_up_at_stereo_out() {
        // 1 has a 480-frame plug-in, 2 has none.
        let (t, master) = solve(&[node(1, 480, None, &[]), node(2, 0, None, &[])]);
        assert_eq!(master, 480);
        assert_eq!(t[&1].output_delay, 0);
        assert_eq!(t[&2].output_delay, 480);
    }

    #[test]
    fn sends_reach_a_latent_aux_early() {
        // 1 (dry) sends to aux 9, which has a 100-frame reverb, and plays to Stereo Out;
        // 2 has a 50-frame plug-in.
        let nodes = [node(1, 0, None, &[(0, 9)]), node(2, 50, None, &[]), node(9, 100, None, &[])];
        let (t, master) = solve(&nodes);
        assert_eq!(master, 100);
        assert_eq!(t[&9].input, 0);
        assert_eq!(t[&1].send_delays[0], 0);
        assert_eq!(t[&1].output_delay, 100);
        assert_eq!(t[&2].output_delay, 50);
        assert_eq!(t[&9].output_delay, 0);
        // Every path arrives at `master`.
        assert_eq!(t[&1].post + t[&1].output_delay, master);
        assert_eq!(t[&1].post + t[&1].send_delays[0] + nodes[2].latency + t[&9].output_delay, master);
    }

    #[test]
    fn stacks_wait_for_their_slowest_subtrack() {
        // 1 and 2 play into summing stack 5 (which has 30 frames itself); 3 goes straight out.
        let nodes = [node(1, 200, Some(5), &[]), node(2, 0, Some(5), &[]), node(5, 30, None, &[]), node(3, 0, None, &[])];
        let (t, master) = solve(&nodes);
        assert_eq!(t[&5].input, 200);
        assert_eq!(t[&2].output_delay, 200);
        assert_eq!(t[&1].output_delay, 0);
        assert_eq!(master, 230);
        assert_eq!(t[&3].output_delay, 230);
    }
}
