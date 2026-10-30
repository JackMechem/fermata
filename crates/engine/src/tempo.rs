//! The tempo map and time-signature changes: Logic's Tempo and Signature global tracks.
//!
//! Timeline positions stay in samples, so audio and MIDI regions keep their place in absolute
//! time when the tempo changes. The map converts between samples and musical time, counted in
//! quarter notes ("quarters"): `quarters(t) = ∫ bpm(t) / 60 dt`. A tempo event either holds
//! until the next one (a step) or ramps linearly, in time, to the next one's tempo.
//!
//! Signature changes start on bar lines and are given by bar number, so the bar grid is laid
//! out in quarters, independently of the tempo. Beats count in the signature's denominator
//! (an eighth in 7/8), as in Logic's LCD.
//!
//! `src/tempo.ts` mirrors this module for the UI; keep the two in step.

use serde::{Deserialize, Serialize};

use crate::model::Project;

/// One point of the Tempo track.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoEvent {
    /// Timeline position in samples.
    pub pos: i64,
    pub bpm: f64,
    /// Ramps linearly (in time) to the next event's tempo instead of holding until it.
    #[serde(default)]
    pub ramp: bool,
}

/// One time-signature change of the Signature track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigEvent {
    /// 1-based bar the signature starts at.
    pub bar: u32,
    pub num: u32,
    pub den: u32,
}

pub const MIN_BPM: f64 = 5.0;
pub const MAX_BPM: f64 = 990.0;

/// Slack for float noise when a position sits on a bar or grid line (in bars or steps).
const EPS: f64 = 1e-9;

pub fn clamp_bpm(bpm: f64) -> f64 {
    if bpm.is_finite() {
        bpm.clamp(MIN_BPM, MAX_BPM)
    } else {
        120.0
    }
}

/// A signature Fermata can show: 1–32 beats of a whole, half, quarter, eighth or sixteenth.
pub fn clamp_sig(num: u32, den: u32) -> (u32, u32) {
    (num.clamp(1, 32), if [1, 2, 4, 8, 16].contains(&den) { den } else { 4 })
}

/// Tempo events sorted by position, one per position (the later one wins), the first at 0.
/// Without an event at 0 the list starts with `base` there.
pub fn normalize_tempo(events: &[TempoEvent], base: f64) -> Vec<TempoEvent> {
    let mut v: Vec<TempoEvent> = events.iter().map(|e| TempoEvent { pos: e.pos.max(0), bpm: clamp_bpm(e.bpm), ramp: e.ramp }).collect();
    v.sort_by_key(|e| e.pos);
    let mut out: Vec<TempoEvent> = Vec::with_capacity(v.len() + 1);
    for e in v {
        match out.last_mut() {
            Some(last) if last.pos == e.pos => *last = e,
            _ => out.push(e),
        }
    }
    if out.first().map(|e| e.pos != 0).unwrap_or(true) {
        out.insert(0, TempoEvent { pos: 0, bpm: clamp_bpm(base), ramp: false });
    }
    if let Some(last) = out.last_mut() {
        // Nothing to ramp to.
        last.ramp = false;
    }
    out
}

/// Signature changes sorted by bar, one per bar (the later one wins), the first at bar 1.
/// Without one at bar 1 the list starts with `base` there.
pub fn normalize_sigs(events: &[SigEvent], base: (u32, u32)) -> Vec<SigEvent> {
    let mut v: Vec<SigEvent> = events
        .iter()
        .map(|e| {
            let (num, den) = clamp_sig(e.num, e.den);
            SigEvent { bar: e.bar.max(1), num, den }
        })
        .collect();
    v.sort_by_key(|e| e.bar);
    let mut out: Vec<SigEvent> = Vec::with_capacity(v.len() + 1);
    for e in v {
        match out.last_mut() {
            Some(last) if last.bar == e.bar => *last = e,
            _ => out.push(e),
        }
    }
    if out.first().map(|e| e.bar != 1).unwrap_or(true) {
        let (num, den) = clamp_sig(base.0, base.1);
        out.insert(0, SigEvent { bar: 1, num, den });
    }
    out
}

