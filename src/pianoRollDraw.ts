// Canvas drawing for the Piano Roll: the note grid, the keyboard, the ruler and the
// velocity / controller lane. Everything draws only what is inside the visible range.
import type { CcKind, Id, Project, Region, Track } from "./api";
import { ccMax, sameKind, type PCc, type PNote } from "./midiEdit";
import type { TempoMap } from "./tempo";
import { mix, theme } from "./theme";
import { pixelRatio } from "./uiScale";

export const KEY_W = 64;
export const PR_RULER_H = 28;
/** Height of the cycle strip at the top of the ruler. */
export const CYCLE_H = 10;

/** Horizontal and vertical mapping of the note area (CSS pixels). */
export interface PrView {
  /** Pixels per sample. */
  pps: number;
  noteH: number;
  left: number;
  top: number;
  W: number;
  H: number;
}

export const xOf = (v: PrView, t: number) => t * v.pps - v.left;
export const yOf = (v: PrView, p: number) => (127 - p) * v.noteH - v.top;
export const timeAt = (v: PrView, x: number) => (x + v.left) / v.pps;
export const pitchAt = (v: PrView, y: number) => Math.max(0, Math.min(127, 127 - Math.floor((y + v.top) / v.noteH)));

export const BLACK = new Set([1, 3, 6, 8, 10]);
const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
/** Logic's octave numbering: middle C (MIDI 60) is C3. */
export const noteName = (p: number) => `${NAMES[p % 12]}${Math.floor(p / 12) - 2}`;

const FONT = "Inter, 'SF Pro Text', system-ui, sans-serif";

// Logic-style velocity colours: violet for soft notes through green and yellow to red.
const VEL_STOPS: [number, string][] = [
  [0, "#6a5be0"],
  [0.3, "#37a6c9"],
  [0.5, "#4fae4c"],
  [0.7, "#c9cf3a"],
  [0.85, "#ef7d2f"],
  [1, "#e5483a"],
];
const velCache: string[] = [];

export function velocityColor(v: number): string {
  const i = Math.max(0, Math.min(127, Math.round(v)));
  if (velCache[i]) return velCache[i];
  const f = i / 127;
  let k = 0;
  while (k < VEL_STOPS.length - 2 && f > VEL_STOPS[k + 1][0]) k++;
  const [a, ca] = VEL_STOPS[k];
  const [b, cb] = VEL_STOPS[k + 1];
  return (velCache[i] = mix(ca, cb, (f - a) / (b - a)));
}

/** CSS variables for canvas use, re-read when the theme changes. */
let cssCache: { theme: string; vars: Map<string, string> } = { theme: "", vars: new Map() };
export function cssVar(name: string, themeId: string, fallback: string): string {
  if (cssCache.theme !== themeId) cssCache = { theme: themeId, vars: new Map() };
  let v = cssCache.vars.get(name);
  if (v === undefined) {
    v = getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
    cssCache.vars.set(name, v);
  }
  return v;
}

/** Sizes a canvas's backing store to `W`×`H` CSS pixels and returns a context in CSS pixels. */
export function setupCanvas(cv: HTMLCanvasElement, W: number, H: number): CanvasRenderingContext2D {
  const dpr = pixelRatio();
  const w = Math.max(1, Math.round(W * dpr));
  const h = Math.max(1, Math.round(H * dpr));
  if (cv.width !== w || cv.height !== h) {
    cv.width = w;
    cv.height = h;
    cv.style.width = `${W}px`;
    cv.style.height = `${H}px`;
  }
  const ctx = cv.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return ctx;
}

/** Bar, beat and grid lengths in samples at the start of the project, the tempo map the
 *  lines follow, and the grid step in quarter notes. */
export interface Grid {
  bar: number;
  beat: number;
  grid: number;
  map: TempoMap;
  gridQ: number;
}

