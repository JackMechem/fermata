//! Region playback parameters, Logic's Region inspector: non-destructive Quantize with Q-Swing,
//! Transpose, Velocity, Dynamics, Gate Time and Delay.
//!
//! The stored notes never change. What a region plays is worked out here, as pure functions
//! of its notes, its position and the tempo map, when the session builds the playback list
//! (`Session::regions_dsp`), so the audio thread does nothing new. The same functions bake the
//! parameters into the notes for Apply Region Parameters, Join and MIDI file export.
//!
//! The order is Logic's: quantize moves note starts (lengths are kept), gate time changes
//! lengths, delay shifts everything in time, then transpose, dynamics and velocity change the
//! notes themselves.

use serde::{Deserialize, Serialize};

use crate::model::{CcEvent, Note, Region, RegionContent};
use crate::tempo::TempoMap;

/// Ticks per quarter note, as the Region inspector's Delay counts them (Logic's 960).
pub const TICKS_PER_QUARTER: f64 = 960.0;

/// `gate` value for Logic's "Fixed": every note very short (a 1/64 note).
pub const GATE_FIXED: u16 = 0;
/// `gate` value for Logic's "Legato": every note lasts until the next one starts.
pub const GATE_LEGATO: u16 = 201;

/// Quantize values: the grid as a fraction of a whole note. Straight notes, then triplets
/// (1/3 is a half-note triplet, 1/12 an eighth-note triplet, as Logic names them).
pub const QUANTIZE_VALUES: [u16; 13] = [1, 2, 4, 8, 16, 32, 64, 3, 6, 12, 24, 48, 96];

/// Dynamics values (the ratio × 100, 0 = Fixed) and how much each scales a velocity's
/// distance from 64: 3:1 compresses it to a third, 0.5:1 expands it to double.
pub const DYNAMICS: [(u16, f64); 10] = [(0, 0.0), (300, 1.0 / 3.0), (200, 0.5), (150, 2.0 / 3.0), (125, 0.8), (100, 1.0), (80, 1.25), (67, 1.5), (50, 2.0), (33, 3.0)];

pub const TRANSPOSE_RANGE: (i16, i16) = (-96, 96);
pub const VELOCITY_RANGE: (i16, i16) = (-99, 127);
/// Delay limits: four quarter notes in ticks, two seconds in ms.
pub const DELAY_TICKS_MAX: f64 = 4.0 * TICKS_PER_QUARTER;
pub const DELAY_MS_MAX: f64 = 2000.0;

/// A region's playback parameters. The defaults change nothing. Audio regions use only the
/// delay.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegionParams {
    /// Quantize grid as a fraction of a whole note (16 = 1/16 note, 12 = 1/12 note, an eighth
    /// triplet); 0 = off. See [`QUANTIZE_VALUES`].
    pub quantize: u16,
    /// Q-Swing in percent: where every second grid point sits between its neighbours. 50 is
    /// straight, above 50 delays it, below 50 pulls it earlier. Only used while quantizing.
    pub swing: u8,
    /// Semitones.
    pub transpose: i16,
    /// Added to every velocity; the result is kept within 1–127.
    pub velocity: i16,
    /// Velocity compression or expansion around 64: the ratio × 100 (300 = 3:1, 100 = off,
    /// 50 = 0.5:1), 0 = Fixed (every note at 64). See [`DYNAMICS`].
    pub dynamics: u16,
    /// Gate time: note lengths in percent (10–200, 100 = unchanged), or [`GATE_FIXED`] or
    /// [`GATE_LEGATO`].
    pub gate: u16,
    /// Delay, in ticks ([`TICKS_PER_QUARTER`] to a quarter note) or, with `delay_ms`, in
    /// milliseconds. Negative values play early.
    pub delay: f64,
    pub delay_ms: bool,
}

