//! Take folders and comping (Logic's Quick Swipe Comping), and how a cycle or autopunch
//! recording is cut into takes. The types are in model.rs; this is the logic on them, kept
//! apart from the session so it can be tested on its own.
//!
//! Everything inside a folder is in *take time* (see [`TakeFolder`]). Playback expands the
//! comp into [`CompPiece`]s, one per stretch of a take, with short crossfades (Settings ›
//! Editing › Crossfade Time and Curve) where one take hands over to the next.
//!
//! A folder can keep several comps and switch between them; MIDI take folders hold whole
//! takes only (see [`TakeFolder`]).

use crate::model::{CcEvent, Comp, CompSegment, Id, Note, Region, RegionContent, Take, TakeFolder};

/// Marks the end of a cycle pass among the MIDI events recorded (the engine sends it when the
/// cycle jumps back while recording). An undefined system real-time byte, so it can never be
/// mistaken for something played.
pub const PASS_MARK: [u8; 3] = [0xF9, 0, 0];

/// Most takes one recording is cut into (a guard against a degenerate cycle).
const MAX_TAKES: i64 = 999;

impl Take {
    pub fn len(&self) -> i64 {
        (self.end - self.start).max(0)
    }
}

impl TakeFolder {
    /// An empty folder.
    pub fn new() -> TakeFolder {
        TakeFolder { takes: Vec::new(), comp: Vec::new(), comps: Vec::new(), current: 0, offset: 0 }
    }

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

    // ---- comps ------------------------------------------------------------------------------

    /// The comps by name, the current one at `current`. A folder that never had a second comp
    /// has one, "Comp 1".
    pub fn comp_names(&self) -> Vec<String> {
        if self.comps.is_empty() {
            return vec!["Comp 1".into()];
        }
        self.comps.iter().map(|c| c.name.clone()).collect()
    }

    /// The name of the comp being heard.
    pub fn comp_name(&self) -> String {
        let mut names = self.comp_names();
        let i = self.current.min(names.len() - 1);
        names.swap_remove(i)
    }

    /// Copies the comp being edited into its place in the list (after every edit).
    pub fn store_comp(&mut self) {
        let comp = self.comp.clone();
        if let Some(c) = self.comps.get_mut(self.current) {
            c.comp = comp;
        }
    }

    /// Turns the implicit single comp into a list entry, so others can be added.
    fn list_comps(&mut self) {
        if self.comps.is_empty() {
            self.comps.push(Comp { name: "Comp 1".into(), comp: self.comp.clone() });
            self.current = 0;
        }
    }

    /// "Comp N", for the lowest N above every comp named that way.
    fn next_comp_name(&self) -> String {
        let n = self.comps.iter().filter_map(|c| c.name.strip_prefix("Comp ")?.trim().parse::<usize>().ok()).max().unwrap_or(0);
        format!("Comp {}", n.max(self.comps.len()) + 1)
    }

    fn add_comp(&mut self, comp: Vec<CompSegment>) {
        self.list_comps();
        self.store_comp();
        let name = self.next_comp_name();
        self.comps.push(Comp { name, comp });
        self.current = self.comps.len() - 1;
        self.comp = self.comps[self.current].comp.clone();
        self.normalize();
        self.store_comp();
    }

    /// Create New Comp: a new comp, as a fresh recording starts out (the newest take wherever
    /// it has audio), becomes the one heard.
    pub fn create_comp(&mut self) {
        let keep = std::mem::take(&mut self.comp);
        self.default_comp();
        let fresh = std::mem::replace(&mut self.comp, keep);
        self.add_comp(fresh);
    }

    /// Duplicate Comp: a copy of the comp being heard, to change without losing it.
    pub fn duplicate_comp(&mut self) {
        let copy = self.comp.clone();
        self.add_comp(copy);
    }

    /// Makes comp `index` the one heard (and edited).
    pub fn switch_comp(&mut self, index: usize) {
        if index >= self.comps.len() || index == self.current {
            return;
        }
        self.store_comp();
        self.current = index;
        self.comp = self.comps[index].comp.clone();
        // Takes deleted since the comp was saved drop out of it.
        self.normalize();
        self.store_comp();
    }

    pub fn rename_comp(&mut self, index: usize, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() || index >= self.comp_names().len() {
            return;
        }
        self.list_comps();
        self.comps[index].name = name;
    }

