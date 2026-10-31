//! Fermata Sei measured through its public contract, and what's specific to it.
use super::*;
use crate::instrument::test_kit::*;
use crate::synth_dsp::test_util::*;

const SR: f32 = 48000.0;

fn sei() -> InstrumentInfo {
    info()
}

/// The filter open, no chorus: the DCO's waves as they are.
fn clean() -> Vec<(u32, f32)> {
    vec![(CHORUS, 0.0), (CUTOFF, 20000.0), (ENV_AMOUNT, 0.0), (KEYBOARD, 0.0), (SUSTAIN, 10.0), (VARIATION, 0.0)]
}

fn with(mut v: Vec<(u32, f32)>, more: &[(u32, f32)]) -> Vec<(u32, f32)> {
    v.extend_from_slice(more);
    v
}

#[test]
fn tuned_within_a_cent_across_the_range_at_each_rate() {
    // The sub, whose fundamental dominates.
    let s = with(clean(), &[(SAW_ON, 0.0), (PULSE_ON, 0.0), (SUB, 10.0)]);
    assert_tuned(&sei(), &s, &[36, 48, 60, 69, 84, 96], -12.0, 1.0);
    let s = with(clean(), &[(CHORUS, 0.0)]);
    assert_tuned(&sei(), &s, &[36, 60, 69, 96], 0.0, 1.0);
    let s = with(clean(), &[(RANGE, 2.0)]);
    assert_tuned(&sei(), &s, &[60], 12.0, 1.0);
    let s = with(clean(), &[(RANGE, 0.0)]);
    assert_tuned(&sei(), &s, &[60], -12.0, 1.0);
}

#[test]
fn saw_pulse_and_sub_stay_locked() {
    // They come from one clock: over a long render their mix is exactly periodic.
    let s = with(clean(), &[(PULSE_ON, 1.0), (SUB, 8.0), (PWM, 3.0)]);
    let mut t = make(&sei(), SR, 77, &s);
    let (l, _) = render(t.as_mut(), SR, 1 << 17, 512, &[(0, [0x90, 69, 100])]);
    // At the sub's period, 880/4 = 2 periods of 220: 96000/440 = 218.18 samples: use a whole number
    // of samples by comparing against the spectrum: everything's a harmonic of 220 Hz.
    let a = alias_db(&l[1 << 16..], SR, 220.0);
    println!("saw, pulse and sub at A4: everything else {a:.1} dB");
    assert!(a < -55.0, "{a}");
}

