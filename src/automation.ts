// Track automation in the arrange area: the lanes' maths (values, scales, labels), drawing each
// track's shown lane over its dimmed regions, and editing points with the mouse. Arrange.tsx
// hands over drawing and pointer events while the automation view is on.

import { useEffect, useRef, useState } from "react";
import { act, call, type AutoLane, type AutoMode, type AutoParam, type AutoPoint, type Id, type ParamInfo, type Project, type Track } from "./api";
import type { MenuItem } from "./components/Menu";
import { indexAt, type Layout } from "./layout";
import { currentTick, getState, onTick, setState } from "./store";
import { mix, theme } from "./theme";
import { dbToFader, faderToDb, formatDb, snapPos } from "./time";
import type { Viewport } from "./view";

export const VOLUME: AutoParam = { type: "volume" };

export const MODE_LABELS: Record<AutoMode, string> = { off: "Off", read: "Read", touch: "Touch", latch: "Latch", write: "Write" };
export const MODES: AutoMode[] = ["off", "read", "touch", "latch", "write"];

// ---- parameters -------------------------------------------------------------------------------

export function sameParam(a: AutoParam, b: AutoParam): boolean {
  if (a.type === "send" && b.type === "send") return a.value === b.value;
  if (a.type === "plugin" && b.type === "plugin") return a.value.uid === b.value.uid && a.value.param === b.value.param;
  return a.type === b.type;
}

export function paramLabel(p: AutoParam): string {
  switch (p.type) {
    case "volume":
      return "Volume";
    case "pan":
      return "Pan";
    case "mute":
      return "Mute";
    case "send":
      return `Send ${p.value + 1}`;
    case "plugin":
      return p.value.name;
  }
}

export function laneOf(t: Track, p: AutoParam): AutoLane | undefined {
  return t.automation?.find((l) => sameParam(l.param, p));
}

/** The lane a track shows in the automation view. */
export function shownParam(t: Track): AutoParam {
  return getState().autoLane[t.id] ?? VOLUME;
}

/** Folders only shape their subtracks, so they have nothing of their own to automate. */
export function automatable(t: Track): boolean {
  return t.kind !== "folder";
}

/** Height in the lane (0..1) for a value. Volume follows the fader's taper, as in Logic. */
export function toUnit(p: AutoParam, v: number): number {
  switch (p.type) {
    case "volume":
    case "send":
      return dbToFader(v);
    case "pan":
      return (v + 1) / 2;
    default:
      return Math.min(1, Math.max(0, v));
  }
}

export function fromUnit(p: AutoParam, u: number): number {
  u = Math.min(1, Math.max(0, u));
  switch (p.type) {
    case "volume":
    case "send":
      return Math.round(faderToDb(u) * 10) / 10;
    case "pan":
      return Math.round((u * 2 - 1) * 64) / 64;
    case "mute":
      return u >= 0.5 ? 1 : 0;
    case "plugin":
      return Math.round(u * 1000) / 1000;
  }
}

export function formatValue(p: AutoParam, v: number): string {
  switch (p.type) {
    case "volume":
    case "send":
      return `${formatDb(v)} dB`;
    case "pan": {
      const n = Math.round(v * 64);
      return n === 0 ? "Center" : n < 0 ? `${-n} L` : `${n} R`;
    }
    case "mute":
      return v >= 0.5 ? "Muted" : "On";
    case "plugin":
      return `${Math.round(v * 100)}%`;
  }
}

/** The control's own value, what an empty lane stands for. */
function liveValue(t: Track, p: AutoParam): number {
  switch (p.type) {
    case "volume":
      return t.volume_db;
    case "pan":
      return t.pan;
    case "mute":
      return t.mute ? 1 : 0;
    case "send":
      return 0;
    case "plugin":
      return 0.5;
  }
}