    /// Delete Comp. The last comp can't go; deleting the one heard switches to its neighbour.
    pub fn delete_comp(&mut self, index: usize) {
        if self.comps.len() < 2 || index >= self.comps.len() {
            return;
        }
        self.store_comp();
        self.comps.remove(index);
        if index < self.current || self.current >= self.comps.len() {
            self.current = self.current.saturating_sub(1);
        }
        self.comp = self.comps[self.current].comp.clone();
        self.normalize();
        self.store_comp();
    }

    // ---- MIDI take folders ------------------------------------------------------------------

    /// The take a MIDI folder plays: the comp's (whole) take, or the newest.
    pub fn midi_take(&self) -> Option<Id> {
        self.comp.first().map(|s| s.take).filter(|&t| self.take(t).is_some()).or(self.takes.last().map(|t| t.id))
    }

    /// The notes and controller events of `take` heard over take time `offset..offset + len`,
    /// moved so `offset` becomes 0 (a MIDI region's content).
    pub fn midi_window(&self, take: Id, offset: i64, len: i64) -> (Vec<Note>, Vec<CcEvent>) {
        let Some(t) = self.take(take) else { return (Vec::new(), Vec::new()) };
        let inside = |p: i64| p >= offset && p < offset + len;
        let notes = t.notes.iter().filter(|n| inside(n.start)).map(|n| Note { start: n.start - offset, ..*n }).collect();
        let cc = t.cc.iter().filter(|c| inside(c.start)).map(|c| CcEvent { start: c.start - offset, ..*c }).collect();
        (notes, cc)
    }

    /// Writes a MIDI region's content back into `take` over take time `offset..offset + len`
    /// (what [`TakeFolder::midi_window`] gave, perhaps edited since). The take keeps what it
    /// has outside that window.
    pub fn store_midi(&mut self, take: Id, offset: i64, len: i64, notes: &[Note], cc: &[CcEvent]) {
        let Some(t) = self.take_mut(take) else { return };
        let inside = |p: i64| p >= offset && p < offset + len;
        t.notes.retain(|n| !inside(n.start));
        t.notes.extend(notes.iter().map(|n| Note { start: n.start + offset, ..*n }));
        t.notes.sort_by_key(|n| n.start);
        t.cc.retain(|c| !inside(c.start));
        t.cc.extend(cc.iter().map(|c| CcEvent { start: c.start + offset, ..*c }));
        t.cc.sort_by_key(|c| c.start);
    }

    /// Moves every take and comp by `delta` frames of take time, and a MIDI folder's offset
    /// with them, so it plays the same.
    pub fn shift(&mut self, delta: i64) {
        for t in &mut self.takes {
            t.start += delta;
            t.end += delta;
            t.notes.iter_mut().for_each(|n| n.start += delta);
            t.cc.iter_mut().for_each(|c| c.start += delta);
        }
        for s in self.comp.iter_mut().chain(self.comps.iter_mut().flat_map(|c| c.comp.iter_mut())) {
            s.start += delta;
            s.end += delta;
        }
        self.offset += delta;
    }
}

impl Default for TakeFolder {
    fn default() -> Self {
        TakeFolder::new()
    }
}

/// Writes a MIDI take folder's content back into the take it plays (see [`TakeFolder`]), before
/// anything reads the takes or changes which one plays.
pub fn save_midi_take(r: &mut Region) {
    let (Some(f), RegionContent::Midi { notes, cc }) = (r.takes.as_mut(), &r.content) else { return };
    if let Some(t) = f.midi_take() {
        let offset = f.offset;
        f.store_midi(t, offset, r.length, notes, cc);
    }
}

/// Makes a MIDI take folder's content the notes of the take it plays, inside the region.
pub fn load_midi_take(r: &mut Region) {
    let (Some(f), RegionContent::Midi { notes, cc }) = (r.takes.as_mut(), &mut r.content) else { return };
    // The comp names the take, even after the one it named was deleted.
    if let (None, Some(t)) = (f.comp.first(), f.midi_take()) {
        f.select_whole(t);
    }
    if let Some(t) = f.midi_take() {
        (*notes, *cc) = f.midi_window(t, f.offset, r.length);
    }
}

