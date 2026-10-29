// The Piano Roll's editing logic as pure functions over a working list of notes and controller
// events. The component turns the project's regions into this list, applies these functions,
// and sends the result back with `set_midi_content`.
//
// Times in the working list are timeline samples (not region-relative), so edits that span
// several regions, or quantize to the project grid, need no conversions. Functions that change
// "the selection" act on every note when nothing is selected, as Logic's do. Every note means
// the whole working list: all the MIDI regions of the track the Piano Roll shows, not just the
// region being edited.
//
// This module only imports types, so it runs under plain Node for the tests in tests/.
import type { CcEvent, CcKind, Id, Note, Region } from "./api";
import type { BarLines } from "./tempo";

// ---- working list ---------------------------------------------------------------------------

/** A note in the working list: timeline times, its region, and whether it is selected. */
export interface PNote {
  region: Id;
  start: number;
  length: number;
  pitch: number;
  velocity: number;
  muted: boolean;
  sel: boolean;
}

/** A controller event in the working list (timeline time). */
export interface PCc {
  region: Id;
  start: number;
  kind: CcKind;
  value: number;
  sel: boolean;
}

/** What `set_midi_content` takes for one region. */
export interface ContentEdit {
  region: Id;
  notes: Note[];
  cc: CcEvent[];
  start?: number;
  length?: number;
}

const noteKey = (region: Id, i: number) => `${region}:${i}`;

/** Builds the working list from MIDI regions. `selNotes` / `selCc` hold "region:index" keys. */
export function fromRegions(regions: Region[], selNotes: Set<string> = new Set(), selCc: Set<string> = new Set()): { notes: PNote[]; cc: PCc[] } {
  const notes: PNote[] = [];
  const cc: PCc[] = [];
  for (const r of regions) {
    if (r.content.type !== "midi") continue;
    r.content.notes.forEach((n, i) =>
      notes.push({ region: r.id, start: r.start + n.start, length: n.length, pitch: n.pitch, velocity: n.velocity, muted: !!n.muted, sel: selNotes.has(noteKey(r.id, i)) }),
    );
    (r.content.cc ?? []).forEach((c, i) => cc.push({ region: r.id, start: r.start + c.start, kind: c.kind, value: c.value, sel: selCc.has(noteKey(r.id, i)) }));
  }
  return { notes, cc };
}

export const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Smallest / largest `f(x)` (no spread, so long lists are fine); ±Infinity when empty. */
export function minOf<T>(xs: Iterable<T>, f: (x: T) => number): number {
  let m = Infinity;
  for (const x of xs) m = Math.min(m, f(x));
  return m;
}

export function maxOf<T>(xs: Iterable<T>, f: (x: T) => number): number {
  let m = -Infinity;
  for (const x of xs) m = Math.max(m, f(x));
  return m;
}

/** The largest value a controller kind takes. */
export const ccMax = (k: CcKind) => (k.type === "pitch_bend" ? 16383 : 127);

export const sameKind = (a: CcKind, b: CcKind) => a.type === b.type && (a.type !== "controller" || a.cc === (b as { cc: number }).cc);

/**
 * Turns the working list back into region content. Regions grow (to whole bars) to hold notes
 * that moved past their edges; regions whose content didn't change are left out. Also returns
 * the selection as keys into the content as the engine will store it, which is sorted the same
 * way (notes by start then pitch, controllers by start, both stable).
 */