/** Straight lines between points, held before the first and after the last (as the engine). */
export function valueAt(points: AutoPoint[], pos: number): number | null {
  if (!points.length) return null;
  if (pos < points[0].pos) return points[0].value;
  let i = 0;
  while (i < points.length && points[i].pos <= pos) i++;
  if (i >= points.length) return points[points.length - 1].value;
  const a = points[i - 1];
  const b = points[i];
  return a.value + ((b.value - a.value) * (pos - a.pos)) / (b.pos - a.pos);
}

// ---- plug-in parameters -------------------------------------------------------------------

const paramCache = new Map<Id, Promise<ParamInfo[]>>();

/** A plug-in instance's automatable parameters (fetched once per instance). */
export function pluginParams(uid: Id): Promise<ParamInfo[]> {
  let p = paramCache.get(uid);
  if (!p) {
    p = call<ParamInfo[]>("list_plugin_params", { uid }).then((r) => r ?? []);
    paramCache.set(uid, p);
  }
  return p;
}

function showParam(t: Track, p: AutoParam) {
  setState({ autoLane: { ...getState().autoLane, [t.id]: p } });
}

/** The parameter picker: Volume, Pan, Mute and the parameters of the track's plug-ins. Lanes
 *  that hold automation are marked. */
export async function paramMenu(t: Track): Promise<MenuItem[]> {
  const cur = shownParam(t);
  const item = (p: AutoParam, label = paramLabel(p)): MenuItem => ({
    label,
    checked: sameParam(cur, p),
    hint: laneOf(t, p)?.points.length ? "●" : undefined,
    onClick: () => showParam(t, p),
  });
  const items: MenuItem[] = [item(VOLUME), item({ type: "pan" }), item({ type: "mute" })];
  const project = getState().snap?.project;
  t.sends.forEach((s, k) => {
    if (!s) return;
    const aux = project?.tracks.find((x) => x.id === s.bus);
    items.push(item({ type: "send", value: k }, `Send ${k + 1} → ${aux?.name ?? "Bus"}`));
  });
  const slots = [t.instrument, ...t.inserts].filter((s) => s && !s.plugin_id.startsWith("builtin:"));
  for (const s of slots) {
    const params = await pluginParams(s!.uid);
    if (!params.length) continue;
    const uid = s!.uid;
    items.push({
      label: s!.name,
      hint: t.automation.some((l) => l.param.type === "plugin" && l.param.value.uid === uid && l.points.length) ? "●" : undefined,
      children: params.map((pi) => item({ type: "plugin", value: { uid, param: pi.id, name: `${s!.name}: ${pi.name}` } }, pi.name)),
    });
  }
  items.push(
    { separator: true },
    { label: `Delete ${paramLabel(cur)} Automation`, disabled: !laneOf(t, cur)?.points.length, onClick: () => act("clear_automation", { track: t.id, param: cur }) },
    { label: "Delete All Automation on Track", disabled: !t.automation.some((l) => l.points.length), onClick: () => act("clear_automation", { track: t.id, param: null }) },
  );
  return items;
}

export function modeMenu(t: Track): MenuItem[] {
  return MODES.map((m) => ({ label: MODE_LABELS[m], checked: t.auto_mode === m, onClick: () => act("set_auto_mode", { track: t.id, mode: m }) }));
}

// ---- following automation in the faders --------------------------------------------------------

/**
 * What a fader or knob shows: the automation being played while the transport runs (and the
 * user isn't holding the control), otherwise the control's own value. Also returns the touch
 * handlers that tell the engine when the control is held, for Touch and Latch writing.
 */
