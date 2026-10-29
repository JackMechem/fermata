import { useEffect, useState } from "react";
import { act, type Marker } from "../api";
import { keyHint } from "../keymap";
import { createMarker, deleteMarker } from "../markers";
import { setState, useStore } from "../store";
import { toBbt } from "../time";
import { Sheet } from "./Dialogs";

function MarkerRow({ m }: { m: Marker }) {
  const project = useStore((s) => s.snap?.project);
  const [name, setName] = useState(m.name);
  useEffect(() => setName(m.name), [m.name]);
  if (!project) return null;
  const b = toBbt(project, m.pos);
  const commit = () => {
    const n = name.trim();
    if (n && n !== m.name) act("rename_marker", { id: m.id, name: n });
    else setName(m.name);
  };
  return (
    <div className="marker-row">
      <span className="swatch" style={{ background: m.color ?? "#9a9aa0" }} />
      <span className="pos">
        {b.bar} {b.beat} {b.div} {b.tick}
      </span>
      <input className="group-name" value={name} onChange={(e) => setName(e.target.value)} onBlur={commit} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} />
      <button className="push" title="Go to this marker" onClick={() => act("locate", { pos: m.pos })}>
        Go To
      </button>
      <button className="push" title="Delete this marker" onClick={() => deleteMarker(m.id)}>
        Delete
      </button>
    </div>
  );
}

/** Logic's Marker List: every marker with its position, an editable name and a Go To button. */
export function MarkerList() {
  const markers = useStore((s) => s.snap?.project.markers ?? []);
  const close = () => setState({ showMarkerList: false });
  return (
    <Sheet title="Marker List" onClose={close} width={480}>
      <div className="marker-list">
        {markers.length === 0 && <div className="pref-hint">No markers yet. Create Marker ({keyHint("create-marker")}) adds one at the playhead.</div>}
        {markers.map((m) => (
          <MarkerRow key={m.id} m={m} />
        ))}
      </div>
      <div className="sheet-foot">
        <button className="push" onClick={() => createMarker()}>
          Create Marker
        </button>
        <div className="sheet-buttons">
          <button className="push default" onClick={close}>
            Done
          </button>
        </div>
      </div>
    </Sheet>
  );
}