impl Default for RegionParams {
    fn default() -> Self {
        RegionParams { quantize: 0, swing: 50, transpose: 0, velocity: 0, dynamics: 100, gate: 100, delay: 0.0, delay_ms: false }
    }
}

impl RegionParams {
    pub fn is_default(&self) -> bool {
        *self == RegionParams::default()
    }

    /// Whether what the region plays changes with the tempo map (so a tempo edit has to
    /// rebuild its playback list).
    pub fn follows_tempo(&self) -> bool {
        self.quantize != 0 || self.gate == GATE_FIXED || (self.delay != 0.0 && !self.delay_ms)
    }

    /// The same parameters with every value brought into range (and unknown quantize and
    /// dynamics values to the nearest that exists).
    pub fn clamped(mut self) -> Self {
        if self.quantize != 0 && !QUANTIZE_VALUES.contains(&self.quantize) {
            self.quantize = *QUANTIZE_VALUES.iter().min_by_key(|v| v.abs_diff(self.quantize)).unwrap();
        }
        self.swing = self.swing.min(100);
        self.transpose = self.transpose.clamp(TRANSPOSE_RANGE.0, TRANSPOSE_RANGE.1);
        self.velocity = self.velocity.clamp(VELOCITY_RANGE.0, VELOCITY_RANGE.1);
        if !DYNAMICS.iter().any(|d| d.0 == self.dynamics) {
            self.dynamics = DYNAMICS.iter().min_by_key(|d| d.0.abs_diff(self.dynamics)).unwrap().0;
        }
        if self.gate != GATE_FIXED && self.gate != GATE_LEGATO {
            self.gate = self.gate.clamp(10, 200);
        }
        let max = if self.delay_ms { DELAY_MS_MAX } else { DELAY_TICKS_MAX };
        self.delay = if self.delay.is_finite() { self.delay.clamp(-max, max) } else { 0.0 };
        self
    }

    /// Where an event at timeline sample `at` plays once delayed.
    pub fn delayed(&self, at: f64, map: &TempoMap) -> f64 {
        if self.delay == 0.0 {
            at
        } else if self.delay_ms {
            at + self.delay * map.sample_rate() / 1000.0
        } else {
            map.sample_at(map.beats_at(at) + self.delay / TICKS_PER_QUARTER)
        }
    }

    /// The delay in the other unit, measured at timeline sample `at` (for switching units
    /// without changing what plays there).
    pub fn delay_in(&self, ms: bool, at: f64, map: &TempoMap) -> f64 {
        if ms == self.delay_ms {
            return self.delay;
        }
        let shift = self.delayed(at, map) - at;
        if ms {
            shift * 1000.0 / map.sample_rate()
        } else {
            (map.beats_at(at + shift) - map.beats_at(at)) * TICKS_PER_QUARTER
        }
    }
}

/// Where a note starting at timeline sample `at` quantizes to: the nearest point of a grid of
/// `div` steps per whole note, counted from its bar line (so odd meters line up), with every
/// second point moved by the swing. A note nearer the next bar line goes there.
pub fn quantize_target(at: f64, div: u16, swing: u8, map: &TempoMap) -> f64 {
    if div == 0 {
        return at;
    }
    let grid = 4.0 / div as f64;
    let q = map.beats_at(at);
    let bar = map.bar_at(at);
    let (start, end) = (map.bar_start_q(bar), map.bar_start_q(bar + 1));
    let k = ((q - start) / grid).round();
    let mut target = start + k * grid;
    if k as i64 % 2 == 1 {
        target += grid * (swing.min(100) as f64 - 50.0) / 50.0;
    }
    let target = if target >= end || end - q < (q - target).abs() { end } else { target };
    map.sample_at(target)
}

/// A velocity after dynamics (around 64) and the velocity offset, within 1–127.
pub fn velocity(v: u8, p: &RegionParams) -> u8 {
    let scale = DYNAMICS.iter().find(|d| d.0 == p.dynamics).map(|d| d.1).unwrap_or(1.0);
    let shaped = 64.0 + (v as f64 - 64.0) * scale;
    (shaped.round() as i64 + p.velocity as i64).clamp(1, 127) as u8
}

