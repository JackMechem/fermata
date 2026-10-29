import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getState, setState } from "./store";

/** Interface sizes offered in Settings › Appearance. */
export const UI_SCALES = [0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];

const KEY = "fermata.uiScale";

// The display's own density, read before any page zoom is applied.
const baseRatio = window.devicePixelRatio || 1;

/**
 * Device pixels per CSS pixel for canvas backing stores. Some webviews fold page zoom into
 * devicePixelRatio and some don't, so take whichever is larger to keep canvases sharp.
 */
export function pixelRatio(): number {
  return Math.max(window.devicePixelRatio || 1, baseRatio * getState().uiScale);
}

export function savedUiScale(): number {
  try {
    const v = parseFloat(localStorage.getItem(KEY) ?? "");
    return UI_SCALES.includes(v) ? v : 1;
  } catch {
    return 1;
  }
}

export async function applyUiScale(scale: number, persist = true) {
  const s = Math.min(2, Math.max(0.9, scale));
  setState({ uiScale: s });
  try {
    await getCurrentWebview().setZoom(s);
  } catch (e) {
    setState({ toast: { text: `Couldn't change the interface size: ${e}`, at: Date.now() } });
  }
  if (persist) {
    try {
      localStorage.setItem(KEY, String(s));
    } catch {
      // Storage can be unavailable; the size still applies for this session.
    }
  }
}

/** Steps to the next larger (+1) or smaller (-1) size. */
export function stepUiScale(dir: 1 | -1) {
  const cur = getState().uiScale;
  const i = UI_SCALES.findIndex((s) => Math.abs(s - cur) < 0.001);
  const next = UI_SCALES[Math.min(UI_SCALES.length - 1, Math.max(0, (i < 0 ? 1 : i) + dir))];
  applyUiScale(next);
}
