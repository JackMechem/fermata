// File › Project Alternatives › Edit Alternatives…: the project's alternatives, to switch to,
// rename or delete, and a button for a new one (as Logic's Edit Alternatives window).
import { useEffect, useState } from "react";
import { call, type ProjectFiles } from "../api";
import { deleteAlternative, newAlternative, renameAlternative, switchAlternative } from "../projectFiles";
import { setState, useStore } from "../store";
import { Sheet } from "./Dialogs";

export function AlternativesSheet() {
  const [files, setFiles] = useState<ProjectFiles | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  // Re-read after anything that can change the list.
  const alternative = useStore((s) => s.snap?.project.alternative);
  const path = useStore((s) => s.snap?.path);
  const saved = useStore((s) => s.snap?.saved);
  const [tick, setTick] = useState(0);
  const close = () => setState({ dialog: null });
  useEffect(() => {
    call<ProjectFiles>("project_files").then((f) => {
      setFiles(f);
      setSelected((cur) => (f && cur && f.alternatives.includes(cur) ? cur : (f?.active ?? null)));
    });
  }, [alternative, path, saved, tick]);
  const after = (p: Promise<unknown>) => p.then(() => setTick((t) => t + 1));
  const list = files?.alternatives ?? [];
  const sel = selected && list.includes(selected) ? selected : null;
  return (
    <Sheet title="Project Alternatives" onClose={close} width={420}>
      <div className="alt-list">
        {list.map((name) => (
          <div key={name} className={"alt-row" + (name === sel ? " on" : "")} onMouseDown={() => setSelected(name)} onDoubleClick={() => after(switchAlternative(name))}>
            <span className="alt-check">{name === files?.active ? "✓" : ""}</span>
            <span className="alt-name">{name}</span>
            {name === files?.active && <span className="dim">current</span>}
          </div>
        ))}
      </div>
      <div className="pref-hint">Alternatives are versions of this project that share its audio files. Double-click one to switch to it; the current one is saved first.</div>
      <div className="sheet-foot">
        <div className="sheet-buttons">
          <button className="push" onClick={() => after(newAlternative())}>
            New…
          </button>
          <button className="push" disabled={!sel} onClick={() => sel && after(renameAlternative(sel, list))}>
            Rename…
          </button>
          <button className="push" disabled={!sel || list.length < 2} onClick={() => sel && after(deleteAlternative(sel))}>
            Delete…
          </button>
        </div>
        <div className="sheet-buttons">
          <button className="push" disabled={!sel || sel === files?.active} onClick={() => sel && after(switchAlternative(sel))}>
            Switch
          </button>
          <button className="push default" onClick={close}>
            Done
          </button>
        </div>
      </div>
    </Sheet>
  );
}
