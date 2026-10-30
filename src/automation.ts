// Track automation in the arrange area: the lanes' maths (values, scales, labels), drawing each
// track's shown lanes (the main one over its dimmed regions, extra ones below it, and the Output
// track's), and editing points and curves with the mouse. Arrange.tsx hands over drawing and
// pointer events while the automation view is on. The pure parts are in autoLanes.ts.

import { useEffect, useRef, useState } from "react";
import { act, BUILTIN_SYNTH, call, type AutoLane, type AutoMode, type AutoParam, type AutoPoint, type Id, type ParamInfo, type Project, type Track } from "./api";
import { boxAt, curveThrough, laneBoxes, nextParam, OUTPUT_ID, sameParam, segmentAt, segmentSamples, usedLayout, valueAt, withCurve, type LaneBox, type LaneRow } from "./autoLanes";
import type { MenuItem } from "./components/Menu";
import { keyHint } from "./keymap";
import type { Layout } from "./layout";
import { currentTick, getState, onTick, setState } from "./store";
import { mix, theme } from "./theme";
import { dbToFader, faderToDb, formatDb, snapPos } from "./time";
import type { Viewport } from "./view";
import type { FxParamSpec } from "./fx";
import { formatValue as fxFormat, fromNorm, toNorm } from "./fxFormat";

export const VOLUME: AutoParam = { type: "volume" };

export const MODE_LABELS: Record<AutoMode, string> = { off: "Off", read: "Read", touch: "Touch", latch: "Latch", write: "Write" };
export const MODES: AutoMode[] = ["off", "read", "touch", "latch", "write"];

// ---- parameters -------------------------------------------------------------------------------

export { sameParam, valueAt };

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

/** Every strip can be automated: a folder its volume and mute (which reach its subtracks),
 *  Stereo Out its volume and plug-ins. */
export function automatable(_t: Track): boolean {
  return true;
}

export function isOutput(t: Track): boolean {
  return t.id === OUTPUT_ID;
}

let outputCache: { project: Project; track: Track } | null = null;

/** Stereo Out as a track, for the Output track and the automation code: its volume, plug-ins
 *  and lanes. The same object for the same project snapshot. */
export function outputTrack(project: Project): Track {
  if (outputCache?.project === project) return outputCache.track;
  const m = project.master;
  const track: Track = {
    id: OUTPUT_ID,
    kind: "aux",
    name: "Stereo Out",
    color: "#8e8e93",
    volume_db: m.volume_db,
    pan: 0,
    mute: false,
    solo: false,
    armed: false,
    monitor: false,
    input: { type: "none" },
    instrument: null,
    inserts: m.inserts,
    regions: [],
    eq: m.eq,
    parent: null,
    collapsed: false,
    group: null,
    hidden: false,
    sends: [],
    output: null,
    bus: 0,
    automation: m.automation ?? [],
    auto_mode: m.auto_mode ?? "read",
    auto_lanes: m.auto_lanes ?? [],
  };
  outputCache = { project, track };
  return track;
}

/** A strip by id: a track, or the Output track for Stereo Out. */
export function stripOf(project: Project | undefined, id: Id): Track | undefined {
  if (!project) return undefined;
  return id === OUTPUT_ID ? outputTrack(project) : project.tracks.find((t) => t.id === id);
}

/** The parameters a strip has besides its plug-ins' (Stereo Out has no pan or mute, a folder
 *  no pan). */
function baseParams(t: Track): AutoParam[] {
  if (isOutput(t)) return [VOLUME];
  if (t.kind === "folder") return [VOLUME, { type: "mute" }];
  return [VOLUME, { type: "pan" }, { type: "mute" }, ...t.sends.flatMap((s, k): AutoParam[] => (s ? [{ type: "send", value: k }] : []))];
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
    case "plugin": {
      // A built-in effect's lane reads in the parameter's own units.
      const spec = builtinSpec(p.value.uid, p.value.param);
      return spec ? fxFormat(spec, fromNorm(spec, v)) : `${Math.round(v * 100)}%`;
    }
  }
}

