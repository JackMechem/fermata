// Automation lanes beyond one per track, and automation curves: the pure parts behind
// automation.ts. A track shows its main lane over its regions and, below it, the extra lanes
// in `auto_lanes` (Logic's sub-lanes), each with its own parameter. Stereo Out's lanes are
// shown on the Output track at the bottom of the Tracks area. A point's `curve` bends the
// segment after it, as the engine plays it (crates/engine/src/automation.rs).

import type { AutoLane, AutoParam, AutoPoint, Track } from "./api";

/** Stereo Out's id (`MASTER_ID` in api.ts), which the Output track stands for. */
export const OUTPUT_ID = 0;

/** A sub-lane is as tall as its track, but never too thin to edit. */
export const MIN_SUB_LANE = 36;

export function subLaneHeight(trackHeight: number): number {
  return Math.max(MIN_SUB_LANE, trackHeight);
}

export function sameParam(a: AutoParam, b: AutoParam): boolean {
  if (a.type === "send" && b.type === "send") return a.value === b.value;
  if (a.type === "plugin" && b.type === "plugin") return a.value.uid === b.value.uid && a.value.param === b.value.param;
  return a.type === b.type;
}

// ---- curves -------------------------------------------------------------------------------------

/** At curve ±1 a segment follows t^(1/8) or t^8 (the engine's `CURVE_RANGE`). */
const CURVE_RANGE = 3;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** How far along a segment's change the value is at `t` (0..1 of the way in time): a power
 *  curve, exactly 0 at the start and 1 at the end. Positive curves make the change early. */
export function shape(t: number, curve: number | undefined): number {
  t = clamp(t, 0, 1);
  if (!curve) return t;
  return Math.pow(t, Math.pow(2, -CURVE_RANGE * clamp(curve, -1, 1)));
}

/** The lane's value at `pos`, as the engine plays it: curved lines between points, held before
 *  the first and after the last. Null for an empty lane. */
export function valueAt(points: AutoPoint[], pos: number): number | null {
  if (!points.length) return null;
  if (pos < points[0].pos) return points[0].value;
  let i = 0;
  while (i < points.length && points[i].pos <= pos) i++;
  if (i >= points.length) return points[points.length - 1].value;
  const a = points[i - 1];
  const b = points[i];
  return a.value + (b.value - a.value) * shape((pos - a.pos) / (b.pos - a.pos), a.curve);
}

/** The curve that takes a segment through `f` (0..1 of the way from its first value to its
 *  second) at `t` (0..1 of the way in time): what a bend drag sets, so the line follows the
 *  pointer. */
export function curveThrough(t: number, f: number): number {
  t = clamp(t, 0.05, 0.95);
  f = clamp(f, 0.001, 0.999);
  const e = Math.log(f) / Math.log(t);
  const c = clamp(-Math.log2(e) / CURVE_RANGE, -1, 1);
  // Close to straight snaps to straight, so a segment can be bent back exactly.
  return Math.abs(c) < 0.03 ? 0 : Math.round(c * 1000) / 1000;
}

/** The segment of `points` (sorted) that `pos` falls in: the index of its first point, or -1
 *  outside the points or on a vertical step. */
export function segmentAt(points: AutoPoint[], pos: number): number {
  for (let i = 0; i + 1 < points.length; i++) {
    if (pos >= points[i].pos && pos < points[i + 1].pos) return i;
  }
  return -1;
}

/** `points` with segment `i` given `curve` (0 makes it straight again). */
export function withCurve(points: AutoPoint[], i: number, curve: number): AutoPoint[] {
  return points.map((p, k) => {
    if (k !== i) return p;
    const { curve: _old, ...rest } = p;
    return curve ? { ...rest, curve } : rest;
  });
}

/** Screen positions along the segment after `a` for drawing it, with how far along its change
 *  each is: a straight segment needs only its end, a curved one is sampled every few pixels.
 *  `x0`/`x1` are the ends' x. */
export function segmentSamples(a: AutoPoint, x0: number, x1: number): { x: number; f: number }[] {
  if (!a.curve || x1 - x0 < 2) return [{ x: x1, f: 1 }];
  const n = Math.min(256, Math.max(4, Math.ceil((x1 - x0) / 3)));
  const out: { x: number; f: number }[] = [];
  for (let k = 1; k <= n; k++) {
    const t = k / n;
    out.push({ x: x0 + (x1 - x0) * t, f: shape(t, a.curve) });
  }
  return out;
}

// ---- several lanes per track ------------------------------------------------------------------

/** The lanes holding points. */
export function usedParams(lanes: AutoLane[] | undefined): AutoParam[] {
  return (lanes ?? []).filter((l) => l.points.length).map((l) => l.param);
}

/** Show Used Automation for one track: the main lane keeps its parameter if that has points
 *  (or nothing does), else takes the first lane that has; every other lane with points gets a
 *  sub-lane, in the track's lane order. */
export function usedLayout(lanes: AutoLane[] | undefined, main: AutoParam): { main: AutoParam; subs: AutoParam[] } {
  const used = usedParams(lanes);
  const m = used.length && !used.some((p) => sameParam(p, main)) ? used[0] : main;
  return { main: m, subs: used.filter((p) => !sameParam(p, m)) };
}

/** The parameter a new sub-lane shows: the first lane with points that isn't shown yet, else
 *  the first of `choices` that isn't. Null when everything is shown. */
export function nextParam(shown: AutoParam[], lanes: AutoLane[] | undefined, choices: AutoParam[]): AutoParam | null {
  const free = (p: AutoParam) => !shown.some((s) => sameParam(s, p));
  return usedParams(lanes).find(free) ?? choices.find(free) ?? null;
}

/** One track's row in the Tracks area, as the automation view sees it. */
export interface LaneRow {
  track: Track;
  /** Content y of the track and the height of its main lane (above any take rows). */
  top: number;
  main: number;
  /** Where its sub-lanes start, and each one's height. */
  subsTop: number;
  sub: number;
}

/** A lane on screen: which strip and parameter, and where. `sub` is the sub-lane's index in
 *  `auto_lanes`, or -1 for the main lane. */
export interface LaneBox {
  track: Track;
  param: AutoParam;
  top: number;
  height: number;
  sub: number;
}

/** Every lane the rows show, top to bottom: each main lane, then its sub-lanes. */
export function laneBoxes(rows: LaneRow[], mainParam: (t: Track) => AutoParam): LaneBox[] {
  const out: LaneBox[] = [];
  for (const r of rows) {
    out.push({ track: r.track, param: mainParam(r.track), top: r.top, height: r.main, sub: -1 });
    (r.track.auto_lanes ?? []).forEach((param, k) => out.push({ track: r.track, param, top: r.subsTop + k * r.sub, height: r.sub, sub: k }));
  }
  return out;
}

/** The lane at content y, if any. */
export function boxAt(boxes: LaneBox[], y: number): LaneBox | null {
  return boxes.find((b) => y >= b.top && y < b.top + b.height) ?? null;
}
