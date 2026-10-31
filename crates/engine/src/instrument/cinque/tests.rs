//! Fermata Cinque measured through its public contract, and what's specific to it: above all Poly Mod.
use super::*;
use crate::instrument::test_kit::*;
use crate::synth_dsp::test_util::*;

const SR: f32 = 48000.0;

fn cq() -> InstrumentInfo {
    info()
}

fn with(mut v: Vec<(u32, f32)>, more: &[(u32, f32)]) -> Vec<(u32, f32)> {
    v.extend_from_slice(more);
    v
}

/// One oscillator (A's saw), filter wide open, nothing moving.
fn clean_a() -> Vec<(u32, f32)> {
    vec![(SLOP, 0.0), (MIX_B, 0.0), (MIX_A, 3.0), (CUTOFF, 20000.0), (ENV_AMOUNT, 0.0), (RESONANCE, 0.0), (KEYBOARD, 0.0), (A_SUSTAIN, 10.0)]
}

fn clean_b() -> Vec<(u32, f32)> {
    vec![(SLOP, 0.0), (MIX_A, 0.0), (MIX_B, 3.0), (B_FINE, 0.0), (B_SAW, 0.0), (B_TRI, 1.0), (CUTOFF, 20000.0), (ENV_AMOUNT, 0.0), (RESONANCE, 0.0), (KEYBOARD, 0.0), (A_SUSTAIN, 10.0)]
}

#[test]
fn tuned_within_a_cent_across_the_range_at_each_rate() {
    assert_tuned(&cq(), &clean_a(), &[24, 36, 48, 60, 69, 84, 96, 108], 0.0, 1.0);
    assert_tuned(&cq(), &clean_b(), &[36, 60, 96], 0.0, 1.0);
    assert_tuned(&cq(), &with(clean_a(), &[(A_FREQ, 7.0)]), &[60], 7.0, 1.0);
    assert_tuned(&cq(), &with(clean_b(), &[(B_FREQ, -12.0)]), &[60], -12.0, 1.0);
}

#[test]
fn aliasing_end_to_end() {
    let at = |s: &[(u32, f32)], note: u8, what: &str, limit: f64, f0_st: f64| {
        let mut t = make(&cq(), SR, 77, s);
        let (l, _) = render(t.as_mut(), SR, 1 << 17, 512, &[(0, [0x90, note, 100])]);
        let a = alias_db(&l[1 << 16..], SR, exact_hz(note as f64 + f0_st) as f64);
        println!("aliasing, {what} at note {note}: {a:.1} dB");
        assert!(a < limit, "{what}: {a}");
    };
    at(&clean_a(), 96, "saw", -55.0, 0.0);
    at(&with(clean_a(), &[(A_SAW, 0.0), (A_PULSE, 1.0), (A_PW, 0.3)]), 96, "pulse", -55.0, 0.0);
    // A hard-synced saw, at the master's pitch.
    at(&with(clean_a(), &[(A_SYNC, 1.0), (A_FREQ, 7.3), (B_FINE, 0.0)]), 84, "sync", -45.0, 0.0);
    // Poly Mod from Osc B: exponential FM at the audio rate, the worst this instrument does. Synced
    // to B the result repeats with B's cycle, so what isn't a harmonic of it is aliasing. (Without
    // sync the exponential FM moves the mean pitch, as it should, and there's nothing to compare with.)
    at(&with(clean_a(), &[(A_SYNC, 1.0), (A_FREQ, 12.0), (PM_OSCB, 1.0), (PM_FREQ_A, 1.0), (B_FINE, 0.0)]), 72, "Poly Mod to pitch, synced", -40.0, 0.0);
}

/// How much of a tone's energy lies away from its harmonics, in dB against its strongest part.
fn off_harmonic_db(x: &[f32], f0: f64) -> f64 {
    alias_db(x, SR, f0)
}

/// Renders note 60 for a second and returns the settled half.
fn settled(s: &[(u32, f32)]) -> Vec<f32> {
    let mut t = make(&cq(), SR, 77, s);
    let (l, _) = render(t.as_mut(), SR, 1 << 17, 512, &[(0, [0x90, 60, 100])]);
    l[1 << 16..].to_vec()
}

