//! Transient (onset) detection, for the Audio Track editor's transient markers and Split at
//! Transients.
//!
//! The signal is measured as the peak level of short hops (about 1.3 ms). A slow envelope
//! follows the recent level; an onset is a hop whose peak jumps above that envelope by more
//! than a threshold, after which the detector waits for the level to settle before it can
//! fire again. Each onset is then moved back to where its attack starts, at a zero crossing,
//! so a split there doesn't cut into the hit.

/// Length of an analysis hop in seconds (64 frames at 48 kHz).
const HOP_SECONDS: f64 = 1.0 / 750.0;
/// Time constant of the slow envelope.
const SLOW_SECONDS: f64 = 0.030;
/// Level treated as silence.
const FLOOR: f32 = 1e-6;

fn db(v: f32) -> f32 {
    20.0 * v.max(FLOOR).log10()
}

/// Detection settings derived from a 0..1 sensitivity.
struct Params {
    /// How far (dB) a hop must rise above the slow envelope.
    rise_db: f32,
    /// Hops quieter than this (dBFS) never count, so noise and reverb tails are ignored.
    gate_db: f32,
    /// Shortest distance between two onsets, in hops.
    min_gap: usize,
}

impl Params {
    fn new(sensitivity: f32, loudest: f32, rate: f64, hop: usize) -> Params {
        let s = sensitivity.clamp(0.0, 1.0);
        Params {
            // 20 dB at the lowest sensitivity down to 6 dB at the highest.
            rise_db: 20.0 - 14.0 * s,
            // 24 dB below the loudest hop down to 60 dB below.
            gate_db: db(loudest) - (24.0 + 36.0 * s),
            // 80 ms down to 30 ms.
            min_gap: (((0.080 - 0.050 * s as f64) * rate) / hop as f64).round().max(1.0) as usize,
        }
    }
}

/// Onset positions (frames from the start of the audio, ascending) in `channels`. Higher
/// `sensitivity` (0..1) finds softer and closer hits.
pub fn detect(channels: &[Vec<f32>], sample_rate: u32, sensitivity: f32) -> Vec<i64> {
    let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    if frames == 0 {
        return Vec::new();
    }
    let rate = sample_rate.max(1) as f64;
    let hop = ((rate * HOP_SECONDS).round() as usize).max(8);
    let hops = frames.div_ceil(hop);

    // Peak per hop, across channels.
    let mut peak = vec![0.0f32; hops];
    for c in channels {
        for (k, chunk) in c[..frames].chunks(hop).enumerate() {
            let m = chunk.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            peak[k] = peak[k].max(m);
        }
    }
    let loudest = peak.iter().fold(0.0f32, |m, &v| m.max(v));
    if loudest < 1e-5 {
        return Vec::new();
    }
    let p = Params::new(sensitivity, loudest, rate, hop);
    let decay = (-(hop as f64) / (SLOW_SECONDS * rate)).exp() as f32;

    let mut onsets = Vec::new();
    let mut slow = FLOOR;
    let mut armed = true;
    let mut last: Option<usize> = None;
    for k in 0..hops {
        let fast_db = db(peak[k]);
        let rise = fast_db - db(slow);
        if armed && rise >= p.rise_db && fast_db >= p.gate_db && last.is_none_or(|l| k - l >= p.min_gap) {
            let at = attack_start(channels, frames, k, hop, slow, peak[k]);
            if onsets.last().is_none_or(|&prev| at > prev) {
                onsets.push(at);
            }
            last = Some(k);
            armed = false;
        } else if rise < p.rise_db * 0.5 {
            // Settled (or decaying): the next jump counts as a new hit.
            armed = true;
        }
        slow = decay * slow + (1.0 - decay) * peak[k];
    }
    onsets
}

