// The No Overlap and Shuffle drag modes, mirroring crates/engine/src/drag_modes.rs (where the
// rules are written out), so the Tracks area can draw where a drag will leave the regions.
// The engine applies the edit; this is only the preview. Pure, so tests can import it.
import type { Id } from "./api";
import type { DragMode } from "./fades";

/** A region on a lane: where it starts and how long it sounds, loop included. */
export interface Span {
  id: Id;
  start: number;
  len: number;
}

export type Side = "left" | "right";

/** The Shuffle side of a drag mode, or null for the others. */
export function shuffleSide(mode: DragMode): Side | null {
  return mode === "shuffle_l" ? "left" : mode === "shuffle_r" ? "right" : null;
}

const end = (s: Span) => s.start + s.len;

/** No Overlap: the regions an edit of `edited` leaves overlapping, with the lengths they are
 *  cut to (0: covered from the start, removed). `lane` is in the track's order. */
export function noOverlap(lane: Span[], edited: Id[]): Map<Id, number> {
  const out = new Map<Id, number>();
  lane.forEach((a, i) => {
    let e = end(a);
    lane.forEach((b, j) => {
      if (i === j || !(edited.includes(a.id) || edited.includes(b.id))) return;
      const later = b.start > a.start || (b.start === a.start && j > i);
      if (later && b.start < e) e = b.start;
    });
    if (e < end(a)) out.set(a.id, e - a.start);
  });
  return out;
}

/** Shuffle: every region's start after `leaving` leave the lane (as it was before the edit)
 *  and `arriving` are dropped onto it at their own starts. */
export function shuffle(lane: Span[], leaving: Id[], arriving: Span[], side: Side): Map<Id, number> {
  return side === "left" ? starts(shuffleLeft(lane, leaving, arriving)) : unmirror(shuffleLeft(mirror(lane), leaving, mirror(arriving)));
}

/** Shuffle after region `id` changes length to `len`: Shuffle L keeps its start, Shuffle R its end. */
export function shuffleResize(lane: Span[], id: Id, len: number, side: Side): Map<Id, number> {
  return side === "left" ? starts(resizeLeft(lane, id, len)) : unmirror(resizeLeft(mirror(lane), id, len));
}

const starts = (lane: Span[]) => new Map(lane.map((s) => [s.id, s.start]));
const mirror = (lane: Span[]) => lane.map((s) => ({ ...s, start: -end(s) }));

function unmirror(lane: Span[]): Map<Id, number> {
  const real = lane.map((s) => [s.id, -end(s)] as const);
  const short = -Math.min(0, ...real.map(([, s]) => s));
  return new Map(real.map(([id, s]) => [id, s + short]));
}

function close(stay: Span[], gone: Span[]): Span[] {
  const out = stay.map((s) => ({ ...s }));
  for (const g of [...gone].sort((a, b) => b.start - a.start)) {
    for (const r of out) if (r.start > g.start) r.start = Math.max(g.start, r.start - g.len);
  }
  return out;
}

function shuffleLeft(lane: Span[], leaving: Id[], arriving: Span[]): Span[] {
  const moving = (id: Id) => leaving.includes(id) || arriving.some((a) => a.id === id);
  const stay = lane.filter((s) => !moving(s.id)).sort((a, b) => a.start - b.start);
  const out = close(
    stay,
    lane.filter((s) => leaving.includes(s.id)),
  );
  if (!arriving.length) return out;
  const p = Math.min(...arriving.map((a) => a.start));
  const len = Math.max(...arriving.map(end)) - p;
  let at: number;
  let first: number;
  let under = -1;
  for (let i = 0; i < stay.length; i++) if (stay[i].start <= p && p < end(stay[i])) under = i;
  let before = -1;
  for (let i = 0; i < stay.length; i++) if (stay[i].start < p) before = i;
  if (under >= 0 && 2 * (p - stay[under].start) < stay[under].len) {
    at = out[under].start;
    first = under;
  } else if (under >= 0 || before >= 0) {
    const i = under >= 0 ? under : before;
    at = Math.max(...out.slice(0, i + 1).map(end));
    first = i + 1;
  } else {
    at = out.length ? Math.min(out[0].start, p) : p;
    first = 0;
  }
  if (first < out.length) {
    const push = Math.max(0, at + len - out[first].start);
    for (const r of out.slice(first)) r.start += push;
  }
  return [...out, ...arriving.map((a) => ({ ...a, start: at + (a.start - p) }))];
}

function resizeLeft(lane: Span[], id: Id, len: number): Span[] {
  const me = lane.find((s) => s.id === id);
  if (!me) return lane;
  const d = len - me.len;
  return lane.map((s) => (s.id === id ? { ...s, len } : s.start > me.start ? { ...s, start: Math.max(me.start, s.start + d) } : s));
}

/** What a drag in progress will do to a region in a No Overlap or Shuffle mode: a new start,
 *  and the length it will sound for (0: it goes). */
export interface Outcome {
  start?: number;
  len?: number;
}

/** A drag in progress: regions moved to a track and start, or one region trimmed to a new
 *  start and sounding length. */
export type DragEdit = { kind: "move"; moved: { id: Id; track: Id; start: number }[] } | { kind: "resize"; id: Id; start: number; len: number };

/** Where a drag will leave the regions, for drawing it: `lanes` holds each track's regions as
 *  spans, in the track's order, before the drag. Empty in the Overlap and X-Fade modes. */
export function dragOutcome(mode: DragMode, lanes: Map<Id, Span[]>, edit: DragEdit): Map<Id, Outcome> {
  const out = new Map<Id, Outcome>();
  const side = shuffleSide(mode);
  if (!side && mode !== "no_overlap") return out;
  const spanOf = (id: Id) => [...lanes.values()].flat().find((s) => s.id === id);
  if (edit.kind === "resize") {
    const before = [...lanes.values()].find((l) => l.some((s) => s.id === edit.id));
    if (!before) return out;
    if (side) for (const [id, start] of shuffleResize(before, edit.id, edit.len, side)) out.set(id, { start });
    else for (const [id, len] of noOverlap(before.map((s) => (s.id === edit.id ? { id: s.id, start: edit.start, len: edit.len } : s)), [edit.id])) out.set(id, { len });
    return out;
  }
  const ids = edit.moved.map((m) => m.id);
  for (const [track, before] of lanes) {
    const arriving = edit.moved.filter((m) => m.track === track).map((m) => ({ id: m.id, start: m.start, len: spanOf(m.id)?.len ?? 0 }));
    const leaving = before.filter((s) => ids.includes(s.id)).map((s) => s.id);
    if (!arriving.length && !leaving.length) continue;
    if (side) {
      for (const [id, start] of shuffle(before, leaving, arriving, side)) out.set(id, { start });
    } else if (arriving.length) {
      // Moved regions go on top, as the engine puts them.
      const lane = [...before.filter((s) => !ids.includes(s.id)), ...arriving];
      for (const [id, len] of noOverlap(lane, ids)) out.set(id, { len });
    }
  }
  return out;
}
