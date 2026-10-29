// Logic's Marker track: the marker lane at the bottom of the ruler, the commands that create
// markers and step through them, and the marker menus. Markers are project data (saved and
// undoable); the engine never sees them.
import { act, type Id, type Marker, type Project } from "./api";
import type { MenuItem } from "./components/Menu";
import { keyHint } from "./keymap";
import { colorMenu } from "./ops";
import { currentTick, getState, setState } from "./store";
import { barsFrom, snapPos } from "./time";
import { mix, theme } from "./theme";
import { locateAndReveal } from "./transport";

const project = () => getState().snap?.project;
const playhead = () => currentTick()?.pos ?? 0;

/** Height of the marker lane. */
export const MARKER_H = 16;

/** Where a marker's section ends: its own length, else the next marker. */
export function markerEnd(p: Project, m: Marker): number | null {
  if (m.length && m.length > 0) return m.pos + m.length;
  const next = p.markers.find((n) => n.pos > m.pos);
  return next ? next.pos : null;
}

/** Create Marker: at the playhead, rounded to the nearest bar as Logic does (or exactly at
 *  `pos`, already snapped, from the marker lane). */
export function createMarker(pos?: number) {
  const p = project();
  if (!p) return;
  const at = pos ?? snapPos(p, playhead(), "bar", 0);
  act("add_marker", { pos: Math.max(0, Math.round(at)), name: null });
}

/** Go to Next / Previous Marker. */
export function gotoMarker(dir: 1 | -1) {
  const p = project();
  if (!p) return;
  const at = playhead();
  // A little slack, so a playhead sitting on a marker moves on to the next one.
  const m = dir > 0 ? p.markers.find((x) => x.pos > at + 1) : [...p.markers].reverse().find((x) => x.pos < at - 1);
  if (m) locateAndReveal(m.pos);
}

/** The marker at the playhead, or the last one before it. */
export function markerAtPlayhead(): Marker | null {
  const p = project();
  if (!p) return null;
  const at = playhead();
  return [...p.markers].reverse().find((m) => m.pos <= at + 1) ?? null;
}

export function renameMarker(m: Marker) {
  const name = window.prompt("Marker name", m.name);
  if (name !== null && name.trim() && name !== m.name) act("rename_marker", { id: m.id, name: name.trim() });
}

export function deleteMarker(id: Id) {
  act("delete_markers", { ids: [id] });
}

/** Set Locators to Marker: the cycle covers the marker's section. */
export function setLocatorsToMarker(m: Marker) {
  const p = project();
  if (!p) return;
  const end = markerEnd(p, m) ?? barsFrom(p, m.pos, 4);
  act("set_cycle", { enabled: true, start: m.pos, end: Math.round(end) });
}

export function openMarkerList() {
  setState({ showMarkerList: !getState().showMarkerList });
}

/** Right-click on a marker. */
export function markerMenu(m: Marker): MenuItem[] {
  return [
    { label: "Go to Marker", onClick: () => act("locate", { pos: m.pos }) },
    { label: "Set Locators to Marker", onClick: () => setLocatorsToMarker(m) },
    { separator: true },
    { label: "Rename…", onClick: () => renameMarker(m) },
    { label: "Color", children: colorMenu(m.color, (c) => act("set_marker_color", { id: m.id, color: c }), "Default") },
    { label: "Delete", onClick: () => deleteMarker(m.id) },
    { separator: true },
    { label: "Marker List…", hint: keyHint("marker-list"), onClick: openMarkerList },
  ];
}

/** Right-click on the empty marker lane, at timeline `pos`. */
export function markerLaneMenu(pos: number): MenuItem[] {
  return [
    { label: "Create Marker Here", onClick: () => createMarker(pos) },
    { label: "Marker List…", hint: keyHint("marker-list"), onClick: openMarkerList },
  ];
}

/** The default marker colour, a neutral that suits the theme. */
const defaultColor = () => (theme.region === "light" ? "#8e8e93" : "#9a9aa0");

/** Marker flags drawn last, for hit testing: content x range per marker. */
let flags: { id: Id; x0: number; x1: number }[] = [];

/** Draws the marker lane at `y` (canvas coordinates). `moving` shows a marker being dragged. */
export function drawMarkers(ctx: CanvasRenderingContext2D, p: Project, pps: number, left: number, W: number, y: number, moving: { id: Id; pos: number } | null) {
  const h = MARKER_H;
  const list = p.markers.map((m) => (moving && m.id === moving.id ? { ...m, pos: moving.pos } : m)).sort((a, b) => a.pos - b.pos);
  ctx.font = "600 10px Inter, 'SF Pro Text', system-ui, sans-serif";
  ctx.textBaseline = "middle";
  flags = [];
  list.forEach((m, i) => {
    const x = m.pos * pps - left;
    const next = list[i + 1];
    const reach = next ? next.pos * pps - left : W + 1;
    const label = m.name;
    const tw = ctx.measureText(label).width + 10;
    const x1 = Math.min(reach - 1, x + Math.max(10, tw));
    if (x1 < 0 || x > W) {
      flags.push({ id: m.id, x0: x + left, x1: x1 + left });
      return;
    }
    const color = m.color ?? defaultColor();
    // A faint band to the next marker, the flag with the name, and a stem at the position.
    ctx.fillStyle = mix(color, theme.ruler, 0.8, 0.9);
    ctx.fillRect(x, y + 1, Math.max(0, reach - x - 1), h - 2);
    ctx.fillStyle = moving?.id === m.id ? mix(color, "#ffffff", 0.25) : color;
    ctx.beginPath();
    ctx.roundRect(x, y + 1, Math.max(2, x1 - x), h - 2, [0, 3, 3, 0]);
    ctx.fill();
    ctx.save();
    ctx.beginPath();
    ctx.rect(x, y, Math.max(0, x1 - x - 2), h);
    ctx.clip();
    ctx.fillStyle = "rgba(10,10,12,0.9)";
    ctx.fillText(label, x + 4, y + h / 2 + 0.5);
    ctx.restore();
    flags.push({ id: m.id, x0: x + left, x1: x1 + left });
  });
}

/** The marker whose flag is at content x (as last drawn). */
export function markerAt(x: number): Marker | null {
  const p = project();
  if (!p) return null;
  // Later flags are drawn on top.
  for (let i = flags.length - 1; i >= 0; i--) {
    const f = flags[i];
    if (x >= f.x0 - 2 && x <= f.x1) return p.markers.find((m) => m.id === f.id) ?? null;
  }
  return null;
}
