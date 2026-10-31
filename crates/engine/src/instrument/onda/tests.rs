//! Fermata Onda measured through its public contract, and what's specific to it: band-limited
//! tables, position scan, unison, the filters and the modulation matrix.
use super::tables::{level_for, TABLES, TOP_HARMONICS};
use super::*;
use crate::instrument::test_kit::*;
use crate::synth_dsp::test_util::*;

const SR: f32 = 48000.0;

fn od() -> InstrumentInfo {
    info()
}

fn with(mut v: Vec<(u32, f32)>, more: &[(u32, f32)]) -> Vec<(u32, f32)> {
    v.extend_from_slice(more);
    v
}

/// A at a table and position, filter off, one copy.
fn plain(table: f32, pos: f32) -> Vec<(u32, f32)> {
    vec![(A_TABLE, table), (A_POS, pos), (FILTER_ON, 0.0), (E1_SUSTAIN, 1.0), (A_LEVEL, 0.5), (VOLUME, 10.0)]
}

fn slot(s: u32, src: f32, dst: f32, amt: f32) -> [(u32, f32); 3] {
    let b = MOD_BASE + 3 * s;
    [(b, src), (b + 1, dst), (b + 2, amt)]
}

#[test]
fn tuned_within_a_cent_across_the_range_at_each_rate() {
    // Position 0 of Basic Shapes is a sine.
    assert_tuned(&od(), &plain(0.0, 0.0), &[24, 36, 48, 60, 69, 84, 96, 108], 0.0, 1.0);
    // A saw.
    assert_tuned(&od(), &plain(0.0, 0.5), &[36, 69], 0.0, 1.0);
    assert_tuned(&od(), &with(plain(0.0, 0.0), &[(A_OCT, 1.0), (A_SEMI, 7.0)]), &[48], 19.0, 1.0);
    assert_tuned(&od(), &with(plain(0.0, 0.0), &[(A_ON, 0.0), (B_ON, 1.0), (B_TABLE, 0.0), (B_POS, 0.0), (B_LEVEL, 0.5), (B_OCT, -1.0)]), &[60], -12.0, 1.0);
}

#[test]
fn the_tables_are_sane_at_every_level() {
    let b = bank();
    for table in 0..TABLES {
        for frame in 0..FRAMES {
            let top = b.cycle(table, frame, 0).samples;
            let peak_top = peak(top);
            assert!(peak_top > 0.9 && peak_top < 1.01, "table {table} frame {frame}: {peak_top}");
            let mean: f32 = top.iter().sum::<f32>() / top.len() as f32;
            assert!(mean.abs() < 1e-3, "table {table} frame {frame} has DC: {mean}");
            for level in 0..tables::LEVELS {
                let c = b.cycle(table, frame, level).samples;
                assert!(c.len().is_power_of_two() && peak(c) < 1.7, "table {table} frame {frame} level {level}: {}", peak(c));
                // Nothing past its harmonics: a spectrum of the cycle itself.
                let s = spectrum(c);
                let limit = TOP_HARMONICS >> level;
                let beyond = s.iter().skip(limit + 1).copied().fold(0.0, f64::max);
                assert!(beyond < 1e-3, "table {table} frame {frame} level {level}: {beyond} past harmonic {limit}");
            }
        }
    }
}

#[test]
fn level_choice_keeps_every_harmonic_under_nyquist() {
    for i in 1..4000 {
        let inc = i as f32 * 1e-4;
        let l = level_for(inc);
        let h = TOP_HARMONICS >> l;
        assert!(h as f32 * inc <= 0.5 + 1e-6 || l == tables::LEVELS - 1, "{inc}: level {l}, {h} harmonics");
        if l > 0 {
            // And it's as many as fit.
            assert!((TOP_HARMONICS >> (l - 1)) as f32 * inc > 0.5, "{inc}: level {l} is lower than need be");
        }
    }
}

