import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { act } from "../api";
import { getState, useStore } from "../store";
import { coalesced, framePos, onFrame } from "../frame";
import { samplesPerBeat, snapPos } from "../time";
import { tempoMap } from "../tempo";
import { pixelRatio } from "../uiScale";
import { mix, theme } from "../theme";
import { onViewport, RULER_H, viewport } from "../view";
import { createMarker, drawMarkers, markerAt, markerLaneMenu, markerMenu, renameMarker } from "../markers";
import { openMenu } from "./Menu";

const CYCLE_H = 12;
/** Bottom of the bar numbers; the Autopunch strip and the marker lane are below. */
const BARS_H = 30;
const PUNCH_H = 6;
const MARKERS_Y = BARS_H + PUNCH_H;

type Strip = "cycle" | "punch";

export function Ruler() {
  const ref = useRef<HTMLCanvasElement>(null);
  const project = useStore((s) => s.snap?.project);
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  useStore((s) => s.themeId);
  useStore((s) => s.uiScale);
  const live = useRef({ project, pxPerBeat });
  live.current = { project, pxPerBeat };
  const preview = useRef<{ strip: Strip; start: number; end: number } | null>(null);
  const movingMarker = useRef<{ id: number; pos: number } | null>(null);

  const draw = () => {
    const cv = ref.current;
    const { project, pxPerBeat } = live.current;
    if (!cv || !project) return;
    const vp = viewport();
    const dpr = pixelRatio();
    const W = vp.width;
    if (cv.width !== Math.round(W * dpr)) {
      cv.width = Math.round(W * dpr);
      cv.height = Math.round(RULER_H * dpr);
      cv.style.width = `${W}px`;
      cv.style.height = `${RULER_H}px`;
    }
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = theme.ruler;
    ctx.fillRect(0, 0, W, RULER_H);
    ctx.fillStyle = theme.rulerCycleZone;
    ctx.fillRect(0, 0, W, CYCLE_H);
    ctx.fillRect(0, BARS_H, W, RULER_H - BARS_H);
    ctx.fillStyle = theme.rulerEdge;
    ctx.fillRect(0, RULER_H - 1, W, 1);
    ctx.fillRect(0, MARKERS_Y, W, 1);

    const pps = pxPerBeat / samplesPerBeat(project);
    const map = tempoMap(project);

    // Cycle and Autopunch strips, dimmed while they're off.
    for (const strip of ["cycle", "punch"] as Strip[]) {
      const range = strip === "cycle" ? project.cycle : project.punch;
      const dragging = preview.current?.strip === strip;
      const c = dragging ? preview.current : range.end > range.start ? range : null;
      if (!c) continue;
      const x0 = c.start * pps - vp.left;
      const x1 = c.end * pps - vp.left;
      const on = dragging || range.enabled;
      if (strip === "punch") ctx.fillStyle = on ? theme.red : mix(theme.red, theme.rulerCycleZone, 0.65);
      else ctx.fillStyle = on ? theme.cycleOn : theme.cycleOff;
      ctx.beginPath();
      if (strip === "cycle") ctx.roundRect(x0, 1.5, x1 - x0, CYCLE_H - 3, 2);
      else ctx.roundRect(x0, BARS_H + 1, x1 - x0, PUNCH_H - 2, 1.5);
      ctx.fill();
    }

    ctx.font = "500 11px Inter, 'SF Pro Text', system-ui, sans-serif";
    ctx.textBaseline = "alphabetic";
    // Bars through the tempo map, so their numbers follow tempo and signature changes.
    for (const b of map.bars(Math.max(0, vp.left / pps), (vp.left + W) / pps)) {
      const x = Math.round(b.start * pps - vp.left) + 0.5;
      const barPx = (b.end - b.start) * pps;
      // Skip bar labels when they would collide.
      const every = barPx >= 34 ? 1 : barPx >= 16 ? 2 : barPx >= 8 ? 4 : 8;
      if ((b.bar - 1) % every === 0) {
        ctx.fillStyle = theme.rulerTick;
        ctx.fillRect(x, CYCLE_H + 1, 1, BARS_H - CYCLE_H - 2);
        ctx.fillStyle = theme.rulerText;
        ctx.fillText(String(b.bar), x + 3, BARS_H - 6);
      } else {
        ctx.fillStyle = theme.rulerMinor;
        ctx.fillRect(x, BARS_H - 8, 1, 7);
      }
      if (barPx / b.num >= 10) {
        ctx.fillStyle = theme.rulerMinor;
        for (let k = 1; k < b.num; k++) {
          ctx.fillRect(Math.round(map.sampleAt(b.startQ + (k * 4) / b.den) * pps - vp.left + 0.5), BARS_H - 5, 1, 4);
        }
      }
    }
    drawMarkers(ctx, project, pps, vp.left, W, MARKERS_Y + 1, movingMarker.current);

  };

  const drawRef = useRef(draw);
  drawRef.current = draw;
  const [requestDraw] = useState(() => coalesced(() => drawRef.current()));
  useLayoutEffect(() => draw());
  // The playhead handle is a DOM element moved every frame; the canvas only repaints when the
  // view, zoom or cycle changes.
  const handleRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const a = onViewport(() => requestDraw());
    let last = "";
    const b = onFrame((now) => {
      const { project, pxPerBeat } = live.current;
      const el = handleRef.current;
      if (!project || !el) return;
      const x = (framePos(now) * pxPerBeat) / samplesPerBeat(project) - viewport().left;
      const tf = `translate3d(${x.toFixed(1)}px,0,0)`;
      if (tf !== last) {
        last = tf;
        el.style.transform = tf;
        el.style.visibility = x < -6 || x > viewport().width + 6 ? "hidden" : "visible";
      }
    });
    return () => {
      a();
      b();
    };
  }, []);

  const onMouseDown = (e: React.MouseEvent) => {
    const { project, pxPerBeat } = live.current;
    if (!project || e.button !== 0) return;
    const rect = ref.current!.getBoundingClientRect();
    const pps = pxPerBeat / samplesPerBeat(project);
    const toSample = (cx: number) => (cx - rect.left + viewport().left) / pps;
    const y = e.clientY - rect.top;
    // The ruler always snaps, to beats at least, even with Snap off: locators and markers set
    // between beats are hardly ever wanted.
    const snap = (v: number) => Math.max(0, snapPos(project, v, getState().snapMode === "off" ? "beat" : getState().snapMode, pxPerBeat));
    const at = toSample(e.clientX);

    if (y >= MARKERS_Y) return markerDown(e, at, toSample, snap);
    if (y >= BARS_H) return stripDown(e, "punch", at, toSample, snap);
    if (y > CYCLE_H) {
      // Lower half: set and scrub the playhead.
      const locate = (s: number) => act("locate", { pos: Math.round(Math.max(0, snap(s))) });
      locate(at);
      const move = (ev: MouseEvent) => locate(toSample(ev.clientX));
      const up = () => {
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", up);
      };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", up);
      return;
    }

    stripDown(e, "cycle", at, toSample, snap);
  };

  /** The cycle strip (top) and the Autopunch strip (below the bar numbers): drag to set or
   *  move the range or its edges, click to switch it on or off, as in Logic. */
  const stripDown = (e: React.MouseEvent, strip: Strip, at: number, toSample: (cx: number) => number, snap: (v: number) => number) => {
    const { project, pxPerBeat } = live.current;
    if (!project) return;
    const pps = pxPerBeat / samplesPerBeat(project);
    const c = strip === "cycle" ? project.cycle : project.punch;
    const commit = (enabled: boolean, start: number, end: number) => act(strip === "cycle" ? "set_cycle" : "set_punch", { enabled, start, end });
    const has = c.end > c.start;
    const edgePx = 6;
    const sx = c.start * pps;
    const ex = c.end * pps;
    const ax = at * pps;
    let mode: "new" | "move" | "start" | "end" = "new";
    if (has && Math.abs(ax - sx) < edgePx) mode = "start";
    else if (has && Math.abs(ax - ex) < edgePx) mode = "end";
    else if (has && ax > sx && ax < ex) mode = "move";
    const startX = e.clientX;
    let moved = false;
    const map = tempoMap(project);

    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < 3) return;
      moved = true;
      const cur = toSample(ev.clientX);
      let s = c.start;
      let en = c.end;
      if (mode === "new") {
        s = snap(Math.min(at, cur));
        en = snap(Math.max(at, cur));
        if (en <= s) en = s + map.samplesPerBarAt(s);
      } else if (mode === "move") {
        const d = snap(c.start + (cur - at)) - c.start;
        s = c.start + d;
        en = c.end + d;
      } else if (mode === "start") {
        s = Math.min(snap(cur), c.end - map.samplesPerBeatAt(c.end));
      } else {
        en = Math.max(snap(cur), c.start + map.samplesPerBeatAt(c.start));
      }
      preview.current = { strip, start: Math.max(0, s), end: Math.max(0, en) };
      draw();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const p = preview.current;
      preview.current = null;
      if (moved && p) {
        commit(true, Math.round(p.start), Math.round(p.end));
      } else if (mode === "move" || mode === "start" || mode === "end") {
        // A click on the strip toggles Cycle (or Autopunch), like Logic.
        commit(!c.enabled, c.start, c.end);
      } else {
        draw();
      }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  /** The marker lane: click a marker to go to it, drag it to move it (snapped), double-click
   *  to rename it; double-click the empty lane to create one there. */
  const markerDown = (e: React.MouseEvent, at: number, toSample: (cx: number) => number, snap: (v: number) => number) => {
    const { project, pxPerBeat } = live.current;
    if (!project) return;
    const pps = pxPerBeat / samplesPerBeat(project);
    const m = markerAt(at * pps);
    if (!m) {
      if (e.detail === 2) createMarker(snap(at));
      return;
    }
    if (e.detail === 2) {
      renameMarker(m);
      return;
    }
    const startX = e.clientX;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < 3) return;
      moved = true;
      movingMarker.current = { id: m.id, pos: Math.round(snap(m.pos + toSample(ev.clientX) - at)) };
      draw();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const mv = movingMarker.current;
      movingMarker.current = null;
      if (moved && mv && mv.pos !== m.pos) act("move_marker", { id: m.id, pos: mv.pos });
      else if (!moved) act("locate", { pos: m.pos });
      else draw();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const onContextMenu = (e: React.MouseEvent) => {
    const { project, pxPerBeat } = live.current;
    const rect = ref.current!.getBoundingClientRect();
    if (!project || e.clientY - rect.top < MARKERS_Y) return;
    e.preventDefault();
    const pps = pxPerBeat / samplesPerBeat(project);
    const at = (e.clientX - rect.left + viewport().left) / pps;
    const m = markerAt(at * pps);
    const snapped = Math.max(0, snapPos(project, at, getState().snapMode === "off" ? "beat" : getState().snapMode, pxPerBeat));
    openMenu(e.clientX, e.clientY, m ? markerMenu(m) : markerLaneMenu(snapped));
  };

  return (
    <div className="ruler-wrap">
      <canvas ref={ref} className="ruler" onMouseDown={onMouseDown} onContextMenu={onContextMenu} />
      <div className="ruler-playhead" ref={handleRef} style={{ ["--ph" as string]: theme.playhead }} />
    </div>
  );
}
