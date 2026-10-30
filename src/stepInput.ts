// Step Input, as in Logic's Step Input Keyboard: notes are entered one step at a time at the
// playhead, each as long as the chosen note value, and the playhead moves on by a step. This
// module is the pure part: step lengths through the tempo map, the chord/advance state machine
// and the region edits it turns into. StepInput.tsx is the panel and does the sending.
//
// How a step ends: notes pressed together (keys held down at the same time) form one step, and
// the playhead advances when every key is up, so chords played on a MIDI keyboard just work.
// With Chord on, the step stays open after the keys are released, and further notes join it
// until Chord is turned off or Rest is pressed. Every step is one undo step (the notes of a
// chord share it); Back deletes the last step and moves the playhead back to it.
import type { Region } from "./api";
// With the extension, so the unit tests can run this file directly in Node.
import * as me from "./midiEdit.ts";
import { barLines, type TempoMap } from "./tempo.ts";

/** Note values, as the denominator of the fraction of a whole note. */
export const NOTE_VALUES = [1, 2, 4, 8, 16, 32, 64, 128] as const;
export type NoteValue = (typeof NOTE_VALUES)[number];

/** Logic's velocity buttons. */
export const DYNAMICS: { name: string; velocity: number }[] = [
  { name: "ppp", velocity: 16 },
  { name: "pp", velocity: 32 },
  { name: "p", velocity: 48 },
  { name: "mp", velocity: 64 },
  { name: "mf", velocity: 80 },
  { name: "f", velocity: 96 },
  { name: "ff", velocity: 112 },
  { name: "fff", velocity: 127 },
];

export interface StepOpts {
  value: NoteValue;
  dotted: boolean;
  triplet: boolean;
  /** Keep the step open after the keys are released (Logic's Chord button). */
  chord: boolean;
  /** Snap the playhead to the step grid before entering a step. */
  quantize: boolean;
}

export const DEFAULT_OPTS: StepOpts = { value: 16, dotted: false, triplet: false, chord: false, quantize: false };

/** A step's length in quarter notes. */
export function stepQuarters(o: Pick<StepOpts, "value" | "dotted" | "triplet">): number {
  return (4 / o.value) * (o.dotted ? 1.5 : 1) * (o.triplet ? 2 / 3 : 1);
}

/** Where a step starting at `at` ends, through the tempo map (so it is a note value long in
 *  musical time, whatever the tempo does). */
export function stepEnd(map: TempoMap, at: number, o: Pick<StepOpts, "value" | "dotted" | "triplet">): number {
  return Math.max(at + 1, Math.round(map.sampleAt(map.beatsAt(at) + stepQuarters(o))));
}

/** `at` snapped to the nearest step of the grid counted from its bar line. */
export function snapToStep(map: TempoMap, at: number, o: Pick<StepOpts, "value" | "dotted" | "triplet">): number {
  return Math.max(0, Math.round(map.sampleAt(map.snapQ(map.beatsAt(at), stepQuarters(o)))));
}

/** A note on the timeline, as Step Input entered it. */
export interface StepNote {
  start: number;
  length: number;
  pitch: number;
  velocity: number;
}

/** One entered step: the notes put in at `at` (none for a rest), or for Sustain the notes it
 *  lengthened, with their lengths before. */
export interface Step {
  /** Counts up per session; the undo key of the step's edits. */
  id: number;
  at: number;
  /** Samples the playhead moved on by. */
  length: number;
  notes: StepNote[];
  sustained?: { note: StepNote; before: number }[];
}

export interface StepState {
  /** Where the next step goes, once one has been entered; null: at the playhead. */
  cursor: number | null;
  /** Where the playhead was before Step Input moved it, until the engine reports the move: a
   *  report that hasn't caught up yet isn't taken for the user moving the playhead. */
  stale: number[];
  /** Keys held down. */
  down: number[];
  /** The step being entered (keys are down, or Chord is on). */
  open: Step | null;
  /** Finished steps, oldest first, for Back. */
  history: Step[];
  nextId: number;
}

export const initialState = (): StepState => ({ cursor: null, stale: [], down: [], open: null, history: [], nextId: 1 });

export type StepEvent =
  | { type: "down"; pitch: number; velocity: number }
  | { type: "up"; pitch: number }
  | { type: "rest" }
  | { type: "back" }
  | { type: "sustain" }
  /** Chord turned off: an open chord ends. */
  | { type: "chord-off" };