/// Tempo events for changes given in quarters, as (quarter, BPM) in order from quarter 0 (a
/// MIDI file's tempo track), at whole-sample positions.
pub fn events_from_quarters(sample_rate: f64, changes: &[(f64, f64)]) -> Vec<TempoEvent> {
    let mut out = Vec::with_capacity(changes.len());
    let mut pos = 0.0;
    let mut last: Option<(f64, f64)> = None;
    for &(q, bpm) in changes {
        let bpm = clamp_bpm(bpm);
        if let Some((lq, lb)) = last {
            pos += (q - lq).max(0.0) * 60.0 * sample_rate / lb;
        }
        out.push(TempoEvent { pos: pos.round() as i64, bpm, ramp: false });
        last = Some((q, bpm));
    }
    out
}

/// Signature changes given in quarters, as (quarter, numerator, denominator), numbered as bars
/// from quarter 0. A change off a bar line starts the bar it falls in. Before the first
/// change, `base` applies.
pub fn sigs_from_quarters(changes: &[(f64, u32, u32)], base: (u32, u32)) -> Vec<SigEvent> {
    let mut out = Vec::with_capacity(changes.len() + 1);
    let (mut bar, mut at, mut cur) = (1u32, 0.0, clamp_sig(base.0, base.1));
    for &(q, num, den) in changes {
        let bar_q = cur.0 as f64 * 4.0 / cur.1 as f64;
        let n = ((q - at) / bar_q + 1e-6).floor().max(0.0);
        bar += n as u32;
        at += n * bar_q;
        cur = clamp_sig(num, den);
        out.push(SigEvent { bar, num: cur.0, den: cur.1 });
    }
    out
}

/// Bar, beat, division (a quarter of a beat) and tick (1/240 of a division), all 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bbt {
    pub bar: i64,
    pub beat: u32,
    pub div: u32,
    pub tick: u32,
}

/// A point of the beat grid: a bar line, a beat or a subdivision of one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridPoint {
    /// Timeline position in samples (not rounded).
    pub pos: f64,
    /// Position in quarters.
    pub q: f64,
    /// 1-based bar.
    pub bar: i64,
    /// Index within the bar, in steps of `1 / div` beat; 0 is the bar line.
    pub sub: u32,
    /// Steps per beat.
    pub div: u32,
}

impl GridPoint {
    pub fn downbeat(&self) -> bool {
        self.sub == 0
    }

    pub fn on_beat(&self) -> bool {
        self.sub % self.div.max(1) == 0
    }
}

/// One bar of the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarSpan {
    /// 1-based bar number.
    pub bar: i64,
    pub start: f64,
    pub end: f64,
    pub start_q: f64,
    pub num: u32,
    pub den: u32,
}

impl BarSpan {
    /// Length of a beat in quarters.
    pub fn beat_q(&self) -> f64 {
        4.0 / self.den as f64
    }
}

/// A stretch between two tempo events.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Seg {
    pos: f64,
    /// Quarters elapsed at `pos`.
    q: f64,
    bpm: f64,
    /// BPM gained per sample (0 for a step).
    slope: f64,
}

/// A run of bars in one signature.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Meter {
    /// 0-based index of its first bar.
    bar: i64,
    /// Quarters at its first bar line.
    q: f64,
    num: u32,
    den: u32,
}

impl Meter {
    fn bar_q(&self) -> f64 {
        self.num as f64 * 4.0 / self.den as f64
    }

    fn beat_q(&self) -> f64 {
        4.0 / self.den as f64
    }
}

/// The project's tempo and signature changes, ready for conversions. Built off the audio
/// thread; the engine gets it as an `Arc` and only reads it (every lookup is a binary search
/// plus a closed form, without allocating).
#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    sr: f64,
    segs: Vec<Seg>,
    meters: Vec<Meter>,
    events: Vec<TempoEvent>,
    sigs: Vec<SigEvent>,
}

