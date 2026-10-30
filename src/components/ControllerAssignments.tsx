// Logic's Controller Assignments window (Control Surfaces › Controller Assignments): every
// assignment with its controller, parameter, takeover mode and range, and the banner shown
// while an assignment is being learned. Assignments are global settings, saved by the engine
// in the config folder, so editing them isn't an undo step.
import { useEffect, useState } from "react";
import { call } from "../api";
import { channelLabel, controllerLabel, fromPercent, MODE_OPTIONS, toPercent, TRANSPORT_TARGETS, type AssignmentRow, type Takeover } from "../controllers";
import { keyHint } from "../keymap";
import { cancelLearn, learn } from "../learn";
import { onTick, setState, useStore } from "../store";
import { Sheet } from "./Dialogs";
import { NumberField, Select } from "./Form";
import { openMenuAt } from "./Menu";

function Row({ row, index, onChange }: { row: AssignmentRow; index: number; onChange: (rows: AssignmentRow[] | null) => void }) {
  const update = (mode: Takeover, min: number, max: number) => call<AssignmentRow[]>("update_controller_assignment", { index, mode, min, max }).then(onChange);
  const c = row.controller;
  return (
    <div className={"ctl-row" + (row.available ? "" : " unavailable")} title={row.available ? undefined : "Not in this project: the assignment does nothing here"}>
      <span className="ctl-controller">{controllerLabel(c)}</span>
      <span className="ctl-channel">{channelLabel(c)}</span>
      <span className="ctl-port" title={c.port}>
        {c.port}
      </span>
      <span className="ctl-param" title={row.name}>
        {row.name}
      </span>
      {row.button ? (
        <span className="ctl-button dim">Button: press toggles</span>
      ) : (
        <>
          <Select value={row.mode} options={MODE_OPTIONS} onChange={(m) => update(m, row.min, row.max)} width={86} />
          <NumberField value={toPercent(row.min)} min={0} max={100} width={64} onChange={(v) => update(row.mode, fromPercent(v), row.max)} />
          <NumberField value={toPercent(row.max)} min={0} max={100} width={64} onChange={(v) => update(row.mode, row.min, fromPercent(v))} />
        </>
      )}
      <button className="push" title="Delete this assignment" onClick={() => call<AssignmentRow[]>("delete_controller_assignment", { index }).then(onChange)}>
        Delete
      </button>
    </div>
  );
}

/** The Controller Assignments sheet. */
export function ControllerAssignmentsSheet() {
  const [rows, setRows] = useState<AssignmentRow[]>([]);
  const project = useStore((s) => s.snap?.project);
  const close = () => setState({ showControllerAssignments: false });
  const apply = (r: AssignmentRow[] | null) => r && setRows(r);
  // Names follow the project (a renamed or deleted track), and a learn adds a row.
  useEffect(() => {
    call<AssignmentRow[]>("controller_assignments").then(apply);
  }, [project]);
  useEffect(
    () =>
      onTick((t) => {
        if (t.learned) call<AssignmentRow[]>("controller_assignments").then(apply);
      }),
    [],
  );
  const learnKey = keyHint("learn-assignment");
  return (
    <Sheet title="Controller Assignments" onClose={close} width={800}>
      <div className="ctl-head">
        <span>Controller</span>
        <span>Channel</span>
        <span>Port</span>
        <span>Parameter</span>
        <span>Mode</span>
        <span>Min %</span>
        <span>Max %</span>
        <span />
      </div>
      <div className="ctl-list">
        {rows.length === 0 && (
          <div className="pref-hint">
            No assignments yet. Move a fader, knob or plug-in parameter, choose Learn Assignment{learnKey ? ` (${learnKey})` : ""}, then move a control on your MIDI
            controller. Right-click a control to learn it directly.
          </div>
        )}
        {rows.map((r, i) => (
          <Row key={`${i}:${r.controller.port}:${r.controller.channel}:${JSON.stringify(r.controller.kind)}`} row={r} index={i} onChange={apply} />
        ))}
      </div>
      <div className="sheet-foot">
        <button className="push" onMouseDown={(e) => openMenuAt(e.currentTarget, TRANSPORT_TARGETS.map(({ label, target }) => ({ label, onClick: () => learn(target) })))}>
          Learn Transport Button…
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

/** "Move a controller to assign it to …" with Cancel, while a learn waits; Esc cancels too. */
export function LearnBanner() {
  const [name, setName] = useState<string | null>(null);
  useEffect(
    () =>
      onTick((t) => {
        setName(t.learn ?? null);
        if (t.learned) setState({ toast: { text: t.learned, at: Date.now(), ms: 3000 } });
      }),
    [],
  );
  useEffect(() => {
    if (!name) return;
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      cancelLearn();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [name]);
  if (!name) return null;
  return (
    <div className="learn-banner">
      <span>
        Move a controller to assign it to <b>{name}</b>…
      </span>
      <button className="push" onClick={cancelLearn}>
        Cancel
      </button>
    </div>
  );
}
