// A region's name bar and body at a given height. Tracks can be made very short, so the bar
// shrinks on short regions to leave the waveform or notes a strip of their own. Pure, for the
// tests.

/** Regions shorter than this (px) get the slim name bar. */
export const SLIM_REGION_H = 30;
/** Bodies shorter than this (px) draw no waveform or notes: there's nothing to read. */
export const MIN_BODY_H = 3;

export interface RegionBar {
  /** Height of the name bar at the region's top. */
  bar: number;
  /** Font size of the name in the bar. */
  font: number;
  /** Offset of the body (waveform, notes, fades) from the region's top, and its height. */
  bodyTop: number;
  bodyH: number;
}

/** The name bar and body of a region drawn `h` px tall. */
export function regionBar(h: number): RegionBar {
  const slim = h < SLIM_REGION_H;
  const bar = slim ? 12 : 14;
  const gap = slim ? 1 : 2;
  return { bar, font: slim ? 10 : 11, bodyTop: bar + gap, bodyH: Math.max(0, h - bar - 2 * gap) };
}

/** Fade-handle band at the top of a region body (`zone` is its full size, used on tall
 *  regions): at most half the body, so the lower half still trims the region's edges. */
export function fadeZone(bodyH: number, zone: number): number {
  return Math.min(zone, Math.ceil(bodyH / 2));
}
