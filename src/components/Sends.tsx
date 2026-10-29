import { invoke } from "@tauri-apps/api/core";
// The channel strip's Sends section and Output slot, laid out like Logic's.
import { useRef, useState } from "react";
import { SEND_SLOTS, type Send, type Track } from "../api";
import { emptySendMenu, outputLabel, outputMenu, sendLabel, sendMenu, setSendLevel } from "../routing";
import { useStore } from "../store";
import { dbToFader, faderToDb, formatDb } from "../time";
import { openMenuAt } from "./Menu";

/** A small unipolar level knob, on the fader's taper. Shift-drag adjusts finely; double-click
 *  is 0 dB. */
function SendKnob({ db, pre, onChange, onDrag }: { db: number; pre: boolean; onChange: (db: number) => void; onDrag: (on: boolean) => void }) {
  // The unrounded knob position. A drag adds to it rather than to the rounded dB it reports,
  // so slow movements still add up.
  const pos = useRef(dbToFader(db));
  pos.current = dbToFader(db);
  const size = 16;
  const r = size / 2 - 2;
  const start = (e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    let last = e.clientY;
    onDrag(true);
    const move = (ev: MouseEvent) => {
      pos.current = Math.min(1, Math.max(0, pos.current + (last - ev.clientY) / (ev.shiftKey ? 600 : 120)));
      last = ev.clientY;
      onChange(Math.round(faderToDb(pos.current) * 10) / 10);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      onDrag(false);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  // -135° (off) .. +135° (+6 dB), clockwise from the top.
  const angle = -135 + dbToFader(db) * 270;
  const at = (a: number, rad = r) => {
    const t = ((a - 90) * Math.PI) / 180;
    return [size / 2 + rad * Math.cos(t), size / 2 + rad * Math.sin(t)];
  };
  const [x0, y0] = at(-135);
  const [x1, y1] = at(angle);
  const [px, py] = at(angle, r - 2.5);
  return (
    <div
      className={"send-knob" + (pre ? " pre" : "")}
      onMouseDown={start}
      onDoubleClick={(e) => {
        e.stopPropagation();
        onChange(0);
      }}
      title={`Send level ${formatDb(db)} dB${pre ? " (pre fader)" : ""} — double-click for 0 dB`}
    >
      <svg width={size} height={size}>
        <circle cx={size / 2} cy={size / 2} r={r} className="send-knob-ring" />
        {angle > -134 && <path d={`M${x0} ${y0} A${r} ${r} 0 ${angle > 45 ? 1 : 0} 1 ${x1} ${y1}`} className="send-knob-arc" />}
        <circle cx={size / 2} cy={size / 2} r={r - 2} className="send-knob-body" />
        <line x1={size / 2} y1={size / 2} x2={px} y2={py} className="send-knob-pointer" />
      </svg>
    </div>
  );
}

function SendSlot({ track, slot, send }: { track: Track; slot: number; send: Send | null }) {
  const aux = useStore((s) => (send ? s.snap?.project.tracks.find((t) => t.id === send.bus) : undefined));
  const [dragging, setDragging] = useState(false);
  const menu = (e: React.MouseEvent) => {
    e.stopPropagation();
    const el = (e.currentTarget as HTMLElement).closest(".slot") as HTMLElement;
    openMenuAt(el, send ? sendMenu(track, slot, send) : emptySendMenu(track, slot));
  };
  if (!send) return <div className="slot empty send" onMouseDown={menu} title="Add a send" />;
  const name = aux ? sendLabel(aux) : "—";
  return (
    <div className={"slot filled send" + (send.muted ? " muted" : "")}>
      <span className="slot-name" onMouseDown={menu} title={`${aux ? `${name} → ${aux.name}` : name}${send.pre_fader ? " · pre fader" : ""}${send.muted ? " · muted" : ""}`}>
        {dragging ? formatDb(send.level_db) : name}
      </span>
      <SendKnob db={send.level_db} pre={send.pre_fader} onChange={(db) => setSendLevel(track.id, slot, db)}
        onDrag={(on) => {
          setDragging(on);
          // Lets Touch/Latch automation know the send is held. Plain invoke, not call(): a
          // failure only costs the touch hint and isn't worth an error toast mid-drag.
          invoke("automation_touch", { track: track.id, param: { type: "send", value: slot }, touching: on }).catch(() => {});
        }}
      />
    </div>
  );
}

/** The Sends section, under Audio FX. Shows the filled slots plus one empty one. */
export function SendsSection({ track }: { track: Track }) {
  const sends = Array.from({ length: SEND_SLOTS }, (_, i) => track.sends?.[i] ?? null);
  const lastFilled = sends.reduce((acc, s, i) => (s ? i : acc), -1);
  const shown = Math.min(SEND_SLOTS, Math.max(lastFilled + 2, 1));
  return (
    <div className="strip-section">
      <div className="strip-label">Sends</div>
      {sends.slice(0, shown).map((s, i) => (
        <SendSlot key={i} track={track} slot={i} send={s} />
      ))}
    </div>
  );
}

/** The Output slot: Stereo Out, the track's summing stack, or an aux. */
export function OutputSlot({ track }: { track: Track }) {
  const label = useStore((s) => (s.snap ? outputLabel(s.snap.project, track) : "Stereo Out"));
  return (
    <div
      className="slot output output-pick"
      onMouseDown={(e) => {
        e.stopPropagation();
        openMenuAt(e.currentTarget as HTMLElement, outputMenu(track));
      }}
      title="Output"
    >
      <span className="slot-name">{label}</span>
    </div>
  );
}
