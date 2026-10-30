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
  if (m) act("locate", { pos: m.pos });
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
    { label: "Length", children: lengthMenu(m) },
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

/** A marker as drawn: its flag (the name) and its bar, in content x. `end` is set for a marker
 *  with its own length, whose bar's right edge can be dragged. */
let flags: { id: Id; x0: number; x1: number; band: number; end: number | null }[] = [];

/** A marker being dragged on the lane: moved to `pos`, or resized to `length`. */
export interface MarkerDrag {
  id: Id;
  pos: number;
  length?: number | null;
}

/** Draws the marker lane at `y` (canvas coordinates): each marker's bar over its length (to
 *  the next marker when it has none), with its name flag at the start. */
export function drawMarkers(ctx: CanvasRenderingContext2D, p: Project, pps: number, left: number, W: number, y: number, moving: MarkerDrag | null) {
  const h = MARKER_H;
  const selected = getState().selectedMarker;
  const list = p.markers.map((m) => (moving && m.id === moving.id ? { ...m, pos: moving.pos, length: moving.length === undefined ? m.length : moving.length } : m)).sort((a, b) => a.pos - b.pos);
  const proj = { ...p, markers: list };
  ctx.font = "600 10px Inter, 'SF Pro Text', system-ui, sans-serif";
  ctx.textBaseline = "middle";
  flags = [];
  list.forEach((m) => {
    const x = m.pos * pps - left;
    const own = !!m.length && m.length > 0;
    const endPos = markerEnd(proj, m);
    const reach = endPos !== null ? endPos * pps - left : W + 1;
    const label = m.name;
    const tw = ctx.measureText(label).width + 10;
    const x1 = Math.min(reach - 1, x + Math.max(10, tw));
    const hit = { id: m.id, x0: x + left, x1: x1 + left, band: reach + left, end: own ? reach + left : null };
    if (reach < 0 || x > W) {
      flags.push(hit);
      return;
    }
    const color = m.color ?? defaultColor();
    const on = selected === m.id || moving?.id === m.id;
    // The bar over the marker's length (stronger when it has a length of its own, or is
    // selected), the flag with the name, and the bar's end.
    ctx.fillStyle = mix(color, theme.ruler, on ? 0.55 : own ? 0.68 : 0.8, on ? 0.95 : 0.9);
    ctx.fillRect(x, y + 1, Math.max(0, reach - x - 1), h - 2);
    if (own || on) {
      ctx.fillStyle = color;
      ctx.fillRect(Math.round(reach) - 2, y + 1, 2, h - 2);
    }
    ctx.fillStyle = on ? mix(color, "#ffffff", 0.25) : color;
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
    flags.push(hit);
  });
}

/** The marker at content x (as last drawn): its flag, or the bar of one with its own length. */
export function markerAt(x: number): Marker | null {
  const p = project();
  if (!p) return null;
  const find = (id: Id) => p.markers.find((m) => m.id === id) ?? null;
  // Later flags are drawn on top.
  for (let i = flags.length - 1; i >= 0; i--) {
    const f = flags[i];
    if (x >= f.x0 - 2 && x <= f.x1) return find(f.id);
  }
  for (let i = flags.length - 1; i >= 0; i--) {
    const f = flags[i];
    if (f.end !== null && x >= f.x0 && x <= f.end) return find(f.id);
  }
  return null;
}

/** The marker whose bar ends at content x, for dragging its length. Only a marker with a
 *  length of its own has an edge to drag (give it one in its menu or the Marker List). */
export function markerEdgeAt(x: number): Marker | null {
  const p = project();
  if (!p) return null;
  for (let i = flags.length - 1; i >= 0; i--) {
    const f = flags[i];
    if (f.end !== null && Math.abs(x - f.end) <= 4 && x > f.x1) return p.markers.find((m) => m.id === f.id) ?? null;
  }
  return null;
}

/** Selects a marker on the Marker track (and in the Marker List). */
export function selectMarker(id: Id | null) {
  if (getState().selectedMarker !== id) setState({ selectedMarker: id });
}

/** Delete with a marker selected removes it. */
export function deleteSelectedMarker(): boolean {
  const id = getState().selectedMarker;
  if (id === null || !project()?.markers.some((m) => m.id === id)) return false;
  setState({ selectedMarker: null });
  deleteMarker(id);
  return true;
}

/** A click on a marker: selects it and goes to it; with Cycle on the cycle takes its length
 *  too, as in Logic. */
export function clickMarker(m: Marker) {
  selectMarker(m.id);
  if (project()?.cycle.enabled) setLocatorsToMarker(m);
  act("locate", { pos: m.pos });
}

/** Sets a marker's own length (null: to the next marker again). */
export function setMarkerLength(m: Marker, length: number | null) {
  act("set_marker_length", { id: m.id, length: length === null ? null : Math.max(1, Math.round(length)) });
}

/** The marker menu's Length submenu. */
function lengthMenu(m: Marker): MenuItem[] {
  const p = project();
  if (!p) return [];
  const own = !!m.length && m.length > 0;
  const bars = [1, 2, 4, 8, 16].map((n) => ({ label: n === 1 ? "1 Bar" : `${n} Bars`, onClick: () => setMarkerLength(m, barsFrom(p, m.pos, n) - m.pos) }));
  return [{ label: "To Next Marker", checked: !own, onClick: () => setMarkerLength(m, null) }, { separator: true }, ...bars];
}