#[test]
fn aliasing_is_measured_low_across_the_tables_at_high_notes() {
    let mut worst = -200.0f64;
    for (table, pos, name) in [(0.0, 0.5, "saw"), (0.0, 0.75, "square"), (3.0, 1.0, "narrow pulse"), (4.0, 1.0, "sync"), (5.0, 1.0, "FM"), (6.0, 1.0, "stepped"), (8.0, 1.0, "folded"), (9.0, 0.5, "comb")] {
        for note in [84u8, 96, 108] {
            let mut t = make(&od(), SR, 77, &plain(table, pos));
            let (l, _) = render(t.as_mut(), SR, 1 << 16, 512, &[(0, [0x90, note, 100])]);
            // A sync sweep's top frame at note 108 has all its energy past the audio band: there's
            // nothing there to measure against.
            if rms(&l[1 << 15..]) < 1e-3 {
                println!("aliasing, {name} at note {note}: nothing in the band");
                continue;
            }
            let a = alias_db(&l[1 << 15..], SR, exact_hz(note as f64) as f64);
            worst = worst.max(a);
            println!("aliasing, {name} at note {note}: {a:.1} dB");
            assert!(a < -60.0, "{name} note {note}: {a}");
        }
    }
    println!("worst alias: {worst:.1} dB");
}

#[test]
fn a_position_scan_has_no_zipper() {
    // A slow envelope sweeps the whole table on a low tone. The biggest step in a stretch of the
    // sweep, against the same tone at the positions it passes through, held still.
    let scan_env = |e: f32| vec![(A_TABLE, 0.0), (A_POS, 0.0), (FILTER_ON, 0.0), (E1_SUSTAIN, 1.0), (A_LEVEL, 0.3), (VOLUME, 10.0), (E2_ATTACK, e), (E2_SUSTAIN, 1.0), (MOD_BASE, 1.0), (MOD_BASE + 1, 1.0), (MOD_BASE + 2, 0.5)];
    let run = |s: &[(u32, f32)], n: usize| {
        let mut t = make(&od(), SR, 77, s);
        render(t.as_mut(), SR, n, 512, &[(0, [0x90, 36, 100])]).0
    };
    // From a sine to a triangle (position 0..0.25 is one segment and 0.5's the saw): both are smooth.
    let moving = run(&with(scan_env(800.0), &[(MOD_BASE + 2, 0.25)]), 96000);
    let held = |pos: f32| {
        let x = run(&with(plain(0.0, pos), &[(A_LEVEL, 0.3)]), 24000);
        biggest_step(&x[12000..])
    };
    let steady = held(0.0).max(held(0.25));
    let sweep = biggest_step(&moving[4800..48000]);
    println!("scanning a sine into a triangle: biggest step {sweep:.5}, still at the ends {steady:.5}, {:.2} times", sweep / steady);
    assert!(sweep < steady * 1.3, "{sweep} {steady}");

    // Nor is the frame's own crossfade stepped: in a slow scan the signal moves gradually, so the
    // difference between a cycle and the one a period later stays small and even.
    let n = (SR / exact_hz(36.0)).round() as usize;
    let diffs: Vec<f32> = moving[9600..40000].windows(n + 1).step_by(n).map(|w| (w[n] - w[0]).abs()).collect();
    let max = diffs.iter().fold(0.0f32, |m, v| m.max(*v));
    println!("one cycle to the next at the same phase: largest difference {max:.4}");
    assert!(max < 0.15, "{max}");
}

