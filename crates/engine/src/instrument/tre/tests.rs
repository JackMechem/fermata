//! Fermata Tre measured through its public contract.
use super::*;
use crate::synth_dsp::test_util::*;
use crate::synth_dsp::{note_hz, tanh_fast};

const SR: f32 = 48000.0;

fn ctx(sr: f32) -> FxContext {
    FxContext { sample_rate: sr, tempo: 120.0, playing: true, position: 0.0 }
}

fn inst(sr: f32, values: &[(u32, f32)]) -> Tre {
    let mut t = Tre::new(sr, 77);
    for &(id, v) in values {
        t.set_param(id, v);
    }
    t.reset();
    t
}

fn on(note: u8, offset: u32) -> MidiEvent {
    MidiEvent { offset, data: [0x90, note, 100] }
}

fn off(note: u8, offset: u32) -> MidiEvent {
    MidiEvent { offset, data: [0x80, note, 0] }
}

/// Renders `frames`, with events at absolute frames, split into blocks of `block`.
fn render(t: &mut Tre, frames: usize, block: usize, events: &[(usize, [u8; 3])]) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0.0f32; frames], vec![0.0f32; frames]);
    let sr = t.patch.rate / t.os as f32;
    let mut at = 0;
    while at < frames {
        let n = block.min(frames - at);
        let midi: Vec<MidiEvent> = events.iter().filter(|(f, _)| *f >= at && *f < at + n).map(|(f, d)| MidiEvent { offset: (*f - at) as u32, data: *d }).collect();
        t.process(&mut l[at..at + n], &mut r[at..at + n], &midi, &ctx(sr));
        at += n;
    }
    (l, r)
}

/// One note's settings where the oscillators' pitch is plain to measure: a bare sine-like
/// triangle through a filter opened wide, no drift.
fn clean_osc(wave: f32, range: f32) -> Vec<(u32, f32)> {
    vec![(DRIFT, 0.0), (WAVE1, wave), (RANGE1, range), (ON2, 0.0), (ON3, 0.0), (LEVEL1, 3.0), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_ATTACK, 1.0), (A_SUSTAIN, 10.0), (TUNE2, 0.0), (TUNE3, 0.0)]
}

#[test]
fn tuned_within_a_cent_across_the_range_at_each_rate() {
    for &sr in &[44100.0f32, 48000.0, 96000.0] {
        let mut worst = 0.0f32;
        for (note, range) in [(24u8, 3.0f32), (36, 3.0), (48, 3.0), (60, 3.0), (69, 3.0), (84, 3.0), (96, 3.0), (108, 3.0), (60, 1.0), (60, 5.0)] {
            let mut t = inst(sr, &clean_osc(0.0, range));
            let n = 1 << 18;
            let (l, _) = render(&mut t, n, 512, &[(0, [0x90, note, 100])]);
            // Worked out exactly, not through the fast math under test.
            let want = (440.0 * 2f64.powf((note as f64 + super::range_st(range) as f64 - 69.0) / 12.0)) as f32;
            let got = pitch_hz(&l, sr);
            let c = cents(got, want);
            worst = worst.max(c.abs());
            assert!(c.abs() < 1.0, "note {note} range {range} at {sr}: {got} Hz vs {want} Hz, {c} cents");
        }
        println!("tuning at {sr}: worst {worst:.3} cents");
    }
}

#[test]
fn osc_2_and_3_knobs_and_keyboard_switch_do_what_they_say() {
    // Osc 2 alone, 7 semitones up.
    let mut t = inst(SR, &[(DRIFT, 0.0), (ON1, 0.0), (WAVE2, 0.0), (RANGE2, 3.0), (TUNE2, 7.0), (LEVEL2, 3.0), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_SUSTAIN, 10.0)]);
    let (l, _) = render(&mut t, 1 << 17, 512, &[(0, [0x90, 60, 100])]);
    assert!(cents(pitch_hz(&l, SR), note_hz(67.0)).abs() < 1.0);
    // Osc 3 off the keyboard: the same pitch from any key.
    let settings = [(DRIFT, 0.0), (ON1, 0.0), (ON2, 0.0), (ON3, 1.0), (WAVE3, 0.0), (RANGE3, 3.0), (TUNE3, 0.0), (OSC3_KBD, 0.0), (LEVEL3, 3.0), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_SUSTAIN, 10.0)];
    for note in [36u8, 60, 90] {
        let mut t = inst(SR, &settings);
        let (l, _) = render(&mut t, 1 << 17, 512, &[(0, [0x90, note, 100])]);
        assert!(cents(pitch_hz(&l, SR), note_hz(60.0)).abs() < 1.0, "from key {note}");
    }
}