/** Vertical bar / beat / grid lines over `H` pixels, through the tempo map. */
function timeLines(ctx: CanvasRenderingContext2D, v: PrView, g: Grid, H: number) {
  for (const b of g.map.bars(Math.max(0, timeAt(v, 0)), timeAt(v, v.W))) {
    const beatQ = 4 / b.den;
    const barQ = beatQ * b.num;
    const barPx = (b.end - b.start) * v.pps;
    const beatPx = barPx / b.num;
    const gridPx = (barPx * g.gridQ) / barQ;
    const x = (q: number) => Math.round(xOf(v, g.map.sampleAt(b.startQ + q)));
    if (gridPx >= 7 && g.gridQ < beatQ) {
      ctx.fillStyle = mix(theme.gridBeat, theme.editorBg, 0.45, 1);
      ctx.globalAlpha = 0.6;
      for (let q = g.gridQ; q < barQ - 1e-6; q += g.gridQ) {
        if (Math.abs(q / beatQ - Math.round(q / beatQ)) < 1e-6) continue;
        ctx.fillRect(x(q), 0, 1, H);
      }
      ctx.globalAlpha = 1;
    }
    if (beatPx >= 6) {
      ctx.fillStyle = theme.gridBeat;
      for (let k = 1; k < b.num; k++) ctx.fillRect(x(k * beatQ), 0, 1, H);
    }
    ctx.fillStyle = theme.gridBar;
    ctx.fillRect(Math.round(xOf(v, b.start)), 0, 1, H);
  }
}

export interface GridScene {
  project: Project;
  regions: Region[];
  focus: Id | null;
  track: Track;
  /** Pitch classes of the project key, when the scale overlay is on. */
  scale: Set<number> | null;
}

/** Key rows, the regions' extent, the cycle range and the time grid. */
export function drawGrid(ctx: CanvasRenderingContext2D, v: PrView, g: Grid, s: GridScene) {
  ctx.fillStyle = theme.editorBg;
  ctx.fillRect(0, 0, v.W, v.H);
  const light = theme.region === "light";
  const pLo = Math.max(0, pitchAt(v, v.H));
  const pHi = Math.min(127, pitchAt(v, 0));
  for (let p = pLo; p <= pHi; p++) {
    const y = yOf(v, p);
    const pc = p % 12;
    let row = BLACK.has(pc) ? theme.pianoBlackRow : theme.pianoWhiteRow;
    if (s.scale) row = s.scale.has(pc) ? mix(theme.pianoWhiteRow, "#ffffff", light ? 0.4 : 0.035) : mix(theme.pianoBlackRow, "#000000", light ? 0.06 : 0.22);
    ctx.fillStyle = row;
    ctx.fillRect(0, y, v.W, v.noteH);
    // Octave line under each C, and a hairline between E and F.
    if (pc === 0 || pc === 5) {
      ctx.fillStyle = pc === 0 ? theme.gridBar : theme.gridBeat;
      ctx.fillRect(0, Math.round(y + v.noteH) - 1, v.W, 1);
    }
  }
  // Shade what lies outside every region; the edited region gets its colour along the edges.
  const shade = light ? "rgba(0,0,0,0.07)" : "rgba(0,0,0,0.26)";
  const spans = s.regions.map((r) => [xOf(v, r.start), xOf(v, r.start + r.length)] as const).sort((a, b) => a[0] - b[0]);
  ctx.fillStyle = shade;
  let cursor = 0;
  for (const [a, b] of spans) {
    if (a > cursor) ctx.fillRect(cursor, 0, Math.min(v.W, a) - cursor, v.H);
    cursor = Math.max(cursor, b);
  }
  if (cursor < v.W) ctx.fillRect(Math.max(0, cursor), 0, v.W - cursor, v.H);
  const c = s.project.cycle;
  if (c.enabled && c.end > c.start) {
    ctx.fillStyle = theme.cycleTint;
    ctx.fillRect(xOf(v, c.start), 0, (c.end - c.start) * v.pps, v.H);
  }
  timeLines(ctx, v, g, v.H);
  for (const r of s.regions) {
    const col = r.color ?? s.track.color;
    ctx.fillStyle = mix(col, theme.editorBg, r.id === s.focus ? 0 : 0.5, r.id === s.focus ? 0.9 : 0.6);
    ctx.fillRect(Math.round(xOf(v, r.start)), 0, 1, v.H);
    ctx.fillRect(Math.round(xOf(v, r.start + r.length)) - 1, 0, 1, v.H);
  }
}