export type Effect =
  | { type: "insert"; note: StepNote; key: number }
  | { type: "remove"; notes: StepNote[] }
  /** Notes (found by start and pitch) get a new length. */
  | { type: "resize"; notes: { note: StepNote; length: number }[]; key: number }
  | { type: "locate"; pos: number };

/** How many steps Back can go through. */
const HISTORY = 512;

/** Where the next step goes: the step cursor, unless the playhead was moved since. */
export function position(s: StepState, playhead: number): number {
  if (s.cursor === null) return playhead;
  return playhead === s.cursor || s.stale.includes(playhead) ? s.cursor : playhead;
}

/** Takes in a playhead position the engine reported: once it shows the cursor, older reports
 *  are over; anywhere else, the user moved it and steps go there from now on. */
export function observe(s: StepState, playhead: number): StepState {
  if (s.cursor === null || s.stale.includes(playhead)) return s;
  if (playhead === s.cursor) return s.stale.length ? { ...s, stale: [] } : s;
  return { ...s, cursor: null, stale: [] };
}

/** The cursor moves to `to`: until the engine says so, the playhead may still show where it was. */
function moveCursor(s: StepState, playhead: number, to: number): StepState {
  const was = position(s, playhead);
  const stale = [...new Set([...s.stale, was, playhead])].filter((p) => p !== to).slice(-16);
  return { ...s, cursor: to, stale };
}

/** The state machine. `playhead` is where the transport is (it is stopped); `opts` the panel's
 *  settings. Returns the new state and what to do. */
export function reduce(s: StepState, ev: StepEvent, map: TempoMap, playhead: number, opts: StepOpts): { state: StepState; effects: Effect[] } {
  const effects: Effect[] = [];
  let st: StepState = { ...s };
  const begin = (): Step => {
    let at = position(st, playhead);
    if (opts.quantize) at = snapToStep(map, at, opts);
    const step: Step = { id: st.nextId, at, length: stepEnd(map, at, opts) - at, notes: [] };
    st = { ...st, nextId: st.nextId + 1 };
    return step;
  };
  /** Ends a step: the playhead moves on by its length. */
  const finish = (step: Step) => {
    const to = step.at + step.length;
    st = { ...moveCursor(st, playhead, to), open: null, history: [...st.history, step].slice(-HISTORY) };
    effects.push({ type: "locate", pos: to });
  };

  switch (ev.type) {
    case "down": {
      if (st.down.includes(ev.pitch)) break;
      const step = st.open ?? begin();
      st = { ...st, down: [...st.down, ev.pitch] };
      // The same key again in one chord adds nothing.
      if (step.notes.some((n) => n.pitch === ev.pitch)) {
        st = { ...st, open: step };
        break;
      }
      const note: StepNote = { start: step.at, length: step.length, pitch: ev.pitch, velocity: Math.max(1, Math.min(127, Math.round(ev.velocity))) };
      st = { ...st, open: { ...step, notes: [...step.notes, note] } };
      effects.push({ type: "insert", note, key: step.id });
      break;
    }
    case "up": {
      if (!st.down.includes(ev.pitch)) break;
      st = { ...st, down: st.down.filter((p) => p !== ev.pitch) };
      if (!st.down.length && st.open && !opts.chord) finish(st.open);
      break;
    }
    case "chord-off": {
      if (st.open && !st.down.length) finish(st.open);
      break;
    }
    case "rest": {
      if (st.down.length) break;
      // With a chord open, Rest ends it; otherwise it moves on by a step of silence.
      finish(st.open ?? begin());
      break;
    }
    case "back": {
      if (st.down.length) break;
      const last = st.open ?? st.history[st.history.length - 1];
      if (!last) break;
      const history = st.open ? st.history : st.history.slice(0, -1);
      if (last.sustained) effects.push({ type: "resize", notes: last.sustained.map((x) => ({ note: x.note, length: x.before })), key: last.id });
      else if (last.notes.length) effects.push({ type: "remove", notes: last.notes });
      st = { ...moveCursor(st, playhead, last.at), open: null, history };
      effects.push({ type: "locate", pos: last.at });
      break;
    }
    case "sustain": {
      if (st.down.length) break;
      if (st.open) finish(st.open);
      // The notes of the last step that put notes in, if nothing but sustains came after it.
      let i = st.history.length - 1;
      while (i >= 0 && st.history[i].sustained) i--;
      const target = i >= 0 && st.history[i].notes.length ? st.history[i] : null;
      const step = begin();
      if (!target) {
        // Nothing to lengthen: it moves on like a rest.
        finish(step);
        break;
      }
      const sustained = target.notes.map((n) => {
        // The note as it is now, after any sustains since.
        const grown = st.history.slice(i + 1).reduce((len, h) => len + h.length, n.length);
        const end = step.at + step.length;
        return { note: { ...n, length: grown }, before: grown, length: end - n.start };
      });
      effects.push({ type: "resize", notes: sustained.map((x) => ({ note: x.note, length: x.length })), key: step.id });
      finish({ ...step, sustained: sustained.map((x) => ({ note: x.note, before: x.before })) });
      break;
    }
  }
  return { state: st, effects };
}

