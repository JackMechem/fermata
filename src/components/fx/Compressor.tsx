// The Compressor's panel: Logic's layout of a gain-reduction meter over the circuit selector,
// big Threshold and Ratio controls, the timing and gain knobs, a transfer curve, and the side
// chain and output sections. Parameter ids and curve maths mirror
// crates/engine/src/fx/compressor.rs; keep them in step.
//
// The small controls at the top of this file (knob, segmented choice, toggle, meters) are
// shared with the Limiter's panel.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { FxPanelProps, FxParamSpec } from "../../fx";
import { onFrame } from "../../frame";
import { useStore } from "../../store";
import { theme } from "../../theme";
import { pixelRatio } from "../../uiScale";

// ---- shared controls ----------------------------------------------------------------------

export function cssVar(name: string, fallback = "#888"): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

/** Formats a value in a parameter's unit, the way Logic's plug-in windows print them. */
export function formatDyn(spec: FxParamSpec | undefined, v: number): string {
  if (!spec) return String(v);
  if (spec.kind.type === "choice") return spec.kind.labels[Math.round(v)] ?? "";
  if (spec.kind.type === "toggle") return v >= 0.5 ? "On" : "Off";
  switch (spec.unit) {
    case "db":
      return `${v > 0.05 ? "+" : ""}${v.toFixed(1)} dB`;
    case "ms":
      return formatMs(v);
    case "hz":
      return v >= 1000 ? `${(v / 1000).toFixed(v >= 10000 ? 1 : 2)} kHz` : `${Math.round(v)} Hz`;
    case "ratio":
      return `${v >= 10 ? v.toFixed(0) : v.toFixed(1)}:1`;
    case "percent":
      return `${Math.round(v * 100)} %`;
    default:
      return v.toFixed(2);
  }
}

export function formatMs(v: number): string {
  if (v < 1) return `${Math.round(v * 1000)} µs`;
  if (v < 10) return `${v.toFixed(1)} ms`;
  if (v < 1000) return `${Math.round(v)} ms`;
  return `${(v / 1000).toFixed(v < 10000 ? 2 : 1)} s`;
}

const KNOB_SWEEP = 135;

function toNorm(v: number, min: number, max: number, log: boolean) {
  if (max <= min) return 0;
  const n = log && min > 0 ? Math.log(v / min) / Math.log(max / min) : (v - min) / (max - min);
  return Math.min(1, Math.max(0, n));
}

function fromNorm(n: number, min: number, max: number, log: boolean) {
  return log && min > 0 ? min * Math.pow(max / min, n) : min + (max - min) * n;
}

/** A point on a knob's circle, `deg` clockwise from the top. */
function polar(c: number, r: number, deg: number): [number, number] {
  const a = ((deg - 90) * Math.PI) / 180;
  return [c + r * Math.cos(a), c + r * Math.sin(a)];
}

function arc(c: number, r: number, from: number, to: number) {
  const [x0, y0] = polar(c, r, from);
  const [x1, y1] = polar(c, r, to);
  return `M${x0} ${y0} A${r} ${r} 0 ${to - from > 180 ? 1 : 0} 1 ${x1} ${y1}`;
}

export interface DynKnobProps {
  label: string;
  value: number;
  min: number;
  max: number;
  log?: boolean;
  /** Double-click resets to this. */
  def: number;
  format: (v: number) => string;
  onChange: (v: number) => void;
  onGrab?: (held: boolean) => void;
  /** Switch positions: the knob snaps to the nearest and draws a tick at each. */
  steps?: number[];
  size?: number;
  disabled?: boolean;
  title?: string;
  /** Text shown instead of the value when disabled. */
  note?: string;
}

/**
 * A rotary control dragged vertically (Shift for fine); double-click resets. Works out each
 * move from the value at mouse-down, so rounding never eats small drags.
 */