#[test]
fn aliasing_of_a_bright_note_end_to_end() {
    for (note, wave, name) in [(96u8, vec![(SAW_ON, 1.0)], "saw"), (96, vec![(SAW_ON, 0.0), (PULSE_ON, 1.0), (PWM, 6.0)], "pulse"), (84, vec![(SAW_ON, 1.0), (PULSE_ON, 1.0), (SUB, 10.0)], "everything")] {
        let s = with(clean(), &wave);
        let mut t = make(&sei(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 1 << 17, 512, &[(0, [0x90, note, 100])]);
        let f0 = exact_hz(note as f64) as f64 / if name == "everything" { 2.0 } else { 1.0 };
        let a = alias_db(&l[1 << 16..], SR, f0);
        println!("aliasing, note {note} {name}: {a:.1} dB");
        assert!(a < -55.0, "{name}: {a}");
    }
}

#[test]
fn no_nan_or_runaway_on_extreme_settings() {
    let cases: Vec<Vec<(u32, f32)>> = vec![
        vec![(SAW_ON, 1.0), (PULSE_ON, 1.0), (SUB, 10.0), (NOISE, 10.0), (CUTOFF, 20000.0), (RESONANCE, 10.0), (ENV_AMOUNT, 10.0), (LEVEL, 10.0), (CHORUS, 3.0), (HPF, 0.0)],
        vec![(CUTOFF, 20.0), (RESONANCE, 10.0), (VCF_LFO, 10.0), (DCO_LFO, 10.0), (LFO_RATE, 10.0), (PWM_MODE, 0.0), (PWM, 10.0), (LEVEL, 10.0), (CHORUS, 2.0)],
        vec![(ASSIGN, 2.0), (UNISON_DETUNE, 1.0), (RESONANCE, 10.0), (LEVEL, 10.0), (SUB, 10.0), (PULSE_ON, 1.0)],
        vec![(ATTACK, 1.5), (DECAY, 1.5), (SUSTAIN, 10.0), (RELEASE, 1.5), (PORTA_ON, 1.0), (PORTA, 1.0), (BEND_DCO, 12.0), (BEND_VCF, 10.0), (ENV_POLARITY, 1.0), (ENV_AMOUNT, 10.0), (LEVEL, 10.0)],
    ];
    assert_no_runaway(&sei(), &cases, 16.0);
    assert_survives_param_jumps(&sei(), 32.0);
}

fn busy() -> Vec<Event> {
    let mut events = Vec::new();
    for i in 0..30usize {
        events.push((i * 3333 + 11, [0x90, (40 + (i * 7) % 40) as u8, 100]));
        events.push((i * 3333 + 3000, [0x80, (40 + (i * 7) % 40) as u8, 0]));
        events.push((i * 3333 + 1500, [0xE0, 10, (50 + i % 30) as u8]));
        events.push((i * 3333 + 2000, [0xB0, 1, (i * 4 % 128) as u8]));
    }
    // A gap long enough to come to rest, and a note after it: the LFO must catch up the same way.
    events.push((120_000, [0x90, 50, 100]));
    events.sort_by_key(|e| e.0);
    events
}

#[test]
fn output_doesnt_depend_on_the_block_size() {
    let s = [(NOISE, 2.0), (PWM_MODE, 0.0), (PWM, 5.0), (DCO_LFO, 3.0), (VCF_LFO, 3.0), (LFO_DELAY, 2.0), (RELEASE, 20.0), (CHORUS, 2.0), (CHORUS_NOISE, 1.0)];
    assert_block_size_independent(&sei(), &s, &busy(), 180_000);
    let s = [(ASSIGN, 2.0), (PORTA_ON, 1.0), (CHORUS, 3.0)];
    assert_block_size_independent(&sei(), &s, &busy(), 180_000);
}

#[test]
fn renders_repeat_and_seed_matters() {
    let s = [(NOISE, 1.0), (VARIATION, 1.0), (DCO_LFO, 2.0), (CHORUS_NOISE, 1.0)];
    let events = [(0usize, [0x90, 48, 100]), (20000, [0x80, 48, 0]), (24000, [0x90, 55, 100]), (40000, [0xB0, 1, 90]), (41000, [0xE0, 0, 90]), (42000, [0xB0, 64, 127])];
    assert_repeats(&sei(), &s, &events, 96000);
    assert_seed_matters(&sei(), &s, &events, 48000);
}

/// A dark steady tone whose steps are small, so anything sudden shows.
fn dark() -> Vec<(u32, f32)> {
    vec![(VARIATION, 0.0), (CHORUS, 0.0), (CUTOFF, 700.0), (ENV_AMOUNT, 0.0), (KEYBOARD, 0.0), (ATTACK, 5.0), (SUSTAIN, 10.0), (RELEASE, 12.0)]
}

#[test]
fn no_click_on_note_off_steal_or_gate() {
    assert_no_click(&sei(), &dark(), &[(0, [0x90, 60, 100]), (24000, [0x80, 60, 0])], 12000..24000, 23990..24800, 1.25, "note-off");
    // All six held, a seventh steals.
    let hold: Vec<Event> = (0..6u8).map(|k| (k as usize, [0x90, 48 + k * 2, 100])).collect();
    let mut steal = hold.clone();
    steal.push((36000, [0x90, 60, 100]));
    assert_no_click(&sei(), &dark(), &steal, 24000..36000, 35990..37000, 2.0, "voice steal");
    let s = with(dark(), &[(VCA_MODE, 1.0)]);
    assert_no_click(&sei(), &s, &[(0, [0x90, 60, 100]), (24000, [0x80, 60, 0]), (30000, [0x90, 60, 100])], 12000..24000, 23990..31000, 1.5, "the gate VCA");
}

#[test]
fn six_voices_and_the_two_poly_modes() {
    let active = |t: &Sei| t.voices.iter().filter(|v| v.active).count();
    let mut t = Sei::new(SR, 7);
    let chord: Vec<Event> = (0..8u8).map(|k| (k as usize, [0x90, 48 + k, 100])).collect();
    render(&mut t, SR, 2000, 512, &chord);
    assert_eq!(active(&t), 6, "an 8-note chord on 6 voices");

    // Unison: all six, on one note.
    let mut t = Sei::new(SR, 7);
    t.set_param(ASSIGN, 2.0);
    render(&mut t, SR, 2000, 512, &[(0, [0x90, 48, 100]), (1, [0x90, 52, 100])]);
    assert_eq!(active(&t), 6);
}

#[test]
fn poly_1_moves_on_to_the_next_voice_and_poly_2_goes_back_to_the_lowest() {
    // A note and its release, finished, then another: which voice does it take?
    let voice_of_second = |assign: f32| {
        let mut t = Sei::new(SR, 7);
        t.set_param(ASSIGN, assign);
        t.set_param(RELEASE, 5.0);
        render(&mut t, SR, 12000, 512, &[(0, [0x90, 48, 100]), (2000, [0x80, 48, 0]), (9000, [0x90, 50, 100])]);
        t.voices.iter().position(|v| v.active).unwrap()
    };
    assert_eq!(voice_of_second(0.0), 1, "Poly 1");
    assert_eq!(voice_of_second(1.0), 0, "Poly 2");
}

#[test]
fn lfo_is_single_triangle_and_its_delay_starts_with_the_first_key() {
    let s = with(clean(), &[(DCO_LFO, 3.0), (LFO_RATE, 5.0), (LFO_DELAY, 8.0)]);
    let mut t = make(&sei(), SR, 77, &s);
    // Crossings per slice give the pitch: no vibrato at first, then some.
    let (l, _) = render(t.as_mut(), SR, 144000, 512, &[(0, [0x90, 69, 100])]);
    let slices: Vec<usize> = l.chunks(960).map(|c| c.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()).collect();
    let spread = |s: &[usize]| s.iter().max().unwrap() - s.iter().min().unwrap();
    let early = spread(&slices[1..4]);
    let late = spread(&slices[100..148]);
    println!("vibrato's reach in zero crossings per slice: early {early}, late {late}");
    assert!(early <= 1 && late >= 2, "{early} {late}");
}

#[test]
fn every_voice_gets_the_same_lfo() {
    let mut t = Sei::new(SR, 7);
    t.set_param(CHORUS, 0.0);
    t.set_param(VCF_LFO, 5.0);
    let (a, b) = (t.lfo_phase, t.lfo_inc);
    render(&mut t, SR, 4800, 512, &[(0, [0x90, 60, 100]), (0, [0x90, 67, 100])]);
    let moved = t.lfo_phase - a;
    assert!((moved - 4800.0 * b).abs() < 1e-6, "one phase, one rate: {moved}");
}

#[test]
fn hpf_positions_do_what_they_say() {
    let level_at = |pos: f32, note: u8| {
        let s = with(clean(), &[(HPF, pos), (SAW_ON, 0.0), (SUB, 10.0)]);
        let mut t = make(&sei(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 48000, 512, &[(0, [0x90, note, 100])]);
        rms(&l[24000..])
    };
    // A low note: 0 lifts it, 1 is the reference, 2 and 3 cut it, 3 more.
    let (p0, p1, p2, p3) = (level_at(0.0, 36), level_at(1.0, 36), level_at(2.0, 36), level_at(3.0, 36));
    println!("note 36's sub through the switch: {p0:.3} {p1:.3} {p2:.3} {p3:.3}");
    assert!(p0 > p1 * 1.2 && p2 < p1 * 0.7 && p3 < p2 * 0.8, "{p0} {p1} {p2} {p3}");
    // High up, they agree.
    let (h1, h3) = (level_at(1.0, 96), level_at(3.0, 96));
    assert!((h3 / h1 - 1.0).abs() < 0.2, "{h1} {h3}");
}

#[test]
fn the_vca_switch_and_the_one_envelope() {
    let run = |vca: f32| {
        let s = with(clean(), &[(VCA_MODE, vca), (ATTACK, 200.0), (DECAY, 150.0), (SUSTAIN, 2.0)]);
        let mut t = make(&sei(), SR, 77, &s);
        render(t.as_mut(), SR, 48000, 512, &[(0, [0x90, 69, 100])]).0
    };
    let (env, gate) = (run(0.0), run(1.0));
    // Gate: full almost at once and it stays. Envelope: slow rise, then down to the sustain.
    let lv = |x: &[f32], from: usize| peak(&x[from..from + 960]);
    assert!(lv(&gate, 480) > 0.9 * lv(&gate, 30000), "{}", lv(&gate, 480));
    assert!(lv(&env, 480) < 0.3 * lv(&env, 9600), "{} {}", lv(&env, 480), lv(&env, 9600));
    assert!(lv(&env, 40000) < 0.4 * lv(&env, 9600));
}

#[test]
fn filter_self_oscillates_near_the_top_of_the_knob() {
    let mut worst = 0.0f32;
    for &sr in &[44100.0f32, 48000.0, 96000.0] {
        for note in (36..=84).step_by(12) {
            let s = [(VARIATION, 0.0), (CHORUS, 0.0), (SAW_ON, 0.0), (CUTOFF, exact_hz(60.0)), (RESONANCE, 10.0), (ENV_AMOUNT, 0.0), (KEYBOARD, 10.0), (SUSTAIN, 10.0)];
            let mut t = make(&sei(), sr, 77, &s);
            let (l, _) = render(t.as_mut(), sr, 1 << 18, 512, &[(0, [0x90, note as u8, 100])]);
            assert!(peak(&l[1 << 17..]) > 0.02, "note {note} at {sr}: silent");
            let c = cents(pitch_hz(&l, sr), exact_hz(note as f64));
            worst = worst.max(c.abs());
            assert!(c.abs() < 25.0, "note {note} at {sr}: {c}");
        }
    }
    println!("self-oscillation across 36..84: worst {worst:.1} cents");
}

#[test]
fn moving_a_knob_mid_note_doesnt_zipper() {
    let mut t = make(&sei(), SR, 77, &dark());
    let (mut l, mut r) = (vec![0.0f32; 96000], vec![0.0f32; 96000]);
    t.process(&mut l[..512], &mut r[..512], &[MidiEvent { offset: 0, data: [0x90, 48, 100] }], &ctx(SR));
    let mut at = 512;
    while at + 512 <= 96000 {
        if at > 48000 {
            let k = ((at / 512) % 4) as f32;
            t.set_param(LEVEL, 5.0 + k * 0.5);
            t.set_param(CUTOFF, 600.0 + 100.0 * k);
            t.set_param(PWM, 2.0 + k);
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
fn chorus_makes_it_wide_and_off_makes_it_mono() {
    let run = |mode: f32| {
        let mut t = make(&sei(), SR, 77, &with(clean(), &[(CHORUS, mode)]));
        render(t.as_mut(), SR, 48000, 512, &[(0, [0x90, 60, 100])])
    };
    let (l, r) = run(0.0);
    assert!(l == r, "no chorus, mono");
    for m in [1.0, 2.0, 3.0] {
        let (l, r) = run(m);
        assert!(l != r && l.iter().zip(&r).fold(0.0f32, |p, (a, b)| p.max((a - b).abs())) > 0.05, "mode {m}");
    }
}

#[test]
fn the_hiss_comes_with_the_voices_and_stops_at_rest() {
    let s = [(CHORUS, 1.0), (CHORUS_NOISE, 1.0), (LEVEL, 0.0), (RELEASE, 20.0)];
    let mut t = make(&sei(), SR, 77, &s);
    let (l, _) = render(t.as_mut(), SR, 96000, 512, &[(0, [0x90, 60, 100]), (24000, [0x80, 60, 0])]);
    let hiss = peak(&l[12000..24000]);
    println!("hiss at Chorus Noise 100 %: peak {:.1} dBFS", db(hiss as f64));
    assert!(hiss > 1e-4 && hiss < 3e-3, "{hiss}");
    assert!(l[60000..].iter().all(|v| *v == 0.0), "{}", peak(&l[60000..]));
    assert_goes_silent(&sei(), &[], 4.0);
}

#[test]
fn every_factory_preset_plays_at_a_sane_level() {
    assert_presets(&sei(), [40, 64]);
}

#[test]
fn tail_reaches_the_end_of_the_release() {
    assert_tail_is_long_enough(&sei(), &[(RELEASE, 800.0)]);
}

#[test]
fn state_is_rest_after_a_note_and_all_notes_off_lets_them_ring() {
    let mut t = Sei::new(SR, 7);
    t.set_param(RELEASE, 30.0);
    render(&mut t, SR, 48000, 512, &[(0, [0x90, 60, 100]), (2000, [0x80, 60, 0])]);
    assert!(t.rest.resting);
    render(&mut t, SR, 1000, 512, &[(0, [0x90, 60, 100])]);
    t.all_notes_off();
    assert!(t.voices.iter().any(|v| v.active), "tails ring on");
    assert!(!t.voices.iter().any(|v| v.gated));
}
