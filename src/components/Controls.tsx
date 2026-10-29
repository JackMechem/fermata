import { useEffect, useRef } from "react";
import type { Id } from "../api";
import { meterLevels, tickSeq, useStore } from "../store";
import { onFrame } from "../frame";
import { pixelRatio } from "../uiScale";
import { theme } from "../theme";
import { dbToFader, faderToDb, gainToDb } from "../time";

/** Shared vertical/horizontal drag helper. Shift gives fine control, like Logic. */
function useDrag(onDelta: (d: number, fine: boolean) => void, onEnd?: () => void) {
  return (e: React.MouseEvent, axis: "x" | "y") => {
    e.preventDefault();
    let last = axis === "x" ? e.clientX : e.clientY;
    const move = (ev: MouseEvent) => {
      const now = axis === "x" ? ev.clientX : ev.clientY;
      const d = axis === "x" ? now - last : last - now;
      last = now;
      onDelta(d, ev.shiftKey);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      onEnd?.();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
}

/** Grabbing and letting go of a control, which automation writing in Touch and Latch follows. */
export interface TouchProps {
  onTouchStart?: () => void;
  onTouchEnd?: () => void;
}

export function Fader({ db, onChange, height = 180, onTouchStart, onTouchEnd }: { db: number; onChange: (db: number) => void; height?: number } & TouchProps) {
  const pos = useRef(dbToFader(db));
  pos.current = dbToFader(db);
  const travel = height - 18;
  const start = useDrag((d, fine) => {
    pos.current = Math.min(1, Math.max(0, pos.current + d / travel / (fine ? 6 : 1)));
    onChange(Math.round(faderToDb(pos.current) * 10) / 10);
  }, onTouchEnd);
  const y = (1 - dbToFader(db)) * travel;
  return (
    <div
      className="fader"
      style={{ height }}
      onMouseDown={(e) => {
        onTouchStart?.();
        start(e, "y");
      }}
      onDoubleClick={() => onChange(0)}
      title="Volume — double-click for 0 dB"
    >
      <div className="fader-track" />
      <div className="fader-unity" style={{ top: (1 - dbToFader(0)) * travel + 9 }} />
      <div className="fader-cap" style={{ top: y }} />
    </div>
  );
}

export function Knob({ value, onChange, size = 26, onTouchStart, onTouchEnd }: { value: number; onChange: (v: number) => void; size?: number } & TouchProps) {
  const v = useRef(value);
  v.current = value;
  const start = useDrag((d, fine) => {
    v.current = Math.min(1, Math.max(-1, v.current + d / (fine ? 600 : 120)));
    onChange(Math.round(v.current * 64) / 64);
  }, onTouchEnd);
  const angle = value * 135;
  const r = size / 2 - 3;
  const arc = (a: number) => {
    const rad = ((a - 90) * Math.PI) / 180;
    return [size / 2 + r * Math.cos(rad), size / 2 + r * Math.sin(rad)];
  };
  const [x0, y0] = arc(0);
  const [x1, y1] = arc(angle);
  return (
    <div
      className="knob"
      style={{ width: size, height: size }}
      onMouseDown={(e) => {
        onTouchStart?.();
        start(e, "y");
      }}
      onDoubleClick={() => onChange(0)}
      title={`Pan ${Math.round(value * 64)}`}
    >
      <svg width={size} height={size}>
        <circle cx={size / 2} cy={size / 2} r={r} className="knob-ring" />
        {Math.abs(angle) > 0.5 && (
          <path d={`M${x0} ${y0} A${r} ${r} 0 0 ${angle > 0 ? 1 : 0} ${x1} ${y1}`} className="knob-arc" />
        )}
        <circle cx={size / 2} cy={size / 2} r={r - 3.5} className="knob-body" />
        <line
          x1={size / 2}
          y1={size / 2}
          x2={size / 2 + (r - 4) * Math.sin((angle * Math.PI) / 180)}
          y2={size / 2 - (r - 4) * Math.cos((angle * Math.PI) / 180)}
          className="knob-pointer"
        />
      </svg>
    </div>
  );
}

export function HSlider({ db, onChange, width = 90, onTouchStart, onTouchEnd }: { db: number; onChange: (db: number) => void; width?: number } & TouchProps) {
  const pos = useRef(dbToFader(db));
  pos.current = dbToFader(db);
  const start = useDrag((d, fine) => {
    pos.current = Math.min(1, Math.max(0, pos.current + d / (width - 10) / (fine ? 6 : 1)));
    onChange(Math.round(faderToDb(pos.current) * 10) / 10);
  }, onTouchEnd);
  const f = dbToFader(db);
  return (
    <div
      className="hslider"
      style={{ width }}
      onMouseDown={(e) => {
        e.stopPropagation();
        onTouchStart?.();
        start(e, "x");
      }}
      onDoubleClick={(e) => {
        e.stopPropagation();
        onChange(0);
      }}
    >
      <div className="hslider-fill" style={{ width: `${f * 100}%` }} />
      <div className="hslider-thumb" style={{ left: `calc(${f * 100}% - 5px)` }} />
    </div>
  );
}

export type MeterSource = { kind: "track"; id: Id } | { kind: "master" } | { kind: "input"; ch: number };

const METER_FLOOR = -60;

function level(db: number) {
  return Math.min(1, Math.max(0, (db - METER_FLOOR) / (6 - METER_FLOOR)));
}

/**
 * Peak meter drawn on a canvas straight from engine ticks. Falls back at ~12 dB/s like
 * Logic's default meter return, with a 2 s peak hold.
 */
export function Meter({
  source,
  height,
  width = 10,
  horizontal = false,
  onPeak,
  resetKey = 0,
}: {
  source: MeterSource;
  resetKey?: number;
  height: number;
  width?: number;
  horizontal?: boolean;
  onPeak?: (db: number) => void;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const src = JSON.stringify(source);
  const themeId = useStore((s) => s.themeId);
  const uiScale = useStore((s) => s.uiScale);
  useEffect(() => {
    const s: MeterSource = JSON.parse(src);
    const cv = ref.current!;
    const dpr = pixelRatio();
    const W = horizontal ? height : width;
    const H = horizontal ? width : height;
    cv.width = W * dpr;
    cv.height = H * dpr;
    const ctx = cv.getContext("2d")!;
    ctx.scale(dpr, dpr);
    const disp = [METER_FLOOR, METER_FLOOR];
    const hold = [METER_FLOOR, METER_FLOOR];
    const holdAt = [0, 0];
    let maxPeak = -Infinity;
    let last = performance.now();
    const len = horizontal ? W : H;
    const grad = horizontal ? ctx.createLinearGradient(0, 0, W, 0) : ctx.createLinearGradient(0, H, 0, 0);
    const stops = theme.meterStops;
    if (theme.meterStyle === "stepped") {
      // Three solid bands with hard edges.
      const mid = level(-12);
      const high = level(-3);
      grad.addColorStop(0, stops[0]);
      grad.addColorStop(mid, stops[0]);
      grad.addColorStop(mid, stops[1]);
      grad.addColorStop(high, stops[1]);
      grad.addColorStop(high, stops[2]);
      grad.addColorStop(1, stops[2]);
    } else {
      grad.addColorStop(0, stops[0]);
      grad.addColorStop(level(-18), stops[1]);
      grad.addColorStop(level(-6), stops[2]);
      grad.addColorStop(level(-1), stops[3]);
      grad.addColorStop(level(0), stops[4]);
      grad.addColorStop(1, stops[5]);
    }
    const key = s.kind === "master" ? -1 : s.kind === "input" ? -100 - s.ch : s.id;
    let seen = -1;
    // What was last painted, per lane: [level px, hold px]. Repaint only when it changes.
    const painted: number[] = [-1, -1, -1, -1];
    const lanes = 2;
    const gap = 1;
    const lw = ((horizontal ? H : W) - gap) / lanes;
    const paint = (levels: number[], holds: number[]) => {
      ctx.clearRect(0, 0, W, H);
      ctx.fillStyle = theme.meterBg;
      ctx.fillRect(0, 0, W, H);
      for (let i = 0; i < lanes; i++) {
        const l = levels[i];
        const hp = holds[i];
        ctx.fillStyle = grad;
        if (horizontal) ctx.fillRect(0, i * (lw + gap), l, lw);
        else ctx.fillRect(i * (lw + gap), H - l, lw, l);
        if (hp > 0) {
          ctx.fillStyle = hold[i] > 0 ? theme.red : theme.meterHold;
          if (horizontal) ctx.fillRect(hp - 1, i * (lw + gap), 1, lw);
          else ctx.fillRect(i * (lw + gap), H - hp, lw, 1);
        }
      }
    };
    paint([0, 0], [0, 0]);
    return onFrame((now) => {
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      // New engine data carries the peak since the previous report; between reports the
      // meter just falls back smoothly.
      let vals: [number, number] = [0, 0];
      const sq = tickSeq();
      if (sq !== seen) {
        seen = sq;
        vals = meterLevels(key);
        const peak = Math.max(gainToDb(vals[0]), gainToDb(vals[1]));
        if (onPeak && peak > maxPeak) {
          maxPeak = peak;
          onPeak(peak);
        }
      }
      const levels: number[] = [];
      const holds: number[] = [];
      let changed = false;
      for (let i = 0; i < lanes; i++) {
        const db = gainToDb(vals[i]);
        disp[i] = Math.max(db, disp[i] - 11.8 * dt);
        if (db > hold[i] || now - holdAt[i] > 2000) {
          hold[i] = Math.max(db, METER_FLOOR);
          holdAt[i] = now;
        }
        const l = Math.round(level(disp[i]) * len);
        const hp = hold[i] > METER_FLOOR ? Math.round(level(hold[i]) * len) : 0;
        levels.push(l);
        holds.push(hp);
        if (l !== painted[i * 2] || hp !== painted[i * 2 + 1]) changed = true;
        painted[i * 2] = l;
        painted[i * 2 + 1] = hp;
      }
      if (changed) paint(levels, holds);
    });
  }, [src, height, width, horizontal, onPeak, resetKey, themeId, uiScale]);
  const style = horizontal ? { width: height, height: width } : { width, height };
  return <canvas ref={ref} className="meter" style={style} />;
}
