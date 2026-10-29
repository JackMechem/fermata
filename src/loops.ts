// Looping regions, as in Logic: a region with a loop end repeats its own window from its end up
// to the loop end, the last repetition cut short there.
import type { Region } from "./api";

/** Most repetitions a loop draws or plays (matches the engine's MAX_LOOP_PASSES). */
const MAX_PASSES = 4096;

/** Where a region stops sounding: its loop end, or its own end. */
export function regionEnd(r: Pick<Region, "start" | "length" | "loop_end">): number {
  return loopEnd(r.start, r.length, r.loop_end);
}

/** The loop end that applies to a region at `start` with `length` (its own end when not looping). */
export function loopEnd(start: number, length: number, loop?: number | null): number {
  const own = start + length;
  if (loop == null || length <= 0) return own;
  return Math.min(Math.max(loop, own), start + length * MAX_PASSES);
}

export const isLooped = (r: Pick<Region, "start" | "length" | "loop_end">) => regionEnd(r) > r.start + r.length;

/** Timeline [start, length] of each loop repetition after the region body. */
export function loopPasses(start: number, length: number, loop?: number | null): [number, number][] {
  const end = loopEnd(start, length, loop);
  const out: [number, number][] = [];
  for (let at = start + length; at < end; at += length) out.push([at, Math.min(length, end - at)]);
  return out;
}
