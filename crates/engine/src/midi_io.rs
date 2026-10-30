//! MIDI inputs. Like Logic, every hardware input is connected and routed to the selected
//! software-instrument track. Inputs can be switched off in Settings › MIDI, note velocities
//! can be reshaped, and the last messages are kept for the activity monitor.
//!
//! Controllers with an assignment (see `controllers.rs`) are taken off that path here, on the
//! input threads, and handed to the session instead of the instrument, as Logic does; while
//! an assignment is being learned, every controller message is.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

use midir::{MidiInput, MidiInputConnection};
use parking_lot::Mutex;
use serde::Serialize;

use crate::controllers::{self, CtlKind};
use crate::prefs::VelocityCurve;

/// How many messages the activity monitor keeps.
const LOG_LEN: usize = 64;

/// One message seen on a hardware input, for Settings › MIDI.
#[derive(Debug, Clone, Serialize)]
pub struct MidiLogEntry {
    /// Counts up with every message, so the UI can tell which ones are new.
    pub seq: u64,
    /// Milliseconds since the MIDI system started.
    pub ms: u64,
    pub port: String,
    pub data: [u8; 3],
}

/// A MIDI input port and whether Fermata listens to it.
#[derive(Debug, Clone, Serialize)]
pub struct MidiPort {
    pub name: String,
    pub enabled: bool,
    pub connected: bool,
}

/// Velocity shaping, shared with the input callbacks (which run on midir's threads).
#[derive(Default)]
struct Filter {
    curve: AtomicU8,
    fixed: AtomicU8,
    seq: AtomicU64,
}

/// How many taken controller messages wait for the session at most (it drains them every
/// 30 ms); beyond that the oldest go.
const TAKEN_LEN: usize = 1024;

/// A controller message taken off the live path, for the session's controller assignments.
#[derive(Debug, Clone)]
pub struct ControlIn {
    /// The port, as `controllers::port_key` names it.
    pub port: Arc<str>,
    pub data: [u8; 3],
}

/// Which controller messages the input callbacks take for the session. The callbacks run on
/// midir's threads, not the audio thread, so locking here is fine.
#[derive(Default)]
struct Controls {
    learning: AtomicBool,
    /// Assigned controllers: port key, channel, controller.
    assigned: Mutex<Vec<(String, u8, CtlKind)>>,
    taken: Mutex<VecDeque<ControlIn>>,
}

impl Controls {
    /// Takes the message if it's an assigned controller's (or any controller's while
    /// learning). Returns whether it was taken.
    fn take(&self, port: &Arc<str>, data: [u8; 3]) -> bool {
        let Some(m) = controllers::parse(&data) else { return false };
        let ours = self.learning.load(Ordering::Relaxed) || self.assigned.lock().iter().any(|(p, ch, k)| **p == **port && *ch == m.channel && *k == m.kind);
        if ours {
            let mut q = self.taken.lock();
            if q.len() >= TAKEN_LEN {
                q.pop_front();
            }
            q.push_back(ControlIn { port: port.clone(), data });
        }
        ours
    }
}

pub struct MidiIo {
    sink: Arc<Mutex<rtrb::Producer<[u8; 3]>>>,
    connections: Vec<(String, MidiInputConnection<()>)>,
    disabled: Vec<String>,
    filter: Arc<Filter>,
    log: Arc<Mutex<VecDeque<MidiLogEntry>>>,
    epoch: Instant,
    controls: Arc<Controls>,
}

impl MidiIo {
    pub fn new(sink: rtrb::Producer<[u8; 3]>) -> MidiIo {
        MidiIo {
            sink: Arc::new(Mutex::new(sink)),
            connections: Vec::new(),
            disabled: Vec::new(),
            filter: Arc::new(Filter { curve: AtomicU8::new(0), fixed: AtomicU8::new(100), seq: AtomicU64::new(0) }),
            log: Arc::default(),
            epoch: Instant::now(),
            controls: Arc::default(),
        }
    }

    /// Feeds a message in as if it came from a hardware port (on-screen keyboard, Musical
    /// Typing). It follows the same routing and recording path.
    pub fn inject(&self, data: [u8; 3]) {
        let _ = self.sink.lock().push(data);
    }

    /// Feeds a message in as if it arrived on the hardware port `port`, through everything a
    /// real input does (controller assignments, the velocity curve, the activity monitor).
    /// For tests, which have no MIDI hardware.
    pub fn inject_from(&self, port: &str, msg: &[u8]) {
        let key: Arc<str> = Arc::from(controllers::port_key(port));
        receive(&self.sink, &self.filter, &self.log, self.epoch, &self.controls, port, &key, msg);
    }

