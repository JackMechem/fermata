// The Audio Track editor (bottom pane): the track's regions on the project timeline, drawn
// large, with Logic's Audio File Editor-style range editing done non-destructively. The state
// and the actions live in audioEdit.ts; this file draws and handles the pointer.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { act, call, type Id, type Project, type Region, type Track } from "../api";
import * as ed from "../audioEdit";
import { coalesced, framePos, onFrame } from "../frame";
import { laneMenu, regionMenu } from "../ops";
import { currentTick, getState, setState, useStore } from "../store";
import { theme } from "../theme";
import { formatDb, formatTime, lengthBbt as lengthAt, PEAK_BUCKET, samplesPerBeat, snapPos, toBbt } from "../time";
import { forEachGridLine, tempoMap } from "../tempo";
import { pixelRatio } from "../uiScale";
import { ensureWave, waveCache, type Wave } from "../waveform";
import { drawRegion, FADE_ZONE, regionCrossfades, setCrossfadeDrag } from "./Arrange";
import { draggedFadeOut, fadeHandles } from "../fades";
import { openMenu, openMenuAt, type MenuItem } from "./Menu";
import { fmt } from "../keymap";
import { onRevealPlayhead, revealLeft } from "../view";
import { DetachButton } from "./DetachButton";
import { moveAutomationAnswer } from "../moveAutomation";

/** Ruler height, with the cycle strip at its top. */
const RULER = 24;
const CYCLE_STRIP = 7;
/** Width of the dB scale column. */
const SCALE_W = 38;
/** Region name bar, as drawRegion draws it. */
const HEADER = 14;
const LANE_PAD = 3;
/** Closest zoom: past this the 32-frame overview only interpolates. */
const MAX_PPS = 0.5;
/** Keeps the scrolled width well inside what the browser can lay out. */
const MAX_CONTENT_PX = 8_000_000;
const DB_MARKS = [0, -1, -2, -3, -4.5, -6, -9, -12, -15, -18, -24, -30, -36, -42, -48, -60];

/** Vertical layout for a canvas `H` high: the lane, and the waveform body inside regions. */
function geometry(H: number) {
  const top = RULER + LANE_PAD;
  const h = Math.max(HEADER + 10, H - RULER - 2 * LANE_PAD);
  return { top, h, bodyTop: top + HEADER + 2, bodyH: h - HEADER - 4 };
}

/** How long the scrollable timeline is, like the arrange area: the regions plus a minute. */
function timelineFrames(p: Project): number {
  return Math.max(p.sample_rate * 60 * 6, ...p.tracks.flatMap((t) => t.regions.map((r) => r.start + r.length + p.sample_rate * 60)));
}

/** A length from `start` as bars, beats, divisions and ticks, counted from zero like Logic's
 *  length field (through the tempo map). */
function lengthBbt(p: Project, start: number, len: number): string {
  const b = lengthAt(p, start, len);
  return `${b.bar} ${b.beat} ${b.div} ${b.tick}`;
}

const bbt = (p: Project, pos: number) => {
  const b = toBbt(p, pos);
  return `${b.bar} ${b.beat} ${b.div} ${b.tick}`;
};

const dbGain = (db: number) => (db <= -96 ? 0 : Math.pow(10, db / 20));

/** First index in sorted `list` whose value is at least `v`. */
function lowerBound(list: number[], v: number): number {
  let lo = 0;
  let hi = list.length;
  while (lo < hi) {
    const m = (lo + hi) >> 1;
    if (list[m] < v) lo = m + 1;
    else hi = m;
  }
  return lo;
}

/** Red marks at the top and bottom of the waveform where the audio, after `gain`, reaches
 *  0 dBFS. Reads the same overview level drawWave uses, one value per pixel column. */
function drawClipMarks(ctx: CanvasRenderingContext2D, wave: Wave, offset: number, x: number, w: number, pps: number, gain: number, top: number, h: number, W: number) {
  if (gain <= 0) return;
  const limit = 0.999 / gain;
  if (limit > 1) return; // the overview stops at full scale, so nothing below it can clip
  const li = Math.max(0, Math.min(wave.levels.length - 1, Math.floor(Math.log2(1 / pps / wave.bucket))));
  const { min, max } = wave.levels[li];
  const bucket = wave.bucket * 2 ** li;
  const n = max.length;
  const x0 = Math.max(0, Math.floor(x));
  const x1 = Math.min(W, Math.ceil(x + w));
  ctx.fillStyle = theme.red;
  let run = -1;
  const flush = (px: number) => {
    if (run < 0) return;
    ctx.fillRect(run, top, px - run, 3);
    ctx.fillRect(run, top + h - 3, px - run, 3);
    run = -1;
  };
  for (let px = x0; px < x1; px++) {
    const f0 = offset + (px - x) / pps;
    let b = Math.max(0, Math.floor(f0 / bucket));
    const b1 = Math.min(n, Math.max(b + 1, Math.ceil((f0 + 1 / pps) / bucket)));
    let hot = false;
    for (; b < b1 && !hot; b++) hot = max[b] >= limit || (min ? -min[b] : max[b]) >= limit;
    if (hot && run < 0) run = px;
    else if (!hot) flush(px);
  }
  flush(x1);
}

