// Controller curves in the Piano Roll's controller lane (Logic's MIDI Draw lines with a curve
// shape). MIDI controller events are discrete, so a curve is drawn as a ramp of events along
// it, as dense as the Ramp Density setting asks:
//   - the Line tool (and Mod+Shift-drag) draws a ramp in the chosen Line Curve shape; an end
//     that starts or stops on an existing event takes that event's time and value, so a line
//     drawn between two events joins them;
//   - Create Ramp fills the gap between each pair of neighbouring selected events;
//   - Thin Out removes the selected events that the line between their neighbours predicts.
//
// This module only imports types and midiEdit.ts, so it runs under plain Node for the tests.
import type { CcKind, Id } from "./api";
import { ccMax, clamp, sameKind, type PCc } from "./midiEdit.ts";

export type CurveShape = "linear" | "exponential" | "logarithmic" | "s";

export const CURVE_SHAPES: [CurveShape, string][] = [
  ["linear", "Linear"],
  ["exponential", "Exponential"],
  ["logarithmic", "Logarithmic"],
  ["s", "S-Curve"],
];

/** Ramp densities: one event per this note value. */
export const DENSITIES = ["1/8", "1/16", "1/32", "1/64"] as const;
export type Density = (typeof DENSITIES)[number];

/** How sharply the exponential and logarithmic shapes bend. */
const BEND = 3;

/** How far along the change a curve is (0 to 1) a fraction `u` of the way along in time. */
export function curve(shape: CurveShape, u: number): number {
  const t = clamp(u, 0, 1);
  switch (shape) {
    case "exponential":
      return (Math.exp(BEND * t) - 1) / (Math.exp(BEND) - 1);
    case "logarithmic":
      return 1 - curve("exponential", 1 - t);
    case "s":
      return (1 - Math.cos(Math.PI * t)) / 2;
    default:
      return t;
  }
}

/** Samples between ramp events at a density, given a quarter note's length in samples. */
export function densityStep(quarter: number, d: Density): number {
  return (quarter * 4) / Number(d.slice(2));
}

/** The events of a ramp from (`t0`, `v0`) to (`t1`, `v1`) along `shape`, one per `step`
 *  samples, values rounded into 0..`max`. A value that doesn't change from the event before
 *  isn't repeated; the end always has an event. */
export function rampPoints(t0: number, v0: number, t1: number, v1: number, shape: CurveShape, step: number, max: number): { start: number; value: number }[] {
  const [a, b, va, vb] = t0 <= t1 ? [t0, t1, v0, v1] : [t1, t0, v1, v0];
  const n = Math.max(1, Math.round((b - a) / Math.max(1, step)));
  const out: { start: number; value: number }[] = [];
  let last = -1;
  for (let i = 0; i <= n; i++) {
    const u = i / n;
    const value = clamp(Math.round(va + (vb - va) * curve(shape, u)), 0, max);
    if (value === last && i !== n) continue;
    last = value;
    out.push({ start: Math.round(a + (b - a) * u), value });
  }
  return out;
}

/** Draws a curve of `kind` events into `region` from (`t0`, `v0`) to (`t1`, `v1`): the events
 *  of that kind between them in that region make way for it. */
export function drawCcCurve(cc: PCc[], region: Id, kind: CcKind, t0: number, v0: number, t1: number, v1: number, step: number, shape: CurveShape): PCc[] {
  const [a, b] = t0 <= t1 ? [t0, t1] : [t1, t0];
  const kept = cc.filter((c) => !(c.region === region && sameKind(c.kind, kind) && c.start >= a && c.start <= b));
  const pts = rampPoints(t0, v0, t1, v1, shape, step, ccMax(kind)).map((p) => ({ region, kind, sel: false, ...p }));
  return [...kept, ...pts].sort((x, y) => x.start - y.start);
}

