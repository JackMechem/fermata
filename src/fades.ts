// Fade shapes and crossfades on audio regions, mirroring crates/engine/src/fades.rs: the curve
// maths (for drawing), the types the Region inspector offers, and where each region's fade
// handles are once crossfades are taken into account. Pure, so tests can import it.
import type { Id, Region, RegionContent } from "./api";

export type FadeInType = "fade" | "speed_up";
export type FadeOutType = "out" | "x" | "eqp" | "xs" | "slow_down";
export type DragMode = "overlap" | "xfade" | "no_overlap" | "shuffle_r" | "shuffle_l";

/** fades::FadeShape. Curves run from -1 to 1 (the inspector shows -100 to 100). */
export interface FadeShape {
  in_curve: number;
  out_curve: number;
  in_type: FadeInType;
  out_type: FadeOutType;
}

/** fades::Crossfade: timeline (start, length) of each side's fade. */
export interface Crossfade {
  out_region: Id;
  in_region: Id;
  kind: FadeOutType;
  curve: number;
  fade_out: [number, number];
  fade_in: [number, number];
}

/** The quarter sine fades had before shapes could be chosen, still the default. */
export const DEFAULT_CURVE = 0.5;
export const DEFAULT_SHAPE: FadeShape = { in_curve: DEFAULT_CURVE, out_curve: DEFAULT_CURVE, in_type: "fade", out_type: "out" };

/** Logic's fade-out Type pop-up. */
export const FADE_OUT_TYPES: [FadeOutType, string][] = [
  ["out", "Out"],
  ["x", "X"],
  ["eqp", "EqP"],
  ["xs", "X S"],
];

export const isCrossfadeType = (t: FadeOutType) => t === "x" || t === "eqp" || t === "xs";

/** A region's shape, with the defaults filled in (an unchanged shape isn't saved). */
export function shapeOf(c: RegionContent): FadeShape {
  return c.type === "audio" ? { ...DEFAULT_SHAPE, ...c.fade_shape } : DEFAULT_SHAPE;
}

/** Fade gain at `x` (0 silent, 1 full) for curve `c`, as fades::curve computes it. */
export function fadeCurve(x: number, c: number): number {
  x = Math.max(0, Math.min(1, x));
  c = Math.max(-1, Math.min(1, c));
  if (c < 0) return 1 - fadeCurve(1 - x, -c);
  if (c < 1e-4) return x;
  if (c <= 0.5) {
    const th = c * Math.PI;
    return Math.sin(x * th) / Math.sin(th);
  }
  const s = Math.sin(x * Math.PI * 0.5);
  return 1 - Math.pow(1 - s, 1 + 6 * (c - 0.5));
}

/** The S-curve crossfade's raised cosine. */
export const sCurve = (x: number) => 0.5 - 0.5 * Math.cos(Math.max(0, Math.min(1, x)) * Math.PI);

/** A gain law: `x` from 0 (silent) to 1 (full level). */
export type Law = (x: number) => number;

/** The outgoing and incoming sides' gain laws (fades::Crossfade::laws). */
export function crossfadeLaws(x: Pick<Crossfade, "kind" | "curve">): [Law, Law] {
  if (x.kind === "eqp") return [(v) => fadeCurve(v, DEFAULT_CURVE), (v) => fadeCurve(v, DEFAULT_CURVE)];
  if (x.kind === "xs") return [sCurve, sCurve];
  return [(v) => fadeCurve(v, x.curve), (v) => 1 - fadeCurve(1 - v, x.curve)];
}

/** Curve value as the inspector shows it, and back. */
export const curveToUi = (c: number) => Math.round(c * 100);
export const uiToCurve = (v: number) => Math.max(-1, Math.min(1, v / 100));

/** The crossfades a region takes part in. */
export interface RegionCrossfades {
  /** Into the following region (this region's fade-out). */
  out?: Crossfade;
  /** From the previous region (replacing this region's fade-in). */
  in?: Crossfade;
}

let indexed: { list: Crossfade[] | undefined; map: Map<Id, RegionCrossfades> } = { list: undefined, map: new Map() };

/** Crossfades by region, rebuilt only when the list changes (once per snapshot). */
export function crossfadesByRegion(list: Crossfade[] | undefined): Map<Id, RegionCrossfades> {
  if (indexed.list === list) return indexed.map;
  const map = new Map<Id, RegionCrossfades>();
  for (const x of list ?? []) {
    map.set(x.out_region, { ...map.get(x.out_region), out: x });
    map.set(x.in_region, { ...map.get(x.in_region), in: x });
  }
  indexed = { list, map };
  return map;
}

/** A crossfade redrawn at a new length while its handle is dragged: it grows or shrinks
 *  about its centre (the engine settles where it really fits on release). */
export function resized(x: Crossfade, length: number): Crossfade {
  const mid = x.fade_out[0] + x.fade_out[1] / 2;
  const butt = x.fade_out[0] !== x.fade_in[0];
  const half = Math.max(0, Math.round(length / 2));
  if (butt) return { ...x, fade_out: [x.fade_in[0] - half, half], fade_in: [x.fade_in[0], half] };
  return { ...x, fade_out: [Math.round(mid - half), 2 * half], fade_in: [Math.round(mid - half), 2 * half] };
}

/** Where a region's fade handles sit, in frames from its start: the fade-in's end (null when a
 *  crossfade replaces the fade-in) and the fade-out's start (a crossfade's start). */
export function fadeHandles(start: number, length: number, fadeIn: number, fadeOut: number, x: RegionCrossfades | undefined): { fadeIn: number | null; fadeOut: number } {
  return {
    fadeIn: x?.in ? null : fadeIn,
    fadeOut: x?.out ? x.out.fade_out[0] - start : length - fadeOut,
  };
}

/** The fade-out length a handle drag sets: a crossfade's handle moves one edge of a fade that
 *  grows about its centre, so it changes twice as fast. */
export function draggedFadeOut(from: number, dx: number, crossfade: boolean, max: number): number {
  return Math.round(Math.max(0, Math.min(max, from - (crossfade ? 2 : 1) * dx)));
}

/** Whether a region can crossfade: plain audio (not a take folder or a loop) that is heard. */
function crossfadeable(r: Region): boolean {
  return r.content.type === "audio" && !r.takes && !r.muted && !(r.loop_end != null && r.loop_end > r.start + r.length) && r.length > 0;
}

/** The region a crossfade from `a` would go into (fades::following): the next one on the
 *  track to start inside `a` or right at its end and run on past it. */
export function followingRegion(regions: Region[], a: Region): Region | undefined {
  const end = a.start + a.length;
  let best: Region | undefined;
  for (const b of regions) {
    if (b.id === a.id || !crossfadeable(b) || b.start <= a.start || b.start > end || b.start + b.length <= end) continue;
    if (!best || b.start < best.start || (b.start === best.start && b.id < best.id)) best = b;
  }
  return best;
}
