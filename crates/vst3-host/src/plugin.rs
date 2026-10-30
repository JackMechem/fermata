//! Instantiating a VST3 plugin and splitting it into an audio-thread half (`PluginProcessor`) and
//! a UI-thread half (`PluginController`).

// C enums are i32 in the MSVC bindings and u32 elsewhere, so a cast that converts on one
// platform is a no-op on the other.
#![cfg_attr(target_env = "msvc", allow(clippy::unnecessary_cast))]

use std::sync::atomic::Ordering;
use std::sync::Arc;

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{ComPtr, ComWrapper};

use crate::host::{host_context, ComponentHandler, Gesture};
use crate::module::{load_module, PluginDescriptor};
use crate::process::{EventList, ParameterChanges};
use crate::stream::{as_stream, MemoryStream};
use crate::util::{unhex, utf16};

/// Parameter changes the host can queue for one `process` call (automation).
const MAX_QUEUED: usize = 512;

#[derive(Debug, Clone, Copy)]
pub struct MidiEvent {
    pub offset: u32,
    pub data: [u8; 3],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TransportInfo {
    pub playing: bool,
    pub recording: bool,
    pub sample_rate: f64,
    pub tempo: f64,
    pub sig_num: i32,
    pub sig_den: i32,
    pub position_samples: i64,
    pub position_quarters: f64,
    pub bar_start_quarters: f64,
    pub cycle: Option<(f64, f64)>,
}

/// A parameter as the plug-in's controller describes it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    pub units: String,
    /// Normalized 0..1.
    pub default: f64,
    /// 0 = continuous, 1 = on/off, n = n+1 discrete values.
    pub steps: i32,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PluginState {
    pub component: Vec<u8>,
    pub controller: Vec<u8>,
}

/// COM objects shared by both halves. Torn down when both halves are gone.
struct Shared {
    component: ComPtr<IComponent>,
    processor: Option<ComPtr<IAudioProcessor>>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    connections: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    _handler: ComWrapper<ComponentHandler>,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Drop for Shared {
    fn drop(&mut self) {
        unsafe {
            if let Some(p) = self.processor.take() {
                p.setProcessing(0);
            }
            self.component.setActive(0);
            if let Some((a, b)) = self.connections.take() {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
            if let Some(c) = self.controller.take() {
                c.setComponentHandler(std::ptr::null_mut());
                if self.separate_controller {
                    c.terminate();
                }
            }
            self.component.terminate();
        }
    }
}

struct Bus {
    channels: usize,
    buffers: Vec<Vec<f32>>,
    ptrs: Vec<*mut f32>,
}

impl Bus {
    fn new(channels: usize, max_block: usize) -> Bus {
        let mut buffers: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.0; max_block]).collect();
        let ptrs = buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
        Bus { channels, buffers, ptrs }
    }
}

/// The audio-thread half. `process` never allocates.
pub struct PluginProcessor {
    _shared: Arc<Shared>,
    // Borrowed from `shared`, which keeps the reference alive.
    processor: *mut IAudioProcessor,
    inputs: Vec<Bus>,
    outputs: Vec<Bus>,
    in_abb: Vec<AudioBusBuffers>,
    out_abb: Vec<AudioBusBuffers>,
    param_in: ComWrapper<ParameterChanges>,
    param_out: ComWrapper<ParameterChanges>,
    events_in: ComWrapper<EventList>,
    events_out: ComWrapper<EventList>,
    edits_rx: rtrb::Consumer<(u32, f64)>,
    out_tx: rtrb::Producer<(u32, f64)>,
    midi_map: [Option<ParamID>; 130],
    max_block: usize,
    started: bool,
    pub is_instrument: bool,
    /// Host parameter changes (automation) for the next block: id, sample offset, value.
    queued: Vec<(ParamID, i32, f64)>,
}

unsafe impl Send for PluginProcessor {}

/// The UI-thread half: parameters, state and the editor window.
pub struct PluginController {
    pub desc: PluginDescriptor,
    shared: Arc<Shared>,
    out_rx: rtrb::Consumer<(u32, f64)>,
    editor: Option<crate::editor::Editor>,
    gestures_rx: rtrb::Consumer<Gesture>,
    /// The plug-in said its latency changed (`restartComponent(kLatencyChanged)`) since the
    /// last `take_latency_changed`.
    latency_changed: bool,
}

unsafe impl Send for PluginController {}

fn channel_count(arr: SpeakerArrangement) -> usize {
    arr.count_ones() as usize
}

pub fn instantiate(
    desc: &PluginDescriptor,
    sample_rate: f64,
    max_block: usize,
) -> Result<(PluginProcessor, PluginController), String> {
    let module = load_module(&desc.path)?;
    let cid = unhex(&desc.cid).ok_or("bad class id")?;
    unsafe {
        let host = host_context();
        let component: ComPtr<IComponent> =
            module.create::<IComponent>(&cid).ok_or("factory could not create the component")?;
        if component.initialize(host.as_ptr()) != kResultOk {
            return Err("component failed to initialize".into());
        }

        // Single-component plugins implement the controller on the same object.
        let mut separate_controller = false;
        let controller = match component.cast::<IEditController>() {
            Some(c) => Some(c),
            None => {
                let mut ccid: TUID = [0; 16];
                if component.getControllerClassId(&mut ccid) == kResultOk {
                    let ccid: [u8; 16] = std::mem::transmute(ccid);
                    match module.create::<IEditController>(&ccid) {
                        Some(c) => {
                            if c.initialize(host.as_ptr()) == kResultOk {
                                separate_controller = true;
                                Some(c)
                            } else {
                                None
                            }
                        }
                        None => None,
                    }
                } else {
                    None
                }
            }
        };

        let mut connections = None;
        if separate_controller {
            if let (Some(a), Some(b)) = (
                component.cast::<IConnectionPoint>(),
                controller.as_ref().and_then(|c| c.cast::<IConnectionPoint>()),
            ) {
                a.connect(b.as_ptr());
                b.connect(a.as_ptr());
                connections = Some((a, b));
            }
        }

        let (edits_tx, edits_rx) = rtrb::RingBuffer::new(4096);
        let (out_tx, out_rx) = rtrb::RingBuffer::new(4096);
        let (gestures_tx, gestures_rx) = rtrb::RingBuffer::new(4096);
        let handler = ComWrapper::new(ComponentHandler {
            edits: parking_lot::Mutex::new(edits_tx),
            restart_flags: Default::default(),
            touched: Default::default(),
            gestures: parking_lot::Mutex::new(gestures_tx),
        });

        if let Some(c) = &controller {
            // Sync the controller with the component's initial state.
            let s = MemoryStream::new(Vec::new());
            if component.getState(as_stream(&s).as_ptr()) == kResultOk {
                s.rewind();
                c.setComponentState(as_stream(&s).as_ptr());
            }
            c.setComponentHandler(handler.to_com_ptr::<IComponentHandler>().unwrap().as_ptr());
        }

        let processor: ComPtr<IAudioProcessor> =
            component.cast::<IAudioProcessor>().ok_or("plugin has no audio processor")?;

        // Ask for stereo on the main buses; keep whatever the plugin wants on the others.
        let n_in = component.getBusCount(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32).max(0) as usize;
        let n_out = component.getBusCount(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32).max(0) as usize;
        let current = |dir: i32, i: usize| -> SpeakerArrangement {
            let mut arr: SpeakerArrangement = 0;
            processor.getBusArrangement(dir, i as i32, &mut arr);
            arr
        };
        let mut in_arr: Vec<SpeakerArrangement> =
            (0..n_in).map(|i| current(BusDirections_::kInput as i32, i)).collect();
        let mut out_arr: Vec<SpeakerArrangement> =
            (0..n_out).map(|i| current(BusDirections_::kOutput as i32, i)).collect();
        if let Some(a) = in_arr.first_mut() {
            *a = SpeakerArr::kStereo;
        }
        if let Some(a) = out_arr.first_mut() {
            *a = SpeakerArr::kStereo;
        }
        processor.setBusArrangements(in_arr.as_mut_ptr(), n_in as i32, out_arr.as_mut_ptr(), n_out as i32);

        let inputs: Vec<Bus> = (0..n_in)
            .map(|i| Bus::new(channel_count(current(BusDirections_::kInput as i32, i)), max_block))
            .collect();
        let outputs: Vec<Bus> = (0..n_out)
            .map(|i| Bus::new(channel_count(current(BusDirections_::kOutput as i32, i)), max_block))
            .collect();

        if n_in > 0 {
            component.activateBus(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32, 0, 1);
        }
        if n_out > 0 {
            component.activateBus(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32, 0, 1);
        }
        let n_ev_in = component.getBusCount(MediaTypes_::kEvent as i32, BusDirections_::kInput as i32);
        if n_ev_in > 0 {
            component.activateBus(MediaTypes_::kEvent as i32, BusDirections_::kInput as i32, 0, 1);
        }

        let mut setup = ProcessSetup {
            processMode: ProcessModes_::kRealtime as i32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            maxSamplesPerBlock: max_block as i32,
            sampleRate: sample_rate,
        };
        if processor.setupProcessing(&mut setup) != kResultOk {
            log::warn!("{}: setupProcessing failed", desc.name);
        }
        if component.setActive(1) != kResultOk {
            log::warn!("{}: setActive failed", desc.name);
        }

        let mut midi_map = [None; 130];
        if let Some(mm) = controller.as_ref().and_then(|c| c.cast::<IMidiMapping>()) {
            for (cc, slot) in midi_map.iter_mut().enumerate() {
                let mut id: ParamID = 0;
                if mm.getMidiControllerAssignment(0, 0, cc as CtrlNumber, &mut id) == kResultOk {
                    *slot = Some(id);
                }
            }
        }

        let shared = Arc::new(Shared {
            component,
            processor: Some(processor.clone()),
            controller,
            separate_controller,
            connections,
            _handler: handler,
        });

        let blank = AudioBusBuffers { numChannels: 0, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: std::ptr::null_mut() } };
        let proc_half = PluginProcessor {
            processor: processor.as_ptr(),
            _shared: shared.clone(),
            in_abb: vec![blank; inputs.len()],
            out_abb: vec![blank; outputs.len()],
            inputs,
            outputs,
            param_in: ParameterChanges::new(),
            param_out: ParameterChanges::new(),
            events_in: EventList::new(),
            events_out: EventList::new(),
            edits_rx,
            out_tx,
            midi_map,
            max_block,
            started: false,
            is_instrument: desc.is_instrument || n_in == 0,
            queued: Vec::with_capacity(MAX_QUEUED),
        };
        let ctrl_half = PluginController { desc: desc.clone(), shared, out_rx, editor: None, gestures_rx, latency_changed: false };
        Ok((proc_half, ctrl_half))
    }
}

