// Colours for canvas-drawn views. `theme` is mutated in place by applyTheme() (themes.ts), and
// views that draw with it subscribe to `themeId` in the store so they redraw on a switch.

export interface CanvasTheme {
  bg0: string;
  text: string;
  /** Text drawn on top of a region's name bar. */
  ink: string;
  red: string;

  lane: string;
  laneSelected: string;
  arrangeBg: string;
  barLine: string;
  beatLine: string;
  cycleTint: string;

  ruler: string;
  rulerCycleZone: string;
  rulerEdge: string;
  rulerText: string;
  rulerTick: string;
  rulerMinor: string;
  playhead: string;
  cycleOn: string;
  cycleOff: string;

  meterBg: string;
  /** "smooth" blends `meterStops`; "stepped" uses three hard bands (low, mid, high). */
  meterStyle: "smooth" | "stepped";
  meterStops: string[];
  meterHold: string;

  /** "glass" = translucent tinted body; "flat" = solid dark body; "light" = solid pale body. */
  region: "glass" | "flat" | "light";
  regionBase: string;
  selectOutline: string;

  pianoWhiteKey: string;
  pianoBlackKey: string;
  pianoWhiteRow: string;
  pianoBlackRow: string;
  pianoLabel: string;
  editorBg: string;
  gridBar: string;
  gridBeat: string;
  /** The channel strip's gain-reduction meter. */
  meterGr: string;
}

export const theme: CanvasTheme = {
  bg0: "#1c1c1e",
  text: "#e2e2e5",
  ink: "rgba(10,10,12,0.92)",
  red: "#e5483a",
  lane: "#29292b",
  laneSelected: "#323235",
  arrangeBg: "#242426",
  barLine: "rgba(255,255,255,0.075)",
  beatLine: "rgba(255,255,255,0.03)",
  cycleTint: "rgba(242,194,48,0.045)",
  ruler: "#38383b",
  rulerCycleZone: "#2f2f32",
  rulerEdge: "#1a1a1c",
  rulerText: "#d4d4d8",
  rulerTick: "#7e7e83",
  rulerMinor: "#5e5e63",
  playhead: "#f0f0f2",
  cycleOn: "#f2c230",
  cycleOff: "#5b5b5f",
  meterBg: "#0c0c0d",
  meterStyle: "smooth",
  meterStops: ["#2f9e44", "#55c23b", "#d8d33a", "#e9a23b", "#e5483a", "#ff3b30"],
  meterHold: "#e8e8e8",
  region: "glass",
  regionBase: "#202022",
  selectOutline: "rgba(255,255,255,0.85)",
  pianoWhiteKey: "#e8e8ea",
  pianoBlackKey: "#1b1b1d",
  pianoWhiteRow: "#2b2b2e",
  pianoBlackRow: "#252527",
  pianoLabel: "#6b6b70",
  editorBg: "#222224",
  gridBar: "rgba(255,255,255,0.12)",
  gridBeat: "rgba(255,255,255,0.04)",
  meterGr: "#e9a23b",
};

function rgb(c: string): [number, number, number] {
  if (c.startsWith("rgba") || c.startsWith("rgb")) {
    const [r, g, b] = c.replace(/[^\d,.]/g, "").split(",").map(Number);
    return [r, g, b];
  }
  const h = c.replace("#", "");
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}

/** Linear mix of two colours, returned as an rgba() string. */
export function mix(a: string, b: string, t: number, alpha = 1): string {
  const [r1, g1, b1] = rgb(a);
  const [r2, g2, b2] = rgb(b);
  const m = (x: number, y: number) => Math.round(x + (y - x) * t);
  return `rgba(${m(r1, r2)},${m(g1, g2)},${m(b1, b2)},${alpha})`;
}
