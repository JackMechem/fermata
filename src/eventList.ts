// The Event List's logic, as pure functions: positions and lengths in bars, beats, divisions
// and ticks through the tempo map, typed values, and edits on the Piano Roll's working list of
// notes and controller events (midiEdit.ts), which the component sends back the same way.
//
// Positions follow the app's LCD: 1-based bar, beat (of the signature's denominator), division
// (a quarter of a beat) and tick (240 a division). Lengths are written the same way but count
// from 0, as in Logic: a quarter note in 4/4 is "0 1 0 0". Both are rounded to the nearest tick,
// so a position typed in shows as typed whatever the sample rate.
//
// This module only imports types, so it runs under plain Node for the tests in tests/.
import type { CcKind } from "./api";
import type { PCc, PNote } from "./midiEdit";
import type { Bbt, TempoMap } from "./tempo";

/** Ticks a division, and a beat. */
export const TICKS_PER_DIV = 240;
export const TICKS_PER_BEAT = TICKS_PER_DIV * 4;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

// ---- positions and lengths ------------------------------------------------------------------

/** The position of timeline sample `s`, to the nearest tick. */
export function positionBbt(map: TempoMap, s: number): Bbt {
  let bar = map.barAt(s);
  let [num, den] = map.sigAtBar(bar);
  let ticks = Math.max(0, Math.round((((map.beatsAt(s) - map.barStartQ(bar)) * den) / 4) * TICKS_PER_BEAT));
  // Rounded up onto the next bar line.
  if (ticks >= num * TICKS_PER_BEAT) {
    ticks -= num * TICKS_PER_BEAT;
    bar++;
    [num, den] = map.sigAtBar(bar);
  }
  const beat = Math.floor(ticks / TICKS_PER_BEAT);
  const rest = ticks - beat * TICKS_PER_BEAT;
  const div = Math.floor(rest / TICKS_PER_DIV);
  return { bar, beat: beat + 1, div: div + 1, tick: rest - div * TICKS_PER_DIV + 1 };
}

/** The timeline sample at a position. Parts past their range carry over (beat 5 in 4/4 is the
 *  next bar's first beat); nothing lands before bar 1. */
export function bbtToSample(map: TempoMap, b: Bbt): number {
  const bar = Math.max(1, Math.round(b.bar));
  const beatQ = 4 / map.sigAtBar(bar)[1];
  const beats = b.beat - 1 + (b.div - 1) / 4 + (b.tick - 1) / TICKS_PER_BEAT;
  const q = map.barStartQ(bar) + beats * beatQ;
  return Math.max(0, Math.round(map.sampleAt(Math.max(0, q))));
}

export const formatBbt = (b: Bbt) => `${b.bar} ${b.beat} ${b.div} ${b.tick}`;

export const formatPosition = (map: TempoMap, s: number) => formatBbt(positionBbt(map, s));

/** A length starting at `start`, in the signature there, to the nearest tick (0-based). */
export function lengthBbt(map: TempoMap, start: number, len: number): Bbt {
  const [num, den] = map.sigAt(start);
  const ticks = Math.max(0, Math.round((((map.beatsAt(start + len) - map.beatsAt(start)) * den) / 4) * TICKS_PER_BEAT));
  const bar = Math.floor(ticks / (num * TICKS_PER_BEAT));
  let rest = ticks - bar * num * TICKS_PER_BEAT;
  const beat = Math.floor(rest / TICKS_PER_BEAT);
  rest -= beat * TICKS_PER_BEAT;
  const div = Math.floor(rest / TICKS_PER_DIV);
  return { bar, beat, div, tick: rest - div * TICKS_PER_DIV };
}

export const formatLength = (map: TempoMap, start: number, len: number) => formatBbt(lengthBbt(map, start, len));

/** A length in samples from a 0-based bars/beats/divisions/ticks length at `start`. Parts carry
 *  over, and the length is at least one tick. */
export function lengthToSamples(map: TempoMap, start: number, b: Bbt): number {
  const [num, den] = map.sigAt(start);
  const ticks = Math.max(1, Math.round(((b.bar * num + b.beat) * 4 + b.div) * TICKS_PER_DIV + b.tick));
  const end = map.sampleAt(map.beatsAt(start) + ((ticks / TICKS_PER_BEAT) * 4) / den);
  return Math.max(1, Math.round(end) - start);
}