function sizeCanvas(cv: HTMLCanvasElement, w: number, h: number, dpr: number) {
  if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) {
    cv.width = Math.round(w * dpr);
    cv.height = Math.round(h * dpr);
    cv.style.width = `${w}px`;
    cv.style.height = `${h}px`;
  }
  const ctx = cv.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return ctx;
}

type GrabKind = "move" | "left" | "right" | "fadeIn" | "fadeOut" | "select";

interface Drag {
  kind: Exclude<GrabKind, "select">;
  id: Id;
  start?: number;
  length?: number;
  fade?: number;
}

/** Scroll position kept while the editor is closed, like its zoom. */
let lastLeft = 0;

const clipInfos = new Map<Id, { frames: number; channels: number; sample_rate: number; path: string } | null>();

export function AudioEditor({ track, region }: { track: Track; region: Region | null }) {
  const project = useStore((s) => s.snap?.project);
  const selectedRegions = useStore((s) => s.selectedRegions);
  const themeId = useStore((s) => s.themeId);
  // Subscribed to without using the value: a change re-renders, and so redraws, the canvas
  // (the draw code reads these itself).
  useStore((s) => s.uiScale);
  useStore((s) => s.waveStyle);
  const pps = ed.useEd((s) => s.pps);
  const amp = ed.useEd((s) => s.amp);
  const showTransients = ed.useEd((s) => s.showTransients);
  ed.useEd((s) => s.sensitivity);
  const sel = ed.useEd((s) => (s.sel && s.sel.track === track.id ? s.sel : null));

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const scaleRef = useRef<HTMLCanvasElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const playheadRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<Drag | null>(null);
  const hoverRef = useRef<Id | null>(null);
  // Where to scroll once the next zoom has laid out; the saved position on opening.
  const pendingLeft = useRef<number | null>(lastLeft);
  const drawRef = useRef<() => void>(() => {});
  const [requestDraw] = useState(() => coalesced(() => drawRef.current()));
  const accent = useMemo(() => getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#4c9eff", [themeId]);

  const focus = region && track.regions.some((r) => r.id === region.id) ? region : null;
  const total = project ? timelineFrames(project) : 1;
  const contentW = Math.ceil(total * pps);
  const live = useRef({ project, track, selectedRegions, pps, amp, showTransients, accent, total });
  live.current = { project, track, selectedRegions, pps, amp, showTransients, accent, total };

  // ---- zoom and scroll ----------------------------------------------------------------------

  const ppsBounds = () => {
    const W = scrollRef.current?.clientWidth || 600;
    const t = live.current.total;
    return [W / t, Math.min(MAX_PPS, MAX_CONTENT_PX / t)] as const;
  };

  /** Sets the zoom, keeping `anchor` (a frame) at `anchorPx` in the view. */
  const setZoom = (next: number, anchor?: number, anchorPx?: number) => {
    const el = scrollRef.current;
    if (!el) return;
    const [lo, hi] = ppsBounds();
    const cur = live.current.pps || next;
    const v = Math.min(hi, Math.max(lo, next));
    const px = anchorPx ?? el.clientWidth / 2;
    const at = anchor ?? (el.scrollLeft + px) / cur;
    const left = Math.max(0, at * v - px);
    if (v === live.current.pps) el.scrollLeft = left;
    else {
      pendingLeft.current = left;
      ed.setEd({ pps: v });
    }
  };

  const zoomTo = (a: number, b: number) => {
    const el = scrollRef.current;
    if (!el) return;
    const W = el.clientWidth;
    const [lo, hi] = ppsBounds();
    const v = Math.min(hi, Math.max(lo, (W * 0.94) / Math.max(1, b - a)));
    const left = Math.max(0, a * v - W * 0.03);
    if (v === live.current.pps) el.scrollLeft = left;
    else {
      pendingLeft.current = left;
      ed.setEd({ pps: v });
    }
  };

  useEffect(() => {
    ed.attachView({
      track: track.id,
      region: focus?.id ?? null,
      zoomTo,
      zoomBy: (f) => setZoom(live.current.pps * f),
      saveView: () => ({ pps: live.current.pps, left: scrollRef.current?.scrollLeft ?? 0 }),
      restoreView: (v) => {
        if (v.pps === live.current.pps && scrollRef.current) scrollRef.current.scrollLeft = v.left;
        else {
          pendingLeft.current = v.left;
          ed.setEd({ pps: v.pps });
        }
      },
    });
  });
  useEffect(() => () => ed.attachView(null), []);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !project) return;
    if (live.current.pps <= 0 && el.clientWidth > 0) {
      // First time open: fit the region (or the track's regions, or eight bars).
      const regs = focus ? [focus] : track.regions;
      if (regs.length) zoomTo(Math.min(...regs.map((r) => r.start)), Math.max(...regs.map((r) => r.start + r.length)));
      else zoomTo(0, tempoMap(project).barStart(9));
      return;
    }
    if (pendingLeft.current !== null) {
      el.scrollLeft = pendingLeft.current;
      pendingLeft.current = null;
    }
    requestDraw();
  });

  // Bring a newly selected region into view if it's off screen.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !focus || pps <= 0) return;
    const x0 = focus.start * pps;
    const x1 = (focus.start + focus.length) * pps;
    if (x1 < el.scrollLeft || x0 > el.scrollLeft + el.clientWidth) el.scrollLeft = Math.max(0, x0 - el.clientWidth * 0.05);
  }, [focus?.id]);

  // ---- drawing --------------------------------------------------------------------------------

  const snapEd = (f: number, free = false) => {
    const { project, pps } = live.current;
    if (!project) return Math.max(0, Math.round(f));
    const mode = getState().snapMode;
    const pxPerBeat = samplesPerBeat(project) * pps;
    // Smart snap stops at sixteenths; zoomed in further, the waveform is edited freely.
    if (free || mode === "off" || (mode === "smart" && pxPerBeat / 4 > 80)) return Math.max(0, Math.round(f));
    return Math.max(0, Math.round(snapPos(project, f, mode, pxPerBeat)));
  };

  const draw = () => {
    const cv = canvasRef.current;
    const sc = scaleRef.current;
    const el = scrollRef.current;
    const { project, track, selectedRegions, pps, amp, showTransients, accent } = live.current;
    if (!cv || !sc || !el || !project || pps <= 0) return;
    const dpr = pixelRatio();
    const W = el.clientWidth;
    const H = el.clientHeight;
    const left = el.scrollLeft;
    const ctx = sizeCanvas(cv, W, H, dpr);
    const g = geometry(H);
    const X = (f: number) => f * pps - left;
    ctx.fillStyle = theme.editorBg;
    ctx.fillRect(0, 0, W, H);

    // Grid: bars, beats and (zoomed in) sixteenths, through the tempo map.
    forEachGridLine(tempoMap(project), Math.max(0, left / pps), (left + W) / pps, pps, { beatsMinPx: 9.01, subdiv: 4, subMinPx: 30.01 }, (pos, kind) => {
      ctx.fillStyle = kind === "bar" ? theme.gridBar : theme.gridBeat;
      ctx.fillRect(Math.round(X(pos)), RULER, 1, H - RULER);
    });
    if (project.cycle.enabled) {
      ctx.fillStyle = theme.cycleTint;
      ctx.fillRect(X(project.cycle.start), RULER, (project.cycle.end - project.cycle.start) * pps, H - RULER);
    }

    // Regions, as in the arrange but tall, with gain and waveform zoom applied.
    const drag = dragRef.current;
    for (const r of track.regions) {
      let start = r.start;
      let length = r.length;
      let shift = 0;
      let fades: [number, number] | undefined;
      if (drag && drag.id === r.id) {
        if (drag.kind === "move" && drag.start !== undefined) start = drag.start;
        else if (drag.fade !== undefined && r.content.type === "audio") fades = drag.kind === "fadeIn" ? [drag.fade, r.content.fade_out] : [r.content.fade_in, drag.fade];
        else if (drag.start !== undefined && drag.length !== undefined) {
          shift = drag.start - r.start;
          start = drag.start;
          length = drag.length;
        }
      }
      const x = X(start);
      const w = Math.max(2, length * pps);
      if (x > W || x + w < 0) continue;
      const audio = r.content.type === "audio" ? r.content : null;
      const gain = audio ? dbGain(audio.gain_db) : 1;
      const isSel = selectedRegions.includes(r.id);
      if (audio && !waveCache.has(audio.clip)) ensureWave(audio.clip, requestDraw);
      drawRegion(ctx, r, track, x, g.top, w, g.h, isSel, pps, W, shift, fades, !!audio && (isSel || hoverRef.current === r.id), false, gain * amp);
      if (!audio) continue;
      const wave = waveCache.get(audio.clip);
      const offset = audio.offset + shift;
      ctx.save();
      ctx.beginPath();
      ctx.rect(Math.max(0, x), g.bodyTop, Math.min(W, x + w) - Math.max(0, x), g.bodyH);
      ctx.clip();
      if (wave && !r.muted) drawClipMarks(ctx, wave, offset, x, w, pps, gain, g.bodyTop, g.bodyH, W);
      if (showTransients) {
        const list = ed.transientsFor(audio.clip, requestDraw);
        if (list) {
          const from = offset + (Math.max(0, x) - x) / pps;
          const to = offset + (Math.min(W, x + w) - x) / pps;
          ctx.fillStyle = theme.selectOutline;
          for (let i = lowerBound(list, from); i < list.length && list[i] <= to; i++) {
            const tx = Math.round(x + (list[i] - offset) * pps);
            ctx.globalAlpha = 0.35;
            ctx.fillRect(tx, g.bodyTop, 1, g.bodyH);
            ctx.globalAlpha = 0.9;
            ctx.beginPath();
            ctx.moveTo(tx - 3.5, g.bodyTop + g.bodyH);
            ctx.lineTo(tx + 4.5, g.bodyTop + g.bodyH);
            ctx.lineTo(tx + 0.5, g.bodyTop + g.bodyH - 5);
            ctx.fill();
          }
          ctx.globalAlpha = 1;
        }
      }
      ctx.restore();
    }
    if (!track.regions.length) {
      ctx.fillStyle = theme.rulerTick;
      ctx.font = "12px Inter, system-ui, sans-serif";
      ctx.textAlign = "center";
      ctx.fillText("This track has no regions yet. Record or drop an audio file on it.", W / 2, g.top + g.h / 2);
      ctx.textAlign = "left";
    }

    // The selected range.
    const s = ed.rangeSel();
    if (s && s.track === track.id) {
      const sx = X(s.start);
      const sw = Math.max(1, (s.end - s.start) * pps);
      ctx.fillStyle = accent;
      ctx.globalAlpha = 0.2;
      ctx.fillRect(sx, RULER, sw, H - RULER);
      ctx.globalAlpha = 0.9;
      ctx.fillRect(Math.round(sx), RULER, 1, H - RULER);
      ctx.fillRect(Math.round(sx + sw), RULER, 1, H - RULER);
      ctx.globalAlpha = 1;
    }

    drawRuler(ctx, W, left);
    if (s && s.track === track.id) {
      ctx.fillStyle = accent;
      ctx.fillRect(X(s.start), RULER - 4, Math.max(1, (s.end - s.start) * pps), 3);
    }
    drawScale(sc, H, dpr);
    const ph = playheadRef.current;
    if (ph) ph.style.height = `${H}px`;
    positionPlayhead();
  };
  drawRef.current = draw;

  const drawRuler = (ctx: CanvasRenderingContext2D, W: number, left: number) => {
    const { project, pps } = live.current;
    if (!project) return;
    ctx.fillStyle = theme.ruler;
    ctx.fillRect(0, 0, W, RULER);
    ctx.fillStyle = theme.rulerCycleZone;
    ctx.fillRect(0, 0, W, CYCLE_STRIP);
    ctx.fillStyle = theme.rulerEdge;
    ctx.fillRect(0, RULER - 1, W, 1);
    const c = project.cycle;
    if (c.end > c.start) {
      ctx.fillStyle = c.enabled ? theme.cycleOn : theme.cycleOff;
      ctx.beginPath();
      ctx.roundRect(c.start * pps - left, 1, (c.end - c.start) * pps, CYCLE_STRIP - 2, 2);
      ctx.fill();
    }
    const map = tempoMap(project);
    // Label spacing from the bar length where the view starts.
    const every = [1, 2, 4, 8, 16, 32, 64, 128].find((n) => map.samplesPerBarAt(left / pps) * pps * n >= 34) ?? 256;
    ctx.textBaseline = "alphabetic";
    for (const bar of map.bars(Math.max(0, left / pps), (left + W) / pps)) {
      const b = bar.bar - 1;
      const x = Math.round(bar.start * pps - left) + 0.5;
      const barPx = (bar.end - bar.start) * pps;
      const beatPx = barPx / bar.num;
      const beatX = (k: number) => Math.round(map.sampleAt(bar.startQ + (k * 4) / bar.den) * pps - left + 0.5);
      if (b % every === 0) {
        ctx.fillStyle = theme.rulerTick;
        ctx.fillRect(x, CYCLE_STRIP + 1, 1, RULER - CYCLE_STRIP - 2);
        ctx.fillStyle = theme.rulerText;
        ctx.font = "500 11px Inter, 'SF Pro Text', system-ui, sans-serif";
        ctx.fillText(String(b + 1), x + 3, RULER - 5);
      } else if (barPx >= 4) {
        ctx.fillStyle = theme.rulerMinor;
        ctx.fillRect(x, RULER - 7, 1, 6);
      }
      if (beatPx < 10) continue;
      ctx.font = "10px Inter, 'SF Pro Text', system-ui, sans-serif";
      for (let k = 1; k < bar.num; k++) {
        const bx = beatX(k);
        ctx.fillStyle = theme.rulerMinor;
        ctx.fillRect(bx, RULER - 5, 1, 4);
        if (beatPx >= 48) {
          ctx.fillStyle = theme.rulerTick;
          ctx.fillText(`${b + 1}.${k + 1}`, bx + 3, RULER - 5);
        }
      }
      if (beatPx >= 48) {
        ctx.fillStyle = theme.rulerMinor;
        for (let k = 1; k < bar.num * 4; k++) if (k % 4) ctx.fillRect(beatX(k / 4), RULER - 3, 1, 2);
      }
    }
  };

  /** The dB scale beside the lane: post-gain level, so it moves with the waveform zoom only. */
  const drawScale = (cv: HTMLCanvasElement, H: number, dpr: number) => {
    const { amp } = live.current;
    const ctx = sizeCanvas(cv, SCALE_W, H, dpr);
    const g = geometry(H);
    ctx.fillStyle = theme.editorBg;
    ctx.fillRect(0, 0, SCALE_W, H);
    ctx.fillStyle = theme.ruler;
    ctx.fillRect(0, 0, SCALE_W, RULER);
    ctx.fillStyle = theme.rulerEdge;
    ctx.fillRect(0, RULER - 1, SCALE_W, 1);
    ctx.font = "10px Inter, 'SF Pro Text', system-ui, sans-serif";
    ctx.textBaseline = "middle";
    ctx.textAlign = "right";
    ctx.fillStyle = theme.rulerTick;
    ctx.fillText("dB", SCALE_W - 7, RULER / 2 + 3);
    const rectified = getState().waveStyle === "rectified";
    const mid = g.bodyTop + g.bodyH / 2;
    const half = g.bodyH / 2 - 1;
    const base = g.bodyTop + g.bodyH;
    const yFor = (lin: number) => (rectified ? base - lin * amp * (g.bodyH - 1) : mid - lin * amp * half);
    const mark = (y: number, label: string) => {
      ctx.fillStyle = theme.rulerMinor;
      ctx.fillRect(SCALE_W - 4, Math.round(y), 4, 1);
      ctx.fillStyle = theme.rulerText;
      ctx.fillText(label, SCALE_W - 7, y);
    };
    let lastY = -Infinity;
    const stop = rectified ? base - 12 : mid - 10;
    for (const d of DB_MARKS) {
      const lin = Math.pow(10, d / 20);
      if (lin * amp > 1.0001) continue;
      const y = yFor(lin);
      if (y > stop) break;
      if (y - lastY < 13) continue;
      mark(y, d === 0 ? "0" : String(d));
      if (!rectified) mark(2 * mid - y, d === 0 ? "0" : String(d));
      lastY = y;
    }
    mark(rectified ? base - 1 : mid, "-∞");
    ctx.textAlign = "left";
  };

  // ---- playhead -------------------------------------------------------------------------------

  const lastPlayhead = useRef("");
  const positionPlayhead = (pos: number = framePos()) => {
    const ph = playheadRef.current;
    const el = scrollRef.current;
    const { pps } = live.current;
    if (!ph || !el || pps <= 0) return;
    const x = pos * pps - el.scrollLeft;
    const visible = x >= -2 && x <= el.clientWidth + 2;
    const key = visible ? `translate3d(${x.toFixed(1)}px,0,0)` : "hidden";
    if (key === lastPlayhead.current) return;
    lastPlayhead.current = key;
    if (visible) ph.style.transform = key;
    ph.style.display = visible ? "block" : "none";
  };

  useEffect(() => {
    const el = scrollRef.current!;
    const ro = new ResizeObserver(() => requestDraw());
    ro.observe(el);
    const wheel = (e: WheelEvent) => {
      const { pps } = live.current;
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        const cx = e.clientX - el.getBoundingClientRect().left;
        setZoom(pps * (e.deltaY < 0 ? 1.2 : 1 / 1.2), (el.scrollLeft + cx) / pps, cx);
      } else if (e.altKey) {
        e.preventDefault();
        ed.setAmp(ed.edState().amp * (e.deltaY < 0 ? 1.25 : 0.8));
      } else if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
        // Nothing scrolls vertically here, so the wheel moves along the timeline.
        e.preventDefault();
        el.scrollLeft += e.deltaY;
      }
    };
    el.addEventListener("wheel", wheel, { passive: false });
    window.addEventListener("keydown", ed.onEditorKey, true);
    window.addEventListener("mousedown", ed.onWindowMouseDown, true);
    const offFrame = onFrame((now) => {
      const { pps } = live.current;
      if (pps <= 0) return;
      const pos = framePos(now);
      positionPlayhead(pos);
      if (currentTick()?.playing && getState().catchPlayhead && !dragRef.current) {
        const x = pos * pps - el.scrollLeft;
        if (x > el.clientWidth - 30 || x < 0) el.scrollLeft = Math.max(0, pos * pps - 60);
      }
    });
    // Go to Beginning and a second Stop bring the playhead into view, as in the Tracks area.
    const offReveal = onRevealPlayhead((pos) => {
      const left = revealLeft(pos * live.current.pps, el.scrollLeft, el.clientWidth);
      if (left !== null) el.scrollLeft = left;
    });
    return () => {
      ro.disconnect();
      el.removeEventListener("wheel", wheel);
      window.removeEventListener("keydown", ed.onEditorKey, true);
      window.removeEventListener("mousedown", ed.onWindowMouseDown, true);
      offFrame();
      offReveal();
    };
  }, []);

  // ---- pointer --------------------------------------------------------------------------------

  /** Presses on the scroll bar are left to it. */
  const onScrollbar = (e: { clientY: number }) => {
    const el = scrollRef.current!;
    return e.clientY - el.getBoundingClientRect().top >= el.clientHeight;
  };

  const frameAt = (clientX: number) => {
    const el = scrollRef.current!;
    return (clientX - el.getBoundingClientRect().left + el.scrollLeft) / live.current.pps;
  };

  const hit = (e: { clientX: number; clientY: number }) => {
    const el = scrollRef.current!;
    const rect = el.getBoundingClientRect();
    const { pps, track } = live.current;
    const x = e.clientX - rect.left + el.scrollLeft;
    const y = e.clientY - rect.top;
    const g = geometry(el.clientHeight);
    let region: Region | undefined;
    if (y >= g.top && y <= g.top + g.h) {
      // Topmost (last drawn) region wins.
      for (let k = track.regions.length - 1; k >= 0; k--) {
        const r = track.regions[k];
        const rx = r.start * pps;
        if (x >= rx && x <= rx + Math.max(2, r.length * pps)) {
          region = r;
          break;
        }
      }
    }
    return { x, y, g, frame: x / pps, region, inRuler: y < RULER };
  };

  /** What a press grabs: the name bar moves the region, the band at the top of the waveform
   *  holds the fade handles, the edges trim, and the rest selects a range. */
  const grabKind = (h: ReturnType<typeof hit>): GrabKind => {
    const r = h.region;
    if (!r) return "select";
    const { pps } = live.current;
    const rx = r.start * pps;
    const rw = r.length * pps;
    if (h.y - h.g.top < HEADER + 1) return "move";
    const edge = Math.min(8, rw / 4);
    const inBody = h.y - h.g.bodyTop;
    if (r.content.type === "audio" && inBody >= 0 && inBody < FADE_ZONE) {
      // As in the arrange: a crossfade's handle is at its start, and replaces the fade-in.
      const hd = fadeHandles(r.start, r.length, r.content.fade_in, r.content.fade_out, regionCrossfades(r.id));
      const fiX = rx + r.content.fade_in * pps;
      const foX = rx + hd.fadeOut * pps;
      const near = Math.min(7, rw / 4);
      if (hd.fadeIn !== null && (Math.abs(h.x - fiX) <= near || (r.content.fade_in === 0 && h.x < rx + edge))) return "fadeIn";
      if (Math.abs(h.x - foX) <= near || (r.content.fade_out === 0 && h.x > rx + rw - edge)) return "fadeOut";
    }
    if (h.x > rx + rw - edge) return "right";
    if (h.x < rx + edge) return "left";
    return "select";
  };

  const drag = (onMove: (ev: MouseEvent) => void, onUp: (ev: MouseEvent) => void) => {
    const up = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", up);
      onUp(ev);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", up);
  };

  const autoScroll = (clientX: number) => {
    const el = scrollRef.current!;
    const r = el.getBoundingClientRect();
    if (clientX > r.right - 16) el.scrollLeft += Math.min(40, clientX - (r.right - 16));
    else if (clientX < r.left + 16) el.scrollLeft -= Math.min(40, r.left + 16 - clientX);
  };

  const selectRegion = (r: Region, shift: boolean) => {
    const cur = getState().selectedRegions;
    const next = shift ? (cur.includes(r.id) ? cur.filter((i) => i !== r.id) : [...cur, r.id]) : [r.id];
    setState({ selectedRegions: next, focus: "regions" });
  };

  const onMouseDown = (e: React.MouseEvent) => {
    ed.setEditorFocus(true);
    if (e.button !== 0 || onScrollbar(e)) return;
    const { project, track } = live.current;
    if (!project) return;
    if (track.id !== getState().snap?.selected) act("select_track", { id: track.id });
    const h = hit(e);

    if (h.inRuler) {
      // Click or scrub the ruler to locate.
      const locate = (cx: number, free: boolean) => act("locate", { pos: snapEd(frameAt(cx), free) });
      locate(e.clientX, e.altKey);
      drag(
        (ev) => locate(ev.clientX, ev.altKey),
        () => {},
      );
      return;
    }

    if (h.region && e.detail === 2) {
      // Double-click: select the whole region.
      ed.setEd({ sel: { track: track.id, start: h.region.start, end: h.region.start + h.region.length } });
      setState({ selectedRegions: [h.region.id], focus: "regions" });
      return;
    }

    const kind = grabKind(h);
    if (kind === "select") {
      beginSelect(e, h.region);
      return;
    }
    const r = h.region!;
    if (!getState().selectedRegions.includes(r.id) || e.shiftKey) selectRegion(r, e.shiftKey);
    const startX = e.clientX;
    const { pps } = live.current;
    let moved = false;
    dragRef.current = { kind, id: r.id };
    const audio = r.content.type === "audio" ? r.content : null;
    const wave = audio ? waveCache.get(audio.clip) : undefined;
    // Same limits as the arrange: trims stay within the file, fades can't overlap.
    const minStart = audio ? r.start - audio.offset : 0;
    const maxLength = audio && wave ? wave.frames - audio.offset : Infinity;
    const fadeFrom = audio ? (kind === "fadeIn" ? audio.fade_in : audio.fade_out) : 0;
    const crossfading = kind === "fadeOut" && !!regionCrossfades(r.id)?.out;
    drag(
      (ev) => {
        if (!moved && Math.abs(ev.clientX - startX) < 3) return;
        moved = true;
        const d = dragRef.current!;
        const dx = (ev.clientX - startX) / pps;
        if (kind === "move") d.start = snapEd(r.start + dx, ev.altKey);
        else if (kind === "fadeIn" || kind === "fadeOut") {
          const other = audio ? (kind === "fadeIn" ? audio.fade_out : audio.fade_in) : 0;
          d.fade = crossfading ? draggedFadeOut(fadeFrom, dx, true, r.length - other) : Math.round(Math.max(0, Math.min(r.length - other, fadeFrom + (kind === "fadeIn" ? dx : -dx))));
          if (crossfading) setCrossfadeDrag({ region: r.id, length: d.fade });
        } else if (kind === "right") {
          d.start = r.start;
          d.length = Math.min(maxLength, Math.max(PEAK_BUCKET, snapEd(r.start + r.length + dx, ev.altKey) - r.start));
        } else {
          const ns = Math.min(r.start + r.length - PEAK_BUCKET, Math.max(0, minStart, snapEd(r.start + dx, ev.altKey)));
          d.start = ns;
          d.length = r.length - (ns - r.start);
        }
        requestDraw();
      },
      () => {
        const d = dragRef.current!;
        dragRef.current = null;
        setCrossfadeDrag(null);
        requestDraw();
        if (!moved) return;
        if (kind === "move" && d.start !== undefined && d.start !== r.start) {
          const start = d.start;
          const project = getState().snap?.project;
          if (project) moveAutomationAnswer(project, [{ id: r.id, start }]).then((automation) => act("move_region", { id: r.id, start, track: null, automation }));
        }
        else if (d.fade !== undefined) act("set_region_fades", { ids: [r.id], fadeIn: kind === "fadeIn" ? d.fade : null, fadeOut: kind === "fadeOut" ? d.fade : null });
        else if (d.start !== undefined && d.length !== undefined) act("resize_region", { id: r.id, start: Math.round(d.start), length: Math.round(d.length) });
      },
    );
  };

  /** Drag across the waveform to select a range; Shift extends the current one. A click
   *  without dragging selects the region under it instead. */
  const beginSelect = (e: React.MouseEvent, under: Region | undefined) => {
    const { track } = live.current;
    const cur = ed.rangeSel();
    const at = frameAt(e.clientX);
    let anchor = snapEd(at, e.altKey);
    const extending = e.shiftKey && !!cur;
    if (extending) {
      anchor = Math.abs(at - cur.start) > Math.abs(at - cur.end) ? cur.start : cur.end;
      const s = snapEd(at, e.altKey);
      ed.setEd({ sel: { track: track.id, start: Math.min(anchor, s), end: Math.max(anchor, s) } });
    }
    const startX = e.clientX;
    let moved = extending;
    drag(
      (ev) => {
        if (!moved && Math.abs(ev.clientX - startX) < 3) return;
        moved = true;
        autoScroll(ev.clientX);
        const s = snapEd(frameAt(ev.clientX), ev.altKey);
        ed.setEd({ sel: s === anchor ? null : { track: track.id, start: Math.min(anchor, s), end: Math.max(anchor, s) } });
      },
      () => {
        if (moved) return;
        ed.clearSel();
        if (under) selectRegion(under, e.shiftKey);
        else setState({ selectedRegions: [] });
      },
    );
  };

  const onHover = (e: React.MouseEvent) => {
    if (dragRef.current) return;
    const el = scrollRef.current!;
    const h = hit(e);
    const id = h.region?.id ?? null;
    if (hoverRef.current !== id) {
      hoverRef.current = id;
      requestDraw();
    }
    const kind = h.inRuler ? null : grabKind(h);
    el.style.cursor = kind === null || kind === "move" ? "default" : kind === "left" || kind === "right" ? "ew-resize" : kind === "select" ? "text" : "col-resize";
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    ed.setEditorFocus(true);
    if (onScrollbar(e)) return;
    const { track } = live.current;
    const h = hit(e);
    const s = ed.rangeSel();
    if (s && h.frame >= s.start && h.frame <= s.end) {
      openMenu(e.clientX, e.clientY, ed.selectionMenu());
      return;
    }
    const r = h.region;
    if (!r) {
      openMenu(e.clientX, e.clientY, laneMenu(track, snapEd(h.frame)));
      return;
    }
    if (!getState().selectedRegions.includes(r.id)) setState({ selectedRegions: [r.id], focus: "regions" });
    const extra: MenuItem[] = [
      { label: "Select Whole Region", hint: "Double-click", onClick: () => ed.setEd({ sel: { track: track.id, start: r.start, end: r.start + r.length } }) },
      // Z zooms to the focused region (or the selection); this one may be another region, so
      // no key is shown. Z still zooms back.
      { label: "Zoom to Region", onClick: () => ed.zoomToSpan(r.start, r.start + r.length) },
      { label: "Split at Transients", hint: fmt("Mod+Shift+T"), disabled: r.content.type !== "audio", onClick: ed.splitAtTransients },
      { separator: true },
    ];
    openMenu(
      e.clientX,
      e.clientY,
      [
        ...extra,
        ...regionMenu(r, () => {
          const name = window.prompt("Region name", r.name);
          if (name) act("rename_region", { id: r.id, name });
        }),
      ],
    );
  };

  // ---- local bar ------------------------------------------------------------------------------

  const [lo, hi] = ppsBounds();
  const zoomValue = pps > 0 && hi > lo ? Math.log(pps / lo) / Math.log(hi / lo) : 0;

  return (
    <div className="editor audio-editor">
      <div className="local-bar">
        <div className="local-menus">
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, ed.editMenu())}>
            Edit
          </button>
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, ed.functionsMenu())}>
            Functions
          </button>
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, ed.viewMenu())}>
            View
          </button>
          <span className="local-title">Audio Track: {track.name}</span>
        </div>
        <div className="local-right">
          <button className={"lb-btn" + (showTransients ? " on" : "")} title="Show Transients" onClick={() => ed.setEd({ showTransients: !showTransients })}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M1 7h2l1.2-4 1.6 8L7 5l1 3.5L9 7h4" />
              <path d="M4.2 1.5v1M9 1.5v1" strokeLinecap="round" />
            </svg>
          </button>
          <span className="lb-label">Gain</span>
          <GainField region={focus} />
          <span className="lb-label" title="Waveform zoom">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M1 6h1.5l1-3 1.5 6 1.5-7 1.5 6 1-2H11" />
            </svg>
          </span>
          <input
            className="zoom vzoom"
            type="range"
            min={0}
            max={1}
            step={0.001}
            value={Math.log(amp) / Math.log(ed.MAX_AMP)}
            title={`Waveform zoom ×${amp.toFixed(1)} (${fmt("Alt")}-scroll, ${fmt("Mod+ArrowUp")} / ${fmt("Mod+ArrowDown")})`}
            onChange={(e) => ed.setAmp(Math.pow(ed.MAX_AMP, parseFloat(e.target.value)))}
            onDoubleClick={() => ed.setAmp(1)}
          />
          <span className="lb-label" title="Horizontal zoom">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M1 6h10M3.5 3.5 1 6l2.5 2.5M8.5 3.5 11 6 8.5 8.5" />
            </svg>
          </span>
          <input
            className="zoom"
            type="range"
            min={0}
            max={1}
            step={0.001}
            value={zoomValue}
            title={`Horizontal zoom (${fmt("Mod")}-scroll, ${fmt("Mod+ArrowLeft")} / ${fmt("Mod+ArrowRight")}, Z)`}
            onChange={(e) => setZoom(lo * Math.pow(hi / lo, parseFloat(e.target.value)))}
          />
          <DetachButton panel="editor" />
        </div>
      </div>
      {project && <InfoStrip project={project} track={track} region={focus} sel={sel} />}
      <div className="aed-body">
        <canvas ref={scaleRef} className="aed-scale" />
        <div className="aed-main">
          <canvas ref={canvasRef} className="aed-canvas" />
          <div className="aed-playhead" ref={playheadRef} style={{ ["--ph" as string]: theme.playhead }} />
          <div className="aed-scroll" ref={scrollRef} onScroll={(e) => {
              lastLeft = e.currentTarget.scrollLeft;
              requestDraw();
            }}
            onMouseDown={onMouseDown} onMouseMove={onHover} onContextMenu={onContextMenu}>
            <div style={{ width: contentW, height: 1 }} />
          </div>
        </div>
      </div>
    </div>
  );
}