/// A base where Osc A sounds alone and B runs out of the mixer, off the keyboard at an inharmonic
/// rate: anything of B's that reaches the output got there through Poly Mod.
fn poly_base() -> Vec<(u32, f32)> {
    with(clean_a(), &[(A_SAW, 1.0), (B_KBD, 0.0), (B_FREQ, 5.3), (B_FINE, 0.0), (B_SAW, 0.0), (B_TRI, 1.0)])
}

#[test]
fn poly_mod_osc_b_reaches_each_destination_only_by_its_switch() {
    let f0 = exact_hz(60.0) as f64;
    let none = off_harmonic_db(&settled(&poly_base()), f0);
    println!("no Poly Mod: {none:.1} dB off the harmonics");
    assert!(none < -60.0, "{none}");
    // The source up with no destination switched on does nothing.
    let src_only = off_harmonic_db(&settled(&with(poly_base(), &[(PM_OSCB, 5.0)])), f0);
    assert!(src_only < -60.0, "source with no destination: {src_only}");
    // Destination on with the source down does nothing either.
    let dst_only = off_harmonic_db(&settled(&with(poly_base(), &[(PM_FREQ_A, 1.0)])), f0);
    assert!(dst_only < -60.0, "destination with no source: {dst_only}");
    // Freq A.
    let freq = off_harmonic_db(&settled(&with(poly_base(), &[(PM_OSCB, 1.0), (PM_FREQ_A, 1.0)])), f0);
    println!("Osc B to Freq A: {freq:.1} dB off the harmonics");
    assert!(freq > -20.0, "{freq}");
    // PW A.
    let pw = off_harmonic_db(&settled(&with(poly_base(), &[(A_SAW, 0.0), (A_PULSE, 1.0), (A_PW, 0.5), (PM_OSCB, 5.0), (PM_PW_A, 1.0)])), f0);
    println!("Osc B to PW A: {pw:.1} dB off the harmonics");
    assert!(pw > -30.0, "{pw}");
    // But not when A has no pulse in it: PW then touches nothing.
    let pw_saw = off_harmonic_db(&settled(&with(poly_base(), &[(PM_OSCB, 5.0), (PM_PW_A, 1.0)])), f0);
    assert!(pw_saw < -60.0, "{pw_saw}");
    // Filter.
    let filt = off_harmonic_db(&settled(&with(poly_base(), &[(CUTOFF, 600.0), (PM_OSCB, 2.0), (PM_FILTER, 1.0)])), f0);
    println!("Osc B to Filter: {filt:.1} dB off the harmonics");
    assert!(filt > -30.0, "{filt}");
}

#[test]
fn poly_mod_filter_envelope_moves_pitch_and_is_per_voice() {
    // The envelope to Freq A: high at the start of the note, back to pitch by the end.
    let s = with(clean_a(), &[(A_SAW, 0.0), (A_PULSE, 1.0), (PM_FENV, 5.0), (PM_FREQ_A, 1.0), (F_ATTACK, 1.0), (F_DECAY, 300.0), (F_SUSTAIN, 0.0)]);
    let mut t = make(&cq(), SR, 77, &s);
    let (l, _) = render(t.as_mut(), SR, 96000, 512, &[(0, [0x90, 48, 100])]);
    let crossings = |x: &[f32]| x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32 * SR / x.len() as f32;
    let (early, late) = (crossings(&l[100..1000]), crossings(&l[70000..96000]));
    println!("envelope to Freq A: {early:.0} Hz early, {late:.0} Hz at the end");
    assert!(early > 2.5 * late, "{early} {late}");
    assert!(cents(late, exact_hz(48.0)).abs() < 25.0, "{late}");

    // Two notes, the second much later, keep their own envelope's pitch: per voice.
    let mut t = make(&cq(), SR, 77, &s);
    let (l, _) = render(t.as_mut(), SR, 144000, 512, &[(0, [0x90, 48, 100]), (96000, [0x90, 48, 100])]);
    // The late restart of a same-note retrigger is another attack; a new key on a new voice is what
    // matters, and each voice has its own envelope.
    assert!(l.iter().all(|v| v.is_finite()));
    let mut c = Cinque::new(SR, 3);
    c.set_param(PM_FENV, 5.0);
    c.set_param(PM_FREQ_A, 1.0);
    c.set_param(F_SUSTAIN, 0.0);
    c.set_param(F_DECAY, 100.0);
    render(&mut c, SR, 24000, 512, &[(0, [0x90, 48, 100]), (23000, [0x90, 55, 100])]);
    let (a, b) = (c.voices[0].fenv(), c.voices[1].fenv());
    println!("two voices' filter envelopes: {a:.3} and {b:.3}");
    assert!(b > a + 0.2, "{a} {b}");
}