/// The notes and controller events of a region at timeline sample `start` as they play with
/// `p`, still relative to `start` and in the order given. Times can end up before 0 or past the
/// region's end (a negative delay, a note quantized back over the region start); see
/// [`window`]. Muted notes are transformed too and stay muted.
pub fn apply(p: &RegionParams, notes: &[Note], cc: &[CcEvent], start: i64, map: &TempoMap) -> (Vec<Note>, Vec<CcEvent>) {
    let s0 = start as f64;
    // Quantize and gate on timeline positions: (on, off) per note.
    let mut timed: Vec<(f64, f64)> = notes
        .iter()
        .map(|n| {
            let at = s0 + n.start as f64;
            let on = if p.quantize != 0 { quantize_target(at, p.quantize, p.swing, map) } else { at };
            (on, on + n.length as f64)
        })
        .collect();
    match p.gate {
        100 => {}
        GATE_FIXED => {
            for t in &mut timed {
                t.1 = t.0 + map.samples_per_quarter_at(t.0) / 16.0;
            }
        }
        GATE_LEGATO => {
            // Up to the next note that starts later (the notes of a chord end together).
            let mut ons: Vec<f64> = timed.iter().map(|t| t.0).collect();
            ons.sort_by(f64::total_cmp);
            for t in &mut timed {
                if let Some(&next) = ons.get(ons.partition_point(|&o| o <= t.0)) {
                    t.1 = next;
                }
            }
        }
        pct => {
            for t in &mut timed {
                t.1 = t.0 + (t.1 - t.0) * pct as f64 / 100.0;
            }
        }
    }
    let out_notes = notes
        .iter()
        .zip(timed)
        .map(|(n, (on, off))| {
            let (on, off) = (p.delayed(on, map), p.delayed(off, map));
            let start = (on - s0).round() as i64;
            Note {
                start,
                length: ((off - s0).round() as i64 - start).max(1),
                pitch: (n.pitch as i16 + p.transpose).clamp(0, 127) as u8,
                velocity: velocity(n.velocity, p),
                ..*n
            }
        })
        .collect();
    let out_cc = cc.iter().map(|c| CcEvent { start: (p.delayed(s0 + c.start as f64, map) - s0).round() as i64, ..*c }).collect();
    (out_notes, out_cc)
}

/// How far the playback window of a region at `start` with `length` has to reach beyond the
/// region so its transformed events still play: `(lead, tail)` in samples. `lead` covers events
/// moved before the region start; `tail` is a positive delay at the region end, so the end
/// moves with the notes.
pub fn window(p: &RegionParams, notes: &[Note], cc: &[CcEvent], start: i64, length: i64, map: &TempoMap) -> (i64, i64) {
    let first = notes.iter().map(|n| n.start).chain(cc.iter().map(|c| c.start)).min().unwrap_or(0);
    let end = (start + length) as f64;
    let tail = (p.delayed(end, map) - end).round() as i64;
    ((-first).max(0), tail.max(0))
}

/// One pass of a MIDI region as the engine plays it: the window starts `lead` samples before
/// the pass and lasts `length`; event times are relative to the window start, sorted.
#[derive(Debug, Clone)]
pub struct Pass {
    pub lead: i64,
    pub length: i64,
    pub notes: Vec<Note>,
    pub cc: Vec<CcEvent>,
}