export function toEdits(notes: PNote[], cc: PCc[], regions: Region[], bar: Bars): { edits: ContentEdit[]; selNotes: Set<string>; selCc: Set<string> } {
  const edits: ContentEdit[] = [];
  const selNotes = new Set<string>();
  const selCc = new Set<string>();
  for (const r of regions) {
    if (r.content.type !== "midi") continue;
    const mine = notes.filter((n) => n.region === r.id);
    const myCc = cc.filter((c) => c.region === r.id);
    let start = r.start;
    let end = r.start + r.length;
    const lo = Math.min(minOf(mine, (n) => n.start), minOf(myCc, (c) => c.start));
    const hi = Math.max(maxOf(mine, (n) => n.start + n.length), maxOf(myCc, (c) => c.start + 1));
    if (lo < start) start = Math.max(0, barFloor(lo, bar));
    if (hi > end) end = barCeil(hi, bar);
    start = Math.round(start);
    end = Math.round(end);
    const rel = mine
      .map((n) => ({
        n: {
          start: Math.round(n.start - start),
          length: Math.max(1, Math.round(n.length)),
          pitch: clamp(Math.round(n.pitch), 0, 127),
          velocity: clamp(Math.round(n.velocity), 1, 127),
          muted: n.muted,
        } as Note,
        sel: n.sel,
      }))
      .sort((a, b) => a.n.start - b.n.start || a.n.pitch - b.n.pitch);
    const relCc = myCc
      .map((c) => ({ c: { start: Math.round(c.start - start), kind: c.kind, value: clamp(Math.round(c.value), 0, ccMax(c.kind)) } as CcEvent, sel: c.sel }))
      .sort((a, b) => a.c.start - b.c.start);
    rel.forEach((x, i) => x.sel && selNotes.add(noteKey(r.id, i)));
    relCc.forEach((x, i) => x.sel && selCc.add(noteKey(r.id, i)));
    const e: ContentEdit = { region: r.id, notes: rel.map((x) => x.n), cc: relCc.map((x) => x.c) };
    if (start !== r.start) e.start = start;
    if (start !== r.start || end !== r.start + r.length) e.length = end - start;
    const before = JSON.stringify([r.content.notes.map((n) => [n.start, n.length, n.pitch, n.velocity, !!n.muted]), r.content.cc ?? []]);
    const after = JSON.stringify([e.notes.map((n) => [n.start, n.length, n.pitch, n.velocity, !!n.muted]), e.cc]);
    if (before !== after || e.length !== undefined) edits.push(e);
  }
  return { edits, selNotes, selCc };
}

/** The regions as they will be once `edits` are applied, for showing an edit straight away. */
export function applyEdits(regions: Region[], edits: ContentEdit[]): Region[] {
  return regions.map((r) => {
    const e = edits.find((x) => x.region === r.id);
    if (!e || r.content.type !== "midi") return r;
    return { ...r, start: e.start ?? r.start, length: e.length ?? r.length, content: { ...r.content, notes: e.notes, cc: e.cc } };
  });
}

/** "region:index" keys of the selected notes and events, for a list in region content order. */
export function selectionKeys(items: { region: Id; sel: boolean }[]): Set<string> {
  const count = new Map<Id, number>();
  const out = new Set<string>();
  for (const x of items) {
    const i = count.get(x.region) ?? 0;
    count.set(x.region, i + 1);
    if (x.sel) out.add(noteKey(x.region, i));
  }
  return out;
}

// ---- time and grid --------------------------------------------------------------------------

export interface TimeBase {
  sampleRate: number;
  tempo: number;
  sigNum: number;
  sigDen: number;
}

export const quarterLength = (tb: TimeBase) => (tb.sampleRate * 60) / tb.tempo;
export const barLength = (tb: TimeBase) => (quarterLength(tb) * 4 * tb.sigNum) / tb.sigDen;

/** Grid and quantize values, as Logic lists them: straight and triplet divisions. */
export const GRID_VALUES = ["1/1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64", "1/2T", "1/4T", "1/8T", "1/16T", "1/32T"] as const;
export type GridValue = (typeof GRID_VALUES)[number];

/** Length of a grid value in samples ("1/16T" is a sixteenth triplet). */
export function gridLength(tb: TimeBase, v: string): number {
  const m = /^1\/(\d+)(T?)$/.exec(v);
  const div = m ? parseInt(m[1]) : 16;
  const whole = quarterLength(tb) * 4;
  return m?.[2] ? (whole / div) * (2 / 3) : whole / div;
}

/** Bar lines: a bar length (one tempo and signature throughout), or the bar line at or before
 *  and at or after a time (a tempo map; see `barLines` and `musicalWarp` in tempo.ts). */
export type Bars = number | BarLines;

export const barFloor = (t: number, bar: Bars) => (typeof bar === "number" ? Math.floor(t / bar) * bar : bar.floor(t));
export const barCeil = (t: number, bar: Bars) => (typeof bar === "number" ? Math.ceil(t / bar) * bar : bar.ceil(t));

/** Snaps a time to the grid, counted from the start of its bar (so odd meters line up). */
export function snapTime(t: number, grid: number, bar: Bars): number {
  const b = barFloor(t, bar);
  return b + Math.round((t - b) / grid) * grid;
}