/** The numbers of a typed position or length: "5 2 1 1", "5.2.1.1", "5:2", "5". Null unless
 *  it is one to four whole numbers. */
function parseParts(text: string): number[] | null {
  const t = text.trim();
  if (!t) return null;
  const parts = t.split(/[\s.,:;/]+/).filter((p) => p !== "");
  if (!parts.length || parts.length > 4 || !parts.every((p) => /^-?\d+$/.test(p))) return null;
  return parts.map((p) => parseInt(p, 10));
}

/** A typed position as a timeline sample. Missing parts are 1 ("5" is bar 5's first beat). */
export function parsePosition(text: string, map: TempoMap): number | null {
  const p = parseParts(text);
  if (!p) return null;
  const [bar, beat = 1, div = 1, tick = 1] = p;
  return bbtToSample(map, { bar, beat, div, tick });
}

/** A typed length (for an event at `start`) in samples. Missing parts are 0 ("0 2" is two
 *  beats). */
export function parseLength(text: string, map: TempoMap, start: number): number | null {
  const p = parseParts(text);
  if (!p) return null;
  const [bar, beat = 0, div = 0, tick = 0] = p;
  return lengthToSamples(map, start, { bar, beat, div, tick });
}

// ---- note names and controllers -------------------------------------------------------------

const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/** Logic's note names: middle C (MIDI 60) is C3. */
export const noteName = (p: number) => `${NAMES[p % 12]}${Math.floor(p / 12) - 2}`;

/** A typed note: a name ("C3", "f#2", "Bb-1", "E♭4") or a MIDI note number. Numbers are clamped
 *  to 0–127; a name outside that range is null. */
export function parseNote(text: string): number | null {
  const t = text.trim();
  if (/^\d+$/.test(t)) return clamp(parseInt(t, 10), 0, 127);
  const m = /^([A-Ga-g])\s*([#♯b♭]?)\s*(-?\d+)$/.exec(t);
  if (!m) return null;
  const base = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 }[m[1].toUpperCase() as "C"];
  const acc = m[2] === "#" || m[2] === "♯" ? 1 : m[2] ? -1 : 0;
  const p = (parseInt(m[3], 10) + 2) * 12 + base + acc;
  return p >= 0 && p <= 127 ? p : null;
}

/** A typed number, clamped to `lo`–`hi`; null unless it is a whole number. */
export function parseNumber(text: string, lo: number, hi: number): number | null {
  const t = text.trim();
  return /^-?\d+$/.test(t) ? clamp(parseInt(t, 10), lo, hi) : null;
}

const CC_NAMES: Record<number, string> = {
  0: "Bank MSB",
  1: "Modulation",
  2: "Breath",
  4: "Foot Control",
  5: "Portamento Time",
  6: "Data Entry",
  7: "Volume",
  8: "Balance",
  10: "Pan",
  11: "Expression",
  32: "Bank LSB",
  64: "Sustain",
  65: "Portamento",
  66: "Sostenuto",
  67: "Soft Pedal",
  68: "Legato",
  71: "Resonance",
  72: "Release Time",
  73: "Attack Time",
  74: "Cutoff",
  91: "Reverb",
  93: "Chorus",
  120: "All Sound Off",
  121: "Reset Controllers",
  123: "All Notes Off",
};

/** A controller's name ("Modulation"), or "" for a number without one. */
export const ccName = (n: number) => CC_NAMES[n] ?? "";

// ---- rows -----------------------------------------------------------------------------------

/** The kinds of event Fermata stores, as the Event List's filter buttons name them. */
export type EventType = "note" | "bend" | "control" | "pressure";

/** `label` is the filter button's, `status` the Status column's, `name` one event's. */
export const EVENT_TYPES: { id: EventType; label: string; status: string; name: string }[] = [
  { id: "note", label: "Notes", status: "Note", name: "Note" },
  { id: "bend", label: "Pitch Bend", status: "Pitch Bend", name: "Pitch Bend" },
  { id: "control", label: "Controller", status: "Control", name: "Controller" },
  { id: "pressure", label: "Chnl Pressure", status: "C-Press", name: "Channel Pressure" },
];

export const ccType = (k: CcKind): EventType => (k.type === "pitch_bend" ? "bend" : k.type === "aftertouch" ? "pressure" : "control");

/** One line of the list: an event of the working list, by kind and index. */
export interface Row {
  /** "n<index>" for a note, "c<index>" for a controller event. */
  key: string;
  note: boolean;
  /** Index in the working notes or controller list. */
  idx: number;
  type: EventType;
  start: number;
}

/** The lists the Event List edits: the Piano Roll's working list (timeline times). */
export interface Lists {
  notes: PNote[];
  cc: PCc[];
}

const TYPE_ORDER: Record<EventType, number> = { note: 0, control: 1, bend: 2, pressure: 3 };

/** The rows of the types shown, by position; at the same position notes come first (lowest
 *  pitch first), then controllers by number, pitch bend and pressure. */
export function buildRows(l: Lists, shown: (t: EventType) => boolean = () => true): Row[] {
  const rows: (Row & { sub: number })[] = [];
  if (shown("note")) l.notes.forEach((n, i) => rows.push({ key: `n${i}`, note: true, idx: i, type: "note", start: n.start, sub: n.pitch }));
  l.cc.forEach((c, i) => {
    const type = ccType(c.kind);
    if (shown(type)) rows.push({ key: `c${i}`, note: false, idx: i, type, start: c.start, sub: c.kind.type === "controller" ? c.kind.cc : 0 });
  });
  rows.sort((a, b) => a.start - b.start || TYPE_ORDER[a.type] - TYPE_ORDER[b.type] || a.sub - b.sub || a.idx - b.idx);
  return rows.map(({ sub: _, ...r }) => r);
}

/** The row playing at `pos`: the last one starting at or before it, or -1. */
export function rowAt(rows: Row[], pos: number): number {
  let lo = 0;
  let hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (rows[mid].start <= pos) lo = mid + 1;
    else hi = mid;
  }
  return lo - 1;
}