/// Trims a MIDI take folder `r` to `start..start + length` while its notes stay put: its take
/// time follows, and its content becomes the take's notes in the new span, so trimming never
/// loses any (unlike a plain MIDI region, whose content is all it has).
pub fn retrim_midi_folder(r: &mut Region, start: i64, length: i64) {
    if r.takes.is_none() || !matches!(r.content, RegionContent::Midi { .. }) {
        return;
    }
    save_midi_take(r);
    r.takes.as_mut().unwrap().offset += start - r.start;
    r.start = start;
    r.length = length;
    load_midi_take(r);
}

/// Splits recorded MIDI events at the [`PASS_MARK`]s into one list per cycle pass, dropping
/// passes in which nothing was played.
pub fn midi_passes(events: &[(i64, [u8; 3])]) -> Vec<Vec<(i64, [u8; 3])>> {
    let mut out = vec![Vec::new()];
    for &(pos, d) in events {
        if d == PASS_MARK {
            out.push(Vec::new());
        } else {
            out.last_mut().unwrap().push((pos, d));
        }
    }
    out.retain(|p| !p.is_empty());
    out
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

/// Where a take folder pass meets a neighbouring region it crossfades with: the folder plays
/// `lead` frames before the pass and `tail` frames after it, and the crossfade's timeline
/// (start, length) replaces the folder's own fade there.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PassEdges {
    pub lead: i64,
    pub tail: i64,
    pub fade_in: Option<(i64, i64)>,
    pub fade_out: Option<(i64, i64)>,
}

/// What a take folder plays over its pass at timeline `ps` (length `pl`; see
/// [`Region::passes`]). Where two stretches touch, each reaches `xfade / 2` past the boundary
/// (as far as its take has audio) and they crossfade over it. A stretch next to silence fades
/// in or out within itself. The folder's own fades shape its first and last stretch.
pub fn pass_pieces(r: &Region, f: &TakeFolder, ps: i64, pl: i64, xfade: i64) -> Vec<CompPiece> {
    pass_pieces_edged(r, f, ps, pl, xfade, PassEdges::default())
}

