//! Fermata Uno measured through its public contract, and what's specific to it: slide and accent.
use super::*;
use crate::instrument::test_kit::*;
use crate::synth_dsp::test_util::*;

const SR: f32 = 48000.0;

fn uno() -> InstrumentInfo {
    info()
}

fn with(mut v: Vec<(u32, f32)>, more: &[(u32, f32)]) -> Vec<(u32, f32)> {
    v.extend_from_slice(more);
    v
}

fn clean() -> Vec<(u32, f32)> {
    vec![(DRIFT, 0.0), (CUTOFF, 5000.0), (RESONANCE, 0.0), (ENV_MOD, 0.0)]
}

#[test]
fn tuned_within_a_cent_across_the_range_at_each_rate() {
    assert_tuned(&uno(), &clean(), &[24, 36, 48, 60, 72, 84], 0.0, 1.0);
    assert_tuned(&uno(), &with(clean(), &[(WAVE, 1.0)]), &[36, 60], 0.0, 1.0);
    assert_tuned(&uno(), &with(clean(), &[(TUNE, 7.0)]), &[48], 7.0, 1.0);
}

#[test]
fn aliasing_end_to_end() {
    for (s, note, what) in [(clean(), 84u8, "saw"), (with(clean(), &[(WAVE, 1.0)]), 84, "square"), (with(clean(), &[(RESONANCE, 9.0), (CUTOFF, 3000.0)]), 84, "saw with resonance")] {
        let mut t = make(&uno(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 1 << 17, 512, &[(0, [0x90, note, 100])]);
        // The VEG's long decay would smear the lines: normalize it out for the measurement.
        let tail = &l[1 << 16..];
        let a = alias_db(tail, SR, exact_hz(note as f64) as f64);
        println!("aliasing, {what} at note {note}: {a:.1} dB");
        assert!(a < -55.0, "{what}: {a}");
    }
}

#[test]
fn cutoff_doesnt_follow_the_keyboard() {
    // A tone well above the cutoff at two notes: the note doesn't move the cutoff.
    let cut_at = |note: u8| {
        let mut t = Uno::new(SR, 7);
        t.set_param(ENV_MOD, 0.0);
        render(&mut t, SR, 4800, 512, &[(0, [0x90, note, 100])]);
        t.cutoff_now()
    };
    assert_eq!(cut_at(36), cut_at(72));
}

#[test]
fn overlapping_notes_slide_and_a_gap_doesnt() {
    let s = with(clean(), &[(SLIDE_TIME, 100.0)]);
    let crossings = |x: &[f32]| x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32 * SR / x.len() as f32;
    let run = |events: &[Event]| {
        let mut t = make(&uno(), SR, 77, &s);
        render(t.as_mut(), SR, 72000, 512, events).0
    };
    // Overlap: 48 held, 72 comes in, 48 goes after.
    let slide = run(&[(0, [0x90, 60, 100]), (24000, [0x90, 84, 100]), (24100, [0x80, 60, 0])]);
    let before = crossings(&slide[20000..24000]);
    let mid = crossings(&slide[24000 + 800..24000 + 2300]);
    let end = crossings(&slide[60000..72000]);
    println!("slide: {before:.0} Hz, in the middle {mid:.0} Hz, {end:.0} Hz at the end");
    assert!(cents(before, exact_hz(60.0)).abs() < 100.0, "{before}");
    assert!(cents(end, exact_hz(84.0)).abs() < 30.0, "{end}");
    assert!(mid > before * 1.2 && mid < end * 0.85, "{before} {mid} {end}");
    // A gap: at pitch straight away.
    let jump = run(&[(0, [0x90, 48, 100]), (23900, [0x80, 48, 0]), (24000, [0x90, 72, 100])]);
    let right_after = crossings(&jump[24100..25600]);
    assert!(cents(right_after, exact_hz(72.0)).abs() < 100.0, "{right_after}");
}

#[test]
fn a_slide_doesnt_restart_the_envelopes_and_a_new_step_does() {
    let level = |x: &[f32]| rms(x);
    let s = [(DRIFT, 0.0), (CUTOFF, 300.0), (RESONANCE, 0.0), (ENV_MOD, 8.0), (DECAY, 200.0)];
    let mut t = make(&uno(), SR, 77, &s);
    let (slid, _) = render(t.as_mut(), SR, 72000, 512, &[(0, [0x90, 48, 100]), (24000, [0x90, 50, 100]), (24100, [0x80, 48, 0])]);
    let mut t = make(&uno(), SR, 77, &s);
    let (fresh, _) = render(t.as_mut(), SR, 72000, 512, &[(0, [0x90, 48, 100]), (23900, [0x80, 48, 0]), (24000, [0x90, 50, 100])]);
    let bright = |x: &[f32]| {
        // A first difference favours the highs.
        let d: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
        level(&d)
    };
    let (a, b) = (bright(&slid[24400..26000]), bright(&fresh[24400..26000]));
    println!("brightness after a slide {a:.5}, after a new step {b:.5}");
    assert!(b > a * 2.0, "{a} {b}");
}

/// The filter's peak cutoff after each note's start, with a line of accented notes.
fn line_of_notes(resonance: f32, velocity: u8) -> Vec<f32> {
    let mut t = Uno::new(SR, 7);
    t.set_param(RESONANCE, resonance);
    t.set_param(ENV_MOD, 0.0);
    t.set_param(ACCENT, 10.0);
    t.reset();
    let step = 6000; // 8 Hz, sixteenths at 120.
    let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
    let mut out = Vec::new();
    let mut at = 0usize;
    for n in 0..6 {
        let mut best = f32::MIN;
        let end = (n + 1) * step;
        let mut first = true;
        while at < end {
            let mut midi = Vec::new();
            if first {
                midi.push(MidiEvent { offset: 0, data: [0x90, 36, velocity] });
                first = false;
            }
            if at + 64 >= n * step + step - 64 {
                midi.push(MidiEvent { offset: 0, data: [0x80, 36, 0] });
            }
            t.process(&mut l, &mut r, &midi, &ctx(SR));
            best = best.max(t.cutoff_now());
            at += 64;
        }
        out.push(best);
    }
    out
}

#[test]
fn accents_lift_the_cutoff_and_stack_at_high_resonance() {
    let plain = line_of_notes(9.0, 100);
    assert!(plain.iter().all(|c| (c - plain[0]).abs() < 0.2), "{plain:?}");
    let low = line_of_notes(0.0, 127);
    let high = line_of_notes(10.0, 127);
    println!("peak cutoff per accented note, low resonance {low:?}");
    println!("peak cutoff per accented note, high resonance {high:?}");
    assert!(high[0] > plain[0] + 3.0, "an accent lifts it: {} {}", high[0], plain[0]);
    // At high resonance, the sweep carries over and climbs.
    assert!(high[3] > high[0] + 1.0, "{high:?}");
    // And at low resonance it hardly stacks, compared with that.
    assert!(high[3] - high[0] > 1.5 * (low[3] - low[0]), "{low:?} {high:?}");
}

#[test]
fn the_sweep_lags_further_behind_at_high_resonance() {
    // Time for the cutoff to reach its peak after an accented note.
    let time = |res: f32| {
        let mut t = Uno::new(SR, 7);
        t.set_param(RESONANCE, res);
        t.set_param(ENV_MOD, 0.0);
        t.set_param(ACCENT, 10.0);
        t.reset();
        let (mut l, mut r) = (vec![0.0f32; 48], vec![0.0f32; 48]);
        t.process(&mut l, &mut r, &[MidiEvent { offset: 0, data: [0x90, 36, 127] }], &ctx(SR));
        let (mut best, mut when) = (f32::MIN, 0usize);
        for i in 0..200 {
            t.process(&mut l, &mut r, &[], &ctx(SR));
            if t.cutoff_now() > best {
                best = t.cutoff_now();
                when = i;
            }
        }
        when as f32 * 48.0 / SR * 1000.0
    };
    let (lo, hi) = (time(0.0), time(10.0));
    println!("accent sweep peaks after {lo:.0} ms at low resonance, {hi:.0} ms at high");
    assert!(hi > lo * 1.3, "{lo} {hi}");
}

#[test]
fn an_accent_is_louder_and_forces_the_short_decay() {
    let run = |velocity: u8, decay: f32| {
        let mut t = make(&uno(), SR, 77, &[(DRIFT, 0.0), (DECAY, decay), (ENV_MOD, 8.0), (CUTOFF, 300.0), (RESONANCE, 3.0), (ACCENT, 10.0)]);
        render(t.as_mut(), SR, 36000, 512, &[(0, [0x90, 40, velocity])]).0
    };
    let (normal, accented) = (run(100, 2000.0), run(127, 2000.0));
    assert!(peak(&accented[..4800]) > peak(&normal[..4800]) * 1.3, "{} {}", peak(&accented[..4800]), peak(&normal[..4800]));
    // With Decay at its longest, the normal note's filter stays open long after: the accented note's
    // MEG has gone. Brightness well after the attack tells them apart.
    let bright = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<f32>>());
    let (a, b) = (bright(&normal[24000..28000]), bright(&accented[24000..28000]));
    println!("brightness at 0.5 s: {a:.5} normal, {b:.5} accented");
    assert!(a > b * 1.4, "{a} {b}");
}

