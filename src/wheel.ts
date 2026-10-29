// What the mouse wheel scrolls in the time views (Settings › Editing › Mouse Wheel Scrolls).
// A mouse has only a vertical wheel, so "horizontal" turns it into scrolling through time,
// with Shift giving back vertical scrolling: the inverse of the usual Shift convention.

/** "vertical": the wheel scrolls tracks (or pitches), Shift scrolls time. "horizontal": the
 *  wheel scrolls time, Shift scrolls tracks. */
export type WheelScrolls = "vertical" | "horizontal";

/** The parts of a WheelEvent the mapping looks at. */
export interface WheelLike {
  deltaX: number;
  deltaY: number;
  /** 0 = pixels, 1 = lines, 2 = pages. */
  deltaMode: number;
  shiftKey: boolean;
}

/** Pixels per line for wheels that report lines (Firefox-style). */
const LINE_PX = 16;

/**
 * The scroll distances in px for a wheel event when the wheel scrolls time
 * ("horizontal"). The vertical setting is left to native scrolling and never comes here.
 *
 * Without Shift, the larger of the two axes scrolls time, so a trackpad's sideways swipe
 * still works and its up/down movement does too. With Shift, the wheel scrolls vertically;
 * some platforms already turn Shift+wheel into a horizontal delta, so that counts as well.
 * `page` is the size of a page for page-based deltas (the view's width or height).
 */
export function timeWheel(e: WheelLike, page = 800): { dx: number; dy: number } {
  const k = e.deltaMode === 1 ? LINE_PX : e.deltaMode === 2 ? page : 1;
  const x = e.deltaX * k;
  const y = e.deltaY * k;
  const d = Math.abs(x) > Math.abs(y) ? x : y;
  return e.shiftKey ? { dx: 0, dy: d } : { dx: d, dy: 0 };
}

/**
 * The horizontal scroll for a wheel over a strip that only scrolls in time (the global
 * tracks): its trackpad sideways movement, Shift+wheel in the vertical setting, or the plain
 * wheel in the horizontal one.
 */
export function timeStripWheel(e: WheelLike, mode: WheelScrolls, page = 800): number {
  if (mode === "horizontal") return timeWheel(e, page).dx;
  const k = e.deltaMode === 1 ? LINE_PX : e.deltaMode === 2 ? page : 1;
  return (e.shiftKey ? e.deltaX || e.deltaY : e.deltaX) * k;
}