export function useAutomation(track: Track | null, param: AutoParam, live: number): [number, { onTouchStart: () => void; onTouchEnd: () => void }] {
  const [touching, setTouching] = useState(false);
  const sent = useRef(false);
  const [, setFrame] = useState(0);
  const lane = track ? laneOf(track, param) : undefined;
  const follows = !!track && !!lane?.points.length && (track.auto_mode === "read" || track.auto_mode === "touch" || track.auto_mode === "latch");
  useEffect(() => {
    if (!follows) return;
    let was = false;
    return onTick((t) => {
      if (t.playing || was) setFrame((n) => n + 1);
      was = t.playing;
    });
  }, [follows]);
  const writing = !!track && (track.auto_mode === "touch" || track.auto_mode === "latch" || track.auto_mode === "write");
  const tick = currentTick();
  // In Read the lane wins even while the control is held, so it springs back as in Logic.
  const value = follows && !(touching && writing) && tick?.playing ? (valueAt(lane!.points, tick.pos) ?? live) : live;
  const touch = (on: boolean) => {
    setTouching(on);
    // Only the writing modes care (Read just plays the lane over the control); a release goes
    // wherever the grab went.
    if (track && (on ? writing : sent.current)) act("automation_touch", { track: track.id, param, touching: on });
    sent.current = on && writing;
  };
  return [value, { onTouchStart: () => touch(true), onTouchEnd: () => touch(false) }];
}

// ---- drawing ----------------------------------------------------------------------------------

const POINT_R = 3;
const HIT_R = 6;

/** A drag in progress: the points to draw instead of the track's lane. */
let preview: { track: Id; points: AutoPoint[] } | null = null;
/** Selected points, by position, on one track's shown lane. */
let selection: { track: Id; param: AutoParam; pos: Set<number> } | null = null;
let band: { x0: number; y0: number; x1: number; y1: number } | null = null;
let hover: { track: Id; x: number; y: number; text: string } | null = null;

export interface AutoView {
  project: Project;
  layout: Layout;
  /** Pixels per sample. */
  pps: number;
  vp: Viewport;
}

/** The lane's drawing span for a track at content-y `top` with height `h`. */
function bounds(top: number, h: number): [number, number] {
  return [top + 5, top + h - 6];
}

function pointsOf(t: Track): AutoPoint[] {
  if (preview && preview.track === t.id) return preview.points;
  return laneOf(t, shownParam(t))?.points ?? [];
}

function lineColor(t: Track): string {
  return theme.region === "light" ? mix(t.color, "#000000", 0.25) : mix(t.color, "#ffffff", 0.4);
}

function tag(ctx: CanvasRenderingContext2D, x: number, y: number, text: string, W: number) {
  ctx.font = "600 11px Inter, system-ui, sans-serif";
  const w = ctx.measureText(text).width + 10;
  const tx = Math.min(W - w - 2, x + 8);
  const ty = Math.max(2, y - 22);
  ctx.fillStyle = theme.region === "light" ? "rgba(255,255,255,0.95)" : "rgba(20,20,22,0.92)";
  ctx.beginPath();
  ctx.roundRect(tx, ty, w, 17, 3);
  ctx.fill();
  ctx.fillStyle = theme.text;
  ctx.textBaseline = "middle";
  ctx.fillText(text, tx + 5, ty + 9);
}

