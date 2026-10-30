//! Linear Phase EQ, after Logic's: the Channel EQ's eight bands, with linear phase.
//!
//! - The bands are the Channel EQ's (`crate::eq`): their combined magnitude response is
//!   realized as a linear-phase FIR (see `fir.rs`), so every frequency is delayed equally and
//!   nothing smears in time, at the cost of latency, which delay compensation makes up.
//! - **Quality** picks the kernel length: Low, Medium and High are 2047, 4095 and 8191 taps at
//!   44.1 or 48 kHz (twice that at 88.2 or 96 kHz, four times at 176.4 or 192). Longer kernels
//!   resolve low frequencies better. The latency is half the kernel: 1023, 2047 or 4095 frames
//!   at 48 kHz (21, 43 or 85 ms).
//! - Designing a kernel takes a few milliseconds, too long for the audio thread, so each
//!   instance has a worker thread. Parameter changes (from the panel or automation) reach it
//!   through atomics; it designs at most every [`DESIGN_INTERVAL`] and hands the kernel over
//!   through a lock-free queue, and the convolver crossfades to it over about
//!   [`FADE_SECS`]. A fast automation sweep is therefore followed in steps of about 25 ms,
//!   each faded into the next.
//! - Offline renders (bounces, freezes) may block, so the effect waits for the worker instead:
//!   changes land at the same point of every render, and the result doesn't depend on the
//!   speed of the machine.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::fir::{linear_phase_taps, plan_forward, Convolver, Kernel};
use super::*;
use crate::model::{BandKind, ChannelEq};

/// The bands, in the Channel EQ's order.
pub const BANDS: usize = 8;
/// The output level (dB).
pub const OUTPUT: u32 = 64;
/// The kernel length: 0 Low, 1 Medium, 2 High.
pub const QUALITY: u32 = 65;
/// One past the highest parameter id.
const ID_SPACE: usize = 66;

/// A band's on switch. Each band's parameters are `8 * band + field`.
pub const fn on(band: usize) -> u32 {
    (band * 8) as u32
}
/// A band's frequency (Hz).
pub const fn freq(band: usize) -> u32 {
    (band * 8 + 1) as u32
}
/// A band's gain (dB); the cut filters have none.
pub const fn gain(band: usize) -> u32 {
    (band * 8 + 2) as u32
}
/// A band's Q.
pub const fn q(band: usize) -> u32 {
    (band * 8 + 3) as u32
}
/// A cut filter's slope: 0 = 12, 1 = 24, 2 = 48 dB/octave.
pub const fn slope(band: usize) -> u32 {
    (band * 8 + 4) as u32
}

/// Whether a band is a cut filter (the first and last).
pub const fn is_cut(band: usize) -> bool {
    band == 0 || band == BANDS - 1
}

/// The fastest the worker designs new kernels while parameters keep changing.
pub const DESIGN_INTERVAL: Duration = Duration::from_millis(25);
/// How long the old and new kernels crossfade.
pub const FADE_SECS: f32 = 0.02;
/// How often an idle worker looks for changes.
const POLL: Duration = Duration::from_millis(10);

const SLOPES: &[&str] = &["12 dB/Oct", "24 dB/Oct", "48 dB/Oct"];
const QUALITIES: &[&str] = &["Low", "Medium", "High"];

const fn toggle(id: u32, name: &'static str, default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 0.0, max: 1.0, default, unit: Unit::None, kind: ParamKind::Toggle, log: false }
}
const fn hz(id: u32, name: &'static str, default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 20.0, max: 20000.0, default, unit: Unit::Hz, kind: ParamKind::Continuous, log: true }
}
const fn db(id: u32, name: &'static str) -> ParamSpec {
    ParamSpec { id, name, min: -24.0, max: 24.0, default: 0.0, unit: Unit::Db, kind: ParamKind::Continuous, log: false }
}
const fn qf(id: u32, name: &'static str, default: f32) -> ParamSpec {
    ParamSpec { id, name, min: 0.1, max: 18.0, default, unit: Unit::None, kind: ParamKind::Continuous, log: true }
}
const fn slope_spec(id: u32, name: &'static str) -> ParamSpec {
    ParamSpec { id, name, min: 0.0, max: 2.0, default: 1.0, unit: Unit::None, kind: ParamKind::Choice(SLOPES), log: false }
}

