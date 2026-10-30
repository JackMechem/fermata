// The Tracks area's tools, as in Logic: a Left-click Tool and a Command-click Tool (Ctrl-click
// outside macOS), chosen from the two pop-ups in the Tracks area's bar or from the Tool menu
// (T, then the tool's key). This is the tool list and the geometry behind the Scissors and
// Fade tools; trackTools.ts does the clicking. Pure, so tests can import it.
import type { Id } from "./api";

export type TrackTool = "pointer" | "pencil" | "eraser" | "scissors" | "glue" | "mute" | "fade" | "marquee";

/** In the order of Logic's Tool menu, with the key that picks each while the menu is open. */
export const TRACK_TOOLS: { id: TrackTool; label: string; key: string }[] = [
  { id: "pointer", label: "Pointer Tool", key: "T" },
  { id: "pencil", label: "Pencil Tool", key: "P" },
  { id: "eraser", label: "Eraser Tool", key: "E" },
  { id: "scissors", label: "Scissors Tool", key: "I" },
  { id: "glue", label: "Glue Tool", key: "G" },
  { id: "mute", label: "Mute Tool", key: "M" },
  { id: "fade", label: "Fade Tool", key: "A" },
  { id: "marquee", label: "Marquee Tool", key: "R" },
];

/** The two tools; Logic's defaults are the Pointer and the Marquee. */
export interface Tools {
  left: TrackTool;
  cmd: TrackTool;
}

export const DEFAULT_TOOLS: Tools = { left: "pointer", cmd: "marquee" };

/** The tool a key picks while the Tool menu is open (`KeyboardEvent.key`): its letter, or Esc
 *  for the Pointer. Null for any other key. */
export function toolForKey(key: string): TrackTool | null {
  if (key === "Escape") return "pointer";
  return TRACK_TOOLS.find((t) => t.key === key.toUpperCase())?.id ?? null;
}

/** The tool a click uses: the Command-click tool while ⌘ (Ctrl) is held. */
export function toolFor(tools: Tools, mod: boolean): TrackTool {
  return mod ? tools.cmd : tools.left;
}

/** Most pieces an Alt-click with the Scissors makes, so a click at a region's very start
 *  can't cut it into millions. */
export const MAX_PIECES = 512;

/**
 * Where an Alt-click with the Scissors at `at` cuts a region from `start` to `end`: into pieces
 * as long as the part before the click (Logic's "split into equal parts"). Empty when the click
 * isn't inside the region.
 */
export function equalPieces(start: number, end: number, at: number): number[] {
  const step = at - start;
  if (step <= 0 || at >= end) return [];
  const out: number[] = [];
  for (let p = at; p < end && out.length < MAX_PIECES - 1; p += step) out.push(Math.round(p));
  return out;
}

/** A region as the Fade tool sees it. */
export interface FadeRegion {
  id: Id;
  start: number;
  length: number;
  fadeIn: number;
  fadeOut: number;
}

/** What a Fade tool drag does: a fade-in or fade-out of `length` on region `id`, or a crossfade
 *  of `length` from region `id` into the one that follows it. */
export type FadeEdit = { kind: "in" | "out" | "xfade"; id: Id; length: number };

/**
 * The Fade tool, dragged over region `r` from `from` to `to` (timeline frames, unsnapped as
 * fades are). `prev` is the region that crosses into `r` and `next` the one `r` crosses into
 * (adjacent or overlapping, see `followingRegion`), if any.
 *
 * A drag across the boundary with either makes a crossfade as long as the drag. Otherwise the
 * drag sets the fade on the nearer end of `r`, from that end to the far end of the drag, so
 * dragging over an existing fade adjusts it. A drag of no length gives null.
 */
export function fadeToolEdit(r: FadeRegion, prev: FadeRegion | undefined, next: FadeRegion | undefined, from: number, to: number): FadeEdit | null {
  const lo = Math.min(from, to);
  const hi = Math.max(from, to);
  if (hi <= lo) return null;
  const end = r.start + r.length;
  // Where two regions meet (the middle of an overlap), which a crossfade is centred on.
  const across = (a: FadeRegion, b: FadeRegion) => {
    const pivot = (a.start + a.length + b.start) / 2;
    return lo <= pivot && hi >= pivot;
  };
  if (next && across(r, next)) return { kind: "xfade", id: r.id, length: Math.round(hi - lo) };
  if (prev && across(prev, r)) return { kind: "xfade", id: prev.id, length: Math.round(hi - lo) };
  const clamp = (v: number, max: number) => Math.round(Math.max(0, Math.min(max, v)));
  if ((lo + hi) / 2 < r.start + r.length / 2) return { kind: "in", id: r.id, length: clamp(hi - r.start, r.length - r.fadeOut) };
  return { kind: "out", id: r.id, length: clamp(end - lo, r.length - r.fadeIn) };
}

/** Which end of a region an Alt-click with the Fade tool clears: the nearer one. */
export function nearerEnd(start: number, length: number, at: number): "in" | "out" {
  return at < start + length / 2 ? "in" : "out";
}
