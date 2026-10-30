//! Take folders and comping (Logic's Quick Swipe Comping), and how a cycle or autopunch
//! recording is cut into takes. The types are in model.rs; this is the logic on them, kept
//! apart from the session so it can be tested on its own.
//!
//! Everything inside a folder is in *take time* (see [`TakeFolder`]). Playback expands the
//! comp into [`CompPiece`]s, one per stretch of a take, with short equal-power crossfades where
//! one take hands over to the next.

use crate::model::{CompSegment, Id, Region, RegionContent, Take, TakeFolder};

/// Length of the crossfade at a comp boundary, centred on it.
pub const COMP_XFADE_MS: f64 = 10.0;

/// Most takes one recording is cut into (a guard against a degenerate cycle).
const MAX_TAKES: i64 = 999;

impl Take {
    pub fn len(&self) -> i64 {
        (self.end - self.start).max(0)
    }
}

impl TakeFolder {
    pub fn take(&self, id: Id) -> Option<&Take> {
        self.takes.iter().find(|t| t.id == id)
    }

    pub fn take_mut(&mut self, id: Id) -> Option<&mut Take> {
        self.takes.iter_mut().find(|t| t.id == id)
    }

    /// Hands take time `start..end` to `take` (clipped to where the take has audio), as a
    /// swipe across a take row does. Whatever the comp had there is replaced.
    pub fn select(&mut self, take: Id, start: i64, end: i64) {
        let Some(t) = self.take(take) else { return };
        let (a, b) = (start.max(t.start), end.min(t.end));
        if a >= b {
            return;
        }
        self.clear(a, b);
        self.comp.push(CompSegment { start: a, end: b, take });
        self.normalize();
    }

    /// Makes `take` the only one heard, over all of its audio.
    pub fn select_whole(&mut self, take: Id) {
        if let Some(t) = self.take(take) {
            self.comp = vec![CompSegment { start: t.start, end: t.end, take }];
        }
    }

    /// Silences take time `start..end` in the comp.
    pub fn clear(&mut self, start: i64, end: i64) {
        let mut out = Vec::with_capacity(self.comp.len() + 1);
        for s in self.comp.drain(..) {
            if s.end <= start || s.start >= end {
                out.push(s);
                continue;
            }
            if s.start < start {
                out.push(CompSegment { end: start, ..s });
            }
            if s.end > end {
                out.push(CompSegment { start: end, ..s });
            }
        }
        self.comp = out;
    }

    /// The comp a fresh recording gets, as in Logic: the newest take wherever it has audio,
    /// older takes filling in where it has none (a last pass stopped part-way through).
    pub fn default_comp(&mut self) {
        self.comp.clear();
        let ids: Vec<Id> = self.takes.iter().map(|t| t.id).collect();
        for id in ids {
            let (s, e) = self.take(id).map(|t| (t.start, t.end)).unwrap_or((0, 0));
            self.select(id, s, e);
        }
    }

    /// The take heard throughout when the comp is one whole take (what the take menu ticks).
    pub fn whole_take(&self) -> Option<Id> {
        match self.comp.as_slice() {
            [s] => self.take(s.take).filter(|t| t.start == s.start && t.end == s.end).map(|t| t.id),
            _ => None,
        }
    }

    /// Takes the comp uses somewhere.
    pub fn used(&self) -> Vec<Id> {
        self.takes.iter().filter(|t| self.comp.iter().any(|s| s.take == t.id)).map(|t| t.id).collect()
    }

    pub fn remove_take(&mut self, id: Id) {
        self.takes.retain(|t| t.id != id);
        self.normalize();
    }

    /// Delete Unused Takes. Keeps everything when the comp is empty, rather than leaving an
    /// empty folder.
    pub fn remove_unused(&mut self) {
        let used = self.used();
        if !used.is_empty() {
            self.takes.retain(|t| used.contains(&t.id));
        }
    }