export const isSelected = (l: Lists, r: Row) => (r.note ? l.notes[r.idx]?.sel : l.cc[r.idx]?.sel) ?? false;

/** The lists with exactly the rows in `keys` selected. */
export function selectKeys(l: Lists, keys: Set<string>): Lists {
  return {
    notes: l.notes.map((n, i) => (n.sel === keys.has(`n${i}`) ? n : { ...n, sel: !n.sel })),
    cc: l.cc.map((c, i) => (c.sel === keys.has(`c${i}`) ? c : { ...c, sel: !c.sel })),
  };
}

/** The keys of the selected events. */
export function selectedKeys(l: Lists): Set<string> {
  const out = new Set<string>();
  l.notes.forEach((n, i) => n.sel && out.add(`n${i}`));
  l.cc.forEach((c, i) => c.sel && out.add(`c${i}`));
  return out;
}

/**
 * The selection after a click on row `i`: just it; with `toggle` (⌘) it flips; with `range`
 * (Shift) every row from `anchor` to it is added.
 */
export function clickSelect(rows: Row[], current: Set<string>, i: number, anchor: number | null, mods: { toggle: boolean; range: boolean }): Set<string> {
  const key = rows[i].key;
  if (mods.range && anchor !== null && anchor >= 0 && anchor < rows.length) {
    const out = new Set(current);
    const [a, b] = anchor <= i ? [anchor, i] : [i, anchor];
    for (let k = a; k <= b; k++) out.add(rows[k].key);
    return out;
  }
  if (mods.toggle || mods.range) {
    const out = new Set(current);
    if (out.has(key)) out.delete(key);
    else out.add(key);
    return out;
  }
  return new Set([key]);
}

/** The lists without the selected events. */
export function deleteSelected(l: Lists): Lists {
  return { notes: l.notes.filter((n) => !n.sel), cc: l.cc.filter((c) => !c.sel) };
}

// ---- values ---------------------------------------------------------------------------------

/** The editable columns. Position and Length have four parts (bar, beat, division, tick). */
export type Field = "pos" | "num" | "val" | "len";

/** Whether a row has a value in a column: Num for notes, controllers and pitch bend (its low
 *  byte), Length for notes. */
export function hasField(type: EventType, f: Field): boolean {
  if (f === "num") return type !== "pressure";
  if (f === "len") return type === "note";
  return true;
}