impl PluginProcessor {
    pub fn max_block(&self) -> usize {
        self.max_block
    }

    /// Queues a parameter change (normalized value) for the next `process` call, at a sample
    /// offset within that block. Points for one parameter must come in offset order; the
    /// plug-in ramps linearly between them. Never allocates: changes past the queue's
    /// capacity are dropped.
    pub fn queue_param(&mut self, id: u32, offset: i32, value: f64) {
        if self.queued.len() < self.queued.capacity() {
            self.queued.push((id, offset, value.clamp(0.0, 1.0)));
        }
    }

    /// Processes one stereo block in place. `left`/`right` must be at most `max_block` long.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], midi: &[MidiEvent], t: &TransportInfo) {
        let n = left.len().min(right.len()).min(self.max_block);
        if n == 0 {
            return;
        }
        unsafe {
            if !self.started {
                vst3::ComRef::from_raw(self.processor).unwrap().setProcessing(1);
                self.started = true;
            }

            self.param_in.clear();
            self.param_out.clear();
            self.events_in.clear();
            self.events_out.clear();
            while let Ok((id, v)) = self.edits_rx.pop() {
                self.param_in.push(id, 0, v);
            }
            for &(id, offset, v) in &self.queued {
                self.param_in.push(id, offset.clamp(0, n as i32 - 1), v);
            }
            self.queued.clear();
            for m in midi {
                self.push_midi(m);
            }

            if let Some(bus) = self.inputs.first_mut() {
                match bus.channels {
                    0 => {}
                    1 => {
                        for i in 0..n {
                            bus.buffers[0][i] = 0.5 * (left[i] + right[i]);
                        }
                    }
                    _ => {
                        bus.buffers[0][..n].copy_from_slice(&left[..n]);
                        bus.buffers[1][..n].copy_from_slice(&right[..n]);
                        for b in bus.buffers.iter_mut().skip(2) {
                            b[..n].fill(0.0);
                        }
                    }
                }
            }
            for bus in self.inputs.iter_mut().skip(1) {
                for b in bus.buffers.iter_mut() {
                    b[..n].fill(0.0);
                }
            }
            for bus in self.outputs.iter_mut() {
                for b in bus.buffers.iter_mut() {
                    b[..n].fill(0.0);
                }
            }
            for (abb, bus) in self.in_abb.iter_mut().zip(self.inputs.iter_mut()) {
                abb.numChannels = bus.channels as i32;
                abb.silenceFlags = 0;
                abb.__field0.channelBuffers32 = bus.ptrs.as_mut_ptr();
            }
            for (abb, bus) in self.out_abb.iter_mut().zip(self.outputs.iter_mut()) {
                abb.numChannels = bus.channels as i32;
                abb.silenceFlags = 0;
                abb.__field0.channelBuffers32 = bus.ptrs.as_mut_ptr();
            }

            let mut ctx: ProcessContext = std::mem::zeroed();
            use ProcessContext_::StatesAndFlags_::*;
            // C enums are i32 in the MSVC bindings and u32 elsewhere; `as _` fits both.
            let mut state = kTempoValid | kTimeSigValid | kProjectTimeMusicValid | kBarPositionValid;
            if t.playing {
                state |= kPlaying;
            }
            if t.recording {
                state |= kRecording;
            }
            ctx.sampleRate = t.sample_rate;
            ctx.projectTimeSamples = t.position_samples;
            ctx.projectTimeMusic = t.position_quarters;
            ctx.barPositionMusic = t.bar_start_quarters;
            ctx.tempo = t.tempo;
            ctx.timeSigNumerator = t.sig_num;
            ctx.timeSigDenominator = t.sig_den;
            if let Some((a, b)) = t.cycle {
                state |= kCycleValid | kCycleActive;
                ctx.cycleStartMusic = a;
                ctx.cycleEndMusic = b;
            }
            ctx.state = state as _;

            let mut data = ProcessData {
                processMode: ProcessModes_::kRealtime as i32,
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
                numSamples: n as i32,
                numInputs: self.in_abb.len() as i32,
                numOutputs: self.out_abb.len() as i32,
                inputs: if self.in_abb.is_empty() { std::ptr::null_mut() } else { self.in_abb.as_mut_ptr() },
                outputs: if self.out_abb.is_empty() { std::ptr::null_mut() } else { self.out_abb.as_mut_ptr() },
                inputParameterChanges: self.param_in.as_com_ref::<IParameterChanges>().unwrap().as_ptr(),
                outputParameterChanges: self.param_out.as_com_ref::<IParameterChanges>().unwrap().as_ptr(),
                inputEvents: self.events_in.as_com_ref::<IEventList>().unwrap().as_ptr(),
                outputEvents: self.events_out.as_com_ref::<IEventList>().unwrap().as_ptr(),
                processContext: &mut ctx,
            };
            vst3::ComRef::from_raw(self.processor).unwrap().process(&mut data);

            match self.outputs.first() {
                Some(bus) if bus.channels >= 2 => {
                    left[..n].copy_from_slice(&bus.buffers[0][..n]);
                    right[..n].copy_from_slice(&bus.buffers[1][..n]);
                }
                Some(bus) if bus.channels == 1 => {
                    left[..n].copy_from_slice(&bus.buffers[0][..n]);
                    right[..n].copy_from_slice(&bus.buffers[0][..n]);
                }
                _ => {}
            }

            let tx = &mut self.out_tx;
            self.param_out.for_each_last(|id, v| {
                let _ = tx.push((id, v));
            });
        }
    }

    unsafe fn push_midi(&mut self, m: &MidiEvent) {
        let status = m.data[0] & 0xF0;
        let channel = (m.data[0] & 0x0F) as i16;
        let offset = m.offset as i32;
        let mut e: Event = std::mem::zeroed();
        e.busIndex = 0;
        e.sampleOffset = offset;
        e.flags = Event_::EventFlags_::kIsLive as u16;
        match status {
            0x90 if m.data[2] > 0 => {
                e.r#type = Event_::EventTypes_::kNoteOnEvent as u16;
                e.__field0.noteOn = NoteOnEvent {
                    channel,
                    pitch: m.data[1] as i16,
                    tuning: 0.0,
                    velocity: m.data[2] as f32 / 127.0,
                    length: 0,
                    noteId: -1,
                };
                self.events_in.push(e);
            }
            0x80 | 0x90 => {
                e.r#type = Event_::EventTypes_::kNoteOffEvent as u16;
                e.__field0.noteOff = NoteOffEvent {
                    channel,
                    pitch: m.data[1] as i16,
                    velocity: m.data[2] as f32 / 127.0,
                    noteId: -1,
                    tuning: 0.0,
                };
                self.events_in.push(e);
            }
            0xB0 => {
                if let Some(id) = self.midi_map[m.data[1] as usize & 0x7F] {
                    self.param_in.push(id, offset, m.data[2] as f64 / 127.0);
                }
            }
            0xD0 => {
                if let Some(id) = self.midi_map[ControllerNumbers_::kAfterTouch as usize] {
                    self.param_in.push(id, offset, m.data[1] as f64 / 127.0);
                }
            }
            0xE0 => {
                if let Some(id) = self.midi_map[ControllerNumbers_::kPitchBend as usize] {
                    let v = ((m.data[2] as u16) << 7 | m.data[1] as u16) as f64 / 16383.0;
                    self.param_in.push(id, offset, v);
                }
            }
            _ => {}
        }
    }
}