// ---- where the notes go -------------------------------------------------------------------

/** The MIDI region on a track a step at `at` goes into: the newest one holding `at`, else the
 *  latest one ending at most a bar before it (which then grows). Null when one must be made.
 *  Not a take folder, nor a pattern region (which plays its steps, not its notes). */
export function regionFor(regions: Region[], at: number, map: TempoMap): Region | null {
  const midi = regions.filter((r) => r.content.type === "midi" && !r.takes && !r.pattern);
  const covering = midi.filter((r) => at >= r.start && at < r.start + r.length);
  if (covering.length) return covering[covering.length - 1];
  const before = midi.filter((r) => r.start + r.length <= at && at - (r.start + r.length) <= map.samplesPerBarAt(at));
  return before.sort((a, b) => a.start + a.length - (b.start + b.length)).pop() ?? null;
}

/** The bar a new region for a step at `at` covers: [start, length]. */
export function newRegionSpan(map: TempoMap, at: number): [number, number] {
  const bar = map.barAt(at);
  const start = Math.round(map.barStart(bar));
  return [start, Math.round(map.barStart(bar + 1)) - start];
}

const same = (a: { start: number; pitch: number }, b: { start: number; pitch: number }) => Math.round(a.start) === Math.round(b.start) && a.pitch === b.pitch;

/** The `set_midi_content` edits that add `notes` to `region` (growing it to the bar line
 *  past them when they reach beyond it), with the new notes selected. */
export function insertEdits(region: Region, notes: StepNote[], map: TempoMap): { edits: me.ContentEdit[]; selNotes: Set<string> } {
  const w = me.fromRegions([region]);
  const added: me.PNote[] = notes.map((n) => ({ region: region.id, ...n, muted: false, sel: true }));
  const { edits, selNotes } = me.toEdits([...w.notes, ...added], w.cc, [region], barLines(map));
  return { edits, selNotes };
}

/** Edits that remove `notes` (matched by start and pitch, one each) from `regions`. */
export function removeEdits(regions: Region[], notes: StepNote[], map: TempoMap): me.ContentEdit[] {
  const w = me.fromRegions(regions);
  const left = [...notes];
  const kept = w.notes.filter((n) => {
    const i = left.findIndex((x) => same(x, n));
    if (i < 0) return true;
    left.splice(i, 1);
    return false;
  });
  return me.toEdits(kept, w.cc, regions, barLines(map)).edits;
}

/** Edits that give notes (matched by start and pitch) new lengths in `regions`. */
export function resizeEdits(regions: Region[], changes: { note: StepNote; length: number }[], map: TempoMap): me.ContentEdit[] {
  const w = me.fromRegions(regions);
  const left = [...changes];
  const notes = w.notes.map((n) => {
    const i = left.findIndex((x) => same(x.note, n));
    if (i < 0) return n;
    const [c] = left.splice(i, 1);
    return { ...n, length: Math.max(1, c.length) };
  });
  return me.toEdits(notes, w.cc, regions, barLines(map)).edits;
}

// ---- notes played on this computer --------------------------------------------------------

type Listener = (data: [number, number, number]) => void;
const local = new Set<Listener>();

/** Musical Typing reports the notes it plays here, so Step Input can enter them (hardware
 *  inputs arrive with the engine's ticks instead). */
export function emitLocalMidi(data: [number, number, number]) {
  local.forEach((l) => l(data));
}

export function onLocalMidi(l: Listener): () => void {
  local.add(l);
  return () => {
    local.delete(l);
  };
}