function builtinSpec(uid: Id, param: number): FxParamSpec | undefined {
  const st = getState();
  const project = st.snap?.project;
  const slot = [...(project?.tracks.flatMap((t) => t.inserts) ?? []), ...(project?.master.inserts ?? [])].find((s) => s?.uid === uid);
  return st.fxList.find((f) => f.id === slot?.plugin_id)?.params.find((p) => p.id === param);
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
      return t.sends?.[p.value]?.level_db ?? 0;
    case "plugin": {
      // A built-in effect's control is known here; a VST3's sits in its editor.
      const spec = builtinSpec(p.value.uid, p.value.param);
      const saved = t.inserts.find((s) => s?.uid === p.value.uid)?.params?.find((x) => x[0] === p.value.param)?.[1];
      return spec ? toNorm(spec, saved ?? spec.default) : 0.5;
    }
  }
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

function setSubLanes(t: Track, lanes: AutoParam[]) {
  act("set_auto_lanes", { lanes: [[t.id, lanes]] });
}

/** Shows another lane below the track's lanes (the "+" on a lane header): the next lane with
 *  automation that isn't shown, else the next of volume, pan, mute and sends. */
export function addLane(t: Track) {
  const subs = t.auto_lanes ?? [];
  const p = nextParam([shownParam(t), ...subs], t.automation, baseParams(t));
  if (p) setSubLanes(t, [...subs, p]);
}

/** Closes sub-lane `k`. */
export function closeLane(t: Track, k: number) {
  setSubLanes(t, (t.auto_lanes ?? []).filter((_, i) => i !== k));
}

/** Show Used Automation: every lane holding points on the selected tracks (all tracks when
 *  none is selected) gets shown, the main lane first. Turns the automation view on. */
export function showUsedAutomation() {
  const s = getState();
  const project = s.snap?.project;
  if (!project) return;
  const sel = s.selectedTracks.length ? s.selectedTracks : s.snap?.selected != null ? [s.snap.selected] : project.tracks.map((t) => t.id);
  const strips = sel.map((id) => stripOf(project, id)).filter((t): t is Track => !!t);
  if (s.showOutput && !strips.some(isOutput)) strips.push(outputTrack(project));
  const autoLane = { ...s.autoLane };
  const lanes: [Id, AutoParam[]][] = [];
  for (const t of strips) {
    const { main, subs } = usedLayout(t.automation, shownParam(t));
    autoLane[t.id] = main;
    lanes.push([t.id, subs]);
  }
  setState({ autoLane, showAutomation: true });
  act("set_auto_lanes", { lanes });
}

/** The parameter picker: Volume, Pan, Mute and the parameters of the track's plug-ins. Lanes
 *  that hold automation are marked. `sub` picks for that sub-lane instead of the main lane. */