impl TempoMap {
    /// A map from tempo and signature changes. `base_bpm` and `base_sig` apply from the
    /// start when the lists don't say otherwise (an empty list means a constant tempo).
    pub fn new(sample_rate: f64, tempo: &[TempoEvent], sigs: &[SigEvent], base_bpm: f64, base_sig: (u32, u32)) -> TempoMap {
        let sr = sample_rate.max(1.0);
        let events = normalize_tempo(tempo, base_bpm);
        let mut segs = Vec::with_capacity(events.len());
        let mut q = 0.0;
        for (i, e) in events.iter().enumerate() {
            let next = events.get(i + 1);
            let slope = match next {
                Some(n) if e.ramp => (n.bpm - e.bpm) / (n.pos - e.pos) as f64,
                _ => 0.0,
            };
            segs.push(Seg { pos: e.pos as f64, q, bpm: e.bpm, slope });
            if let Some(n) = next {
                let dt = (n.pos - e.pos) as f64;
                q += (e.bpm * dt + 0.5 * slope * dt * dt) / (60.0 * sr);
            }
        }
        let sigs = normalize_sigs(sigs, base_sig);
        let mut meters: Vec<Meter> = Vec::with_capacity(sigs.len());
        for s in &sigs {
            let bar = s.bar as i64 - 1;
            let q = match meters.last() {
                Some(m) => m.q + (bar - m.bar) as f64 * m.bar_q(),
                None => 0.0,
            };
            meters.push(Meter { bar, q, num: s.num, den: s.den });
        }
        TempoMap { sr, segs, meters, events, sigs }
    }

    /// One tempo and one signature throughout.
    pub fn constant(sample_rate: f64, bpm: f64, num: u32, den: u32) -> TempoMap {
        TempoMap::new(sample_rate, &[], &[], bpm, (num, den))
    }

    pub fn from_project(p: &Project) -> TempoMap {
        TempoMap::new(p.sample_rate as f64, &p.tempo_map, &p.signatures, p.tempo, (p.sig_num, p.sig_den))
    }

    pub fn sample_rate(&self) -> f64 {
        self.sr
    }

    /// The tempo events, normalized: sorted, the first at 0.
    pub fn events(&self) -> &[TempoEvent] {
        &self.events
    }

    /// The signature changes, normalized: sorted, the first at bar 1.
    pub fn signatures(&self) -> &[SigEvent] {
        &self.sigs
    }

    /// One tempo and one signature throughout.
    pub fn is_constant(&self) -> bool {
        self.segs.len() == 1 && self.meters.len() == 1
    }

    fn seg_at_sample(&self, s: f64) -> &Seg {
        let i = self.segs.partition_point(|g| g.pos <= s);
        &self.segs[i.saturating_sub(1)]
    }

    fn seg_at_q(&self, q: f64) -> &Seg {
        let i = self.segs.partition_point(|g| g.q <= q);
        &self.segs[i.saturating_sub(1)]
    }

    fn meter_at_q(&self, q: f64) -> &Meter {
        let i = self.meters.partition_point(|m| m.q <= q + EPS);
        &self.meters[i.saturating_sub(1)]
    }

    fn meter_at_bar(&self, bar0: i64) -> &Meter {
        let i = self.meters.partition_point(|m| m.bar <= bar0);
        &self.meters[i.saturating_sub(1)]
    }

    /// Quarters elapsed at timeline sample `s`. Before 0 the first tempo carries on.
    pub fn beats_at(&self, s: f64) -> f64 {
        let g = self.seg_at_sample(s);
        let dt = s - g.pos;
        if dt <= 0.0 {
            return g.q + g.bpm * dt / (60.0 * self.sr);
        }
        g.q + (g.bpm * dt + 0.5 * g.slope * dt * dt) / (60.0 * self.sr)
    }

    /// The timeline sample (not rounded) where `q` quarters have elapsed.
    pub fn sample_at(&self, q: f64) -> f64 {
        let g = self.seg_at_q(q);
        // Samples × BPM needed to cover the quarters.
        let c = (q - g.q) * 60.0 * self.sr;
        if c <= 0.0 || g.slope == 0.0 {
            return g.pos + c / g.bpm;
        }
        // Solve bpm·dt + slope·dt²/2 = c for dt, in the form that stays accurate as the
        // slope goes to 0. Within the segment the discriminant is the end tempo squared.
        let d = (g.bpm * g.bpm + 2.0 * g.slope * c).max(0.0);
        g.pos + 2.0 * c / (g.bpm + d.sqrt())
    }

    /// The tempo at timeline sample `s`.
    pub fn bpm_at(&self, s: f64) -> f64 {
        let g = self.seg_at_sample(s);
        g.bpm + g.slope * (s - g.pos).max(0.0)
    }