/// The parameters, defaulting to the Channel EQ's defaults (`ChannelEq::default`).
static PARAMS: [ParamSpec; 34] = [
    toggle(on(0), "Low Cut On", 0.0),
    hz(freq(0), "Low Cut Frequency", 30.0),
    qf(q(0), "Low Cut Q", 0.71),
    slope_spec(slope(0), "Low Cut Slope"),
    toggle(on(1), "Low Shelf On", 1.0),
    hz(freq(1), "Low Shelf Frequency", 80.0),
    db(gain(1), "Low Shelf Gain"),
    qf(q(1), "Low Shelf Q", 0.71),
    toggle(on(2), "Bell 1 On", 1.0),
    hz(freq(2), "Bell 1 Frequency", 160.0),
    db(gain(2), "Bell 1 Gain"),
    qf(q(2), "Bell 1 Q", 1.1),
    toggle(on(3), "Bell 2 On", 1.0),
    hz(freq(3), "Bell 2 Frequency", 500.0),
    db(gain(3), "Bell 2 Gain"),
    qf(q(3), "Bell 2 Q", 1.1),
    toggle(on(4), "Bell 3 On", 1.0),
    hz(freq(4), "Bell 3 Frequency", 1500.0),
    db(gain(4), "Bell 3 Gain"),
    qf(q(4), "Bell 3 Q", 1.1),
    toggle(on(5), "Bell 4 On", 1.0),
    hz(freq(5), "Bell 4 Frequency", 5000.0),
    db(gain(5), "Bell 4 Gain"),
    qf(q(5), "Bell 4 Q", 1.1),
    toggle(on(6), "High Shelf On", 1.0),
    hz(freq(6), "High Shelf Frequency", 10000.0),
    db(gain(6), "High Shelf Gain"),
    qf(q(6), "High Shelf Q", 0.71),
    toggle(on(7), "High Cut On", 0.0),
    hz(freq(7), "High Cut Frequency", 18000.0),
    qf(q(7), "High Cut Q", 0.71),
    slope_spec(slope(7), "High Cut Slope"),
    db(OUTPUT, "Output Gain"),
    ParamSpec { id: QUALITY, name: "Quality", min: 0.0, max: 2.0, default: 1.0, unit: Unit::None, kind: ParamKind::Choice(QUALITIES), log: false },
];

pub const PRESETS: &[FactoryPreset] = &[
    FactoryPreset { name: "Flat", values: &[] },
    FactoryPreset { name: "Gentle Mastering Lift", values: &[(gain(1), 1.0), (freq(4), 3000.0), (gain(4), 1.0), (q(4), 0.8), (freq(6), 12000.0), (gain(6), 1.5)] },
    FactoryPreset { name: "Low End Cleanup", values: &[(on(0), 1.0), (freq(0), 35.0), (slope(0), 2.0), (freq(2), 250.0), (gain(2), -2.0), (q(2), 1.2)] },
    FactoryPreset { name: "Mid Scoop", values: &[(gain(3), -3.0), (q(3), 0.9)] },
    FactoryPreset { name: "Vocal Presence", values: &[(on(0), 1.0), (freq(0), 90.0), (freq(3), 300.0), (gain(3), -1.5), (freq(4), 3500.0), (gain(4), 2.5), (q(4), 1.0)] },
    FactoryPreset { name: "Air", values: &[(freq(6), 12000.0), (gain(6), 3.0)] },
];

pub fn info() -> EffectInfo {
    EffectInfo { id: "builtin:linear-phase-eq", name: "Linear Phase EQ", category: "EQ", params: &PARAMS, meters: &[], sidechain: false, new: |sr| Box::new(LinearPhaseEq::new(sr)) }
}

/// The kernel length for a quality (0..=2) at a sample rate: odd, so the centre is a sample.
pub fn taps(quality: usize, sample_rate: f32) -> usize {
    let base = [2048, 4096, 8192][quality.min(2)];
    let scale = ((sample_rate / 48_000.0).round() as usize).clamp(1, 4).next_power_of_two().min(4);
    base * scale - 1
}

/// The Channel EQ settings the parameter values describe.
pub fn channel_eq(value: impl Fn(u32) -> f32) -> ChannelEq {
    let mut eq = ChannelEq::default();
    for (b, band) in eq.bands.iter_mut().enumerate() {
        band.on = value(on(b)) >= 0.5;
        band.freq = value(freq(b));
        band.q = value(q(b));
        if is_cut(b) {
            band.slope = [12, 24, 48][(value(slope(b)).round().max(0.0) as usize).min(2)];
        } else {
            band.gain = value(gain(b));
        }
    }
    debug_assert!(matches!(eq.bands[0].kind, BandKind::LowCut) && matches!(eq.bands[BANDS - 1].kind, BandKind::HighCut));
    eq.output_db = value(OUTPUT);
    eq
}

