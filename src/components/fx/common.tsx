// Shared pieces for built-in effect panels: the big knob, toggle and choice buttons, meter bars,
// the side-chain pop-up and value formatting. Custom panels (FX_PANELS in src/fx.ts) build from
// these so every effect looks and handles the same.
import { useRef, useState } from "react";
import { act, type Id } from "../../api";
import type { FxParamSpec } from "../../fx";
import { clampValue, formatValue, fromNorm, parseValue, tidy, toNorm } from "../../fxFormat";
import { findFxSlot } from "../../fxHost";
import { useStore } from "../../store";
import { openMenuAt, type MenuItem } from "../Menu";
import { useFxLearn } from "../../learn";

export { formatValue, toNorm, fromNorm, tidy, parseValue, clampValue };

/** The knob's sweep: 135° each side of straight up, as Logic's plug-in knobs. */
const SWEEP = 135;

/**
 * A plug-in knob with its name above and value below. Drag up and down (Shift for fine),
 * double-click or Option-click for the default, double-click the value to type one.
 */
export function FxKnob({
  spec,
  value,
  onChange,
  onTouch,
  size = 46,
  label,
  format = formatValue,
}: {
  spec: FxParamSpec;
  value: number;
  onChange: (v: number) => void;
  onTouch?: (on: boolean) => void;
  size?: number;
  /** Overrides the parameter's name. */
  label?: string;
  /** Overrides the readout (a pan as "L 35", say); typing a value still parses plain units. */
  format?: (spec: FxParamSpec, v: number) => string;
}) {
  const pos = useRef(toNorm(spec, value));
  const [editing, setEditing] = useState(false);
  const learn = useFxLearn(spec.id);
  const n = toNorm(spec, value);
  // Bipolar ranges (a gain from -24 to +24 dB) fill from the centre.
  const origin = spec.min < 0 && spec.max > 0 ? toNorm(spec, 0) : 0;
  const angle = (u: number) => -SWEEP + u * 2 * SWEEP;
  const r = size / 2 - 3;
  const c = size / 2;
  const at = (a: number, rad = r) => [c + rad * Math.sin((a * Math.PI) / 180), c - rad * Math.cos((a * Math.PI) / 180)];
  const arc = (from: number, to: number, rad = r) => {
    const [x0, y0] = at(from, rad);
    const [x1, y1] = at(to, rad);
    const large = Math.abs(to - from) > 180 ? 1 : 0;
    return `M${x0} ${y0} A${rad} ${rad} 0 ${large} ${to > from ? 1 : 0} ${x1} ${y1}`;
  };
  const down = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    if (e.altKey) {
      onChange(spec.default);
      return;
    }
    pos.current = n;
    let last = e.clientY;
    onTouch?.(true);
    const move = (ev: MouseEvent) => {
      const d = last - ev.clientY;
      last = ev.clientY;
      pos.current = Math.min(1, Math.max(0, pos.current + d / (ev.shiftKey ? 900 : 150)));
      onChange(tidy(spec, fromNorm(spec, pos.current)));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      onTouch?.(false);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  const a0 = angle(origin);
  const a1 = angle(n);
  return (
    <div className="fx-knob" style={{ minWidth: Math.max(size + 16, 64) }}>
      <div className="fx-label">{label ?? spec.name}</div>
      <div className="knob" style={{ width: size, height: size }} onMouseDown={down} onDoubleClick={() => onChange(spec.default)} onContextMenu={learn} title={`${spec.name}: ${format(spec, value)}`}>
        <svg width={size} height={size}>
          <path d={arc(-SWEEP, SWEEP)} className="knob-ring" />
          {Math.abs(a1 - a0) > 0.5 && <path d={arc(Math.min(a0, a1), Math.max(a0, a1))} className="knob-arc" />}
          <circle cx={c} cy={c} r={r - 4.5} className="knob-body" />
          <line x1={at(a1, (r - 4.5) * 0.35)[0]} y1={at(a1, (r - 4.5) * 0.35)[1]} x2={at(a1, r - 6)[0]} y2={at(a1, r - 6)[1]} className="knob-pointer" />
        </svg>
      </div>
      {editing ? (
        <input
          className="fx-value-edit"
          autoFocus
          defaultValue={formatValue(spec, value)}
          onFocus={(e) => e.currentTarget.select()}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") {
              const v = parseValue(spec, e.currentTarget.value);
              if (v !== null) onChange(v);
              setEditing(false);
            } else if (e.key === "Escape") setEditing(false);
          }}
          onBlur={() => setEditing(false)}
        />
      ) : (
        <div className="fx-value" onDoubleClick={() => setEditing(true)} title="Double-click to type a value">
          {format(spec, value)}
        </div>
      )}
    </div>
  );
}

/** An on/off button for a toggle parameter. */
export function FxToggle({ spec, value, onChange, label }: { spec: FxParamSpec; value: number; onChange: (v: number) => void; label?: string }) {
  const on = value >= 0.5;
  const learn = useFxLearn(spec.id);
  return (
    <button className={"fx-toggle" + (on ? " on" : "")} onClick={() => onChange(on ? 0 : 1)} onContextMenu={learn} title={spec.name}>
      {label ?? spec.name}
    </button>
  );
}