export async function paramMenu(t: Track, sub = -1): Promise<MenuItem[]> {
  const cur = sub < 0 ? shownParam(t) : (t.auto_lanes?.[sub] ?? VOLUME);
  const pick = (p: AutoParam) => {
    if (sub < 0) return showParam(t, p);
    const lanes = [...(t.auto_lanes ?? [])];
    lanes[sub] = p;
    setSubLanes(t, lanes);
  };
  const item = (p: AutoParam, label = paramLabel(p)): MenuItem => ({
    label,
    checked: sameParam(cur, p),
    hint: laneOf(t, p)?.points.length ? "●" : undefined,
    onClick: () => pick(p),
  });
  const project = getState().snap?.project;
  const items: MenuItem[] = baseParams(t).map((p) => {
    if (p.type !== "send") return item(p);
    const aux = project?.tracks.find((x) => x.id === t.sends[p.value]?.bus);
    return item(p, `Send ${p.value + 1} → ${aux?.name ?? "Bus"}`);
  });
  // The built-in synth has no parameters; built-in effects list theirs.
  const slots = [t.instrument, ...t.inserts].filter((s) => s && s.plugin_id !== BUILTIN_SYNTH);
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

/** A drag in progress: the points to draw instead of one lane's. */
let preview: { track: Id; param: AutoParam; points: AutoPoint[] } | null = null;
/** The point the last click added, and the lane before it: a double-click on a curved
 *  segment (whose first click adds a point) puts the lane back with the segment straight. */
let lastAdd: { track: Id; param: AutoParam; pos: number; before: AutoPoint[]; onCurve: boolean } | null = null;
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

function pointsOf(t: Track, param: AutoParam): AutoPoint[] {
  if (preview && preview.track === t.id && sameParam(preview.param, param)) return preview.points;
  return laneOf(t, param)?.points ?? [];
}

/** The rows the automation view draws: every visible track, then the Output track. */
function laneRows(project: Project, layout: Layout): LaneRow[] {
  const rows: LaneRow[] = [];
  project.tracks.forEach((track, i) => {
    if (!layout.heights[i]) return;
    const top = layout.tops[i];
    rows.push({ track, top, main: layout.heights[i] - layout.takeRows[i], subsTop: top + layout.heights[i], sub: layout.subLane[i] });
  });
  const o = layout.output;
  if (o) rows.push({ track: outputTrack(project), top: o.top, main: o.height, subsTop: o.top + o.height, sub: o.subLane });
  return rows;
}

function boxesOf(project: Project, layout: Layout): LaneBox[] {
  return laneBoxes(laneRows(project, layout), shownParam);
}

/** Strokes a lane's curve: lines between points (sampled where a segment is curved), held
 *  out to the edges. */
function strokeLane(ctx: CanvasRenderingContext2D, pts: AutoPoint[], px: (pos: number) => number, py: (v: number) => number, W: number) {
  ctx.moveTo(Math.min(0, px(pts[0].pos)), py(pts[0].value));
  ctx.lineTo(px(pts[0].pos), py(pts[0].value));
  for (let k = 1; k < pts.length; k++) {
    const a = pts[k - 1];
    const b = pts[k];
    const x0 = px(a.pos);
    const x1 = px(b.pos);
    // Off-screen curves are drawn straight; only their ends show.
    if (x1 < 0 || x0 > W) ctx.lineTo(x1, py(b.value));
    else for (const s of segmentSamples(a, x0, x1)) ctx.lineTo(s.x, py(a.value + (b.value - a.value) * s.f));
  }
  ctx.lineTo(Math.max(W, px(pts[pts.length - 1].pos)), py(pts[pts.length - 1].value));
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

function drawLane(ctx: CanvasRenderingContext2D, box: LaneBox, y: number, pps: number, left: number, W: number) {
  const { track: t, param } = box;
  const pts = pointsOf(t, param);
  const [top, bottom] = bounds(y, box.height);
  const py = (val: number) => bottom - toUnit(param, val) * (bottom - top);
  const px = (pos: number) => pos * pps - left;
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
  strokeLane(ctx, pts, px, py, W);
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
}

/** Draws every visible track's lanes: the main one over its (dimmed) regions, the extra ones
 *  below it, and the Output track's. */
export function drawAutomation(ctx: CanvasRenderingContext2D, v: AutoView) {
  const { project, layout, pps, vp } = v;
  const W = vp.width;
  const H = vp.height;
  project.tracks.forEach((_, i) => {
    const th = layout.heights[i];
    const y = layout.tops[i] - vp.top;
    if (th === 0 || y > H || y + th < 0) return;
    // Regions stay visible but recede, as in Logic.
    ctx.fillStyle = theme.region === "light" ? "rgba(255,255,255,0.5)" : "rgba(0,0,0,0.4)";
    ctx.fillRect(0, y, W, th - 1);
  });
  for (const box of boxesOf(project, layout)) {
    const y = box.top - vp.top;
    if (y > H || y + box.height < 0) continue;
    // Sub-lanes and the Output track have no regions: they get a lane of their own.
    if (box.sub >= 0 || isOutput(box.track)) {
      ctx.fillStyle = theme.lane;
      ctx.globalAlpha = 0.7;
      ctx.fillRect(0, y, W, box.height - 1);
      ctx.globalAlpha = 1;
    }
    drawLane(ctx, box, y, pps, vp.left, W);
  }
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
  /** The lane under the pointer. */
  box: LaneBox;
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
  const box = boxAt(boxesOf(v.project, v.layout), y);
  if (!box) return null;
  const { track, param } = box;
  const points = laneOf(track, param)?.points ?? [];
  const [top, bottom] = bounds(box.top, box.height);
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
  return { track, box, param, points, x, y, top, bottom, point };
}

const unitAt = (h: { top: number; bottom: number }, y: number) => (h.bottom - y) / Math.max(1, h.bottom - h.top);

/** The curve's height on screen at content x, for a hit's lane. */
function curveY(h: Hit, pts: AutoPoint[], x: number, pps: number): number | null {
  const v = valueAt(pts, x / pps);
  return v === null ? null : h.bottom - toUnit(h.param, v) * (h.bottom - h.top);
}

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

/** Keeps drawing a drag's points until the lane's new snapshot arrives. */
function endPreview(v: AutoInput) {
  const done = () => {
    if (inflight) return void setTimeout(done, 16);
    preview = null;
    v.redraw();
  };
  done();
}

/** Ctrl-Shift-drag (or Alt-Shift-drag) on a segment bends it into a curve, as in Logic: the
 *  line follows the pointer up and down. */
function bendSegment(e: React.MouseEvent, h: Hit, v: AutoInput): boolean {
  const pos = h.x / v.pps;
  const seg = segmentAt(h.points, pos);
  if (seg < 0) return false;
  const a = h.points[seg];
  const b = h.points[seg + 1];
  if (a.value === b.value) return false;
  const t = (pos - a.pos) / (b.pos - a.pos);
  selection = null;
  let result = h.points;
  let moved = false;
  const move = (ev: MouseEvent) => {
    moved = true;
    const c = hitPoint(ev, v);
    // In the parameter's own units, so the curve passes through the value under the pointer.
    const val = fromUnitExact(h.param, unitAt(h, c.y));
    const curve = curveThrough(t, (val - a.value) / (b.value - a.value));
    result = withCurve(h.points, seg, curve);
    preview = { track: h.track.id, param: h.param, points: result };
    hover = { track: h.track.id, x: h.x, y: c.y, text: curve ? `Curve ${curve > 0 ? "+" : ""}${Math.round(curve * 100)}` : "Linear" };
    sendLane(h.track.id, h.param, result);
    v.redraw();
  };
  const up = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    hover = null;
    if (moved) sendLane(h.track.id, h.param, result);
    endPreview(v);
  };
  e.preventDefault();
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
  return true;
}

/** `fromUnit` without the rounding, for following the pointer smoothly. */
function fromUnitExact(p: AutoParam, u: number): number {
  u = Math.min(1, Math.max(0, u));
  switch (p.type) {
    case "volume":
    case "send":
      return faderToDb(u);
    case "pan":
      return u * 2 - 1;
    default:
      return u;
  }
}

/** A press on a lane while the automation view is on. */
export function automationMouseDown(e: React.MouseEvent, v: AutoInput) {
  const h = hitTest(e, v);
  if (!h) {
    selection = null;
    v.redraw();
    return;
  }
  if (!isOutput(h.track) && h.track.id !== getState().snap?.selected) act("select_track", { id: h.track.id });
  const snap = (pos: number) => Math.max(0, Math.round(snapPos(v.project, pos, getState().snapMode, getState().pxPerBeat)));
  const sameLane = selection && selection.track === h.track.id && sameParam(selection.param, h.param);

  if (h.point < 0 && e.shiftKey && (e.ctrlKey || e.altKey) && bendSegment(e, h, v)) return;

  if (h.point >= 0) {
    const grabbed = h.points[h.point];
    // Double-click or Alt-click deletes a point. The second click of a double-click on a
    // curved segment lands on the point its first click added: that straightens the segment.
    if (e.altKey || e.detail >= 2) {
      selection = null;
      const add = lastAdd;
      lastAdd = null;
      if (e.detail >= 2 && add && add.track === h.track.id && sameParam(add.param, h.param) && add.pos === grabbed.pos && add.onCurve) {
        sendLane(h.track.id, h.param, withCurve(add.before, segmentAt(add.before, add.pos), 0));
        return;
      }
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
      result = h.points.map((p, k) => (picked[k] ? { ...p, pos: Math.max(0, p.pos + dPos), value: fromUnit(h.param, toUnit(h.param, p.value) + dU) } : p));
      result.sort((a, b) => a.pos - b.pos);
      preview = { track: h.track.id, param: h.param, points: result };
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
      endPreview(v);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    return;
  }

  // The second click of a double-click whose first one added a point that hasn't arrived yet.
  const add = lastAdd;
  if (e.detail >= 2 && add && add.track === h.track.id && sameParam(add.param, h.param)) {
    lastAdd = null;
    selection = null;
    sendLane(h.track.id, h.param, add.onCurve ? withCurve(add.before, segmentAt(add.before, add.pos), 0) : add.before);
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
    const pos = snap(h.x / v.pps);
    // A point added inside a curved segment keeps both halves bent the same way.
    const seg = segmentAt(h.points, pos);
    const curve = seg >= 0 ? h.points[seg].curve : undefined;
    const p: AutoPoint = { pos, value: fromUnit(h.param, unitAt(h, h.y)), ...(curve ? { curve } : {}) };
    const cy = curveY(h, h.points, h.x, v.pps);
    lastAdd = { track: h.track.id, param: h.param, pos, before: h.points, onCurve: !!curve && cy !== null && Math.abs(cy - h.y) <= HIT_R + 2 };
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
    // Over a segment with the bend keys held: the curve tool's cursor.
    v.el.style.cursor = e.shiftKey && (e.ctrlKey || e.altKey) && segmentAt(h.points, h.x / v.pps) >= 0 ? "ns-resize" : "crosshair";
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
  const t = stripOf(s.snap?.project, selection.track);
  const lane = t && laneOf(t, selection.param);
  if (!lane) return false;
  const gone = selection.pos;
  selection = null;
  sendLane(t.id, lane.param, lane.points.filter((p) => !gone.has(p.pos)));
  return true;
}

/** Right-click on the lanes in the automation view: for the lane under the pointer. */
export function automationMenu(e: { clientX: number; clientY: number }, v: AutoInput): MenuItem[] {
  const hide: MenuItem = { label: "Hide Automation", hint: keyHint("automation"), onClick: () => setState({ showAutomation: false }) };
  const used: MenuItem = { label: "Show Used Automation", hint: keyHint("used-automation"), onClick: showUsedAutomation };
  const h = hitTest(e, v);
  if (!h) return [used, hide];
  const { track: t, param: p, box } = h;
  const n = selection && selection.track === t.id && sameParam(selection.param, p) ? selection.pos.size : 0;
  const seg = segmentAt(h.points, h.x / v.pps);
  const curved = seg >= 0 && !!h.points[seg].curve;
  return [
    { label: n > 1 ? "Delete Selected Points" : "Delete Selected Point", disabled: !n, hint: keyHint("delete-regions"), onClick: () => deleteSelectedPoints() },
    { label: "Straighten Segment", disabled: !curved, onClick: () => sendLane(t.id, p, withCurve(h.points, seg, 0)) },
    { label: `Delete ${paramLabel(p)} Automation`, disabled: !laneOf(t, p)?.points.length, onClick: () => act("clear_automation", { track: t.id, param: p }) },
    { label: "Delete All Automation on Track", disabled: !t.automation.some((l) => l.points.length), onClick: () => act("clear_automation", { track: t.id, param: null }) },
    { separator: true },
    { label: "Show Another Lane", onClick: () => addLane(t) },
    ...(box.sub >= 0 ? [{ label: "Close Lane", onClick: () => closeLane(t, box.sub) }] : []),
    used,
    hide,
  ];
}
