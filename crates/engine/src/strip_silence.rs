//! Strip Silence (Logic's Functions › Remove Silence): finds the parts of a region's audio that
//! sound, so the region can be split into one region per part and the silence between them
//! left out. Pure; `session/arrange_cmds.rs` cuts the regions.
//!
//! A sample sounds when its level (the loudest channel, times the region's gain) reaches the
//! threshold. Sounding samples less than the minimum time apart belong to the same part, and so
//! do any less than [`MIN_GAP`] apart whatever the setting, or a low note's zero crossings
//! would cut it into pieces. Each part then starts the pre-attack time earlier and ends the
//! post-release time later (so attacks and tails aren't clipped), and parts that now touch are
//! joined.

use serde::Deserialize;

/// The shortest silence that ever counts, in seconds: half a period of 20 Hz.
pub const MIN_GAP: f64 = 0.025;

/// The Strip Silence sheet's settings.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct StripSettings {
    /// Linear level (1.0 is full scale) a sample must reach to count as sound.
    pub threshold: f32,
    /// Minimum Time to Accept as Silence, in seconds.
    pub min_time: f64,
    /// Pre-Attack Time, in seconds: how much earlier each part starts.
    pub pre_attack: f64,
    /// Post-Release Time, in seconds: how much later each part ends.
    pub post_release: f64,
}

impl Default for StripSettings {
    /// Logic's defaults: 4 %, 0.1 s, 0.02 s and 0.1 s.
    fn default() -> Self {
        StripSettings { threshold: 0.04, min_time: 0.1, pre_attack: 0.02, post_release: 0.1 }
    }
}

/// The parts of `channels[..][from..to]` that sound, as frame ranges relative to `from`, in
/// order and not touching. `gain` scales the samples (the region's gain); `rate` is the sample
/// rate. Frames past the end of the audio are silent.
pub fn sounding_parts(channels: &[Vec<f32>], from: usize, to: usize, gain: f32, rate: f64, s: &StripSettings) -> Vec<(i64, i64)> {
    let n = to.saturating_sub(from);
    let secs = |t: f64| (t.max(0.0) * rate).round() as i64;
    let gap = secs(s.min_time.max(MIN_GAP)).max(1);
    let thr = s.threshold.max(1e-6);
    let loud = |i: usize| channels.iter().any(|c| c.get(from + i).is_some_and(|x| (x * gain).abs() >= thr));
    // Runs of sound, bridging silences shorter than `gap`.
    let mut runs: Vec<(i64, i64)> = Vec::new();
    for i in 0..n {
        if !loud(i) {
            continue;
        }
        let i = i as i64;
        match runs.last_mut() {
            Some(r) if i - r.1 < gap => r.1 = i + 1,
            _ => runs.push((i, i + 1)),
        }
    }
    // Pre-attack and post-release, then join what now touches.
    let (pre, post) = (secs(s.pre_attack), secs(s.post_release));
    let mut out: Vec<(i64, i64)> = Vec::with_capacity(runs.len());
    for (a, b) in runs {
        let (a, b) = ((a - pre).max(0), (b + post).min(n as i64));
        match out.last_mut() {
            Some(r) if a <= r.1 => r.1 = r.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// A threshold in dBFS as the linear level [`StripSettings::threshold`] takes.
pub fn db_to_level(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 1000.0;

    /// A 100 Hz tone at `amp` for each (start, end) frame range, silence elsewhere.
    fn tone(len: usize, parts: &[(usize, usize)], amp: f32) -> Vec<f32> {
        let mut v = vec![0.0f32; len];
        for &(a, b) in parts {
            for (i, x) in v[a..b].iter_mut().enumerate() {
                *x = amp * (std::f32::consts::TAU * 100.0 * i as f32 / SR as f32).sin();
            }
        }
        v
    }

    fn tight(threshold: f32) -> StripSettings {
        StripSettings { threshold, min_time: 0.05, pre_attack: 0.0, post_release: 0.0 }
    }

    #[test]
    fn a_tone_with_gaps_splits_into_its_notes() {
        // Notes at 100..300 and 500..600, a quiet one at 800..900, in a second of audio.
        let mut a = tone(1000, &[(100, 300), (500, 600)], 0.5);
        a[800..900].copy_from_slice(&tone(100, &[(0, 100)], 0.01));
        let ch = vec![a];
        let parts = sounding_parts(&ch, 0, 1000, 1.0, SR, &tight(0.04));
        // The sine's first sample is a zero crossing, and the last loud one is a little before
        // the end; zero crossings inside a note don't cut it.
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert!((parts[0].0 - 100).abs() <= 2 && (parts[0].1 - 300).abs() <= 3, "{parts:?}");
        assert!((parts[1].0 - 500).abs() <= 2 && (parts[1].1 - 600).abs() <= 3, "{parts:?}");
        // A lower threshold takes in the quiet note too.
        assert_eq!(sounding_parts(&ch, 0, 1000, 1.0, SR, &tight(0.005)).len(), 3);
        // Region gain counts: at -40 dB nothing reaches 4 %.
        assert!(sounding_parts(&ch, 0, 1000, 0.01, SR, &tight(0.04)).is_empty());
    }

    #[test]
    fn short_silences_dont_count() {
        let ch = vec![tone(1000, &[(100, 300), (340, 600)], 0.5)];
        assert_eq!(sounding_parts(&ch, 0, 1000, 1.0, SR, &tight(0.04)).len(), 1, "a 40 ms gap is under the 50 ms minimum");
        let s = StripSettings { min_time: 0.03, ..tight(0.04) };
        assert_eq!(sounding_parts(&ch, 0, 1000, 1.0, SR, &s).len(), 2);
    }

    #[test]
    fn pre_attack_and_post_release_widen_the_parts_and_join_them() {
        let ch = vec![tone(1000, &[(100, 300), (500, 600)], 0.5)];
        let s = StripSettings { pre_attack: 0.02, post_release: 0.05, ..tight(0.04) };
        let parts = sounding_parts(&ch, 0, 1000, 1.0, SR, &s);
        assert!((parts[0].0 - 80).abs() <= 2 && (parts[0].1 - 350).abs() <= 3, "{parts:?}");
        // A long release reaches the next note: one part.
        let s = StripSettings { post_release: 0.25, ..s };
        let parts = sounding_parts(&ch, 0, 1000, 1.0, SR, &s);
        assert_eq!(parts.len(), 1);
        assert!((parts[0].1 - 850).abs() <= 3, "{parts:?}");
        // Widening stops at the region's edges.
        let s = StripSettings { pre_attack: 1.0, post_release: 1.0, ..tight(0.04) };
        assert_eq!(sounding_parts(&ch, 0, 1000, 1.0, SR, &s), vec![(0, 1000)]);
    }

    #[test]
    fn works_on_a_window_of_the_file_and_any_channel() {
        let left = vec![0.0f32; 1000];
        let right = tone(1000, &[(400, 500)], 0.5);
        let ch = vec![left, right];
        let parts = sounding_parts(&ch, 300, 700, 1.0, SR, &tight(0.04));
        assert_eq!(parts.len(), 1);
        assert!((parts[0].0 - 100).abs() <= 2, "relative to the window: {parts:?}");
        // Past the end of the file is silence.
        assert!(sounding_parts(&ch, 900, 2000, 1.0, SR, &tight(0.04)).is_empty());
        assert!((db_to_level(-20.0) - 0.1).abs() < 1e-6);
    }
}
