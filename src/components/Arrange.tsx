import { pixelRatio } from "../uiScale";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { act, actMade, type Id, type Project, type Region, type Track } from "../api";
import { getState, liveTakes, onTick, setState, currentTick, useStore } from "../store";
import { drawWave, ensureWave, waveCache } from "../waveform";
import { coalesced, framePos, onFrame } from "../frame";
import { PEAK_BUCKET, samplesPerBeat, snapPos, toBbt } from "../time";
import { forEachGridLine, tempoMap } from "../tempo";
import { mix, theme } from "../theme";
import { indexAt, stepVisible, useLayout, visibleSteps, zoomTracks, type Layout } from "../layout";
import { onViewport, registerScroller, setViewport, viewport } from "../view";
import { openMenu } from "./Menu";
import { laneMenu, linkedRegions, regionMenu } from "../ops";
import { isLooped, loopEnd, loopPasses, regionEnd } from "../loops";
import { automationHover, automationLeave, automationMenu, automationMouseDown, drawAutomation } from "../automation";
import { refuseFrozen } from "../freeze";
import { getSettings } from "../prefs";
import { crossfadeLaws, crossfadesByRegion, draggedFadeOut, fadeCurve, fadeHandles, resized, shapeOf, type Crossfade, type RegionCrossfades } from "../fades";
import { drawCompWave, drawFolderBadge, drawTakeRows, ensureTakeWaves, FOLDER_BADGE_W, isFolder, isOpen, swipeComp, takeAtRow, takeFolderMenu, takeShift, toggleFolder } from "../takes";
import { onRevealPlayhead, revealLeft } from "../view";
import { timeWheel } from "../wheel";
import { toggleOverview } from "./Overview";
import { fadeZone, MIN_BODY_H, regionBar } from "../regionBar";
import { playedNotes } from "../regionParams";
import { dragOutcome, type Outcome } from "../dragModes";
import { moveAutomationAnswer } from "../moveAutomation";
import { activeTool, drawToolOverlay, escapeMarquee, notePointer, pressOnMarquee, toolHover, toolLeave, toolMouseDown, type ToolView } from "../trackTools";

// ---- peaks ----------------------------------------------------------------------------------

let redraw: () => void = () => {};

function ensurePeaks(project: Project) {
  for (const t of project.tracks) for (const r of t.regions) if (r.content.type === "audio") ensureWave(r.content.clip, () => redraw());
  ensureTakeWaves(project.tracks, () => redraw());
}

// ---- drawing ----------------------------------------------------------------------------------

interface Drag {
  kind: "move" | "left" | "right" | "fadeIn" | "fadeOut" | "loop";
  ids: Id[];
  dx: number;
  /** Tracks moved, counted in visible tracks (collapsed and hidden ones are skipped). */
  dTrack: number;
  /** Alt (Option) is held: the move drops copies, as in Logic. */
  copy?: boolean;
  /** For trims, the new start/length of the grabbed region. */
  start?: number;
  length?: number;
  /** For fade drags, the new fade length. */
  fade?: number;
  /** For loop drags, the new loop end. */
  loopEnd?: number;
  grabbed: Id;
}

/** The track a region on track `i` lands on when a drag moves it `dTrack` visible tracks. A
 *  region can only go onto a track of its own kind; otherwise it stays on its track, as
 *  `move_region` does. */