/** Segmented buttons for a choice parameter (a pop-up when there are many choices). */
export function FxChoice({ spec, value, onChange, label }: { spec: FxParamSpec; value: number; onChange: (v: number) => void; label?: string }) {
  const labels = spec.kind.type === "choice" ? spec.kind.labels : [];
  const cur = clampValue(spec, value);
  const learn = useFxLearn(spec.id);
  if (labels.length > 4) {
    return (
      <div className="fx-choice" onContextMenu={learn}>
        <div className="fx-label">{label ?? spec.name}</div>
        <button className="fx-popup" onMouseDown={(e) => openMenuAt(e.currentTarget, labels.map((l, i) => ({ label: l, checked: i === cur, onClick: () => onChange(i) })))}>
          {labels[cur]}
        </button>
      </div>
    );
  }
  return (
    <div className="fx-choice" onContextMenu={learn}>
      <div className="fx-label">{label ?? spec.name}</div>
      <div className="fx-segments">
        {labels.map((l, i) => (
          <button key={l} className={i === cur ? "on" : ""} onClick={() => onChange(i)}>
            {l}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Whether a meter shows gain reduction (drawn down from the top) rather than a level. */
export const isReductionMeter = (name: string) => /reduction/i.test(name);

/**
 * A vertical meter with its name and value. A level fills up from `min`; a gain reduction
 * meter (`kind="reduction"`, values in positive dB) hangs down from the top.
 */
export function FxMeter({
  label,
  value,
  min,
  max,
  kind = "level",
  height = 120,
  marker,
}: {
  label: string;
  value: number;
  min?: number;
  max?: number;
  kind?: "level" | "reduction";
  /** The bar's height at the panel's smallest; it grows when the panel is made taller. */
  height?: number;
  /** A line across the bar at this value, such as the threshold. */
  marker?: number;
}) {
  // Levels default to -60..0 dBFS, reduction to 0..30 dB.
  const lo = min ?? (kind === "reduction" ? 0 : -60);
  const hi = max ?? (kind === "reduction" ? 30 : 0);
  const v = Number.isFinite(value) ? value : lo;
  const f = Math.min(1, Math.max(0, (v - lo) / (hi - lo || 1)));
  const text = kind === "reduction" ? (v > 0.05 ? `−${v.toFixed(1)}` : "0.0") : v <= lo ? "−∞" : v.toFixed(1).replace("-", "−");
  return (
    <div className="fx-meter">
      <div className="fx-label">{label}</div>
      <div className={"fx-meter-bar " + kind} style={{ minHeight: height }}>
        <div className="fx-meter-fill" style={kind === "reduction" ? { top: 0, height: `${f * 100}%` } : { bottom: 0, height: `${f * 100}%` }} />
        {marker !== undefined && <div className="fx-meter-mark" style={{ bottom: `${Math.min(1, Math.max(0, (marker - lo) / (hi - lo || 1))) * 100}%` }} />}
      </div>
      <div className="fx-value">{text}</div>
    </div>
  );
}

/**
 * The Side Chain pop-up: None, then every track with audio of its own (the effect's own track
 * left out). Picks the key source with `set_sidechain` (undoable; loops are refused). With a
 * source, Pre / Post picks where the key leaves it: before its fader or (the default) after.
 */
export function FxSideChain({ uid }: { uid: Id }) {
  const project = useStore((s) => s.snap?.project);
  const at = findFxSlot(project, uid);
  if (!project || !at) return null;
  const source = at.plugin.sidechain ?? null;
  const set = (src: Id | null) => act("set_sidechain", { track: at.track, slot: at.slot, source: src });
  const pre = !!at.plugin.sidechain_pre;
  const setPre = (on: boolean) => on !== pre && act("set_sidechain_pre", { track: at.track, slot: at.slot, pre: on });
  const name = source === null ? "None" : project.tracks.find((t) => t.id === source)?.name ?? "None";
  const open = (e: React.MouseEvent<HTMLButtonElement>) => {
    const items: MenuItem[] = [{ label: "None", checked: source === null, onClick: () => set(null) }, { separator: true }];
    for (const t of project.tracks) {
      if (t.id === at.track || t.kind === "folder") continue;
      items.push({ label: t.name, checked: t.id === source, onClick: () => set(t.id) });
    }
    openMenuAt(e.currentTarget, items);
  };
  return (
    <span className="fx-sidechain">
      <span className="dim">Side Chain</span>
      <button className="fx-popup" onMouseDown={open} title="The track whose output drives the detector">
        {name}
      </button>
      {source !== null && (
        <span className="segmented fx-key-tap">
          <button className={pre ? "on" : ""} onMouseDown={() => setPre(true)} title="Key from the source before its fader (its mute still silences it)">
            Pre
          </button>
          <button className={pre ? "" : "on"} onMouseDown={() => setPre(false)} title="Key from the source after its fader">
            Post
          </button>
        </span>
      )}
    </span>
  );
}

/** A titled group of controls inside a panel. */
export function FxGroup({ title, children, className }: { title?: string; children: React.ReactNode; className?: string }) {
  return (
    <div className={"fx-group" + (className ? " " + className : "")}>
      {title && <div className="fx-group-title">{title}</div>}
      <div className="fx-group-body">{children}</div>
    </div>
  );
}

/** The control a parameter gets in the generic panel: a knob, a toggle or a choice. */
export function FxControl({ spec, value, onChange, onTouch }: { spec: FxParamSpec; value: number; onChange: (v: number) => void; onTouch?: (on: boolean) => void }) {
  if (spec.kind.type === "toggle") return <FxToggle spec={spec} value={value} onChange={onChange} />;
  if (spec.kind.type === "choice") return <FxChoice spec={spec} value={value} onChange={onChange} />;
  return <FxKnob spec={spec} value={value} onChange={onChange} onTouch={onTouch} />;
}