impl PluginController {
    pub fn has_editor(&self) -> bool {
        self.shared.controller.is_some()
    }

    pub fn editor_open(&self) -> bool {
        self.editor.is_some()
    }

    /// Call periodically on the UI thread.
    pub fn idle(&mut self) {
        if let Some(c) = &self.shared.controller {
            while let Ok((id, v)) = self.out_rx.pop() {
                unsafe {
                    c.setParamNormalized(id, v);
                }
            }
        }
        let closed = self.editor.as_mut().map(|e| e.pump()).unwrap_or(false);
        if closed {
            self.close_editor();
        }
        let flags = self.shared._handler.restart_flags.swap(0, Ordering::Relaxed);
        if flags != 0 {
            log::debug!("{}: restartComponent({flags:#x})", self.desc.name);
        }
        if flags & RestartFlags_::kLatencyChanged != 0 {
            self.latency_changed = true;
        }
    }

    /// The processing latency the plug-in reports, in samples (`getLatencySamples`). Its
    /// output lags its input by this much; the host compensates the other paths.
    pub fn latency(&self) -> u32 {
        match &self.shared.processor {
            Some(p) => unsafe { p.getLatencySamples() },
            None => 0,
        }
    }

    /// Whether the plug-in reported a new latency since the last call (checked in `idle`).
    pub fn take_latency_changed(&mut self) -> bool {
        std::mem::take(&mut self.latency_changed)
    }