#[test]
fn self_oscillating_filter_tracks_the_keyboard_in_tune() {
    for &sr in &[44100.0f32, 48000.0, 96000.0] {
        let mut worst = 0.0f32;
        for note in (36..=96).step_by(12) {
            // No oscillators: the filter is the sound. Cutoff at middle C, full keyboard tracking.
            let mut t = inst(sr, &[(DRIFT, 0.0), (ON1, 0.0), (ON2, 0.0), (CUTOFF, note_hz(60.0)), (EMPHASIS, 10.0), (CONTOUR, 0.0), (KBD1, 1.0), (KBD2, 1.0), (A_SUSTAIN, 10.0)]);
            let (l, _) = render(&mut t, 1 << 18, 512, &[(0, [0x90, note as u8, 100])]);
            let peak = l[1 << 17..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.02, "note {note} at {sr}: silent ({peak})");
            let c = cents(pitch_hz(&l, sr), note_hz(note as f32));
            worst = worst.max(c.abs());
            assert!(c.abs() < 5.0, "note {note} at {sr}: {c} cents");
        }
        println!("self-oscillation tracking at {sr}, notes 36..96: worst {worst:.2} cents");
    }
}

#[test]
fn aliasing_of_a_bright_note_end_to_end() {
    // A saw high up with the filter wide open, through the mixer's saturation and the ladder.
    const N: usize = 1 << 16;
    let note = 96u8; // about 2093 Hz
    let mut t = inst(SR, &[(DRIFT, 0.0), (WAVE1, 2.0), (RANGE1, 3.0), (ON2, 0.0), (LEVEL1, 7.0), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_SUSTAIN, 10.0)]);
    let (l, _) = render(&mut t, 2 * N, 512, &[(0, [0x90, note, 100])]);
    // Blackman-Harris, so nothing leaks: its side lobes are at -92 dB and it's 4 bins wide.
    let tail: Vec<f32> = l[N..].iter().enumerate().map(|(i, v)| {
        let a = std::f64::consts::TAU * i as f64 / N as f64;
        v * (0.35875 - 0.48829 * a.cos() + 0.14128 * (2.0 * a).cos() - 0.01168 * (3.0 * a).cos()) as f32
    }).collect();
    let s = spectrum(&tail);
    let f0 = note_hz(note as f32) as f64;
    let bin_hz = SR as f64 / N as f64;
    // Everything further than a few bins from a harmonic, in the audio band.
    let worst = s.iter().enumerate().filter(|(k, _)| {
        let hz = *k as f64 * bin_hz;
        let h = (hz / f0).round() * f0;
        (hz - h).abs() > 8.0 * bin_hz && hz > 30.0 && hz < 20000.0
    }).map(|(_, v)| *v).fold(0.0, f64::max);
    let peak = s.iter().copied().fold(0.0, f64::max);
    let a = db(worst / peak);
    println!("end-to-end aliasing, note 96 saw, filter open: {a:.1} dB");
    assert!(a < -55.0, "{a}");
}

