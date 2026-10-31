//! The measurements every model's tests share: what `tre/tests.rs` does, generic over the
//! instrument, so a model's own `tests.rs` is left with what's specific to it.
use super::{FxContext, Instrument, InstrumentInfo, MidiEvent};
use crate::synth_dsp::test_util::*;

pub fn ctx(sr: f32) -> FxContext {
    FxContext { sample_rate: sr, tempo: 120.0, playing: true, position: 0.0 }
}

pub fn make(info: &InstrumentInfo, sr: f32, seed: u64, values: &[(u32, f32)]) -> Box<dyn Instrument> {
    let mut t = (info.new)(sr, seed);
    for &(id, v) in values {
        t.set_param(id, v);
    }
    t.reset();
    t
}

pub type Event = (usize, [u8; 3]);

/// Renders `frames`, with events at absolute frames, split into blocks of `block`.
pub fn render(t: &mut dyn Instrument, sr: f32, frames: usize, block: usize, events: &[Event]) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0.0f32; frames], vec![0.0f32; frames]);
    let mut at = 0;
    while at < frames {
        let n = block.min(frames - at);
        let midi: Vec<MidiEvent> = events.iter().filter(|(f, _)| *f >= at && *f < at + n).map(|(f, d)| MidiEvent { offset: (*f - at) as u32, data: *d }).collect();
        t.process(&mut l[at..at + n], &mut r[at..at + n], &midi, &ctx(sr));
        at += n;
    }
    (l, r)
}

pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// The largest step between samples.
pub fn biggest_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()))
}

/// Exact, not through the fast math under test.
pub fn exact_hz(note: f64) -> f32 {
    (440.0 * 2f64.powf((note - 69.0) / 12.0)) as f32
}

/// Tuning against the exact frequency at 44.1, 48 and 96 kHz: `settings` is a patch whose output's
/// strongest partial is the fundamental at `note + offset`. Returns the worst error in cents.
pub fn assert_tuned(info: &InstrumentInfo, settings: &[(u32, f32)], notes: &[u8], offset_st: f64, limit: f32) -> f32 {
    let mut worst = 0.0f32;
    for &sr in &[44100.0f32, 48000.0, 96000.0] {
        for &note in notes {
            let mut t = make(info, sr, 77, settings);
            let (l, _) = render(t.as_mut(), sr, 1 << 18, 512, &[(0, [0x90, note, 100])]);
            let want = exact_hz(note as f64 + offset_st);
            let c = cents(pitch_hz(&l, sr), want);
            worst = worst.max(c.abs());
            assert!(c.abs() < limit, "{}: note {note} at {sr}: {c} cents", info.name);
        }
    }
    println!("{} tuning: worst {worst:.3} cents", info.name);
    worst
}

/// The largest thing in the audio band that isn't a harmonic of `f0`, in dB against the largest
/// thing there is, Blackman-Harris windowed (side lobes -92 dB, 4 bins wide). `x` should be a
/// steady tone, a power of 2 long.
pub fn alias_db(x: &[f32], sr: f32, f0: f64) -> f64 {
    let n = x.len();
    let w: Vec<f32> = x
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            v * (0.35875 - 0.48829 * a.cos() + 0.14128 * (2.0 * a).cos() - 0.01168 * (3.0 * a).cos()) as f32
        })
        .collect();
    let s = spectrum(&w);
    let bin_hz = sr as f64 / n as f64;
    let worst = s
        .iter()
        .enumerate()
        .filter(|(k, _)| {
            let hz = *k as f64 * bin_hz;
            let h = (hz / f0).round() * f0;
            (hz - h).abs() > 8.0 * bin_hz && hz > 30.0 && hz < 20000.0
        })
        .map(|(_, v)| *v)
        .fold(0.0, f64::max);
    db(worst / s.iter().copied().fold(0.0, f64::max))
}

/// A busy stretch of notes, bends and wheel for the torture tests.
pub fn torture_events() -> Vec<Event> {
    let mut events = Vec::new();
    for i in 0..80usize {
        events.push((i * 1100, [0x90, (10 + (i * 19) % 108) as u8, 127]));
        events.push((i * 1100 + 900, [0x80, (10 + ((i + 41) * 19) % 108) as u8, 0]));
        events.push((i * 1100 + 500, [0xE0, (i % 128) as u8, (i * 3 % 128) as u8]));
        events.push((i * 1100 + 600, [0xB0, 1, (i * 5 % 128) as u8]));
    }
    events.sort_by_key(|e| e.0);
    events
}

/// No NaN and no runaway through a list of settings, with `torture_events`.
pub fn assert_no_runaway(info: &InstrumentInfo, cases: &[Vec<(u32, f32)>], limit: f32) {
    for &sr in &[44100.0f32, 96000.0] {
        for (k, settings) in cases.iter().enumerate() {
            let mut t = make(info, sr, 77, settings);
            let (l, r) = render(t.as_mut(), sr, 96000, 333, &torture_events());
            let p = l.iter().chain(&r).fold(0.0f32, |m, v| {
                assert!(v.is_finite(), "{}: case {k}", info.name);
                m.max(v.abs())
            });
            println!("{} extreme case {k} at {sr}: peak {p:.2}", info.name);
            assert!(p < limit, "{}: case {k}: {p}", info.name);
        }
    }
}

/// Every parameter jumping between its ends, each block, mid-note.
pub fn assert_survives_param_jumps(info: &InstrumentInfo, limit: f32) {
    let mut t = make(info, 48000.0, 77, &[]);
    let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
    t.process(&mut l, &mut r, &[MidiEvent { offset: 0, data: [0x90, 36, 100] }], &ctx(48000.0));
    for block in 0..400u32 {
        for p in info.params {
            let v = if (block + p.id) % 2 == 0 { p.min } else { p.max };
            t.set_param(p.id, v);
        }
        t.process(&mut l, &mut r, &[MidiEvent { offset: 7, data: [0x90, 36 + (block % 60) as u8, 100] }], &ctx(48000.0));
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < limit), "{}: block {block}: {}", info.name, peak(&l));
    }
}

