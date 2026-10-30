//! Pattern regions, as edited in Logic's Step Sequencer: rows of steps, each row one pitch.
//!
//! A pattern region is a MIDI region with a [`Pattern`] (`Region::pattern`). Its notes are
//! made from the pattern when the session builds the playback list (`Session::regions_dsp`),
//! as pure functions of the pattern, the region's position and length, and the tempo map, so
//! the audio thread does nothing new and every render (bounce, export, freeze, Bounce in
//! Place) plays it. [`realize`] turns a pattern region into the plain MIDI region it plays,
//! which is also Convert to MIDI Region; [`from_notes`] goes the other way.
//!
//! Timing: every row counts steps of its own rate from the region start. A row of `n` steps
//! repeats every `n` steps (rows of different lengths drift against each other: polymeter),
//! and the pattern plays on to the region's end when it loops, or stops after its own length
//! when it doesn't. Swing delays every second step of a straight row. A tied step carries the
//! note before it on instead of starting one.

use serde::{Deserialize, Serialize};

use crate::model::{Note, Region, RegionContent};
use crate::tempo::TempoMap;

/// Step rates as the steps per whole note: 1/4 to 1/32, then their triplets (6 = 1/4 triplet).
pub const RATES: [u16; 8] = [4, 8, 16, 32, 6, 12, 24, 48];
/// The most steps a pattern or a row has.
pub const MAX_STEPS: u16 = 64;
/// The most rows a pattern has.
pub const MAX_ROWS: usize = 64;
/// A new step's velocity.
pub const DEFAULT_VELOCITY: u8 = 100;

/// The rows of a new pattern region: the General MIDI drum map's kit pieces, from the kick up
/// (Logic's naming: C1 is 36).
pub const DEFAULT_KIT: [u8; 8] = [36, 38, 39, 42, 46, 45, 48, 49];

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_default_velocity(v: &u8) -> bool {
    *v == DEFAULT_VELOCITY
}

fn is_full_gate(g: &u8) -> bool {
    *g == 100
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// One step of a row. Off steps keep their values, so turning one on again brings them back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    #[serde(skip_serializing_if = "is_false")]
    pub on: bool,
    /// 1–127.
    #[serde(skip_serializing_if = "is_default_velocity")]
    pub velocity: u8,
    /// Note length in percent of the step, 1–100.
    #[serde(skip_serializing_if = "is_full_gate")]
    pub gate: u8,
    /// Carries the note of the step before on through this step (Logic's Tie).
    #[serde(skip_serializing_if = "is_false")]
    pub tie: bool,
}

impl Default for Step {
    fn default() -> Self {
        Step { on: false, velocity: DEFAULT_VELOCITY, gate: 100, tie: false }
    }
}

/// One row: a pitch and its steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternRow {
    pub pitch: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub solo: bool,
    /// The row's own step rate (see [`RATES`]); None follows the pattern's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<u16>,
    /// The row's own length in steps, after which it repeats; None follows the pattern's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u16>,
    /// The steps; missing ones at the end are off.
    #[serde(default)]
    pub steps: Vec<Step>,
}

impl PatternRow {
    pub fn new(pitch: u8) -> Self {
        PatternRow { pitch, muted: false, solo: false, rate: None, length: None, steps: Vec::new() }
    }

    /// Step `i` (off when the list doesn't reach it).
    pub fn step(&self, i: usize) -> Step {
        self.steps.get(i).copied().unwrap_or_default()
    }
}

/// A pattern region's pattern (Logic's Step Sequencer).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    /// Length in steps.
    pub length: u16,
    /// Steps per whole note (see [`RATES`]).
    pub rate: u16,
    /// Swing in percent: every second step of a straight row moves up to half a step later
    /// (0 = straight; about 66 is the triplet feel).
    #[serde(default)]
    pub swing: u8,
    /// Repeats to the region's end; off, the pattern plays once.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub looped: bool,
    pub rows: Vec<PatternRow>,
}

impl Default for Pattern {
    /// The pattern of a new pattern region: a bar of sixteenth notes on the default kit.
    fn default() -> Self {
        Pattern { length: 16, rate: 16, swing: 0, looped: true, rows: DEFAULT_KIT.iter().map(|&p| PatternRow::new(p)).collect() }
    }
}