/** Num and Val as stored: a pitch bend is split into its low (Num) and high (Val) 7 bits, as
 *  Logic lists it, so 8192 (centre) is 0 and 64. */
export function numOf(l: Lists, r: Row): number {
  if (r.note) return l.notes[r.idx].pitch;
  const c = l.cc[r.idx];
  if (c.kind.type === "controller") return c.kind.cc;
  return c.kind.type === "pitch_bend" ? c.value & 127 : 0;
}

export function valOf(l: Lists, r: Row): number {
  if (r.note) return l.notes[r.idx].velocity;
  const c = l.cc[r.idx];
  return c.kind.type === "pitch_bend" ? c.value >> 7 : c.value;
}

/** Lowest and highest value of a column (velocity can't be 0: that would be a note-off). */
export function fieldRange(type: EventType, f: "num" | "val"): [number, number] {
  return f === "val" && type === "note" ? [1, 127] : [0, 127];
}

/** A note moved to `start`, keeping its length in beats (through tempo changes). */
function moveNote(map: TempoMap, n: PNote, start: number): PNote {
  const q = map.beatsAt(n.start + n.length) - map.beatsAt(n.start);
  const end = Math.round(map.sampleAt(map.beatsAt(start) + q));
  return { ...n, start, length: Math.max(1, end - start) };
}

/** Moves position or length part `part` (0 bar … 3 tick) by `d` units. */
export function shiftPosition(map: TempoMap, s: number, part: number, d: number): number {
  if (!d) return s;
  const b = positionBbt(map, s);
  const k = (["bar", "beat", "div", "tick"] as const)[part];
  return bbtToSample(map, { ...b, [k]: b[k] + d });
}

export function shiftLength(map: TempoMap, start: number, len: number, part: number, d: number): number {
  if (!d) return len;
  const b = lengthBbt(map, start, len);
  const k = (["bar", "beat", "div", "tick"] as const)[part];
  return lengthToSamples(map, start, { ...b, [k]: b[k] + d });
}

/** How a value changes: by `delta` (a drag; positions and lengths in units of `part`), or to
 *  `to` (typed; positions and lengths in samples). */
export type Change = { field: Field; part?: number; delta: number } | { field: Field; to: number };

function changeNum(l: Lists, r: Row, v: (old: number) => number): Lists {
  if (r.note) {
    const notes = l.notes.slice();
    notes[r.idx] = { ...notes[r.idx], pitch: clamp(Math.round(v(notes[r.idx].pitch)), 0, 127) };
    return { ...l, notes };
  }
  const cc = l.cc.slice();
  const c = cc[r.idx];
  if (c.kind.type === "controller") cc[r.idx] = { ...c, kind: { type: "controller", cc: clamp(Math.round(v(c.kind.cc)), 0, 127) } };
  else if (c.kind.type === "pitch_bend") cc[r.idx] = { ...c, value: (c.value & ~127) | clamp(Math.round(v(c.value & 127)), 0, 127) };
  else return l;
  return { ...l, cc };
}

function changeVal(l: Lists, r: Row, v: (old: number) => number): Lists {
  const [lo, hi] = fieldRange(r.type, "val");
  if (r.note) {
    const notes = l.notes.slice();
    notes[r.idx] = { ...notes[r.idx], velocity: clamp(Math.round(v(notes[r.idx].velocity)), lo, hi) };
    return { ...l, notes };
  }
  const cc = l.cc.slice();
  const c = cc[r.idx];
  cc[r.idx] = c.kind.type === "pitch_bend" ? { ...c, value: (clamp(Math.round(v(c.value >> 7)), lo, hi) << 7) | (c.value & 127) } : { ...c, value: clamp(Math.round(v(c.value)), lo, hi) };
  return { ...l, cc };
}

/**
 * Applies a change to the rows given. Indices stay as they are (the rows keep their keys while
 * a drag goes on); the engine sorts the events again when the result is sent. Rows without the
 * field are left alone.
 */