    /// The controller's automatable parameters (hidden, read-only and program-change ones are
    /// left out).
    pub fn parameters(&self) -> Vec<ParamInfo> {
        use ParameterInfo_::ParameterFlags_::*;
        let Some(c) = &self.shared.controller else { return Vec::new() };
        let mut out = Vec::new();
        unsafe {
            for i in 0..c.getParameterCount() {
                let mut info: ParameterInfo = std::mem::zeroed();
                if c.getParameterInfo(i, &mut info) != kResultOk {
                    continue;
                }
                if info.flags & kCanAutomate == 0 || info.flags & (kIsHidden | kIsReadOnly | kIsProgramChange) != 0 {
                    continue;
                }
                out.push(ParamInfo {
                    id: info.id,
                    name: utf16(&info.title),
                    units: utf16(&info.units),
                    default: info.defaultNormalizedValue,
                    steps: info.stepCount,
                });
            }
        }
        out
    }

    /// The controller's current normalized value of a parameter.
    pub fn param_value(&self, id: u32) -> Option<f64> {
        self.shared.controller.as_ref().map(|c| unsafe { c.getParamNormalized(id) })
    }

    /// Moves the controller (and so the editor) to a value the host is playing, such as
    /// automation. Doesn't reach the processor: queue that with `PluginProcessor::queue_param`.
    pub fn set_param_value(&self, id: u32, value: f64) {
        if let Some(c) = &self.shared.controller {
            unsafe {
                c.setParamNormalized(id, value.clamp(0.0, 1.0));
            }
        }
    }