/** The region's details, and the selected range when there is one. */
function InfoStrip({ project, track, region, sel }: { project: Project; track: Track; region: Region | null; sel: ed.RangeSel | null }) {
  const clip = region?.content.type === "audio" ? region.content.clip : null;
  const [, setLoaded] = useState(0);
  useEffect(() => {
    if (clip === null || clipInfos.has(clip)) return;
    call<{ frames: number; channels: number; sample_rate: number; path: string }>("clip_info", { id: clip }).then((info) => {
      clipInfos.set(clip, info);
      setLoaded((n) => n + 1);
    });
  }, [clip]);
  const info = clip !== null ? clipInfos.get(clip) : undefined;
  const file = info?.path.split(/[\\/]/).pop();
  return (
    <div className="aed-info">
      {region ? (
        <>
          <span>
            <b>{region.name}</b>
            {region.muted ? " (muted)" : ""}
          </span>
          <span>
            Start <b>{bbt(project, region.start)}</b>
          </span>
          <span>
            Length <b>{lengthBbt(project, region.start, region.length)}</b> · {formatTime(project, region.length)}
          </span>
          {region.content.type === "audio" && (
            <span>
              Gain <b>{formatDb(region.content.gain_db)} dB</b>
            </span>
          )}
          {info && (
            <span title={info.path}>
              {(info.sample_rate / 1000).toFixed(1)} kHz · {info.channels === 1 ? "Mono" : info.channels === 2 ? "Stereo" : `${info.channels} ch`} · {file}
            </span>
          )}
        </>
      ) : (
        <span>
          {track.regions.length ? `${track.regions.length} region${track.regions.length > 1 ? "s" : ""} on this track. Click one to select it, or drag across the waveform to select a range.` : "No regions."}
        </span>
      )}
      {sel && (
        <span className="aed-sel">
          Selection <b>{bbt(project, sel.start)}</b> – <b>{bbt(project, sel.end)}</b> · {formatTime(project, sel.end - sel.start)}
        </span>
      )}
    </div>
  );
}