    /// Sorts the comp, drops empty or orphaned segments and merges touching ones of one take.
    pub fn normalize(&mut self) {
        let takes: Vec<(Id, i64, i64)> = self.takes.iter().map(|t| (t.id, t.start, t.end)).collect();
        let mut segs: Vec<CompSegment> = self
            .comp
            .iter()
            .filter_map(|s| {
                let &(_, ts, te) = takes.iter().find(|t| t.0 == s.take)?;
                let (a, b) = (s.start.max(ts), s.end.min(te));
                (a < b).then_some(CompSegment { start: a, end: b, take: s.take })
            })
            .collect();
        segs.sort_by_key(|s| s.start);
        let mut out: Vec<CompSegment> = Vec::with_capacity(segs.len());
        for s in segs {
            match out.last_mut() {
                Some(l) if l.take == s.take && l.end == s.start => l.end = s.end,
                // Overlaps can only come from a hand-edited file; the earlier one wins.
                Some(l) if s.start < l.end => {
                    let end = l.end;
                    if s.end > end {
                        out.push(CompSegment { start: end, ..s });
                    }
                }
                _ => out.push(s),
            }
        }
        self.comp = out;
    }
}

/// A stretch of one take as a folder plays it: timeline `start..end`, from clip frame
/// `offset`, with fades as `RegionDsp::Audio` takes them (timeline start, length).
#[derive(Debug, Clone, PartialEq)]
pub struct CompPiece {
    pub take: Id,
    pub clip: Id,
    pub gain_db: f32,
    pub start: i64,
    pub end: i64,
    pub offset: i64,
    pub fade_in: (i64, i64),
    pub fade_out: (i64, i64),
}

/// The comp's stretches within one pass of the folder: take time `tt0..tt0 + len`, where
/// `tt0` is the folder's offset. Unclipped to the takes' audio means a stretch is silent.
fn raw_pieces(f: &TakeFolder, tt0: i64, len: i64) -> Vec<(&Take, i64, i64)> {
    let mut out = Vec::new();
    for s in &f.comp {
        let Some(t) = f.take(s.take) else { continue };
        let (a, b) = (s.start.max(t.start).max(tt0), s.end.min(t.end).min(tt0 + len));
        if a < b {
            out.push((t, a, b));
        }
    }
    out
}

/// What a take folder plays over its pass at timeline `ps` (length `pl`; see
/// [`Region::passes`]). Where two stretches touch, each reaches `xfade / 2` past the boundary
/// (as far as its take has audio) and they crossfade over it. A stretch next to silence fades
/// in or out within itself. The folder's own fades shape its first and last stretch.
pub fn pass_pieces(r: &Region, f: &TakeFolder, ps: i64, pl: i64, xfade: i64) -> Vec<CompPiece> {
    let RegionContent::Audio { offset: tt0, fade_in, fade_out, .. } = r.content else { return Vec::new() };
    let raw = raw_pieces(f, tt0, pl);
    let end = r.end();
    let first_pass = ps == r.start;
    let last_pass = ps + pl >= end;
    let last_len = r.passes().last().map(|p| p.1).unwrap_or(r.length);
    let fi = fade_in.clamp(0, r.length);
    let fo = if r.is_looped() { fade_out.clamp(0, last_len) } else { fade_out.clamp(0, r.length - fi) };
    let h = (xfade / 2).max(0);
    let tl = |t: i64| ps + (t - tt0);
    // Half-width of the crossfade at the boundary in front of piece i (0 if none).
    let boundary = |i: usize| -> i64 {
        if i == 0 || i >= raw.len() || raw[i - 1].2 != raw[i].1 {
            return 0;
        }
        h.min((raw[i - 1].2 - raw[i - 1].1) / 2).min((raw[i].2 - raw[i].1) / 2)
    };
    raw.iter()
        .enumerate()
        .map(|(i, &(t, a, b))| {
            let short = h.min((b - a) / 2);
            let (hb_in, hb_out) = (boundary(i), boundary(i + 1));
            let joined_in = i > 0 && raw[i - 1].2 == a;
            let joined_out = i + 1 < raw.len() && raw[i + 1].1 == b;
            // How far past each boundary this take's audio reaches.
            let ext_in = if joined_in { hb_in.min(a - t.start) } else { 0 };
            let ext_out = if joined_out { hb_out.min(t.end - b) } else { 0 };
            let (start, stop) = (tl(a) - ext_in, tl(b) + ext_out);
            let fade_in = if joined_in {
                (start, hb_in + ext_in)
            } else if a == tt0 {
                if first_pass { (r.start, fi) } else { (start, 0) }
            } else {
                (start, short)
            };
            let fade_out = if joined_out {
                (tl(b) - hb_out, hb_out + ext_out)
            } else if b == tt0 + pl {
                if last_pass { (end - fo, fo) } else { (stop, 0) }
            } else {
                (stop - short, short)
            };
            CompPiece { take: t.id, clip: t.clip, gain_db: t.gain_db, start, end: stop, offset: t.offset + a - ext_in, fade_in, fade_out }
        })
        .collect()
}