/// What a pass of `pass_len` samples through MIDI region `r` plays (None for audio): the notes
/// that start inside the pass and aren't muted, and the controllers before its end, with the
/// region's parameters applied.
pub fn pass(r: &Region, pass_len: i64, map: &TempoMap) -> Option<Pass> {
    let RegionContent::Midi { notes, cc } = &r.content else { return None };
    let notes: Vec<Note> = notes.iter().filter(|n| !n.muted && n.start >= 0 && n.start < pass_len).copied().collect();
    let cc: Vec<CcEvent> = cc.iter().filter(|c| c.start >= 0 && c.start < pass_len).copied().collect();
    let (mut notes, mut cc) = apply(&r.params, &notes, &cc, r.start, map);
    let (lead, tail) = window(&r.params, &notes, &cc, r.start, pass_len, map);
    for n in &mut notes {
        n.start += lead;
    }
    for c in &mut cc {
        c.start += lead;
    }
    notes.sort_by_key(|n| (n.start, n.pitch));
    cc.sort_by_key(|c| c.start);
    Some(Pass { lead, length: pass_len + lead + tail, notes, cc })
}

/// The region with its parameters written into it and reset (Apply Region Parameters).
///
/// A MIDI region gets the transformed notes, and grows to cover notes a negative delay or
/// quantize moved before its start and the end a positive delay moved later, so it plays
/// exactly as before. A looping region keeps its length (so the loop keeps its period) and its
/// notes are kept inside it instead. Notes outside the region, which didn't play, stay where
/// they are on the timeline and are left out if they would now play. An audio region moves by
/// its delay (losing whatever would fall before the project start).
pub fn bake(r: &Region, map: &TempoMap) -> Region {
    // A pattern region bakes as the notes its pattern plays.
    if r.pattern.is_some() {
        return bake(&crate::pattern::realize(r, map), map);
    }
    let mut out = Region { params: RegionParams::default(), ..r.clone() };
    if r.params.is_default() {
        return out;
    }
    match &mut out.content {
        RegionContent::Audio { offset, .. } => {
            let to = r.params.delayed(r.start as f64, map).round() as i64;
            // Trim the part before 0: nothing plays there.
            *offset -= to.min(0);
            out.length = (out.length + to.min(0)).max(1);
            out.move_to(to.max(0));
        }
        RegionContent::Midi { notes, cc } => {
            let inside = |start: i64| start >= 0 && start < r.length;
            let played: Vec<bool> = notes.iter().map(|n| inside(n.start)).collect();
            let (mut new_notes, mut new_cc) = apply(&r.params, notes, cc, r.start, map);
            let looped = r.is_looped();
            let (lead, tail) = if looped {
                (0, 0)
            } else {
                let playing: Vec<Note> = new_notes.iter().zip(&played).filter(|p| *p.1).map(|p| *p.0).collect();
                let early: Vec<CcEvent> = new_cc.iter().zip(cc.iter()).filter(|(_, c)| inside(c.start)).map(|p| *p.0).collect();
                window(&r.params, &playing, &early, r.start, r.length, map)
            };
            let length = r.length + lead + tail;
            let mut kept = Vec::with_capacity(new_notes.len());
            for ((n, old), was) in new_notes.iter_mut().zip(notes.iter()).zip(played) {
                if was {
                    n.start = if looped { n.start.clamp(0, r.length - 1) } else { n.start + lead };
                    kept.push(*n);
                } else {
                    // Didn't play: unchanged on the timeline, and still not playing.
                    let start = old.start + lead;
                    if start < 0 || start >= length {
                        kept.push(Note { start, ..*old });
                    }
                }
            }
            for c in &mut new_cc {
                c.start = if looped { c.start.max(0) } else { c.start + lead };
            }
            new_cc.sort_by_key(|c| c.start);
            kept.sort_by_key(|n| (n.start, n.pitch));
            *notes = kept;
            *cc = new_cc;
            out.start = r.start - lead;
            out.length = length;
        }
    }
    out
}