/// Quarter notes per step at `rate` steps per whole note.
pub fn step_quarters(rate: u16) -> f64 {
    4.0 / rate.max(1) as f64
}

/// Whether a rate is a triplet (triplet rows don't swing).
pub fn is_triplet(rate: u16) -> bool {
    rate % 3 == 0
}

fn nearest_rate(rate: u16) -> u16 {
    if RATES.contains(&rate) {
        rate
    } else {
        *RATES.iter().min_by_key(|r| r.abs_diff(rate)).unwrap()
    }
}

impl Pattern {
    /// The rate row `r` plays at.
    pub fn row_rate(&self, r: &PatternRow) -> u16 {
        r.rate.unwrap_or(self.rate)
    }

    /// The steps row `r` has before it repeats.
    pub fn row_length(&self, r: &PatternRow) -> u16 {
        r.length.unwrap_or(self.length).max(1)
    }

    /// The pattern's length in quarter notes.
    pub fn quarters(&self) -> f64 {
        self.length as f64 * step_quarters(self.rate)
    }

    /// The same pattern with every value brought into range: rates to the nearest that
    /// exists, lengths 1–[`MAX_STEPS`], velocities 1–127, gates 1–100, at most [`MAX_ROWS`]
    /// rows, and no steps stored past a row's length.
    pub fn clamped(mut self) -> Self {
        self.rate = nearest_rate(self.rate);
        self.length = self.length.clamp(1, MAX_STEPS);
        self.swing = self.swing.min(100);
        self.rows.truncate(MAX_ROWS);
        for r in &mut self.rows {
            r.pitch = r.pitch.min(127);
            r.rate = r.rate.map(nearest_rate);
            r.length = r.length.map(|l| l.clamp(1, MAX_STEPS));
            let len = r.length.unwrap_or(MAX_STEPS) as usize;
            r.steps.truncate(len.max(self.length as usize).min(MAX_STEPS as usize));
            for s in &mut r.steps {
                s.velocity = s.velocity.clamp(1, 127);
                s.gate = s.gate.clamp(1, 100);
            }
        }
        self
    }

    /// The notes the pattern plays in a region at timeline sample `start` lasting `length`,
    /// relative to the region start and sorted. Muted rows are left out; when any row is
    /// soloed, only soloed rows play.
    pub fn notes(&self, start: i64, length: i64, map: &TempoMap) -> Vec<Note> {
        let q0 = map.beats_at(start as f64);
        let span = map.beats_at((start + length) as f64) - q0;
        let limit = if self.looped { span } else { span.min(self.quarters()) };
        let solo = self.rows.iter().any(|r| r.solo);
        let mut out = Vec::new();
        for row in self.rows.iter().filter(|r| if solo { r.solo } else { !r.muted }) {
            let rate = self.row_rate(row);
            let step = step_quarters(rate);
            let len = self.row_length(row) as usize;
            let swing = if is_triplet(rate) { 0.0 } else { step * self.swing as f64 / 200.0 };
            // Where step k starts sounding, swing included.
            let on_at = |k: usize| k as f64 * step + if k % 2 == 1 { swing } else { 0.0 };
            let count = ((limit / step) - 1e-9).ceil().max(0.0) as usize;
            let mut push = |on: f64, off: f64, velocity: u8| {
                let s = map.sample_at(q0 + on).round() as i64 - start;
                let e = map.sample_at(q0 + off).round() as i64 - start;
                if s < length {
                    out.push(Note { start: s, length: (e - s).max(1), pitch: row.pitch, velocity, muted: false });
                }
            };
            // The note being played: (on, off, velocity) in quarters from the region start.
            let mut held: Option<(f64, f64, u8)> = None;
            for k in 0..count {
                let s = row.step(k % len);
                let on = on_at(k);
                // A note ends at its gate, but never after the next step starts.
                let off = (on + step * s.gate as f64 / 100.0).min(on_at(k + 1));
                match &mut held {
                    Some(h) if s.tie => h.1 = off,
                    _ => {
                        if let Some((a, b, v)) = held.take() {
                            push(a, b, v);
                        }
                        if s.on {
                            held = Some((on, off, s.velocity));
                        }
                    }
                }
            }
            if let Some((a, b, v)) = held {
                push(a, b, v);
            }
        }
        out.sort_by_key(|n| (n.start, n.pitch));
        out
    }
}