/** The `kind` event nearest the point (`x`, `y`) within `radius` pixels, with `xOf` and `yOf`
 *  placing events on screen; null when none is that close. */
export function nearestEvent(cc: PCc[], kind: CcKind, x: number, y: number, xOf: (t: number) => number, yOf: (v: number) => number, radius = 7): PCc | null {
  let best: PCc | null = null;
  let d = radius;
  for (const c of cc) {
    if (!sameKind(c.kind, kind)) continue;
    const e = Math.hypot(xOf(c.start) - x, yOf(c.value) - y);
    if (e <= d) [best, d] = [c, e];
  }
  return best;
}

/** The selected `kind` events in time order, by region. */
function selectedRuns(cc: PCc[], kind: CcKind): Map<Id, PCc[]> {
  const runs = new Map<Id, PCc[]>();
  for (const c of cc) {
    if (!c.sel || !sameKind(c.kind, kind)) continue;
    const r = runs.get(c.region) ?? [];
    r.push(c);
    runs.set(c.region, r);
  }
  for (const r of runs.values()) r.sort((a, b) => a.start - b.start);
  return runs;
}

/** Whether Create Ramp has something to do: two selected `kind` events in one region. */
export function canRamp(cc: PCc[], kind: CcKind): boolean {
  return [...selectedRuns(cc, kind).values()].some((r) => r.length >= 2);
}

/** Create Ramp: between each pair of neighbouring selected `kind` events in a region, the
 *  events in between are replaced by a ramp from one to the other along `shape`. The selected
 *  events stay as they are, and selected. */
export function createRamps(cc: PCc[], kind: CcKind, shape: CurveShape, step: number): PCc[] {
  let out = cc;
  for (const [region, run] of selectedRuns(cc, kind)) {
    for (let i = 0; i + 1 < run.length; i++) {
      const [a, b] = [run[i], run[i + 1]];
      if (b.start - a.start < 2) continue;
      const inner = rampPoints(a.start, a.value, b.start, b.value, shape, step, ccMax(kind)).filter((p) => p.start > a.start && p.start < b.start);
      out = out.filter((c) => !(c.region === region && sameKind(c.kind, kind) && c.start > a.start && c.start < b.start));
      out = [...out, ...inner.map((p) => ({ region, kind, sel: false, ...p }))];
    }
  }
  return out === cc ? cc : [...out].sort((x, y) => x.start - y.start);
}

/** Thin Out: drops the selected `kind` events (all of them when none is selected) that a
 *  straight line between the events kept either side predicts to within `tolerance` values
 *  (Ramer–Douglas–Peucker over each region's run). Each run keeps its first and last event. */
export function thinOut(cc: PCc[], kind: CcKind, tolerance: number): PCc[] {
  const any = cc.some((c) => c.sel && sameKind(c.kind, kind));
  const pick = any ? cc : cc.map((c) => (sameKind(c.kind, kind) ? { ...c, sel: true } : c));
  const drop = new Set<PCc>();
  for (const run of selectedRuns(pick, kind).values()) {
    const keep = new Array<boolean>(run.length).fill(false);
    const rdp = (i: number, j: number) => {
      keep[i] = keep[j] = true;
      let worst = -1;
      let at = -1;
      for (let k = i + 1; k < j; k++) {
        const u = (run[k].start - run[i].start) / Math.max(1, run[j].start - run[i].start);
        const e = Math.abs(run[i].value + (run[j].value - run[i].value) * u - run[k].value);
        if (e > worst) [worst, at] = [e, k];
      }
      if (at >= 0 && worst > tolerance) {
        rdp(i, at);
        rdp(at, j);
      }
    };
    if (run.length > 2) rdp(0, run.length - 1);
    else keep.fill(true);
    run.forEach((c, k) => !keep[k] && drop.add(c));
  }
  if (!drop.size) return cc;
  // `pick` holds the same events as `cc`, in the same order.
  return cc.filter((_, i) => !drop.has(pick[i]));
}