export interface NoteStyle {
  /** Fill colour for a note (before selection / dimming). */
  color: (n: PNote) => string;
  /** The region being edited; notes of the others are drawn dimmed. Null: none dimmed. */
  focus: Id | null;
  mutedRegions: Set<Id>;
  themeId: string;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  r = Math.max(0, Math.min(r, w / 2, h / 2));
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

/** Draws notes inside the view. Selected notes go on top. */
export function drawNotes(ctx: CanvasRenderingContext2D, v: PrView, notes: PNote[], st: NoteStyle) {
  const t0 = timeAt(v, -2);
  const t1 = timeAt(v, v.W + 2);
  const pLo = pitchAt(v, v.H) - 1;
  const pHi = pitchAt(v, 0) + 1;
  const h = Math.max(2, v.noteH - 1);
  const labels = v.noteH >= 11;
  ctx.font = `600 ${Math.min(10, v.noteH - 3)}px ${FONT}`;
  ctx.textBaseline = "middle";
  const outline = theme.selectOutline;
  const ink = theme.region === "light" ? "rgba(255,255,255,0.95)" : theme.ink;
  const draw = (n: PNote) => {
    if (n.start > t1 || n.start + n.length < t0 || n.pitch < pLo || n.pitch > pHi) return;
    const x = xOf(v, n.start);
    const w = Math.max(3, n.length * v.pps);
    const y = yOf(v, n.pitch) + 0.5;
    const dim = (st.focus !== null && n.region !== st.focus) || st.mutedRegions.has(n.region);
    let fill = st.color(n);
    if (n.muted) fill = mix("#8e8e93", theme.editorBg, 0.45);
    else if (n.sel) fill = mix(fill, "#ffffff", 0.5);
    ctx.globalAlpha = dim && !n.sel ? 0.42 : 1;
    roundRect(ctx, x + 0.5, y, w - 1, h, Math.min(2.5, h / 3));
    ctx.fillStyle = fill;
    ctx.fill();
    ctx.lineWidth = n.sel ? 1.25 : 1;
    ctx.strokeStyle = n.sel ? outline : "rgba(0,0,0,0.45)";
    ctx.stroke();
    if (!n.muted && h >= 6 && w > 8) {
      // Logic's velocity bar: a line inside the note as long as the note is loud.
      ctx.fillStyle = "rgba(0,0,0,0.28)";
      ctx.fillRect(x + 2, y + h / 2 - 0.75, Math.max(1, ((w - 4) * n.velocity) / 127), 1.5);
    }
    if (labels && w > 26) {
      ctx.fillStyle = n.muted ? theme.text : ink;
      ctx.fillText(noteName(n.pitch), x + 4, y + h / 2 + 0.5, w - 6);
    }
  };
  for (const n of notes) if (!n.sel) draw(n);
  for (const n of notes) if (n.sel) draw(n);
  ctx.globalAlpha = 1;
}

/** The piano keyboard down the left side, with C labels in Logic's numbering. */
export function drawKeys(ctx: CanvasRenderingContext2D, v: PrView, pressed: number | null, hover: number | null, themeId: string) {
  const H = v.H;
  ctx.fillStyle = theme.pianoWhiteKey;
  ctx.fillRect(0, 0, KEY_W, H);
  const accent = cssVar("--accent", themeId, "#4c9eff");
  const blackW = Math.round(KEY_W * 0.6);
  const sep = mix(theme.pianoBlackKey, theme.pianoWhiteKey, 0.72);
  const pLo = pitchAt(v, H);
  const pHi = pitchAt(v, 0);
  ctx.font = `500 ${Math.max(8, Math.min(10, v.noteH - 1))}px ${FONT}`;
  ctx.textBaseline = "middle";
  ctx.textAlign = "right";
  for (let p = pLo; p <= pHi; p++) {
    const y = yOf(v, p);
    const pc = p % 12;
    const black = BLACK.has(pc);
    if (!black && (p === pressed || p === hover)) {
      ctx.fillStyle = p === pressed ? accent : mix(theme.pianoWhiteKey, accent, 0.18);
      ctx.fillRect(0, y, KEY_W, v.noteH);
    }
    if (black) {
      // The white keys meet halfway across a black key.
      ctx.fillStyle = sep;
      ctx.fillRect(blackW, Math.round(y + v.noteH / 2), KEY_W - blackW, 1);
    } else if (pc === 4 || pc === 11) {
      ctx.fillStyle = sep;
      ctx.fillRect(0, Math.round(y), KEY_W, 1);
    }
    if (pc === 0 && v.noteH >= 7) {
      ctx.fillStyle = p === pressed ? "#ffffff" : theme.pianoLabel;
      ctx.fillText(noteName(p), KEY_W - 4, y + v.noteH / 2 + 0.5);
    }
  }
  for (let p = pLo; p <= pHi; p++) {
    if (!BLACK.has(p % 12)) continue;
    const y = yOf(v, p);
    ctx.fillStyle = p === pressed ? accent : p === hover ? mix(theme.pianoBlackKey, accent, 0.35) : theme.pianoBlackKey;
    roundRect(ctx, -2, y + 0.5, blackW + 2, v.noteH - 1, 1.5);
    ctx.fill();
  }
  ctx.textAlign = "left";
  ctx.fillStyle = theme.rulerEdge;
  ctx.fillRect(KEY_W - 1, 0, 1, H);
}

/** Bar ruler with the cycle strip and the regions on the track. */
export function drawRuler(ctx: CanvasRenderingContext2D, v: PrView, g: Grid, s: GridScene, cyclePreview: { start: number; end: number } | null) {
  const W = v.W;
  const H = PR_RULER_H;
  ctx.fillStyle = theme.ruler;
  ctx.fillRect(0, 0, W, H);
  ctx.fillStyle = theme.rulerCycleZone;
  ctx.fillRect(0, 0, W, CYCLE_H);
  // Regions: a band under the numbers, the edited one in its colour.
  for (const r of s.regions) {
    const col = r.color ?? s.track.color;
    const x = xOf(v, r.start);
    const w = r.length * v.pps;
    if (x > W || x + w < 0) continue;
    ctx.fillStyle = r.id === s.focus ? col : mix(col, theme.ruler, 0.55);
    ctx.fillRect(x, H - 4, w, 3);
  }
  const c = cyclePreview ?? (s.project.cycle.end > s.project.cycle.start ? s.project.cycle : null);
  if (c) {
    ctx.fillStyle = cyclePreview || s.project.cycle.enabled ? theme.cycleOn : theme.cycleOff;
    ctx.beginPath();
    ctx.roundRect(xOf(v, c.start), 1.5, (c.end - c.start) * v.pps, CYCLE_H - 3, 2);
    ctx.fill();
  }
  ctx.font = `500 11px ${FONT}`;
  ctx.textBaseline = "alphabetic";
  for (const bar of g.map.bars(Math.max(0, timeAt(v, 0)), timeAt(v, W))) {
    const b = bar.bar - 1;
    const barPx = (bar.end - bar.start) * v.pps;
    const every = barPx >= 34 ? 1 : barPx >= 16 ? 2 : barPx >= 8 ? 4 : 8;
    const beatPx = barPx / bar.num;
    const x = Math.round(xOf(v, bar.start)) + 0.5;
    if (b % every === 0) {
      ctx.fillStyle = theme.rulerTick;
      ctx.fillRect(x, CYCLE_H + 1, 1, H - CYCLE_H - 6);
      ctx.fillStyle = theme.rulerText;
      ctx.fillText(String(b + 1), x + 3, H - 8);
    }
    if (beatPx >= 10) {
      for (let k = 1; k < bar.num; k++) {
        const bx = Math.round(xOf(v, g.map.sampleAt(bar.startQ + (k * 4) / bar.den)) + 0.5);
        ctx.fillStyle = theme.rulerMinor;
        ctx.fillRect(bx, H - 10, 1, 5);
        if (beatPx >= 48) {
          ctx.font = `400 10px ${FONT}`;
          ctx.fillText(`${b + 1}.${k + 1}`, bx + 3, H - 8);
          ctx.font = `500 11px ${FONT}`;
        }
      }
    }
  }
  ctx.fillStyle = theme.rulerEdge;
  ctx.fillRect(0, H - 1, W, 1);
}

/** A velocity or controller lane under the notes. */
export interface LaneScene {
  /** null = velocity. */
  kind: CcKind | null;
  notes: PNote[];
  cc: PCc[];
  color: (n: PNote) => string;
  focus: Id | null;
  /** A line being drawn (lane coordinates). */
  line: { x0: number; y0: number; x1: number; y1: number } | null;
  rubber: { x0: number; y0: number; x1: number; y1: number } | null;
  themeId: string;
  trackColor: string;
  /** Where each region ends (timeline samples): a controller holds its last value until then. */
  ends: Map<Id, number>;
}

export const LANE_PAD = 6;

/** Lane y for a value (0–max) and back. */
export const laneY = (H: number, value: number, max: number) => LANE_PAD + (1 - value / max) * (H - 2 * LANE_PAD);
export const laneValue = (H: number, y: number, max: number) => Math.round(Math.max(0, Math.min(1, 1 - (y - LANE_PAD) / (H - 2 * LANE_PAD))) * max);

export function drawLane(ctx: CanvasRenderingContext2D, v: PrView, g: Grid, H: number, s: LaneScene) {
  const W = v.W;
  ctx.fillStyle = theme.editorBg;
  ctx.fillRect(0, 0, W, H);
  timeLines(ctx, { ...v, H }, g, H);
  const accent = cssVar("--accent", s.themeId, "#4c9eff");
  ctx.fillStyle = theme.gridBeat;
  for (const f of [0, 0.5, 1]) ctx.fillRect(0, Math.round(LANE_PAD + f * (H - 2 * LANE_PAD)), W, 1);
  const t0 = timeAt(v, -6);
  const t1 = timeAt(v, W + 6);
  if (!s.kind) {
    const anySel = s.notes.some((n) => n.sel);
    const draw = (n: PNote) => {
      if (n.start < t0 || n.start > t1) return;
      const x = Math.round(xOf(v, n.start)) + 0.5;
      const y = laneY(H, n.velocity, 127);
      const dim = (s.focus !== null && n.region !== s.focus) || (anySel && !n.sel);
      const col = n.muted ? "#8e8e93" : n.sel ? mix(s.color(n), "#ffffff", 0.45) : s.color(n);
      ctx.globalAlpha = dim ? 0.38 : 1;
      ctx.fillStyle = col;
      ctx.fillRect(x - 0.5, y, 2, H - LANE_PAD - y);
      ctx.beginPath();
      ctx.arc(x + 0.5, y, n.sel ? 3.5 : 3, 0, Math.PI * 2);
      ctx.fill();
      if (n.sel) {
        ctx.strokeStyle = theme.selectOutline;
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    };
    for (const n of s.notes) if (!n.sel) draw(n);
    for (const n of s.notes) if (n.sel) draw(n);
    ctx.globalAlpha = 1;
  } else {
    const kind = s.kind;
    const max = ccMax(kind);
    if (kind.type === "pitch_bend") {
      ctx.fillStyle = theme.gridBar;
      ctx.fillRect(0, Math.round(laneY(H, 8192, max)), W, 1);
    }
    const col = s.trackColor;
    // Per region: a step line (values hold until the next event) and a dot per event.
    const byRegion = new Map<Id, PCc[]>();
    for (const c of s.cc) {
      if (!sameKind(c.kind, kind)) continue;
      const l = byRegion.get(c.region);
      if (l) l.push(c);
      else byRegion.set(c.region, [c]);
    }
    for (const [rid, list] of byRegion) {
      const dim = s.focus !== null && rid !== s.focus;
      const endX = (c: PCc) => Math.max(xOf(v, c.start), xOf(v, s.ends.get(rid) ?? c.start));
      ctx.globalAlpha = dim ? 0.4 : 1;
      ctx.strokeStyle = col;
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      let started = false;
      for (let i = 0; i < list.length; i++) {
        const c = list[i];
        const next = list[i + 1];
        if (next && next.start < t0) continue;
        if (c.start > t1) break;
        const x = xOf(v, c.start);
        const y = laneY(H, c.value, max);
        if (!started) {
          ctx.moveTo(x, y);
          started = true;
        } else ctx.lineTo(x, y);
        ctx.lineTo(next ? xOf(v, next.start) : endX(c), y);
        if (next) ctx.lineTo(xOf(v, next.start), laneY(H, next.value, max));
      }
      ctx.stroke();
      ctx.fillStyle = mix(col, "#ffffff", 0.2);
      ctx.globalAlpha = dim ? 0.4 : 0.22;
      // Area under the line, for legibility.
      for (let i = 0; i < list.length; i++) {
        const c = list[i];
        const next = list[i + 1];
        if ((next && next.start < t0) || c.start > t1) continue;
        const x = xOf(v, c.start);
        const xe = next ? xOf(v, next.start) : endX(c);
        const y = laneY(H, c.value, max);
        ctx.fillRect(x, y, xe - x, H - LANE_PAD - y);
      }
      ctx.globalAlpha = dim ? 0.4 : 1;
      for (const c of list) {
        if (c.start < t0 || c.start > t1) continue;
        const x = xOf(v, c.start);
        const y = laneY(H, c.value, max);
        ctx.fillStyle = c.sel ? "#ffffff" : col;
        ctx.beginPath();
        ctx.arc(x, y, c.sel ? 3.5 : 2.5, 0, Math.PI * 2);
        ctx.fill();
        if (c.sel) {
          ctx.strokeStyle = accent;
          ctx.lineWidth = 1.5;
          ctx.stroke();
        }
      }
    }
    ctx.globalAlpha = 1;
  }
  if (s.line) {
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5;
    ctx.setLineDash([4, 3]);
    ctx.beginPath();
    ctx.moveTo(s.line.x0, s.line.y0);
    ctx.lineTo(s.line.x1, s.line.y1);
    ctx.stroke();
    ctx.setLineDash([]);
  }
  if (s.rubber) drawRubber(ctx, s.rubber, s.themeId);
  ctx.fillStyle = theme.rulerEdge;
  ctx.fillRect(0, 0, W, 1);
}

export function drawRubber(ctx: CanvasRenderingContext2D, r: { x0: number; y0: number; x1: number; y1: number }, themeId: string) {
  const accent = cssVar("--accent", themeId, "#4c9eff");
  const x = Math.min(r.x0, r.x1);
  const y = Math.min(r.y0, r.y1);
  const w = Math.abs(r.x1 - r.x0);
  const h = Math.abs(r.y1 - r.y0);
  ctx.fillStyle = accent;
  ctx.globalAlpha = 0.14;
  ctx.fillRect(x, y, w, h);
  ctx.globalAlpha = 1;
  ctx.strokeStyle = accent;
  ctx.lineWidth = 1;
  ctx.strokeRect(Math.round(x) + 0.5, Math.round(y) + 0.5, Math.round(w), Math.round(h));
}

/** The lane's value scale, drawn in the header column. */
export function drawLaneScale(ctx: CanvasRenderingContext2D, H: number, kind: CcKind | null) {
  ctx.fillStyle = theme.pianoWhiteRow;
  ctx.fillRect(0, 0, KEY_W, H);
  ctx.fillStyle = theme.rulerEdge;
  ctx.fillRect(KEY_W - 1, 0, 1, H);
  ctx.fillRect(0, 0, KEY_W, 1);
  ctx.font = `500 9px ${FONT}`;
  ctx.fillStyle = theme.pianoLabel;
  ctx.textAlign = "right";
  const labels = kind?.type === "pitch_bend" ? ["+8191", "0", "−8192"] : kind?.type === "controller" && kind.cc === 64 ? ["On", "", "Off"] : ["127", "64", "0"];
  // The top of the column holds the lane picker, so the top label sits below it.
  if (H > 64) {
    ctx.textBaseline = "top";
    ctx.fillText(labels[0], KEY_W - 5, 24);
  }
  if (H > 46) {
    ctx.textBaseline = "middle";
    ctx.fillText(labels[1], KEY_W - 5, H / 2 + 6);
  }
  ctx.textBaseline = "bottom";
  ctx.fillText(labels[2], KEY_W - 5, H - LANE_PAD + 3);
  ctx.textAlign = "left";
}