#[test]
fn no_nan_or_runaway_on_extreme_settings() {
    let extremes: Vec<Vec<(u32, f32)>> = vec![
        // Everything up, feedback screaming.
        vec![(ON3, 1.0), (NOISE_ON, 1.0), (FEEDBACK_ON, 1.0), (LEVEL1, 10.0), (LEVEL2, 10.0), (LEVEL3, 10.0), (NOISE, 10.0), (FEEDBACK, 10.0), (CUTOFF, 20000.0), (EMPHASIS, 10.0), (CONTOUR, 10.0), (VOLUME, 10.0), (DRIFT, 1.0)],
        // Filter at the bottom with full resonance and audio-rate cutoff modulation.
        vec![(CUTOFF, 20.0), (EMPHASIS, 10.0), (FILTER_MOD, 1.0), (OSC_MOD, 1.0), (MOD_AMOUNT, 10.0), (MOD_MIX, 5.0), (RANGE3, 5.0), (WAVE3, 2.0), (VOLUME, 10.0)],
        // Every voice at once, hard unison.
        vec![(VOICE_MODE, 1.0), (VOICES, 4.0), (UNISON_DETUNE, 1.0), (SPREAD, 1.0), (FEEDBACK_ON, 1.0), (FEEDBACK, 10.0), (EMPHASIS, 10.0), (VOLUME, 10.0)],
        vec![(VOICE_MODE, 2.0), (VOICES, 4.0), (F_ATTACK, 1.0), (F_DECAY, 4.0), (F_SUSTAIN, 10.0), (A_ATTACK, 1.0), (A_DECAY, 4.0), (A_SUSTAIN, 10.0), (GLIDE_ON, 1.0), (GLIDE, 1.0), (BEND_RANGE, 12.0), (VOLUME, 10.0)],
    ];
    for (k, settings) in extremes.iter().enumerate() {
        let mut t = inst(SR, settings);
        let mut events = Vec::new();
        for i in 0..80 {
            events.push((i * 1100, [0x90, (10 + (i * 19) % 108) as u8, 127]));
            events.push((i * 1100 + 900, [0x80, (10 + ((i + 41) * 19) % 108) as u8, 0]));
            events.push((i * 1100 + 500, [0xE0, (i % 128) as u8, (i * 3 % 128) as u8]));
            events.push((i * 1100 + 600, [0xB0, 1, (i * 5 % 128) as u8]));
        }
        events.sort_by_key(|e| e.0);
        let (l, r) = render(&mut t, 96000, 333, &events);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| {
            assert!(v.is_finite(), "case {k}");
            m.max(v.abs())
        });
        println!("extreme case {k}: peak {peak:.2}");
        // Well below the engine's containment limit, and worth knowing: the top of the knobs is loud.
        assert!(peak < 16.0, "case {k}: {peak}");
    }
}

#[test]
fn stays_clean_through_extreme_settings_changed_mid_note() {
    let mut t = inst(SR, &[]);
    let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
    t.process(&mut l, &mut r, &[on(36, 0)], &ctx(SR));
    for block in 0..400 {
        // Every parameter jumps between its ends.
        for p in PARAMS {
            let v = if (block + p.id) % 2 == 0 { p.min } else { p.max };
            t.set_param(p.id, v);
        }
        t.process(&mut l, &mut r, &[on(36 + (block % 60) as u8, 7)], &ctx(SR));
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 32.0), "block {block}");
    }
}

#[test]
fn output_doesnt_depend_on_the_block_size() {
    let settings: Vec<(u32, f32)> = vec![(ON3, 1.0), (NOISE_ON, 1.0), (NOISE, 2.0), (GLIDE_ON, 1.0), (FILTER_MOD, 1.0), (OSC_MOD, 1.0), (RANGE3, 0.0), (OSC3_KBD, 0.0), (DRIFT, 1.0), (MOD_AMOUNT, 3.0)];
    let mut events = Vec::new();
    for i in 0..30usize {
        events.push((i * 3333 + 11, [0x90, (40 + (i * 7) % 40) as u8, 100]));
        events.push((i * 3333 + 3000, [0x80, (40 + (i * 7) % 40) as u8, 0]));
        events.push((i * 3333 + 1500, [0xE0, 10, (50 + i % 30) as u8]));
        events.push((i * 3333 + 2000, [0xB0, 1, (i * 4 % 128) as u8]));
    }
    events.sort_by_key(|e| e.0);
    let run = |block| {
        let mut t = inst(SR, &settings);
        render(&mut t, 120_000, block, &events)
    };
    assert!(!events.is_empty());
    let a = run(512);
    for block in [1usize, 37, 128, 1024] {
        assert!(a == run(block), "block size {block} changed the output");
    }
    assert!(a.0.iter().any(|v| v.abs() > 0.01));
}

