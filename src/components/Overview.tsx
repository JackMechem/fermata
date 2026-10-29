// The overview strip under the Tracks area, in place of its horizontal scroll bar (like
// Ardour's Summary): the whole project drawn small, the playhead, and a box around the part
// shown in the Tracks area. Drag the box to scroll, drag its edges to zoom, click elsewhere to
// centre the view there. The mapping maths is in overview.ts.
import { useEffect, useLayoutEffect, useMemo, useRef } from "react";
import type { Project, Region, Track } from "../api";
import { getState, setState, useStore } from "../store";
import { framePos, onFrame } from "../frame";
import { useLayout, type Layout } from "../layout";
import { regionEnd } from "../loops";
import { samplesPerBeat } from "../time";
import { mix, theme } from "../theme";
import { pixelRatio } from "../uiScale";
import { scrollTo, viewport } from "../view";
import { timeWheel } from "../wheel";
import { openMenu } from "./Menu";
import {
  OVERVIEW_BAND,
  anchoredLeft,
  centreOn,
  dragScroll,
  hitBox,
  overviewRows,
  overviewSpan,
  timeToX,
  visibleBox,
  wheelZoom,
  xToTime,
  zoomFromEdge,
  type BoxPart,
  type Rect,
  type StripMap,
} from "../overview";

// ---- showing and hiding (View › Show Overview), remembered per machine -----------------------

const KEY = "fermata.showOverview";

export function savedShowOverview(): boolean {
  try {
    return localStorage.getItem(KEY) !== "0";
  } catch {
    return true;
  }
}

export function toggleOverview() {
  const on = !getState().showOverview;
  setState({ showOverview: on });
  try {
    localStorage.setItem(KEY, on ? "1" : "0");
  } catch {
    // Storage can be unavailable; the choice still applies for this session.
  }
}

/**
 * The strip replaces the Tracks area's horizontal scroll bar, which CSS hides while it is
 * shown (`.with-overview`). WebKit only re-reads scroll bar styles when the scroller's layout
 * changes, so give it a nudge whenever the strip comes or goes.
 */
function refreshScrollbars() {
  const el = document.querySelector(".arrange-scroll") as HTMLElement | null;
  if (!el) return;
  const { scrollLeft, scrollTop } = el;
  el.style.overflow = "hidden";
  void el.offsetWidth;
  el.style.overflow = "";
  el.scrollLeft = scrollLeft;
  el.scrollTop = scrollTop;
}

// ---- drawing ----------------------------------------------------------------------------------

/** The end of the project's content: its last region (loops included), marker or cycle range.
 *  `regions` is the end of the last region alone, drawn as the project end. */
function contentEnds(p: Project): { regions: number; all: number } {
  let regions = 0;
  for (const t of p.tracks) for (const r of t.regions) regions = Math.max(regions, regionEnd(r));
  let all = regions;
  for (const m of p.markers) all = Math.max(all, m.pos + (m.length ?? 0));
  if (p.cycle.end > p.cycle.start) all = Math.max(all, p.cycle.end);
  return { regions, all };
}

/** Each stack's subtracks (all levels), for drawing a closed stack's row. */
function stackContents(tracks: Track[]): Map<number, Track[]> {
  const out = new Map<number, Track[]>();
  const byId = new Map(tracks.map((t) => [t.id, t]));
  for (const t of tracks) {
    for (let p = t.parent != null ? byId.get(t.parent) : undefined, d = 0; p && d < 32; p = p.parent != null ? byId.get(p.parent) : undefined, d++) {
      const list = out.get(p.id) ?? [];
      list.push(t);
      out.set(p.id, list);
    }
  }
  return out;
}

/** A region's bar colour: its colour (or its track's), softened toward the lane so a strip of
 *  saturated colours stays calm. Muted regions are grey, as in the Tracks area. */
function barColor(r: Region, t: Track): string {
  const c = r.muted ? "#6c6c70" : r.color ?? t.color;
  return theme.region === "light" ? mix(c, "#ffffff", 0.12) : mix(c, theme.lane, 0.28);
}

interface Geometry {
  /** Strip size in CSS px and the device pixel ratio. */
  W: number;
  H: number;
  dpr: number;
  map: StripMap;
}

/**
 * The region layer: lanes, the area past the project end, the cycle range, markers and
 * every region as a bar on its track's row. Drawn in device pixels so hairlines stay sharp.
 */
