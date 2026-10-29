// The Loudness Meter's panel: momentary and short-term bars on an EBU scale around the target,
// big LUFS readouts, loudness range and true peak, and a scrolling short-term history. The
// history lives here, sampled from the ~30 Hz meter updates.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { FxPanelProps } from "../../fx";
import { onFrame } from "../../frame";
import { useStore } from "../../store";
import { pixelRatio } from "../../uiScale";
import { Check, Select } from "../Form";

// Parameter ids and meter indexes (crates/engine/src/fx/loudness.rs).
const TARGET = 0;
const CUSTOM_TARGET = 1;
const SCALE = 2;
const RESET = 3;
const AUTO_RESET = 4;
const M_MOMENTARY = 0;
const M_SHORT = 1;
const M_INTEGRATED = 2;
const M_RANGE = 3;
const M_TRUE_PEAK = 4;
const M_PEAK_L = 5;
const M_PEAK_R = 6;

/** The Target choices' values in LUFS, in the engine's order (the last choice is Custom). */
const TARGETS = [-23, -16, -14, -24];
/** Below this the engine is reporting silence or no measurement. */
const SILENT = -100;
/** Streaming services ask for true peaks under this. */
const TRUE_PEAK_LIMIT = -1;
/** The history graph's length in seconds, at one point per 100 ms. */
const HISTORY_SECS = 60;
const HISTORY_POINTS = HISTORY_SECS * 10;

/** EBU Tech 3341 scales, in LU around the target, with the label step. */
const SCALES = [
  { lo: -18, hi: 9, step: 3 },
  { lo: -36, hi: 18, step: 6 },
];

const minus = (s: string) => s.replace("-", "−");
const lufs = (v: number) => (v <= SILENT ? "—" : minus(v.toFixed(1)));
const signed = (v: number) => (Math.abs(v) < 0.05 ? "0.0" : (v > 0 ? "+" : "−") + Math.abs(v).toFixed(1));

/** A loudness bar: zone colours painted along the whole bar and uncovered up to the level. */
function LoudnessBar({ label, value, target, scale }: { label: string; value: number; target: number; scale: (typeof SCALES)[number] }) {
  const pos = (lu: number) => Math.min(1, Math.max(0, (lu - scale.lo) / (scale.hi - scale.lo)));
  const level = value <= SILENT ? 0 : pos(value - target);
  // Blue below the target, green within 1 LU of it, orange above.
  const lowEdge = pos(-1) * 100;
  const highEdge = pos(1) * 100;
  const zones =
    `linear-gradient(to top, var(--accent) 0 ${lowEdge}%, var(--green) ${lowEdge}% ${highEdge}%, ` +
    `var(--mon) ${highEdge}% 100%)`;
  return (
    <div className="loud-bar-col">
      <div className="loud-bar" style={{ background: zones }}>
        <div className="loud-bar-cover" style={{ height: `${(1 - level) * 100}%` }} />
        <div className="loud-bar-target" style={{ bottom: `${pos(0) * 100}%` }} />
      </div>
      <span className="loud-bar-label">{label}</span>
    </div>
  );
}

function Scale({ scale }: { scale: (typeof SCALES)[number] }) {
  const marks: number[] = [];
  for (let lu = scale.hi; lu >= scale.lo; lu -= scale.step) marks.push(lu);
  return (
    <div className="loud-scale">
      {marks.map((lu) => (
        <span key={lu} className={lu === 0 ? "zero" : ""} style={{ bottom: `${((lu - scale.lo) / (scale.hi - scale.lo)) * 100}%` }}>
          {lu > 0 ? `+${lu}` : minus(String(lu))}
        </span>
      ))}
    </div>
  );
}

interface HistoryState {
  /** Short-term loudness per 100 ms, oldest first; NaN for silence. */
  points: number[];
  last: number;
}