fn quality_of(values: &[f32; ID_SPACE]) -> usize {
    values[QUALITY as usize].round().clamp(0.0, 2.0) as usize
}

/// Designs the kernel for a set of parameter values. Allocates and takes a few
/// milliseconds: never on the audio thread.
fn design(values: &[f32; ID_SPACE], sample_rate: f32, gen: u64, fft: &dyn realfft::RealToComplex<f32>) -> Box<Kernel> {
    let eq = channel_eq(|id| values[id as usize]);
    let len = taps(quality_of(values), sample_rate);
    let bands_flat = ChannelEq { output_db: 0.0, ..eq.clone() }.is_flat();
    let taps = if bands_flat {
        // Only the output level: an exact delay, which also costs nearly nothing to run.
        let mut t = vec![0.0f32; len];
        t[(len - 1) / 2] = util::db_to_gain(eq.output_db);
        t
    } else {
        let c = crate::eq::coefficients(&eq, sample_rate as f64);
        linear_phase_taps(len, |w| c.magnitude(w))
    };
    Kernel::new(&taps, gen, fft)
}

/// What the effect shares with its worker.
struct Shared {
    values: [AtomicU32; ID_SPACE],
    /// Bumped after every parameter change.
    gen: AtomicU64,
    /// An offline render waits: design now, whatever the interval.
    urgent: AtomicBool,
    stop: AtomicBool,
}

/// The effect's end of the worker: kernels come in, used ones go back to be freed.
struct Link {
    shared: Arc<Shared>,
    kernels: rtrb::Consumer<Box<Kernel>>,
    spent: rtrb::Producer<Box<Kernel>>,
    worker: std::thread::Thread,
}

/// The worker: designs a kernel whenever the parameters changed, no more often than
/// `DESIGN_INTERVAL` unless an offline render asks, and frees kernels the effect is done with.
fn run_worker(shared: Arc<Shared>, mut out: rtrb::Producer<Box<Kernel>>, mut spent: rtrb::Consumer<Box<Kernel>>, sample_rate: f32, mut done: u64) {
    let fft = plan_forward();
    let mut last: Option<Instant> = None;
    while !shared.stop.load(Ordering::Acquire) {
        while spent.pop().is_ok() {}
        let gen = shared.gen.load(Ordering::Acquire);
        let urgent = shared.urgent.load(Ordering::Acquire);
        let since = last.map(|t| t.elapsed()).unwrap_or(DESIGN_INTERVAL);
        if gen != done && (urgent || since >= DESIGN_INTERVAL) && out.slots() > 0 {
            // The values are at least as new as `gen` (they're stored before it).
            let values: [f32; ID_SPACE] = std::array::from_fn(|i| f32::from_bits(shared.values[i].load(Ordering::Relaxed)));
            shared.urgent.store(false, Ordering::Release);
            let k = design(&values, sample_rate, gen, fft.as_ref());
            if out.push(k).is_ok() {
                done = gen;
            }
            last = Some(Instant::now());
            continue;
        }
        let wait = if gen != done { DESIGN_INTERVAL.saturating_sub(since).max(Duration::from_millis(1)) } else { POLL };
        std::thread::park_timeout(wait);
    }
}

/// Hands a kernel back to the worker to free. Never frees on the audio thread: if the queue
/// is full (the worker has stopped), the kernel is leaked instead.
fn dispose(link: &mut Option<Link>, k: Box<Kernel>) {
    match link {
        Some(l) => {
            if let Err(rtrb::PushError::Full(k)) = l.spent.push(k) {
                std::mem::forget(k);
            }
        }
        None => std::mem::forget(k),
    }
}

pub struct LinearPhaseEq {
    sample_rate: f32,
    values: [f32; ID_SPACE],
    /// Counts parameter changes; a kernel carries the count it was designed for.
    gen: u64,
    conv: Convolver,
    link: Option<Link>,
    /// Rendering offline: wait for kernels instead of fading to them when they're ready.
    offline: bool,
    /// Reset (or new) and not yet run: a new kernel replaces the old one without a fade.
    fresh: bool,
    /// Frames since an offline render last waited for a kernel, and the least it waits between
    /// two (`DESIGN_INTERVAL` in frames, so renders follow automation as playback does).
    since_design: usize,
    design_frames: usize,
}

