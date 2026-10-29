//! Audio device enumeration and the duplex stream that drives the engine.
//!
//! cpal exposes input and output as separate streams. The output callback is the engine clock;
//! the input callback feeds a lock-free ring buffer that the output callback drains.

use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Device, FromSample, Sample, SampleFormat, SizedSample, StreamConfig, SupportedBufferSize};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::dsp::{Engine, EngineStatus};
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub channels: u16,
    pub sample_rates: Vec<u32>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostInfo {
    pub name: String,
    pub devices: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceList {
    pub host: String,
    pub hosts: Vec<HostInfo>,
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub host: Option<String>,
    pub input: Option<String>,
    pub output: Option<String>,
    pub sample_rate: u32,
    pub buffer_size: u32,
    /// When false, no input stream is opened (playback only).
    pub input_enabled: bool,
    /// Play the input of record-enabled tracks without needing their I button.
    #[serde(default)]
    pub auto_monitor: bool,
    /// Extra offset applied to recordings on top of the measured latency, like Logic's
    /// Recording Delay. Positive values move recordings earlier.
    #[serde(default)]
    pub recording_delay_ms: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig {
            host: None,
            input: None,
            output: None,
            sample_rate: 48000,
            buffer_size: 256,
            input_enabled: true,
            auto_monitor: false,
            recording_delay_ms: 0.0,
        }
    }
}

