import { useEffect, useState } from "react";
import { act, type Marker } from "../api";
import { keyHint } from "../keymap";
import { createMarker, deleteMarker, markerEnd, selectMarker } from "../markers";
import { formatLength, parseLength } from "../eventList";
import { tempoMap } from "../tempo";
import { setState, useStore } from "../store";
import { toBbt } from "../time";
import { Sheet } from "./Dialogs";

/** A marker's length in bars, beats, divisions and ticks: its own, or (dimmed) to the next
 *  marker. Type a length to give it one; clear the field to make it reach the next marker. */
function LengthField({ m }: { m: Marker }) {
  const project = useStore((s) => s.snap?.project);
  const own = !!m.length && m.length > 0;
  const end = project ? markerEnd(project, m) : null;
  const shown = project && end !== null ? formatLength(tempoMap(project), m.pos, end - m.pos) : "";
  const [text, setText] = useState(own ? shown : "");
  useEffect(() => setText(own ? shown : ""), [own, shown]);
  if (!project) return null;
  const commit = () => {
    const t = text.trim();
    if (!t) {
      if (own) act("set_marker_length", { id: m.id, length: null });
      return;
    }
    const len = parseLength(t, tempoMap(project), m.pos);
    if (len !== null && len > 0 && len !== m.length) act("set_marker_length", { id: m.id, length: Math.round(len) });
    else setText(own ? shown : "");
  };
  return (
    <input
      className="group-name marker-length"
      value={text}
      placeholder={shown || "—"}
      title="Length (bars beats divisions ticks); empty: to the next marker"
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
    />
  );
}

function MarkerRow({ m }: { m: Marker }) {
  const project = useStore((s) => s.snap?.project);
  const selected = useStore((s) => s.selectedMarker === m.id);
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
    <div className={"marker-row" + (selected ? " selected" : "")} onMouseDown={() => selectMarker(m.id)}>
      <span className="swatch" style={{ background: m.color ?? "#9a9aa0" }} />
      <span className="pos">
        {b.bar} {b.beat} {b.div} {b.tick}
      </span>
      <input className="group-name" value={name} onChange={(e) => setName(e.target.value)} onBlur={commit} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} />
      <LengthField m={m} />
      <button className="push" title="Go to this marker" onClick={() => act("locate", { pos: m.pos })}>
        Go To
      </button>
      <button className="push" title="Delete this marker" onClick={() => deleteMarker(m.id)}>
        Delete
      </button>
    </div>
  );
}

/** Logic's Marker List: every marker with its position, an editable name and length, and a Go
 *  To button. */
export function MarkerList() {
  const markers = useStore((s) => s.snap?.project.markers) ?? [];
  const close = () => setState({ showMarkerList: false });
  return (
    <Sheet title="Marker List" onClose={close} width={580}>
      <div className="marker-list">
        {markers.length === 0 && <div className="pref-hint">No markers yet. Create Marker ({keyHint("create-marker")}) adds one at the playhead.</div>}
        {markers.length > 0 && (
          <div className="marker-row marker-head">
            <span />
            <span>Position</span>
            <span>Name</span>
            <span>Length</span>
          </div>
        )}
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