#[test]
fn osc_b_stays_a_modulator_with_its_mixer_knob_down() {
    let s = with(poly_base(), &[(PM_OSCB, 1.0), (PM_FREQ_A, 1.0)]);
    let a = settled(&s);
    let b = settled(&with(s.clone(), &[(MIX_B, 0.0)]));
    assert!(a == b);
    // And with no B wave on, it has nothing to modulate with.
    let none = off_harmonic_db(&settled(&with(s, &[(B_TRI, 0.0)])), exact_hz(60.0) as f64);
    assert!(none < -60.0, "{none}");
}

#[test]
fn sync_locks_a_to_bs_pitch() {
    let s = with(clean_a(), &[(A_SYNC, 1.0), (A_FREQ, 7.0), (B_FINE, 0.0)]);
    let x = settled(&s);
    // Everything's a harmonic of B (note 60), though A alone would be note 67.
    let a = alias_db(&x, SR, exact_hz(60.0) as f64);
    println!("sync: {a:.1} dB off B's harmonics");
    assert!(a < -50.0, "{a}");
}

#[test]
fn wheel_mod_goes_through_its_switches() {
    let vib = |wheel: u8, sw: f32| {
        let s = with(clean_a(), &[(A_SAW, 0.0), (A_PULSE, 1.0), (LFO_RATE, 7.0), (WM_FREQ_A, sw), (WM_FREQ_B, 0.0)]);
        let mut t = make(&cq(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 96000, 512, &[(0, [0xB0, 1, wheel]), (1, [0x90, 69, 100])]);
        let slices: Vec<usize> = l[4800..].chunks(960).map(|c| c.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()).collect();
        slices.iter().max().unwrap() - slices.iter().min().unwrap()
    };
    assert!(vib(0, 1.0) <= 1, "no wheel: {}", vib(0, 1.0));
    assert!(vib(127, 0.0) <= 1, "no switch: {}", vib(127, 0.0));
    assert!(vib(127, 1.0) > 5, "both: {}", vib(127, 1.0));
}

#[test]
fn five_voices_and_unison() {
    let active = |t: &Cinque| t.voices.iter().filter(|v| v.active).count();
    let mut t = Cinque::new(SR, 7);
    let chord: Vec<Event> = (0..8u8).map(|k| (k as usize, [0x90, 48 + k, 100])).collect();
    render(&mut t, SR, 2000, 512, &chord);
    assert_eq!(active(&t), 5);
    // Unison: all five on the lowest key.
    let mut t = Cinque::new(SR, 7);
    t.set_param(UNISON, 1.0);
    t.set_param(SLOP, 0.0);
    t.set_param(UNISON_DETUNE, 0.0);
    t.set_param(MIX_B, 0.0);
    t.set_param(CUTOFF, 20000.0);
    t.set_param(ENV_AMOUNT, 0.0);
    t.set_param(KEYBOARD, 0.0);
    let (l, _) = render(&mut t, SR, 1 << 17, 512, &[(0, [0x90, 60, 100]), (5, [0x90, 72, 100])]);
    assert_eq!(active(&t), 5);
    assert!(cents(pitch_hz(&l, SR), exact_hz(60.0)).abs() < 1.0);
}

#[test]
fn release_switch_and_the_chips() {
    let tail = |on: f32| {
        let s = with(clean_a(), &[(A_RELEASE, 1000.0), (RELEASE_ON, on)]);
        let mut t = make(&cq(), SR, 77, &s);
        let (l, _) = render(t.as_mut(), SR, 48000, 512, &[(0, [0x90, 60, 100]), (12000, [0x80, 60, 0])]);
        peak(&l[14400..16800])
    };
    assert!(tail(1.0) > 0.05 && tail(0.0) < 1e-3, "{} {}", tail(1.0), tail(0.0));

    // The chips differ, at a level where each stage works.
    let brightness = |chip: f32| {
        let s = with(clean_a(), &[(CHIP, chip), (MIX_A, 10.0), (CUTOFF, 800.0), (RESONANCE, 5.0)]);
        rms(&settled(&s))
    };
    let (e, l) = (brightness(0.0), brightness(1.0));
    println!("early chip {e:.3}, late chip {l:.3}");
    assert!((e - l).abs() > 0.01, "{e} {l}");
}

#[test]
fn no_nan_or_runaway_on_extreme_settings() {
    let cases: Vec<Vec<(u32, f32)>> = vec![
        vec![(MIX_A, 10.0), (MIX_B, 10.0), (MIX_NOISE, 10.0), (A_PULSE, 1.0), (B_TRI, 1.0), (B_PULSE, 1.0), (RESONANCE, 10.0), (ENV_AMOUNT, 10.0), (VOLUME, 10.0)],
        vec![(PM_FENV, 10.0), (PM_OSCB, 10.0), (PM_FREQ_A, 1.0), (PM_PW_A, 1.0), (PM_FILTER, 1.0), (A_PULSE, 1.0), (B_FREQ, 36.0), (VOLUME, 10.0), (RESONANCE, 10.0)],
        vec![(UNISON, 1.0), (UNISON_DETUNE, 1.0), (A_SYNC, 1.0), (A_FREQ, 36.0), (MOD_REST, 10.0), (WM_MIX, 5.0), (WM_FILTER, 1.0), (WM_PW_A, 1.0), (WM_PW_B, 1.0), (VOLUME, 10.0), (CHIP, 0.0)],
        vec![(F_ATTACK, 1.0), (F_DECAY, 2.0), (F_SUSTAIN, 10.0), (A_ATTACK, 1.0), (A_DECAY, 2.0), (A_SUSTAIN, 10.0), (GLIDE, 10.0), (BEND_RANGE, 12.0), (VOLUME, 10.0), (B_LO, 1.0), (B_KBD, 0.0)],
    ];
    assert_no_runaway(&cq(), &cases, 16.0);
    assert_survives_param_jumps(&cq(), 32.0);
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
    let s = [(PM_OSCB, 2.0), (PM_FENV, 2.0), (PM_FREQ_A, 1.0), (PM_FILTER, 1.0), (A_PULSE, 1.0), (MIX_NOISE, 1.0), (SLOP, 1.0), (WM_MIX, 3.0), (WM_FILTER, 1.0), (A_RELEASE, 20.0), (F_RELEASE, 20.0)];
    assert_block_size_independent(&cq(), &s, &busy(), 180_000);
    let s = [(UNISON, 1.0), (GLIDE, 3.0), (A_SYNC, 1.0), (SLOP, 1.0)];
    assert_block_size_independent(&cq(), &s, &busy(), 180_000);
}

#[test]
fn renders_repeat_and_seed_matters() {
    let s = [(SLOP, 1.0), (MIX_NOISE, 1.0), (WM_MIX, 5.0)];
    let events = [(0usize, [0x90, 48, 100]), (20000, [0x80, 48, 0]), (24000, [0x90, 55, 100]), (40000, [0xB0, 1, 90]), (41000, [0xE0, 0, 90]), (42000, [0xB0, 64, 127])];
    assert_repeats(&cq(), &s, &events, 96000);
    assert_seed_matters(&cq(), &s, &events, 48000);
}

fn dark() -> Vec<(u32, f32)> {
    vec![(SLOP, 0.0), (B_FINE, 0.0), (MIX_B, 0.0), (MIX_A, 6.0), (CUTOFF, 700.0), (ENV_AMOUNT, 0.0), (KEYBOARD, 0.0), (RESONANCE, 0.0), (A_ATTACK, 5.0), (A_SUSTAIN, 10.0), (RELEASE_ON, 0.0)]
}

#[test]
fn no_click_on_note_off_steal_and_knobs() {
    assert_no_click(&cq(), &dark(), &[(0, [0x90, 60, 100]), (24000, [0x80, 60, 0])], 12000..24000, 23990..24800, 1.25, "note-off");
    let mut steal: Vec<Event> = (0..5u8).map(|k| (k as usize, [0x90, 48 + k * 2, 100])).collect();
    steal.push((36000, [0x90, 60, 100]));
    assert_no_click(&cq(), &dark(), &steal, 24000..36000, 35990..37000, 2.0, "voice steal");

    let mut t = make(&cq(), SR, 77, &dark());
    let (mut l, mut r) = (vec![0.0f32; 96000], vec![0.0f32; 96000]);
    t.process(&mut l[..512], &mut r[..512], &[MidiEvent { offset: 0, data: [0x90, 48, 100] }], &ctx(SR));
    let mut at = 512;
    while at + 512 <= 96000 {
        if at > 48000 {
            let k = ((at / 512) % 4) as f32;
            t.set_param(VOLUME, 5.0 + k * 0.5);
            t.set_param(CUTOFF, 600.0 + 100.0 * k);
            t.set_param(MIX_A, 5.0 + 0.5 * k);
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
fn filter_self_oscillates_near_the_top_and_tracks() {
    let mut worst = 0.0f32;
    for &sr in &[44100.0f32, 48000.0, 96000.0] {
        for note in (36..=84).step_by(12) {
            let s = [(SLOP, 0.0), (MIX_A, 0.0), (MIX_B, 0.0), (CUTOFF, exact_hz(60.0)), (RESONANCE, 10.0), (ENV_AMOUNT, 0.0), (KEYBOARD, 2.0), (A_SUSTAIN, 10.0)];
            let mut t = make(&cq(), sr, 77, &s);
            let (l, _) = render(t.as_mut(), sr, 1 << 18, 512, &[(0, [0x90, note as u8, 100])]);
            assert!(peak(&l[1 << 17..]) > 0.02, "note {note} at {sr}");
            let c = cents(pitch_hz(&l, sr), exact_hz(note as f64));
            worst = worst.max(c.abs());
            assert!(c.abs() < 30.0, "note {note} at {sr}: {c}");
        }
    }
    println!("Cinque self-oscillation 36..84: worst {worst:.1} cents");
}

#[test]
fn every_factory_preset_plays_at_a_sane_level() {
    assert_presets(&cq(), [40, 64]);
}

#[test]
fn goes_to_rest_and_tail_reaches_the_end() {
    assert_goes_silent(&cq(), &[(A_RELEASE, 40.0)], 2.0);
    assert_tail_is_long_enough(&cq(), &[(A_RELEASE, 800.0)]);
    assert_tail_is_long_enough(&cq(), &[(RELEASE_ON, 0.0)]);
}

#[test]
fn switching_unison_while_playing_doesnt_strand_a_note() {
    let mut t = Cinque::new(SR, 7);
    t.set_param(RELEASE_ON, 0.0);
    let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
    t.process(&mut l, &mut r, &[MidiEvent { offset: 0, data: [0x90, 60, 100] }], &ctx(SR));
    t.set_param(UNISON, 1.0);
    t.process(&mut l, &mut r, &[], &ctx(SR));
    t.process(&mut l, &mut r, &[MidiEvent { offset: 0, data: [0x80, 60, 0] }], &ctx(SR));
    for _ in 0..1000 {
        t.process(&mut l, &mut r, &[], &ctx(SR));
    }
    assert!(t.voices.iter().all(|v| !v.active));
}