/// Where region `r` starts and stops sounding on the timeline, its parameters included (for
/// render ranges). Never before 0.
pub fn bounds(r: &Region, map: &TempoMap) -> (i64, i64) {
    if r.params.is_default() {
        return (r.start, r.end());
    }
    let b = bake(r, map);
    (b.start.max(0), b.end().max(r.end()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CcKind;
    use crate::tempo::TempoEvent;

    const SR: f64 = 48000.0;
    /// 120 BPM: 24000 samples per quarter.
    const Q: i64 = 24000;

    fn note(start: i64, length: i64, pitch: u8, velocity: u8) -> Note {
        Note { start, length, pitch, velocity, muted: false }
    }

    fn map120() -> TempoMap {
        TempoMap::constant(SR, 120.0, 4, 4)
    }

    fn params(f: impl FnOnce(&mut RegionParams)) -> RegionParams {
        let mut p = RegionParams::default();
        f(&mut p);
        p
    }

    fn starts(notes: &[Note]) -> Vec<i64> {
        notes.iter().map(|n| n.start).collect()
    }

    #[test]
    fn defaults_change_nothing() {
        let notes = vec![note(13, 500, 60, 90), note(7001, 20, 72, 3)];
        let cc = vec![CcEvent { start: 5, kind: CcKind::Controller(1), value: 40 }];
        let (n, c) = apply(&RegionParams::default(), &notes, &cc, 12345, &map120());
        assert_eq!(starts(&n), vec![13, 7001]);
        assert_eq!(n.iter().map(|n| (n.length, n.pitch, n.velocity)).collect::<Vec<_>>(), vec![(500, 60, 90), (20, 72, 3)]);
        assert_eq!(c[0].start, 5);
        assert!(RegionParams::default().is_default());
        assert!(!RegionParams::default().follows_tempo());
    }

    #[test]
    fn quantize_snaps_starts_and_keeps_lengths() {
        let map = map120();
        let p = params(|p| p.quantize = 16);
        let s = Q / 4;
        // The region starts mid-bar: the grid still counts from the bar line.
        let start = 3 * s + 100;
        let notes = vec![note(-100 + s + 1000, 777, 60, 100), note(-100 + 2 * s - 2000, 50, 62, 100)];
        let (n, _) = apply(&p, &notes, &[], start, &map);
        assert_eq!(starts(&n), vec![4 * s - start, 5 * s - start]);
        assert_eq!(n[0].length, 777);
        // A triplet grid: 1/12 note = a third of a quarter.
        let p = params(|p| p.quantize = 12);
        let (n, _) = apply(&p, &[note(Q / 3 + 500, 10, 60, 100)], &[], 0, &map);
        assert_eq!(n[0].start, Q / 3);
    }

    #[test]
    fn swing_moves_every_second_grid_point() {
        let map = map120();
        let e = Q / 2;
        // 1/8 grid, 66 % swing: the off-beat eighths move 32 % of an eighth later (66.7 % would
        // be a third, the triplet feel).
        let p = params(|p| {
            p.quantize = 8;
            p.swing = 66;
        });
        let notes = vec![note(10, 100, 60, 100), note(e - 30, 100, 60, 100), note(2 * e + 5, 100, 60, 100), note(3 * e + 40, 100, 60, 100)];
        let (n, _) = apply(&p, &notes, &[], 0, &map);
        let late = (e as f64 * 0.32).round() as i64;
        assert_eq!(starts(&n), vec![0, e + late, 2 * e, 3 * e + late]);
        // Below 50 pulls them earlier; swing does nothing without quantize.
        let p = params(|p| {
            p.quantize = 8;
            p.swing = 25;
        });
        let (n, _) = apply(&p, &notes, &[], 0, &map);
        assert_eq!(n[1].start, e / 2);
        let p = params(|p| p.swing = 80);
        let (n, _) = apply(&p, &notes, &[], 0, &map);
        assert_eq!(n[1].start, e - 30);
    }

    #[test]
    fn quantize_follows_the_tempo_map() {
        // 120 BPM for one bar, then 60 BPM: a quarter is 24000 samples, then 48000.
        let map = TempoMap::new(SR, &[TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: 4 * Q, bpm: 60.0, ramp: false }], &[], 120.0, (4, 4));
        let p = params(|p| p.quantize = 4);
        let bar2 = 4 * Q;
        let notes = vec![note(bar2 + 2 * Q + 1000, 100, 60, 100), note(bar2 + 3 * Q, 100, 60, 100)];
        let (n, _) = apply(&p, &notes, &[], 0, &map);
        // Beat 2 of bar 2 is at bar2 + 48000; the second note (at 1.5 slow beats) rounds up to
        // beat 3 (bar2 + 96000).
        assert_eq!(starts(&n), vec![bar2 + 2 * Q, bar2 + 4 * Q]);
    }

    #[test]
    fn transpose_clamps_to_the_midi_range() {
        let p = params(|p| p.transpose = 24);
        let (n, _) = apply(&p, &[note(0, 1, 60, 100), note(1, 1, 120, 100)], &[], 0, &map120());
        assert_eq!(n.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![84, 127]);
        let p = params(|p| p.transpose = -96);
        let (n, _) = apply(&p, &[note(0, 1, 60, 100)], &[], 0, &map120());
        assert_eq!(n[0].pitch, 0);
        assert_eq!(params(|p| p.transpose = 500).clamped().transpose, 96);
    }

    #[test]
    fn velocity_and_dynamics() {
        let off = |v: i16| params(|p| p.velocity = v);
        assert_eq!(velocity(100, &off(20)), 120);
        assert_eq!(velocity(100, &off(50)), 127);
        assert_eq!(velocity(30, &off(-99)), 1);
        // 2:1 halves the distance from 64; 0.5:1 doubles it; Fixed puts everything at 64.
        let dynamics = |d: u16, v: i16| params(|p| (p.dynamics, p.velocity) = (d, v));
        assert_eq!(velocity(100, &dynamics(200, 0)), 82);
        assert_eq!(velocity(40, &dynamics(200, 0)), 52);
        assert_eq!(velocity(80, &dynamics(50, 0)), 96);
        assert_eq!(velocity(10, &dynamics(50, 0)), 1);
        assert_eq!(velocity(127, &dynamics(0, 0)), 64);
        assert_eq!(velocity(1, &dynamics(0, 10)), 74);
        assert_eq!(velocity(100, &dynamics(300, 0)), 76);
        assert_eq!(velocity(70, &dynamics(33, 0)), 82);
        // Unknown values go to the nearest one there is.
        assert_eq!(dynamics(140, 0).clamped().dynamics, 150);
    }

    #[test]
    fn gate_time() {
        let map = map120();
        let notes = vec![note(0, 1000, 60, 100), note(0, 400, 64, 100), note(Q, 2000, 60, 100), note(3 * Q, 10, 60, 100)];
        let lengths = |p: RegionParams| apply(&p, &notes, &[], 0, &map).0.iter().map(|n| n.length).collect::<Vec<_>>();
        assert_eq!(lengths(params(|p| p.gate = 50)), vec![500, 200, 1000, 5]);
        assert_eq!(lengths(params(|p| p.gate = 200)), vec![2000, 800, 4000, 20]);
        assert_eq!(lengths(params(|p| p.gate = GATE_FIXED)), vec![Q / 16; 4]);
        // Legato: up to the next later start; the last note keeps its length.
        assert_eq!(lengths(params(|p| p.gate = GATE_LEGATO)), vec![Q, Q, 2 * Q, 10]);
        assert_eq!(params(|p| p.gate = 5).clamped().gate, 10);
        assert_eq!(params(|p| p.gate = GATE_LEGATO).clamped().gate, GATE_LEGATO);
    }

    #[test]
    fn delay_in_ticks_and_ms() {
        let map = map120();
        let notes = vec![note(1000, 500, 60, 100)];
        let cc = vec![CcEvent { start: 0, kind: CcKind::PitchBend, value: 9000 }];
        // 480 ticks = an eighth = 12000 samples at 120 BPM.
        let p = params(|p| p.delay = 480.0);
        let (n, c) = apply(&p, &notes, &cc, 0, &map);
        assert_eq!((n[0].start, n[0].length, c[0].start), (13000, 500, 12000));
        let p = params(|p| (p.delay, p.delay_ms) = (-10.0, true));
        let (n, c) = apply(&p, &notes, &cc, 0, &map);
        assert_eq!((n[0].start, c[0].start), (520, -480));
        assert_eq!(window(&p, &n, &c, 0, Q, &map), (480, 0));
        let p = params(|p| p.delay = 960.0);
        assert_eq!(window(&p, &[], &[], 0, Q, &map), (0, Q));
        assert!(p.follows_tempo());
        assert!(!params(|p| (p.delay, p.delay_ms) = (5.0, true)).follows_tempo());
    }

    #[test]
    fn delay_in_ticks_across_a_tempo_change() {
        // 120 BPM, then 60 BPM from sample 4Q: a quarter of delay is 24000 samples before the
        // change and 48000 after it; a note just before the change gets a mix of both.
        let map = TempoMap::new(SR, &[TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: 4 * Q, bpm: 60.0, ramp: false }], &[], 120.0, (4, 4));
        let p = params(|p| p.delay = 960.0);
        let notes = vec![note(0, 100, 60, 100), note(4 * Q - Q / 2, 100, 60, 100), note(5 * Q, 100, 60, 100)];
        let (n, _) = apply(&p, &notes, &[], 0, &map);
        // Half a quarter at 120 (12000) then half a quarter at 60 (24000).
        assert_eq!(starts(&n), vec![Q, 4 * Q + 24000, 5 * Q + 48000]);
        // Switching units keeps what plays at the position.
        assert!((p.delay_in(true, 0.0, &map) - 500.0).abs() < 1e-6);
        assert!((p.delay_in(true, (5 * Q) as f64, &map) - 1000.0).abs() < 1e-6);
        let ms = params(|p| (p.delay, p.delay_ms) = (1000.0, true));
        assert!((ms.delay_in(false, (5 * Q) as f64, &map) - 960.0).abs() < 1e-6);
    }

    #[test]
    fn everything_together_in_order() {
        // Quantize, then gate on the quantized start, then delay, then the note values.
        let map = map120();
        let p = params(|p| {
            p.quantize = 4;
            p.gate = 50;
            p.delay = 240.0;
            p.transpose = -12;
            p.velocity = 10;
            p.dynamics = 200;
        });
        let (n, _) = apply(&p, &[note(Q + 3000, 8000, 60, 100), note(Q / 2 - 10, 400, 70, 20)], &[], 0, &map);
        // In the order given.
        assert_eq!((n[0].start, n[0].length, n[0].pitch, n[0].velocity), (Q + Q / 4, 4000, 48, 92));
        assert_eq!((n[1].start, n[1].length, n[1].pitch, n[1].velocity), (Q / 4, 200, 58, 52));
    }

    fn midi_region(start: i64, length: i64, notes: Vec<Note>, p: RegionParams) -> Region {
        Region {
            id: 1,
            name: "M".into(),
            start,
            length,
            muted: false,
            color: None,
            content: RegionContent::Midi { notes, cc: vec![CcEvent { start: 0, kind: CcKind::Controller(7), value: 100 }] },
            loop_end: None,
            takes: None,
            params: p,
            pattern: None,
        }
    }

    /// Timeline (on, off, pitch, velocity) of what a region plays in its first pass.
    fn heard(r: &Region, map: &TempoMap) -> Vec<(i64, i64, u8, u8)> {
        let ps = match r.params.is_default() {
            true => {
                let RegionContent::Midi { notes, .. } = &r.content else { unreachable!() };
                let mut v: Vec<_> = notes.iter().filter(|n| !n.muted && n.start >= 0 && n.start < r.length).map(|n| (r.start + n.start, r.start + (n.start + n.length).min(r.length), n.pitch, n.velocity)).collect();
                v.sort();
                return v;
            }
            false => pass(r, r.length, map).unwrap(),
        };
        let ws = r.start - ps.lead;
        ps.notes.iter().filter(|n| n.start < ps.length).map(|n| (ws + n.start, ws + (n.start + n.length).min(ps.length), n.pitch, n.velocity)).collect()
    }

    #[test]
    fn pass_widens_the_window_for_moved_notes() {
        let map = map120();
        let p = params(|p| (p.delay, p.delay_ms) = (-20.0, true));
        let r = midi_region(Q, 2 * Q, vec![note(0, 100, 60, 100), note(Q, 100, 62, 100), note(2 * Q + 5, 100, 64, 100)], p);
        let ps = pass(&r, r.length, &map).unwrap();
        // 20 ms = 960 samples early; the note past the end is left out.
        assert_eq!((ps.lead, ps.length), (960, 2 * Q + 960));
        assert_eq!(starts(&ps.notes), vec![0, Q]);
        assert_eq!(ps.cc[0].start, 0);
        // A shorter (last loop) pass only plays the notes that start inside it.
        assert_eq!(pass(&r, Q, &map).unwrap().notes.len(), 1);
    }

    #[test]
    fn bake_plays_exactly_as_before() {
        let map = TempoMap::new(SR, &[TempoEvent { pos: 0, bpm: 120.0, ramp: false }, TempoEvent { pos: 2 * Q, bpm: 90.0, ramp: false }], &[], 120.0, (4, 4));
        let notes = vec![note(30, 5000, 60, 100), note(Q - 700, 9000, 64, 70), note(3 * Q + 11, 3000, 67, 40), note(-500, 900, 50, 50)];
        for p in [
            params(|p| (p.quantize, p.swing, p.delay) = (8, 60, -120.0)),
            params(|p| (p.transpose, p.velocity, p.dynamics, p.gate) = (5, -10, 150, 140)),
            params(|p| (p.delay, p.gate) = (300.0, GATE_LEGATO)),
            params(|p| (p.quantize, p.delay, p.delay_ms) = (12, 15.0, true)),
        ] {
            let r = midi_region(Q / 3, 4 * Q, notes.clone(), p);
            let b = bake(&r, &map);
            assert!(b.params.is_default());
            assert_eq!(heard(&r, &map), heard(&b, &map), "{p:?}");
        }
        // Looping regions keep their length; their notes stay inside.
        let mut r = midi_region(0, Q, vec![note(10, 100, 60, 100)], params(|p| (p.delay, p.delay_ms) = (-10.0, true)));
        r.loop_end = Some(4 * Q);
        let b = bake(&r, &map);
        assert_eq!((b.start, b.length, b.loop_end), (0, Q, Some(4 * Q)));
        let RegionContent::Midi { notes, .. } = &b.content else { unreachable!() };
        assert_eq!(notes[0].start, 0);
    }

    #[test]
    fn bake_moves_audio_by_its_delay() {
        let map = map120();
        let audio = |start: i64, delay: f64| Region {
            content: RegionContent::Audio { clip: 1, offset: 100, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() },
            ..midi_region(start, Q, Vec::new(), params(|p| (p.delay, p.delay_ms) = (delay, true)))
        };
        let b = bake(&audio(Q, 10.0), &map);
        assert_eq!((b.start, b.length), (Q + 480, Q));
        let b = bake(&audio(100, -10.0), &map);
        let RegionContent::Audio { offset, .. } = b.content else { unreachable!() };
        assert_eq!((b.start, b.length, offset), (0, Q - 380, 480));
    }

    #[test]
    fn serde_defaults() {
        let p: RegionParams = serde_json::from_str("{\"transpose\": 5}").unwrap();
        assert_eq!(p, params(|p| p.transpose = 5));
    }
}