function drawStatic(ctx: CanvasRenderingContext2D, g: Geometry, p: Project, layout: Layout, regionsEnd: number) {
  const { dpr, map } = g;
  const DW = ctx.canvas.width;
  const DH = ctx.canvas.height;
  const X = (t: number) => Math.round(timeToX(t, map.span, map.width) * dpr);
  const px = Math.max(1, Math.round(dpr));
  const light = theme.region === "light";
  ctx.setTransform(1, 0, 0, 1, 0, 0);

  // The project, on the lane colour; the timeline past its end a shade darker.
  ctx.fillStyle = theme.lane;
  ctx.fillRect(0, 0, DW, DH);
  const endX = regionsEnd > 0 ? X(regionsEnd) : DW;
  if (endX < DW) {
    ctx.fillStyle = theme.arrangeBg;
    ctx.fillRect(endX, 0, DW - endX, DH);
    ctx.fillStyle = light ? "rgba(0,0,0,0.035)" : "rgba(0,0,0,0.22)";
    ctx.fillRect(endX, 0, DW - endX, DH);
  }

  // Cycle range: a thin bar in the top band (yellow when on, grey when off), and the same
  // faint tint the tracks get when it's on.
  const c = p.cycle;
  if (c.end > c.start) {
    const x0 = X(c.start);
    const w = Math.max(px, X(c.end) - x0);
    if (c.enabled) {
      ctx.fillStyle = theme.cycleTint;
      ctx.fillRect(x0, 0, w, DH);
    }
    ctx.fillStyle = c.enabled ? theme.cycleOn : theme.cycleOff;
    ctx.fillRect(x0, 0, w, Math.round(2 * dpr));
  }

  // Regions. Rows thinner than a pixel still get a pixel, so a big session reads as texture
  // rather than vanishing; rows with room are inset to leave a gap between tracks.
  const rows = overviewRows(layout.tops, layout.heights, layout.total, map.rowsTop, map.rowsH);
  const stacks = stackContents(p.tracks);
  p.tracks.forEach((t, i) => {
    if (!layout.heights[i]) return;
    const y0 = Math.round(rows[i].y * dpr);
    const rh = Math.max(1, Math.round((rows[i].y + rows[i].h) * dpr) - y0);
    const inset = rh >= 5 ? Math.max(1, Math.round(rh * 0.16)) : 0;
    const y = y0 + inset;
    const h = Math.max(1, rh - inset * 2);
    const alpha = t.hidden ? 0.45 : 1;
    const bars = (owner: Track, a: number) => {
      for (const r of owner.regions) {
        const x0 = X(r.start);
        const own = X(r.start + r.length);
        const end = X(regionEnd(r));
        if (end < 0 || x0 > DW) continue;
        ctx.fillStyle = barColor(r, owner);
        // A one-pixel gap after each region so neighbours stay apart when there's room.
        const gap = own - x0 > 3 * px ? px : 0;
        ctx.globalAlpha = a;
        ctx.fillRect(x0, y, Math.max(px, own - x0 - gap), h);
        if (end > own) {
          ctx.globalAlpha = a * 0.45;
          ctx.fillRect(own, y, Math.max(px, end - own - gap), h);
        }
      }
    };
    bars(t, alpha);
    // A stack's row shows its subtracks' regions, like its lane in the Tracks area; faintly
    // when the stack is open and they have rows of their own.
    if (t.kind === "folder" || t.kind === "summing") for (const s of stacks.get(t.id) ?? []) bars(s, alpha * (t.collapsed ? 1 : 0.35));
  });
  ctx.globalAlpha = 1;

  // Markers: ticks in the top band, in their colour.
  for (const m of p.markers) {
    ctx.fillStyle = m.color ?? theme.rulerText;
    ctx.fillRect(X(m.pos), 0, px, Math.round(OVERVIEW_BAND * dpr));
  }

  // The project end.
  if (endX < DW) {
    ctx.fillStyle = theme.rulerTick;
    ctx.fillRect(endX, 0, px, DH);
  }
}

/** The visible-area box and the playhead over the region layer. Everything outside the box is
 *  veiled, so the box reads as the lit part, as in Ardour. */
