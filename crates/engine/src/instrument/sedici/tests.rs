//! Fermata Sedici measured through its public contract, and what's specific to it: the algorithms
//! against the published diagrams, the envelope and its scalings, and feedback.
use super::*;
use crate::instrument::test_kit::*;
use crate::synth_dsp::test_util::*;

const SR: f32 = 48000.0;

fn sd() -> InstrumentInfo {
    info()
}

fn with(mut v: Vec<(u32, f32)>, more: &[(u32, f32)]) -> Vec<(u32, f32)> {
    v.extend_from_slice(more);
    v
}

fn op(n: u32) -> u32 {
    O1 + (n - 1) * OP_STRIDE
}

/// Operator 1 alone, a sine, at its full sustained level.
fn sine() -> Vec<(u32, f32)> {
    vec![(ALGORITHM, 32.0), (VOLUME, 10.0)]
}

#[test]
fn tuned_within_a_cent_across_the_range_at_each_rate() {
    assert_tuned(&sd(), &sine(), &[24, 36, 48, 60, 69, 84, 96, 108], 0.0, 1.0);
    assert_tuned(&sd(), &with(sine(), &[(GRIT, 0.0)]), &[33, 69], 0.0, 1.0);
    assert_tuned(&sd(), &with(sine(), &[(O1 + COARSE, 2.0)]), &[60], 12.0, 1.0);
    assert_tuned(&sd(), &with(sine(), &[(O1 + COARSE, 0.0)]), &[60], -12.0, 1.0);
    assert_tuned(&sd(), &with(sine(), &[(TRANSPOSE, 7.0)]), &[60], 7.0, 1.0);
    // Fine is linear on the ratio: 1.5 is a just fifth, not quite 7 semitones.
    let want = 12.0 * 1.5f64.log2();
    assert_tuned(&sd(), &with(sine(), &[(O1 + FINE, 50.0)]), &[60], want, 1.0);
}