#[test]
fn modulation_routes_where_a_slot_says_and_nowhere_else() {
    // A base tone we can measure, and a slow LFO for a source.
    let base = plain(0.0, 0.0);
    let measure = |extra: &[(u32, f32)], event: Event| {
        let mut t = make(&od(), SR, 77, &with(base.clone(), extra));
        render(t.as_mut(), SR, 96000, 512, &[(0, [0x90, 69, 100]), event]).0
    };
    let silent_event: Event = (10, [0xB0, 7, 0]);
    let lfo = [(L1_RATE, 4.0), (L1_WAVE, 3.0)];
    // No slot: it's steady.
    let plain = measure(&lfo, silent_event);
    let crossing_spread = |x: &[f32]| {
        let s: Vec<usize> = x[4800..].chunks(1200).map(|c| c.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()).collect();
        s.iter().max().unwrap() - s.iter().min().unwrap()
    };
    assert!(crossing_spread(&plain) <= 1);
    // LFO 1 to A pitch.
    let v: Vec<(u32, f32)> = lfo.iter().copied().chain(slot(0, 3.0, 3.0, 0.1)).collect();
    assert!(crossing_spread(&measure(&v, silent_event)) > 5, "{}", crossing_spread(&measure(&v, silent_event)));
    // The same with the amount at zero, or the destination elsewhere.
    let v: Vec<(u32, f32)> = lfo.iter().copied().chain(slot(0, 3.0, 3.0, 0.0)).collect();
    assert!(crossing_spread(&measure(&v, silent_event)) <= 1);
    let v: Vec<(u32, f32)> = lfo.iter().copied().chain(slot(0, 3.0, 4.0, 0.1)).collect();
    assert!(crossing_spread(&measure(&v, silent_event)) <= 1, "B's pitch isn't A's");
    // LFO 2 to A level.
    let v: Vec<(u32, f32)> = vec![(L2_RATE, 4.0), (L2_WAVE, 3.0)].into_iter().chain(slot(0, 4.0, 5.0, 0.5)).collect();
    let x = measure(&v, silent_event);
    let peaks: Vec<f32> = x[9600..].chunks(600).map(peak).collect();
    let (hi, lo) = (peaks.iter().fold(0.0f32, |m, p| m.max(*p)), peaks.iter().fold(9.0f32, |m, p| m.min(*p)));
    assert!(hi > lo * 2.0, "{hi} {lo}");
    // Env 2 to cutoff, on a saw: the filter opens and shuts by the envelope.
    let s = vec![(FILTER_ON, 1.0), (FILTER_TYPE, 1.0), (CUTOFF, 200.0), (A_POS, 0.5), (E2_DECAY, 300.0), (E2_SUSTAIN, 0.0)];
    let bright = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<f32>>());
    let with_env = measure(&s.iter().copied().chain(slot(0, 1.0, 7.0, 0.8)).collect::<Vec<_>>(), silent_event);
    let without = measure(&s, silent_event);
    assert!(bright(&with_env[1200..3600]) > 3.0 * bright(&without[1200..3600]), "{} {}", bright(&with_env[1200..3600]), bright(&without[1200..3600]));
    assert!((bright(&with_env[60000..]) / bright(&without[60000..]) - 1.0).abs() < 0.2, "back down by the end");
    // Env 3 to A position: from a sine to a saw.
    let s = [(E3_ATTACK, 1.0), (E3_DECAY, 300.0), (E3_SUSTAIN, 0.0)];
    let x = measure(&s.iter().copied().chain(slot(0, 2.0, 1.0, 0.5)).collect::<Vec<_>>(), silent_event);
    assert!(bright(&x[600..1800]) > 2.0 * bright(&x[60000..]), "{} {}", bright(&x[600..1800]), bright(&x[60000..]));
    // The wheel and velocity are sources too.
    let v: Vec<(u32, f32)> = slot(0, 7.0, 1.0, 0.5).to_vec();
    let up = measure(&v, (1, [0xB0, 1, 127]));
    let down = measure(&v, silent_event);
    assert!(bright(&up[48000..]) > 2.0 * bright(&down[48000..]));
    let v: Vec<(u32, f32)> = slot(0, 5.0, 5.0, 1.0).to_vec();
    let lv = |velocity: u8| {
        let mut t = make(&od(), SR, 77, &with(base.clone(), &v));
        rms(&render(t.as_mut(), SR, 24000, 512, &[(0, [0x90, 69, velocity])]).0[12000..])
    };
    assert!(lv(127) > lv(20) * 1.3, "{} {}", lv(127), lv(20));
    // Slots add up.
    let one: Vec<(u32, f32)> = lfo.iter().copied().chain(slot(0, 3.0, 3.0, 0.05)).collect();
    let two: Vec<(u32, f32)> = lfo.iter().copied().chain(slot(0, 3.0, 3.0, 0.05)).chain(slot(1, 3.0, 3.0, 0.05)).collect();
    assert!(crossing_spread(&measure(&two, silent_event)) > crossing_spread(&measure(&one, silent_event)));
}

#[test]
fn unison_is_stereo_detuned_and_keeps_its_level() {
    let u = |n: f32, det: f32| with(plain(0.0, 0.0), &[(A_VOICES, n), (A_DETUNE, det), (A_BLEND, 1.0), (A_SPREAD, 1.0)]);
    let run = |s: &[(u32, f32)]| {
        let mut t = make(&od(), SR, 77, s);
        render(t.as_mut(), SR, 96000, 512, &[(0, [0x90, 57, 100])])
    };
    let (l, r) = run(&u(0.0, 0.0));
    assert!(l == r, "one copy, centred");
    let (l7, r7) = run(&u(6.0, 0.3));
    assert!(l7 != r7);
    let (a, b) = (rms(&l[48000..]), rms(&l7[48000..]));
    println!("one copy rms {a:.3}, seven copies {b:.3}");
    assert!(b / a < 2.0 && b / a > 0.5, "{a} {b}");
    // No detune at all on the centre: a wide stack's beat is there only with detune.
    let beat = |x: &[f32]| {
        let p: Vec<f32> = x[24000..].chunks(600).map(peak).collect();
        p.iter().fold(0.0f32, |m, v| m.max(*v)) - p.iter().fold(9.0f32, |m, v| m.min(*v))
    };
    assert!(beat(&l7) > 0.05, "{}", beat(&l7));
    // Its fundamental's still at the note.
    assert!(cents(pitch_hz(&run(&u(2.0, 0.05)).0, SR), exact_hz(57.0)).abs() < 3.0);
}

