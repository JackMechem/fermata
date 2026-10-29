// Track header geometry, kept pure so it can be tested: which layout a header uses at a given
// height, and how the volume slider with its built-in level meter maps levels to pixels.
// (No runtime imports, so the tests can load it directly; the fader taper is passed in.)

/** A fader taper: dB to a position 0..1 (`dbToFader` in time.ts). */
export type Taper = (db: number) => number;

/**
 * How a track header lays itself out at a given height:
 * - `full`: the icon tile, the name above a row of M S R I and freeze buttons;
 * - `compact`: everything on one line with a smaller icon; the buttons move into a "⋯" menu,
 *   and only the ones switched on stay in the header;
 * - `tiny`: as compact without the icon, with a smaller name and controls.
 */
export type HeaderTier = "full" | "compact" | "tiny";

/** Smallest height (px) with room for the name above the button row. */
export const FULL_TIER_H = 52;
/** Smallest height (px) that still shows the track icon. */
export const COMPACT_TIER_H = 32;

export function headerTier(height: number): HeaderTier {
  if (height >= FULL_TIER_H) return "full";
  if (height >= COMPACT_TIER_H) return "compact";
  return "tiny";
}

/** The header volume slider's size at a tier: the groove (which holds the meter) and the
 *  thumb that sits over it, in px. */
export function levelSliderSize(tier: HeaderTier): { groove: number; thumbW: number; thumbH: number } {
  return tier === "tiny" ? { groove: 8, thumbW: 7, thumbH: 14 } : { groove: 10, thumbW: 8, thumbH: 18 };
}

/** Where the slider's thumb is centred, in px from the groove's left edge, for fader position
 *  `f` (0..1). The thumb never runs past either end. */
export function thumbCenter(f: number, width: number, thumbW: number): number {
  const c = Math.min(1, Math.max(0, f));
  return thumbW / 2 + c * Math.max(0, width - thumbW);
}

/** Level below which the meter shows nothing. */
export const SLIDER_METER_FLOOR = -60;
/** Below this the meter leaves the fader taper and runs straight down to the floor. */
const BLEND_DB = -40;

/**
 * A level in dBFS as a fraction (0..1) of the slider's travel. Above -40 dB this is the
 * fader's own `taper`, so the meter lines up with the scale the thumb moves on: the track meter
 * is post-fader, so a full-scale signal reaches just the thumb, and only a hotter one passes
 * it. Below that it runs linearly down to nothing at the floor, so silence draws no stub.
 */
export function meterTravel(db: number, taper: Taper): number {
  if (!(db > SLIDER_METER_FLOOR)) return 0;
  if (db < BLEND_DB) return ((db - SLIDER_METER_FLOOR) / (BLEND_DB - SLIDER_METER_FLOOR)) * taper(BLEND_DB);
  return taper(db);
}

/** The meter scale for a slider `width` px wide with a `thumbW` px thumb: a level as a fraction
 *  of the whole groove, measured to where the thumb's centre would be at the same position. */
export function meterScale(width: number, thumbW: number, taper: Taper): (db: number) => number {
  return (db) => {
    const t = meterTravel(db, taper);
    return t > 0 && width > 0 ? Math.min(1, thumbCenter(t, width, thumbW) / width) : 0;
  };
}