impl AudioConfig {
    /// True when the two configs open the same streams (other fields can change live).
    pub fn same_devices(&self, o: &AudioConfig) -> bool {
        self.host == o.host
            && self.input == o.input
            && self.output == o.output
            && self.sample_rate == o.sample_rate
            && self.buffer_size == o.buffer_size
            && self.input_enabled == o.input_enabled
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AudioStatus {
    pub running: bool,
    pub host: String,
    pub input_name: Option<String>,
    pub output_name: Option<String>,
    pub input_channels: u16,
    pub output_channels: u16,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub error: Option<String>,
}

const COMMON_RATES: [u32; 6] = [44100, 48000, 88200, 96000, 176400, 192000];

/// Preferred driver order: talk to PipeWire natively where it runs, since going through its
/// ALSA plugin adds a second buffering stage.
const HOST_PREFERENCE: [&str; 4] = ["PipeWire", "PulseAudio", "JACK", "ALSA"];

fn host_names() -> Vec<String> {
    let mut names: Vec<String> = cpal::available_hosts().iter().map(|h| h.name().to_string()).collect();
    names.sort_by_key(|n| HOST_PREFERENCE.iter().position(|p| p == n).unwrap_or(HOST_PREFERENCE.len()));
    names
}

fn host_by_name(name: &str) -> Option<cpal::Host> {
    cpal::available_hosts().into_iter().find(|id| id.name() == name).and_then(|id| cpal::host_from_id(id).ok())
}

fn pick_host(name: Option<&str>) -> cpal::Host {
    if let Some(h) = name.and_then(host_by_name) {
        return h;
    }
    // JACK only counts as a default when a server is actually running.
    for n in host_names() {
        if let Some(h) = host_by_name(&n) {
            if n != "JACK" || h.devices().map(|mut d| d.next().is_some()).unwrap_or(false) {
                return h;
            }
        }
    }
    cpal::default_host()
}

fn device_name(d: &Device) -> String {
    let name = d.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "Unknown device".into());
    let id = device_id(d);
    if id == "alsa:default" || id == "pipewire:input_default" || id == "pipewire:output_default" {
        "System Default".into()
    } else if id == "alsa:pipewire" {
        "PipeWire".into()
    } else if let Some(hw) = id.strip_prefix("alsa:hw:") {
        // "M-Track 2X2, USB Audio" -> "M-Track 2X2"; secondary PCMs get their number appended.
        let base = name.split(',').next().unwrap_or(&name).trim().to_string();
        match hw.rsplit_once("DEV=") {
            Some((_, dev)) if dev != "0" => format!("{base} ({dev})"),
            _ => base,
        }
    } else {
        name
    }
}

fn device_id(d: &Device) -> String {
    d.id().map(|i| i.to_string()).unwrap_or_default()
}

fn describe(d: &Device, input: bool, default_id: &str) -> Option<DeviceInfo> {
    let configs: Vec<_> = if input {
        d.supported_input_configs().ok()?.collect()
    } else {
        d.supported_output_configs().ok()?.collect()
    };
    if configs.is_empty() {
        return None;
    }
    let channels = if input { d.default_input_config().ok() } else { d.default_output_config().ok() }
        .map(|c| c.channels())
        .unwrap_or_else(|| configs.iter().map(|c| c.channels()).min().unwrap_or(0));
    let mut rates: Vec<u32> = COMMON_RATES
        .iter()
        .copied()
        .filter(|r| configs.iter().any(|c| c.min_sample_rate() <= *r && *r <= c.max_sample_rate()))
        .collect();
    rates.dedup();
    let id = device_id(d);
    Some(DeviceInfo { is_default: id == default_id, id, name: device_name(d), channels, sample_rates: rates })
}

/// ALSA exposes every PCM alias (front:, surround51:, iec958:, ...). Keep the system default,
/// PipeWire, and each card's direct hardware device.
fn worth_listing(id: &str, input: bool) -> bool {
    if let Some(rest) = id.strip_prefix("alsa:") {
        return rest == "pipewire" || rest.starts_with("hw:");
    }
    if let Some(rest) = id.strip_prefix("pipewire:") {
        // The synthetic defaults are covered by "System Default"; sinks also appear as
        // capture devices (their monitors), which aren't recording inputs.
        return !rest.ends_with("_default") && !(input && rest.starts_with("alsa_output"));
    }
    if id.starts_with("pulseaudio:") {
        return !(input && id.ends_with(".monitor"));
    }
    true
}

pub fn list_devices(host: Option<&str>) -> DeviceList {
    let host = pick_host(host);
    let def_in = host.default_input_device().map(|d| device_id(&d)).unwrap_or_default();
    let def_out = host.default_output_device().map(|d| device_id(&d)).unwrap_or_default();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    if let Ok(devs) = host.devices() {
        for d in devs {
            let id = device_id(&d);
            if worth_listing(&id, true) {
                if let Some(i) = describe(&d, true, &def_in) {
                    inputs.push(i);
                }
            }
            if worth_listing(&id, false) {
                if let Some(o) = describe(&d, false, &def_out) {
                    outputs.push(o);
                }
            }
        }
    }
    DeviceList {
        host: host.id().name().to_string(),
        hosts: host_names()
            .into_iter()
            .map(|name| {
                let devices = host_by_name(&name).and_then(|h| h.devices().ok()).map(|d| d.count()).unwrap_or(0);
                HostInfo { name, devices }
            })
            .collect(),
        inputs,
        outputs,
    }
}

fn find_device(host: &cpal::Host, id: Option<&str>, input: bool) -> Option<Device> {
    if let Some(id) = id {
        if let Ok(devs) = host.devices() {
            for d in devs {
                if device_id(&d) == id {
                    return Some(d);
                }
            }
        }
    }
    if input {
        host.default_input_device()
    } else {
        host.default_output_device()
    }
}

/// Chooses the widest channel layout at `rate`, preferring float samples.
fn choose_config(d: &Device, input: bool, rate: u32, buffer: u32) -> Result<(StreamConfig, SampleFormat)> {
    let configs: Vec<_> = if input { d.supported_input_configs()?.collect() } else { d.supported_output_configs()?.collect() };
    let fmt_rank = |f: SampleFormat| match f {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::I16 => 2,
        _ => 9,
    };
    // Use the device's own default layout: ALSA plug devices advertise up to 64 channels.
    let default_ch = if input { d.default_input_config().ok() } else { d.default_output_config().ok() }
        .map(|c| c.channels())
        .unwrap_or(2);
    let usable = |c: &&cpal::SupportedStreamConfigRange| {
        c.min_sample_rate() <= rate && rate <= c.max_sample_rate() && fmt_rank(c.sample_format()) < 9
    };
    let best = configs
        .iter()
        .filter(usable)
        .filter(|c| c.channels() == default_ch)
        .min_by_key(|c| fmt_rank(c.sample_format()))
        .or_else(|| configs.iter().filter(usable).min_by_key(|c| (c.channels().abs_diff(2), fmt_rank(c.sample_format()))))
        .ok_or_else(|| anyhow!("{} does not support {rate} Hz", device_name(d)))?;
    let buffer_size = match best.buffer_size() {
        SupportedBufferSize::Range { min, max } if *min <= buffer && buffer <= *max => BufferSize::Fixed(buffer),
        SupportedBufferSize::Unknown => BufferSize::Fixed(buffer),
        _ => BufferSize::Default,
    };
    Ok((StreamConfig { channels: best.channels(), sample_rate: rate, buffer_size }, best.sample_format()))
}

pub struct AudioIo {
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for AudioIo {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn build_output<T>(
    d: &Device,
    cfg: StreamConfig,
    engine: Arc<Mutex<Engine>>,
    mut in_rx: Option<rtrb::Consumer<f32>>,
    in_ch: usize,
    status: Arc<EngineStatus>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let out_ch = cfg.channels as usize;
    let mut out_buf = vec![0.0f32; 8192 * out_ch];
    let mut in_buf = vec![0.0f32; 8192 * in_ch.max(1)];
    let stream = d.build_output_stream::<T, _, _>(
        cfg,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            let ts = info.timestamp();
            let lat = ts.playback.duration_since(ts.callback).as_micros() as u64;
            status.output_latency_us.store(lat, Ordering::Relaxed);
            let frames = data.len() / out_ch;
            if out_buf.len() < frames * out_ch {
                out_buf.resize(frames * out_ch, 0.0);
            }
            if in_buf.len() < frames * in_ch {
                in_buf.resize(frames * in_ch, 0.0);
            }
            let want = frames * in_ch;
            let inp = &mut in_buf[..want];
            match &mut in_rx {
                Some(rx) => {
                    // Keep input latency bounded if the two device clocks drift apart.
                    let excess = rx.slots().saturating_sub(want * 3);
                    if excess > 0 {
                        if let Ok(c) = rx.read_chunk(excess - excess % in_ch.max(1)) {
                            c.commit_all();
                        }
                    }
                    status.ring_frames.store((rx.slots() / in_ch.max(1)) as u64, Ordering::Relaxed);
                    let got = rx.slots().min(want);
                    let got = got - got % in_ch.max(1);
                    if let Ok(chunk) = rx.read_chunk(got) {
                        let (a, b) = chunk.as_slices();
                        inp[..a.len()].copy_from_slice(a);
                        inp[a.len()..a.len() + b.len()].copy_from_slice(b);
                        chunk.commit_all();
                    }
                    inp[got..].fill(0.0);
                }
                None => inp.fill(0.0),
            }
            let out = &mut out_buf[..frames * out_ch];
            match engine.try_lock() {
                Some(mut e) => {
                    // A panic must never reach the audio driver: unwinding into its C callback
                    // aborts the whole app. Silence the block instead; the session reports it.
                    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.process(inp, in_ch, out, out_ch))).is_ok();
                    if !ok {
                        out.fill(0.0);
                        e.status.engine_errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
                None => out.fill(0.0),
            }
            for (d, s) in data.iter_mut().zip(out.iter()) {
                *d = T::from_sample(s.clamp(-4.0, 4.0));
            }
        },
        |e| log::error!("output stream error: {e}"),
        None,
    )?;
    Ok(stream)
}

fn build_input<T>(d: &Device, cfg: StreamConfig, mut tx: rtrb::Producer<f32>, status: Arc<EngineStatus>) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let stream = d.build_input_stream::<T, _, _>(
        cfg,
        move |data: &[T], info: &cpal::InputCallbackInfo| {
            let ts = info.timestamp();
            let lat = ts.callback.duration_since(ts.capture).as_micros() as u64;
            status.input_latency_us.store(lat, Ordering::Relaxed);
            let n = data.len().min(tx.slots());
            if let Ok(mut chunk) = tx.write_chunk(n) {
                let (a, b) = chunk.as_mut_slices();
                for (dst, src) in a.iter_mut().chain(b.iter_mut()).zip(data.iter()) {
                    *dst = f32::from_sample(*src);
                }
                chunk.commit_all();
            }
        },
        |e| log::error!("input stream error: {e}"),
        None,
    )?;
    Ok(stream)
}

macro_rules! by_format {
    ($fmt:expr, $f:ident, $($args:expr),*) => {
        match $fmt {
            SampleFormat::F32 => $f::<f32>($($args),*),
            SampleFormat::I32 => $f::<i32>($($args),*),
            SampleFormat::I16 => $f::<i16>($($args),*),
            other => Err(anyhow!("unsupported sample format {other:?}")),
        }
    };
}

pub fn start(engine: Arc<Mutex<Engine>>, cfg: &AudioConfig) -> (Option<AudioIo>, AudioStatus) {
    let (res_tx, res_rx) = std::sync::mpsc::channel::<AudioStatus>();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let cfg = cfg.clone();
    let thread = std::thread::Builder::new()
        .name("audio-io".into())
        .spawn(move || {
            let mut status = AudioStatus { sample_rate: cfg.sample_rate, buffer_size: cfg.buffer_size, ..Default::default() };
            let status_arc = engine.lock().status.clone();
            status_arc.input_latency_us.store(0, Ordering::Relaxed);
            status_arc.output_latency_us.store(0, Ordering::Relaxed);
            status_arc.ring_frames.store(0, Ordering::Relaxed);
            let result = (|| -> Result<(cpal::Stream, Option<cpal::Stream>)> {
                let host = pick_host(cfg.host.as_deref());
                status.host = host.id().name().to_string();
                let out_dev = find_device(&host, cfg.output.as_deref(), false).ok_or_else(|| anyhow!("no output device"))?;
                status.output_name = Some(device_name(&out_dev));
                let (out_cfg, out_fmt) = choose_config(&out_dev, false, cfg.sample_rate, cfg.buffer_size)?;
                status.output_channels = out_cfg.channels;
                if let BufferSize::Fixed(n) = out_cfg.buffer_size {
                    status.buffer_size = n;
                }

                let mut input = None;
                if cfg.input_enabled {
                    if let Some(in_dev) = find_device(&host, cfg.input.as_deref(), true) {
                        match choose_config(&in_dev, true, cfg.sample_rate, cfg.buffer_size) {
                            Ok((in_cfg, in_fmt)) => {
                                let ch = in_cfg.channels as usize;
                                let (tx, rx) = rtrb::RingBuffer::new(cfg.sample_rate as usize * ch);
                                match by_format!(in_fmt, build_input, &in_dev, in_cfg, tx, status_arc.clone()) {
                                    Ok(s) => {
                                        status.input_name = Some(device_name(&in_dev));
                                        status.input_channels = ch as u16;
                                        input = Some((s, rx, ch));
                                    }
                                    Err(e) => status.error = Some(format!("input: {e}")),
                                }
                            }
                            Err(e) => status.error = Some(format!("input: {e}")),
                        }
                    }
                }
                let (in_stream, in_rx, in_ch) = match input {
                    Some((s, rx, ch)) => (Some(s), Some(rx), ch),
                    None => (None, None, 0),
                };
                let out = by_format!(out_fmt, build_output, &out_dev, out_cfg, engine.clone(), in_rx, in_ch, status_arc.clone())?;
                if let Some(s) = &in_stream {
                    s.play()?;
                }
                out.play()?;
                Ok((out, in_stream))
            })();
            match result {
                Ok(streams) => {
                    status.running = true;
                    let _ = res_tx.send(status);
                    let _ = stop_rx.recv();
                    drop(streams);
                }
                Err(e) => {
                    status.error = Some(e.to_string());
                    let _ = res_tx.send(status);
                }
            }
        })
        .expect("spawn audio thread");

    let status = res_rx.recv().unwrap_or_default();
    if status.running {
        (Some(AudioIo { stop: Some(stop_tx), thread: Some(thread) }), status)
    } else {
        let _ = thread.join();
        (None, status)
    }
}