#[test]
fn renders_repeat_exactly_after_a_reset_and_an_instance_differs_by_seed() {
    let settings = [(DRIFT, 1.0), (NOISE_ON, 1.0), (NOISE, 1.0), (ON3, 1.0)];
    let mut t = inst(SR, &settings);
    let events = [(0usize, [0x90, 48, 100]), (20000, [0x80, 48, 0]), (24000, [0x90, 55, 100]), (40000, [0xB0, 1, 90]), (41000, [0xE0, 0, 90]), (42000, [0xB0, 64, 127])];
    let a = render(&mut t, 48000, 512, &events);
    t.reset();
    let b = render(&mut t, 48000, 512, &events);
    assert!(a == b, "a reset must put everything back");
    // Another instance with the same seed is the same.
    let mut same = inst(SR, &settings);
    assert!(a == render(&mut same, 48000, 512, &events));
    // Another seed isn't.
    let mut other = Tre::new(SR, 78);
    for &(id, v) in &settings {
        other.set_param(id, v);
    }
    other.reset();
    assert!(a != render(&mut other, 48000, 512, &events));
}

/// The largest step between samples within a span, against what the tone does by itself.
fn biggest_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()))
}

/// A dark steady tone whose steps are small, so anything sudden shows.
fn dark() -> Vec<(u32, f32)> {
    vec![(DRIFT, 0.0), (WAVE1, 0.0), (ON2, 0.0), (LEVEL1, 6.0), (CUTOFF, 800.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_ATTACK, 5.0), (A_SUSTAIN, 10.0)]
}

#[test]
fn no_click_on_note_off_with_the_decay_switch_off() {
    let mut s = dark();
    s.push((DECAY_ON, 0.0));
    let mut t = inst(SR, &s);
    let (l, _) = render(&mut t, 48000, 512, &[(0, [0x90, 60, 100]), (24000, [0x80, 60, 0])]);
    let steady = biggest_step(&l[12000..24000]);
    let around = biggest_step(&l[23990..24600]);
    println!("note-off: steady step {steady:.5}, around the release {around:.5}");
    assert!(around <= steady * 1.25, "{around} vs {steady}");
    // And it does end soon: it's the fast release.
    assert!(l[25500..].iter().all(|v| v.abs() < 1e-3), "still sounding");
}

#[test]
fn no_click_on_a_voice_steal_or_a_new_note_over_an_old_one() {
    // A full poly bank, each new note stealing the oldest.
    let mut s = dark();
    s.extend([(VOICE_MODE, 2.0), (VOICES, 0.0), (A_ATTACK, 2.0), (WAVE1, 0.0)]);
    let mut t = inst(SR, &s);
    let (l, _) = render(&mut t, 96000, 512, &[(0, [0x90, 48, 100]), (100, [0x90, 55, 100]), (36000, [0x90, 60, 100]), (72000, [0x90, 67, 100])]);
    let steady = biggest_step(&l[24000..36000]);
    let at_steal = biggest_step(&l[35990..37000]);
    println!("voice steal: steady step {steady:.5}, around the steal {at_steal:.5}");
    // Allow for the new, higher note's own steeper slope.
    assert!(at_steal < steady * 2.0, "{at_steal} vs {steady}");
    // Mono: a note over a released one.
    let mut t = inst(SR, &dark());
    let (l, _) = render(&mut t, 48000, 512, &[(0, [0x90, 48, 100]), (20000, [0x80, 48, 0]), (20200, [0x90, 48, 100])]);
    let steady = biggest_step(&l[10000..20000]);
    let at = biggest_step(&l[19990..21000]);
    assert!(at <= steady * 1.25, "{at} vs {steady}");
}

#[test]
fn moving_a_knob_mid_note_doesnt_zipper() {
    // Jumps of the cutoff, volume and a mixer level, in the worst way: a step per block.
    let mut t = inst(SR, &dark());
    let (mut l, mut r) = (vec![0.0f32; 96000], vec![0.0f32; 96000]);
    t.process(&mut l[..512], &mut r[..512], &[on(48, 0)], &ctx(SR));
    let mut at = 512;
    while at + 512 <= 96000 {
        if at > 48000 {
            let k = ((at / 512) % 4) as f32;
            t.set_param(VOLUME, 5.0 + k);
            t.set_param(CUTOFF, 600.0 + 100.0 * k);
            t.set_param(LEVEL1, 5.0 + 0.5 * k);
        }
        t.process(&mut l[at..at + 512], &mut r[at..at + 512], &[], &ctx(SR));
        at += 512;
    }
    let steady = biggest_step(&l[24000..48000]);
    let moving = biggest_step(&l[48000..at]);
    println!("knob steps: steady step {steady:.5}, while moving {moving:.5}");
    assert!(moving < steady * 3.0, "{moving} vs {steady}");
}