    /// The controllers whose messages go to the session rather than the instrument.
    pub fn set_assigned(&self, list: Vec<(String, u8, CtlKind)>) {
        *self.controls.assigned.lock() = list;
    }

    /// While learning, every controller message goes to the session.
    pub fn set_learning(&self, on: bool) {
        self.controls.learning.store(on, Ordering::Relaxed);
    }

    /// The controller messages taken since the last call, oldest first.
    pub fn take_controls(&self) -> Vec<ControlIn> {
        self.controls.taken.lock().drain(..).collect()
    }

    pub fn port_names() -> Vec<String> {
        let Ok(input) = MidiInput::new("Fermata") else { return Vec::new() };
        input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect()
    }

    pub fn connected(&self) -> Vec<String> {
        self.connections.iter().map(|(n, _)| n.clone()).collect()
    }

    /// Every input port the system offers, with its state.
    pub fn ports(&self) -> Vec<MidiPort> {
        Self::port_names()
            .into_iter()
            .filter(|n| !n.contains("Midi Through"))
            .map(|name| MidiPort {
                enabled: !self.disabled.contains(&name),
                connected: self.connections.iter().any(|(n, _)| *n == name),
                name,
            })
            .collect()
    }

    /// Ports to leave unconnected; takes effect at once.
    pub fn set_disabled(&mut self, names: Vec<String>) {
        self.disabled = names;
        self.refresh();
    }

    pub fn set_velocity(&self, curve: VelocityCurve, fixed: u8) {
        self.filter.curve.store(curve.to_u8(), Ordering::Relaxed);
        self.filter.fixed.store(fixed, Ordering::Relaxed);
    }

    /// The most recent messages from hardware inputs, oldest first.
    pub fn recent(&self) -> Vec<MidiLogEntry> {
        self.log.lock().iter().cloned().collect()
    }

    /// Connects to any ports not already connected and drops ones that disappeared or were
    /// switched off.
    pub fn refresh(&mut self) {
        let names = Self::port_names();
        let disabled = &self.disabled;
        self.connections.retain(|(n, _)| names.contains(n) && !disabled.contains(n));
        let Ok(probe) = MidiInput::new("Fermata") else { return };
        for port in probe.ports() {
            let Ok(name) = probe.port_name(&port) else { continue };
            if self.connections.iter().any(|(n, _)| *n == name) || name.contains("Midi Through") || self.disabled.contains(&name) {
                continue;
            }
            let Ok(mut input) = MidiInput::new("Fermata") else { continue };
            input.ignore(midir::Ignore::All);
            let sink = self.sink.clone();
            let filter = self.filter.clone();
            let log = self.log.clone();
            let epoch = self.epoch;
            let controls = self.controls.clone();
            let port_name = name.clone();
            let key: Arc<str> = Arc::from(controllers::port_key(&name));
            match input.connect(&port, "fermata-in", move |_, msg, _| receive(&sink, &filter, &log, epoch, &controls, &port_name, &key, msg), ()) {
                Ok(c) => {
                    log::info!("MIDI input connected: {name}");
                    self.connections.push((name, c));
                }
                Err(e) => log::warn!("MIDI input {name}: {e}"),
            }
        }
    }
}

/// One message from an input port: system messages are dropped, assigned controllers go to the
/// session, the rest (with the velocity curve applied) to the engine's live MIDI ring; every
/// message is logged for the activity monitor.
#[allow(clippy::too_many_arguments)]
fn receive(
    sink: &Mutex<rtrb::Producer<[u8; 3]>>,
    filter: &Filter,
    log: &Mutex<VecDeque<MidiLogEntry>>,
    epoch: Instant,
    controls: &Controls,
    port_name: &str,
    key: &Arc<str>,
    msg: &[u8],
) {
    if msg.is_empty() || msg[0] >= 0xF0 {
        return;
    }
    let mut d = [0u8; 3];
    for (o, i) in d.iter_mut().zip(msg.iter()) {
        *o = *i;
    }
    if !controls.take(key, d) {
        if d[0] & 0xF0 == 0x90 && d[2] > 0 {
            let curve = VelocityCurve::from_u8(filter.curve.load(Ordering::Relaxed));
            d[2] = curve.apply(d[2], filter.fixed.load(Ordering::Relaxed));
        }
        let _ = sink.lock().push(d);
    }
    let mut l = log.lock();
    if l.len() >= LOG_LEN {
        l.pop_front();
    }
    let seq = filter.seq.fetch_add(1, Ordering::Relaxed);
    l.push_back(MidiLogEntry { seq, ms: epoch.elapsed().as_millis() as u64, port: port_name.to_string(), data: d });
}