/// The same MIDI gives the same bits at any block size.
pub fn assert_block_size_independent(info: &InstrumentInfo, settings: &[(u32, f32)], events: &[Event], frames: usize) {
    let run = |block| {
        let mut t = make(info, 48000.0, 77, settings);
        render(t.as_mut(), 48000.0, frames, block, events)
    };
    let a = run(512);
    assert!(peak(&a.0) > 0.005, "{}: silent ({})", info.name, peak(&a.0));
    for block in [1usize, 37, 128, 1024] {
        let b = run(block);
        if a != b {
            let first = a.0.iter().zip(&b.0).position(|(x, y)| x != y);
            panic!("{}: block size {block} changed the output, first at {first:?}", info.name);
        }
    }
}

/// A reset puts everything back; so does a new instance with the same seed. Returns the render.
pub fn assert_repeats(info: &InstrumentInfo, settings: &[(u32, f32)], events: &[Event], frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut t = make(info, 48000.0, 77, settings);
    let a = render(t.as_mut(), 48000.0, frames, 512, events);
    t.reset();
    assert!(a == render(t.as_mut(), 48000.0, frames, 512, events), "{}: a reset must put everything back", info.name);
    let mut same = make(info, 48000.0, 77, settings);
    assert!(a == render(same.as_mut(), 48000.0, frames, 512, events), "{}: same seed", info.name);
    a
}

pub fn assert_seed_matters(info: &InstrumentInfo, settings: &[(u32, f32)], events: &[Event], frames: usize) {
    let mut a = make(info, 48000.0, 77, settings);
    let mut b = make(info, 48000.0, 78, settings);
    assert!(render(a.as_mut(), 48000.0, frames, 512, events) != render(b.as_mut(), 48000.0, frames, 512, events), "{}: another seed should differ", info.name);
}

/// The biggest step around an event against the steady note's, by `ratio`.
pub fn assert_no_click(info: &InstrumentInfo, settings: &[(u32, f32)], events: &[Event], steady: std::ops::Range<usize>, around: std::ops::Range<usize>, ratio: f32, what: &str) {
    let frames = around.end.max(steady.end);
    let mut t = make(info, 48000.0, 77, settings);
    let (l, _) = render(t.as_mut(), 48000.0, frames, 512, events);
    let s = biggest_step(&l[steady]);
    let a = biggest_step(&l[around]);
    println!("{} {what}: steady step {s:.5}, around it {a:.5}, {:.2} times", info.name, a / s);
    assert!(a <= s * ratio, "{}: {what}: {a} vs {s}", info.name);
}

/// Every factory preset plays at a sane level, and the names are unique and in the agreed count.
pub fn assert_presets(info: &InstrumentInfo, notes: [u8; 2]) {
    assert!((12..=20).contains(&info.presets.len()), "{}: {} presets", info.name, info.presets.len());
    let names: std::collections::HashSet<_> = info.presets.iter().map(|p| p.name).collect();
    assert_eq!(names.len(), info.presets.len());
    for p in info.presets {
        let mut t = make(info, 48000.0, 5, p.values);
        let events = [(0usize, [0x90u8, notes[0], 110u8]), (48000, [0x80, notes[0], 0]), (60000, [0x90, notes[1], 110]), (120000, [0x80, notes[1], 0])];
        let (l, r) = render(t.as_mut(), 48000.0, 192000, 512, &events);
        let pk = l.iter().chain(&r).fold(0.0f32, |m, v| {
            assert!(v.is_finite(), "{}", p.name);
            m.max(v.abs())
        });
        let level = rms(&l[..120000]);
        println!("{} {:24} peak {:6.1} dBFS, rms {:6.1} dBFS", info.name, p.name, db(pk as f64), db(level as f64));
        assert!(pk < 1.6, "{}: too loud, peak {pk}", p.name);
        assert!(level > 0.005 && pk > 0.03, "{}: too quiet, peak {pk} rms {level}", p.name);
    }
}

/// The instance goes properly silent: bit zero, some time after the last note.
pub fn assert_goes_silent(info: &InstrumentInfo, settings: &[(u32, f32)], seconds: f32) {
    let mut t = make(info, 48000.0, 77, settings);
    let n = (seconds * 48000.0) as usize;
    let (l, r) = render(t.as_mut(), 48000.0, n, 512, &[(0, [0x90, 60, 100]), (20000, [0x80, 60, 0])]);
    let tail = n * 3 / 4;
    assert!(l[tail..].iter().chain(&r[tail..]).all(|v| *v == 0.0), "{}: not silent after {seconds} s: {}", info.name, peak(&l[tail..]));
}

/// `tail_secs` covers what actually rings on.
pub fn assert_tail_is_long_enough(info: &InstrumentInfo, settings: &[(u32, f32)]) {
    let mut t = make(info, 48000.0, 77, settings);
    let tail = t.tail_secs();
    let on = 4800;
    let n = on + ((tail + 1.0) * 48000.0) as usize;
    let (l, _) = render(t.as_mut(), 48000.0, n, 512, &[(0, [0x90, 60, 100]), (on, [0x80, 60, 0])]);
    let at = on + (tail * 48000.0) as usize;
    let after = peak(&l[at.min(n - 1)..]);
    println!("{}: tail {tail:.2} s leaves {:.1} dB", info.name, db(after as f64));
    assert!(after < 1e-3, "{}: {after} still sounding after tail_secs {tail}", info.name);
}
