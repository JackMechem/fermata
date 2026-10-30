// Region playback parameters (Logic's Region inspector): the values the Inspector offers and
// how it shows them. The engine applies them (crates/engine/src/region_params.rs) and sends
// the notes as they play in the snapshot's `played` list, which the Tracks area draws.
//
// This module only imports types, so it runs under plain Node for the tests in tests/.
import type { Id, Note, Region, RegionParams, Snapshot } from "./api";

export const DEFAULT_PARAMS: RegionParams = { quantize: 0, swing: 50, transpose: 0, velocity: 0, dynamics: 100, gate: 100, delay: 0, delay_ms: false };

export const GATE_FIXED = 0;
export const GATE_LEGATO = 201;
/** Delay limits (as the engine clamps them): four quarters in ticks, two seconds in ms. */
export const DELAY_TICKS_MAX = 3840;
export const DELAY_MS_MAX = 2000;

/** A region's parameters, the defaults filled in. */
export function paramsOf(r: Region): RegionParams {
  return { ...DEFAULT_PARAMS, ...r.params };
}

export function isDefault(p: RegionParams): boolean {
  return (Object.keys(DEFAULT_PARAMS) as (keyof RegionParams)[]).every((k) => p[k] === DEFAULT_PARAMS[k]);
}

/** Quantize choices: straight note values, then triplets, named as Logic names them (1/12 Note
 *  is an eighth-note triplet). */
export const QUANTIZE_OPTIONS: [number, string][] = [
  [0, "Off"],
  ...[1, 2, 4, 8, 16, 32, 64].map((d): [number, string] => [d, `1/${d} Note`]),
  ...[3, 6, 12, 24, 48, 96].map((d): [number, string] => [d, `1/${d} Note`]),
];

/** Dynamics choices: compression ratios, off (1:1), then expansion. */
export const DYNAMICS_OPTIONS: [number, string][] = [
  [0, "Fixed"],
  [300, "3:1"],
  [200, "2:1"],
  [150, "1.5:1"],
  [125, "1.25:1"],
  [100, "1:1"],
  [80, "0.8:1"],
  [67, "0.67:1"],
  [50, "0.5:1"],
  [33, "0.33:1"],
];

/** Gate Time choices: Fixed, 10 % to 200 % in steps of 10, Legato. */
export const GATE_OPTIONS: [number, string][] = [
  [GATE_FIXED, "Fixed"],
  ...Array.from({ length: 20 }, (_, k): [number, string] => [(k + 1) * 10, `${(k + 1) * 10} %`]),
  [GATE_LEGATO, "Legato"],
];

/** The label of a value in a choice list (the nearest one for a value not in it). */
export function optionLabel(options: [number, string][], v: number): string {
  let best = options[0];
  for (const o of options) if (Math.abs(o[0] - v) < Math.abs(best[0] - v)) best = o;
  return best[1];
}

/** The value all `regions` share for a parameter, or null when they differ (shown as "*",
 *  as Logic's Inspector does for a mixed selection). */
export function shared<K extends keyof RegionParams>(regions: Region[], key: K): RegionParams[K] | null {
  if (!regions.length) return null;
  const v = paramsOf(regions[0])[key];
  return regions.every((r) => paramsOf(r)[key] === v) ? v : null;
}

// The played notes, looked up by region; one map per snapshot list.
const playedCache = new WeakMap<NonNullable<Snapshot["played"]>, Map<Id, Note[]>>();

/** The notes region `id` plays (with its parameters), or undefined when it has none set. */
export function playedNotes(played: Snapshot["played"], id: Id): Note[] | undefined {
  if (!played?.length) return undefined;
  let m = playedCache.get(played);
  if (!m) {
    m = new Map(played.map((p) => [p.region, p.notes]));
    playedCache.set(played, m);
  }
  return m.get(id);
}