    /// Sets a parameter as a control surface would: moves the controller (and so the editor)
    /// and queues the value for the processor, like an edit made in the plug-in's own editor.
    pub fn edit_param(&self, id: u32, value: f64) {
        let v = value.clamp(0.0, 1.0);
        self.set_param_value(id, v);
        let _ = self.shared._handler.edits.lock().push((id, v));
    }

    /// The editor's begin/perform/end edit calls since the last call, oldest first.
    pub fn take_gestures(&mut self) -> Vec<Gesture> {
        let mut out = Vec::new();
        while let Ok(g) = self.gestures_rx.pop() {
            out.push(g);
        }
        out
    }

    pub fn open_editor(&mut self, title: &str) -> Result<(), String> {
        if self.editor.is_some() {
            if let Some(e) = &self.editor {
                e.raise();
            }
            return Ok(());
        }
        let c = self.shared.controller.as_ref().ok_or("plugin has no editor")?;
        let view = unsafe {
            let v = c.createView(ViewType::kEditor);
            ComPtr::<IPlugView>::from_raw(v).ok_or("plugin did not create an editor view")?
        };
        self.editor = Some(crate::editor::Editor::open(view, title)?);
        Ok(())
    }

    pub fn close_editor(&mut self) {
        if let Some(e) = self.editor.take() {
            e.close(self.shared.clone());
        }
    }

    pub fn save_state(&self) -> PluginState {
        let mut st = PluginState::default();
        unsafe {
            let s = MemoryStream::new(Vec::new());
            if self.shared.component.getState(as_stream(&s).as_ptr()) == kResultOk {
                st.component = s.data();
            }
            if let Some(c) = &self.shared.controller {
                let s = MemoryStream::new(Vec::new());
                if c.getState(as_stream(&s).as_ptr()) == kResultOk {
                    st.controller = s.data();
                }
            }
        }
        st
    }

    pub fn load_state(&self, st: &PluginState) {
        unsafe {
            if !st.component.is_empty() {
                let s = MemoryStream::new(st.component.clone());
                self.shared.component.setState(as_stream(&s).as_ptr());
                if let Some(c) = &self.shared.controller {
                    s.rewind();
                    c.setComponentState(as_stream(&s).as_ptr());
                }
            }
            if let (Some(c), false) = (&self.shared.controller, st.controller.is_empty()) {
                let s = MemoryStream::new(st.controller.clone());
                c.setState(as_stream(&s).as_ptr());
            }
        }
    }
}

impl Drop for PluginController {
    fn drop(&mut self) {
        self.close_editor();
    }
}