/** Draws every visible track's shown lane over its (dimmed) regions. */
export function drawAutomation(ctx: CanvasRenderingContext2D, v: AutoView) {
  const { project, layout, pps, vp } = v;
  const W = vp.width;
  const H = vp.height;
  project.tracks.forEach((t, i) => {
    const th = layout.heights[i];
    const y = layout.tops[i] - vp.top;
    if (th === 0 || y > H || y + th < 0) return;
    // Regions stay visible but recede, as in Logic.
    ctx.fillStyle = theme.region === "light" ? "rgba(255,255,255,0.5)" : "rgba(0,0,0,0.4)";
    ctx.fillRect(0, y, W, th - 1);
    if (!automatable(t)) return;
    const param = shownParam(t);
    const pts = pointsOf(t);
    const [top, bottom] = bounds(y, th);
    const py = (val: number) => bottom - toUnit(param, val) * (bottom - top);
    const px = (pos: number) => pos * pps - vp.left;
    const color = lineColor(t);
    ctx.save();
    if (t.auto_mode === "off") ctx.globalAlpha = 0.45;
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    if (!pts.length) {
      // No automation yet: the control's own value, where a first point would continue from.
      const ly = Math.round(py(liveValue(t, param))) + 0.5;
      ctx.setLineDash([4, 3]);
      ctx.moveTo(0, ly);
      ctx.lineTo(W, ly);
      ctx.stroke();
      ctx.restore();
      return;
    }
    ctx.moveTo(Math.min(0, px(pts[0].pos)), py(pts[0].value));
    for (const p of pts) ctx.lineTo(px(p.pos), py(p.value));
    ctx.lineTo(W, py(pts[pts.length - 1].value));
    ctx.stroke();
    const sel = selection && selection.track === t.id && sameParam(selection.param, param) ? selection.pos : null;
    for (const p of pts) {
      const x = px(p.pos);
      if (x < -POINT_R || x > W + POINT_R) continue;
      const on = !!sel?.has(p.pos);
      ctx.beginPath();
      ctx.arc(x, py(p.value), on ? POINT_R + 0.5 : POINT_R, 0, Math.PI * 2);
      ctx.fillStyle = on ? "#ffffff" : color;
      ctx.fill();
      ctx.lineWidth = 1;
      ctx.strokeStyle = on ? mix(t.color, "#000000", 0.2) : "rgba(0,0,0,0.55)";
      ctx.stroke();
    }
    ctx.restore();
  });
  if (band) {
    const x = Math.min(band.x0, band.x1) - vp.left;
    const y = Math.min(band.y0, band.y1) - vp.top;
    ctx.fillStyle = "rgba(255,255,255,0.08)";
    ctx.strokeStyle = "rgba(255,255,255,0.6)";
    ctx.lineWidth = 1;
    ctx.fillRect(x, y, Math.abs(band.x1 - band.x0), Math.abs(band.y1 - band.y0));
    ctx.strokeRect(x + 0.5, y + 0.5, Math.abs(band.x1 - band.x0), Math.abs(band.y1 - band.y0));
  }
  if (hover) tag(ctx, hover.x - vp.left, hover.y - vp.top, hover.text, W);
}

// ---- editing ----------------------------------------------------------------------------------

export interface AutoInput extends Omit<AutoView, "vp"> {
  /** The scrolling element the pointer coordinates are relative to. */
  el: HTMLElement;
  redraw: () => void;
}

interface Hit {
  track: Track;
  index: number;
  param: AutoParam;
  points: AutoPoint[];
  /** Content coordinates. */
  x: number;
  y: number;
  top: number;
  bottom: number;
  /** Index of the point under the pointer, or -1. */
  point: number;
}

function hitTest(e: { clientX: number; clientY: number }, v: AutoInput): Hit | null {
  const rect = v.el.getBoundingClientRect();
  const x = e.clientX - rect.left + v.el.scrollLeft;
  const y = e.clientY - rect.top + v.el.scrollTop;
  if (y >= v.layout.total) return null;
  const index = indexAt(v.layout, y);
  const track = v.project.tracks[index];
  if (!track || !automatable(track)) return null;
  const param = shownParam(track);
  const points = laneOf(track, param)?.points ?? [];
  const [top, bottom] = bounds(v.layout.tops[index], v.layout.heights[index]);
  let point = -1;
  let best = HIT_R * HIT_R;
  points.forEach((p, k) => {
    const dx = p.pos * v.pps - x;
    const dy = bottom - toUnit(param, p.value) * (bottom - top) - y;
    const d = dx * dx + dy * dy;
    if (d <= best) {
      best = d;
      point = k;
    }
  });
  return { track, index, param, points, x, y, top, bottom, point };
}

const unitAt = (h: { top: number; bottom: number }, y: number) => (h.bottom - y) / Math.max(1, h.bottom - h.top);

/** Sends a lane's points, at most one request at a time with the newest points winning, so a
 *  drag can be heard while it happens without queueing up stale edits. */