/** The scrolling short-term graph, drawn on the frame loop whenever a point is added. */
function History({
  meters,
  history,
  target,
  scale,
}: {
  meters: React.RefObject<number[]>;
  history: React.RefObject<HistoryState>;
  target: number;
  scale: (typeof SCALES)[number];
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [w, setW] = useState(400);
  const themeId = useStore((s) => s.themeId);
  const uiScale = useStore((s) => s.uiScale);
  const H = 84;

  useLayoutEffect(() => {
    const el = ref.current!.parentElement!;
    const ro = new ResizeObserver(([e]) => setW(Math.max(160, Math.floor(e.contentRect.width))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    const cv = ref.current!;
    const dpr = pixelRatio();
    cv.width = Math.round(w * dpr);
    cv.height = Math.round(H * dpr);
    cv.style.width = `${w}px`;
    cv.style.height = `${H}px`;
    const ctx = cv.getContext("2d")!;
    const cs = getComputedStyle(cv);
    const css = (n: string, f: string) => cs.getPropertyValue(n).trim() || f;
    const accent = css("--accent", "#4c9eff");
    const green = css("--green", "#45c25b");
    const grid = css("--lcd-sep", "#2c2c2e");
    const label = css("--lcd-label", "#85858a");
    const y = (v: number) => {
      const t = (v - target - scale.lo) / (scale.hi - scale.lo);
      return H - 1 - Math.min(1, Math.max(0, t)) * (H - 2);
    };
    let drawnCount = -1;

    const draw = () => {
      const pts = history.current.points;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, H);
      ctx.fillStyle = grid;
      for (let lu = scale.lo + scale.step; lu < scale.hi; lu += scale.step) ctx.fillRect(0, Math.round(y(target + lu)), w, 1);
      // Ten-second marks.
      for (let s = 10; s < HISTORY_SECS; s += 10) ctx.fillRect(Math.round(w - (s / HISTORY_SECS) * w), 0, 1, H);

      const dx = w / HISTORY_POINTS;
      const x0 = w - pts.length * dx;
      // The filled area, broken where the signal was silent.
      ctx.fillStyle = accent;
      ctx.globalAlpha = 0.28;
      let run: number[] = [];
      const flush = () => {
        if (run.length < 2) {
          run = [];
          return;
        }
        ctx.beginPath();
        ctx.moveTo(x0 + run[0] * dx, H);
        for (const i of run) ctx.lineTo(x0 + i * dx, y(pts[i]));
        ctx.lineTo(x0 + run[run.length - 1] * dx, H);
        ctx.closePath();
        ctx.fill();
        run = [];
      };
      pts.forEach((v, i) => (Number.isNaN(v) ? flush() : run.push(i)));
      flush();
      ctx.globalAlpha = 1;
      ctx.strokeStyle = accent;
      ctx.lineWidth = 1.25;
      ctx.beginPath();
      let pen = false;
      pts.forEach((v, i) => {
        if (Number.isNaN(v)) {
          pen = false;
          return;
        }
        if (pen) ctx.lineTo(x0 + i * dx, y(v));
        else ctx.moveTo(x0 + i * dx, y(v));
        pen = true;
      });
      ctx.stroke();

      ctx.strokeStyle = green;
      ctx.setLineDash([4, 3]);
      ctx.beginPath();
      ctx.moveTo(0, Math.round(y(target)) + 0.5);
      ctx.lineTo(w, Math.round(y(target)) + 0.5);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.fillStyle = label;
      ctx.font = "600 9px system-ui, sans-serif";
      ctx.textBaseline = "top";
      ctx.fillText(`−${HISTORY_SECS} s`, 4, 3);
    };

    return onFrame((now) => {
      const h = history.current;
      if (now - h.last >= 100) {
        h.last = now;
        const st = meters.current[M_SHORT] ?? -120;
        h.points.push(st <= SILENT ? NaN : st);
        if (h.points.length > HISTORY_POINTS) h.points.shift();
      }
      // Redraw when a point was added or the history was cleared.
      const stamp = h.points.length === HISTORY_POINTS ? h.last : h.points.length;
      if (stamp !== drawnCount) {
        drawnCount = stamp;
        draw();
      }
    });
  }, [w, themeId, uiScale, target, scale, history, meters]);

  return <canvas ref={ref} className="loud-history-canvas" />;
}

export function LoudnessPanel({ info, values, set, touch, meters }: FxPanelProps) {
  const val = (id: number) => values[id] ?? info.params.find((p) => p.id === id)?.default ?? 0;
  const targetIdx = Math.round(val(TARGET));
  const target = TARGETS[targetIdx] ?? val(CUSTOM_TARGET);
  const scale = SCALES[Math.round(val(SCALE))] ?? SCALES[0];
  const autoReset = val(AUTO_RESET) >= 0.5;
  const targetSpec = info.params.find((p) => p.id === TARGET);
  const targetLabels = targetSpec?.kind.type === "choice" ? targetSpec.kind.labels : ["−23 LUFS", "−16 LUFS", "−14 LUFS", "−24 LUFS", "Custom"];

  const m = (i: number) => meters[i] ?? -120;
  const momentary = m(M_MOMENTARY);
  const short = m(M_SHORT);
  const integrated = m(M_INTEGRATED);
  const range = meters[M_RANGE] ?? 0;
  const truePeak = m(M_TRUE_PEAK);

  const meterRef = useRef<number[]>(meters);
  meterRef.current = meters;
  const history = useRef<HistoryState>({ points: [], last: 0 });
  // A new measurement (Reset, or play start with Auto-Reset) starts a new history too.
  const prevIntegrated = useRef(integrated);
  useEffect(() => {
    if (prevIntegrated.current > SILENT && integrated <= SILENT) history.current.points = [];
    prevIntegrated.current = integrated;
  }, [integrated]);

  const reset = () => {
    // A pulse: the engine resets on the rising edge.
    touch(RESET, true);
    set(RESET, 1);
    history.current.points = [];
    window.setTimeout(() => {
      set(RESET, 0);
      touch(RESET, false);
    }, 120);
  };

  const dragCustom = (e: React.MouseEvent) => {
    e.preventDefault();
    touch(CUSTOM_TARGET, true);
    let last = e.clientY;
    let v = val(CUSTOM_TARGET);
    const move = (ev: MouseEvent) => {
      v = Math.min(-6, Math.max(-36, v + (last - ev.clientY) * (ev.shiftKey ? 0.02 : 0.1)));
      last = ev.clientY;
      set(CUSTOM_TARGET, Math.round(v * 10) / 10);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      touch(CUSTOM_TARGET, false);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const offTarget = integrated > SILENT ? integrated - target : null;

  return (
    <div className="loud">
      <div className="loud-main">
        <div className="loud-meters">
          <Scale scale={scale} />
          <LoudnessBar label="M" value={momentary} target={target} scale={scale} />
          <LoudnessBar label="S" value={short} target={target} scale={scale} />
        </div>
        <div className="loud-readouts">
          <div className="loud-read">
            <span className="loud-read-label">Momentary</span>
            <span className="loud-read-value">{lufs(momentary)}</span>
            <span className="loud-read-unit">LUFS</span>
          </div>
          <div className="loud-read">
            <span className="loud-read-label">Short-Term</span>
            <span className="loud-read-value">{lufs(short)}</span>
            <span className="loud-read-unit">LUFS</span>
          </div>
          <div className="loud-read integrated">
            <span className="loud-read-label">Integrated</span>
            <span className="loud-read-value">{lufs(integrated)}</span>
            <span className="loud-read-unit">LUFS</span>
            <span className={`loud-delta${offTarget !== null && Math.abs(offTarget) <= 1 ? " on" : ""}`}>
              {offTarget === null ? `Target ${minus(target.toFixed(1))}` : `${signed(offTarget)} LU from target`}
            </span>
          </div>
          <div className="loud-small">
            <div className="loud-read small">
              <span className="loud-read-label">Range</span>
              <span className="loud-read-value">{integrated > SILENT || range > 0 ? range.toFixed(1) : "—"}</span>
              <span className="loud-read-unit">LU</span>
            </div>
            <div className={`loud-read small${truePeak > TRUE_PEAK_LIMIT ? " over" : ""}`} title={`Above ${minus(TRUE_PEAK_LIMIT.toFixed(1))} dBTP shows red`}>
              <span className="loud-read-label">True Peak</span>
              <span className="loud-read-value">{lufs(truePeak)}</span>
              <span className="loud-read-unit">dBTP</span>
            </div>
            <div className="loud-read small">
              <span className="loud-read-label">Peak L / R</span>
              <span className="loud-read-value">
                {lufs(m(M_PEAK_L))} <i>/</i> {lufs(m(M_PEAK_R))}
              </span>
            </div>
          </div>
        </div>
      </div>

      <div className="loud-history">
        <History meters={meterRef} history={history} target={target} scale={scale} />
      </div>

      <div className="loud-controls">
        <label>Target</label>
        <Select
          value={targetIdx}
          options={targetLabels.map((l, i) => [i, l] as [number, string])}
          onChange={(i) => set(TARGET, i)}
          width={200}
        />
        {targetIdx >= TARGETS.length && (
          <div
            className="scrub loud-custom"
            onMouseDown={dragCustom}
            onDoubleClick={() => set(CUSTOM_TARGET, -23)}
            title="Custom target — drag (Shift for fine), double-click for −23 LUFS"
          >
            {minus(val(CUSTOM_TARGET).toFixed(1))} LUFS
          </div>
        )}
        <div className="segmented">
          {["EBU +9", "EBU +18"].map((l, i) => (
            <button key={l} className={scale === SCALES[i] ? "on" : ""} onClick={() => set(SCALE, i)}>
              {l}
            </button>
          ))}
        </div>
        <span className="loud-spacer" />
        <Check checked={autoReset} onChange={(on) => set(AUTO_RESET, on ? 1 : 0)} title="Start a new measurement each time playback starts">
          Auto-Reset
        </Check>
        <button className="loud-reset" onClick={reset} title="Start a new integrated, range and true-peak measurement">
          Reset
        </button>
      </div>
    </div>
  );
}