/** Selected notes, or all of them when none are. */
export function targets<T extends { sel: boolean }>(items: T[]): Set<T> {
  const sel = items.filter((n) => n.sel);
  return new Set(sel.length ? sel : items);
}

// ---- quantize -------------------------------------------------------------------------------

export interface QuantizeOpts {
  /** Grid step in samples. */
  grid: number;
  /** Bar length in samples (or the bar lines); the grid is counted from each bar line. */
  bar: Bars;
  /** 0–1: how far notes move towards the grid. */
  strength: number;
  /** 0–1: delays every second grid step by up to half a step (0.66 is a triplet feel). */
  swing: number;
  /** Also quantize note ends. */
  ends: boolean;
}

/** Where `t` quantizes to (fully), swing included. */
export function quantizeTarget(t: number, o: QuantizeOpts): number {
  const b = barFloor(t, o.bar);
  const step = Math.round((t - b) / o.grid);
  return b + step * o.grid + (step % 2 === 1 ? (o.swing * o.grid) / 2 : 0);
}

export function quantize(notes: PNote[], o: QuantizeOpts): PNote[] {
  const tg = targets(notes);
  return notes.map((n) => {
    if (!tg.has(n)) return n;
    const start = n.start + (quantizeTarget(n.start, o) - n.start) * o.strength;
    let end = n.start + n.length;
    if (o.ends) {
      end += (snapTime(end, o.grid, o.bar) - end) * o.strength;
      // A note squashed to nothing gets one grid step.
      if (end - start < o.grid / 2) end = start + o.grid;
    } else {
      end = start + n.length;
    }
    return { ...n, start: Math.max(0, Math.round(start)), length: Math.max(1, Math.round(end - start)) };
  });
}

// ---- pitch, velocity, length --------------------------------------------------------------

export function transpose(notes: PNote[], semis: number): PNote[] {
  const tg = targets(notes);
  return notes.map((n) => (tg.has(n) ? { ...n, pitch: clamp(n.pitch + semis, 0, 127) } : n));
}

/** Moves the selection by `dt` samples and `dp` semitones (not before 0, within 0–127). */
export function nudge(notes: PNote[], dt: number, dp: number): PNote[] {
  const sel = notes.filter((n) => n.sel);
  if (!sel.length) return notes;
  // Keep the selection's shape: clamp the move, not each note.
  const minStart = minOf(sel, (n) => n.start);
  const lo = minOf(sel, (n) => n.pitch);
  const hi = maxOf(sel, (n) => n.pitch);
  const t = Math.max(-minStart, dt);
  const p = clamp(dp, -lo, 127 - hi);
  return notes.map((n) => (n.sel ? { ...n, start: n.start + t, pitch: n.pitch + p } : n));
}

export function setVelocity(notes: PNote[], v: number): PNote[] {
  const tg = targets(notes);
  return notes.map((n) => (tg.has(n) ? { ...n, velocity: clamp(Math.round(v), 1, 127) } : n));
}

/** Scales velocities by a percentage (100 = unchanged). */
export function scaleVelocity(notes: PNote[], percent: number): PNote[] {
  const tg = targets(notes);
  return notes.map((n) => (tg.has(n) ? { ...n, velocity: clamp(Math.round((n.velocity * percent) / 100), 1, 127) } : n));
}

export function offsetVelocity(notes: PNote[], delta: number): PNote[] {
  const tg = targets(notes);
  return notes.map((n) => (tg.has(n) ? { ...n, velocity: clamp(Math.round(n.velocity + delta), 1, 127) } : n));
}

/** Velocities along a line from (t0, v0) to (t1, v1), for the notes starting between them. */
export function rampVelocity(notes: PNote[], t0: number, v0: number, t1: number, v1: number): PNote[] {
  const tg = targets(notes);
  const [a, b, va, vb] = t0 <= t1 ? [t0, t1, v0, v1] : [t1, t0, v1, v0];
  return notes.map((n) => {
    if (!tg.has(n) || n.start < a || n.start > b) return n;
    const f = b > a ? (n.start - a) / (b - a) : 0;
    return { ...n, velocity: clamp(Math.round(va + (vb - va) * f), 1, 127) };
  });
}

export function setLength(notes: PNote[], length: number): PNote[] {
  const tg = targets(notes);
  return notes.map((n) => (tg.has(n) ? { ...n, length: Math.max(1, Math.round(length)) } : n));
}