function drawOverlay(ctx: CanvasRenderingContext2D, layer: HTMLCanvasElement, box: [number, number, number, number], phx: number, dpr: number) {
  const DW = ctx.canvas.width;
  const DH = ctx.canvas.height;
  const px = Math.max(1, Math.round(dpr));
  const light = theme.region === "light";
  const [x0, y0, x1, y1] = box;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.drawImage(layer, 0, 0);

  ctx.fillStyle = light ? "rgba(0,0,0,0.07)" : "rgba(0,0,0,0.3)";
  ctx.fillRect(0, 0, x0, DH);
  ctx.fillRect(x1, 0, DW - x1, DH);
  ctx.fillRect(x0, 0, x1 - x0, y0);
  ctx.fillRect(x0, y1, x1 - x0, DH - y1);

  ctx.fillStyle = mix(theme.text, theme.text, 0, light ? 0.05 : 0.07);
  ctx.fillRect(x0, y0, x1 - x0, y1 - y0);
  ctx.fillStyle = mix(theme.text, theme.text, 0, light ? 0.55 : 0.6);
  ctx.fillRect(x0, y0, x1 - x0, px);
  ctx.fillRect(x0, y1 - px, x1 - x0, px);
  ctx.fillRect(x0, y0, px, y1 - y0);
  ctx.fillRect(x1 - px, y0, px, y1 - y0);

  if (phx >= 0 && phx < DW) {
    ctx.fillStyle = theme.playhead;
    ctx.fillRect(phx, 0, px, DH);
  }
}

// ---- component --------------------------------------------------------------------------------

interface Drag {
  part: BoxPart;
  /** Pointer and scroll position where the drag started (or where a click outside centred it). */
  x: number;
  y: number;
  left: number;
  top: number;
  /** The mapping is frozen while dragging so the strip doesn't rescale under the pointer. */
  map: StripMap;
  /** For edge drags: the other edge's time, and the pointer's distance from the dragged edge. */
  fixed: number;
  offset: number;
}