/// The comp of a folder's first pass as plain stretches (no crossfade overlap): timeline
/// `(start, length)`, the take, and the clip frame at `start`. What Flatten turns into regions.
pub fn flat_pieces(r: &Region, f: &TakeFolder) -> Vec<(i64, i64, Id, i64)> {
    let RegionContent::Audio { offset: tt0, .. } = r.content else { return Vec::new() };
    raw_pieces(f, tt0, r.length).into_iter().map(|(t, a, b)| (r.start + a - tt0, b - a, t.id, t.offset + a)).collect()
}

/// How a recording was set up, so it can be cut into takes once it stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecPlan {
    /// Where the transport started.
    pub from: i64,
    /// The cycle, when recording in cycle mode.
    pub cycle: Option<(i64, i64)>,
    /// The Autopunch range.
    pub punch: Option<(i64, i64)>,
    /// Recording latency compensation (frames recordings move earlier).
    pub latency: i64,
    /// A pass stopped part-way through is kept as a take if it got at least this far.
    pub min_take: i64,
}

impl RecPlan {
    /// For cycle recording: the take-time window `w0..w1` (the cycle, narrowed by the punch
    /// range), and where each pass stops capturing (`g1`, the punch-out plus the latency, so
    /// the compensated take still reaches the punch-out). None when the window is empty.
    pub fn cycle_window(&self) -> Option<(i64, i64, i64)> {
        let (cs, ce) = self.cycle?;
        let (w0, w1) = match self.punch {
            Some((a, b)) => (a.max(cs), b.min(ce)),
            None => (cs, ce),
        };
        if w1 <= w0 {
            return None;
        }
        let g1 = if self.punch.is_some() { (w1 + self.latency).min(ce) } else { ce };
        Some((w0, w1, g1))
    }

    /// Engine positions between which input is captured: from the record start (`playhead`
    /// outside cycle mode) to the end, per pass in cycle mode. None captures nothing.
    pub fn capture(&self, playhead: i64) -> Option<(i64, i64)> {
        if self.cycle.is_some() {
            let (w0, _, g1) = self.cycle_window()?;
            return Some((w0, if self.punch.is_some() { g1 } else { i64::MAX }));
        }
        match self.punch {
            Some((a, b)) => {
                let start = playhead.max(a);
                (start < b).then_some((start, b + self.latency))
            }
            None => Some((playhead, i64::MAX)),
        }
    }

