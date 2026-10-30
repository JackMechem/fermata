// Moving regions in bars and beats across tempo changes. A drag or nudge takes one region
// (the grabbed one, or the first) from `from` to `to`; with snapping on, every other region
// moves by the same distance in quarter notes through the tempo map rather than the same
// number of samples, so a region two bars on from the grabbed one is still two bars on after
// crossing from 120 to 90 BPM. The engine does the same for nudges
// (crates/engine/src/session/musical_moves.rs).
//
// This module only imports types, so it runs under plain Node for the tests in tests/.
import type { TempoMap } from "./tempo";
import type { Nudge } from "./prefs";

/** Where a region at `pos` goes when a move takes `from` to `to`: the same distance in quarter
 *  notes when `musical`, otherwise in samples. Never before 0. */
export function movedStart(map: TempoMap, pos: number, from: number, to: number, musical: boolean): number {
  if (pos === from) return Math.max(0, Math.round(to));
  if (!musical) return Math.max(0, Math.round(pos + (to - from)));
  const dq = map.beatsAt(to) - map.beatsAt(from);
  return Math.max(0, Math.round(map.sampleAt(map.beatsAt(pos) + dq)));
}

/** Where a nudge by `step` in direction `dir` (±1) takes a region at `at`, through the tempo
 *  map: a bar keeps the position within the bar, beats and note values count in quarter notes.
 *  Null for the steps in milliseconds, which stay a number of samples. */
export function nudgeTarget(map: TempoMap, at: number, step: Nudge, dir: number): number | null {
  const q = map.beatsAt(at);
  let to: number;
  if (step === "bar") {
    const bar = map.barAt(at);
    to = map.sampleAt(map.barStartQ(bar + dir) + (q - map.barStartQ(bar)));
  } else if (step === "beat") {
    to = map.sampleAt(q + (dir * 4) / map.sigAt(at)[1]);
  } else if (step === "8" || step === "16" || step === "32") {
    to = map.sampleAt(q + (dir * 4) / Number(step));
  } else return null;
  return Math.max(0, Math.round(to));
}

/** `r` moved to `start`, its loop end with it (for a copy that is pasted where it is). */
export function placedAt<R extends { start: number; loop_end?: number | null }>(r: R, start: number): R {
  return { ...r, start, loop_end: r.loop_end == null ? r.loop_end : r.loop_end + (start - r.start) };
}