#[test]
fn the_filter_types_shape_differently_and_key_track_works() {
    let band = |ty: f32, res: f32, note: u8| {
        let mut t = make(&od(), SR, 77, &[(A_POS, 0.5), (A_LEVEL, 0.5), (FILTER_TYPE, ty), (CUTOFF, 1000.0), (RESONANCE, res), (E1_SUSTAIN, 1.0), (VOLUME, 10.0)]);
        let (l, _) = render(t.as_mut(), SR, 1 << 16, 512, &[(0, [0x90, note, 100])]);
        // Blackman-Harris: a rectangular window's leakage from the low partials would drown what's up there.
        let n = 1usize << 15;
        let w: Vec<f32> = l[n..].iter().enumerate().map(|(i, v)| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            v * (0.35875 - 0.48829 * a.cos() + 0.14128 * (2.0 * a).cos() - 0.01168 * (3.0 * a).cos()) as f32
        }).collect();
        let s = spectrum(&w);
        let bin = |hz: f32| (hz / SR * (1 << 15) as f32) as usize;
        let e = |a: f32, b: f32| db(s[bin(a)..bin(b)].iter().map(|v| v * v).sum::<f64>().sqrt());
        (e(100.0, 400.0), e(800.0, 1250.0), e(5000.0, 15000.0))
    };
    let (ll, lm, lh) = band(0.0, 0.1, 36);
    let (hl, _hm, hh) = band(3.0, 0.1, 36);
    let (bl, bm, bh) = band(2.0, 0.9, 36);
    println!("low {ll:.0} {lm:.0} {lh:.0}, high {hl:.0} {hh:.0}, band {bl:.0} {bm:.0} {bh:.0}");
    assert!(ll > lh + 20.0, "low-pass: {ll} {lh}");
    assert!(hh > hl - 5.0 && hl < ll - 10.0, "high-pass: {hl} {ll}");
    assert!(bm > bl + 6.0 && bm > bh + 6.0, "band-pass: {bl} {bm} {bh}");
    // A notch near its centre takes it out.
    let (_, nm, _) = band(4.0, 0.3, 36);
    let (_, fm, _) = band(0.0, 0.0, 36);
    assert!(nm < fm - 6.0, "notch: {nm} {fm}");
    // 24 is steeper than 12.
    let (_, _, h12) = band(0.0, 0.0, 36);
    let (_, _, h24) = band(1.0, 0.0, 36);
    let (_, _, hlad) = band(5.0, 0.0, 36);
    assert!(h24 < h12 - 10.0 && hlad < h12 - 10.0, "{h12} {h24} {hlad}");
    // Key track moves the filter.
    let cut_of = |track: f32, note: u8| {
        let mut o = Onda::new(SR, 7);
        o.set_param(KEY_TRACK, track);
        o.set_param(E1_SUSTAIN, 1.0);
        render(&mut o, SR, 1024, 512, &[(0, [0x90, note, 100])]);
        o.voices[0].cut
    };
    assert!((cut_of(0.0, 84) - cut_of(0.0, 36)).abs() < 1e-3);
    assert!((cut_of(1.0, 84) - cut_of(1.0, 36) - 48.0).abs() < 0.1, "{}", cut_of(1.0, 84) - cut_of(1.0, 36));
}

#[test]
fn polyphony_and_mono_modes() {
    let active = |t: &Onda| t.voices.iter().filter(|v| v.active).count();
    let mut t = Onda::new(SR, 7);
    t.set_param(POLYPHONY, 16.0);
    let chord: Vec<Event> = (0..20u8).map(|k| (k as usize, [0x90, 40 + k, 100])).collect();
    render(&mut t, SR, 2000, 512, &chord);
    assert_eq!(active(&t), 16);
    let mut t = Onda::new(SR, 7);
    t.set_param(POLYPHONY, 3.0);
    render(&mut t, SR, 2000, 512, &chord);
    assert_eq!(active(&t), 3);
    let mut t = Onda::new(SR, 7);
    t.set_param(VOICE_MODE, 2.0);
    render(&mut t, SR, 2000, 512, &chord);
    assert_eq!(active(&t), 1);
}

