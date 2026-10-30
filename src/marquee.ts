// The Tracks area's marquee selection (Logic's Marquee tool): a time range across a run of
// tracks. This is its geometry; trackTools.ts draws it and the session cuts regions with it
// (session/marquee.rs). Pure, so tests can import it.
import type { Id } from "./api";

export interface Marquee {
  /** Timeline frames, start < end. */
  start: number;
  end: number;
  /** The marked tracks, in track order. */
  tracks: Id[];
}

/** The tracks from index `i0` to `i1` (either way round, both included) that show a lane:
 *  hidden tracks and the subtracks of a closed stack (lane height 0) are left out. */
export function tracksBetween(ids: Id[], laneH: number[], i0: number, i1: number): Id[] {
  const lo = Math.max(0, Math.min(i0, i1));
  const hi = Math.min(ids.length - 1, Math.max(i0, i1));
  const out: Id[] = [];
  for (let i = lo; i <= hi; i++) if (laneH[i] > 0) out.push(ids[i]);
  return out;
}

/**
 * The marquee a drag makes, from position `a` on the track at index `i0` to `b` on `i1`
 * (positions already snapped). With `base` (a Shift-drag), the marquee grows to take in the
 * old one too. Null for a drag with no length and no tracks.
 */
export function dragMarquee(ids: Id[], laneH: number[], a: number, i0: number, b: number, i1: number, base?: Marquee | null): Marquee | null {
  let start = Math.min(a, b);
  let end = Math.max(a, b);
  let lo = Math.min(i0, i1);
  let hi = Math.max(i0, i1);
  if (base) {
    start = Math.min(start, base.start);
    end = Math.max(end, base.end);
    for (const t of base.tracks) {
      const i = ids.indexOf(t);
      if (i < 0) continue;
      lo = Math.min(lo, i);
      hi = Math.max(hi, i);
    }
  }
  const tracks = tracksBetween(ids, laneH, lo, hi);
  if (end <= start || !tracks.length) return null;
  return { start: Math.max(0, Math.round(start)), end: Math.round(end), tracks };
}

/** Whether a point (a track and a timeline position) is inside the marquee. */
export function inMarquee(m: Marquee | null | undefined, track: Id | undefined, pos: number): boolean {
  return !!m && track !== undefined && m.tracks.includes(track) && pos >= m.start && pos <= m.end;
}

/** The top and bottom of the marked lanes, in content pixels (`tops` and `laneH` by track
 *  index, as in the layout). Null when none of them is in view. */
export function marqueeRows(m: Marquee, ids: Id[], tops: number[], laneH: number[]): { top: number; bottom: number } | null {
  let top = Infinity;
  let bottom = -Infinity;
  ids.forEach((id, i) => {
    if (!m.tracks.includes(id) || laneH[i] <= 0) return;
    top = Math.min(top, tops[i]);
    bottom = Math.max(bottom, tops[i] + laneH[i]);
  });
  return top < bottom ? { top, bottom } : null;
}