    /// The takes of a cycle recording `frames` long: the folder's timeline start and length,
    /// then per take its clip offset and the take-time range it has audio for.
    ///
    /// Without a punch range the file is one continuous stream, the passes back to back. With
    /// one, each pass only captured `w0..g1`, so the passes are `g1 - w0` frames apart. Either
    /// way the latency puts the audio heard at `w0` `latency` frames into its pass.
    pub fn cycle_takes(&self, frames: i64) -> Option<(i64, i64, Vec<(i64, i64, i64)>)> {
        let (cs, ce) = self.cycle?;
        let (w0, w1, g1) = self.cycle_window()?;
        let stride = g1 - w0;
        let continuous = w0 == cs && g1 == ce;
        let len = if continuous { w1 - w0 } else { (w1 - w0).min(stride - self.latency) };
        if len <= 0 {
            return None;
        }
        // Recording that starts inside the window begins part-way into the first pass.
        let first = if self.from > w0 && self.from < g1 { self.from } else { w0 };
        let base = self.latency + (w0 - first);
        let mut takes = Vec::new();
        for k in 0..MAX_TAKES {
            let offset = base + k * stride;
            if offset >= frames {
                break;
            }
            let (start, end) = ((-offset).max(0), len.min(frames - offset));
            if end - start >= self.min_take.min(len).max(1) {
                takes.push((offset, start, end));
            }
        }
        Some((w0, len, takes))
    }