#[test]
fn fixed_frequency_ignores_the_key() {
    let s = with(sine(), &[(O1 + MODE, 1.0), (O1 + COARSE, 2.0), (O1 + FINE, 30.0)]);
    let want = 100.0 * 10f64.powf(0.3);
    for note in [36u8, 60, 90] {
        let mut t = make(&sd(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 1 << 17, 512, &[(0, [0x90, note, 100])]);
        let c = cents(pitch_hz(&l, SR), want as f32);
        assert!(c.abs() < 1.0, "from key {note}: {c}");
    }
}

#[test]
fn a_lone_sine_is_clean_and_fm_folds_as_the_instrument_does() {
    let measure = |s: &[(u32, f32)], note: u8, sr: f32, n: usize| {
        let mut t = make(&sd(), sr, 77, s);
        let (l, _) = render(t.as_mut(), sr, n, 512, &[(0, [0x90, note, 100])]);
        alias_db(&l[n / 2..], sr, exact_hz(note as f64) as f64)
    };
    let clean = measure(&with(sine(), &[(GRIT, 0.0)]), 84, SR, 1 << 17);
    let gritty = measure(&sine(), 84, SR, 1 << 17);
    println!("sine at note 84: everything else {clean:.1} dB clean, {gritty:.1} dB with the grit on");
    assert!(clean < -75.0, "{clean}");
    assert!(gritty < -50.0, "{gritty}");
    assert!(gritty > clean, "the grit should be measurable");
    // FM up high, where it folds. There's no limit to pass: this is what its rate does, and the
    // figure is reported, not judged.
    let fm = vec![(ALGORITHM, 1.0), (O2 + LEVEL, 90.0), (VOLUME, 10.0), (GRIT, 0.0)];
    let hot = vec![(ALGORITHM, 1.0), (O2 + LEVEL, 99.0), (VOLUME, 10.0), (GRIT, 0.0)];
    for note in [84u8, 96] {
        let a = measure(&hot, note, SR, 1 << 17);
        println!("op 2 into op 1, level 99, at note {note}: folded partials {a:.1} dB");
    }
    for note in [60u8, 84, 96] {
        let a = measure(&fm, note, SR, 1 << 17);
        let b = measure(&fm, note, 96000.0, 1 << 18);
        println!("op 2 into op 1, level 90, at note {note}: folded partials {a:.1} dB at 48 kHz, {b:.1} dB at 96 kHz");
        assert!((a - b).abs() < 6.0 || note < 70, "the folding is the engine's, not the session's: {a} {b}");
    }
}

/// The published diagrams, typed out a second time and differently: `m>c` edges, the carriers and
/// the feedback, one string per algorithm.
const DIAGRAMS: [&str; 32] = [
    "2>1 6>5 5>4 4>3 | 1 3 | 6>6",
    "2>1 6>5 5>4 4>3 | 1 3 | 2>2",
    "3>2 2>1 6>5 5>4 | 1 4 | 6>6",
    "3>2 2>1 6>5 5>4 | 1 4 | 4>6",
    "2>1 4>3 6>5 | 1 3 5 | 6>6",
    "2>1 4>3 6>5 | 1 3 5 | 5>6",
    "2>1 4>3 5>3 6>5 | 1 3 | 6>6",
    "2>1 4>3 5>3 6>5 | 1 3 | 4>4",
    "2>1 4>3 5>3 6>5 | 1 3 | 2>2",
    "3>2 2>1 5>4 6>4 | 1 4 | 3>3",
    "3>2 2>1 5>4 6>4 | 1 4 | 6>6",
    "2>1 4>3 5>3 6>3 | 1 3 | 2>2",
    "2>1 4>3 5>3 6>3 | 1 3 | 6>6",
    "2>1 4>3 5>4 6>4 | 1 3 | 6>6",
    "2>1 4>3 5>4 6>4 | 1 3 | 2>2",
    "2>1 3>1 5>1 4>3 6>5 | 1 | 6>6",
    "2>1 3>1 5>1 4>3 6>5 | 1 | 2>2",
    "2>1 3>1 4>1 5>4 6>5 | 1 | 3>3",
    "3>2 2>1 6>4 6>5 | 1 4 5 | 6>6",
    "3>1 3>2 5>4 6>4 | 1 2 4 | 3>3",
    "3>1 3>2 6>4 6>5 | 1 2 4 5 | 3>3",
    "2>1 6>3 6>4 6>5 | 1 3 4 5 | 6>6",
    "3>2 6>4 6>5 | 1 2 4 5 | 6>6",
    "6>3 6>4 6>5 | 1 2 3 4 5 | 6>6",
    "6>4 6>5 | 1 2 3 4 5 | 6>6",
    "3>2 5>4 6>4 | 1 2 4 | 6>6",
    "3>2 5>4 6>4 | 1 2 4 | 3>3",
    "2>1 4>3 5>4 | 1 3 6 | 5>5",
    "4>3 6>5 | 1 2 3 5 | 6>6",
    "4>3 5>4 | 1 2 3 6 | 5>5",
    "6>5 | 1 2 3 4 5 | 6>6",
    " | 1 2 3 4 5 6 | 6>6",
];

struct Diagram {
    edges: Vec<(u32, u32)>,
    carriers: Vec<u32>,
    fb: (u32, u32),
}

fn parse(s: &str) -> Diagram {
    let parts: Vec<&str> = s.split('|').collect();
    let pair = |x: &str| {
        let (a, b) = x.split_once('>').unwrap();
        (a.parse().unwrap(), b.parse().unwrap())
    };
    Diagram { edges: parts[0].split_whitespace().map(pair).collect(), carriers: parts[1].split_whitespace().map(|x| x.parse().unwrap()).collect(), fb: pair(parts[2].trim()) }
}

#[test]
fn the_table_matches_the_published_diagrams() {
    for (k, text) in DIAGRAMS.iter().enumerate() {
        let d = parse(text);
        let a = &ALGORITHMS[k];
        let mut edges = Vec::new();
        for c in 0..6 {
            for m in 0..a.mod_count[c] as usize {
                edges.push((a.mods[c][m] as u32 + 1, c as u32 + 1));
            }
        }
        let mut want = d.edges.clone();
        want.sort();
        edges.sort();
        assert_eq!(edges, want, "algorithm {}", k + 1);
        let bits: u8 = d.carriers.iter().map(|c| 1u8 << (c - 1)).sum();
        assert_eq!(a.carriers, bits, "algorithm {} carriers", k + 1);
        assert_eq!((a.fb_from as u32 + 1, a.fb_to as u32 + 1), d.fb, "algorithm {} feedback", k + 1);
        // No modulator is lower-numbered: the playing order depends on it.
        for (m, c) in &edges {
            assert!(m > c, "algorithm {}: {m}>{c}", k + 1);
        }
    }
}

/// Plays with just these operators on. Every level is the same, and no envelope moves, so a
/// difference between two renders is a route between them.
fn route(alg: usize, on: &[u32], ratio_of: impl Fn(u32) -> f32, fb: f32) -> Vec<f32> {
    let mut s = vec![(ALGORITHM, alg as f32), (FEEDBACK, fb), (GRIT, 0.0), (VOLUME, 10.0)];
    for n in 1..=6u32 {
        s.push((op(n) + LEVEL, if on.contains(&n) { 80.0 } else { 0.0 }));
        s.push((op(n) + COARSE, ratio_of(n)));
    }
    let mut t = make(&sd(), SR, 77, &s);
    render(t.as_mut(), SR, 3000, 512, &[(0, [0x90, 60, 100])]).0
}

/// The operators from `c` down to a carrier, by the diagram.
fn path_to_carrier(d: &Diagram, c: u32) -> Vec<u32> {
    let mut at = c;
    let mut path = vec![c];
    while !d.carriers.contains(&at) {
        at = d.edges.iter().find(|e| e.0 == at).unwrap().1;
        path.push(at);
    }
    path
}

fn differ(a: &[f32], b: &[f32]) -> bool {
    a.iter().zip(b).any(|(x, y)| (x - y).abs() > 1e-3)
}

#[test]
fn every_algorithm_routes_by_ear_as_its_diagram_says() {
    let ratio = |n: u32| n as f32;
    for (k, text) in DIAGRAMS.iter().enumerate() {
        let d = parse(text);
        let alg = k + 1;
        for n in 1..=6u32 {
            // Alone, an operator is heard only if it's a carrier.
            let x = route(alg, &[n], ratio, 0.0);
            assert_eq!(peak(&x) > 1e-3, d.carriers.contains(&n), "algorithm {alg}: operator {n} alone");
        }
        for &(m, c) in &d.edges {
            // On its route to the output, a modulator changes what's there.
            let path = path_to_carrier(&d, c);
            let without = route(alg, &path, ratio, 0.0);
            let mut on = path.clone();
            on.push(m);
            let got = route(alg, &on, ratio, 0.0);
            assert!(differ(&without, &got), "algorithm {alg}: {m}>{c} does nothing");
        }
        // A pair the diagram doesn't join stays apart.
        for m in 2..=6u32 {
            if d.carriers.contains(&m) {
                continue;
            }
            for c in d.carriers.iter().copied() {
                let reaches = d.edges.contains(&(m, c));
                if reaches {
                    continue;
                }
                let without = route(alg, &[c], ratio, 0.0);
                let got = route(alg, &[c, m], ratio, 0.0);
                assert!(!differ(&without, &got), "algorithm {alg}: {m} shouldn't reach {c}");
            }
        }
    }
}

#[test]
fn feedback_is_where_each_algorithm_has_it() {
    let ratio = |_| 1.0;
    for (k, text) in DIAGRAMS.iter().enumerate() {
        let d = parse(text);
        let alg = k + 1;
        let (from, to) = d.fb;
        // Its loop with the path out.
        let mut on = path_to_carrier(&d, to);
        if !on.contains(&from) {
            on.push(from);
        }
        let a = route(alg, &on, ratio, 0.0);
        let b = route(alg, &on, ratio, 7.0);
        assert!(differ(&a, &b), "algorithm {alg}: feedback {from}>{to} does nothing");
        // The operators elsewhere don't care about it.
        for &c in &d.carriers {
            if !on.contains(&c) && !d.edges.iter().any(|e| e.1 == c && on.contains(&e.0)) {
                let (x, y) = (route(alg, &[c], ratio, 0.0), route(alg, &[c], ratio, 7.0));
                assert!(!differ(&x, &y), "algorithm {alg}: carrier {c} shouldn't feel the loop");
            }
        }
    }
    // The longer loops: from a different operator, so with that one off it's silent.
    for (alg, on) in [(4usize, vec![4u32, 5, 6]), (6, vec![5, 6])] {
        let a = route(alg, &on, ratio, 0.0);
        let b = route(alg, &on, ratio, 7.0);
        assert!(differ(&a, &b), "algorithm {alg}");
        // Without the operator it comes from, there's no loop: op 6 isn't heard either way here.
        let c = route(alg, &on[..1], ratio, 7.0);
        let d = route(alg, &on[..1], ratio, 0.0);
        assert!(!differ(&c, &d), "algorithm {alg} with the loop open");
    }
}

#[test]
fn feedback_goes_from_a_sine_to_brighter_as_it_rises() {
    // Operator 6 as a carrier in algorithm 32: its own output is its modulator.
    let level = |fb: f32| {
        let s = [(ALGORITHM, 32.0), (O1 + LEVEL, 0.0), (O6 + LEVEL, 99.0), (FEEDBACK, fb), (VOLUME, 10.0), (GRIT, 0.0)];
        let mut t = make(&sd(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 1 << 16, 512, &[(0, [0x90, 60, 100])]);
        // The second harmonic against the first.
        let sp = spectrum(&l[1 << 15..]);
        let bin = (exact_hz(60.0) as f64 / SR as f64 * (1 << 15) as f64).round() as usize;
        let around = |c: usize| sp[c - 2..c + 3].iter().copied().fold(0.0, f64::max);
        db(around(2 * bin) / around(bin))
    };
    let (none, mid, full) = (level(0.0), level(4.0), level(7.0));
    println!("second harmonic against the first: feedback 0 {none:.1} dB, 4 {mid:.1} dB, 7 {full:.1} dB");
    assert!(none < -40.0 && mid > none + 3.0 && full > mid + 3.0, "{none} {mid} {full}");
}

#[test]
fn the_envelope_is_a_steady_fall_in_db_and_its_times_scale() {
    // R2 and L2 at the bottom: a straight line in dB from full to silence.
    let s = with(sine(), &[(O1 + R2, 50.0), (O1 + L2, 0.0), (O1 + L3, 0.0)]);
    let mut t = make(&sd(), SR, 77, &s);
    let (l, _) = render(t.as_mut(), SR, 96000, 512, &[(0, [0x90, 60, 100])]);
    let levels: Vec<f64> = l.chunks(2400).map(|c| db(peak(c) as f64)).collect();
    let falls: Vec<f64> = levels.windows(2).skip(1).take(4).map(|w| w[1] - w[0]).collect();
    println!("dB per 50 ms: {falls:?}");
    let (a, b) = (falls[0], falls[3]);
    assert!(a < -1.0, "{a}");
    assert!((a - b).abs() < 1.0, "steady in dB: {a} {b}");
    // Rates 4 steps apart are about a doubling of speed... a higher rate is a shorter fall.
    let t_to_60db = |r: f32| {
        let s = with(sine(), &[(O1 + R2, r), (O1 + L2, 0.0), (O1 + L3, 0.0)]);
        let mut t = make(&sd(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 240000, 512, &[(0, [0x90, 60, 100])]);
        let p = peak(&l[..2400]);
        l.iter().rposition(|v| v.abs() > p * 1e-3).unwrap() as f32 / SR
    };
    let (slow, fast) = (t_to_60db(40.0), t_to_60db(70.0));
    println!("fall to -60 dB: {slow:.2} s at rate 40, {fast:.3} s at rate 70");
    assert!(slow > fast * 4.0);
}

#[test]
fn level_4_above_zero_drones_until_the_transport_stops() {
    let s = with(sine(), &[(O1 + L4, 70.0)]);
    let mut t = make(&sd(), SR, 77, &s);
    let (l, _) = render(t.as_mut(), SR, 48000, 512, &[(0, [0x90, 60, 100]), (4800, [0x80, 60, 0])]);
    assert!(peak(&l[40000..]) > 0.005, "the hardware's quirk: {}", peak(&l[40000..]));
    t.all_notes_off();
    let (l, _) = render(t.as_mut(), SR, 96000, 512, &[]);
    assert!(peak(&l[80000..]) == 0.0, "{}", peak(&l[80000..]));
}

#[test]
fn keyboard_scaling_of_level_and_rate() {
    // Depth to the right, with a linear falling curve: higher notes are quieter.
    let lv = |note: u8| {
        let s = with(sine(), &[(O1 + BREAK, 39.0), (O1 + R_DEPTH, 60.0), (O1 + R_CURVE, 0.0), (O1 + L_DEPTH, 0.0)]);
        let mut t = make(&sd(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 24000, 512, &[(0, [0x90, note, 100])]);
        rms(&l[12000..])
    };
    let (at, up, far) = (lv(60), lv(72), lv(96));
    println!("right depth: {at:.3} at the break, {up:.3} an octave up, {far:.3} three up");
    assert!(lv(48) == at || (lv(48) - at).abs() < at * 0.01, "no left depth");
    assert!(up < at * 0.8 && far < up * 0.6, "{at} {up} {far}");
    // The rising curve goes the other way.
    let rising = |note: u8| {
        let s = with(sine(), &[(O1 + LEVEL, 70.0), (O1 + BREAK, 39.0), (O1 + R_DEPTH, 60.0), (O1 + R_CURVE, 3.0)]);
        let mut t = make(&sd(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 24000, 512, &[(0, [0x90, note, 100])]);
        rms(&l[12000..])
    };
    assert!(rising(84) > rising(60) * 1.3);
    // Rate scaling: higher notes end sooner.
    let dur = |note: u8, rs: f32| {
        let s = with(sine(), &[(O1 + R2, 40.0), (O1 + L2, 0.0), (O1 + L3, 0.0), (O1 + RATE_SCALE, rs)]);
        let mut t = make(&sd(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 300000, 512, &[(0, [0x90, note, 100])]);
        let p = peak(&l[..2400]);
        l.iter().rposition(|v| v.abs() > p * 1e-3).unwrap() as f32 / SR
    };
    assert!((dur(96, 0.0) / dur(36, 0.0) - 1.0).abs() < 0.1, "no scale: {} {}", dur(96, 0.0), dur(36, 0.0));
    let (lo, hi) = (dur(36, 7.0), dur(96, 7.0));
    println!("rate scale 7: {lo:.2} s at note 36, {hi:.2} s at note 96");
    assert!(lo > hi * 2.0, "{lo} {hi}");
}

#[test]
fn velocity_sensitivity_works_per_operator() {
    let lv = |velocity: u8, sens: f32| {
        let s = with(sine(), &[(O1 + VEL, sens)]);
        let mut t = make(&sd(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 24000, 512, &[(0, [0x90, 60, velocity])]);
        rms(&l[12000..])
    };
    // None, then all of it. Velocity 100 is the reference.
    assert!((lv(30, 0.0) / lv(127, 0.0) - 1.0).abs() < 1e-3);
    assert!((lv(100, 7.0) / lv(100, 0.0) - 1.0).abs() < 0.1);
    let (soft, hard) = (lv(30, 7.0), lv(127, 7.0));
    println!("sensitivity 7: velocity 30 {soft:.4}, 127 {hard:.4}");
    assert!(hard > soft * 8.0, "{soft} {hard}");
}

#[test]
fn pitch_envelope_and_lfo_move_every_operator() {
    let crossings = |x: &[f32]| x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32 * SR / x.len() as f32;
    let s = with(sine(), &[(PL1, 75.0), (PL2, 50.0), (PL3, 50.0), (PL4, 50.0), (PR1, 99.0), (PR2, 60.0)]);
    let mut t = make(&sd(), SR, 77, &s);
    let (l, _) = render(t.as_mut(), SR, 96000, 512, &[(0, [0x90, 69, 100])]);
    // It climbs at its rate: at the fastest, 12 octaves a second.
    let (early, late) = (crossings(&l[3400..6400]), crossings(&l[72000..]));
    println!("pitch envelope: {early:.0} Hz early, {late:.0} Hz late");
    assert!(early > late * 1.2, "{early} {late}");
    assert!(cents(late, 440.0).abs() < 10.0, "{late}");

    // Fixed-frequency operators don't follow it.
    let s = with(s, &[(O1 + MODE, 1.0), (O1 + COARSE, 2.0), (O1 + FINE, 64.0)]);
    let mut t = make(&sd(), SR, 77, &s);
    let (l, _) = render(t.as_mut(), SR, 96000, 512, &[(0, [0x90, 69, 100])]);
    let (e, f) = (crossings(&l[200..4000]), crossings(&l[72000..]));
    assert!((e / f - 1.0).abs() < 0.05, "{e} {f}");

    // The LFO.
    let vib = |pmd: f32, delay: f32| {
        let s = with(sine(), &[(LFO_PMD, pmd), (LFO_PMS, 7.0), (LFO_SPEED, 50.0), (LFO_DELAY, delay)]);
        let mut t = make(&sd(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 96000, 512, &[(0, [0x90, 69, 100])]);
        let sl: Vec<usize> = l.chunks(1200).skip(1).map(|c| c.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()).collect();
        (*sl.iter().max().unwrap() - *sl.iter().min().unwrap(), sl[2].abs_diff(*sl.first().unwrap()))
    };
    assert!(vib(0.0, 0.0).0 <= 1);
    assert!(vib(99.0, 0.0).0 > 4, "{}", vib(99.0, 0.0).0);
    // Amp mod: only for an operator with sensitivity.
    let trem = |ams: f32| {
        let s = with(sine(), &[(LFO_AMD, 99.0), (LFO_SPEED, 50.0), (O1 + AMS_FIELD, ams)]);
        let mut t = make(&sd(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 96000, 512, &[(0, [0x90, 69, 100])]);
        let p: Vec<f32> = l[9600..].chunks(480).map(peak).collect();
        p.iter().fold(0.0f32, |m, v| m.max(*v)) / p.iter().fold(9.0f32, |m, v| m.min(*v)).max(1e-6)
    };
    assert!(trem(0.0) < 1.05, "{}", trem(0.0));
    assert!(trem(3.0) > 3.0, "{}", trem(3.0));
    assert!(env::lfo_hz(0) > 0.0 && env::lfo_hz(99) > 15.0, "{}", env::lfo_hz(99));
}

#[test]
fn the_resampler_keeps_the_level_at_every_rate() {
    for &sr in &[44100.0f32, 48000.0, 96000.0] {
        for note in [48u8, 96] {
            let mut t = make(&sd(), sr, 77, &with(sine(), &[(GRIT, 0.0)]));
            let (l, _) = render(t.as_mut(), sr, 65536, 512, &[(0, [0x90, note, 100])]);
            let g = peak(&l[32768..]);
            // A carrier at full and volume full: the same everywhere.
            assert!((g - 0.5).abs() < 0.02, "{sr} {note}: {g}");
        }
    }
}

#[test]
fn sixteen_voices() {
    let active = |t: &Sedici| t.voices.iter().filter(|v| v.active).count();
    let mut t = Sedici::new(SR, 7);
    let chord: Vec<Event> = (0..20u8).map(|k| (k as usize, [0x90, 40 + k, 100])).collect();
    render(&mut t, SR, 2000, 512, &chord);
    assert_eq!(active(&t), 16);
}

#[test]
fn mono_mode_and_portamento() {
    let s = with(sine(), &[(POLY, 1.0), (PORTA, 60.0)]);
    let mut t = make(&sd(), SR, 77, &s);
    let (l, _) = render(t.as_mut(), SR, 96000, 512, &[(0, [0x90, 60, 100]), (24000, [0x90, 84, 100])]);
    let crossings = |x: &[f32]| x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32 * SR / x.len() as f32;
    let (a, m, b) = (crossings(&l[16000..24000]), crossings(&l[24400..25000]), crossings(&l[80000..]));
    println!("portamento: {a:.0}, then {m:.0}, then {b:.0} Hz");
    assert!(m > a * 1.1 && m < b * 0.9, "{a} {m} {b}");
}

#[test]
fn no_nan_or_runaway_on_extreme_settings() {
    let all = |alg: f32, fb: f32| {
        let mut s = vec![(ALGORITHM, alg), (FEEDBACK, fb), (VOLUME, 10.0)];
        for n in 1..=6 {
            s.push((op(n) + LEVEL, 99.0));
            s.push((op(n) + COARSE, 31.0));
            s.push((op(n) + VEL, 7.0));
            s.push((op(n) + RATE_SCALE, 7.0));
            s.push((op(n) + R_DEPTH, 99.0));
            s.push((op(n) + R_CURVE, 3.0));
            s.push((op(n) + L_DEPTH, 99.0));
            s.push((op(n) + L_CURVE, 3.0));
        }
        s
    };
    let cases: Vec<Vec<(u32, f32)>> = vec![all(32.0, 7.0), all(1.0, 7.0), all(4.0, 7.0), all(6.0, 7.0), with(all(16.0, 7.0), &[(PL1, 99.0), (PL2, 0.0), (PR1, 99.0), (PR2, 99.0), (LFO_PMD, 99.0), (LFO_AMD, 99.0), (LFO_PMS, 7.0), (LFO_SPEED, 99.0)])];
    assert_no_runaway(&sd(), &cases, 16.0);
    assert_survives_param_jumps(&sd(), 32.0);
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
    let s = [(ALGORITHM, 1.0), (FEEDBACK, 5.0), (O2 + LEVEL, 70.0), (O3 + LEVEL, 80.0), (O4 + LEVEL, 60.0), (LFO_PMD, 20.0), (LFO_WAVE, 5.0), (LFO_SYNC, 0.0), (O1 + R4, 70.0), (O3 + R4, 70.0)];
    assert_block_size_independent(&sd(), &s, &busy(), 180_000);
    let s = [(POLY, 1.0), (PORTA, 40.0), (KEY_SYNC, 0.0), (ALGORITHM, 4.0), (FEEDBACK, 7.0), (O4 + LEVEL, 80.0), (O6 + LEVEL, 70.0)];
    assert_block_size_independent(&sd(), &s, &busy(), 180_000);
}

#[test]
fn renders_repeat_and_seed_matters() {
    let s = [(KEY_SYNC, 0.0), (ALGORITHM, 5.0), (O2 + LEVEL, 70.0), (LFO_WAVE, 5.0), (LFO_PMD, 30.0), (LFO_SYNC, 0.0)];
    let events = [(0usize, [0x90, 48, 100]), (20000, [0x80, 48, 0]), (24000, [0x90, 55, 100]), (40000, [0xB0, 1, 90]), (41000, [0xE0, 0, 90]), (42000, [0xB0, 64, 127])];
    assert_repeats(&sd(), &s, &events, 96000);
    // Free-running operators and sample-and-hold are what a seed changes; with key sync on and no
    // random LFO there's nothing to differ, as on the hardware.
    assert_seed_matters(&sd(), &s, &events, 48000);
}

#[test]
fn no_click_on_note_off_steal_or_stopping() {
    let dark = vec![(ALGORITHM, 32.0), (VOLUME, 10.0), (O1 + R4, 60.0), (GRIT, 0.0)];
    assert_no_click(&sd(), &dark, &[(0, [0x90, 60, 100]), (24000, [0x80, 60, 0])], 12000..24000, 23990..24800, 1.25, "note-off");
    // All sixteen held, a seventeenth steals, with key sync on: the phases jump.
    let mut steal: Vec<Event> = (0..16u8).map(|k| (k as usize, [0x90, 36 + k, 100])).collect();
    steal.push((36000, [0x90, 36, 100]));
    let held: Vec<(u32, f32)> = with(dark.clone(), &[(VOLUME, 4.0)]);
    assert_no_click(&sd(), &held, &steal, 24000..36000, 35990..37000, 2.5, "voice steal");
}

#[test]
fn moving_volume_doesnt_zipper() {
    let mut t = make(&sd(), SR, 77, &[(ALGORITHM, 32.0), (GRIT, 0.0)]);
    let (mut l, mut r) = (vec![0.0f32; 96000], vec![0.0f32; 96000]);
    t.process(&mut l[..512], &mut r[..512], &[MidiEvent { offset: 0, data: [0x90, 48, 100] }], &ctx(SR));
    let mut at = 512;
    while at + 512 <= 96000 {
        if at > 48000 {
            t.set_param(VOLUME, 5.0 + ((at / 512) % 4) as f32 * 0.5);
            t.set_param(O1 + LEVEL, 95.0 + ((at / 512) % 3) as f32);
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
fn an_operator_knob_moves_the_note_under_it() {
    // A level, and an envelope's level, on a note already holding at its third level.
    let measure = |id: u32, v: f32| {
        let mut t = make(&sd(), SR, 77, &sine());
        let (mut l, mut r) = (vec![0.0f32; 48000], vec![0.0f32; 48000]);
        t.process(&mut l[..1024], &mut r[..1024], &[MidiEvent { offset: 0, data: [0x90, 60, 100] }], &ctx(SR));
        let mut at = 1024;
        while at + 1024 <= 48000 {
            if at >= 24000 {
                t.set_param(id, v);
            }
            t.process(&mut l[at..at + 1024], &mut r[at..at + 1024], &[], &ctx(SR));
            at += 1024;
        }
        (rms(&l[12000..24000]), rms(&l[36000..at]))
    };
    let (a, b) = measure(O1 + LEVEL, 70.0);
    assert!(b < a * 0.5, "{a} {b}");
    let (a, b) = measure(O1 + L3, 70.0);
    assert!(b < a * 0.7, "{a} {b}");
}

#[test]
fn every_factory_preset_plays_at_a_sane_level() {
    assert_presets(&sd(), [48, 67]);
}

#[test]
fn goes_to_rest_and_tail_reaches_the_end() {
    assert_goes_silent(&sd(), &[(O1 + R4, 70.0)], 4.0);
    assert_tail_is_long_enough(&sd(), &[(O1 + R4, 40.0)]);
    assert_tail_is_long_enough(&sd(), &[(ALGORITHM, 32.0), (O3 + LEVEL, 99.0), (O3 + R4, 30.0), (O1 + R4, 90.0)]);
}

#[test]
fn every_parameter_has_a_home_and_each_operator_the_same_fields() {
    assert_eq!(PARAMS.len(), 28 + 6 * 22);
    assert!(PARAMS.iter().all(|p| (p.id as usize) < COUNT));
}