#[test]
fn contours_take_their_times_and_decay_switch_works() {
    // The loudness contour through a wide open filter: measured on the signal's envelope.
    let level = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let s = |decay_on: f32| vec![(DRIFT, 0.0), (WAVE1, 3.0), (ON2, 0.0), (LEVEL1, 3.0), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_ATTACK, 100.0), (A_DECAY, 200.0), (A_SUSTAIN, 5.0), (DECAY_ON, decay_on)];
    let mut t = inst(SR, &s(1.0));
    let sec = |t: f32| (t * SR) as usize;
    let (l, _) = render(&mut t, sec(1.5), 512, &[(0, [0x90, 60, 100]), (sec(0.8), [0x80, 60, 0])]);
    let span = |from: f32| level(&l[sec(from)..sec(from) + 512]);
    let peak = span(0.098);
    let sustain = span(0.7);
    assert!((sustain / peak - 0.5).abs() < 0.03, "{}", sustain / peak);
    // Half way up the attack, an RC charge is past half.
    let half = span(0.05) / peak;
    assert!((0.6..0.8).contains(&half), "attack shape {half}");
    // 95 % of the way from full to the sustain 200 ms after the peak.
    let d = span(0.1 + 0.2) / peak;
    assert!((d - (0.5 + 0.05 * 0.5)).abs() < 0.04, "decay {d}");
    // The release takes the Decay time, with the switch on.
    let rel = span(0.8 + 0.2) / sustain;
    assert!((rel - 0.05).abs() < 0.03, "release {rel}");
    // With it off, the same note ends at once.
    let mut t = inst(SR, &s(0.0));
    let (l, _) = render(&mut t, sec(1.5), 512, &[(0, [0x90, 60, 100]), (sec(0.8), [0x80, 60, 0])]);
    assert!(level(&l[sec(0.84)..]) < 1e-3, "{}", level(&l[sec(0.84)..]));
}

#[test]
fn low_note_priority_single_trigger_by_default() {
    // Holding a low note, a higher key changes nothing, and neither do its contours restart.
    let s = [(DRIFT, 0.0), (WAVE1, 0.0), (ON2, 0.0), (LEVEL1, 3.0), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_SUSTAIN, 10.0)];
    let mut t = inst(SR, &s);
    assert_eq!(t.param(RANGE1), 3.0, "8' by default");
    let (l, _) = render(&mut t, 1 << 18, 512, &[(0, [0x90, 48, 100]), (2000, [0x90, 60, 100])]);
    assert!(cents(pitch_hz(&l, SR), note_hz(48.0)).abs() < 1.0);
    // Let the low one go: the voice moves to the one still held, without a new contour.
    let mut t = inst(SR, &[(DRIFT, 0.0), (WAVE1, 0.0), (ON2, 0.0), (LEVEL1, 3.0), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_SUSTAIN, 5.0), (A_DECAY, 100.0)]);
    let (l, _) = render(&mut t, 1 << 18, 512, &[(0, [0x90, 48, 100]), (2000, [0x90, 60, 100]), (40000, [0x80, 48, 0])]);
    assert!(cents(pitch_hz(&l, SR), note_hz(60.0)).abs() < 1.0);
    let before = l[30000..39000].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let after = l[40000..52000].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(after < before * 1.1, "no new attack: {after} vs {before}");
}