export function Overview() {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const project = useStore((s) => s.snap?.project);
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  const layout = useLayout();
  // Redraw on a theme or interface size change.
  useStore((s) => s.themeId);
  useStore((s) => s.uiScale);

  const ends = useMemo(() => (project ? contentEnds(project) : { regions: 0, all: 0 }), [project]);
  const spb = project ? samplesPerBeat(project) : 1;
  const pps = pxPerBeat / spb;
  // Some room past the end: two bars, or 6% of a long project.
  const pad = project ? Math.max(ends.all * 0.06, 2 * project.sig_num * spb) : 0;

  const live = useRef({ project, layout, pps, spb, ends, pad });
  live.current = { project, layout, pps, spb, ends, pad };
  const size = useRef({ W: 0, H: 0 });
  const dragRef = useRef<Drag | null>(null);
  const layerRef = useRef<HTMLCanvasElement | null>(null);
  // The region layer is redrawn when a render (new project, layout, zoom or theme) or a change
  // of strip size, scale or span calls for it; the box and playhead are cheap, every frame
  // something moved.
  const staticKey = useRef("");
  const overlayKey = useRef("");
  const dirty = useRef(true);

  useLayoutEffect(() => {
    dirty.current = true;
  });

  /** The mapping as things stand (frozen during a drag). */
  const geometry = (): Geometry | null => {
    const { project, layout, pps, ends, pad } = live.current;
    const { W, H } = size.current;
    if (!project || W <= 0 || H <= 0 || pps <= 0) return null;
    const vp = viewport();
    const span = overviewSpan(ends.all, (vp.left + vp.width) / pps, pad);
    const map = dragRef.current?.map ?? { span, width: W, total: layout.total, rowsTop: OVERVIEW_BAND, rowsH: H - OVERVIEW_BAND - 1 };
    return { W, H, dpr: pixelRatio(), map };
  };

  const draw = () => {
    const cv = canvasRef.current;
    const g = geometry();
    const { project, layout, pps, ends } = live.current;
    if (!cv || !g || !project) return;
    const DW = Math.round(g.W * g.dpr);
    const DH = Math.round(g.H * g.dpr);
    if (cv.width !== DW || cv.height !== DH) {
      cv.width = DW;
      cv.height = DH;
      cv.style.width = `${g.W}px`;
      cv.style.height = `${g.H}px`;
    }
    const layer = (layerRef.current ??= document.createElement("canvas"));
    const sk = `${DW}x${DH}|${g.map.span}|${g.map.total}`;
    if (dirty.current || sk !== staticKey.current) {
      dirty.current = false;
      staticKey.current = sk;
      overlayKey.current = "";
      layer.width = DW;
      layer.height = DH;
      drawStatic(layer.getContext("2d")!, g, project, layout, ends.regions);
    }
    const b = visibleBox(viewport(), pps, g.map);
    const box: [number, number, number, number] = [
      Math.round(b.x * g.dpr),
      Math.round(b.y * g.dpr),
      Math.round((b.x + b.w) * g.dpr),
      Math.round((b.y + b.h) * g.dpr),
    ];
    const phx = Math.round(timeToX(framePos(), g.map.span, g.map.width) * g.dpr);
    const ok = `${sk}|${box.join(",")}|${phx}`;
    if (ok === overlayKey.current) return;
    overlayKey.current = ok;
    drawOverlay(cv.getContext("2d")!, layer, box, phx, g.dpr);
  };
  const drawRef = useRef(draw);
  drawRef.current = draw;

  // One check per display frame covers the playhead, scrolling, zoom and resizing; it draws
  // only when something on the strip moved.
  useEffect(() => onFrame(() => drawRef.current()), []);

  useEffect(() => {
    const el = wrapRef.current!;
    const ro = new ResizeObserver(() => {
      size.current = { W: el.clientWidth, H: el.clientHeight };
    });
    ro.observe(el);
    refreshScrollbars();
    // Wheel: scrolls through time (Shift: the tracks); Ctrl/Cmd zooms around the time under
    // the pointer, or the middle of the view when the pointer is outside it.
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      const g = geometry();
      const { project, pps, spb } = live.current;
      if (!g || !project) return;
      const vp = viewport();
      if (e.ctrlKey || e.metaKey) {
        const x = e.clientX - el.getBoundingClientRect().left;
        const t = xToTime(x, g.map.span, g.map.width);
        const inView = t * pps >= vp.left && t * pps <= vp.left + vp.width;
        const anchor = inView ? t : (vp.left + vp.width / 2) / pps;
        const cx = anchor * pps - vp.left;
        const next = wheelZoom(getState().pxPerBeat, e.deltaY);
        setState({ pxPerBeat: next });
        requestAnimationFrame(() => scrollTo(anchoredLeft(anchor, cx, next / spb)));
        return;
      }
      const { dx, dy } = timeWheel(e, vp.width);
      scrollTo(Math.max(0, vp.left + dx), Math.max(0, vp.top + dy));
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => {
      ro.disconnect();
      el.removeEventListener("wheel", wheel);
      refreshScrollbars();
    };
  }, []);

  const local = (e: { clientX: number; clientY: number }) => {
    const r = wrapRef.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const boxNow = (g: Geometry): Rect => visibleBox(viewport(), live.current.pps, g.map);

  const onMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const g = geometry();
    if (!g) return;
    e.preventDefault();
    const { x, y } = local(e);
    const vp = viewport();
    const { pps } = live.current;
    const b = boxNow(g);
    const part = hitBox(b, x, y);
    let left = vp.left;
    let top = vp.top;
    // A click outside the box centres the view there (as in Ardour), and the press carries
    // on as a drag of the box.
    if (part === "outside") {
      ({ left, top } = centreOn(x, y, vp, pps, g.map));
      scrollTo(left, top);
    }
    const fixed = part === "left" ? (vp.left + vp.width) / pps : vp.left / pps;
    const offset = part === "left" ? x - b.x : part === "right" ? x - (b.x + b.w) : 0;
    dragRef.current = { part, x, y, left, top, map: g.map, fixed, offset };
    const wrap = wrapRef.current!;
    wrap.style.cursor = part === "left" || part === "right" ? "ew-resize" : "grabbing";

    const move = (ev: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      const p = local(ev);
      const { pps, spb } = live.current;
      if (d.part === "left" || d.part === "right") {
        const t = Math.max(0, xToTime(p.x - d.offset, d.map.span, d.map.width));
        const z = zoomFromEdge(d.part, d.fixed, t, viewport().width, spb);
        setState({ pxPerBeat: z.pxPerBeat });
        // Scroll once the Tracks area has re-rendered at the new width.
        requestAnimationFrame(() => scrollTo((z.start * z.pxPerBeat) / spb));
        return;
      }
      const { dLeft, dTop } = dragScroll(p.x - d.x, p.y - d.y, pps, d.map);
      scrollTo(Math.max(0, d.left + dLeft), Math.max(0, d.top + dTop));
    };
    const up = () => {
      dragRef.current = null;
      wrap.style.cursor = "";
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  // The pointer shows what a press would do.
  const onHover = (e: React.MouseEvent) => {
    if (dragRef.current) return;
    const g = geometry();
    if (!g) return;
    const { x, y } = local(e);
    const part = hitBox(boxNow(g), x, y);
    wrapRef.current!.style.cursor = part === "left" || part === "right" ? "ew-resize" : part === "inside" ? "grab" : "";
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    openMenu(e.clientX, e.clientY, [{ label: "Show Overview", checked: true, onClick: toggleOverview }]);
  };

  return (
    <div className="overview" ref={wrapRef} onMouseDown={onMouseDown} onMouseMove={onHover} onContextMenu={onContextMenu}>
      <canvas ref={canvasRef} />
    </div>
  );
}