#[test]
fn velocity_threshold_is_the_accent() {
    let is_accented = |threshold: f32, velocity: u8| {
        let mut t = Uno::new(SR, 7);
        t.set_param(ACCENT_VELOCITY, threshold);
        t.process(&mut [0.0; 32], &mut [0.0; 32], &[MidiEvent { offset: 0, data: [0x90, 40, velocity] }], &ctx(SR));
        t.accent
    };
    assert!(!is_accented(110.0, 109));
    assert!(is_accented(110.0, 110));
    assert!(is_accented(1.0, 1));
    // The default is above what the panel's keyboard and new notes play (velocity 100).
    assert!(!is_accented(PARAMS[ACCENT_VELOCITY as usize].default, 100));
}

#[test]
fn the_volume_envelope_decays_and_a_key_up_closes_it_fast() {
    let s = with(clean(), &[(CUTOFF, 2000.0)]);
    let mut t = make(&uno(), SR, 77, &s);
    let n = 6 * 48000;
    let (l, _) = render(t.as_mut(), SR, n, 512, &[(0, [0x90, 45, 100])]);
    let (early, late) = (peak(&l[4800..9600]), peak(&l[n - 4800..]));
    println!("held note: {early:.3} early, {late:.5} after 6 s");
    assert!(late < early * 0.1, "it fades with the key down: {early} {late}");
    // Key up, gone within 50 ms.
    let mut t = make(&uno(), SR, 77, &s);
    let (l, _) = render(t.as_mut(), SR, 12000, 512, &[(0, [0x90, 45, 100]), (6000, [0x80, 45, 0])]);
    assert!(peak(&l[8400..]) < 1e-3, "{}", peak(&l[8400..]));
}

