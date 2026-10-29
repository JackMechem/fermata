// The Reverb panel (builtin:reverb), plus the small controls it shares with the Delay panel.
// The controls are local until the shared ones in ./common.tsx land; they follow the same
// conventions: drag vertically (Shift for fine), double-click or Option-click for the default,
// and touch() around every drag so Touch/Latch automation records it.
import { useLayoutEffect, useRef } from "react";
import type { FxPanelProps, FxParamSpec } from "../../fx";
import { openMenuAt } from "../Menu";
import { pixelRatio } from "../../uiScale";
import { useStore } from "../../store";

// ---- shared controls ------------------------------------------------------------------------

/** Tempo-sync note values, mirroring NOTE_LABELS in crates/engine/src/fx/delay.rs. */
export const NOTE_LABELS = [
  "1/1", "1/1.", "1/1T", "1/2", "1/2.", "1/2T", "1/4", "1/4.", "1/4T",
  "1/8", "1/8.", "1/8T", "1/16", "1/16.", "1/16T", "1/32", "1/32.", "1/32T",
];

/** A note value's length in ms at a tempo (see `note_ms` in the engine). */
export function noteMs(index: number, tempo: number): number {
  const base = [4, 2, 1, 0.5, 0.25, 0.125][Math.min(5, Math.floor(index / 3))];
  const beats = index % 3 === 0 ? base : index % 3 === 1 ? base * 1.5 : (base * 2) / 3;
  return (beats * 60000) / Math.max(1, tempo);
}

const clamp01 = (x: number) => Math.min(1, Math.max(0, x));

export function specOf(p: FxPanelProps, id: number): FxParamSpec {
  return p.info.params.find((s) => s.id === id)!;
}

/** A parameter's current value, falling back to its default. */
export function val(p: FxPanelProps, id: number): number {
  return p.values[id] ?? specOf(p, id).default;
}

function toNorm(s: FxParamSpec, v: number): number {
  if (s.log && s.min > 0) return clamp01(Math.log(v / s.min) / Math.log(s.max / s.min));
  return clamp01((v - s.min) / (s.max - s.min));
}

function fromNorm(s: FxParamSpec, n: number): number {
  if (s.log && s.min > 0) return s.min * Math.pow(s.max / s.min, n);
  return s.min + (s.max - s.min) * n;
}