/// Where the attack detected in hop `k` begins: the first sample, from the hop before, that
/// clearly stands out from the `background` level, moved back to the zero crossing before it.
fn attack_start(channels: &[Vec<f32>], frames: usize, k: usize, hop: usize, background: f32, level: f32) -> i64 {
    let mono = |i: usize| channels.iter().map(|c| c[i]).sum::<f32>();
    let abs_max = |i: usize| channels.iter().fold(0.0f32, |m, c| m.max(c[i].abs()));
    let from = k.saturating_sub(1) * hop;
    let to = ((k + 1) * hop).min(frames);
    let threshold = (background * 2.0).max(level * 0.1);
    let Some(first) = (from..to).find(|&i| abs_max(i) >= threshold) else { return (k * hop) as i64 };
    // Follow the attack's slope back towards zero (same sign, shrinking), at most a hop.
    let sign = mono(first) >= 0.0;
    let mut i = first;
    let limit = first.saturating_sub(hop);
    while i > limit && (mono(i - 1) >= 0.0) == sign && mono(i - 1).abs() < mono(i).abs() {
        i -= 1;
    }
    i as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48000;

    /// Deterministic low-level noise, so tests don't depend on a random source.
    fn noise(n: usize, level: f32) -> Vec<f32> {
        let mut x: u32 = 0x1234_5678;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * level
            })
            .collect()
    }

    /// Adds a decaying sine burst (a drum-like hit) at `at`.
    fn hit(buf: &mut [f32], at: usize, amp: f32, freq: f32) {
        for i in 0..(RATE as usize / 4).min(buf.len() - at) {
            let t = i as f32 / RATE as f32;
            buf[at + i] += amp * (-t / 0.05).exp() * (std::f32::consts::TAU * freq * t).sin();
        }
    }

    /// Adds a short click: a few samples of a fast-decaying impulse.
    fn click(buf: &mut [f32], at: usize, amp: f32) {
        for (i, g) in [1.0, -0.7, 0.45, -0.25, 0.1].iter().enumerate() {
            buf[at + i] += amp * g;
        }
    }

    fn near(found: &[i64], expected: &[usize], before_ms: f64, after_ms: f64) -> bool {
        let ms = |v: f64| (v * RATE as f64 / 1000.0) as i64;
        found.len() == expected.len() && found.iter().zip(expected).all(|(&f, &e)| f >= e as i64 - ms(before_ms) && f <= e as i64 + ms(after_ms))
    }

    #[test]
    fn finds_each_hit_of_a_drum_pattern() {
        let mut buf = noise(RATE as usize * 4, 0.001);
        // Eighth notes at 120 bpm with uneven levels and two pitches (kick and snare-ish).
        let at: Vec<usize> = (0..14).map(|i| 6000 + i * 12000).collect();
        for (i, &a) in at.iter().enumerate() {
            hit(&mut buf, a, [0.9, 0.4, 0.7, 0.3][i % 4], if i % 2 == 0 { 70.0 } else { 220.0 });
        }
        let found = detect(&[buf], RATE, 0.5);
        assert!(near(&found, &at, 1.0, 1.0), "{found:?} vs {at:?}");
    }

    #[test]
    fn finds_clicks_to_the_sample_in_stereo() {
        let n = RATE as usize * 2;
        let (mut l, mut r) = (noise(n, 0.0005), noise(n, 0.0005));
        let at: Vec<usize> = (0..8).map(|i| 1000 + i * 11_025).collect();
        for &a in &at {
            click(&mut l, a, 0.8);
            click(&mut r, a, 0.6);
        }
        let found = detect(&[l, r], RATE, 0.5);
        assert!(near(&found, &at, 0.1, 0.05), "{found:?} vs {at:?}");
    }

    #[test]
    fn sustained_tone_is_one_onset() {
        // Silence, then a tone with gentle tremolo: only its start is a transient.
        let mut buf = vec![0.0f32; RATE as usize * 3];
        let start = 4800;
        for (i, v) in buf.iter_mut().enumerate().skip(start) {
            let t = (i - start) as f32 / RATE as f32;
            let trem = 1.0 + 0.15 * (std::f32::consts::TAU * 5.0 * t).sin();
            *v = 0.5 * trem * (std::f32::consts::TAU * 440.0 * t).sin();
        }
        let found = detect(&[buf], RATE, 0.8);
        assert!(near(&found, &[start], 1.0, 1.0), "{found:?}");
    }

    #[test]
    fn sensitivity_decides_about_soft_hits() {
        let mut buf = noise(RATE as usize * 2, 0.00002);
        hit(&mut buf, 4800, 0.9, 120.0);
        // 36 dB down: below the gate at low sensitivity, found at high.
        hit(&mut buf, 48000, 0.9 * 0.016, 120.0);
        hit(&mut buf, 72000, 0.9, 120.0);
        assert_eq!(detect(std::slice::from_ref(&buf), RATE, 0.0).len(), 2);
        let all = detect(&[buf], RATE, 1.0);
        assert!(near(&all, &[4800, 48000, 72000], 1.0, 1.0), "{all:?}");
    }

    #[test]
    fn silence_and_empty_audio_have_none() {
        assert!(detect(&[vec![0.0; 48000]], RATE, 1.0).is_empty());
        assert!(detect(&[], RATE, 0.5).is_empty());
        assert!(detect(&[noise(48000, 1e-6)], RATE, 1.0).is_empty());
    }
}