impl LinearPhaseEq {
    pub fn new(sample_rate: f32) -> LinearPhaseEq {
        let mut values = [0.0f32; ID_SPACE];
        for p in PARAMS.iter() {
            values[p.id as usize] = p.default;
        }
        let fft = plan_forward();
        let kernel = design(&values, sample_rate, 0, fft.as_ref());
        let conv = Convolver::new(taps(2, sample_rate), kernel, (FADE_SECS * sample_rate) as usize);
        let shared = Arc::new(Shared { values: std::array::from_fn(|i| AtomicU32::new(values[i].to_bits())), gen: AtomicU64::new(0), urgent: AtomicBool::new(false), stop: AtomicBool::new(false) });
        let (k_tx, k_rx) = rtrb::RingBuffer::new(2);
        let (s_tx, s_rx) = rtrb::RingBuffer::new(8);
        let worker_shared = shared.clone();
        let link = std::thread::Builder::new()
            .name("linear-phase-eq".into())
            .spawn(move || run_worker(worker_shared, k_tx, s_rx, sample_rate, 0))
            .map(|h| Link { shared, kernels: k_rx, spent: s_tx, worker: h.thread().clone() })
            .map_err(|e| log::error!("Linear Phase EQ: no worker thread, so its settings stay as they are: {e}"))
            .ok();
        LinearPhaseEq {
            sample_rate,
            values,
            gen: 0,
            conv,
            link,
            offline: false,
            fresh: true,
            since_design: 0,
            design_frames: (DESIGN_INTERVAL.as_secs_f32() * sample_rate) as usize,
        }
    }

    /// Puts a kernel to use: at once after a reset, else with a crossfade.
    fn adopt(&mut self, k: Box<Kernel>) {
        let old = if self.fresh { Some(self.conv.replace(k)) } else { self.conv.queue(k) };
        if let Some(old) = old {
            dispose(&mut self.link, old);
        }
    }

    /// Takes the newest kernel the worker has finished (playing live).
    fn collect(&mut self) {
        let Some(link) = &mut self.link else { return };
        let mut newest: Option<Box<Kernel>> = None;
        while let Ok(k) = link.kernels.pop() {
            if let Some(old) = newest.replace(k) {
                if let Err(rtrb::PushError::Full(k)) = link.spent.push(old) {
                    std::mem::forget(k);
                }
            }
        }
        if let Some(k) = newest {
            self.adopt(k);
        }
    }

    /// Waits for the kernel of the current settings (offline only: this blocks).
    fn wait_for_kernel(&mut self) {
        let want = self.gen;
        let Some(link) = &mut self.link else { return };
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut best: Option<Box<Kernel>> = None;
        loop {
            while let Ok(k) = link.kernels.pop() {
                if let Some(old) = best.replace(k) {
                    drop(old);
                }
            }
            if best.as_ref().is_some_and(|k| k.gen >= want) || Instant::now() > deadline {
                break;
            }
            link.shared.urgent.store(true, Ordering::Release);
            link.worker.unpark();
            std::thread::sleep(Duration::from_micros(200));
        }
        if let Some(k) = best {
            self.adopt(k);
        }
        self.since_design = 0;
    }

    fn dispose_retired(&mut self) {
        let link = &mut self.link;
        for k in self.conv.retired() {
            dispose(link, k);
        }
    }

    /// The settings changed since the newest kernel was designed.
    fn stale(&self) -> bool {
        self.conv.newest_gen() != self.gen
    }
}

impl Drop for LinearPhaseEq {
    fn drop(&mut self) {
        if let Some(l) = &self.link {
            l.shared.stop.store(true, Ordering::Release);
            l.worker.unpark();
        }
    }
}

impl Effect for LinearPhaseEq {
    fn set_param(&mut self, id: u32, value: f32) {
        let Some(v) = self.values.get_mut(id as usize) else { return };
        if *v == value {
            return;
        }
        *v = value;
        self.gen += 1;
        if let Some(l) = &self.link {
            l.shared.values[id as usize].store(value.to_bits(), Ordering::Relaxed);
            l.shared.gen.store(self.gen, Ordering::Release);
        }
    }

    fn param(&self, id: u32) -> f32 {
        self.values.get(id as usize).copied().unwrap_or(0.0)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _key: Key, _ctx: &FxContext) {
        if self.offline {
            if self.stale() && (self.fresh || self.since_design >= self.design_frames) {
                self.wait_for_kernel();
            }
        } else {
            self.collect();
        }
        self.fresh = false;
        self.conv.process(l, r);
        self.since_design += l.len();
        self.dispose_retired();
    }