export function formatMs(v: number): string {
  if (v >= 1000) return `${(v / 1000).toFixed(v >= 10000 ? 1 : 2)} s`;
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ms`;
}

function formatValue(s: FxParamSpec, v: number): string {
  if (s.kind.type === "choice") return s.kind.labels[Math.round(v)] ?? "";
  switch (s.unit) {
    case "ms":
      return formatMs(v);
    case "hz":
      if (v >= 1000) return `${(v / 1000).toFixed(v >= 10000 ? 1 : 2)} kHz`;
      return v < 10 ? `${v.toFixed(2)} Hz` : `${Math.round(v)} Hz`;
    case "percent":
      return `${Math.round(v * 100)} %`;
    case "db":
      return `${v.toFixed(1)} dB`;
    default:
      return v.toFixed(2);
  }
}

function arcPoint(cx: number, cy: number, r: number, deg: number): [number, number] {
  const a = (deg * Math.PI) / 180;
  return [cx + r * Math.sin(a), cy - r * Math.cos(a)];
}

function arc(cx: number, cy: number, r: number, from: number, to: number): string {
  const [x0, y0] = arcPoint(cx, cy, r, from);
  const [x1, y1] = arcPoint(cx, cy, r, to);
  return `M${x0} ${y0} A${r} ${r} 0 ${to - from > 180 ? 1 : 0} 1 ${x1} ${y1}`;
}

/** A rotary control for one parameter. */
export function SpKnob({
  p,
  id,
  label,
  size = 38,
  format,
  disabled,
}: {
  p: FxPanelProps;
  id: number;
  label?: string;
  size?: number;
  format?: (v: number) => string;
  disabled?: boolean;
}) {
  const s = specOf(p, id);
  const v = val(p, id);
  const n = toNorm(s, v);
  const start = (e: React.MouseEvent) => {
    if (e.button !== 0 || disabled) return;
    e.preventDefault();
    if (e.altKey) {
      p.set(id, s.default);
      return;
    }
    p.touch(id, true);
    let pos = n;
    let last = e.clientY;
    const move = (ev: MouseEvent) => {
      pos = clamp01(pos + (last - ev.clientY) / (ev.shiftKey ? 900 : 180));
      last = ev.clientY;
      p.set(id, fromNorm(s, pos));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      p.touch(id, false);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  const c = size / 2;
  const r = c - 3;
  const angle = -135 + 270 * n;
  return (
    <div className={"sp-knob" + (size >= 56 ? " big" : "") + (disabled ? " disabled" : "")} title={`${s.name}: ${formatValue(s, v)}`}>
      <div className="sp-knob-label">{label ?? s.name}</div>
      <svg width={size} height={size} onMouseDown={start} onDoubleClick={() => !disabled && p.set(id, s.default)}>
        <path d={arc(c, c, r, -135, 135)} className="sp-knob-track" />
        {n > 0.003 && <path d={arc(c, c, r, -135, angle)} className="sp-knob-arc" />}
        <circle cx={c} cy={c} r={r - 4} className="knob-body" />
        <line x1={c} y1={c} x2={arcPoint(c, c, r - 5, angle)[0]} y2={arcPoint(c, c, r - 5, angle)[1]} className="knob-pointer" />
      </svg>
      <div className="sp-knob-value">{format ? format(v) : formatValue(s, v)}</div>
    </div>
  );
}

/** An on/off button for a toggle parameter. */
export function SpToggle({ p, id, label, title }: { p: FxPanelProps; id: number; label?: string; title?: string }) {
  const on = val(p, id) >= 0.5;
  return (
    <button type="button" className={"sp-toggle" + (on ? " on" : "")} title={title} onClick={() => p.set(id, on ? 0 : 1)}>
      {label ?? specOf(p, id).name}
    </button>
  );
}

/** A segmented control for a choice parameter. */
export function SpSegments({ p, id }: { p: FxPanelProps; id: number }) {
  const s = specOf(p, id);
  const labels = s.kind.type === "choice" ? s.kind.labels : [];
  const v = Math.round(val(p, id));
  return (
    <div className="segmented sp-segments">
      {labels.map((label, i) => (
        <button type="button" key={label} className={i === v ? "on" : ""} onClick={() => p.set(id, i)}>
          {label}
        </button>
      ))}
    </div>
  );
}

/** A note-value pop-up for a synced time, showing what it comes to at the project tempo. */
export function SpNote({ p, id, label }: { p: FxPanelProps; id: number; label: string }) {
  const tempo = useStore((s) => s.snap?.project.tempo ?? 120);
  const v = Math.round(val(p, id));
  return (
    <div className="sp-knob sp-note-wrap">
      <div className="sp-knob-label">{label}</div>
      <button
        type="button"
        className="sp-note"
        onMouseDown={(e) => {
          e.preventDefault();
          e.stopPropagation();
          openMenuAt(
            e.currentTarget,
            NOTE_LABELS.flatMap((l, i) => [
              ...(i > 0 && i % 3 === 0 ? [{ separator: true }] : []),
              { label: l, hint: formatMs(noteMs(i, tempo)), checked: i === v, onClick: () => p.set(id, i) },
            ]),
          );
        }}
      >
        {NOTE_LABELS[v]}
      </button>
      <div className="sp-knob-value">{formatMs(noteMs(v, tempo))}</div>
    </div>
  );
}

export function cssVar(name: string, fallback = "#888"): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

/** Sizes a canvas to its parent at the device pixel ratio and returns a 2D context and size. */
export function fitCanvas(cv: HTMLCanvasElement): { ctx: CanvasRenderingContext2D; w: number; h: number } {
  const parent = cv.parentElement!;
  const w = parent.clientWidth;
  const h = parent.clientHeight;
  const dpr = pixelRatio();
  if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) {
    cv.width = Math.round(w * dpr);
    cv.height = Math.round(h * dpr);
    cv.style.width = `${w}px`;
    cv.style.height = `${h}px`;
  }
  const ctx = cv.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  return { ctx, w, h };
}

/** Redraws a canvas when its inputs, the theme or its size change. */
export function useCanvas(draw: (cv: HTMLCanvasElement) => void, deps: unknown[]) {
  const ref = useRef<HTMLCanvasElement>(null);
  const themeId = useStore((s) => s.themeId);
  const uiScale = useStore((s) => s.uiScale);
  const drawRef = useRef(draw);
  drawRef.current = draw;
  useLayoutEffect(() => {
    if (ref.current) drawRef.current(ref.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, themeId, uiScale]);
  useLayoutEffect(() => {
    const cv = ref.current;
    if (!cv?.parentElement) return;
    const ro = new ResizeObserver(() => drawRef.current(cv));
    ro.observe(cv.parentElement);
    return () => ro.disconnect();
  }, []);
  return ref;
}

// ---- reverb ---------------------------------------------------------------------------------

/** Parameter ids (crates/engine/src/fx/reverb.rs). */
const R = {
  room: 0,
  predelay: 1,
  sync: 2,
  note: 3,
  size: 4,
  decay: 5,
  density: 6,
  diffusion: 7,
  damping: 8,
  lowDecay: 9,
  balance: 10,
  modRate: 11,
  modDepth: 12,
  width: 13,
  lowCut: 14,
  highCut: 15,
  freeze: 16,
  dry: 17,
  wet: 18,
};

/**
 * Each room's early pattern (taps, first and last tap in ms, seed), when the late reverb starts
 * after the pre-delay and its shortest line, in ms. Mirrors ROOMS in reverb.rs.
 */
const ROOMS: { er: [number, number, number, number]; late: number; line: number }[] = [
  { er: [18, 0.7, 13.0, 0x68e31da4], late: 0, line: 30 }, // Plate (its tank's first output)
  { er: [14, 2.5, 42.0, 0x02f6e2b1], late: 8, line: 9 }, // Room
  { er: [12, 8.0, 85.0, 0x9e3779b9], late: 22, line: 21 }, // Hall
  { er: [16, 3.5, 58.0, 0x7f4a7c15], late: 12, line: 13 }, // Chamber
  { er: [10, 16.0, 150.0, 0x5bd1e995], late: 40, line: 37 }, // Cathedral
  { er: [20, 1.1, 22.0, 0x1b873593], late: 3, line: 4.3 }, // Ambience
];

/** The early reflection taps (ms at 1× size, gain) for a channel, as `er_pattern` makes them. */
function erPattern(room: number, channel: number): [number, number][] {
  const [n, t0, t1, seed] = ROOMS[room].er;
  let s = (seed + channel * 7919) >>> 0;
  const rnd = () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return (s >>> 8) / 16777216;
  };
  const taps: [number, number][] = [];
  let energy = 0;
  for (let k = 0; k < n; k++) {
    const pos = (k + 0.2 + 0.6 * rnd()) / n;
    const amp = Math.pow(0.2, pos) * (0.55 + 0.45 * rnd());
    const sign = rnd() < 0.5 ? -1 : 1;
    taps.push([t0 + (t1 - t0) * pos, sign * amp]);
    energy += amp * amp;
  }
  const norm = energy > 0 ? 0.8 / Math.sqrt(energy) : 0;
  return taps.map(([t, g]) => [t, g * norm]);
}

const FLOOR_DB = -66;

function ReverbGraph({ p, predelayMs }: { p: FxPanelProps; predelayMs: number }) {
  const room = Math.round(val(p, R.room));
  const scale = 0.35 + 1.3 * val(p, R.size);
  const rt = val(p, R.decay) / 1000;
  const rtLow = rt * val(p, R.lowDecay);
  const rtHigh = rt * (1 - 0.85 * val(p, R.damping));
  const balance = val(p, R.balance);
  const early = Math.min(1, 2 * (1 - balance));
  const late = Math.min(1, 2 * balance);
  const freeze = val(p, R.freeze) >= 0.5;

  const ref = useCanvas(
    (cv) => {
      const { ctx, w, h } = fitCanvas(cv);
      const padL = 8;
      const padR = 8;
      const top = 14;
      const bottom = h - 16;
      const pd = predelayMs / 1000;
      const span = Math.min(30, Math.max(0.25, pd + (freeze ? 2.5 : 1.15 * Math.max(rt, rtLow))));
      // A square-root time axis keeps the early reflections readable next to a long tail.
      const x = (t: number) => padL + (w - padL - padR) * Math.sqrt(Math.max(0, t) / span);
      const y = (db: number) => top + ((bottom - top) * Math.min(-db, -FLOOR_DB)) / -FLOOR_DB;
      const line = cssVar("--line");
      const faint = cssVar("--faint");
      const accent = cssVar("--accent", "#4c9eff");
      const text = cssVar("--dim");
      ctx.font = "9.5px Inter, system-ui, sans-serif";

      // Grid: dB rows and time columns.
      ctx.fillStyle = line;
      for (let db = -12; db > FLOOR_DB; db -= 12) {
        ctx.globalAlpha = 0.5;
        ctx.fillRect(padL, Math.round(y(db)) + 0.5, w - padL - padR, 1);
      }
      ctx.globalAlpha = 1;
      ctx.textBaseline = "alphabetic";
      for (const t of [0.01, 0.05, 0.1, 0.25, 0.5, 1, 2, 5, 10, 20]) {
        if (t > span * 0.97) break;
        const gx = Math.round(x(t)) + 0.5;
        ctx.fillStyle = line;
        ctx.globalAlpha = 0.45;
        ctx.fillRect(gx, top, 1, bottom - top);
        ctx.globalAlpha = 1;
        ctx.fillStyle = faint;
        ctx.fillText(t < 1 ? `${Math.round(t * 1000)}` : `${t}s`, gx + 3, h - 4);
      }

      // Pre-delay: the quiet gap before anything sounds.
      if (pd > 0) {
        ctx.fillStyle = accent;
        ctx.globalAlpha = 0.07;
        ctx.fillRect(x(0), top, x(pd) - x(0), bottom - top);
        ctx.globalAlpha = 1;
      }

      // Late decay: builds up over the room's line lengths, then falls 60 dB per RT60.
      const t0 = pd + ((ROOMS[room].late + ROOMS[room].line) * scale) / 1000;
      const build = (room === 0 ? 0.06 : 0.04) * scale;
      const lateDb = 20 * Math.log10(Math.max(1e-4, late)) - 6;
      const curve = (rt60: number, t: number) => {
        if (t < t0) return FLOOR_DB;
        const rise = Math.min(1, (t - t0) / build);
        const fall = freeze ? 0 : (-60 * Math.max(0, t - t0 - build)) / rt60;
        return lateDb + 20 * Math.log10(Math.max(1e-4, rise)) * 0.5 + fall;
      };
      const steps = Math.max(60, Math.floor(w / 2));
      const pts: [number, number][] = [];
      for (let i = 0; i <= steps; i++) {
        const t = span * (i / steps) ** 2;
        pts.push([x(t), y(curve(rt, t))]);
      }
      if (late > 0.001) {
        ctx.beginPath();
        ctx.moveTo(pts[0][0], bottom);
        pts.forEach(([px, py]) => ctx.lineTo(px, py));
        ctx.lineTo(pts[pts.length - 1][0], bottom);
        ctx.closePath();
        const grad = ctx.createLinearGradient(0, top, 0, bottom);
        grad.addColorStop(0, accent);
        grad.addColorStop(1, "transparent");
        ctx.fillStyle = grad;
        ctx.globalAlpha = 0.22;
        ctx.fill();
        ctx.globalAlpha = 1;
        // The low and high bands, when they differ from the mids.
        const band = (rt60: number, dash: number[]) => {
          ctx.beginPath();
          for (let i = 0; i <= steps; i++) {
            const t = span * (i / steps) ** 2;
            (i ? ctx.lineTo : ctx.moveTo).call(ctx, x(t), y(curve(rt60, t)));
          }
          ctx.setLineDash(dash);
          ctx.strokeStyle = text;
          ctx.lineWidth = 1;
          ctx.globalAlpha = 0.7;
          ctx.stroke();
          ctx.setLineDash([]);
          ctx.globalAlpha = 1;
        };
        if (!freeze && Math.abs(rtLow - rt) > 0.01) band(rtLow, [1, 2]);
        if (!freeze && Math.abs(rtHigh - rt) > 0.01) band(rtHigh, [4, 3]);
        ctx.beginPath();
        pts.forEach(([px, py], i) => (i ? ctx.lineTo : ctx.moveTo).call(ctx, px, py));
        ctx.strokeStyle = accent;
        ctx.lineWidth = 1.5;
        ctx.stroke();
      }

      // Early reflections: one tick per tap, left above right.
      if (early > 0.001) {
        for (const ch of [1, 0]) {
          ctx.fillStyle = ch === 0 ? cssVar("--text") : accent;
          ctx.globalAlpha = ch === 0 ? 0.85 : 0.6;
          for (const [t, g] of erPattern(room, ch)) {
            const tx = Math.round(x(pd + (t * scale) / 1000)) + (ch ? 1 : 0);
            const db = 20 * Math.log10(Math.max(1e-4, Math.abs(g) * early)) - 2;
            const ty = y(db);
            ctx.fillRect(tx, ty, 1, bottom - ty);
          }
        }
        ctx.globalAlpha = 1;
      }

      // Readouts.
      ctx.fillStyle = text;
      ctx.textAlign = "right";
      ctx.fillText(freeze ? "FREEZE  ∞" : `RT60 ${formatMs(rt * 1000)}`, w - padR, top - 3);
      ctx.textAlign = "left";
      if (pd > 0.004) ctx.fillText(`PRE ${formatMs(predelayMs)}`, padL + 2, top - 3);
      ctx.fillStyle = line;
      ctx.fillRect(padL, bottom, w - padL - padR, 1);
    },
    [room, scale, rt, rtLow, rtHigh, early, late, freeze, predelayMs],
  );
  return (
    <div className="sp-graph">
      <canvas ref={ref} />
    </div>
  );
}

const lowDecayFormat = (v: number) => `×${v.toFixed(2)}`;
const balanceFormat = (v: number) => (v < 0.5 ? `E ${Math.round((1 - v) * 200 - 100)}` : v > 0.5 ? `L ${Math.round(v * 200 - 100)}` : "E = L");

/** The custom panel for builtin:reverb. */
export function ReverbPanel(p: FxPanelProps) {
  const tempo = useStore((s) => s.snap?.project.tempo ?? 120);
  const sync = val(p, R.sync) >= 0.5;
  const predelayMs = sync ? Math.min(1000, noteMs(Math.round(val(p, R.note)), tempo)) : val(p, R.predelay);
  return (
    <div className="sp-panel sp-reverb">
      <div className="sp-bar">
        <SpSegments p={p} id={R.room} />
        <div className="sp-spacer" />
        <SpToggle p={p} id={R.freeze} title="Hold the reverb tail indefinitely; new input is ignored" />
      </div>
      <div className="sp-main">
        <div className="sp-col">
          <SpKnob p={p} id={R.decay} size={64} />
          <SpKnob p={p} id={R.size} size={64} />
        </div>
        <ReverbGraph p={p} predelayMs={predelayMs} />
        <div className="sp-col">
          {sync ? <SpNote p={p} id={R.note} label="Pre-delay" /> : <SpKnob p={p} id={R.predelay} size={48} />}
          <SpToggle p={p} id={R.sync} label="Sync" title="Pre-delay in note values at the project tempo" />
        </div>
      </div>
      <div className="sp-row">
        <div className="sp-group">
          <div className="sp-group-name">Character</div>
          <SpKnob p={p} id={R.density} />
          <SpKnob p={p} id={R.diffusion} />
          <SpKnob p={p} id={R.balance} label="Early/Late" format={balanceFormat} />
        </div>
        <div className="sp-group">
          <div className="sp-group-name">Decay EQ</div>
          <SpKnob p={p} id={R.damping} />
          <SpKnob p={p} id={R.lowDecay} label="Low ×" format={lowDecayFormat} />
        </div>
        <div className="sp-group">
          <div className="sp-group-name">Modulation</div>
          <SpKnob p={p} id={R.modRate} label="Rate" />
          <SpKnob p={p} id={R.modDepth} label="Depth" />
        </div>
        <div className="sp-group">
          <div className="sp-group-name">Wet EQ</div>
          <SpKnob p={p} id={R.lowCut} />
          <SpKnob p={p} id={R.highCut} />
        </div>
        <div className="sp-group">
          <div className="sp-group-name">Output</div>
          <SpKnob p={p} id={R.width} />
          <SpKnob p={p} id={R.dry} />
          <SpKnob p={p} id={R.wet} />
        </div>
      </div>
    </div>
  );
}