#[test]
fn free_lfos_and_tempo_synced_lfos() {
    // A synced LFO lands on the beat wherever playback started.
    let sync_val = |position: f64| {
        let mut t = Onda::new(SR, 7);
        t.set_param(L1_SYNC, 5.0);
        t.set_param(L1_WAVE, 2.0);
        let (mut l, mut r) = ([0.0f32; 16], [0.0f32; 16]);
        let c = FxContext { sample_rate: SR, tempo: 120.0, playing: true, position };
        t.process(&mut l, &mut r, &[], &c);
        t.lfo[0].phase
    };
    assert!((sync_val(10.25) - 0.25).abs() < 1e-6, "{}", sync_val(10.25));
    assert!((sync_val(3.0) - 0.0).abs() < 1e-6);
    // Free: the same at any song position.
    let mut a = Onda::new(SR, 7);
    let (mut l, mut r) = ([0.0f32; 16], [0.0f32; 16]);
    a.process(&mut l, &mut r, &[], &FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: 99.0 });
    assert_eq!(a.lfo[0].phase, 0.0);
    // Synced output nearly agrees across block sizes, and doesn't exactly.
    let s = [(L1_SYNC, 6.0), (L1_WAVE, 0.0), (A_POS, 0.3), (MOD_BASE, 3.0), (MOD_BASE + 1, 7.0), (MOD_BASE + 2, 0.2), (CUTOFF, 900.0)];
    let run = |block: usize| {
        let mut t = make(&od(), SR, 77, &s);
        let n = 48000;
        let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
        let mut at = 0;
        while at < n {
            let m = block.min(n - at);
            let midi = if at == 0 { vec![MidiEvent { offset: 0, data: [0x90, 48, 100] }] } else { vec![] };
            let beats = at as f64 / 24000.0;
            t.process(&mut l[at..at + m], &mut r[at..at + m], &midi, &FxContext { sample_rate: SR, tempo: 120.0, playing: true, position: beats });
            at += m;
        }
        l
    };
    let (x, y) = (run(128), run(512));
    let far = x.iter().zip(&y).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    println!("a tempo-synced LFO across block sizes: largest difference {far:.5}");
    assert!(far < 0.03, "{far}");
}

#[test]
fn no_nan_or_runaway_on_extreme_settings() {
    let mut all_slots: Vec<(u32, f32)> = Vec::new();
    for (k, (s, d)) in [(1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0), (5.0, 7.0), (6.0, 8.0), (7.0, 9.0), (8.0, 10.0)].iter().enumerate() {
        all_slots.extend(slot(k as u32, *s, *d, 1.0));
    }
    let wide = vec![(A_VOICES, 6.0), (B_ON, 1.0), (B_VOICES, 6.0), (A_DETUNE, 1.0), (B_DETUNE, 1.0), (A_LEVEL, 1.0), (B_LEVEL, 1.0), (SUB_LEVEL, 1.0), (NOISE_LEVEL, 1.0), (VOLUME, 10.0), (POLYPHONY, 16.0), (L1_RATE, 40.0), (L2_RATE, 40.0), (L2_WAVE, 4.0)];
    let mut cases: Vec<Vec<(u32, f32)>> = Vec::new();
    for ty in 0..7 {
        cases.push(with(with(wide.clone(), &all_slots), &[(FILTER_TYPE, ty as f32), (RESONANCE, 1.0), (DRIVE, 1.0), (CUTOFF, 20000.0)]));
    }
    cases.push(with(wide.clone(), &[(CUTOFF, 20.0), (RESONANCE, 1.0), (FM_AMOUNT, 1.0), (FILTER_TYPE, 5.0)]));
    assert_no_runaway(&od(), &cases, 16.0);
    assert_survives_param_jumps(&od(), 32.0);
}

