// Draggable dividers between panes. Sizes live in the store and are remembered per machine.
import { getState, setState, useStore, type PaneSizes } from "../store";

const KEY = "fermata.paneSizes";

export const PANE_DEFAULTS: PaneSizes = { library: 230, inspector: 222, browser: 230, mixer: 400, editor: 300, headers: 296, fx: 420 };

/** Limits per pane: [min, max] in CSS pixels. */
const LIMITS: Record<keyof PaneSizes, [number, number]> = {
  library: [170, 480],
  inspector: [200, 460],
  browser: [180, 520],
  mixer: [180, 900],
  editor: [140, 900],
  headers: [220, 520],
  fx: [160, 900],
};

export function loadPaneSizes() {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<PaneSizes>;
    const sizes = { ...PANE_DEFAULTS };
    for (const k of Object.keys(PANE_DEFAULTS) as (keyof PaneSizes)[]) {
      if (typeof v[k] === "number") sizes[k] = clamp(k, v[k]!);
    }
    setState({ paneSizes: sizes });
  } catch {
    // Defaults are fine.
  }
}

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(getState().paneSizes));
  } catch {
    // Not critical: sizes fall back to the defaults next time.
  }
}

const clamp = (k: keyof PaneSizes, v: number) => Math.round(Math.min(LIMITS[k][1], Math.max(LIMITS[k][0], v)));

export function setPaneSize(k: keyof PaneSizes, v: number) {
  setState({ paneSizes: { ...getState().paneSizes, [k]: clamp(k, v) } });
}

/**
 * A divider that resizes `pane`. `dir` is the drag axis; `sign` is +1 when dragging right/down
 * grows the pane (it sits on the pane's right/bottom edge) and -1 when it shrinks it.
 * Double-click restores the default size.
 */
export function Splitter({ pane, dir, sign }: { pane: keyof PaneSizes; dir: "x" | "y"; sign: 1 | -1 }) {
  useStore((s) => s.paneSizes[pane]);
  const start = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const from = dir === "x" ? e.clientX : e.clientY;
    const base = getState().paneSizes[pane];
    document.body.style.cursor = dir === "x" ? "col-resize" : "row-resize";
    el.classList.add("dragging");
    const move = (ev: PointerEvent) => setPaneSize(pane, base + sign * ((dir === "x" ? ev.clientX : ev.clientY) - from));
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      el.classList.remove("dragging");
      document.body.style.cursor = "";
      save();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };
  return (
    <div
      className={`splitter splitter-${dir}`}
      onPointerDown={start}
      onDoubleClick={() => {
        setPaneSize(pane, PANE_DEFAULTS[pane]);
        save();
      }}
      title="Drag to resize · double-click to reset"
    />
  );
}