export function applyChange(map: TempoMap, l: Lists, rows: Row[], ch: Change): Lists {
  let out = l;
  for (const r of rows) {
    if (!hasField(r.type, ch.field)) continue;
    const by = "delta" in ch ? ch.delta : null;
    const to = "to" in ch ? ch.to : null;
    const part = "part" in ch ? (ch.part ?? 3) : 3;
    switch (ch.field) {
      case "pos": {
        const s0 = r.note ? out.notes[r.idx].start : out.cc[r.idx].start;
        const s = Math.max(0, Math.round(to ?? shiftPosition(map, s0, part, by!)));
        if (s === s0) break;
        if (r.note) {
          const notes = out.notes.slice();
          notes[r.idx] = moveNote(map, notes[r.idx], s);
          out = { ...out, notes };
        } else {
          const cc = out.cc.slice();
          cc[r.idx] = { ...cc[r.idx], start: s };
          out = { ...out, cc };
        }
        break;
      }
      case "len": {
        const n = out.notes[r.idx];
        const len = Math.max(1, Math.round(to ?? shiftLength(map, n.start, n.length, part, by!)));
        if (len === n.length) break;
        const notes = out.notes.slice();
        notes[r.idx] = { ...n, length: len };
        out = { ...out, notes };
        break;
      }
      case "num":
        out = changeNum(out, r, (old) => to ?? old + by!);
        break;
      case "val":
        out = changeVal(out, r, (old) => to ?? old + by!);
        break;
    }
  }
  return out;
}

/** What a cell shows, for the text field that edits it. */
export function cellText(map: TempoMap, l: Lists, r: Row, f: Field): string {
  switch (f) {
    case "pos":
      return formatPosition(map, r.note ? l.notes[r.idx].start : l.cc[r.idx].start);
    case "len": {
      const n = l.notes[r.idx];
      return formatLength(map, n.start, n.length);
    }
    case "num":
      return r.note ? noteName(numOf(l, r)) : String(numOf(l, r));
    case "val":
      return String(valOf(l, r));
  }
}

/** The change a typed value makes, or null when it doesn't read. */
export function parseCell(map: TempoMap, l: Lists, r: Row, f: Field, text: string): Change | null {
  let to: number | null;
  switch (f) {
    case "pos":
      to = parsePosition(text, map);
      break;
    case "len":
      to = parseLength(text, map, l.notes[r.idx].start);
      break;
    case "num":
      to = r.note ? parseNote(text) : parseNumber(text, ...fieldRange(r.type, "num"));
      break;
    case "val":
      to = parseNumber(text, ...fieldRange(r.type, "val"));
      break;
  }
  return to === null ? null : { field: f, to };
}

/** Undo labels. */
export function changeLabel(type: EventType, f: Field): string {
  if (f === "pos") return "Move Events";
  if (f === "len") return "Change Note Length";
  if (f === "num") return type === "note" ? "Change Pitch" : type === "control" ? "Change Controller Number" : "Change Pitch Bend";
  return type === "note" ? "Change Velocity" : type === "bend" ? "Change Pitch Bend" : "Change Event Value";
}

// ---- new events -----------------------------------------------------------------------------

/** A new event of `type` at `at` in `region`, selected, copying the values of `like` (an
 *  event of that type) when there is one: Logic's Create. Notes are a beat long by default. */
export function newEvent(map: TempoMap, type: EventType, at: number, region: number, like: PNote | PCc | null): { note?: PNote; cc?: PCc } {
  const start = bbtToSample(map, positionBbt(map, Math.max(0, at)));
  if (type === "note") {
    const n = like && "pitch" in like ? like : null;
    const length = n ? Math.max(1, Math.round(map.sampleAt(map.beatsAt(start) + map.beatsAt(n.start + n.length) - map.beatsAt(n.start))) - start) : lengthToSamples(map, start, { bar: 0, beat: 1, div: 0, tick: 0 });
    return { note: { region, start, length, pitch: n?.pitch ?? 60, velocity: n?.velocity ?? 100, muted: false, sel: true } };
  }
  const c = like && "kind" in like && ccType(like.kind) === type ? like : null;
  const kind: CcKind = type === "bend" ? { type: "pitch_bend" } : type === "pressure" ? { type: "aftertouch" } : (c?.kind ?? { type: "controller", cc: 1 });
  return { cc: { region, start, kind, value: c?.value ?? (type === "bend" ? 8192 : 0), sel: true } };
}
