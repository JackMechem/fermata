// Region gain dragged in the Tracks area: a small handle at the top centre of an audio
// region's name bar, shown on hover, that drags the gain up and down (Shift for fine steps)
// and resets to 0 dB on a double-click. Every selected audio region moves by the same number
// of dB from where it was. This is the geometry and the arithmetic; regionGainDrag.ts does the
// dragging. Pure, so tests can import it.
import type { Id } from "./api";

/** Region gain's range (crates/engine/src/session/region_gain.rs). The bottom shows as −∞. */
export const GAIN_MIN = -96;
export const GAIN_MAX = 30;
/** dB per pixel of vertical drag, and with Shift. */
export const GAIN_PER_PX = 0.25;
export const FINE_GAIN_PER_PX = 0.05;
/** The handle's width, and the narrowest region that shows one. */
export const HANDLE_W = 52;
export const MIN_REGION_W = HANDLE_W + 40;

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Where the handle is on a region drawn at `x`..`x + w` with its name bar `barH` tall from
 *  `y`: centred over the part of the region in view (`view0`..`view1`), so a long region's
 *  handle stays on screen. Null when the region is too narrow (in view) to carry one. */
export function handleRect(x: number, y: number, w: number, barH: number, view0: number, view1: number): Rect | null {
  const a = Math.max(x, view0);
  const b = Math.min(x + w, view1);
  if (b - a < MIN_REGION_W) return null;
  const h = Math.max(8, barH - 2);
  return { x: Math.round((a + b) / 2 - HANDLE_W / 2), y: y + 1, w: HANDLE_W, h };
}

export function inRect(r: Rect | null, px: number, py: number): boolean {
  return !!r && px >= r.x && px <= r.x + r.w && py >= r.y && py <= r.y + r.h;
}

export function clampGain(db: number): number {
  return Math.min(GAIN_MAX, Math.max(GAIN_MIN, db));
}

/** The drag's change in dB after the pointer moved `dyPx` (down is positive) from where it
 *  last was, starting from `delta`. Kept within the range a region could still use. */
export function dragDelta(delta: number, dyPx: number, fine: boolean): number {
  const span = GAIN_MAX - GAIN_MIN;
  return Math.min(span, Math.max(-span, delta - dyPx * (fine ? FINE_GAIN_PER_PX : GAIN_PER_PX)));
}

/** Each region's gain with `delta` dB added to where it started, clamped and rounded to a
 *  tenth of a dB, as the engine stores it. */
export function shiftedGains(from: [Id, number][], delta: number): [Id, number][] {
  return from.map(([id, db]) => [id, Math.round(clampGain(db + delta) * 10) / 10]);
}

/** The handle's text: "+1.5 dB", "0.0 dB", "−∞ dB". */
export function gainLabel(db: number): string {
  if (db <= GAIN_MIN) return "−∞ dB";
  const r = Math.round(db * 10) / 10;
  return `${r > 0 ? "+" : r < 0 ? "−" : ""}${Math.abs(r).toFixed(1)} dB`;
}

/** Linear amplitude of a gain in dB, for scaling the waveform (silence at the bottom). */
export function gainScale(db: number): number {
  return db <= GAIN_MIN ? 0 : 10 ** (db / 20);
}