/** Extends each note to the start of the next one (chords move together), like Force Legato. */
export function legato(notes: PNote[]): PNote[] {
  const tg = targets(notes);
  const starts = [...new Set([...tg].map((n) => n.start))].sort((a, b) => a - b);
  return notes.map((n) => {
    if (!tg.has(n)) return n;
    const next = starts.find((s) => s > n.start);
    return next === undefined ? n : { ...n, length: next - n.start };
  });
}

/** Shortens notes that overlap the next note of the same pitch (Logic's Overlap Correction). */
export function removeOverlaps(notes: PNote[]): PNote[] {
  const tg = targets(notes);
  const out = notes.slice();
  const byPitch = new Map<number, number[]>();
  out.forEach((n, i) => {
    if (!tg.has(n)) return;
    const l = byPitch.get(n.pitch) ?? [];
    l.push(i);
    byPitch.set(n.pitch, l);
  });
  for (const idx of byPitch.values()) {
    idx.sort((a, b) => out[a].start - out[b].start);
    for (let k = 0; k + 1 < idx.length; k++) {
      const n = out[idx[k]];
      const next = out[idx[k + 1]];
      if (n.start + n.length > next.start && next.start > n.start) out[idx[k]] = { ...n, length: next.start - n.start };
    }
  }
  return out;
}

/** Removes notes that duplicate another note's pitch and start (keeps the louder). */
export function deleteDuplicates(notes: PNote[]): PNote[] {
  const tg = targets(notes);
  const best = new Map<string, PNote>();
  for (const n of notes) {
    if (!tg.has(n)) continue;
    const k = `${n.region}:${n.pitch}:${Math.round(n.start)}`;
    const b = best.get(k);
    if (!b || n.velocity > b.velocity) best.set(k, n);
  }
  return notes.filter((n) => !tg.has(n) || best.get(`${n.region}:${n.pitch}:${Math.round(n.start)}`) === n);
}

/** Random offsets in ±`time` samples, ±`velocity` and ±`length` samples. */
export function humanize(notes: PNote[], o: { time: number; velocity: number; length: number }, rand: () => number = Math.random): PNote[] {
  const tg = targets(notes);
  const r = () => rand() * 2 - 1;
  return notes.map((n) => {
    if (!tg.has(n)) return n;
    return {
      ...n,
      start: Math.max(0, Math.round(n.start + r() * o.time)),
      velocity: clamp(Math.round(n.velocity + r() * o.velocity), 1, 127),
      length: Math.max(1, Math.round(n.length + r() * o.length)),
    };
  });
}

/** Mutes the selection, or unmutes it when it is all muted already. */
export function toggleMute(notes: PNote[]): PNote[] {
  const tg = targets(notes);
  const muted = ![...tg].every((n) => n.muted);
  return notes.map((n) => (tg.has(n) ? { ...n, muted } : n));
}

/** Mirrors the selection in time within its span (retrograde). */
export function reverse(notes: PNote[]): PNote[] {
  const tg = [...targets(notes)];
  if (!tg.length) return notes;
  const lo = minOf(tg, (n) => n.start);
  const hi = maxOf(tg, (n) => n.start + n.length);
  const set = new Set(tg);
  return notes.map((n) => (set.has(n) ? { ...n, start: lo + hi - (n.start + n.length) } : n));
}