    /// Samples per quarter note at `s` (at the tempo right there).
    pub fn samples_per_quarter_at(&self, s: f64) -> f64 {
        60.0 * self.sr / self.bpm_at(s)
    }

    /// Samples per beat (of the signature's denominator) at `s`.
    pub fn samples_per_beat_at(&self, s: f64) -> f64 {
        let (_, den) = self.sig_at(s);
        self.samples_per_quarter_at(s) * 4.0 / den as f64
    }

    /// Samples per bar at the tempo and signature in effect at `s`, as if the tempo held.
    pub fn samples_per_bar_at(&self, s: f64) -> f64 {
        let (num, den) = self.sig_at(s);
        self.samples_per_quarter_at(s) * num as f64 * 4.0 / den as f64
    }

    /// The signature of 1-based `bar`.
    pub fn sig_at_bar(&self, bar: i64) -> (u32, u32) {
        let m = self.meter_at_bar(bar - 1);
        (m.num, m.den)
    }

    /// The signature in effect at timeline sample `s`.
    pub fn sig_at(&self, s: f64) -> (u32, u32) {
        let m = self.meter_at_q(self.beats_at(s));
        (m.num, m.den)
    }

    /// The 0-based bar holding `q` quarters, with its start in quarters and its meter.
    fn bar_of_q(&self, q: f64) -> (i64, f64, &Meter) {
        let m = self.meter_at_q(q);
        let n = ((q - m.q) / m.bar_q() + EPS).floor();
        (m.bar + n as i64, m.q + n * m.bar_q(), m)
    }

    /// Quarters at the start of 1-based `bar`.
    pub fn bar_start_q(&self, bar: i64) -> f64 {
        let m = self.meter_at_bar(bar - 1);
        m.q + (bar - 1 - m.bar) as f64 * m.bar_q()
    }

    /// Timeline sample (not rounded) where 1-based `bar` starts.
    pub fn bar_start(&self, bar: i64) -> f64 {
        self.sample_at(self.bar_start_q(bar))
    }

    /// The 1-based bar playing at `s`.
    pub fn bar_at(&self, s: f64) -> i64 {
        self.bar_of_q(self.beats_at(s)).0 + 1
    }

    /// Bars elapsed at `s`, with the fraction of the current bar (0 at bar 1's start).
    pub fn bar_position(&self, s: f64) -> f64 {
        let (bar0, start, m) = self.bar_of_q(self.beats_at(s));
        bar0 as f64 + ((self.beats_at(s) - start) / m.bar_q()).max(0.0)
    }

    /// Bar, beat, division and tick at `s`, counting signature changes.
    pub fn bar_beat_at(&self, s: f64) -> Bbt {
        let q = self.beats_at(s);
        let (bar0, start, m) = self.bar_of_q(q);
        let beats = ((q - start) / m.beat_q()).max(0.0);
        let beat = beats.floor().min(m.num as f64 - 1.0);
        let frac = (beats - beat).clamp(0.0, 1.0 - 1e-12);
        let div = (frac * 4.0).floor();
        let tick = ((frac * 4.0 - div) * 240.0).floor();
        Bbt { bar: bar0 + 1, beat: beat as u32 + 1, div: div as u32 + 1, tick: tick as u32 + 1 }
    }

    fn grid_point(&self, bar0: i64, sub: u32, div: u32) -> GridPoint {
        let m = self.meter_at_bar(bar0);
        let start = m.q + (bar0 - m.bar) as f64 * m.bar_q();
        let q = start + sub as f64 * m.beat_q() / div as f64;
        GridPoint { pos: self.sample_at(q), q, bar: bar0 + 1, sub, div }
    }

    /// The first grid point (bar lines, beats and `div` steps per beat) at or after `q`.
    pub fn grid_at_or_after(&self, q: f64, div: u32) -> GridPoint {
        let div = div.max(1);
        let (bar0, start, m) = self.bar_of_q(q);
        let step = m.beat_q() / div as f64;
        let j = ((q - start) / step - EPS).ceil().max(0.0) as u32;
        if j >= m.num * div {
            self.grid_point(bar0 + 1, 0, div)
        } else {
            self.grid_point(bar0, j, div)
        }
    }

