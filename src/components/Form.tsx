// Themed form controls. Native <select> and number inputs render as GTK widgets on Linux,
// which clash with the rest of the interface, so these draw their own.
import { useEffect, useState } from "react";
import { Icon } from "../icons";
import { openMenuAt } from "./Menu";

/**
 * onKeyDown for a sheet: keys typed in it stay there, so the main window's shortcuts don't
 * fire. That also keeps them from the sheet's own window Esc listener, so Esc is handled here
 * too (an open menu takes Esc first, in the capture phase).
 */
export function sheetKey(e: React.KeyboardEvent, onClose: () => void) {
  e.stopPropagation();
  if (e.key === "Escape") onClose();
}

export function Select<T extends string | number>({
  value,
  options,
  onChange,
  width,
}: {
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
  width?: number | string;
}) {
  const current = options.find((o) => o[0] === value);
  return (
    <button
      type="button"
      className="select"
      style={{ width }}
      title={current?.[1]}
      onMouseDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        openMenuAt(
          e.currentTarget,
          options.map(([v, label]) => ({ label, checked: v === value, onClick: () => onChange(v) })),
        );
      }}
    >
      <span className="select-label">{current?.[1] ?? ""}</span>
      <span className="select-arrows">
        <Icon.chevron />
      </span>
    </button>
  );
}

export function NumberField({
  value,
  onChange,
  step = 1,
  min = -Infinity,
  max = Infinity,
  width = 76,
  resolution = step,
}: {
  value: number;
  onChange: (v: number) => void;
  step?: number;
  min?: number;
  max?: number;
  width?: number;
  /** Typed values are rounded to this; by default, to `step`. */
  resolution?: number;
}) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  const clamp = (v: number) => Math.min(max, Math.max(min, v));
  const commit = () => {
    const v = parseFloat(text);
    if (Number.isFinite(v)) {
      // Rounded to the resolution, then tidied of float noise (0.1 * 3 = 0.30000000000000004).
      const r = clamp(Math.round(Math.round(v / resolution) * resolution * 1e6) / 1e6);
      setText(String(r));
      onChange(r);
    } else setText(String(value));
  };
  const nudge = (d: number) => onChange(clamp(Math.round((value + d) * 1000) / 1000));
  return (
    <div className="numfield" style={{ width }}>
      <button type="button" className="numfield-step" tabIndex={-1} onClick={() => nudge(-step)} aria-label="Decrease">
        −
      </button>
      <input
        value={text}
        inputMode="decimal"
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") commit();
          if (e.key === "ArrowUp") nudge(step);
          if (e.key === "ArrowDown") nudge(-step);
        }}
      />
      <button type="button" className="numfield-step" tabIndex={-1} onClick={() => nudge(step)} aria-label="Increase">
        +
      </button>
    </div>
  );
}

/** A labelled checkbox in the sheets' style. */
export function Check({ checked, onChange, children, title }: { checked: boolean; onChange: (v: boolean) => void; children: React.ReactNode; title?: string }) {
  return (
    <label className="check" title={title}>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      {children}
    </label>
  );
}

/** A horizontal slider with its value shown beside it. `format` renders the value. */
export function Slider({
  value,
  onChange,
  min,
  max,
  step = 1,
  width = 160,
  format = (v) => String(v),
}: {
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
  width?: number;
  format?: (v: number) => string;
}) {
  return (
    <div className="form-slider">
      <input type="range" min={min} max={max} step={step} value={value} style={{ width }} onChange={(e) => onChange(parseFloat(e.target.value))} />
      <span className="form-slider-value">{format(value)}</span>
    </div>
  );
}