    fn reset(&mut self) {
        self.conv.reset();
        self.fresh = true;
        if self.offline && self.stale() {
            self.wait_for_kernel();
        }
        self.dispose_retired();
    }

    fn latency(&self) -> usize {
        (taps(quality_of(&self.values), self.sample_rate) - 1) / 2
    }

    fn tail_secs(&self) -> f32 {
        (taps(quality_of(&self.values), self.sample_rate) / 2) as f32 / self.sample_rate
    }

    fn set_offline(&mut self, offline: bool) {
        self.offline = offline;
    }
}

#[cfg(test)]
mod tests {
    use super::super::chorus::tests::{CTX, SR};
    use super::*;

    fn noise(len: usize) -> Vec<f32> {
        let mut s = 0x9e37_79b9u32;
        (0..len)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.5
            })
            .collect()
    }

    fn offline(values: &[(u32, f32)]) -> LinearPhaseEq {
        let mut e = LinearPhaseEq::new(SR);
        e.set_offline(true);
        for &(id, v) in values {
            e.set_param(id, v);
        }
        e.reset();
        e
    }

    fn run(e: &mut LinearPhaseEq, l: &mut [f32], r: &mut [f32], block: usize) {
        for (bl, br) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
            e.process(bl, br, None, &CTX);
        }
    }

    /// The impulse response, `len` samples.
    fn impulse(e: &mut LinearPhaseEq, len: usize) -> Vec<f32> {
        let mut l = vec![0.0f32; len];
        l[0] = 1.0;
        let mut r = l.clone();
        run(e, &mut l, &mut r, 512);
        l
    }

    /// The response at `f` Hz of an impulse response, delayed back by `d` frames: magnitude in
    /// dB and the phase left over (zero for a pure delay of `d`).
    fn response(h: &[f32], f: f64, d: usize) -> (f64, f64) {
        let w = std::f64::consts::TAU * f / SR as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (n, &x) in h.iter().enumerate() {
            let ph = -w * (n as f64 - d as f64);
            re += x as f64 * ph.cos();
            im += x as f64 * ph.sin();
        }
        (10.0 * (re * re + im * im).log10(), im.atan2(re))
    }

    #[test]
    fn quality_sets_length_and_latency() {
        assert_eq!(taps(0, 48_000.0), 2047);
        assert_eq!(taps(1, 44_100.0), 4095);
        assert_eq!(taps(2, 96_000.0), 16_383);
        let mut e = LinearPhaseEq::new(SR);
        assert_eq!(e.latency(), 2047);
        e.set_param(QUALITY, 0.0);
        assert_eq!(e.latency(), 1023);
        e.set_param(QUALITY, 2.0);
        assert_eq!(e.latency(), 4095);
    }

    #[test]
    fn flat_is_a_pure_delay() {
        for quality in 0..3 {
            let mut e = offline(&[(QUALITY, quality as f32)]);
            let d = e.latency();
            let x = noise(30_000);
            let (mut l, mut r) = (x.clone(), x.clone());
            run(&mut e, &mut l, &mut r, 333);
            let err = (d..x.len()).map(|n| (l[n] - x[n - d]).abs()).fold(0.0f32, f32::max);
            assert!(err < 1e-6, "quality {quality}: error {err}");
            assert!(l[..d].iter().all(|v| v.abs() < 1e-6));
        }
    }

    #[test]
    fn a_bell_hits_its_gain_with_linear_phase() {
        let mut e = offline(&[(freq(4), 1000.0), (gain(4), 6.0), (q(4), 1.1)]);
        let d = e.latency();
        let h = impulse(&mut e, 2 * d + 1);
        let (g, _) = response(&h, 1000.0, d);
        assert!((g - 6.0).abs() < 0.1, "gain at 1 kHz {g}");
        // Symmetric about the centre: constant group delay.
        for k in 1..=d {
            assert!((h[d - k] - h[d + k]).abs() < 1e-6, "tap ±{k}: {} {}", h[d - k], h[d + k]);
        }
        // So the phase, once the delay is taken out, is zero (or π where the gain would be
        // negative, which a bell never is) at every frequency.
        for f in [50.0, 300.0, 1000.0, 2500.0, 8000.0, 15000.0] {
            let (_, ph) = response(&h, f, d);
            assert!(ph.abs() < 1e-3, "phase at {f} Hz: {ph}");
        }
        // And it follows the Channel EQ away from the bell.
        let c = crate::eq::coefficients(&channel_eq(|id| e.param(id)), SR as f64);
        for f in [100.0, 500.0, 2000.0, 5000.0] {
            let want = 20.0 * c.magnitude(std::f64::consts::TAU * f / SR as f64).log10();
            let (got, _) = response(&h, f, d);
            assert!((got - want).abs() < 0.1, "{f} Hz: {got} vs {want}");
        }
    }

    #[test]
    fn output_gain_alone_stays_exact() {
        let mut e = offline(&[(OUTPUT, -6.0)]);
        let d = e.latency();
        let h = impulse(&mut e, d + 10);
        assert!((h[d] - util::db_to_gain(-6.0)).abs() < 1e-6);
    }

    #[test]
    fn changes_while_playing_fade_without_clicks() {
        // Playing live: the worker designs the new kernel and the convolver fades to it.
        let mut e = LinearPhaseEq::new(SR);
        let f = 1000.0;
        let amp = 0.5f32;
        let mut t = 0usize;
        let mut sine = |n: usize| -> Vec<f32> {
            let v = (t..t + n).map(|i| amp * (std::f32::consts::TAU * f * i as f32 / SR).sin()).collect();
            t += n;
            v
        };
        let mut out = Vec::new();
        let mut block = |e: &mut LinearPhaseEq, out: &mut Vec<f32>| {
            let mut l = sine(256);
            let mut r = l.clone();
            e.process(&mut l, &mut r, None, &CTX);
            out.extend_from_slice(&l);
        };
        for _ in 0..40 {
            block(&mut e, &mut out);
        }
        e.set_param(freq(4), f);
        e.set_param(gain(4), 12.0);
        // Give the worker time between blocks, as real time would.
        for _ in 0..60 {
            block(&mut e, &mut out);
            std::thread::sleep(Duration::from_millis(2));
        }
        let end_peak = out[out.len() - 2000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((end_peak / amp - 4.0).abs() < 0.1, "the boost arrived: peak {end_peak}");
        // The largest step of the boosted sine is 2π·f/sr of its peak; a hard switch from the
        // old level to the new would step by up to 1.5.
        let biggest = 4.0 * amp * std::f32::consts::TAU * f / SR;
        let step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(step < biggest * 1.1, "largest step {step} (the sine's own: {biggest})");
    }

    #[test]
    fn an_instance_starts_at_its_saved_values() {
        // As the session makes one: no waiting for the worker once it plays.
        let info = info();
        let mut b = crate::dsp::BuiltinDsp::new(&info, SR, &[(gain(4), 12.0), (freq(4), 1000.0)], None);
        let sine: Vec<f32> = (0..8192).map(|i| 0.25 * (std::f32::consts::TAU * 1000.0 * i as f32 / SR).sin()).collect();
        let (mut l, mut r) = (sine.clone(), sine);
        for (bl, br) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            b.process(bl, br, None, &CTX);
        }
        let peak = l[6000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!((peak / 0.25 - 4.0).abs() < 0.1, "peak {peak}");
    }

    #[test]
    fn presets_and_extremes_stay_finite() {
        for p in PRESETS {
            let mut e = offline(p.values);
            let (mut l, mut r) = (noise(20_000), noise(20_000));
            run(&mut e, &mut l, &mut r, 512);
            assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 4.0), "{}", p.name);
        }
        // Every band at full boost, narrow, with both cuts at 48 dB/oct.
        let mut vals = vec![(OUTPUT, 24.0), (QUALITY, 0.0)];
        for b in 0..BANDS {
            vals.push((on(b), 1.0));
            vals.push((q(b), 18.0));
            if is_cut(b) {
                vals.push((slope(b), 2.0));
            } else {
                vals.push((gain(b), 24.0));
            }
        }
        let mut e = offline(&vals);
        let (mut l, mut r) = (noise(20_000), noise(20_000));
        run(&mut e, &mut l, &mut r, 1024);
        assert!(l.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn params_match_the_channel_eq_defaults() {
        let eq = channel_eq(|id| PARAMS.iter().find(|p| p.id == id).map(|p| p.default).unwrap_or(0.0));
        let def = ChannelEq::default();
        assert_eq!(serde_json::to_string(&eq).unwrap(), serde_json::to_string(&def).unwrap());
        assert_eq!(PARAMS.len(), 8 * 4 + 2);
    }
}