/// The plain MIDI region a pattern region plays: its notes made from the pattern, the pattern
/// gone (Convert to MIDI Region). Controllers and playback parameters stay. Any other region
/// comes back unchanged.
pub fn realize(r: &Region, map: &TempoMap) -> Region {
    let mut out = r.clone();
    if let (Some(p), RegionContent::Midi { notes, .. }) = (out.pattern.take(), &mut out.content) {
        *notes = p.notes(r.start, r.length, map);
    }
    out
}

/// A pattern that plays MIDI notes as closely as sixteenth steps can (Convert to Pattern
/// Region): a row per pitch, lowest first, each note on the nearest step with its velocity, and
/// notes longer than a step tied on. The pattern covers the region up to [`MAX_STEPS`] steps.
pub fn from_notes(notes: &[Note], start: i64, length: i64, map: &TempoMap) -> Pattern {
    let rate = 16;
    let step = step_quarters(rate);
    let q0 = map.beats_at(start as f64);
    let span = map.beats_at((start + length) as f64) - q0;
    let steps = ((span / step - 1e-6).ceil() as i64).clamp(1, MAX_STEPS as i64) as usize;
    let mut pitches: Vec<u8> = notes.iter().filter(|n| !n.muted).map(|n| n.pitch).collect();
    pitches.sort_unstable();
    pitches.dedup();
    let mut rows: Vec<PatternRow> = pitches.iter().take(MAX_ROWS).map(|&p| PatternRow { steps: vec![Step::default(); steps], ..PatternRow::new(p) }).collect();
    for n in notes.iter().filter(|n| !n.muted && n.start >= 0 && n.start < length) {
        let Some(row) = rows.iter_mut().find(|r| r.pitch == n.pitch) else { continue };
        let on = map.beats_at((start + n.start) as f64) - q0;
        let k = (on / step).round() as usize;
        if k >= steps {
            continue;
        }
        let dur = (map.beats_at((start + n.start + n.length) as f64) - q0 - on) / step;
        // Whole steps it covers after its own, then what is left of the last one.
        let more = ((dur - 1e-6).ceil() as usize).saturating_sub(1);
        let last = ((dur - more as f64) * 100.0).round().clamp(1.0, 100.0) as u8;
        row.steps[k] = Step { on: true, velocity: n.velocity.clamp(1, 127), gate: if more == 0 { last } else { 100 }, tie: false };
        for j in 1..=more {
            let Some(s) = row.steps.get_mut(k + j) else { break };
            if s.on {
                break;
            }
            *s = Step { tie: true, gate: if j == more { last } else { 100 }, ..*s };
        }
    }
    Pattern { length: steps as u16, rate, swing: 0, looped: true, rows }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::TempoEvent;

    const SR: f64 = 48000.0;
    /// 120 BPM: 24000 samples per quarter, 6000 per sixteenth.
    const Q: i64 = 24000;
    const S16: i64 = Q / 4;

    fn map120() -> TempoMap {
        TempoMap::constant(SR, 120.0, 4, 4)
    }

    fn on(velocity: u8) -> Step {
        Step { on: true, velocity, ..Step::default() }
    }

    fn row(pitch: u8, on_steps: &[usize]) -> PatternRow {
        let mut r = PatternRow::new(pitch);
        for &k in on_steps {
            if r.steps.len() <= k {
                r.steps.resize(k + 1, Step::default());
            }
            r.steps[k] = on(DEFAULT_VELOCITY);
        }
        r
    }

    fn pattern(rows: Vec<PatternRow>) -> Pattern {
        Pattern { rows, ..Pattern::default() }
    }

    fn starts(notes: &[Note], pitch: u8) -> Vec<i64> {
        notes.iter().filter(|n| n.pitch == pitch).map(|n| n.start).collect()
    }

    #[test]
    fn steps_become_notes_and_loop_to_the_region_end() {
        let p = pattern(vec![row(36, &[0, 4, 8, 12]), row(38, &[4, 12])]);
        // Two bars: the one-bar pattern plays twice.
        let n = p.notes(0, 32 * S16, &map120());
        assert_eq!(starts(&n, 36), (0..8).map(|k| k * 4 * S16).collect::<Vec<_>>());
        assert_eq!(starts(&n, 38), vec![4 * S16, 12 * S16, 20 * S16, 28 * S16]);
        assert!(n.iter().all(|n| n.length == S16 && n.velocity == DEFAULT_VELOCITY));
        // Sorted by start.
        assert!(n.windows(2).all(|w| w[0].start <= w[1].start));
        // Half a bar: only what starts inside the region.
        let n = p.notes(0, 8 * S16, &map120());
        assert_eq!(starts(&n, 36), vec![0, 4 * S16]);
        // Not looped: once, then silence.
        let p = Pattern { looped: false, ..p };
        assert_eq!(starts(&p.notes(0, 32 * S16, &map120()), 36), vec![0, 4 * S16, 8 * S16, 12 * S16]);
    }

    #[test]
    fn timing_counts_from_the_region_start_through_the_tempo_map() {
        let p = pattern(vec![row(36, &[0, 1])]);
        let n = p.notes(1000, 16 * S16, &map120());
        assert_eq!(starts(&n, 36)[..2], [0, S16]);
        // 120 BPM for a bar, then 60: steps after the change are twice as long.
        let map = TempoMap::new(SR, &[TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: 4 * Q, bpm: 60.0, ramp: false }], &[], 120.0, (4, 4));
        let p = pattern(vec![row(36, &[0, 1, 2])]);
        let n = p.notes(4 * Q, 32 * S16, &map);
        assert_eq!(starts(&n, 36)[..3], [0, 2 * S16, 4 * S16]);
        assert_eq!(n[0].length, 2 * S16);
    }

    #[test]
    fn swing_delays_every_second_step_of_straight_rows() {
        let mut p = pattern(vec![row(42, &[0, 1, 2, 3])]);
        p.swing = 50;
        let n = p.notes(0, 16 * S16, &map120());
        // Half the swing range: a quarter of a step late.
        assert_eq!(starts(&n, 42), vec![0, S16 + S16 / 4, 2 * S16, 3 * S16 + S16 / 4]);
        // The swung note ends where the next step starts; the one before it keeps its length.
        assert_eq!(n[0].length, S16);
        assert_eq!(n[1].length, S16 - S16 / 4);
        // Triplet rows don't swing.
        p.rows[0].rate = Some(24);
        let n = p.notes(0, 16 * S16, &map120());
        assert_eq!(starts(&n, 42)[..2], [0, Q / 6]);
    }

    #[test]
    fn rows_of_other_lengths_and_rates_drift() {
        let mut three = row(36, &[0]);
        three.length = Some(3);
        let mut eighths = row(38, &[0, 1]);
        eighths.rate = Some(8);
        eighths.length = Some(4);
        let p = pattern(vec![three, eighths]);
        let n = p.notes(0, 16 * S16, &map120());
        // Every 3 sixteenths, through the pattern's own 16.
        assert_eq!(starts(&n, 36), vec![0, 3, 6, 9, 12, 15].iter().map(|k| k * S16).collect::<Vec<_>>());
        // Eighths, 4 to a cycle: steps 0, 1 then 4, 5.
        assert_eq!(starts(&n, 38), vec![0, 2 * S16, 8 * S16, 10 * S16]);
        assert_eq!(n.iter().find(|n| n.pitch == 38).unwrap().length, 2 * S16);
    }

    #[test]
    fn ties_gates_velocity_mute_and_solo() {
        let mut r = row(36, &[0, 4]);
        r.steps.resize(8, Step::default());
        r.steps[0].velocity = 77;
        r.steps[1].tie = true;
        r.steps[2] = Step { tie: true, gate: 50, ..Step::default() };
        // A tie after an off step does nothing.
        r.steps[6].tie = true;
        r.steps[4].gate = 25;
        let p = pattern(vec![r.clone(), row(38, &[0])]);
        let n = p.notes(0, 16 * S16, &map120());
        let kick: Vec<&Note> = n.iter().filter(|n| n.pitch == 36).collect();
        assert_eq!(kick.len(), 2);
        assert_eq!((kick[0].start, kick[0].length, kick[0].velocity), (0, 2 * S16 + S16 / 2, 77));
        assert_eq!((kick[1].start, kick[1].length), (4 * S16, S16 / 4));
        // Muted rows are silent; a soloed row silences the others.
        let mut p2 = p.clone();
        p2.rows[1].muted = true;
        assert!(starts(&p2.notes(0, 16 * S16, &map120()), 38).is_empty());
        p2.rows[1].solo = true;
        let n = p2.notes(0, 16 * S16, &map120());
        assert!(starts(&n, 36).is_empty());
        assert_eq!(starts(&n, 38), vec![0]);
        // A tie at step 0 carries the last step of the cycle before on, across the loop.
        let mut wrap = row(40, &[3]);
        wrap.length = Some(4);
        wrap.steps.resize(4, Step::default());
        wrap.steps[0].tie = true;
        let n = pattern(vec![wrap]).notes(0, 8 * S16, &map120());
        assert_eq!(n.iter().map(|n| (n.start, n.length)).collect::<Vec<_>>(), vec![(3 * S16, 2 * S16), (7 * S16, S16)]);
    }

    #[test]
    fn clamped_keeps_values_in_range() {
        let mut p = pattern(vec![row(200, &[0])]);
        p.rate = 15;
        p.length = 0;
        p.swing = 250;
        p.rows[0].rate = Some(100);
        p.rows[0].length = Some(500);
        p.rows[0].steps[0].velocity = 0;
        p.rows[0].steps[0].gate = 0;
        p.rows[0].steps.resize(200, Step::default());
        let c = p.clamped();
        assert_eq!((c.rate, c.length, c.swing), (16, 1, 100));
        assert_eq!((c.rows[0].pitch, c.rows[0].rate, c.rows[0].length), (127, Some(48), Some(MAX_STEPS)));
        assert_eq!((c.rows[0].steps[0].velocity, c.rows[0].steps[0].gate), (1, 1));
        assert_eq!(c.rows[0].steps.len(), MAX_STEPS as usize);
    }

    #[test]
    fn realize_and_from_notes_round_trip() {
        let map = map120();
        let mut r = row(36, &[0, 5, 10]);
        r.steps[5].velocity = 64;
        r.steps.resize(16, Step::default());
        r.steps[11].tie = true;
        r.steps[11].gate = 50;
        let p = pattern(vec![row(42, &[2, 6]), r]);
        let region = Region {
            id: 1,
            name: "Pattern".into(),
            start: 7 * Q,
            length: 16 * S16,
            muted: false,
            color: None,
            content: RegionContent::Midi { notes: Vec::new(), cc: Vec::new() },
            loop_end: None,
            takes: None,
            params: Default::default(),
            pattern: Some(p),
        };
        let plain = realize(&region, &map);
        assert!(plain.pattern.is_none());
        let RegionContent::Midi { notes, .. } = &plain.content else { unreachable!() };
        assert_eq!(notes.len(), 5);
        let back = from_notes(notes, region.start, region.length, &map);
        assert_eq!((back.length, back.rate), (16, 16));
        assert_eq!(back.rows.iter().map(|r| r.pitch).collect::<Vec<_>>(), vec![36, 42]);
        // Lowest pitch first, so the rows come back in a different order; the notes match.
        let key = |n: &[Note]| n.iter().map(|n| (n.start, n.length, n.pitch, n.velocity)).collect::<Vec<_>>();
        assert_eq!(key(&back.notes(region.start, region.length, &map)), key(notes));
        // A plain region is left alone.
        assert!(realize(&plain, &map).pattern.is_none());
    }

    #[test]
    fn old_and_compact_json() {
        // Off steps with default values are stored as {}; `looped` only when off.
        let p = pattern(vec![row(36, &[1])]);
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""steps":[{},{"on":true}]"#), "{json}");
        assert!(!json.contains("looped"));
        let back: Pattern = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }
}
