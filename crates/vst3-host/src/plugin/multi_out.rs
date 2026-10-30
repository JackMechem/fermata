//! Output buses past the main one: what a plug-in has, and activating them while it plays.
//!
//! VST3 lets a bus be switched only while the component is inactive, and `setActive` belongs on
//! the main thread, so the audio thread has to be kept out meanwhile without ever waiting. Each
//! `process` call takes a gate with one compare-and-swap. To change buses, the controller asks
//! the processor to park: it fades its outputs out over a few milliseconds and stops the plug-in
//! itself. If no block comes (nothing is calling `process`), the controller takes the gate
//! itself, waiting only for a block under way. While parked `process` outputs silence, the other
//! strips play on untouched, and afterwards the plug-in's outputs fade back in.
//!
//! The buffers of every bus are allocated when the plug-in is instantiated, so there is nothing
//! to size here, and a bus nobody uses is just never activated.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use super::*;

/// How long the outputs fade each way.
const FADE_SECS: f64 = 0.005;
/// How long the controller waits for the processor to park itself, before taking the gate. Long
/// enough for a fade at the largest block size.
const PARK_WAIT: Duration = Duration::from_millis(250);

const FREE: u8 = 0;
const BUSY: u8 = 1;
const PARKED: u8 = 2;

/// An audio output bus of a plug-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputBus {
    /// The bus's index in the plug-in; 0 is the main output, which plays through its strip.
    pub index: u32,
    pub name: String,
    pub channels: u32,
    /// The plug-in plays this bus now.
    pub active: bool,
}

/// What the audio thread and the controller share to change the active buses.
#[derive(Default)]
pub(super) struct OutGate {
    state: AtomicU8,
    /// The controller wants the processor to park.
    park: AtomicBool,
    /// The plug-in was told `setProcessing(1)`.
    pub processing: AtomicBool,
    /// The next block fades in.
    resume: AtomicBool,
}

impl OutGate {
    /// The audio thread's way in, for `process` and `suspend`: false while parked.
    pub fn enter(&self) -> bool {
        self.state.compare_exchange(FREE, BUSY, Ordering::Acquire, Ordering::Relaxed).is_ok()
    }

    pub fn leave(&self) {
        self.state.store(FREE, Ordering::Release);
    }

    /// Waits for the audio thread to be out of the plug-in. Returns whether it was playing.
    fn close(&self) -> bool {
        let playing = self.processing.load(Ordering::Relaxed);
        if playing {
            self.park.store(true, Ordering::Relaxed);
            let deadline = Instant::now() + PARK_WAIT;
            while self.state.load(Ordering::Acquire) != PARKED && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        // Parked by the processor; if not, no block came, or one is just finishing.
        while let Err(s) = self.state.compare_exchange(FREE, PARKED, Ordering::Acquire, Ordering::Relaxed) {
            if s == PARKED {
                break;
            }
            std::thread::yield_now();
        }
        self.park.store(false, Ordering::Relaxed);
        playing
    }

    fn open(&self, fade_in: bool) {
        self.resume.store(fade_in, Ordering::Relaxed);
        self.state.store(FREE, Ordering::Release);
    }
}

/// The audio thread's progress through a fade.
pub(super) struct OutFade {
    len: u32,
    /// Frames left of a fade out (None: not fading out) and of a fade in.
    falling: Option<u32>,
    rising: u32,
}

impl OutFade {
    pub fn new(sample_rate: f64) -> OutFade {
        OutFade { len: ((FADE_SECS * sample_rate) as u32).max(1), falling: None, rising: 0 }
    }

    /// Advances over `n` frames, with what to multiply each by.
    fn ramp(&mut self, n: usize, mut each: impl FnMut(usize, f32)) {
        let len = self.len as f32;
        for i in 0..n {
            let g = match &mut self.falling {
                Some(left) => {
                    let g = *left as f32 / len;
                    *left = left.saturating_sub(1);
                    g
                }
                None if self.rising > 0 => {
                    self.rising -= 1;
                    1.0 - self.rising as f32 / len
                }
                None => 1.0,
            };
            each(i, g);
        }
    }

