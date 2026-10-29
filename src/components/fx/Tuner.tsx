// The Tuner's panel: a hardware-style tuner face with the note name, a cents needle (or a
// strobe) and the frequency, over a row of settings. The engine sends the pitch about 30 times
// a second; the needle and strobe animate between updates on the shared frame loop.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { FxPanelProps } from "../../fx";
import { onFrame } from "../../frame";
import { useStore } from "../../store";
import { pixelRatio } from "../../uiScale";

// Parameter ids and meter indexes (crates/engine/src/fx/tuner.rs).
const REFERENCE = 0;
const MUTE = 1;
const DISPLAY = 2;
const NOTATION = 3;
const TRANSPOSE = 4;
const THRESHOLD = 5;
const M_FREQ = 0;
const M_NOTE = 1;
const M_CENTS = 2;
const M_LEVEL = 4;

/** Within this many cents the needle turns green. */
const IN_TUNE = 2;

const SHARPS = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
const FLATS = ["C", "D♭", "D", "E♭", "E", "F", "G♭", "G", "A♭", "A", "B♭", "B"];

/** A MIDI note's name in Logic's numbering, where middle C (60) is C3. */
export function tunerNoteName(note: number, flats: boolean): { letter: string; accidental: string; octave: number } {
  const n = Math.round(note);
  const name = (flats ? FLATS : SHARPS)[((n % 12) + 12) % 12];
  return { letter: name[0], accidental: name.slice(1), octave: Math.floor(n / 12) - 2 };
}

const minus = (s: string) => s.replace("-", "−");

function signed(v: number, digits: number): string {
  const s = Math.abs(v).toFixed(digits);
  return Number(s) === 0 ? s : (v > 0 ? "+" : "−") + s;
}

/** A value dragged vertically (Shift for fine), double-click for its default. */
function DragValue({
  value,
  min,
  max,
  perPixel,
  resolution,
  def,
  format,
  onChange,
  onTouch,
  title,
  className = "",
}: {
  value: number;
  min: number;
  max: number;
  perPixel: number;
  resolution: number;
  def: number;
  format: (v: number) => string;
  onChange: (v: number) => void;
  onTouch: (on: boolean) => void;
  title: string;
  className?: string;
}) {
  const clamp = (v: number) => Math.min(max, Math.max(min, v));
  const snap = (v: number) => Math.round(Math.round(v / resolution) * resolution * 1e6) / 1e6;
  return (
    <div
      className={`scrub ${className}`}
      title={title}
      onMouseDown={(e) => {
        e.preventDefault();
        onTouch(true);
        let last = e.clientY;
        let v = value;
        const move = (ev: MouseEvent) => {
          v = clamp(v + (last - ev.clientY) * perPixel * (ev.shiftKey ? 0.1 : 1));
          last = ev.clientY;
          onChange(snap(v));
        };
        const up = () => {
          window.removeEventListener("mousemove", move);
          window.removeEventListener("mouseup", up);
          onTouch(false);
        };
        window.addEventListener("mousemove", move);
        window.addEventListener("mouseup", up);
      }}
      onDoubleClick={() => onChange(def)}
    >
      {format(value)}
    </div>
  );
}

interface Colors {
  text: string;
  dim: string;
  faint: string;
  green: string;
  red: string;
  face: string;
}

function readColors(el: HTMLElement): Colors {
  const cs = getComputedStyle(el);
  const v = (name: string, fallback: string) => cs.getPropertyValue(name).trim() || fallback;
  return {
    text: v("--lcd-text", "#ececef"),
    dim: v("--lcd-label", "#85858a"),
    faint: v("--lcd-sep", "#2c2c2e"),
    green: v("--green", "#45c25b"),
    red: v("--red", "#ec3d33"),
    face: v("--lcd-bg", "#161617"),
  };
}

/** What the canvas animates towards; updated from the meters on every render. */
interface Target {
  active: boolean;
  cents: number;
  strobe: boolean;
}

/** The dial's drawing height; a taller box scales the whole drawing up. */
const DIAL_H = 112;
/** The width the dial is drawn for at its smallest scale. */
const DIAL_W = 360;

/**
 * The needle (or strobe) display, drawn on the frame loop. It fills its box: wider is drawn
 * wider at the same scale, and once the box is taller than the drawing the whole dial scales up
 * (as far as the width allows), centred.
 */