export function DynKnob({ label, value, min, max, log = false, def, format, onChange, onGrab, steps, size = 40, disabled, title, note }: DynKnobProps) {
  const n = toNorm(value, min, max, log);
  const angle = -KNOB_SWEEP + n * 2 * KNOB_SWEEP;
  const c = size / 2;
  const r = c - 3;
  const snap = (v: number) => {
    if (!steps?.length) return v;
    const nv = toNorm(v, min, max, log);
    return steps.reduce((best, s) => (Math.abs(toNorm(s, min, max, log) - nv) < Math.abs(toNorm(best, min, max, log) - nv) ? s : best), steps[0]);
  };
  const down = (e: React.MouseEvent) => {
    if (disabled || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const n0 = n;
    let px = 0;
    let last = e.clientY;
    onGrab?.(true);
    const move = (ev: MouseEvent) => {
      px += (last - ev.clientY) * (ev.shiftKey ? 0.15 : 1);
      last = ev.clientY;
      const nn = Math.min(1, Math.max(0, n0 + px / 180));
      const v = snap(fromNorm(nn, min, max, log));
      if (v !== value) onChange(v);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      onGrab?.(false);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  return (
    <div className={"dyn-knob" + (disabled ? " disabled" : "")} title={title ?? `${label} · drag · Shift for fine · double-click to reset`}>
      <svg
        width={size}
        height={size}
        onMouseDown={down}
        onDoubleClick={() => {
          if (disabled) return;
          onGrab?.(true);
          onChange(snap(def));
          onGrab?.(false);
        }}
      >
        <path d={arc(c, r, -KNOB_SWEEP, KNOB_SWEEP)} className="dyn-knob-track" />
        {!disabled && n > 0.002 && <path d={arc(c, r, -KNOB_SWEEP, angle)} className="dyn-knob-arc" />}
        {steps?.map((s) => {
          const [x, y] = polar(c, r, -KNOB_SWEEP + toNorm(s, min, max, log) * 2 * KNOB_SWEEP);
          return <circle key={s} cx={x} cy={y} r={1.1} className="dyn-knob-step" />;
        })}
        <circle cx={c} cy={c} r={r - 4.5} className="dyn-knob-body" />
        <line x1={polar(c, r * 0.18, angle)[0]} y1={polar(c, r * 0.18, angle)[1]} x2={polar(c, r - 6, angle)[0]} y2={polar(c, r - 6, angle)[1]} className="dyn-knob-pointer" />
      </svg>
      <div className="dyn-knob-label">{label}</div>
      <div className="dyn-knob-value">{disabled ? note ?? "—" : format(value)}</div>
    </div>
  );
}

/** A knob bound to one of the effect's parameters, optionally with a narrower range. */
export function ParamKnob({
  props,
  id,
  label,
  range,
  ...rest
}: { props: FxPanelProps; id: number; label?: string; range?: [number, number] } & Partial<Omit<DynKnobProps, "value" | "onChange" | "onGrab" | "min" | "max">>) {
  const spec = props.info.params.find((p) => p.id === id);
  if (!spec) return null;
  const [min, max] = range ?? [spec.min, spec.max];
  const raw = props.values[id] ?? spec.default;
  return (
    <DynKnob
      label={label ?? spec.name}
      value={Math.min(max, Math.max(min, raw))}
      min={min}
      max={max}
      log={spec.log}
      def={Math.min(max, Math.max(min, spec.default))}
      format={(v) => formatDyn(spec, v)}
      onChange={(v) => props.set(id, v)}
      onGrab={(on) => props.touch(id, on)}
      {...rest}
    />
  );
}

/** A row of buttons for a choice, lit on the current one. */
export function Segmented({ labels, value, onChange, title, vertical }: { labels: string[]; value: number; onChange: (i: number) => void; title?: string; vertical?: boolean }) {
  return (
    <div className={"dyn-seg" + (vertical ? " vertical" : "")} title={title}>
      {labels.map((l, i) => (
        <button key={l + i} className={i === Math.round(value) ? "on" : ""} onClick={() => onChange(i)}>
          {l}
        </button>
      ))}
    </div>
  );
}

export function ParamChoice({ props, id, labels, vertical }: { props: FxPanelProps; id: number; labels?: string[]; vertical?: boolean }) {
  const spec = props.info.params.find((p) => p.id === id);
  if (!spec || spec.kind.type !== "choice") return null;
  return (
    <Segmented
      labels={labels ?? spec.kind.labels}
      value={props.values[id] ?? spec.default}
      title={spec.name}
      vertical={vertical}
      onChange={(i) => {
        props.touch(id, true);
        props.set(id, i);
        props.touch(id, false);
      }}
    />
  );
}

export function ParamToggle({ props, id, label, title }: { props: FxPanelProps; id: number; label?: string; title?: string }) {
  const spec = props.info.params.find((p) => p.id === id);
  if (!spec) return null;
  const on = (props.values[id] ?? spec.default) >= 0.5;
  return (
    <button
      className={"dyn-toggle" + (on ? " on" : "")}
      title={title ?? spec.name}
      onClick={() => {
        props.touch(id, true);
        props.set(id, on ? 0 : 1);
        props.touch(id, false);
      }}
    >
      <span className="dyn-led" />
      {label ?? spec.name}
    </button>
  );
}

/** A labelled group of controls, like the framed sections on Logic's plug-in faces. */
export function Section({ title, children, className }: { title: string; children: React.ReactNode; className?: string }) {
  return (
    <div className={"dyn-section" + (className ? " " + className : "")}>
      <div className="dyn-section-title">{title}</div>
      <div className="dyn-section-body">{children}</div>
    </div>
  );
}

const BAR_FLOOR = -60;

/**
 * A level meter in dBFS for one meter value, with a 1.5 s peak hold and a readout that holds
 * the maximum until clicked. The value arrives about 30 times a second; the bar eases between.
 * With `grow`, `length` is the bar's smallest and it fills the space its column has.
 */
export function LevelBar({ db, label, horizontal = false, length = 120, grow = false }: { db: number; label: string; horizontal?: boolean; length?: number; grow?: boolean }) {
  const bar = useRef<HTMLDivElement>(null);
  const fill = useRef<HTMLDivElement>(null);
  const hold = useRef<HTMLDivElement>(null);
  const target = useRef(db);
  target.current = db;
  const [max, setMax] = useState(-Infinity);
  // The channel meters' colours, so the bar follows the theme.
  useStore((s) => s.themeId);
  const gradient = `linear-gradient(to ${horizontal ? "right" : "top"}, ${theme.meterStops.join(", ")})`;
  useEffect(() => {
    let shown = BAR_FLOOR;
    let held = BAR_FLOOR;
    let heldAt = 0;
    let last = performance.now();
    return onFrame((now) => {
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      const t = Math.max(BAR_FLOOR, target.current);
      shown = t > shown ? t : Math.max(t, shown - 20 * dt);
      if (t >= held || now - heldAt > 1500) {
        held = t;
        heldAt = now;
      }
      const f = (v: number) => `${((Math.min(6, v) - BAR_FLOOR) / (6 - BAR_FLOOR)) * 100}%`;
      if (fill.current) fill.current.style[horizontal ? "width" : "height"] = f(shown);
      if (hold.current) {
        hold.current.style[horizontal ? "left" : "bottom"] = f(held);
        hold.current.classList.toggle("over", held > 0);
      }
    });
  }, [horizontal]);
  useEffect(() => {
    if (db > max) setMax(db);
  }, [db, max]);
  // A growing bar's colours stretch with it, so the zones stay at the same levels.
  useLayoutEffect(() => {
    const el = bar.current;
    if (!grow || !el) return;
    const ro = new ResizeObserver(([e]) => el.style.setProperty("--dyn-bar-len", `${horizontal ? e.contentRect.width : e.contentRect.height}px`));
    ro.observe(el);
    return () => ro.disconnect();
  }, [grow, horizontal]);
  const dim = horizontal ? "width" : "height";
  const barStyle = grow ? { [horizontal ? "minWidth" : "minHeight"]: length, flex: "1 1 auto" } : { [dim]: length, ["--dyn-bar-len" as string]: `${length}px` };
  return (
    <div className={"dyn-level" + (horizontal ? " horizontal" : "")}>
      <div className="dyn-level-bar" ref={bar} style={barStyle}>
        <div className="dyn-level-fill" ref={fill} style={{ backgroundImage: gradient }} />
        <div className="dyn-level-hold" ref={hold} />
        <div className="dyn-level-zero" style={horizontal ? { left: `${(-BAR_FLOOR / (6 - BAR_FLOOR)) * 100}%` } : { bottom: `${(-BAR_FLOOR / (6 - BAR_FLOOR)) * 100}%` }} />
      </div>
      <div className="dyn-level-label">{label}</div>
      <button className={"dyn-level-peak" + (max > 0 ? " over" : "")} title="Peak · click to reset" onClick={() => setMax(-Infinity)}>
        {max <= -100 || !isFinite(max) ? "−∞" : max.toFixed(1)}
      </button>
    </div>
  );
}

/** GR meter scale: where `gr` dB sits, 0..1 from no reduction to the end of the scale. */
function grPos(gr: number, top: number) {
  return Math.log10(1 + Math.max(0, gr)) / Math.log10(1 + top);
}

/**
 * Gain reduction on a bar that grows leftwards from 0 dB, as on Logic's Platinum face and the
 * Limiter. Eases at VU-like speed so it reads as movement rather than flicker.
 */
export function GrBar({ gr, top = 30, marks = [0, 1, 2, 3, 6, 10, 20, 30] }: { gr: number; top?: number; marks?: number[] }) {
  const fill = useRef<HTMLDivElement>(null);
  const target = useRef(gr);
  target.current = gr;
  useEffect(() => {
    let shown = 0;
    let last = performance.now();
    return onFrame((now) => {
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      const t = target.current;
      shown += (t - shown) * Math.min(1, dt / (t > shown ? 0.03 : 0.12));
      if (fill.current) fill.current.style.width = `${grPos(shown, top) * 100}%`;
    });
  }, [top]);
  return (
    <div className="dyn-grbar">
      <div className="dyn-grbar-track">
        <div className="dyn-grbar-fill" ref={fill} />
      </div>
      <div className="dyn-grbar-scale">
        {marks.map((m) => (
          <span key={m} style={{ right: `${grPos(m, top) * 100}%` }}>
            {m}
          </span>
        ))}
      </div>
    </div>
  );
}

/**
 * An analogue gain-reduction meter: a needle resting at 0 on the right that swings left with
 * reduction, with VU ballistics (about 300 ms to settle).
 */
function GrNeedle({ gr }: { gr: number }) {
  const needle = useRef<SVGLineElement>(null);
  const target = useRef(gr);
  target.current = gr;
  const W = 300;
  const H = 104;
  const cx = W / 2;
  const cy = H + 46;
  const R = 128;
  const SWEEP = 42;
  const top = 20;
  const angle = (g: number) => SWEEP - grPos(g, top) * 2 * SWEEP;
  useEffect(() => {
    let shown = 0;
    let vel = 0;
    let last = performance.now();
    return onFrame((now) => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      // A damped mass on a spring, like a moving-coil movement: a slight, quick overshoot.
      const k = 260;
      const damp = 26;
      vel += (k * (target.current - shown) - damp * vel) * dt;
      shown = Math.max(-0.4, Math.min(top + 1, shown + vel * dt));
      if (needle.current) needle.current.setAttribute("transform", `rotate(${angle(shown)} ${cx} ${cy})`);
    });
  }, []);
  const marks = [0, 1, 2, 3, 5, 7, 10, 15, 20];
  return (
    <svg className="dyn-vu" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="xMidYMid meet">
      <path d={`M${polar(cx, R, -SWEEP - 2).join(" ")} A${R} ${R} 0 0 1 ${polar(cx, R, SWEEP + 2).join(" ")}`} transform={`translate(0 ${cy - cx})`} className="dyn-vu-arc" />
      <path d={`M${polar(cx, R, -SWEEP).join(" ")} A${R} ${R} 0 0 1 ${polar(cx, R, angle(10)).join(" ")}`} transform={`translate(0 ${cy - cx})`} className="dyn-vu-arc hot" />
      {marks.map((m) => {
        const a = angle(m);
        const [x0, y0] = polar(cx, R, a);
        const [x1, y1] = polar(cx, R + (m % 5 === 0 || m < 4 ? 9 : 5), a);
        const [tx, ty] = polar(cx, R + 19, a);
        return (
          <g key={m} transform={`translate(0 ${cy - cx})`}>
            <line x1={x0} y1={y0} x2={x1} y2={y1} className="dyn-vu-tick" />
            <text x={tx} y={ty} className="dyn-vu-num">
              {m}
            </text>
          </g>
        );
      })}
      <text x={cx} y={H - 30} className="dyn-vu-legend">
        GAIN REDUCTION · dB
      </text>
      <line ref={needle} x1={cx} y1={cy} x2={cx} y2={cy - R - 8} className="dyn-vu-needle" transform={`rotate(${SWEEP} ${cx} ${cy})`} />
      <rect x={0} y={H - 14} width={W} height={14} className="dyn-vu-foot" />
    </svg>
  );
}

// ---- compressor model (mirrors compressor.rs) ----------------------------------------------

const P = {
  circuit: 0,
  threshold: 1,
  ratio: 2,
  knee: 3,
  attack: 4,
  release: 5,
  autoRelease: 6,
  makeup: 7,
  autoGain: 8,
  detector: 9,
  distortion: 10,
  mix: 11,
  output: 12,
  link: 13,
  scMode: 14,
  scFreq: 15,
  listen: 16,
  lookahead: 17,
  fetRatio: 18,
  optoMode: 19,
  timeConstant: 20,
} as const;

enum Circuit {
  Platinum = 0,
  Vca = 1,
  Fet = 2,
  Opto = 3,
  VariMu = 4,
}

const CIRCUITS: { name: string; short: string; key: string; about: string }[] = [
  { name: "Platinum Digital", short: "Platinum", key: "platinum", about: "Clean, exact feedforward compression" },
  { name: "Studio VCA", short: "Studio VCA", key: "vca", about: "SSL / dbx-style VCA: punchy, stepped attack and release, Auto release" },
  { name: "Studio FET", short: "Studio FET", key: "fet", about: "1176-style FET feedback compressor: very fast, ratio buttons, All mode" },
  { name: "Classic Opto", short: "Classic Opto", key: "opto", about: "LA-2A-style optical cell: soft, program-dependent two-stage release" },
  { name: "Vintage Vari-Mu", short: "Vari-Mu", key: "varimu", about: "Fairchild-style variable-mu tube: very soft knee, time-constant positions" },
];

const VCA_ATTACKS = [0.1, 0.3, 1, 3, 10, 30];
const VCA_RELEASES = [100, 300, 600, 1200];
const FET_ATTACK: [number, number] = [0.02, 0.8];
const FET_RELEASE: [number, number] = [50, 1100];
/** (feedback slope S = R - 1, input-side knee dB) per FET ratio button. */
const FET_CURVES: [number, number][] = [
  [3, 8],
  [7, 6],
  [11, 5],
  [19, 4],
  [19, 4],
];
const ABI_SHIFT = 3;
const ABI_KNEE = 4;
const ABI_FOLD = 1.15;
const VARI_MU_WIDTH = 8;
const VARI_MU_TC = [
  { attack: 0.2, release: "0.3 s" },
  { attack: 0.2, release: "0.8 s" },
  { attack: 0.4, release: "2 s" },
  { attack: 0.4, release: "5 s" },
  { attack: 0.4, release: "2–10 s" },
  { attack: 0.2, release: "0.3–25 s" },
];

function kneeRed(over: number, slope: number, knee: number) {
  if (2 * over <= -knee) return 0;
  if (knee > 0 && 2 * over < knee) {
    const t = over + knee / 2;
    return (slope * t * t) / (2 * knee);
  }
  return slope * over;
}

function kneeSlope(over: number, slope: number, knee: number) {
  if (2 * over <= -knee) return 0;
  if (knee > 0 && 2 * over < knee) return (slope * (over + knee / 2)) / knee;
  return slope;
}

function programRed(over: number, slope: number, w: number) {
  const z = over / w;
  const sp = z > 20 ? z : z < -20 ? Math.exp(z) : Math.log1p(Math.exp(z));
  return slope * w * sp;
}

/** Steady-state gain reduction (dB) for a detector level `x` dB — `static_reduction` in Rust. */
function staticReduction(v: (id: number) => number, x: number): number {
  const circuit = Math.round(v(P.circuit));
  const thr = v(P.threshold);
  const ratio = Math.max(1, v(P.ratio));
  switch (circuit) {
    case Circuit.Fet: {
      const mode = Math.round(v(P.fetRatio));
      const [s, kIn] = FET_CURVES[mode] ?? FET_CURVES[0];
      const kn = (2 * kIn) / (s + 2);
      const abi = mode >= 4;
      const xo = x - (abi ? thr - ABI_SHIFT : thr);
      const extra = abi ? ABI_FOLD * Math.max(0, xo - ABI_KNEE) : 0;
      let c = 0;
      for (let i = 0; i < 30; i++) {
        const f = c - kneeRed(xo - c, s, kn) - extra;
        c -= f / (1 + kneeSlope(xo - c, s, kn));
      }
      return Math.max(0, c);
    }
    case Circuit.Opto: {
      const limit = v(P.optoMode) >= 0.5;
      return programRed(x - thr, limit ? 1 - 1 / 12 : 1 - 1 / 3, limit ? 3 : 5);
    }
    case Circuit.VariMu:
      return programRed(x - thr, 1 - 1 / ratio, VARI_MU_WIDTH);
    default:
      return kneeRed(x - thr, 1 - 1 / ratio, Math.max(0, v(P.knee)));
  }
}

// ---- transfer curve ------------------------------------------------------------------------

const G_MIN = -60;
const G_MAX = 0;

function TransferGraph({ v, level, size = 150 }: { v: (id: number) => number; level: number; size?: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const themeId = useStore((s) => s.themeId);
  const uiScale = useStore((s) => s.uiScale);
  const key = [P.circuit, P.threshold, P.ratio, P.knee, P.fetRatio, P.optoMode].map(v).join(",");
  useLayoutEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const dpr = pixelRatio();
    cv.width = size * dpr;
    cv.height = size * dpr;
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, size, size);
    const px = (db: number) => ((db - G_MIN) / (G_MAX - G_MIN)) * size;
    const py = (db: number) => size - px(db);
    const line = cssVar("--line");
    const faint = cssVar("--faint");
    const accent = cssVar("--accent");

    ctx.lineWidth = 1;
    for (let db = G_MIN; db <= G_MAX; db += 12) {
      ctx.strokeStyle = line;
      ctx.globalAlpha = 0.6;
      ctx.beginPath();
      ctx.moveTo(Math.round(px(db)) + 0.5, 0);
      ctx.lineTo(Math.round(px(db)) + 0.5, size);
      ctx.moveTo(0, Math.round(py(db)) + 0.5);
      ctx.lineTo(size, Math.round(py(db)) + 0.5);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    // Unity, for reference.
    ctx.setLineDash([3, 3]);
    ctx.strokeStyle = faint;
    ctx.beginPath();
    ctx.moveTo(0, size);
    ctx.lineTo(size, 0);
    ctx.stroke();
    ctx.setLineDash([]);
    // Threshold.
    const thr = v(P.threshold) - (Math.round(v(P.circuit)) === Circuit.Fet && Math.round(v(P.fetRatio)) >= 4 ? ABI_SHIFT : 0);
    ctx.fillStyle = accent;
    ctx.globalAlpha = 0.18;
    ctx.fillRect(px(thr), 0, 1, size);
    ctx.globalAlpha = 1;

    ctx.beginPath();
    for (let i = 0; i <= size; i++) {
      const x = G_MIN + (i / size) * (G_MAX - G_MIN);
      const y = x - staticReduction(v, x);
      if (i) ctx.lineTo(i, py(y));
      else ctx.moveTo(i, py(y));
    }
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    ctx.stroke();

    ctx.font = "9px Inter, system-ui, sans-serif";
    ctx.fillStyle = faint;
    ctx.textBaseline = "top";
    ctx.fillText("out", 4, 4);
    ctx.textBaseline = "bottom";
    ctx.textAlign = "right";
    ctx.fillText("in", size - 4, size - 3);
    ctx.textAlign = "left";
  }, [key, size, themeId, uiScale]);

  const showDot = level > G_MIN;
  const out = level - staticReduction(v, level);
  const dotX = ((Math.min(G_MAX, level) - G_MIN) / (G_MAX - G_MIN)) * 100;
  const dotY = ((Math.min(G_MAX, Math.max(G_MIN, out)) - G_MIN) / (G_MAX - G_MIN)) * 100;
  return (
    <div className="dyn-graph" style={{ width: size, height: size }} title="Transfer curve: output level against input level">
      <canvas ref={ref} style={{ width: size, height: size }} />
      {showDot && <div className="dyn-graph-dot" style={{ left: `${dotX}%`, bottom: `${dotY}%` }} />}
    </div>
  );
}

// ---- the panel -----------------------------------------------------------------------------

export function CompressorPanel(props: FxPanelProps) {
  const { info, values, meters } = props;
  const v = (id: number) => values[id] ?? info.params.find((p) => p.id === id)?.default ?? 0;
  const circuit = Math.round(v(P.circuit)) as Circuit;
  const meta = CIRCUITS[circuit] ?? CIRCUITS[0];
  const [mIn, mOut, mGr, mLevel] = [meters[0] ?? -120, meters[1] ?? -120, meters[2] ?? 0, meters[3] ?? -120];
  const setNow = (id: number, value: number) => {
    props.touch(id, true);
    props.set(id, value);
    props.touch(id, false);
  };

  const thrSpec = info.params.find((p) => p.id === P.threshold);
  const opto = circuit === Circuit.Opto;
  const fet = circuit === Circuit.Fet;
  const varimu = circuit === Circuit.VariMu;
  const digital = circuit === Circuit.Platinum || circuit === Circuit.Vca;
  const tc = VARI_MU_TC[Math.round(v(P.timeConstant))] ?? VARI_MU_TC[0];

  // The meter row takes the height a taller panel has to spare; the transfer curve grows with
  // it (square, up to two fifths of the row) while the GR meter takes the rest of the width.
  const top = useRef<HTMLDivElement>(null);
  const [graph, setGraph] = useState(136);
  useLayoutEffect(() => {
    const el = top.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => {
      const { width, height } = e.contentRect;
      setGraph(Math.max(136, Math.min(Math.floor(height) - 6, Math.round(0.4 * width))));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const ratioControl = fet ? (
    <div className="dyn-ratio-buttons">
      <div className="dyn-knob-label">Ratio</div>
      <ParamChoice props={props} id={P.fetRatio} labels={["4", "8", "12", "20", "All"]} vertical />
    </div>
  ) : opto ? (
    <div className="dyn-opto-switch">
      <div className="dyn-knob-label">Mode</div>
      <ParamChoice props={props} id={P.optoMode} vertical />
    </div>
  ) : (
    <ParamKnob props={props} id={P.ratio} size={74} />
  );

  // The opto face shows the threshold as the LA-2A's Peak Reduction: 0–100, clockwise for
  // more reduction.
  const threshold = opto ? (
    <DynKnob
      label="Peak Reduction"
      value={(-v(P.threshold) / 60) * 100}
      min={0}
      max={100}
      def={(-(thrSpec?.default ?? -20) / 60) * 100}
      format={(pr) => pr.toFixed(0)}
      onChange={(pr) => props.set(P.threshold, Math.round(-pr * 6) / 10)}
      onGrab={(on) => props.touch(P.threshold, on)}
      size={74}
    />
  ) : (
    <ParamKnob props={props} id={P.threshold} size={74} />
  );

  const attack =
    circuit === Circuit.Vca ? (
      <ParamKnob props={props} id={P.attack} steps={VCA_ATTACKS} range={[VCA_ATTACKS[0], VCA_ATTACKS[VCA_ATTACKS.length - 1]]} />
    ) : fet ? (
      <ParamKnob props={props} id={P.attack} range={FET_ATTACK} />
    ) : opto ? (
      <ParamKnob props={props} id={P.attack} disabled note="10 ms" title="The optical cell's attack is fixed (about 10 ms)" />
    ) : varimu ? (
      <ParamKnob props={props} id={P.attack} disabled note={formatMs(tc.attack)} title="Set by the Time Constant" />
    ) : (
      <ParamKnob props={props} id={P.attack} />
    );
  const autoRel = v(P.autoRelease) >= 0.5;
  const release =
    circuit === Circuit.Vca ? (
      autoRel ? (
        <ParamKnob props={props} id={P.release} disabled note="Auto" />
      ) : (
        <ParamKnob props={props} id={P.release} steps={VCA_RELEASES} range={[VCA_RELEASES[0], VCA_RELEASES[VCA_RELEASES.length - 1]]} />
      )
    ) : fet ? (
      <ParamKnob props={props} id={P.release} range={FET_RELEASE} title="Program-dependent: recovers more slowly after heavy compression" />
    ) : opto ? (
      <ParamKnob props={props} id={P.release} disabled note="T4 cell" title="Two-stage, program-dependent: ~60 ms, then 1–15 s" />
    ) : varimu ? (
      <ParamKnob props={props} id={P.release} disabled note={tc.release} title="Set by the Time Constant" />
    ) : (
      <ParamKnob props={props} id={P.release} />
    );

  return (
    <div className={`dyn-panel dyn-comp circuit-${meta.key}`}>
      <div className="dyn-circuits" role="tablist">
        {CIRCUITS.map((c, i) => (
          <button key={c.key} className={i === circuit ? "on" : ""} title={c.about} onClick={() => setNow(P.circuit, i)}>
            {c.short}
          </button>
        ))}
      </div>

      <div className="dyn-top" ref={top}>
        <LevelBar db={mIn} label="In" length={108} grow />
        <div className="dyn-meter-face">{circuit === Circuit.Platinum ? <GrBar gr={mGr} /> : <GrNeedle gr={mGr} />}</div>
        <LevelBar db={mOut} label="Out" length={108} grow />
        <TransferGraph v={v} level={mLevel} size={graph} />
      </div>

      <div className="dyn-main">
        <div className="dyn-big">
          {threshold}
          {ratioControl}
        </div>
        <div className="dyn-small">
          {attack}
          {release}
          {digital ? <ParamKnob props={props} id={P.knee} /> : <ParamKnob props={props} id={P.knee} disabled note={varimu ? "Wide" : opto ? "Soft" : "Fixed"} title="This circuit's knee is part of its character" />}
          <ParamKnob props={props} id={P.makeup} label={opto ? "Gain" : "Make Up"} />
          <ParamKnob props={props} id={P.mix} />
          <ParamKnob props={props} id={P.output} />
        </div>
      </div>

      <div className="dyn-sections">
        <Section title="Detection">
          {digital ? <ParamChoice props={props} id={P.detector} /> : <span className="dyn-note">Peak</span>}
          {digital && <ParamToggle props={props} id={P.autoRelease} label="Auto Release" />}
          {varimu && (
            <div className="dyn-field">
              <span className="dyn-field-label">Time Constant</span>
              <ParamChoice props={props} id={P.timeConstant} labels={["1", "2", "3", "4", "5", "6"]} />
            </div>
          )}
          <ParamKnob props={props} id={P.link} label="Link" size={30} />
          <ParamKnob props={props} id={P.lookahead} size={30} />
        </Section>
        <Section title="Side Chain">
          <ParamChoice props={props} id={P.scMode} />
          <ParamKnob props={props} id={P.scFreq} label="Freq" size={30} disabled={Math.round(v(P.scMode)) === 0} note="Off" />
          <ParamToggle props={props} id={P.listen} label="Listen" title="Hear what the detector hears" />
        </Section>
        <Section title="Output">
          <div className="dyn-field">
            <span className="dyn-field-label">Auto Gain</span>
            <ParamChoice props={props} id={P.autoGain} labels={["Off", "0", "−12", "−24"]} />
          </div>
          <div className="dyn-field">
            <span className="dyn-field-label">Distortion</span>
            <ParamChoice props={props} id={P.distortion} />
          </div>
        </Section>
      </div>
      <div className="dyn-footer">
        <span className="dyn-circuit-name">{meta.name}</span>
        <span className="dyn-circuit-about">{meta.about}</span>
        <span className="dyn-footer-gr" title="Gain reduction">
          {mGr > 0.05 ? `−${mGr.toFixed(1)} dB` : "0.0 dB"}
        </span>
      </div>
    </div>
  );
}

export default CompressorPanel;