    /// The grid point after `g` (same `div`).
    pub fn grid_after(&self, g: &GridPoint) -> GridPoint {
        let bar0 = g.bar - 1;
        let (num, _) = self.sig_at_bar(g.bar);
        if g.sub + 1 >= num * g.div {
            self.grid_point(bar0 + 1, 0, g.div)
        } else {
            self.grid_point(bar0, g.sub + 1, g.div)
        }
    }

    /// `q` snapped to the nearest multiple of `step` quarters counted from its bar line, or to
    /// the next bar line when that is nearer (so odd meters line up, as in Logic).
    pub fn snap_q(&self, q: f64, step: f64) -> f64 {
        let (_, start, m) = self.bar_of_q(q);
        let end = start + m.bar_q();
        if !(step > 0.0) {
            return q;
        }
        let at = (start + ((q - start) / step).round() * step).min(end);
        if end - q < (q - at).abs() {
            end
        } else {
            at
        }
    }

    /// The first beat (or bar line) strictly after `s`.
    pub fn next_beat(&self, s: f64) -> f64 {
        let g = self.grid_at_or_after(self.beats_at(s), 1);
        if g.pos > s + 1e-6 {
            g.pos
        } else {
            self.grid_after(&g).pos
        }
    }

    /// The last beat (or bar line) strictly before `s`.
    pub fn prev_beat(&self, s: f64) -> f64 {
        let q = self.beats_at(s);
        let (bar0, start, m) = self.bar_of_q(q);
        let j = ((q - start) / m.beat_q() - EPS).ceil() as i64 - 1;
        if j >= 0 {
            self.grid_point(bar0, j as u32, 1).pos
        } else {
            let (num, _) = self.sig_at_bar(bar0);
            self.grid_point(bar0 - 1, num - 1, 1).pos
        }
    }

    /// The first bar line strictly after `s`.
    pub fn next_bar(&self, s: f64) -> f64 {
        self.bar_start(self.bar_at(s) + 1)
    }

    /// The last bar line strictly before `s`.
    pub fn prev_bar(&self, s: f64) -> f64 {
        let bar = self.bar_at(s);
        let start = self.bar_start(bar);
        if start < s - 1e-6 {
            start
        } else {
            self.bar_start(bar - 1)
        }
    }