    /// Outside cycle mode, with Autopunch on: where the take lands, trimmed to the punch range:
    /// timeline start, length and clip offset. `start` is where capture began.
    pub fn punch_span(&self, start: i64, frames: i64) -> Option<(i64, i64, i64)> {
        let (pin, pout) = self.punch?;
        let heard = start - self.latency;
        let a = heard.max(pin).max(0);
        let b = (heard + frames).min(pout);
        (a < b).then_some((a, b - a, a - heard))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(takes: &[(Id, i64, i64)]) -> TakeFolder {
        TakeFolder {
            takes: takes.iter().map(|&(id, start, end)| Take { id, name: format!("Take {id}"), clip: id * 10, offset: 1000 * id as i64, start, end, gain_db: 0.0 }).collect(),
            comp: Vec::new(),
        }
    }

    fn region(start: i64, length: i64, offset: i64, f: TakeFolder) -> Region {
        Region {
            id: 99,
            name: "F".into(),
            start,
            length,
            muted: false,
            color: None,
            content: RegionContent::Audio { clip: 10, offset, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() },
            loop_end: None,
            takes: Some(f),
            params: Default::default(),
            pattern: None,
        }
    }

    #[test]
    fn swipes_replace_and_merge() {
        let mut f = folder(&[(1, 0, 1000), (2, 0, 1000), (3, 0, 600)]);
        f.default_comp();
        // Newest wherever it has audio, the one before it after that.
        assert_eq!(f.comp, vec![CompSegment { start: 0, end: 600, take: 3 }, CompSegment { start: 600, end: 1000, take: 2 }]);
        f.select(1, 200, 400);
        assert_eq!(f.comp.len(), 4);
        f.select(1, 400, 800);
        let spans: Vec<(i64, i64, Id)> = f.comp.iter().map(|s| (s.start, s.end, s.take)).collect();
        assert_eq!(spans, vec![(0, 200, 3), (200, 800, 1), (800, 1000, 2)]);
        // Clipped to the take's audio.
        f.select(3, 500, 900);
        assert_eq!(f.comp.iter().find(|s| s.take == 3 && s.start == 500).map(|s| s.end), Some(600));
        assert_eq!(f.whole_take(), None);
        f.select_whole(2);
        assert_eq!(f.whole_take(), Some(2));
        f.remove_unused();
        assert_eq!(f.takes.len(), 1);
        f.remove_take(2);
        assert!(f.comp.is_empty());
    }

    #[test]
    fn pieces_crossfade_at_boundaries() {
        let mut f = folder(&[(1, 0, 1000), (2, 0, 1000)]);
        f.select(1, 0, 500);
        f.select(2, 500, 1000);
        let r = region(5000, 1000, 0, f);
        let p = pass_pieces(&r, r.takes.as_ref().unwrap(), 5000, 1000, 100);
        assert_eq!(p.len(), 2);
        // Take 1 runs 50 frames past the boundary at 5500 and fades out across it; take 2
        // starts 50 early and fades in over the same 100 frames.
        assert_eq!((p[0].start, p[0].end, p[0].offset), (5000, 5550, 1000));
        assert_eq!(p[0].fade_out, (5450, 100));
        assert_eq!((p[1].start, p[1].end, p[1].offset), (5450, 6000, 2450));
        assert_eq!(p[1].fade_in, (5450, 100));
        // Trimming the folder's front (offset 200) shifts the take-time window.
        let mut r2 = r.clone();
        r2.start = 5200;
        r2.length = 800;
        r2.content = RegionContent::Audio { clip: 10, offset: 200, gain_db: 0.0, fade_in: 0, fade_out: 0, fade_shape: Default::default() };
        let p = pass_pieces(&r2, r2.takes.as_ref().unwrap(), 5200, 800, 100);
        assert_eq!((p[0].start, p[0].offset), (5200, 1200));
        assert_eq!(flat_pieces(&r2, r2.takes.as_ref().unwrap()), vec![(5200, 300, 1, 1200), (5500, 500, 2, 2500)]);
    }

    #[test]
    fn pieces_next_to_silence_fade_inside() {
        let mut f = folder(&[(1, 0, 1000)]);
        f.select(1, 300, 600);
        let r = region(0, 1000, 0, f);
        let p = pass_pieces(&r, r.takes.as_ref().unwrap(), 0, 1000, 100);
        assert_eq!((p[0].start, p[0].end, p[0].fade_in, p[0].fade_out), (300, 600, (300, 50), (550, 50)));
    }

    fn plan(cycle: Option<(i64, i64)>, punch: Option<(i64, i64)>, latency: i64) -> RecPlan {
        RecPlan { from: 0, cycle, punch, latency, min_take: 10 }
    }

    #[test]
    fn cycle_recordings_split_into_passes() {
        // A 1000-frame cycle at 2000, 30 frames of latency, three and a half passes recorded.
        let p = plan(Some((2000, 3000)), None, 30);
        assert_eq!(p.capture(2500), Some((2000, i64::MAX)));
        let (start, len, takes) = p.cycle_takes(3530).unwrap();
        assert_eq!((start, len), (2000, 1000));
        assert_eq!(takes, vec![(30, 0, 1000), (1030, 0, 1000), (2030, 0, 1000), (3030, 0, 500)]);
        // Stopped just after a pass boundary: the scrap is dropped.
        assert_eq!(p.cycle_takes(2035).unwrap().2.len(), 2);
    }

    #[test]
    fn punch_inside_the_cycle_gates_each_pass() {
        let p = plan(Some((2000, 3000)), Some((2200, 2600)), 30);
        assert_eq!(p.cycle_window(), Some((2200, 2600, 2630)));
        assert_eq!(p.capture(0), Some((2200, 2630)));
        let (start, len, takes) = p.cycle_takes(3 * 430).unwrap();
        assert_eq!((start, len), (2200, 400));
        assert_eq!(takes, vec![(30, 0, 400), (460, 0, 400), (890, 0, 400)]);
    }

    #[test]
    fn punch_without_cycle_trims_to_the_range() {
        let p = plan(None, Some((1000, 2000)), 30);
        assert_eq!(p.capture(400), Some((1000, 2030)));
        assert_eq!(p.capture(2500), None);
        // Captured from 1000 for 1030 frames: heard from 970, trimmed to 1000..2000.
        assert_eq!(p.punch_span(1000, 1030), Some((1000, 1000, 30)));
        // Recording started inside the range: placed as heard, like any recording.
        assert_eq!(p.punch_span(1500, 530), Some((1470, 530, 0)));
    }
}