fn busy() -> Vec<Event> {
    let mut events = Vec::new();
    for i in 0..30usize {
        events.push((i * 3333 + 11, [0x90, (40 + (i * 7) % 40) as u8, 100]));
        events.push((i * 3333 + 3000, [0x80, (40 + (i * 7) % 40) as u8, 0]));
        events.push((i * 3333 + 1500, [0xE0, 10, (50 + i % 30) as u8]));
        events.push((i * 3333 + 2000, [0xB0, 1, (i * 4 % 128) as u8]));
    }
    events.push((130_000, [0x90, 50, 100]));
    events.sort_by_key(|e| e.0);
    events
}

#[test]
fn output_doesnt_depend_on_the_block_size() {
    let mut s = vec![(A_VOICES, 3.0), (B_ON, 1.0), (B_TABLE, 5.0), (NOISE_LEVEL, 0.1), (SUB_LEVEL, 0.3), (L1_WAVE, 4.0), (L2_RETRIG, 1.0), (E1_RELEASE, 30.0), (FM_AMOUNT, 0.1)];
    s.extend(slot(0, 3.0, 1.0, 0.4));
    s.extend(slot(1, 4.0, 7.0, 0.3));
    s.extend(slot(2, 1.0, 2.0, 0.3));
    assert_block_size_independent(&od(), &s, &busy(), 180_000);
    let s = [(VOICE_MODE, 2.0), (GLIDE, 60.0), (FILTER_TYPE, 6.0)];
    assert_block_size_independent(&od(), &s, &busy(), 180_000);
}

#[test]
fn renders_repeat_and_seed_matters() {
    let s = [(A_VOICES, 4.0), (L1_WAVE, 4.0), (NOISE_LEVEL, 0.2)];
    let events = [(0usize, [0x90, 48, 100]), (20000, [0x80, 48, 0]), (24000, [0x90, 55, 100]), (40000, [0xB0, 1, 90]), (41000, [0xE0, 0, 90]), (42000, [0xB0, 64, 127])];
    assert_repeats(&od(), &s, &events, 96000);
    assert_seed_matters(&od(), &s, &events, 48000);
}

fn dark() -> Vec<(u32, f32)> {
    vec![(A_POS, 0.25), (A_LEVEL, 0.7), (FILTER_TYPE, 1.0), (CUTOFF, 900.0), (E1_ATTACK, 5.0), (E1_SUSTAIN, 1.0), (E1_RELEASE, 12.0), (VOLUME, 10.0)]
}

#[test]
fn no_click_on_note_off_steal_and_knobs() {
    assert_no_click(&od(), &dark(), &[(0, [0x90, 60, 100]), (24000, [0x80, 60, 0])], 12000..24000, 23990..24800, 1.25, "note-off");
    let mut steal: Vec<Event> = (0..4u8).map(|k| (k as usize, [0x90, 48 + k * 2, 100])).collect();
    steal.push((36000, [0x90, 60, 100]));
    assert_no_click(&od(), &with(dark(), &[(POLYPHONY, 4.0)]), &steal, 24000..36000, 35990..37000, 2.0, "voice steal");
    let mut t = make(&od(), SR, 77, &dark());
    let (mut l, mut r) = (vec![0.0f32; 96000], vec![0.0f32; 96000]);
    t.process(&mut l[..512], &mut r[..512], &[MidiEvent { offset: 0, data: [0x90, 48, 100] }], &ctx(SR));
    let mut at = 512;
    while at + 512 <= 96000 {
        if at > 48000 {
            let k = ((at / 512) % 4) as f32;
            t.set_param(VOLUME, 8.0 + k * 0.4);
            t.set_param(CUTOFF, 800.0 + 100.0 * k);
            t.set_param(A_POS, 0.2 + 0.01 * k);
            t.set_param(A_LEVEL, 0.6 + 0.05 * k);
        }
        t.process(&mut l[at..at + 512], &mut r[at..at + 512], &[], &ctx(SR));
        at += 512;
    }
    let steady = biggest_step(&l[24000..48000]);
    let moving = biggest_step(&l[48000..at]);
    println!("knob steps: steady {steady:.5}, moving {moving:.5}");
    assert!(moving < steady * 3.0, "{moving} {steady}");
}

#[test]
fn every_factory_preset_plays_at_a_sane_level() {
    assert_presets(&od(), [40, 64]);
}

#[test]
fn goes_to_rest_and_tail_reaches_the_end() {
    assert_goes_silent(&od(), &[(E1_RELEASE, 30.0)], 2.0);
    assert_tail_is_long_enough(&od(), &[(E1_RELEASE, 800.0)]);
}