    fn active(&self) -> bool {
        self.falling.is_some() || self.rising > 0
    }
}

/// Reads what the plug-in says about its output buses (`buses` has their channel counts), and
/// switches off the extra ones, which many plug-ins mark as active by default: they cost it work
/// nobody hears. Call while the component is inactive.
pub(super) fn read_outputs(component: &ComPtr<IComponent>, buses: &[Bus]) -> Vec<OutputBus> {
    let mut out = Vec::with_capacity(buses.len());
    for (i, bus) in buses.iter().enumerate() {
        let mut info: BusInfo = unsafe { std::mem::zeroed() };
        let ok = unsafe { component.getBusInfo(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32, i as i32, &mut info) } == kResultOk;
        if i > 0 {
            unsafe { component.activateBus(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32, i as i32, 0) };
        }
        let name = if ok { utf16(&info.name) } else { String::new() };
        let name = if name.trim().is_empty() { format!("Output {}", i + 1) } else { name };
        out.push(OutputBus { index: i as u32, name, channels: bus.channels as u32, active: i == 0 });
    }
    out
}

impl PluginProcessor {
    /// The block just processed on output bus `bus` (mono on both sides), or None if the plug-in
    /// has no such bus. An inactive bus reads as silence.
    pub fn output_bus(&self, bus: usize, n: usize) -> Option<(&[f32], &[f32])> {
        let b = self.outputs.get(bus)?;
        match b.channels {
            0 => None,
            1 => Some((&b.buffers[0][..n], &b.buffers[0][..n])),
            _ => Some((&b.buffers[0][..n], &b.buffers[1][..n])),
        }
    }

    /// A block while the controller has the plug-in: silence on every output.
    pub(super) fn parked_block(&mut self, left: &mut [f32], right: &mut [f32], n: usize) {
        left[..n].fill(0.0);
        right[..n].fill(0.0);
        for b in self.outputs.iter_mut().flat_map(|bus| bus.buffers.iter_mut()) {
            b[..n].fill(0.0);
        }
        self.out_fade.falling = None;
    }

    /// The end of `process`: the fades, and parking when asked.
    pub(super) fn after_block(&mut self, left: &mut [f32], right: &mut [f32], n: usize) {
        let gate = &self._shared.out_gate;
        if gate.resume.load(Ordering::Relaxed) {
            gate.resume.store(false, Ordering::Relaxed);
            self.out_fade.falling = None;
            self.out_fade.rising = self.out_fade.len;
        }
        let parking = gate.park.load(Ordering::Relaxed);
        if parking && self.out_fade.falling.is_none() {
            self.out_fade.rising = 0;
            self.out_fade.falling = Some(self.out_fade.len);
        }
        if self.out_fade.active() {
            // Every bus goes through the same ramp, starting again from the same place.
            let start = (self.out_fade.falling, self.out_fade.rising);
            self.out_fade.ramp(n, |i, g| {
                left[i] *= g;
                right[i] *= g;
            });
            let end = (self.out_fade.falling, self.out_fade.rising);
            for b in self.outputs.iter_mut().skip(1).flat_map(|bus| bus.buffers.iter_mut()) {
                (self.out_fade.falling, self.out_fade.rising) = start;
                self.out_fade.ramp(n, |i, g| b[i] *= g);
            }
            (self.out_fade.falling, self.out_fade.rising) = end;
        }
        if parking && self.out_fade.falling == Some(0) {
            self.out_fade.falling = None;
            let gate = &self._shared.out_gate;
            if gate.processing.swap(false, Ordering::Relaxed) {
                unsafe {
                    vst3::ComRef::from_raw(self.processor).unwrap().setProcessing(0);
                }
            }
            gate.state.store(PARKED, Ordering::Release);
        } else {
            self._shared.out_gate.leave();
        }
    }
}

impl PluginController {
    /// The plug-in's audio output buses, main one first.
    pub fn output_buses(&self) -> &[OutputBus] {
        &self.outputs
    }

    /// Makes `want` the extra output buses in use (indexes; the main bus is always on). Off the
    /// audio thread; takes a few milliseconds when something changes, and nothing otherwise. An
    /// index the plug-in doesn't have is ignored.
    pub fn set_active_outputs(&mut self, want: &[u32]) {
        let changes = |b: &OutputBus| b.index != 0 && b.active != want.contains(&b.index);
        if !self.outputs.iter().any(changes) {
            return;
        }
        let gate = &self.shared.out_gate;
        let fade_in = gate.close();
        unsafe {
            if gate.processing.swap(false, Ordering::Relaxed) {
                if let Some(p) = &self.shared.processor {
                    p.setProcessing(0);
                }
            }
            let component = &self.shared.component;
            if component.setActive(0) != kResultOk {
                log::warn!("{}: setActive(0) failed", self.desc.name);
            }
            for b in self.outputs.iter_mut().filter(|b| changes(b)) {
                b.active = !b.active;
                let on = b.active as u8;
                if component.activateBus(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32, b.index as i32, on) != kResultOk {
                    log::warn!("{}: couldn't switch output bus {}", self.desc.name, b.index);
                }
            }
            if component.setActive(1) != kResultOk {
                log::warn!("{}: setActive(1) failed", self.desc.name);
            }
        }
        gate.open(fade_in);
    }
}