#[test]
fn mono_only_with_the_last_key() {
    let s = clean();
    let mut t = make(&uno(), SR, 77, &with(s, &[(SLIDE_TIME, 10.0)]));
    let (l, _) = render(t.as_mut(), SR, 1 << 17, 512, &[(0, [0x90, 60, 100]), (100, [0x90, 40, 100])]);
    assert!(cents(pitch_hz(&l, SR), exact_hz(40.0)).abs() < 1.0, "the last key sounds, low or high");
}

#[test]
fn no_nan_or_runaway_on_extreme_settings() {
    let cases: Vec<Vec<(u32, f32)>> = vec![
        vec![(RESONANCE, 10.0), (ENV_MOD, 10.0), (ACCENT, 10.0), (CUTOFF, 5000.0), (VOLUME, 10.0), (ACCENT_VELOCITY, 1.0)],
        vec![(RESONANCE, 10.0), (ENV_MOD, 10.0), (ACCENT, 10.0), (CUTOFF, 100.0), (VOLUME, 10.0), (WAVE, 1.0), (SLIDE_TIME, 500.0)],
        vec![(RESONANCE, 0.0), (ENV_MOD, 0.0), (CUTOFF, 100.0), (VOLUME, 10.0), (TUNE, 12.0), (BEND_RANGE, 12.0)],
    ];
    assert_no_runaway(&uno(), &cases, 16.0);
    assert_survives_param_jumps(&uno(), 32.0);
}

