// The Tracks area's overview strip (like Ardour's Summary): the mapping between the whole
// project, drawn small across the strip, and the Tracks area's scroll position and zoom.
// Pure, so it can be unit tested; Overview.tsx does the drawing and the mouse handling.
//
// Coordinates: time is in samples; "strip" x/y are CSS pixels in the overview canvas; "view"
// left/top/width/height are CSS pixels of the Tracks area's scrolling content, as in view.ts.

/** Height of the strip in CSS pixels, under its 1px top border. */
export const OVERVIEW_H = 40;
/** The band along the top of the strip that holds the cycle range and the markers; the track
 *  rows fill the rest. */
export const OVERVIEW_BAND = 4;
/** The horizontal zoom limits in px per beat, the same as Ctrl+wheel over the tracks. */
export const ZOOM_MIN = 2;
export const ZOOM_MAX = 400;
/** Zoom factor per wheel notch, as over the tracks. */
export const ZOOM_STEP = 1.15;

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The Tracks area's scroll position and size (see `Viewport` in view.ts). */
export interface View {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** How the project maps onto the strip. */
export interface StripMap {
  /** Samples shown across the strip, from 0. */
  span: number;
  /** Strip width in CSS px. */
  width: number;
  /** Height of all the tracks in the Tracks area (Layout.total), in CSS px. */
  total: number;
  /** Where the track rows start and how tall they are in the strip. */
  rowsTop: number;
  rowsH: number;
}

/**
 * Samples shown across the strip: the project's content (`contentEnd`, the end of the last
 * region, marker or cycle range) plus `pad`, stretched when needed so the part of the
 * timeline in view (ending at `viewEnd`) is always inside the strip.
 */
export function overviewSpan(contentEnd: number, viewEnd: number, pad: number): number {
  const base = contentEnd > 0 ? contentEnd + pad : 0;
  return Math.max(base, viewEnd, 1);
}

export function timeToX(t: number, span: number, width: number): number {
  return (t / span) * width;
}

export function xToTime(x: number, span: number, width: number): number {
  return (x / width) * span;
}

/**
 * Each track's row in the strip: its top and height in the Tracks area scaled down so all
 * the tracks fill `rowsH` px from `rowsTop`. Tracks without height (hidden, or inside a
 * closed stack) get height 0. Rows can be thinner than a pixel; the drawing code decides how
 * to show those.
 */
export function overviewRows(tops: number[], heights: number[], total: number, rowsTop: number, rowsH: number): { y: number; h: number }[] {
  const k = total > 0 ? rowsH / total : 0;
  return tops.map((t, i) => ({ y: rowsTop + t * k, h: heights[i] * k }));
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Grows the span [a, b] to at least `min`, around its middle, kept within [lo, hi]. */
function atLeast(a: number, b: number, min: number, lo: number, hi: number): [number, number] {
  if (b - a >= min) return [a, b];
  const m = Math.min(min, hi - lo);
  const start = clamp((a + b) / 2 - m / 2, lo, hi - m);
  return [start, start + m];
}

/**
 * The visible-area box in strip px: across, the time in view; down, the tracks scrolled into
 * view. It is kept inside the strip and at least `minW` by `minH` px so it can still be seen
 * and grabbed when the view is a tiny part of a long project. With no tracks it spans the
 * rows' full height.
 */
export function visibleBox(v: View, pps: number, m: StripMap, minW = 6, minH = 4): Rect {
  const t0 = v.left / pps;
  const t1 = (v.left + v.width) / pps;
  let x0 = clamp(timeToX(t0, m.span, m.width), 0, m.width);
  let x1 = clamp(timeToX(t1, m.span, m.width), 0, m.width);
  [x0, x1] = atLeast(x0, x1, minW, 0, m.width);
  let y0 = m.rowsTop;
  let y1 = m.rowsTop + m.rowsH;
  if (m.total > 0) {
    y0 = m.rowsTop + clamp(v.top / m.total, 0, 1) * m.rowsH;
    y1 = m.rowsTop + clamp((v.top + v.height) / m.total, 0, 1) * m.rowsH;
  }
  [y0, y1] = atLeast(y0, y1, minH, m.rowsTop, m.rowsTop + m.rowsH);
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** The scroll position that centres the view on strip point (x, y), clamped at 0 (the
 *  Tracks area clamps the far end itself). */
export function centreOn(x: number, y: number, v: View, pps: number, m: StripMap): { left: number; top: number } {
  const t = xToTime(x, m.span, m.width);
  const cy = m.rowsH > 0 ? ((y - m.rowsTop) / m.rowsH) * m.total : 0;
  return { left: Math.max(0, t * pps - v.width / 2), top: Math.max(0, cy - v.height / 2) };
}

/** How far the Tracks area scrolls (in its px) when the box is dragged (dx, dy) strip px. */
export function dragScroll(dx: number, dy: number, pps: number, m: StripMap): { dLeft: number; dTop: number } {
  return {
    dLeft: xToTime(dx, m.span, m.width) * pps,
    dTop: m.rowsH > 0 ? (dy / m.rowsH) * m.total : 0,
  };
}

/**
 * The zoom from dragging one edge of the box: the other edge stays at `fixed` (samples) and
 * the dragged one goes to `dragged`, so a view `viewWidth` px wide shows the time between
 * them. Returns the new px per beat, held within the zoom limits, and the time at the left
 * of the view. When a limit stops the zoom, the dragged edge lands where the limit puts it;
 * dragging past the fixed edge zooms in as far as it goes.
 */
export function zoomFromEdge(
  edge: "left" | "right",
  fixed: number,
  dragged: number,
  viewWidth: number,
  samplesPerBeat: number,
): { pxPerBeat: number; start: number } {
  const span = Math.max(1, edge === "left" ? fixed - dragged : dragged - fixed);
  const pxPerBeat = clamp((viewWidth / span) * samplesPerBeat, ZOOM_MIN, ZOOM_MAX);
  const shown = (viewWidth / pxPerBeat) * samplesPerBeat;
  return { pxPerBeat, start: Math.max(0, edge === "left" ? fixed - shown : fixed) };
}

/** One wheel notch of zoom (in for a wheel turned away from the user), within the limits. */
export function wheelZoom(pxPerBeat: number, deltaY: number): number {
  return clamp(pxPerBeat * (deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP), ZOOM_MIN, ZOOM_MAX);
}

/** The scroll that keeps sample `anchor` at `cx` px from the view's left edge at `pps`. */
export function anchoredLeft(anchor: number, cx: number, pps: number): number {
  return Math.max(0, anchor * pps - cx);
}

export type BoxPart = "left" | "right" | "inside" | "outside";

/**
 * What a press at strip (x, y) grabs: an edge of the box (within `grab` px either side of it,
 * at any height, since the strip is short), the box itself, or the strip outside it. A narrow
 * box keeps its middle third for moving it.
 */
export function hitBox(b: Rect, x: number, y: number, grab = 4): BoxPart {
  const inner = Math.min(grab, b.w / 3);
  if (x >= b.x - grab && x <= b.x + inner) return "left";
  if (x >= b.x + b.w - inner && x <= b.x + b.w + grab) return "right";
  if (x > b.x && x < b.x + b.w && y >= b.y && y <= b.y + b.h) return "inside";
  return "outside";
}