    /// The bars that overlap `[from, to)`.
    pub fn bars(&self, from: f64, to: f64) -> impl Iterator<Item = BarSpan> + '_ {
        let mut bar = self.bar_at(from);
        std::iter::from_fn(move || {
            let start_q = self.bar_start_q(bar);
            let start = self.sample_at(start_q);
            if start >= to {
                return None;
            }
            let (num, den) = self.sig_at_bar(bar);
            let end = self.bar_start(bar + 1);
            let span = BarSpan { bar, start, end, start_q, num, den };
            bar += 1;
            Some(span)
        })
    }

    /// The grid points (`div` per beat) in `[from, to)`.
    pub fn grid(&self, from: f64, to: f64, div: u32) -> impl Iterator<Item = GridPoint> + '_ {
        let mut g = self.grid_at_or_after(self.beats_at(from), div);
        if g.pos < from {
            g = self.grid_after(&g);
        }
        std::iter::from_fn(move || {
            if g.pos >= to {
                return None;
            }
            let out = g;
            g = self.grid_after(&g);
            Some(out)
        })
    }

    /// Where a position at `s` in this map lands in `other` to keep its bar and beat.
    pub fn remap(&self, other: &TempoMap, s: f64) -> f64 {
        other.sample_at(self.beats_at(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn ev(pos: i64, bpm: f64, ramp: bool) -> TempoEvent {
        TempoEvent { pos, bpm, ramp }
    }

    fn sig(bar: u32, num: u32, den: u32) -> SigEvent {
        SigEvent { bar, num, den }
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn empty_map_is_the_old_constant_timing() {
        for (bpm, num, den) in [(120.0, 4, 4), (97.0, 7, 8), (133.3, 3, 4), (60.0, 6, 8)] {
            let m = TempoMap::constant(44100.0, bpm, num, den);
            let spb = 44100.0 * 60.0 / bpm * (4.0 / den as f64);
            let spbar = spb * num as f64;
            assert!(m.is_constant());
            for k in [0i64, 1, 7, 100, 12345] {
                assert!(close(m.bar_start(k + 1), k as f64 * spbar, 1e-6), "{bpm} bar {k}");
            }
            for pos in [0.0, 1.0, 22050.0, 123456.0, 9_999_999.0] {
                // The old toBbt.
                let beats = pos / spb;
                let bar = (beats / num as f64).floor();
                let in_bar = beats - bar * num as f64;
                let beat = in_bar.floor();
                let frac = in_bar - beat;
                let div = (frac * 4.0).floor();
                let tick = ((frac * 4.0 - div) * 240.0).floor();
                let want = Bbt { bar: bar as i64 + 1, beat: beat as u32 + 1, div: div as u32 + 1, tick: tick as u32 + 1 };
                assert_eq!(m.bar_beat_at(pos), want, "{bpm} at {pos}");
                assert!(close(m.beats_at(pos), pos / (44100.0 * 60.0 / bpm), 1e-9));
                assert_eq!(m.bpm_at(pos), bpm);
                assert_eq!(m.sig_at(pos), (num, den));
            }
            // The old click grid: k * spb / div, rounded.
            let div = 2;
            let mut g = m.grid_at_or_after(0.0, div);
            for k in 0..64i64 {
                assert_eq!(g.pos.round() as i64, (k as f64 * spb / div as f64).round() as i64);
                assert_eq!(g.downbeat(), k % (num as i64 * div as i64) == 0);
                g = m.grid_after(&g);
            }
        }
    }

    #[test]
    fn steps_round_trip() {
        // 120 BPM for 2 s (4 quarters), then 60 BPM.
        let m = TempoMap::new(SR, &[ev(0, 120.0, false), ev(96000, 60.0, false)], &[], 120.0, (4, 4));
        assert!(close(m.beats_at(96000.0), 4.0, 1e-12));
        assert!(close(m.beats_at(144000.0), 5.0, 1e-12));
        assert!(close(m.sample_at(5.0), 144000.0, 1e-9));
        assert_eq!(m.bpm_at(95999.0), 120.0);
        assert_eq!(m.bpm_at(96000.0), 60.0);
        for s in [0.0, 1.0, 50000.0, 95999.5, 96000.0, 96001.0, 1e7] {
            assert!(close(m.sample_at(m.beats_at(s)), s, 1e-6), "{s}");
        }
        // Bar 2 starts at the tempo change; bar 3 four seconds later.
        assert!(close(m.bar_start(2), 96000.0, 1e-9));
        assert!(close(m.bar_start(3), 96000.0 + 4.0 * SR, 1e-9));
        assert_eq!(m.bar_beat_at(96000.0 + 2.0 * SR), Bbt { bar: 2, beat: 3, div: 1, tick: 1 });
    }

    #[test]
    fn ramps_integrate_exactly() {
        // 60 → 180 BPM over 4 s: the mean is 120 BPM, so 8 quarters.
        let m = TempoMap::new(SR, &[ev(0, 60.0, true), ev(4 * 48000, 180.0, false)], &[], 60.0, (4, 4));
        assert!(close(m.beats_at(4.0 * SR), 8.0, 1e-9));
        assert!(close(m.bpm_at(2.0 * SR), 120.0, 1e-9));
        // First second: from 60 to 90 BPM, mean 75: 1.25 quarters.
        assert!(close(m.beats_at(SR), 1.25, 1e-9));
        // After the ramp, 180 BPM holds: 3 quarters a second.
        assert!(close(m.beats_at(5.0 * SR), 11.0, 1e-9));
        for q in [0.0, 0.001, 1.25, 3.3, 7.999, 8.0, 8.5, 40.0] {
            assert!(close(m.beats_at(m.sample_at(q)), q, 1e-9), "{q}");
        }
        assert!(close(m.sample_at(1.25), SR, 1e-6));
        // A slowing ramp too.
        let d = TempoMap::new(SR, &[ev(0, 200.0, true), ev(96000, 50.0, false)], &[], 200.0, (4, 4));
        assert!(close(d.beats_at(96000.0), 2.0 * 125.0 / 60.0, 1e-9));
        for s in [0.0, 1000.0, 48000.0, 95999.0, 96000.0, 200000.0] {
            assert!(close(d.sample_at(d.beats_at(s)), s, 1e-6), "{s}");
        }
        // Positions keep increasing through the ramp.
        let mut last = -1.0;
        for k in 0..200 {
            let s = d.sample_at(k as f64 * 0.05);
            assert!(s > last);
            last = s;
        }
    }

    #[test]
    fn nearly_flat_ramp_is_stable() {
        let m = TempoMap::new(SR, &[ev(0, 120.0, true), ev(10_000_000, 120.000001, false)], &[], 120.0, (4, 4));
        for q in [0.5, 10.0, 400.0] {
            // Within a tenth of a sample of 120 BPM (the drift is a few hundredths), and the
            // inverse holds.
            assert!(close(m.sample_at(q), q * 24000.0, 0.1), "{q}");
            assert!(close(m.beats_at(m.sample_at(q)), q, 1e-9), "{q}");
        }
    }

    #[test]
    fn signature_changes_number_bars() {
        // 4/4 for bars 1–2, 7/8 for bars 3–4, 3/4 from bar 5. 120 BPM: 24000 samples a quarter.
        let m = TempoMap::new(SR, &[], &[sig(1, 4, 4), sig(3, 7, 8), sig(5, 3, 4)], 120.0, (4, 4));
        let q = 24000.0;
        let starts = [0.0, 4.0, 8.0, 11.5, 15.0, 18.0, 21.0];
        for (i, s) in starts.iter().enumerate() {
            assert!(close(m.bar_start(i as i64 + 1), s * q, 1e-6), "bar {}", i + 1);
            assert_eq!(m.bar_at(s * q), i as i64 + 1);
            assert_eq!(m.bar_beat_at(s * q), Bbt { bar: i as i64 + 1, beat: 1, div: 1, tick: 1 });
        }
        assert_eq!(m.sig_at_bar(2), (4, 4));
        assert_eq!(m.sig_at_bar(3), (7, 8));
        assert_eq!(m.sig_at_bar(4), (7, 8));
        assert_eq!(m.sig_at_bar(5), (3, 4));
        assert_eq!(m.sig_at_bar(99), (3, 4));
        assert_eq!(m.sig_at(9.0 * q), (7, 8));
        // In 7/8, beats are eighths: half a quarter past bar 3 is beat 2.
        assert_eq!(m.bar_beat_at(8.5 * q), Bbt { bar: 3, beat: 2, div: 1, tick: 1 });
        assert_eq!(m.bar_beat_at(11.0 * q), Bbt { bar: 3, beat: 7, div: 1, tick: 1 });
        // The last sixteenth of bar 4 is in beat 7 of 7/8.
        assert_eq!(m.bar_beat_at(14.875 * q).beat, 7);
        // Bar lines and beats in a range: 7 beats in each 7/8 bar.
        let bars: Vec<_> = m.bars(8.0 * q, 15.0 * q).map(|b| (b.bar, b.num, b.den)).collect();
        assert_eq!(bars, vec![(3, 7, 8), (4, 7, 8)]);
        let beats: Vec<_> = m.grid(8.0 * q, 15.0 * q, 1).collect();
        assert_eq!(beats.len(), 14);
        assert!(beats[0].downbeat() && beats[7].downbeat() && !beats[6].downbeat());
        assert!(close(beats[7].pos, 11.5 * q, 1e-6));
        // Stepping
        assert!(close(m.next_bar(8.0 * q), 11.5 * q, 1e-6));
        assert!(close(m.prev_bar(11.5 * q), 8.0 * q, 1e-6));
        assert!(close(m.prev_bar(11.6 * q), 11.5 * q, 1e-6));
        assert!(close(m.next_beat(11.0 * q), 11.5 * q, 1e-6));
        assert!(close(m.prev_beat(11.5 * q), 11.0 * q, 1e-6));
        assert!(close(m.prev_beat(8.0 * q), 7.0 * q, 1e-6));
        assert!(close(m.bar_position(13.25 * q), 3.0 + 1.75 / 3.5, 1e-9));
        // Snapping counts from the bar line: in 7/8 the quarter grid of bar 3 is 8, 9, 10
        // and 11, then bar 4 starts at 11.5.
        assert!(close(m.snap_q(11.3, 1.0), 11.5, 1e-9));
        assert!(close(m.snap_q(11.2, 1.0), 11.0, 1e-9));
        assert!(close(m.snap_q(12.4, 1.0), 12.5, 1e-9));
        assert!(close(m.snap_q(8.4, 0.25), 8.5, 1e-9));
    }

    #[test]
    fn signatures_and_tempo_changes_together() {
        // 7/8 from bar 2, and the tempo halves at bar 2's line (4 quarters = 2 s at 120).
        let m = TempoMap::new(SR, &[ev(0, 120.0, false), ev(96000, 60.0, false)], &[sig(2, 7, 8)], 120.0, (4, 4));
        // Bar 2 is 3.5 quarters at 60 BPM: 3.5 s.
        assert!(close(m.bar_start(3), 96000.0 + 3.5 * SR, 1e-6));
        assert_eq!(m.bar_beat_at(96000.0 + 0.5 * SR), Bbt { bar: 2, beat: 2, div: 1, tick: 1 });
        assert!(close(m.samples_per_beat_at(100000.0), 24000.0, 1e-9));
        assert!(close(m.samples_per_bar_at(100000.0), 7.0 * 24000.0, 1e-9));
        // The click grid across both changes: 4 quarters in bar 1, then eighths every 0.5 s.
        let pts: Vec<_> = m.grid(0.0, 96000.0 + 3.5 * SR, 1).map(|g| g.pos.round() as i64).collect();
        let mut want: Vec<i64> = (0..4).map(|k| k * 24000).collect();
        want.extend((0..7).map(|k| 96000 + k * 24000));
        assert_eq!(pts, want);
    }

    #[test]
    fn normalizing_sorts_dedupes_and_anchors() {
        let n = normalize_tempo(&[ev(500, 90.0, true), ev(100, 1000.0, false), ev(500, 80.0, true)], 110.0);
        assert_eq!(n, vec![ev(0, 110.0, false), ev(100, 990.0, false), ev(500, 80.0, false)]);
        let s = normalize_sigs(&[sig(4, 5, 3), sig(0, 3, 4)], (4, 4));
        assert_eq!(s, vec![sig(1, 3, 4), sig(4, 5, 4)]);
        assert_eq!(normalize_sigs(&[], (6, 8)), vec![sig(1, 6, 8)]);
    }

    #[test]
    fn negative_positions_extend_the_first_tempo() {
        let m = TempoMap::new(SR, &[ev(0, 60.0, true), ev(48000, 120.0, false)], &[], 60.0, (3, 4));
        assert!(close(m.beats_at(-48000.0), -1.0, 1e-12));
        assert!(close(m.sample_at(-3.0), -3.0 * SR, 1e-9));
        assert_eq!(m.bar_at(-1.0), 0);
        assert!(close(m.bar_start(0), -3.0 * SR, 1e-9));
    }

    #[test]
    fn midi_file_changes_convert() {
        // 120 BPM for 4 quarters, then 60.
        let ev = events_from_quarters(SR, &[(0.0, 120.0), (4.0, 60.0)]);
        assert_eq!(ev, vec![ev_(0, 120.0), ev_(96000, 60.0)]);
        // 4/4 for two bars, then 7/8 for one bar, then 3/4.
        let s = sigs_from_quarters(&[(0.0, 4, 4), (8.0, 7, 8), (11.5, 3, 4)], (4, 4));
        assert_eq!(s, vec![sig(1, 4, 4), sig(3, 7, 8), sig(4, 3, 4)]);
    }

    fn ev_(pos: i64, bpm: f64) -> TempoEvent {
        ev(pos, bpm, false)
    }

    #[test]
    fn remap_keeps_bars_and_beats() {
        let a = TempoMap::constant(SR, 120.0, 4, 4);
        let b = TempoMap::new(SR, &[ev(0, 120.0, false), ev(96000, 60.0, false)], &[], 120.0, (4, 4));
        // Bar 3 at 120 throughout is 4 s; with the tempo halved after bar 2 it is 6 s.
        assert!(close(a.remap(&b, 4.0 * SR), 6.0 * SR, 1e-6));
        assert!(close(a.remap(&b, 1.0 * SR), 1.0 * SR, 1e-6));
    }
}
