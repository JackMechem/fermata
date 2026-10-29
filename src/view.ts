// Scroll position of the tracks area, shared by the ruler, headers and arrange canvas without
// going through React state on every scroll event.

export interface Viewport {
  left: number;
  top: number;
  width: number;
  height: number;
}

let vp: Viewport = { left: 0, top: 0, width: 800, height: 400 };
const subs = new Set<(v: Viewport) => void>();

export function viewport(): Viewport {
  return vp;
}

export function setViewport(p: Partial<Viewport>) {
  vp = { ...vp, ...p };
  subs.forEach((s) => s(vp));
}

export function onViewport(f: (v: Viewport) => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}

/** Set by the arrange view so other components can scroll it. */
export let scrollTo: (left: number, top?: number) => void = () => {};
export function registerScroller(f: (left: number, top?: number) => void) {
  scrollTo = f;
}

/** Cycle strip, bar numbers, Autopunch strip and marker lane (see Ruler.tsx). */
export const RULER_H = 52;
export const HEADER_W = 296;

export const TRACK_COLORS = [
  "#e5483a", "#ef7d2f", "#f2a63b", "#f2c230", "#c9cf3a", "#8cc63f", "#4fae4c", "#3aa776",
  "#35a8a0", "#37a6c9", "#3b8fd9", "#4f6fe0", "#6a5be0", "#8a54d6", "#b14fc8", "#d24c9d",
  "#d9546e", "#a0745a", "#8e8e93", "#6b7a8f",
];