function TunerDial({ target }: { target: React.RefObject<Target> }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [box, setBox] = useState({ w: DIAL_W, h: DIAL_H });
  const themeId = useStore((s) => s.themeId);
  const uiScale = useStore((s) => s.uiScale);

  useLayoutEffect(() => {
    const el = ref.current!.parentElement!;
    const ro = new ResizeObserver(([e]) => setBox({ w: Math.max(200, Math.floor(e.contentRect.width)), h: Math.max(DIAL_H, Math.floor(e.contentRect.height)) }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    const cv = ref.current!;
    const dpr = pixelRatio();
    cv.width = Math.round(box.w * dpr);
    cv.height = Math.round(box.h * dpr);
    cv.style.width = `${box.w}px`;
    cv.style.height = `${box.h}px`;
    // Everything below draws in a DIAL_H-tall space `w` wide.
    const k = Math.max(1, Math.min(box.h / DIAL_H, box.w / DIAL_W));
    const w = box.w / k;
    const H = DIAL_H;
    const offY = (box.h - H * k) / 2;
    const ctx = cv.getContext("2d")!;
    const col = readColors(cv);
    let needle = 0;
    let glow = 0;
    let phase = 0;
    let last = performance.now();
    let drawn = "";

    const drawNeedle = (t: Target) => {
      // The scale is an arc around a pivot below the canvas; ±50 cents spans ±40°.
      const cx = w / 2;
      const cy = H + 78;
      const r = cy - 24;
      const span = (40 * Math.PI) / 180;
      const ang = (c: number) => (c / 50) * span - Math.PI / 2;
      const at = (c: number, rad: number) => [cx + Math.cos(ang(c)) * rad, cy + Math.sin(ang(c)) * rad];

      // The in-tune zone.
      ctx.beginPath();
      ctx.arc(cx, cy, r - 4, ang(-IN_TUNE), ang(IN_TUNE));
      ctx.lineWidth = 8;
      ctx.strokeStyle = col.green;
      ctx.globalAlpha = 0.35 + 0.65 * glow;
      ctx.stroke();
      ctx.globalAlpha = 1;

      // Ticks every 5 cents, long ones every 25 with labels.
      ctx.lineWidth = 1;
      ctx.font = "600 9.5px system-ui, sans-serif";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      for (let c = -50; c <= 50; c += 5) {
        const major = c % 25 === 0;
        const [x0, y0] = at(c, r);
        const [x1, y1] = at(c, r + (major ? 11 : 6));
        ctx.strokeStyle = major ? col.text : col.dim;
        ctx.beginPath();
        ctx.moveTo(x0, y0);
        ctx.lineTo(x1, y1);
        ctx.stroke();
        if (major) {
          const [lx, ly] = at(c, r - 14);
          ctx.fillStyle = c === 0 ? col.text : col.dim;
          ctx.fillText(c === 0 ? "0" : signed(c, 0), lx, ly);
        }
      }
      ctx.beginPath();
      ctx.arc(cx, cy, r, ang(-50), ang(50));
      ctx.strokeStyle = col.dim;
      ctx.stroke();

      // The needle, from below the labels to past the ticks.
      const c = Math.max(-52, Math.min(52, needle));
      const [nx0, ny0] = at(c, r - 64);
      const [nx1, ny1] = at(c, r + 14);
      ctx.lineCap = "round";
      ctx.lineWidth = 2;
      ctx.strokeStyle = !t.active ? col.dim : Math.abs(t.cents) <= IN_TUNE ? col.green : col.red;
      ctx.globalAlpha = t.active ? 1 : 0.45;
      ctx.beginPath();
      ctx.moveTo(nx0, ny0);
      ctx.lineTo(nx1, ny1);
      ctx.stroke();
      ctx.globalAlpha = 1;
      ctx.lineCap = "butt";
    };

    const drawStrobe = (t: Target) => {
      // Three bands, each twice the pattern frequency of the one above, drift sideways at a
      // speed set by the cents, like a mechanical strobe tuner; they stand still in tune.
      const top = 14;
      const bandH = 22;
      const gap = 6;
      const x0 = 18;
      const x1 = w - 18;
      const inTune = t.active && Math.abs(t.cents) <= IN_TUNE;
      for (let b = 0; b < 3; b++) {
        const y = top + b * (bandH + gap);
        const period = 48 / 2 ** b;
        const off = (((phase * 2 ** b) % period) + period) % period;
        ctx.save();
        ctx.beginPath();
        ctx.rect(x0, y, x1 - x0, bandH);
        ctx.clip();
        ctx.fillStyle = col.faint;
        ctx.fillRect(x0, y, x1 - x0, bandH);
        ctx.fillStyle = inTune ? col.green : col.text;
        ctx.globalAlpha = t.active ? 0.9 : 0.18;
        for (let x = x0 - period + off; x < x1; x += period) ctx.fillRect(x, y, period / 2, bandH);
        ctx.restore();
      }
      // The centre mark, lit in tune.
      ctx.fillStyle = col.green;
      ctx.globalAlpha = 0.3 + 0.7 * glow;
      ctx.beginPath();
      ctx.moveTo(w / 2 - 6, top - 9);
      ctx.lineTo(w / 2 + 6, top - 9);
      ctx.lineTo(w / 2, top - 2);
      ctx.fill();
      ctx.globalAlpha = 1;
      ctx.font = "600 9.5px system-ui, sans-serif";
      ctx.textBaseline = "middle";
      ctx.fillStyle = col.dim;
      ctx.textAlign = "left";
      ctx.fillText("♭", x0, H - 12);
      ctx.textAlign = "right";
      ctx.fillText("♯", x1, H - 12);
    };

    return onFrame((now) => {
      const t = target.current;
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      // A damped needle: it follows the 30 Hz meter steps smoothly.
      const goal = t.active ? t.cents : 0;
      needle += (goal - needle) * (1 - Math.exp(-dt / 0.06));
      const lit = t.active && Math.abs(t.cents) <= IN_TUNE ? 1 : 0;
      glow += (lit - glow) * (1 - Math.exp(-dt / 0.08));
      if (t.active) phase += t.cents * 4 * dt;
      const key = `${t.strobe}|${t.active}|${needle.toFixed(2)}|${glow.toFixed(2)}|${t.strobe && t.active ? phase.toFixed(1) : ""}`;
      if (key === drawn) return;
      drawn = key;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, cv.width, cv.height);
      ctx.setTransform(dpr * k, 0, 0, dpr * k, 0, dpr * offY);
      if (t.strobe) drawStrobe(t);
      else drawNeedle(t);
    });
  }, [box, themeId, uiScale, target]);

  return (
    <div className="tuner-dial-box">
      <canvas ref={ref} className="tuner-dial" />
    </div>
  );
}

export function TunerPanel({ info, values, set, touch, meters }: FxPanelProps) {
  const val = (id: number) => values[id] ?? info.params.find((p) => p.id === id)?.default ?? 0;
  const reference = val(REFERENCE);
  const muted = val(MUTE) >= 0.5;
  const strobe = Math.round(val(DISPLAY)) === 1;
  const flats = Math.round(val(NOTATION)) === 1;
  const transpose = Math.round(val(TRANSPOSE));
  const threshold = val(THRESHOLD);

  const freq = meters[M_FREQ] ?? 0;
  const active = freq > 0 && (meters[M_NOTE] ?? -1) >= 0;
  const cents = active ? (meters[M_CENTS] ?? 0) : 0;
  const level = meters[M_LEVEL] ?? -120;
  const name = active ? tunerNoteName(meters[M_NOTE], flats) : null;
  const inTune = active && Math.abs(cents) <= IN_TUNE;

  const target = useRef<Target>({ active, cents, strobe });
  target.current = { active, cents, strobe };

  const levelPct = (db: number) => Math.min(100, Math.max(0, ((db + 90) / 90) * 100));

  return (
    <div className="tuner">
      <div className={`tuner-face${muted ? " muted" : ""}`}>
        <div className="tuner-head">
          <span className={`tuner-led flat${active && cents < -IN_TUNE ? " lit" : ""}`} title="Flat" />
          <div className={`tuner-note${active ? "" : " idle"}${inTune ? " in-tune" : ""}`}>
            {name ? (
              <>
                <span className="tuner-letter">{name.letter}</span>
                <span className="tuner-marks">
                  <span className="tuner-acc">{name.accidental}</span>
                  <span className="tuner-oct">{minus(String(name.octave))}</span>
                </span>
              </>
            ) : (
              <span className="tuner-letter">—</span>
            )}
          </div>
          <span className={`tuner-led sharp${active && cents > IN_TUNE ? " lit" : ""}`} title="Sharp" />
        </div>
        <TunerDial target={target} />
        <div className="tuner-foot">
          <span className="tuner-readout">
            {active ? signed(cents, 1) : "—"}
            <small>cents</small>
          </span>
          <span className="tuner-ref-badge">
            A4 = {reference.toFixed(1)} Hz{transpose !== 0 ? ` · Transpose ${signed(transpose, 0)}` : ""}
          </span>
          <span className="tuner-readout right">
            {active ? freq.toFixed(freq < 100 ? 2 : 1) : "—"}
            <small>Hz</small>
          </span>
        </div>
        <div className="tuner-level" title={`Input ${level <= -120 ? "−∞" : minus(level.toFixed(0))} dB · gate ${minus(threshold.toFixed(0))} dB`}>
          <div className={`tuner-level-fill${level >= threshold ? " open" : ""}`} style={{ width: `${levelPct(level)}%` }} />
          <div className="tuner-level-gate" style={{ left: `${levelPct(threshold)}%` }} />
        </div>
        {muted && <div className="tuner-muted">Output muted</div>}
      </div>

      <div className="tuner-controls">
        <div className="tuner-field">
          <label>Reference</label>
          <div className="tuner-stepper">
            <button className="tuner-step" onClick={() => set(REFERENCE, Math.max(400, Math.round(reference - 1)))} title="Down 1 Hz">
              −
            </button>
            <DragValue
              className="tuner-ref"
              value={reference}
              min={400}
              max={480}
              perPixel={0.1}
              resolution={0.1}
              def={440}
              format={(v) => `${v.toFixed(1)} Hz`}
              onChange={(v) => set(REFERENCE, v)}
              onTouch={(on) => touch(REFERENCE, on)}
              title="Reference pitch for A4 — drag (Shift for fine), double-click for 440 Hz"
            />
            <button className="tuner-step" onClick={() => set(REFERENCE, Math.min(480, Math.round(reference + 1)))} title="Up 1 Hz">
              +
            </button>
          </div>
        </div>
        <div className="tuner-field">
          <label>Display</label>
          <div className="segmented">
            <button className={strobe ? "" : "on"} onClick={() => set(DISPLAY, 0)}>
              Needle
            </button>
            <button className={strobe ? "on" : ""} onClick={() => set(DISPLAY, 1)}>
              Strobe
            </button>
          </div>
        </div>
        <div className="tuner-field">
          <label>Notation</label>
          <div className="segmented tuner-notation">
            <button className={flats ? "" : "on"} onClick={() => set(NOTATION, 0)} title="Sharps">
              ♯
            </button>
            <button className={flats ? "on" : ""} onClick={() => set(NOTATION, 1)} title="Flats">
              ♭
            </button>
          </div>
        </div>
        <div className="tuner-field">
          <label>Transpose</label>
          <DragValue
            className="tuner-small"
            value={transpose}
            min={-24}
            max={24}
            perPixel={0.15}
            resolution={1}
            def={0}
            format={(v) => (v === 0 ? "0" : signed(v, 0))}
            onChange={(v) => set(TRANSPOSE, v)}
            onTouch={(on) => touch(TRANSPOSE, on)}
            title="Shows the written note for a transposing instrument, in semitones (B♭ instruments +2, E♭ alto +9, F horn +7)"
          />
        </div>
        <div className="tuner-field">
          <label>Gate</label>
          <DragValue
            className="tuner-small"
            value={threshold}
            min={-90}
            max={-20}
            perPixel={0.5}
            resolution={1}
            def={-60}
            format={(v) => `${minus(v.toFixed(0))} dB`}
            onChange={(v) => set(THRESHOLD, v)}
            onTouch={(on) => touch(THRESHOLD, on)}
            title="Input below this level reads as no note"
          />
        </div>
        <button className={`tuner-mute${muted ? " on" : ""}`} onClick={() => set(MUTE, muted ? 0 : 1)} title="Silence the output while tuning">
          Mute
        </button>
      </div>
    </div>
  );
}
