// Floating plug-in panels: an in-app window per panel taken out of the bottom section (or opened
// floating, as Settings › General says), hosting the effect's panel or the Channel EQ. The
// windows move by their title bar and resize by their edges; each effect type remembers its
// last size. Also here, because it is always mounted: Esc and closing panels whose instance
// went away, for docked panels too.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { DEFAULT_WIDTH, keepOnScreen, resizeRect, type Edge, type PanelKey, type Size } from "../../fxDock";
import { closePanel, dockPanel, escTarget, focusPanel, prunePanels, savedPanelSize, savePanelSize } from "../../fxDockState";
import { getState, useStore } from "../../store";
import { isMain } from "../../detach";
import { DetachButton } from "../DetachButton";
import { DockGlyph, PanelContent, PanelLead, PanelTools, panelExists, usePanelTitle } from "./FxPanel";

/** Where each floating panel was, so floating it again puts it back there. */
const positions = new Map<PanelKey, { x: number; y: number }>();

/** Every floating panel, plus the Esc key and clean-up for all open panels. */
export function FxWindows() {
  const open = useStore((s) => s.fxDock.open);
  const floating = useStore((s) => s.fxDock.floating);
  const project = useStore((s) => s.snap?.project);

  // The instance is gone (removed, undone, another project): its panel goes with it.
  useEffect(() => {
    if (project && open.length) prunePanels((k) => panelExists(project, k));
  }, [project, open]);

  useEffect(() => {
    if (!open.length) return;
    // Esc closes the frontmost panel, unless it belongs to a sheet, a menu or a field.
    const k = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || getState().dialog) return;
      const tgt = e.target as HTMLElement | null;
      if (tgt && ["INPUT", "SELECT", "TEXTAREA"].includes(tgt.tagName)) return;
      const key = escTarget();
      if (key !== null) {
        e.preventDefault();
        closePanel(key);
      }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [open.length]);

  return (
    <>
      {floating.map((key, i) => (
        <FloatingPanel key={key} panelKey={key} z={i} />
      ))}
    </>
  );
}

const EDGES: Edge[] = ["e", "w", "s", "se", "sw"];

/** The narrowest the window's content allows, at its current settings. */
function minWidth(el: HTMLElement): number {
  const w = el.style.width;
  el.style.width = "min-content";
  const min = el.offsetWidth;
  el.style.width = w;
  return min;
}

/** The height the window's content needs at its current width (graphs at their smallest). */
function naturalHeight(el: HTMLElement): number {
  const h = el.style.height;
  el.style.height = "auto";
  const nat = el.offsetHeight;
  el.style.height = h;
  return nat;
}

function FloatingPanel({ panelKey, z }: { panelKey: PanelKey; z: number }) {
  const title = usePanelTitle(panelKey);
  const type = title?.type ?? null;
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState(() => positions.get(panelKey) ?? { x: Math.max(20, window.innerWidth / 2 - 280 + z * 24), y: 120 + z * 24 });
  // Null until the window is first resized: it then sizes to its content.
  const [size, setSize] = useState<Size | null>(() => (type ? savedPanelSize(type) : null));
  const sizeType = useRef(type);

  useEffect(() => {
    positions.set(panelKey, pos);
  }, [panelKey, pos]);

  // The type is known once the effect list has loaded; a remembered size that the content no
  // longer fits (a larger interface size, another circuit) grows to fit.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!type || !el) return;
    let s = size;
    if (sizeType.current !== type) {
      sizeType.current = type;
      s = savedPanelSize(type);
    }
    if (s) {
      el.style.width = `${s.w}px`;
      const fit = { w: Math.max(s.w, minWidth(el)), h: Math.max(s.h, naturalHeight(el)) };
      if (fit.w !== s.w || fit.h !== s.h || s !== size) setSize(fit);
    }
    // Only when the panel's type changes; drags keep the size within limits themselves.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [type]);

  if (!title) return null;

  const startMove = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest("button, .fx-presets, .fx-sidechain")) return;
    const sx = e.clientX - pos.x;
    const sy = e.clientY - pos.y;
    const move = (ev: MouseEvent) => setPos({ x: Math.max(0, ev.clientX - sx), y: Math.max(0, ev.clientY - sy) });
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const startResize = (edge: Edge) => (e: React.PointerEvent) => {
    const el = ref.current;
    if (e.button !== 0 || !el) return;
    e.preventDefault();
    e.stopPropagation();
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const r = el.getBoundingClientRect();
    const start = { x: pos.x, y: pos.y, w: r.width, h: r.height };
    const minW = minWidth(el);
    const west = edge === "w" || edge === "sw";
    const maxW = west ? start.x + start.w : window.innerWidth - start.x;
    const maxH = window.innerHeight - start.y;
    let last = start;
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - e.clientX;
      const dy = ev.clientY - e.clientY;
      // The width first: the height the content needs depends on it (rows wrap).
      const across = resizeRect(start, edge, dx, 0, { w: minW, h: 0 }, { w: maxW, h: maxH });
      el.style.width = `${across.w}px`;
      last = resizeRect(start, edge, dx, dy, { w: minW, h: naturalHeight(el) }, { w: maxW, h: maxH });
      setPos({ x: last.x, y: last.y });
      setSize({ w: last.w, h: last.h });
    };
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("pointercancel", up);
      document.body.style.cursor = "";
      if (type && last !== start) savePanelSize(type, last);
    };
    document.body.style.cursor = getComputedStyle(handle).cursor;
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("pointercancel", up);
  };

  const at = keepOnScreen({ ...pos, w: size?.w ?? 0, h: size?.h ?? 0 }, { w: window.innerWidth, h: window.innerHeight });
  return (
    <div
      ref={ref}
      className={"eq-window fx-window" + (size ? " sized" : "")}
      style={{ left: at.x, top: at.y, zIndex: 41 + z, width: size?.w ?? (type ? DEFAULT_WIDTH[type] : undefined), height: size?.h }}
      onMouseDown={(e) => {
        e.stopPropagation();
        focusPanel(panelKey);
      }}
    >
      <div className="eq-titlebar" onMouseDown={startMove}>
        <PanelLead panelKey={panelKey} />
        <span className="eq-title">{title.name}</span>
        <span className="eq-track">{title.track}</span>
        <PanelTools panelKey={panelKey} />
        <span className="fx-title-gap" />
        <DetachButton panel={panelKey} className="fx-mode" />
        {/* A detached Mixer's window has no bottom section to dock in. */}
        {isMain && (
          <button className="fx-mode" title="Dock in the bottom section" onClick={() => dockPanel(panelKey)}>
            <DockGlyph />
          </button>
        )}
        <button className="eq-close" title="Close (Esc)" onClick={() => closePanel(panelKey)}>
          ×
        </button>
      </div>
      <PanelContent panelKey={panelKey} />
      {EDGES.map((edge) => (
        <div key={edge} className={`fx-resize ${edge}`} onPointerDown={startResize(edge)} />
      ))}
    </div>
  );
}