#[test]
fn glide_slides_the_pitch_over_its_time() {
    let s = [(DRIFT, 0.0), (WAVE1, 0.0), (ON2, 0.0), (LEVEL1, 3.0), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_SUSTAIN, 10.0), (GLIDE_ON, 1.0), (GLIDE, 500.0), (PRIORITY, 1.0)];
    let mut t = inst(SR, &s);
    // Two octaves up, legato: measure how far it got by the zero crossings' rate.
    let (l, _) = render(&mut t, 96000, 512, &[(0, [0x90, 48, 100]), (24000, [0x90, 72, 100])]);
    let pitch_in = |from: usize, len: usize| {
        let x = &l[from..from + len];
        let ups = x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        ups as f32 * SR / len as f32
    };
    let before = pitch_in(20000, 4000);
    let mid = pitch_in(24000 + 3000, 3000);
    let end = pitch_in(70000, 8000);
    assert!(cents(before, note_hz(48.0)).abs() < 30.0, "{before}");
    assert!(cents(end, note_hz(72.0)).abs() < 30.0, "{end}");
    assert!(mid > before * 1.3 && mid < end * 0.9, "in between: {before} {mid} {end}");
    // Without glide it jumps.
    let mut t = inst(SR, &[(DRIFT, 0.0), (WAVE1, 0.0), (ON2, 0.0), (LEVEL1, 3.0), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_SUSTAIN, 10.0), (PRIORITY, 1.0)]);
    let (l, _) = render(&mut t, 96000, 512, &[(0, [0x90, 48, 100]), (24000, [0x90, 72, 100])]);
    let ups = l[24100..27100].windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    assert!(cents(ups as f32 * SR / 3000.0, note_hz(72.0)).abs() < 40.0, "{ups}");
}

#[test]
fn osc_3_modulates_the_oscillators_and_the_filter_when_the_wheel_says_so() {
    let base = vec![(DRIFT, 0.0), (WAVE1, 0.0), (ON2, 0.0), (LEVEL1, 3.0), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_SUSTAIN, 10.0), (RANGE3, 0.0), (OSC3_KBD, 0.0), (WAVE3, 0.0), (OSC_MOD, 1.0)];
    let spread = |wheel: u8| {
        let mut t = inst(SR, &base);
        let (l, _) = render(&mut t, 96000, 512, &[(0, [0xB0, 1, wheel]), (1, [0x90, 60, 100])]);
        // Crossings per slice: the pitch's range.
        let slices: Vec<usize> = l[24000..].chunks(1200).map(|c| c.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()).collect();
        (*slices.iter().max().unwrap(), *slices.iter().min().unwrap())
    };
    let (hi0, lo0) = spread(0);
    assert!(hi0 - lo0 <= 1, "no wheel, no vibrato: {hi0} {lo0}");
    let (hi1, lo1) = spread(127);
    assert!(hi1 > lo1 * 2, "full wheel: {hi1} {lo1}");
    // The modulation doesn't reach the filter with its switch off, and does with it on.
    let filt = |sw: f32| {
        let mut b = base.clone();
        b.extend([(OSC_MOD, 0.0), (FILTER_MOD, sw), (CUTOFF, 400.0), (WAVE1, 2.0)]);
        let mut t = inst(SR, &b);
        let (l, _) = render(&mut t, 96000, 512, &[(0, [0xB0, 1, 127]), (1, [0x90, 60, 100])]);
        let peaks: Vec<f32> = l[24000..].chunks(1200).map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs()))).collect();
        peaks.iter().fold(0.0f32, |m, v| m.max(*v)) - peaks.iter().fold(9.0f32, |m, v| m.min(*v))
    };
    assert!(filt(1.0) > filt(0.0) * 2.0, "{} {}", filt(1.0), filt(0.0));
}

#[test]
fn the_mixer_overdrives_as_its_knobs_go_up() {
    // Level grows slower than the knobs once the sum passes where tanh bends.
    let level = |lvl: f32| {
        let mut t = inst(SR, &[(DRIFT, 0.0), (WAVE1, 3.0), (ON2, 0.0), (LEVEL1, lvl), (CUTOFF, 20000.0), (CONTOUR, 0.0), (EMPHASIS, 0.0), (A_SUSTAIN, 10.0)]);
        let (l, _) = render(&mut t, 24000, 512, &[(0, [0x90, 48, 100])]);
        l[12000..].iter().fold(0.0f32, |m, v| m.max(v.abs()))
    };
    let (quiet, mid, full) = (level(1.0), level(5.0), level(10.0));
    assert!(mid / quiet > 3.0, "{mid} {quiet}");
    assert!(full / mid < 1.8, "{full} {mid}");
    assert!((tanh_fast(0.9) - 0.9f32.tanh()).abs() < 1e-3);
}