/// [`pass_pieces`] for a pass that crossfades with a neighbour at either end (`edges`): the
/// stretches heard at that end reach into the crossfade and fade with it.
pub fn pass_pieces_edged(r: &Region, f: &TakeFolder, ps: i64, pl: i64, xfade: i64, edges: PassEdges) -> Vec<CompPiece> {
    let RegionContent::Audio { offset: tt0, fade_in, fade_out, .. } = r.content else { return Vec::new() };
    let (lead, tail) = (edges.lead.max(0), edges.tail.max(0));
    let raw = raw_pieces(f, tt0 - lead, pl + lead + tail);
    let end = r.end();
    let first_pass = ps == r.start;
    let last_pass = ps + pl >= end;
    let last_len = r.passes().last().map(|p| p.1).unwrap_or(r.length);
    let fi = fade_in.clamp(0, r.length);
    let fo = if r.is_looped() { fade_out.clamp(0, last_len) } else { fade_out.clamp(0, r.length - fi) };
    let own_in = edges.fade_in.unwrap_or((r.start, fi));
    let own_out = edges.fade_out.unwrap_or((end - fo, fo));
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
            } else if a == tt0 - lead {
                if first_pass { own_in } else { (start, 0) }
            } else {
                (start, short)
            };
            let fade_out = if joined_out {
                (tl(b) - hb_out, hb_out + ext_out)
            } else if b == tt0 + pl + tail {
                if last_pass { own_out } else { (stop, 0) }
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
            takes: takes
                .iter()
                .map(|&(id, start, end)| Take { id, name: format!("Take {id}"), clip: id * 10, offset: 1000 * id as i64, start, end, gain_db: 0.0, notes: Vec::new(), cc: Vec::new() })
                .collect(),
            ..TakeFolder::new()
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

    #[test]
    fn comps_switch_and_keep_their_edits() {
        let mut f = folder(&[(1, 0, 1000), (2, 0, 1000)]);
        f.default_comp();
        assert_eq!((f.comp_names(), f.comp_name()), (vec!["Comp 1".to_string()], "Comp 1".to_string()));
        // Create New Comp starts from the newest take; edits stay with the comp.
        f.create_comp();
        assert_eq!((f.comp_names(), f.current), (vec!["Comp 1".to_string(), "Comp 2".to_string()], 1));
        f.select(1, 0, 500);
        f.store_comp();
        assert_eq!(f.comps[0].comp, vec![CompSegment { start: 0, end: 1000, take: 2 }]);
        f.switch_comp(0);
        assert_eq!(f.comp, vec![CompSegment { start: 0, end: 1000, take: 2 }]);
        f.switch_comp(1);
        assert_eq!(f.comp.len(), 2);
        // Duplicate copies the comp heard.
        f.duplicate_comp();
        assert_eq!((f.current, f.comp_name(), f.comp.len()), (2, "Comp 3".to_string(), 2));
        f.rename_comp(2, "  Lead  ".into());
        f.rename_comp(0, "".into());
        assert_eq!(f.comp_names(), vec!["Comp 1".to_string(), "Comp 2".to_string(), "Lead".to_string()]);
        // Deleting one before the current keeps the current one heard; the last stays.
        f.delete_comp(0);
        assert_eq!((f.current, f.comp_name()), (1, "Lead".to_string()));
        f.delete_comp(1);
        assert_eq!((f.current, f.comp_name(), f.comp.len()), (0, "Comp 2".to_string(), 2));
        f.delete_comp(0);
        assert_eq!(f.comps.len(), 1);
        // A deleted take drops out of every comp it is switched back to.
        f.create_comp();
        f.remove_take(2);
        f.switch_comp(0);
        assert!(f.comp.iter().all(|s| s.take == 1));
    }

    #[test]
    fn midi_takes_window_and_store() {
        let note = |start, pitch| Note { start, length: 10, pitch, velocity: 100, muted: false };
        let mut f = TakeFolder::new();
        f.takes.push(Take { id: 1, name: "Take 1".into(), clip: 0, offset: 0, start: 0, end: 1000, gain_db: 0.0, notes: vec![note(50, 60), note(500, 62)], cc: Vec::new() });
        f.takes.push(Take { id: 2, name: "Take 2".into(), clip: 0, offset: 0, start: 0, end: 1000, gain_db: 0.0, notes: vec![note(300, 72)], cc: Vec::new() });
        assert_eq!(f.midi_take(), Some(2), "the newest without a comp");
        f.select_whole(1);
        assert_eq!(f.midi_take(), Some(1));
        let key = |n: &[Note]| n.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>();
        let (n, _) = f.midi_window(1, 100, 900);
        assert_eq!(key(&n), vec![(400, 62)]);
        // An edit inside the window replaces it; what's outside stays.
        f.store_midi(1, 100, 900, &[note(10, 64)], &[]);
        assert_eq!(key(&f.take(1).unwrap().notes), vec![(50, 60), (110, 64)]);
        // A region over the folder plays the take it selects, and trimming loses nothing.
        let mut r = Region {
            id: 9,
            name: "M".into(),
            start: 2000,
            length: 1000,
            muted: false,
            color: None,
            content: RegionContent::Midi { notes: Vec::new(), cc: Vec::new() },
            loop_end: None,
            takes: Some(f),
            params: Default::default(),
            pattern: None,
        };
        load_midi_take(&mut r);
        retrim_midi_folder(&mut r, 2100, 900);
        assert!(matches!(&r.content, RegionContent::Midi { notes, .. } if key(notes) == vec![(10, 64)]));
        retrim_midi_folder(&mut r, 2000, 1000);
        assert!(matches!(&r.content, RegionContent::Midi { notes, .. } if notes.len() == 2));
        assert_eq!(r.takes.as_ref().unwrap().offset, 0);
    }

    #[test]
    fn recorded_midi_splits_at_pass_marks() {
        let on = |p| (p, [0x90, 60, 100]);
        let events = vec![on(10), (0, PASS_MARK), (0, PASS_MARK), on(20), (0, PASS_MARK), on(30)];
        assert_eq!(midi_passes(&events), vec![vec![on(10)], vec![on(20)], vec![on(30)]], "an empty pass is dropped");
        assert_eq!(midi_passes(&[]).len(), 0);
    }

    #[test]
    fn edged_pieces_reach_into_a_neighbour_crossfade() {
        let mut f = folder(&[(1, 0, 2000)]);
        f.select(1, 0, 2000);
        let r = region(5000, 1000, 500, f);
        let edges = PassEdges { lead: 100, tail: 50, fade_in: Some((4900, 200)), fade_out: Some((5950, 100)) };
        let p = pass_pieces_edged(&r, r.takes.as_ref().unwrap(), 5000, 1000, 100, edges);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].start, p[0].end, p[0].offset), (4900, 6050, 1400));
        assert_eq!((p[0].fade_in, p[0].fade_out), ((4900, 200), (5950, 100)));
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