/** Region gain in the local bar: drag up or down (Shift for fine steps), scroll, or
 *  double-click for 0 dB. A drag is one undo step. */
function GainField({ region }: { region: Region | null }) {
  const audio = region?.content.type === "audio" ? region.content : null;
  const db = audio?.gain_db ?? 0;
  const want = useRef<{ id: Id; db: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const [send] = useState(() =>
    coalesced(() => {
      const w = want.current;
      // Once the store has the value, the shown one is current again.
      if (w) act("set_region_gain", { ids: [w.id], db: w.db, delta: null }).then(() => want.current === w && (want.current = null));
    }),
  );
  const set = (v: number) => {
    if (!region) return;
    want.current = { id: region.id, db: Math.round(Math.min(30, Math.max(-96, v)) * 10) / 10 };
    send();
  };
  // The wheel steps from the value last asked for, not the one shown: fast ticks arrive before
  // the store catches up. A native listener, because React's wheel events are passive and
  // can't stop the pane from scrolling.
  const live = useRef({ region, db, set });
  live.current = { region, db, set };
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      const { region, db, set } = live.current;
      if (region?.content.type !== "audio") return;
      e.preventDefault();
      const from = want.current?.id === region.id ? want.current.db : db;
      set(from + (e.deltaY < 0 ? 0.5 : -0.5));
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => el.removeEventListener("wheel", wheel);
  }, []);
  return (
    <div
      ref={ref}
      className={"aed-gain" + (audio ? "" : " disabled")}
      title="Region gain: drag, scroll, or double-click for 0 dB"
      onMouseDown={(e) => {
        if (!audio) return;
        e.preventDefault();
        const y0 = e.clientY;
        let v = db;
        let last = y0;
        const move = (ev: MouseEvent) => {
          v += ((last - ev.clientY) * (ev.shiftKey ? 0.05 : 0.25));
          last = ev.clientY;
          set(v);
        };
        const up = () => {
          window.removeEventListener("mousemove", move);
          window.removeEventListener("mouseup", up);
        };
        window.addEventListener("mousemove", move);
        window.addEventListener("mouseup", up);
      }}
      onDoubleClick={() => set(0)}
    >
      {audio ? `${formatDb(db)} dB` : "—"}
    </div>
  );
}
