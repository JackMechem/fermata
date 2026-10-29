// The Audio Track editor's range arithmetic, kept free of app imports so tests/ can run it
// under plain Node (audioEdit.ts uses it).
import type { Region, Track } from "./api";

/** Regions of `t` that overlap [a, b). */
export function regionsIn(t: Track, a: number, b: number): Region[] {
  return t.regions.filter((r) => r.start < b && r.start + r.length > a);
}

/** The part of `r` between `a` and `b` as a region of its own (for the clipboard). Fades stay
 *  only on the region's own ends, as the engine's range edits do. */
export function subRegion(r: Region, a: number, b: number): Region {
  const rs = r.start;
  const re = r.start + r.length;
  const s = Math.round(Math.max(a, rs));
  const e = Math.round(Math.min(b, re));
  const length = Math.max(1, e - s);
  const cut = s - rs;
  if (r.content.type !== "audio") {
    const inside = <T extends { start: number }>(xs: T[]) => xs.filter((n) => n.start >= cut && n.start < cut + length).map((n) => ({ ...n, start: n.start - cut }));
    return { ...r, start: s, length, loop_end: null, content: { type: "midi", notes: inside(r.content.notes), cc: inside(r.content.cc ?? []) } };
  }
  const c = r.content;
  const fadeIn = s > rs ? 0 : Math.min(c.fade_in, length);
  const fadeOut = e < re ? 0 : Math.min(c.fade_out, length - fadeIn);
  return { ...r, start: s, length, loop_end: null, content: { ...c, offset: c.offset + cut, fade_in: fadeIn, fade_out: fadeOut } };
}
