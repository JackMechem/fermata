// A small sheet that asks for a name (New Alternative, Save as Template, renames): `askName`
// resolves with the trimmed name, or null when cancelled. It sits above the other sheets and
// the Project Chooser, so they can ask too.
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { nameProblem } from "../alternativesModel";
import { sheetKey } from "./Form";

interface Ask {
  title: string;
  label: string;
  value: string;
  ok: string;
  /** Checks the name as typed: a message keeps the sheet open with it. */
  check: (name: string) => string | null;
  resolve: (name: string | null) => void;
}

let current: Ask | null = null;
const subs = new Set<() => void>();
const emit = () => subs.forEach((s) => s());

export function askName(opts: { title: string; label?: string; value?: string; ok?: string; check?: (name: string) => string | null }): Promise<string | null> {
  current?.resolve(null);
  return new Promise((resolve) => {
    current = { title: opts.title, label: opts.label ?? "Name:", value: opts.value ?? "", ok: opts.ok ?? "OK", check: opts.check ?? nameProblem, resolve };
    emit();
  });
}

function finish(name: string | null) {
  const a = current;
  current = null;
  emit();
  a?.resolve(name);
}

export function NameSheetHost() {
  const ask = useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => current,
  );
  return ask ? <NameSheet key={ask.title + ask.value} ask={ask} /> : null;
}

function NameSheet({ ask }: { ask: Ask }) {
  const [name, setName] = useState(ask.value);
  const [problem, setProblem] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.select(), []);
  const submit = () => {
    const p = ask.check(name);
    if (p) setProblem(p);
    else finish(name.trim());
  };
  return (
    <div className="sheet-backdrop name-sheet-backdrop" onMouseDown={() => finish(null)}>
      <div className="sheet" style={{ width: 380 }} onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => sheetKey(e, () => finish(null))}>
        <div className="sheet-title">{ask.title}</div>
        <div className="name-sheet-row">
          <label>{ask.label}</label>
          <input
            ref={input}
            className="text-field"
            value={name}
            autoFocus
            onChange={(e) => {
              setName(e.target.value);
              setProblem(null);
            }}
            onKeyDown={(e) => e.key === "Enter" && submit()}
          />
        </div>
        {problem && <div className="name-sheet-problem">{problem}</div>}
        <div className="sheet-foot">
          <span />
          <div className="sheet-buttons">
            <button className="push" onClick={() => finish(null)}>
              Cancel
            </button>
            <button className="push default" disabled={!name.trim()} onClick={submit}>
              {ask.ok}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