fn busy() -> Vec<Event> {
    let mut events = Vec::new();
    for i in 0..30usize {
        // Some overlap, some don't, some accented.
        events.push((i * 3333 + 11, [0x90, (30 + (i * 7) % 30) as u8, if i % 3 == 0 { 120 } else { 80 }]));
        let len = if i % 2 == 0 { 3400 } else { 3000 };
        events.push((i * 3333 + 11 + len, [0x80, (30 + (i * 7) % 30) as u8, 0]));
        events.push((i * 3333 + 1500, [0xE0, 10, (50 + i % 30) as u8]));
    }
    events.push((130_000, [0x90, 40, 100]));
    events.sort_by_key(|e| e.0);
    events
}

#[test]
fn output_doesnt_depend_on_the_block_size() {
    assert_block_size_independent(&uno(), &[(RESONANCE, 8.0), (DRIFT, 1.0), (ACCENT, 8.0)], &busy(), 180_000);
    assert_block_size_independent(&uno(), &[(WAVE, 1.0), (DRIFT, 1.0), (RESONANCE, 3.0)], &busy(), 180_000);
}

#[test]
fn renders_repeat_and_seed_matters() {
    let s = [(DRIFT, 1.0), (RESONANCE, 8.0)];
    let events = [(0usize, [0x90, 48, 100]), (20000, [0x80, 48, 0]), (24000, [0x90, 55, 127]), (40000, [0xE0, 0, 90])];
    assert_repeats(&uno(), &s, &events, 96000);
    assert_seed_matters(&uno(), &s, &events, 48000);
}

fn dark() -> Vec<(u32, f32)> {
    vec![(DRIFT, 0.0), (CUTOFF, 500.0), (RESONANCE, 0.0), (ENV_MOD, 0.0)]
}

#[test]
fn no_click_on_note_off_and_a_new_step() {
    assert_no_click(&uno(), &dark(), &[(0, [0x90, 45, 100]), (24000, [0x80, 45, 0])], 12000..24000, 23990..24800, 1.25, "note-off");
    // A new step straight after the old one closes: the envelope carries on from where it was.
    assert_no_click(&uno(), &dark(), &[(0, [0x90, 45, 100]), (24000, [0x80, 45, 0]), (24100, [0x90, 45, 100])], 12000..24000, 23990..25000, 1.5, "new step");
    // A slide.
    assert_no_click(&uno(), &dark(), &[(0, [0x90, 45, 100]), (24000, [0x90, 48, 100]), (24100, [0x80, 45, 0])], 12000..24000, 23990..27000, 1.5, "slide");
}

#[test]
fn moving_a_knob_mid_note_doesnt_zipper() {
    let mut t = make(&uno(), SR, 77, &dark());
    let (mut l, mut r) = (vec![0.0f32; 96000], vec![0.0f32; 96000]);
    t.process(&mut l[..512], &mut r[..512], &[MidiEvent { offset: 0, data: [0x90, 36, 100] }], &ctx(SR));
    let mut at = 512;
    while at + 512 <= 96000 {
        if at > 48000 {
            let k = ((at / 512) % 4) as f32;
            t.set_param(VOLUME, 5.0 + k * 0.5);
            t.set_param(CUTOFF, 450.0 + 100.0 * k);
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
fn doesnt_quite_self_oscillate_at_full_resonance() {
    // No oscillator can be turned off on this instrument, so listen after the key's gone: at full
    // resonance the ring dies away rather than going on.
    let mut f = DiodeLadder::default();
    f.set_feedback_highpass(cutoff_g(FEEDBACK_HP_HZ, 96000.0));
    f.set(cutoff_g(800.0, 96000.0), RES_MAX);
    let x: Vec<f32> = (0..96000).map(|i| f.tick(if i < 8 { 0.2 } else { 0.0 })).collect();
    let (a, b) = (peak(&x[100..4000]), peak(&x[48000..]));
    println!("full resonance's ring: {a:.4} then {b:.6}");
    assert!(b < a * 0.05, "{a} {b}");
}

#[test]
fn every_factory_preset_plays_at_a_sane_level() {
    assert_presets(&uno(), [36, 48]);
}

#[test]
fn goes_to_rest_and_tail_is_long_enough() {
    assert_goes_silent(&uno(), &[], 2.0);
    assert_tail_is_long_enough(&uno(), &[]);
}