#[test]
fn state_goes_exactly_to_rest_after_a_note() {
    let mut t = inst(SR, &[(DECAY_ON, 0.0)]);
    let (l, _) = render(&mut t, 96000 * 3, 512, &[(0, [0x90, 60, 100]), (20000, [0x80, 60, 0])]);
    assert!(t.at_rest(), "still running after 3 s");
    assert!(l[96000 * 2..].iter().all(|v| *v == 0.0));
}

#[test]
fn every_factory_preset_plays_at_a_sane_level() {
    assert!((10..=20).contains(&PRESETS.len()), "{}", PRESETS.len());
    let names: std::collections::HashSet<_> = PRESETS.iter().map(|p| p.name).collect();
    assert_eq!(names.len(), PRESETS.len());
    for p in PRESETS {
        let mut t = Tre::new(SR, 5);
        for (id, v) in p.values {
            t.set_param(*id, *v);
        }
        t.reset();
        let events = [(0usize, [0x90u8, 40u8, 110u8]), (24000, [0x80, 40, 0]), (30000, [0x90, 64, 110]), (60000, [0x80, 64, 0])];
        let (l, r) = render(&mut t, 96000, 512, &events);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| {
            assert!(v.is_finite(), "{}", p.name);
            m.max(v.abs())
        });
        let rms = (l.iter().map(|v| v * v).sum::<f32>() / l.len() as f32).sqrt();
        println!("{:24} peak {:6.1} dBFS, rms {:6.1} dBFS", p.name, db(peak as f64), db(rms as f64));
        assert!(peak < 1.6, "{}: too loud, peak {peak}", p.name);
        assert!(rms > 0.005 && peak > 0.03, "{}: too quiet, peak {peak} rms {rms}", p.name);
    }
}

#[test]
fn tail_reaches_the_end_of_the_release() {
    let t = inst(SR, &[(A_DECAY, 1000.0)]);
    assert!((t.tail_secs() - 3.89).abs() < 0.02, "{}", t.tail_secs());
    let t = inst(SR, &[(A_DECAY, 24000.0)]);
    assert!(t.tail_secs() <= 60.1);
    let t = inst(SR, &[(A_DECAY, 24000.0), (DECAY_ON, 0.0)]);
    assert!(t.tail_secs() < 0.2);
}

#[test]
fn unison_and_poly_play_as_many_voices_as_asked() {
    let active = |t: &Tre| t.voices.iter().filter(|v| v.active).count();
    let mut t = inst(SR, &[(VOICE_MODE, 2.0), (VOICES, 2.0)]);
    render(&mut t, 2000, 512, &[(0, [0x90, 40, 100]), (1, [0x90, 44, 100]), (2, [0x90, 47, 100]), (3, [0x90, 50, 100]), (4, [0x90, 54, 100])]);
    assert_eq!(active(&t), 4, "a five-note chord on four voices");
    let mut t = inst(SR, &[(VOICE_MODE, 1.0), (VOICES, 2.0), (SPREAD, 1.0), (UNISON_DETUNE, 1.0)]);
    let (l, r) = render(&mut t, 24000, 512, &[(0, [0x90, 40, 100]), (1, [0x90, 44, 100])]);
    assert_eq!(active(&t), 4, "a unison group of four, one note at a time");
    assert!(l != r, "spread makes it stereo");
    // A mono voice stays centred.
    let mut t = inst(SR, &[]);
    let (l, r) = render(&mut t, 4800, 512, &[(0, [0x90, 40, 100])]);
    assert!(l == r);
}

#[test]
fn switching_the_voice_mode_while_playing_doesnt_strand_a_note() {
    let mut t = inst(SR, &[(DECAY_ON, 0.0)]);
    let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    t.process(&mut l, &mut r, &[on(60, 0)], &ctx(SR));
    t.set_param(VOICE_MODE, 2.0);
    t.process(&mut l, &mut r, &[], &ctx(SR));
    t.process(&mut l, &mut r, &[off(60, 0)], &ctx(SR));
    for _ in 0..1000 {
        t.process(&mut l, &mut r, &[], &ctx(SR));
    }
    assert!(t.voices.iter().all(|v| !v.active));
}