function dragTarget(layout: Layout, i: number, dTrack: number, tracks: Track[]): number {
  const ti = stepVisible(layout, i, dTrack);
  return tracks[ti]?.kind === tracks[i].kind ? ti : i;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  r = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

export function drawRegion(
  ctx: CanvasRenderingContext2D,
  r: Region,
  t: Track,
  x: number,
  y: number,
  w: number,
  h: number,
  selected: boolean,
  pps: number,
  W: number,
  /** Frames trimmed off (positive) or revealed at (negative) the front while dragging the left edge. */
  shift = 0,
  /** Fade lengths to show instead of the region's own (while dragging one). */
  fades?: [number, number],
  handles = false,
  /** A loop repetition: drawn without the name and selection outline. */
  ghost = false,
  /** Amplitude scale for the waveform (the Audio Track editor's gain and waveform zoom). */
  waveScale = 1,
) {
  const color = r.muted ? "#6c6c70" : r.color ?? t.color;
  // Short tracks get a slimmer name bar, so the body keeps a strip of its own.
  const geom = regionBar(h);
  const headerH = geom.bar;
  const style = theme.region;
  // Body, name bar and content (waveform / notes) colours for each region style:
  //   glass — translucent tint over the lane (Studio)
  //   flat  — solid dark body under a solid name bar
  //   light — solid pale body for light themes
  let body: string, bar: string, content: string;
  if (style === "glass") {
    body = selected ? mix(color, "#ffffff", 0.22, 0.9) : mix(color, theme.regionBase, 0.18, 0.72);
    bar = selected ? mix(color, "#ffffff", 0.45) : mix(color, "#ffffff", 0.12);
    content = selected ? "rgba(20,20,24,0.85)" : mix(color, "#ffffff", 0.62, 0.95);
  } else if (style === "flat") {
    body = selected ? mix(color, theme.regionBase, 0.12) : mix(color, theme.regionBase, 0.6);
    bar = selected ? mix(color, "#ffffff", 0.3) : color;
    content = selected ? mix(color, theme.regionBase, 0.72) : mix(color, "#ffffff", 0.28);
  } else {
    body = selected ? mix(color, "#ffffff", 0.35) : mix(color, "#ffffff", 0.68);
    bar = selected ? mix(color, "#000000", 0.12) : color;
    content = mix(color, "#000000", selected ? 0.55 : 0.35);
  }
  ctx.save();
  roundRect(ctx, x + 0.5, y + 0.5, w - 1, h - 1, style === "glass" ? 3 : 2);
  ctx.fillStyle = body;
  ctx.fill();
  ctx.clip();
  ctx.fillStyle = bar;
  ctx.fillRect(x, y, w, headerH);
  if (style === "glass") {
    ctx.fillStyle = "rgba(0,0,0,0.18)";
    ctx.fillRect(x, y + headerH, w, 1);
  }
  const visX0 = Math.max(0, x);
  const visX1 = Math.min(W, x + w);
  // A take folder's name bar starts with its disclosure triangle and take badge.
  const badge = isFolder(r) && !ghost && w > FOLDER_BADGE_W ? FOLDER_BADGE_W : 0;
  if (badge) drawFolderBadge(ctx, r as Region & { takes: NonNullable<Region["takes"]> }, x, y, style === "light" ? "#ffffff" : theme.ink);
  if (w > 24 + badge && !ghost) {
    ctx.fillStyle = style === "light" ? "#ffffff" : theme.ink;
    ctx.font = `600 ${geom.font}px Inter, 'SF Pro Text', system-ui, sans-serif`;
    ctx.textBaseline = "middle";
    ctx.fillText(r.muted ? `${r.name} (muted)` : r.name, Math.max(x, 0) + 5 + badge, y + headerH / 2 + 0.5, Math.max(0, w - 8 - badge));
  }
  const bodyTop = y + geom.bodyTop;
  const bodyH = geom.bodyH;
  // A body too short to read (the smallest track heights) shows no waveform or notes.
  const showBody = bodyH >= MIN_BODY_H;
  if (showBody && r.content.type === "audio") {
    const wave = waveCache.get(r.content.clip);
    if (isFolder(r)) drawCompWave(ctx, r, x, w, bodyTop, bodyH, pps, content, visX0, visX1, shift, waveScale * 10 ** (r.content.gain_db / 20));
    else if (wave) drawWave(ctx, wave, r.content.offset + shift, x, w, bodyTop, bodyH, pps, content, visX0, visX1, undefined, waveScale);
    const [fi, fo] = fades ?? [r.content.fade_in, r.content.fade_out];
    drawFades(ctx, r, x - shift * pps, x, w, bodyTop, bodyH, fi, fo, pps, content, handles, ghost ? undefined : regionCrossfades(r.id));
  } else if (showBody && r.content.type === "midi") {
    // As they play: with the region's playback parameters (quantize, transpose, ...) applied.
    const notes = playedNotes(getState().snap?.played, r.id) ?? r.content.notes;
    if (notes.length) {
      let lo = 127;
      let hi = 0;
      for (const n of notes) {
        lo = Math.min(lo, n.pitch);
        hi = Math.max(hi, n.pitch);
      }
      lo -= 2;
      hi += 2;
      const span = Math.max(12, hi - lo);
      const nh = Math.max(1.5, Math.min(4, bodyH / span));
      ctx.fillStyle = content;
      for (const n of notes) {
        const nx = x + (n.start - shift) * pps;
        const nw = Math.max(2, n.length * pps);
        if (nx > visX1 || nx + nw < visX0) continue;
        const ny = bodyTop + bodyH - ((n.pitch - lo) / span) * bodyH - nh / 2;
        ctx.fillRect(nx, ny, nw - 0.5, nh);
      }
    }
  }
  ctx.restore();
  if (selected && !ghost) {
    roundRect(ctx, x + 0.75, y + 0.75, w - 1.5, h - 1.5, style === "glass" ? 3 : 2);
    ctx.strokeStyle = theme.selectOutline;
    ctx.lineWidth = style === "glass" ? 1 : 1.5;
    ctx.stroke();
  }
}

/** A looping region's repetitions after its body (`x` is the timeline origin in pixels): the
 *  region drawn again, lighter, with a notch at each repetition boundary as in Logic. The
 *  fade-out sits at the end of the last repetition, where the loop stops. */
function drawLoops(
  ctx: CanvasRenderingContext2D,
  r: Region,
  t: Track,
  originX: number,
  start: number,
  length: number,
  loop: number | null | undefined,
  y: number,
  h: number,
  selected: boolean,
  pps: number,
  W: number,
  shift: number,
) {
  const passes = loopPasses(start, length, loop);
  const fo = r.content.type === "audio" ? r.content.fade_out : 0;
  const alpha = ctx.globalAlpha;
  passes.forEach(([ps, pl], k) => {
    const x = originX + ps * pps;
    const w = Math.max(2, pl * pps);
    if (x > W || x + w < 0) return;
    const last = k === passes.length - 1;
    ctx.globalAlpha = alpha * 0.5;
    drawRegion(ctx, r, t, x, y, w, h, selected, pps, W, shift, [0, last ? Math.min(fo, pl) : 0], false, true);
    ctx.globalAlpha = alpha;
    // The notch: a small wedge at the top edge and a hairline down the name bar.
    const nx = Math.round(x) + 0.5;
    ctx.fillStyle = "rgba(0,0,0,0.45)";
    ctx.fillRect(nx - 0.5, y, 1, regionBar(h).bar);
    ctx.beginPath();
    ctx.moveTo(nx - 4, y);
    ctx.lineTo(nx + 4, y);
    ctx.lineTo(nx, y + 5);
    ctx.closePath();
    ctx.fill();
  });
}

/** Logic's loop tool pointer: a circular arrow. */
const LOOP_CURSOR =
  "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='18' height='18' viewBox='0 0 18 18'%3E%3Cg fill='none' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M14 9a5 5 0 1 1-1.5-3.6' stroke='%23000' stroke-width='3.4'/%3E%3Cpath d='M14 9a5 5 0 1 1-1.5-3.6' stroke='%23fff' stroke-width='1.6'/%3E%3Cpath d='M13.5 2.5v3.2h-3.2' stroke='%23000' stroke-width='3.4'/%3E%3Cpath d='M13.5 2.5v3.2h-3.2' stroke='%23fff' stroke-width='1.6'/%3E%3C/g%3E%3C/svg%3E\") 9 9, ew-resize";

/** Fade curves over a region body: the faded part shaded, the curve drawn (dashed for a
 *  Speed Up or Slow Down), and (when `handles`) the grab points at the top. `origin` is where
 *  the region's own start is drawn. A crossfade replaces the fade on its side and shows both
 *  regions' curves crossing; each region draws the part over itself. */
function drawFades(
  ctx: CanvasRenderingContext2D,
  r: Region,
  origin: number,
  x: number,
  w: number,
  top: number,
  h: number,
  fi: number,
  fo: number,
  pps: number,
  ink: string,
  handles: boolean,
  xf: RegionCrossfades | undefined,
) {
  const shape = shapeOf(r.content);
  // `gain(u)` for u from 0 at the left of the fade to 1 at its right.
  const side = (x0: number, len: number, gain: (u: number) => number, dashed: boolean, shade: boolean) => {
    if (len < 1) return;
    const steps = Math.max(8, Math.min(64, Math.round(len / 3)));
    const pt = (k: number) => [x0 + (k / steps) * len, top + h * (1 - gain(k / steps))] as const;
    if (shade) {
      ctx.beginPath();
      ctx.moveTo(x0, top);
      for (let k = 0; k <= steps; k++) ctx.lineTo(...pt(k));
      ctx.lineTo(x0 + len, top);
      ctx.closePath();
      ctx.fillStyle = "rgba(0,0,0,0.28)";
      ctx.fill();
    }
    ctx.beginPath();
    for (let k = 0; k <= steps; k++) (k ? ctx.lineTo : ctx.moveTo).call(ctx, ...pt(k));
    ctx.strokeStyle = ink;
    ctx.lineWidth = 1.2;
    if (dashed) ctx.setLineDash([3, 2]);
    ctx.stroke();
    ctx.setLineDash([]);
  };
  const at = (frame: number) => origin + (frame - r.start) * pps;
  if (!xf?.in) side(x, fi * pps, (u) => fadeCurve(u, shape.in_curve), shape.in_type === "speed_up", true);
  if (!xf?.out) side(x + w - fo * pps, fo * pps, (u) => fadeCurve(1 - u, shape.out_curve), shape.out_type === "slow_down", true);
  for (const c of [xf?.in, xf?.out]) {
    if (!c) continue;
    const [outLaw, inLaw] = crossfadeLaws(c);
    side(at(c.fade_out[0]), c.fade_out[1] * pps, (u) => outLaw(1 - u), false, c === xf?.out);
    side(at(c.fade_in[0]), c.fade_in[1] * pps, inLaw, false, c === xf?.in);
  }
  if (handles) {
    const hd = fadeHandles(r.start, r.length, fi, fo, xf);
    ctx.fillStyle = ink;
    if (hd.fadeIn !== null) ctx.fillRect(Math.round(x + hd.fadeIn * pps) - 3, top, 6, 6);
    ctx.fillRect(Math.round(xf?.out ? at(r.start + hd.fadeOut) : x + w - fo * pps) - 3, top, 6, 6);
  }
}

/** A crossfade whose handle is being dragged: its outgoing region and the length shown. */
let crossfadeDrag: { region: Id; length: number } | null = null;

export function setCrossfadeDrag(v: { region: Id; length: number } | null) {
  crossfadeDrag = v;
}

/** The crossfades a region takes part in, with a handle drag in progress applied. */
export function regionCrossfades(id: Id): RegionCrossfades | undefined {
  const x = crossfadesByRegion(getState().snap?.crossfades).get(id);
  const d = crossfadeDrag;
  if (!x || !d) return x;
  const fix = (c?: Crossfade) => (c && c.out_region === d.region ? resized(c, d.length) : c);
  return { out: fix(x.out), in: fix(x.in) };
}

/** Height of the band at the top of an audio region's body where its fade handles are. */
export const FADE_ZONE = 14;

// ---- component --------------------------------------------------------------------------------

export function Arrange() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const playheadRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<Drag | null>(null);
  // A Quick Swipe Comping drag in progress: the take and take-time range.
  const swipeRef = useRef<{ region: Id; take: Id; start: number; end: number } | null>(null);
  // A rubber-band selection in progress, in content coordinates.
  const marqueeRef = useRef<{ left: number; top: number; right: number; bottom: number } | null>(null);
  // Region under the pointer, which shows its fade handles.
  const hoverRef = useRef<Id | null>(null);
  const drawRef = useRef<() => void>(() => {});
  // Repaints are coalesced to at most one per display frame.
  const [requestDraw] = useState(() => coalesced(() => drawRef.current()));
  const project = useStore((s) => s.snap?.project);
  const selectedTrack = useStore((s) => s.snap?.selected ?? null);
  const selectedRegions = useStore((s) => s.selectedRegions);
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  const layout = useLayout();
  const baseHeight = useStore((s) => s.trackHeight);
  // Redraw while a file drag moves over the tracks.
  useStore((s) => s.dropTarget);
  // Re-render (and so redraw) when the theme changes.
  useStore((s) => s.themeId);
  useStore((s) => s.uiScale);
  useStore((s) => s.waveStyle);
  const showAutomation = useStore((s) => s.showAutomation);
  useStore((s) => s.autoLane);
  useStore((s) => s.marquee);

  const pps = project ? pxPerBeat / samplesPerBeat(project) : 0;
  const lengthSamples = project
    ? Math.max(
        project.sample_rate * 60 * 6,
        ...project.tracks.flatMap((t) => t.regions.map(regionEnd)).map((e) => e + project.sample_rate * 60),
      )
    : 0;
  const contentW = Math.ceil(lengthSamples * pps);
  const contentH = layout.total + baseHeight * 4;

  // Keep the latest values for the imperative draw routine.
  const live = useRef({ project, selectedTrack, selectedRegions, pps, layout });
  live.current = { project, selectedTrack, selectedRegions, pps, layout };

  const draw = () => {
    const cv = canvasRef.current;
    const { project, selectedTrack, selectedRegions, pps, layout } = live.current;
    if (!cv || !project) return;
    const vp = viewport();
    const dpr = pixelRatio();
    const W = vp.width;
    const H = vp.height;
    if (cv.width !== Math.round(W * dpr) || cv.height !== Math.round(H * dpr)) {
      cv.width = Math.round(W * dpr);
      cv.height = Math.round(H * dpr);
      cv.style.width = `${W}px`;
      cv.style.height = `${H}px`;
    }
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = theme.arrangeBg;
    ctx.fillRect(0, 0, W, H);

    // Lanes (hidden tracks, shown in hide mode, are dimmed)
    const multi = getState().selectedTracks;
    project.tracks.forEach((t, i) => {
      const y = layout.tops[i] - vp.top;
      const th = layout.heights[i];
      if (th === 0 || y > H || y + th < 0) return;
      ctx.fillStyle = t.id === selectedTrack || (selectedTrack !== null && multi.includes(selectedTrack) && multi.includes(t.id)) ? theme.laneSelected : theme.lane;
      if (t.hidden) ctx.globalAlpha = 0.45;
      ctx.fillRect(0, y, W, th - 1);
      ctx.globalAlpha = 1;
    });

    // Grid, through the tempo map: bar lines, and beats when they are far enough apart.
    const gridBottom = Math.min(H, layout.total - vp.top);
    forEachGridLine(tempoMap(project), Math.max(0, vp.left / pps), (vp.left + W) / pps, pps, { beatsMinPx: 9 }, (pos, kind) => {
      ctx.fillStyle = kind === "bar" ? theme.barLine : theme.beatLine;
      ctx.fillRect(kind === "bar" ? Math.round(pos * pps - vp.left) + 0.5 : Math.round(pos * pps - vp.left + 0.5), 0, 1, gridBottom);
    });
    // Cycle range tint, as Logic does when cycle is on.
    if (project.cycle.enabled) {
      const cx = project.cycle.start * pps - vp.left;
      const cw = (project.cycle.end - project.cycle.start) * pps;
      ctx.fillStyle = theme.cycleTint;
      ctx.fillRect(cx, 0, cw, gridBottom);
    }

    // A stack's lane shows where its subtracks have regions.
    project.tracks.forEach((t, i) => {
      if (t.kind !== "folder" && t.kind !== "summing") return;
      const y = layout.tops[i] - vp.top;
      const th = layout.heights[i];
      if (th === 0 || y > H || y + th < 0) return;
      const inside = new Set<number>([t.id]);
      for (const c of project.tracks) if (c.parent !== null && inside.has(c.parent)) inside.add(c.id);
      const subs = project.tracks.filter((c) => c.id !== t.id && inside.has(c.id) && c.regions.length);
      // A row per subtrack down the lane; on short lanes the margins shrink and the rows
      // thin out (down to a pixel), clipped so they never spill into the next lane.
      const pad = th >= 30 ? 5 : 3;
      const rowH = Math.min(8, (th - 1 - 2 * pad) / Math.max(1, subs.length));
      ctx.save();
      ctx.beginPath();
      ctx.rect(0, y + pad, W, Math.max(0, th - 1 - 2 * pad));
      ctx.clip();
      subs.forEach((c, k) => {
        ctx.fillStyle = mix(c.color, theme.regionBase, 0.25, 0.85);
        for (const r of c.regions) {
          const x = r.start * pps - vp.left;
          const w = Math.max(2, (regionEnd(r) - r.start) * pps);
          if (x > W || x + w < 0) continue;
          ctx.fillRect(x, y + pad + k * rowH, w, Math.max(1, rowH - 1));
        }
      });
      ctx.restore();
    });

    // Regions
    const drag = dragRef.current;
    const spbSnap = (v: number) => snapPos(project, v, getState().snapMode, getState().pxPerBeat);
    // In the No Overlap and Shuffle drag modes, where the drag will leave every region.
    let outcome: Map<Id, Outcome> | null = null;
    // Where each region draws, with any drag in progress applied.
    const place = (r: Region, i: number, still = false) => {
      let start = r.start;
      let length = r.length;
      let loop = r.loop_end;
      let ti = i;
      let shift = 0;
      let fades: [number, number] | undefined;
      if (drag && !still && drag.ids.includes(r.id)) {
        if (drag.kind === "move") {
          start = Math.max(0, r.id === drag.grabbed ? spbSnap(r.start + drag.dx) : r.start + (spbSnap(getGrabbedStart() + drag.dx) - getGrabbedStart()));
          if (loop != null) loop += start - r.start;
          ti = dragTarget(layout, i, drag.dTrack, project.tracks);
        } else if (r.id === drag.grabbed && drag.fade !== undefined && r.content.type === "audio") {
          fades = drag.kind === "fadeIn" ? [drag.fade, r.content.fade_out] : [r.content.fade_in, drag.fade];
        } else if (r.id === drag.grabbed && drag.loopEnd !== undefined) {
          loop = drag.loopEnd;
        } else if (r.id === drag.grabbed && drag.start !== undefined && drag.length !== undefined) {
          shift = drag.start - r.start;
          start = drag.start;
          length = drag.length;
        }
      }
      const o = still ? undefined : outcome?.get(r.id);
      if (o?.start !== undefined) {
        if (loop != null) loop += o.start - start;
        start = o.start;
      }
      const gone = o?.len !== undefined && o.len <= 0;
      if (o?.len !== undefined && o.len < length) {
        length = Math.max(0, o.len);
        loop = null;
      } else if (o?.len !== undefined) loop = start + o.len;
      // A looping region's fade-out is drawn where the loop ends.
      if (r.content.type === "audio" && loopEnd(start, length, loop) > start + length) fades = [fades?.[0] ?? r.content.fade_in, 0];
      const y = layout.tops[ti] - vp.top + 1;
      // Open take folders' rows sit below the lane; regions keep the lane's own height.
      const th = layout.heights[ti] - layout.takeRows[ti];
      return { start, length, loop, ti, shift, fades, y, th, hidden: project.tracks[ti].hidden, gone };
    };
    const mode = project.drag_mode ?? "overlap";
    if (drag && !drag.copy && mode !== "overlap" && mode !== "xfade") {
      const lanes = new Map(project.tracks.map((t) => [t.id, t.regions.map((r) => ({ id: r.id, start: r.start, len: regionEnd(r) - r.start }))]));
      const grabbed = project.tracks.flatMap((t) => t.regions).find((r) => r.id === drag.grabbed);
      if (drag.kind === "move") {
        const moved = project.tracks.flatMap((t, i) =>
          t.regions
            .filter((r) => drag.ids.includes(r.id))
            .map((r) => {
              const g = place(r, i);
              return { id: r.id, track: project.tracks[g.ti].id, start: g.start };
            }),
        );
        outcome = dragOutcome(mode, lanes, { kind: "move", moved });
      } else if (grabbed && (drag.kind === "left" || drag.kind === "right") && drag.start !== undefined && drag.length !== undefined) {
        const len = loopEnd(drag.start, drag.length, grabbed.loop_end) - drag.start;
        outcome = dragOutcome(mode, lanes, { kind: "resize", id: drag.grabbed, start: drag.start, len });
      }
    }
    // Loop repetitions first: region bodies cover them, as they do in playback.
    // A copy drag (Alt) leaves the originals in place and draws the copies where they'll land.
    const passes = (r: Region) => (drag?.copy && drag.ids.includes(r.id) ? [true, false] : [false]);
    project.tracks.forEach((t, i) => {
      for (const r of t.regions) for (const still of passes(r)) {
        if (!isLooped(r) && !(drag?.kind === "loop" && drag.grabbed === r.id)) continue;
        const g = place(r, i, still);
        if (g.gone) continue;
        const x0 = (g.start + g.length) * pps - vp.left;
        const x1 = loopEnd(g.start, g.length, g.loop) * pps - vp.left;
        if (g.th === 0 || x0 > W || x1 < 0 || g.y > H || g.y + g.th < 0) continue;
        ctx.globalAlpha = g.hidden ? 0.4 : 1;
        drawLoops(ctx, r, t, -vp.left, g.start, g.length, g.loop, g.y, g.th - 3, selectedRegions.includes(r.id), pps, W, g.shift);
        ctx.globalAlpha = 1;
      }
    });
    project.tracks.forEach((t, i) => {
      for (const r of t.regions) for (const still of passes(r)) {
        const { start, length, shift, fades, y, th, hidden, gone } = place(r, i, still);
        const x = start * pps - vp.left;
        const w = Math.max(2, length * pps);
        if (gone || th === 0 || x > W || x + w < 0 || y > H || y + th < 0) continue;
        const sel = selectedRegions.includes(r.id);
        ctx.globalAlpha = hidden ? 0.4 : 1;
        drawRegion(ctx, r, t, x, y, w, th - 3, sel, pps, W, shift, fades, r.content.type === "audio" && (sel || hoverRef.current === r.id) && !t.frozen);
        if (t.frozen) {
          // Frozen: a frosted veil over the region, which can't be edited.
          ctx.globalAlpha = 0.28;
          ctx.fillStyle = theme.arrangeBg;
          ctx.fillRect(x, y, w, th - 3);
        }
        ctx.globalAlpha = 1;
      }
    });

    // Open take folders: a row per take below the lane.
    project.tracks.forEach((t, i) => {
      if (!layout.takeRows[i]) return;
      const rowsTop = layout.tops[i] + layout.heights[i] - layout.takeRows[i] - vp.top;
      if (rowsTop > H || rowsTop + layout.takeRows[i] < 0) return;
      for (const r of t.regions) {
        if (!isFolder(r) || !isOpen(r.id)) continue;
        const x = r.start * pps - vp.left;
        const w = Math.max(2, r.length * pps);
        if (x > W || x + w < 0) continue;
        const sw = swipeRef.current;
        drawTakeRows(ctx, r, r.muted ? "#6c6c70" : r.color ?? t.color, x, w, rowsTop, pps, W, sw && sw.region === r.id ? sw : null);
      }
    });

    if (getState().showAutomation) drawAutomation(ctx, { project, layout, pps, vp });

    // Recording in progress
    for (const [trackId, take] of liveTakes) {
      const i = project.tracks.findIndex((t) => t.id === trackId);
      if (i < 0) continue;
      // Cycle recording: show the pass in progress, over the cycle.
      const c = project.cycle;
      const pass = !c.enabled || take.start < c.start || take.start >= c.end ? 0 : project.punch.enabled ? Math.min(project.punch.end, c.end) - Math.max(project.punch.start, c.start) : c.end - c.start;
      const passFrom = pass > 0 ? Math.floor(take.frames / pass) * pass : 0;
      const x = take.start * pps - vp.left;
      const w = Math.max(2, (take.frames - passFrom) * pps);
      const y = layout.tops[i] - vp.top + 1;
      const h = layout.heights[i] - layout.takeRows[i] - 3;
      ctx.save();
      roundRect(ctx, x, y, w, h, 2);
      ctx.fillStyle = theme.region === "light" ? mix(theme.red, "#ffffff", 0.6) : mix(theme.red, theme.regionBase, 0.5);
      ctx.fill();
      ctx.clip();
      ctx.fillStyle = theme.red;
      const g = regionBar(h);
      ctx.fillRect(x, y, w, g.bar);
      if (g.bodyH >= MIN_BODY_H) drawWave(ctx, { frames: take.frames, bucket: PEAK_BUCKET, levels: [{ min: null, max: take.peaks }] }, passFrom, x, w, y + g.bodyTop, g.bodyH, pps, theme.region === "light" ? mix(theme.red, "#000000", 0.35) : mix(theme.red, "#ffffff", 0.4), 0, W);
      ctx.restore();
    }
    // Where dragged audio files will land: the target lane (or a ghost lane for a new track),
    // a line at the snapped position, and a bar/beat tag.
    const drop = getState().dropTarget;
    if (drop && drop.kind !== "headers") {
      const css = getComputedStyle(document.documentElement);
      const accent = css.getPropertyValue("--accent").trim() || "#4c9eff";
      const ink = css.getPropertyValue("--accent-ink").trim() || "#fff";
      const x = Math.round(drop.sample * pps - vp.left);
      const n = project.tracks.length;
      const isNew = drop.kind === "new";
      const y = (drop.kind === "lane" ? layout.tops[drop.index] : drop.index < n ? layout.tops[drop.index] : layout.total) - vp.top;
      const h = drop.kind === "lane" ? layout.heights[drop.index] - 1 : getState().trackHeight;
      ctx.fillStyle = accent;
      ctx.globalAlpha = isNew ? 0.12 : 0.2;
      ctx.fillRect(0, y, W, h);
      ctx.globalAlpha = 1;
      ctx.strokeStyle = accent;
      ctx.lineWidth = 2;
      if (isNew) ctx.setLineDash([6, 4]);
      ctx.strokeRect(1, y + 1, W - 2, h - 2);
      ctx.setLineDash([]);
      ctx.fillRect(x - 1, y, 2, h);
      const b = toBbt(project, drop.sample);
      const label = `${isNew ? "New track · " : ""}${b.bar} ${b.beat}`;
      ctx.font = "600 11px Inter, system-ui, sans-serif";
      const tw = ctx.measureText(label).width + 12;
      // The tag shrinks to fit the shortest lanes.
      const tagH = Math.min(18, h - 4);
      const tagY = y + Math.min(4, (h - tagH) / 2);
      ctx.beginPath();
      ctx.roundRect(x + 4, tagY, tw, tagH, 4);
      ctx.fill();
      ctx.fillStyle = ink;
      ctx.textBaseline = "middle";
      ctx.fillText(label, x + 10, tagY + tagH / 2 + 0.5);
    }
    drawToolOverlay(ctx, { project, layout, pps, vp, W, H });
    const mq = marqueeRef.current;
    if (mq) {
      const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#4c9eff";
      const x = Math.round(mq.left - vp.left) + 0.5;
      const y = Math.round(mq.top - vp.top) + 0.5;
      const w = Math.round(mq.right - mq.left);
      const h = Math.round(mq.bottom - mq.top);
      ctx.fillStyle = accent;
      ctx.globalAlpha = 0.12;
      ctx.fillRect(x, y, w, h);
      ctx.globalAlpha = 0.8;
      ctx.strokeStyle = accent;
      ctx.lineWidth = 1;
      ctx.strokeRect(x, y, w, h);
      ctx.globalAlpha = 1;
    }
    // A hoisted function so `place` above can use it: the other dragged regions follow the
    // grabbed one's snapped move, which keeps their spacing.
    function getGrabbedStart() {
      for (const t of project!.tracks) for (const r of t.regions) if (r.id === drag!.grabbed) return r.start;
      return 0;
    }
    positionPlayhead();
  };
  drawRef.current = draw;
  redraw = requestDraw;

  const lastPlayhead = useRef("");
  const positionPlayhead = (pos: number = framePos()) => {
    const ph = playheadRef.current;
    const { project, pps } = live.current;
    if (!ph || !project) return;
    const x = pos * pps - viewport().left;
    const tf = `translate3d(${x.toFixed(1)}px,0,0)`;
    const visible = !(x < -2 || x > viewport().width + 2);
    // Touch the DOM only when something changed.
    const key = visible ? tf : "hidden";
    if (key === lastPlayhead.current) return;
    lastPlayhead.current = key;
    ph.style.transform = tf;
    ph.style.display = visible ? "block" : "none";
  };

  useEffect(() => {
    if (project) ensurePeaks(project);
  }, [project]);

  useLayoutEffect(() => {
    requestDraw();
  });

  // Viewport sizing
  useEffect(() => {
    const el = scrollRef.current!;
    const ro = new ResizeObserver(() => {
      setViewport({ width: el.clientWidth, height: el.clientHeight });
    });
    ro.observe(el);
    registerScroller((left, top) => {
      el.scrollLeft = left;
      if (top !== undefined) el.scrollTop = top;
    });
    const off = onViewport(() => requestDraw());
    // Ctrl/Cmd + wheel zooms horizontally around the pointer.
    const wheel = (e: WheelEvent) => {
      if (e.altKey) {
        e.preventDefault();
        zoomTracks(e.deltaY < 0 ? 1.1 : 1 / 1.1);
        return;
      }
      if (!(e.ctrlKey || e.metaKey)) {
        // Settings › Editing › Mouse Wheel Scrolls: Horizontally. The default is left to native
        // scrolling.
        if (getSettings().ui.wheel_scrolls !== "horizontal") return;
        e.preventDefault();
        const { dx, dy } = timeWheel(e, el.clientWidth);
        el.scrollLeft += dx;
        el.scrollTop += dy;
        return;
      }
      e.preventDefault();
      const { project, pps } = live.current;
      if (!project) return;
      const cx = e.clientX - el.getBoundingClientRect().left;
      const sample = (el.scrollLeft + cx) / pps;
      const s = getState();
      const next = Math.min(400, Math.max(2, s.pxPerBeat * (e.deltaY < 0 ? 1.15 : 1 / 1.15)));
      setState({ pxPerBeat: next });
      const nextPps = next / samplesPerBeat(project);
      requestAnimationFrame(() => {
        el.scrollLeft = Math.max(0, sample * nextPps - cx);
      });
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => {
      ro.disconnect();
      off();
      el.removeEventListener("wheel", wheel);
    };
  }, []);

  // Playhead, catch and live recording previews
  useEffect(() => {
    let wasRecording = false;
    return onTick((t) => {
      if (!live.current.project) return;
      for (const p of t.previews) {
        let take = liveTakes.get(p.track);
        if (!take || take.start !== p.start) {
          take = { start: p.start, frames: 0, peaks: [] };
          liveTakes.set(p.track, take);
        }
        take.peaks.length = p.from;
        take.peaks.push(...p.peaks);
        take.frames = p.frames;
      }
      const finished = wasRecording && !t.recording;
      if (finished) liveTakes.clear();
      wasRecording = t.recording;
      if (finished || t.previews.length || liveTakes.size) requestDraw();
    });
  }, []);

  // Playhead and Catch follow the interpolated position every display frame.
  useEffect(
    () =>
      onFrame((now) => {
        const { project, pps } = live.current;
        if (!project) return;
        const pos = framePos(now);
        positionPlayhead(pos);
        const t = currentTick();
        if (t?.playing && getState().catchPlayhead) {
          const vp = viewport();
          const x = pos * pps - vp.left;
          const el = scrollRef.current!;
          if (x > vp.width - 30) el.scrollLeft = pos * pps - 60;
          else if (x < 0) el.scrollLeft = Math.max(0, pos * pps - 60);
        }
      }),
    [],
  );

  // Go to Beginning, a second Stop and the like move the playhead while stopped, when Catch
  // doesn't follow it: bring it into view (horizontally only).
  useEffect(
    () =>
      onRevealPlayhead((pos) => {
        const el = scrollRef.current!;
        const left = revealLeft(pos * live.current.pps, el.scrollLeft, el.clientWidth);
        if (left !== null) el.scrollLeft = left;
      }),
    [],
  );

  // Esc removes the marquee.
  useEffect(() => {
    window.addEventListener("keydown", escapeMarquee);
    return () => window.removeEventListener("keydown", escapeMarquee);
  }, []);

  // ---- interaction ------------------------------------------------------------------------

  const hit = (e: React.MouseEvent) => {
    const el = scrollRef.current!;
    const rect = el.getBoundingClientRect();
    const x = e.clientX - rect.left + el.scrollLeft;
    const y = e.clientY - rect.top + el.scrollTop;
    const { project, pps, layout } = live.current;
    if (!project) return null;
    const ti = y < layout.total ? indexAt(layout, y) : -1;
    const track = ti >= 0 ? project.tracks[ti] : undefined;
    let region: Region | undefined;
    let inLoop = false;
    // Below the lane, in an open take folder's rows: the folder and the take row.
    const laneH = ti >= 0 ? layout.heights[ti] - layout.takeRows[ti] : 0;
    let row: { region: Region & { takes: NonNullable<Region["takes"]> }; take: NonNullable<Region["takes"]>["takes"][number] } | null = null;
    if (track && layout.takeRows[ti] && y - layout.tops[ti] >= laneH) {
      for (const r of track.regions) {
        if (!isFolder(r) || !isOpen(r.id) || x < r.start * pps || x > (r.start + r.length) * pps) continue;
        const take = takeAtRow(r, y - layout.tops[ti], laneH);
        if (take) row = { region: r, take };
      }
    } else if (track) {
      // Topmost (last drawn) region wins; region bodies are drawn over loop repetitions.
      for (let k = track.regions.length - 1; k >= 0 && !region; k--) {
        const r = track.regions[k];
        const rx = r.start * pps;
        if (x >= rx && x <= rx + Math.max(2, r.length * pps)) region = r;
      }
      for (let k = track.regions.length - 1; k >= 0 && !region; k--) {
        const r = track.regions[k];
        if (isLooped(r) && x > (r.start + r.length) * pps && x <= regionEnd(r) * pps) {
          region = r;
          inLoop = true;
        }
      }
    }
    const localY = ti >= 0 ? y - layout.tops[ti] : 0;
    return { x, y, ti, track, region, inLoop, localY, laneH, sample: x / pps, row };
  };

  /** What a press at `h` on a region grabs: the loop (the name bar at the region's or its loop's
   *  right end, Logic's loop tool), an edge to trim (lower part), a fade handle (the top of an
   *  audio region's body, at its edges or current fade points), or the region. A press on a
   *  loop repetition grabs the region. */
  const grabKind = (h: NonNullable<ReturnType<typeof hit>>): Drag["kind"] => {
    const r = h.region!;
    const { pps } = live.current;
    const rx = r.start * pps;
    const rw = r.length * pps;
    // Trim zones are 8 px wide, or a quarter of a narrow region so its middle still moves it.
    const edge = Math.min(8, rw / 4);
    // The name bar (see regionBar; regions are drawn 1 px into the lane and 3 px shorter) and
    // the lane margin above it: the loop handle is up there, trims and fades below.
    const { bodyTop, bodyH } = regionBar(h.laneH - 3);
    const zone = fadeZone(bodyH, FADE_ZONE);
    const looped = isLooped(r);
    const endX = regionEnd(r) * pps;
    if (h.localY <= bodyTop && h.x > endX - (looped ? Math.min(8, (endX - rx - rw) / 2) : edge)) return "loop";
    if (h.inLoop) return "move";
    if (r.content.type === "audio" && h.localY > bodyTop && h.localY < bodyTop + zone) {
      // A crossfade's handle is at its start; a region crossfaded into has no fade-in handle.
      const hd = fadeHandles(r.start, r.length, r.content.fade_in, r.content.fade_out, regionCrossfades(r.id));
      const fiX = rx + r.content.fade_in * pps;
      const foX = rx + hd.fadeOut * pps;
      // How close to a fade point counts as grabbing it, again shrinking on narrow regions.
      const near = Math.min(7, rw / 4);
      if (hd.fadeIn !== null && (Math.abs(h.x - fiX) <= near || (r.content.fade_in === 0 && h.x < rx + edge))) return "fadeIn";
      // A looping region's fade-out is at the loop end; set it from the menu or inspector.
      if (!looped && (Math.abs(h.x - foX) <= near || (r.content.fade_out === 0 && h.x > rx + rw - edge))) return "fadeOut";
    }
    if (h.localY > bodyTop && h.x > rx + rw - edge) return "right";
    if (h.localY > bodyTop && h.x < rx + edge) return "left";
    return "move";
  };

  /** What the tools (trackTools.ts) need to handle the pointer. */
  const toolView = (): ToolView => ({
    ...live.current,
    project: live.current.project!,
    el: scrollRef.current!,
    redraw: requestDraw,
    // The Fade tool shows the fade it will set as a fade handle drag would.
    previewFade: (p) => {
      dragRef.current = p ? { kind: p.kind, ids: [p.id], grabbed: p.id, dx: 0, dTrack: 0, fade: p.length } : null;
    },
    clickLocate,
  });

  /** What the automation view needs to handle the pointer. */
  const autoInput = () => ({ ...live.current, project: live.current.project!, el: scrollRef.current!, redraw: requestDraw });

  const onMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    // Regions can't be edited while automation is shown, as in Logic.
    if (showAutomation && live.current.project) return automationMouseDown(e, autoInput());
    const h = hit(e);
    if (!h) return;
    // The Tracks-area tools; the Pointer carries on below.
    const tool = activeTool(e);
    if (pressOnMarquee(tool, h) || toolMouseDown(tool, e, h, toolView())) return;
    if (!h.track) return startMarquee(e, h);
    const { project, pps } = live.current;
    if (!project) return;
    if (h.track.id !== getState().snap?.selected) act("select_track", { id: h.track.id });
    if (h.row) return startSwipe(e, h.row.region, h.row.take.id);
    if (!h.region) return startMarquee(e, h);
    const r = h.region;
    // A take folder's triangle opens it; its badge opens the take folder menu.
    if (isFolder(r) && h.localY <= regionBar(h.laneH - 3).bar && !h.inLoop) {
      const bx = Math.max(r.start * pps, scrollRef.current!.scrollLeft);
      if (h.x < bx + 17) {
        toggleFolder(r.id);
        return;
      }
      if (h.x < bx + FOLDER_BADGE_W) {
        setState({ selectedRegions: [r.id], focus: "regions" });
        openMenu(e.clientX, e.clientY, takeFolderMenu(r));
        return;
      }
    }
    let sel = getState().selectedRegions;
    // Shift-clicking a selected region only takes it out of the selection; nothing is dragged.
    const deselect = e.shiftKey && sel.includes(r.id);
    if (e.shiftKey) {
      sel = deselect ? sel.filter((i) => i !== r.id) : [...sel, ...linkedRegions(r.id)];
    } else if (!sel.includes(r.id)) {
      sel = linkedRegions(r.id);
    }
    setState({ selectedRegions: sel, focus: "regions" });
    if (deselect) return;
    // A frozen track's regions can be selected but not moved, trimmed or faded.
    if (h.track.frozen) {
      refuseFrozen([h.track.id]);
      return;
    }

    const kind = grabKind(h);
    const startX = e.clientX;
    const startY = e.clientY;
    const startContentY = h.y;
    const startIndex = h.ti;
    let moved = false;
    dragRef.current = { kind, ids: kind === "move" ? sel : [r.id], dx: 0, dTrack: 0, grabbed: r.id };
    const fadeFrom = r.content.type === "audio" ? (kind === "fadeIn" ? r.content.fade_in : r.content.fade_out) : 0;
    // Dragging a crossfade's handle resizes it about its centre.
    const crossfading = kind === "fadeOut" && !!regionCrossfades(r.id)?.out;
    const snap = (v: number) => snapPos(project, v, getState().snapMode, getState().pxPerBeat);
    // An audio region can't be trimmed out past the ends of its file.
    const audio = r.content.type === "audio" ? r.content : null;
    const wave = audio ? waveCache.get(audio.clip) : undefined;
    const minStart = audio ? r.start - audio.offset : 0;
    // A take folder reaches as far as its takes do.
    const maxLength = audio && r.takes ? Math.max(0, ...r.takes.takes.map((t) => t.end)) - audio.offset : audio && wave ? wave.frames - audio.offset : Infinity;

    const move = (ev: MouseEvent) => {
      const dxPx = ev.clientX - startX;
      const dyPx = ev.clientY - startY;
      if (!moved && Math.abs(dxPx) < 3 && Math.abs(dyPx) < 3) return;
      moved = true;
      const d = dragRef.current!;
      d.dx = dxPx / pps;
      d.dTrack = visibleSteps(live.current.layout, startIndex, indexAt(live.current.layout, startContentY + dyPx));
      d.copy = kind === "move" && ev.altKey;
      if (kind === "fadeIn" || kind === "fadeOut") {
        const other = r.content.type === "audio" ? (kind === "fadeIn" ? r.content.fade_out : r.content.fade_in) : 0;
        const v = fadeFrom + (kind === "fadeIn" ? d.dx : -d.dx);
        d.fade = crossfading ? draggedFadeOut(fadeFrom, d.dx, true, r.length - other) : Math.round(Math.max(0, Math.min(r.length - other, v)));
        if (crossfading) setCrossfadeDrag({ region: r.id, length: d.fade });
      } else if (kind === "right") {
        d.start = r.start;
        d.length = Math.min(maxLength, Math.max(PEAK_BUCKET, snap(r.start + r.length + d.dx) - r.start));
      } else if (kind === "left") {
        const ns = Math.min(r.start + r.length - PEAK_BUCKET, Math.max(0, minStart, snap(r.start + d.dx)));
        d.start = ns;
        d.length = r.length - (ns - r.start);
      } else if (kind === "loop") {
        // Dragging back onto the region's own end turns the loop off.
        d.loopEnd = Math.max(r.start + r.length, snap(regionEnd(r) + d.dx));
      }
      requestDraw();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const d = dragRef.current!;
      dragRef.current = null;
      setCrossfadeDrag(null);
      if (!moved) {
        clickLocate(e, true);
        requestDraw();
        return;
      }
      if (kind === "move") {
        const newStart = Math.max(0, snap(r.start + d.dx));
        const delta = newStart - r.start;
        const tracks = project.tracks;
        const layout = live.current.layout;
        const moves = d.ids.flatMap((id) => {
          const ti = tracks.findIndex((t) => t.regions.some((x) => x.id === id));
          if (ti < 0) return [];
          const reg = tracks[ti].regions.find((x) => x.id === id)!;
          const target = tracks[dragTarget(layout, ti, d.dTrack, tracks)];
          return [{ reg, id, start: Math.max(0, Math.round(reg.start + delta)), track: target.id }];
        });
        if (d.copy) {
          // Option-drag: copies at the drop position in one undo step; the copies get selected.
          const items = moves.map((m) => [m.track, m.reg]);
          actMade("paste_regions", { items, offset: Math.round(delta), label: "Copy Regions" }).then((made) => made && setState({ selectedRegions: made }));
        } else {
          const plain = moves.map(({ id, start, track }) => ({ id, start, track }));
          moveAutomationAnswer(project, plain).then((automation) => act("move_regions", { moves: plain, automation }));
        }
      } else if (d.loopEnd !== undefined) {
        const end = Math.round(d.loopEnd);
        act("set_region_loop", { id: r.id, loopEnd: end > r.start + r.length ? end : null });
      } else if (d.fade !== undefined) {
        act("set_region_fades", { ids: [r.id], fadeIn: kind === "fadeIn" ? d.fade : null, fadeOut: kind === "fadeOut" ? d.fade : null });
      } else if (d.start !== undefined && d.length !== undefined) {
        act("resize_region", { id: r.id, start: Math.round(d.start), length: Math.round(d.length) });
      }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  /** A click without a drag moves the playhead to it, snapped like a region drag would be
   *  (Settings › Editing › Click Moves Playhead). */
  const clickLocate = (e: React.MouseEvent, onRegion: boolean) => {
    const mode = getSettings().ui.click_locates ?? "anywhere";
    if (mode === "off" || (onRegion && mode === "empty")) return;
    const { project, pps } = live.current;
    if (!project) return;
    const el = scrollRef.current!;
    const x = e.clientX - el.getBoundingClientRect().left + el.scrollLeft;
    act("locate", { pos: Math.round(Math.max(0, snapPos(project, x / pps, getState().snapMode, getState().pxPerBeat))) });
  };

  /** Rubber-band selection, from empty lane space: the regions the marquee touches are selected
   *  (Shift adds them to the selection). A click without a drag deselects, as before. */
  const startMarquee = (e: React.MouseEvent, h: NonNullable<ReturnType<typeof hit>>) => {
    const el = scrollRef.current!;
    const base = e.shiftKey ? getState().selectedRegions : [];
    const startX = e.clientX;
    const startY = e.clientY;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < 3 && Math.abs(ev.clientY - startY) < 3) return;
      moved = true;
      const rect = el.getBoundingClientRect();
      const x = ev.clientX - rect.left + el.scrollLeft;
      const y = ev.clientY - rect.top + el.scrollTop;
      const box = { left: Math.min(h.x, x), right: Math.max(h.x, x), top: Math.min(h.y, y), bottom: Math.max(h.y, y) };
      marqueeRef.current = box;
      const { project, pps, layout } = live.current;
      if (!project) return;
      const hits = new Set(base);
      project.tracks.forEach((t, i) => {
        // The lane itself: hidden and collapsed tracks have none, take rows don't count.
        const top = layout.tops[i];
        const lane = layout.heights[i] - layout.takeRows[i];
        if (lane <= 0 || top > box.bottom || top + lane < box.top) return;
        for (const r of t.regions) if (r.start * pps <= box.right && regionEnd(r) * pps >= box.left) hits.add(r.id);
      });
      setState({ selectedRegions: [...hits], focus: "regions" });
      requestDraw();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      marqueeRef.current = null;
      if (!moved && !e.shiftKey) setState({ selectedRegions: [] });
      if (!moved) clickLocate(e, false);
      requestDraw();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  /** Quick Swipe Comping: dragging across a take row puts that stretch in the comp; a click
   *  without a drag makes the whole take the one heard. */
  const startSwipe = (e: React.MouseEvent, r: Region & { takes: NonNullable<Region["takes"]> }, take: Id) => {
    const { project, pps } = live.current;
    if (!project) return;
    setState({ selectedRegions: [r.id], focus: "regions" });
    const el = scrollRef.current!;
    const left = el.getBoundingClientRect().left - el.scrollLeft;
    const snap = (v: number) => Math.min(r.start + r.length, Math.max(r.start, snapPos(project, v, getState().snapMode, getState().pxPerBeat)));
    const toSample = (cx: number) => (cx - left) / pps;
    const from = snap(toSample(e.clientX));
    const startX = e.clientX;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < 3) return;
      moved = true;
      const to = snap(toSample(ev.clientX));
      const shift = takeShift(r);
      swipeRef.current = { region: r.id, take, start: Math.min(from, to) + shift, end: Math.max(from, to) + shift };
      requestDraw();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const sw = swipeRef.current;
      swipeRef.current = null;
      if (!moved) act("select_take", { region: r.id, take });
      else if (sw && sw.end > sw.start) swipeComp(r, take, sw.start - takeShift(r), sw.end - takeShift(r));
      else requestDraw();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    // A right-click on the horizontal scroll bar (shown while the overview is hidden) offers
    // the overview back.
    const el = scrollRef.current!;
    if (!getState().showOverview && e.clientY >= el.getBoundingClientRect().top + el.clientHeight) {
      return openMenu(e.clientX, e.clientY, [{ label: "Show Overview", onClick: toggleOverview }]);
    }
    const h = hit(e);
    const { project } = live.current;
    if (!h || !project) return;
    if (showAutomation) return openMenu(e.clientX, e.clientY, automationMenu(h.track));
    if (h.row) return openMenu(e.clientX, e.clientY, takeFolderMenu(h.row.region));
    if (!h.region) {
      const pos = Math.max(0, Math.round(snapPos(project, h.sample, getState().snapMode, getState().pxPerBeat)));
      if (h.track && h.track.id !== getState().snap?.selected) act("select_track", { id: h.track.id });
      openMenu(e.clientX, e.clientY, laneMenu(h.track ?? null, pos));
      return;
    }
    const r = h.region;
    if (!getState().selectedRegions.includes(r.id)) setState({ selectedRegions: linkedRegions(r.id), focus: "regions" });
    openMenu(
      e.clientX,
      e.clientY,
      regionMenu(r, () => {
        const name = window.prompt("Region name", r.name);
        if (name) act("rename_region", { id: r.id, name });
      }),
    );
  };

  const onHover = (e: React.MouseEvent) => {
    notePointer({ x: e.clientX, y: e.clientY });
    if (dragRef.current) return;
    if (showAutomation && live.current.project) return automationHover(e, autoInput());
    const h = hit(e);
    const el = scrollRef.current!;
    if (live.current.project && toolHover(activeTool(e), h, { project: live.current.project, el, redraw: requestDraw })) return;
    if (!h?.region) {
      el.style.cursor = "default";
      if (hoverRef.current !== null) {
        hoverRef.current = null;
        requestDraw();
      }
      return;
    }
    if (hoverRef.current !== h.region.id) {
      hoverRef.current = h.region.id;
      requestDraw();
    }
    const kind = grabKind(h);
    el.style.cursor = kind === "loop" ? LOOP_CURSOR : kind === "left" || kind === "right" ? "ew-resize" : kind === "move" ? "default" : "col-resize";
  };

  return (
    <div className="arrange">
      <canvas ref={canvasRef} className="arrange-canvas" />
      <div className="playhead" ref={playheadRef} />
      <div
        className="arrange-scroll"
        ref={scrollRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          setViewport({ left: el.scrollLeft, top: el.scrollTop });
        }}
        onMouseDown={onMouseDown}
        onMouseMove={onHover}
        onMouseLeave={() => {
          automationLeave({ redraw: requestDraw });
          toolLeave(requestDraw);
        }}
        onContextMenu={onContextMenu}
      >
        <div style={{ width: contentW, height: contentH }} />
      </div>
    </div>
  );
}