let inflight = false;
let queued: { track: Id; param: AutoParam; points: AutoPoint[] } | null = null;
function sendLane(track: Id, param: AutoParam, points: AutoPoint[]) {
  queued = { track, param, points: [...points].sort((a, b) => a.pos - b.pos) };
  if (inflight) return;
  const next = () => {
    const q = queued;
    queued = null;
    if (!q) {
      inflight = false;
      return;
    }
    inflight = true;
    act("set_automation_lane", { track: q.track, param: q.param, points: q.points }).finally(next);
  };
  next();
}

/** A press on a lane while the automation view is on. */
export function automationMouseDown(e: React.MouseEvent, v: AutoInput) {
  const h = hitTest(e, v);
  if (!h) {
    selection = null;
    v.redraw();
    return;
  }
  if (h.track.id !== getState().snap?.selected) act("select_track", { id: h.track.id });
  const snap = (pos: number) => Math.max(0, Math.round(snapPos(v.project, pos, getState().snapMode, getState().pxPerBeat)));
  const sameLane = selection && selection.track === h.track.id && sameParam(selection.param, h.param);

  if (h.point >= 0) {
    const grabbed = h.points[h.point];
    // Double-click or Alt-click deletes a point.
    if (e.altKey || e.detail >= 2) {
      selection = null;
      sendLane(h.track.id, h.param, h.points.filter((_, k) => k !== h.point));
      return;
    }
    let pos = sameLane ? new Set(selection!.pos) : new Set<number>();
    if (e.shiftKey) {
      if (pos.has(grabbed.pos)) pos.delete(grabbed.pos);
      else pos.add(grabbed.pos);
    } else if (!pos.has(grabbed.pos)) {
      pos = new Set([grabbed.pos]);
    }
    selection = { track: h.track.id, param: h.param, pos };
    v.redraw();
    if (!pos.has(grabbed.pos)) return;
    // Drag the selected points; Shift keeps them in time and moves only the value.
    const startX = e.clientX;
    const startY = e.clientY;
    const picked = h.points.map((p) => pos.has(p.pos));
    let moved = false;
    let result = h.points;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < 3 && Math.abs(ev.clientY - startY) < 3) return;
      moved = true;
      const dPos = ev.shiftKey ? 0 : snap(grabbed.pos + (ev.clientX - startX) / v.pps) - grabbed.pos;
      const dU = (startY - ev.clientY) / Math.max(1, h.bottom - h.top);
      result = h.points.map((p, k) => (picked[k] ? { pos: Math.max(0, p.pos + dPos), value: fromUnit(h.param, toUnit(h.param, p.value) + dU) } : p));
      result.sort((a, b) => a.pos - b.pos);
      preview = { track: h.track.id, points: result };
      selection = { track: h.track.id, param: h.param, pos: new Set(h.points.filter((_, k) => picked[k]).map((p) => Math.max(0, p.pos + dPos))) };
      const g = { pos: grabbed.pos + dPos, value: fromUnit(h.param, toUnit(h.param, grabbed.value) + dU) };
      hover = { track: h.track.id, x: g.pos * v.pps, y: h.bottom - toUnit(h.param, g.value) * (h.bottom - h.top), text: formatValue(h.param, g.value) };
      sendLane(h.track.id, h.param, result);
      v.redraw();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      if (moved) sendLane(h.track.id, h.param, result);
      // Keep drawing the dragged points until the lane's new snapshot arrives.
      const done = () => {
        if (inflight) return void setTimeout(done, 16);
        preview = null;
        v.redraw();
      };
      done();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return;
  }

  // Empty lane space: a click adds a point, a drag selects points with a rubber band.
  const x0 = h.x;
  const y0 = h.y;
  const extend = e.shiftKey && sameLane ? new Set(selection!.pos) : new Set<number>();
  let banding = false;
  const move = (ev: MouseEvent) => {
    const c = hitPoint(ev, v);
    if (!banding && Math.abs(c.x - x0) < 3 && Math.abs(c.y - y0) < 3) return;
    banding = true;
    band = { x0, y0, x1: c.x, y1: c.y };
    // The band selects on the lane it started in.
    const [lo, hi] = [Math.min(x0, c.x) / v.pps, Math.max(x0, c.x) / v.pps];
    const [ulo, uhi] = [unitAt(h, Math.max(y0, c.y)), unitAt(h, Math.min(y0, c.y))];
    const pos = new Set(extend);
    for (const p of h.points) {
      const u = toUnit(h.param, p.value);
      if (p.pos >= lo && p.pos <= hi && u >= ulo - 0.02 && u <= uhi + 0.02) pos.add(p.pos);
    }
    selection = { track: h.track.id, param: h.param, pos };
    v.redraw();
  };
  const up = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    if (banding) {
      band = null;
      v.redraw();
      return;
    }
    const p = { pos: snap(h.x / v.pps), value: fromUnit(h.param, unitAt(h, h.y)) };
    selection = { track: h.track.id, param: h.param, pos: new Set([p.pos]) };
    sendLane(h.track.id, h.param, [...h.points.filter((q) => q.pos !== p.pos), p]);
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
}