/** Pitch classes (0 = C) of a key such as "E♭ major" or "F# minor"; natural minor. */
export function scaleOf(key: string): Set<number> {
  const m = /^([A-G])([#♯b♭]?)\s*(major|minor)?/i.exec(key.trim());
  if (!m) return new Set([0, 2, 4, 5, 7, 9, 11]);
  const base = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 }[m[1].toUpperCase() as "C"]!;
  const acc = m[2] === "#" || m[2] === "♯" ? 1 : m[2] === "b" || m[2] === "♭" ? -1 : 0;
  const root = (base + acc + 12) % 12;
  const steps = m[3]?.toLowerCase() === "minor" ? [0, 2, 3, 5, 7, 8, 10] : [0, 2, 4, 5, 7, 9, 11];
  return new Set(steps.map((s) => (root + s) % 12));
}

/** Moves notes outside the scale to the nearest scale note (down on a tie), like Scale Quantize. */
export function scaleQuantize(notes: PNote[], scale: Set<number>): PNote[] {
  const tg = targets(notes);
  return notes.map((n) => {
    if (!tg.has(n) || scale.has(n.pitch % 12)) return n;
    for (let d = 1; d < 12; d++) {
      if (n.pitch - d >= 0 && scale.has((n.pitch - d) % 12)) return { ...n, pitch: n.pitch - d };
      if (n.pitch + d <= 127 && scale.has((n.pitch + d) % 12)) return { ...n, pitch: n.pitch + d };
    }
    return n;
  });
}

// ---- creating, splitting, joining -----------------------------------------------------------

/** Cuts the notes under `t` in two there (the Scissors tool). Both halves stay selected. */
export function splitAt(notes: PNote[], t: number, only?: Set<PNote>): PNote[] {
  const out: PNote[] = [];
  for (const n of notes) {
    if ((only && !only.has(n)) || t <= n.start || t >= n.start + n.length) {
      out.push(n);
      continue;
    }
    out.push({ ...n, length: t - n.start }, { ...n, start: t, length: n.start + n.length - t });
  }
  return out;
}

/** Joins selected notes of the same pitch into one note each (the Glue tool). */
export function glue(notes: PNote[]): PNote[] {
  const sel = notes.filter((n) => n.sel);
  const groups = new Map<string, PNote[]>();
  for (const n of sel) {
    const k = `${n.region}:${n.pitch}`;
    groups.set(k, [...(groups.get(k) ?? []), n]);
  }
  const drop = new Set<PNote>();
  const add: PNote[] = [];
  for (const g of groups.values()) {
    if (g.length < 2) continue;
    g.forEach((n) => drop.add(n));
    const start = minOf(g, (n) => n.start);
    const end = maxOf(g, (n) => n.start + n.length);
    add.push({ ...g[0], start, length: end - start, velocity: maxOf(g, (n) => n.velocity) });
  }
  return [...notes.filter((n) => !drop.has(n)), ...add];
}

/** A note on the clipboard, relative to the earliest copied note. */
export interface ClipNote {
  offset: number;
  length: number;
  pitch: number;
  velocity: number;
  muted: boolean;
}

export function copyNotes(notes: PNote[]): ClipNote[] {
  const sel = notes.filter((n) => n.sel);
  if (!sel.length) return [];
  const lo = minOf(sel, (n) => n.start);
  return sel.map((n) => ({ offset: n.start - lo, length: n.length, pitch: n.pitch, velocity: n.velocity, muted: n.muted }));
}

/** Adds the clipboard at `at` in `region`; the pasted notes become the selection. */
export function pasteNotes(notes: PNote[], clip: ClipNote[], at: number, region: Id): PNote[] {
  return [
    ...notes.map((n) => (n.sel ? { ...n, sel: false } : n)),
    ...clip.map((c) => ({ region, start: Math.max(0, Math.round(at + c.offset)), length: c.length, pitch: c.pitch, velocity: c.velocity, muted: c.muted, sel: true })),
  ];
}

/** A controller event on the clipboard, relative to the earliest copied event. */
export interface ClipCc {
  offset: number;
  kind: CcKind;
  value: number;
}

export function copyCc(cc: PCc[]): ClipCc[] {
  const sel = cc.filter((c) => c.sel);
  if (!sel.length) return [];
  const lo = minOf(sel, (c) => c.start);
  return sel.map((c) => ({ offset: c.start - lo, kind: c.kind, value: c.value }));
}

/** Adds copied controller events at `at` in `region`; they become the selection. */
export function pasteCc(cc: PCc[], clip: ClipCc[], at: number, region: Id): PCc[] {
  const pasted = clip.map((c) => ({ region, start: Math.max(0, Math.round(at + c.offset)), kind: c.kind, value: c.value, sel: true }));
  return [...cc.map((c) => (c.sel ? { ...c, sel: false } : c)), ...pasted].sort((a, b) => a.start - b.start);
}

/** Copies the selection straight after itself, rounded up to the grid (⌘D). */
export function duplicate(notes: PNote[], grid: number): PNote[] {
  const sel = notes.filter((n) => n.sel);
  if (!sel.length) return notes;
  const lo = minOf(sel, (n) => n.start);
  const hi = maxOf(sel, (n) => n.start + n.length);
  const span = Math.max(grid, Math.ceil((hi - lo) / grid - 1e-6) * grid);
  return [...notes.map((n) => (n.sel ? { ...n, sel: false } : n)), ...sel.map((n) => ({ ...n, start: n.start + span, sel: true }))];
}

// ---- selection ------------------------------------------------------------------------------

export const selectAll = (notes: PNote[]) => notes.map((n) => (n.sel ? n : { ...n, sel: true }));
export const selectNone = (notes: PNote[]) => notes.map((n) => (n.sel ? { ...n, sel: false } : n));
export const invertSelection = (notes: PNote[]) => notes.map((n) => ({ ...n, sel: !n.sel }));

/** Selects every note with a pitch that a selected note has. */
export function selectSamePitch(notes: PNote[]): PNote[] {
  const pitches = new Set(notes.filter((n) => n.sel).map((n) => n.pitch));
  return notes.map((n) => ({ ...n, sel: pitches.has(n.pitch) }));
}

/**
 * Keeps only the highest (or lowest) note of each chord in the selection (all notes when none
 * are selected). Notes starting within `tolerance` samples of each other form a chord.
 */
export function selectTop(notes: PNote[], highest: boolean, tolerance = 0): PNote[] {
  const tg = [...targets(notes)].sort((a, b) => a.start - b.start);
  const keep = new Set<PNote>();
  let i = 0;
  while (i < tg.length) {
    let j = i;
    let best = tg[i];
    while (j < tg.length && tg[j].start - tg[i].start <= tolerance) {
      if (highest ? tg[j].pitch > best.pitch : tg[j].pitch < best.pitch) best = tg[j];
      j++;
    }
    keep.add(best);
    i = j;
  }
  return notes.map((n) => ({ ...n, sel: keep.has(n) }));
}

/** Selects the notes inside a time/pitch rectangle; `add` keeps the current selection. */
export function selectRect(notes: PNote[], t0: number, t1: number, p0: number, p1: number, add: boolean): PNote[] {
  const [a, b] = t0 < t1 ? [t0, t1] : [t1, t0];
  const [lo, hi] = p0 < p1 ? [p0, p1] : [p1, p0];
  return notes.map((n) => {
    const inside = n.start < b && n.start + n.length > a && n.pitch >= lo && n.pitch <= hi;
    const sel = add ? n.sel || inside : inside;
    return sel === n.sel ? n : { ...n, sel };
  });
}

// ---- controller events ----------------------------------------------------------------------

/**
 * Replaces the `kind` events of `region` between t0 and t1 with points along a line (the
 * pencil and line tools), one every `step` samples.
 */
export function drawCcLine(cc: PCc[], region: Id, kind: CcKind, t0: number, v0: number, t1: number, v1: number, step: number): PCc[] {
  const [a, b, va, vb] = t0 <= t1 ? [t0, t1, v0, v1] : [t1, t0, v1, v0];
  const kept = cc.filter((c) => !(c.region === region && sameKind(c.kind, kind) && c.start >= a && c.start <= b));
  const max = ccMax(kind);
  const pts: PCc[] = [];
  const n = Math.max(1, Math.round((b - a) / Math.max(1, step)));
  let last = -1;
  for (let i = 0; i <= n; i++) {
    const t = a + ((b - a) * i) / n;
    const v = clamp(Math.round(va + (vb - va) * (b > a ? (t - a) / (b - a) : 0)), 0, max);
    // Skip repeats: a flat line needs only its ends.
    if (v === last && i !== n) continue;
    last = v;
    pts.push({ region, start: Math.round(t), kind, value: v, sel: false });
  }
  return [...kept, ...pts].sort((x, y) => x.start - y.start);
}

/** Removes the `kind` events between t0 and t1 (the eraser), only in `region` if given. */
export function eraseCc(cc: PCc[], kind: CcKind, t0: number, t1: number, region?: Id): PCc[] {
  const [a, b] = t0 <= t1 ? [t0, t1] : [t1, t0];
  return cc.filter((c) => !(sameKind(c.kind, kind) && c.start >= a && c.start <= b && (region === undefined || c.region === region)));
}

/** The value a controller has at `t` (the last event at or before it), or null. */
export function ccValueAt(cc: PCc[], kind: CcKind, t: number): number | null {
  let v: number | null = null;
  let at = -Infinity;
  for (const c of cc) if (sameKind(c.kind, kind) && c.start <= t && c.start >= at) [v, at] = [c.value, c.start];
  return v;
}