function hitPoint(e: MouseEvent, v: AutoInput) {
  const rect = v.el.getBoundingClientRect();
  return { x: e.clientX - rect.left + v.el.scrollLeft, y: e.clientY - rect.top + v.el.scrollTop };
}

/** Pointer moves over the lanes: the value under the pointer (a point's, or the curve's). */
export function automationHover(e: React.MouseEvent, v: AutoInput) {
  const h = hitTest(e, v);
  const prev = hover;
  if (!h) {
    hover = null;
    v.el.style.cursor = "default";
  } else if (h.point >= 0) {
    const p = h.points[h.point];
    hover = { track: h.track.id, x: p.pos * v.pps, y: h.bottom - toUnit(h.param, p.value) * (h.bottom - h.top), text: formatValue(h.param, p.value) };
    v.el.style.cursor = "pointer";
  } else {
    const val = valueAt(h.points, h.x / v.pps) ?? liveValue(h.track, h.param);
    hover = { track: h.track.id, x: h.x, y: h.y, text: `${paramLabel(h.param)} ${formatValue(h.param, val)}` };
    v.el.style.cursor = "crosshair";
  }
  if (prev || hover) v.redraw();
}

export function automationLeave(v: { redraw: () => void }) {
  if (hover) {
    hover = null;
    v.redraw();
  }
}

/** Deletes the selected points (Delete key). Returns false when none are selected. */
export function deleteSelectedPoints(): boolean {
  const s = getState();
  if (!s.showAutomation || !selection?.pos.size) return false;
  const t = s.snap?.project.tracks.find((x) => x.id === selection!.track);
  const lane = t && laneOf(t, selection.param);
  if (!lane) return false;
  const gone = selection.pos;
  selection = null;
  sendLane(t.id, lane.param, lane.points.filter((p) => !gone.has(p.pos)));
  return true;
}

/** Right-click on a lane in the automation view. */
export function automationMenu(t: Track | undefined): MenuItem[] {
  if (!t || !automatable(t)) return [{ label: "Hide Automation", hint: "A", onClick: () => setState({ showAutomation: false }) }];
  const p = shownParam(t);
  const n = selection && selection.track === t.id && sameParam(selection.param, p) ? selection.pos.size : 0;
  return [
    { label: n > 1 ? "Delete Selected Points" : "Delete Selected Point", disabled: !n, hint: "⌫", onClick: () => deleteSelectedPoints() },
    { label: `Delete ${paramLabel(p)} Automation`, disabled: !laneOf(t, p)?.points.length, onClick: () => act("clear_automation", { track: t.id, param: p }) },
    { label: "Delete All Automation on Track", disabled: !t.automation.some((l) => l.points.length), onClick: () => act("clear_automation", { track: t.id, param: null }) },
    { separator: true },
    { label: "Hide Automation", hint: "A", onClick: () => setState({ showAutomation: false }) },
  ];
}
