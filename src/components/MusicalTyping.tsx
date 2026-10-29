// Musical Typing: play the selected software instrument from the computer keyboard or by
// clicking an on-screen piano, as in Logic (⌘K). Keys map by physical position, so the layout
// works on non-QWERTY keyboards too.
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { setState, useStore } from "../store";
import { fmt } from "../keymap";

// Physical key -> semitone above the current octave's C.
const NOTE_KEYS: Record<string, number> = {
  KeyA: 0, KeyW: 1, KeyS: 2, KeyE: 3, KeyD: 4, KeyF: 5, KeyT: 6, KeyG: 7, KeyY: 8, KeyH: 9,
  KeyU: 10, KeyJ: 11, KeyK: 12, KeyO: 13, KeyL: 14, KeyP: 15, Semicolon: 16, Quote: 17,
};
const KEY_LABELS: Record<string, string> = {
  KeyA: "A", KeyW: "W", KeyS: "S", KeyE: "E", KeyD: "D", KeyF: "F", KeyT: "T", KeyG: "G", KeyY: "Y", KeyH: "H",
  KeyU: "U", KeyJ: "J", KeyK: "K", KeyO: "O", KeyL: "L", KeyP: "P", Semicolon: ";", Quote: "'",
};
const LABEL_FOR_OFFSET: Record<number, string> = Object.fromEntries(Object.entries(NOTE_KEYS).map(([code, off]) => [off, KEY_LABELS[code]]));

const BLACK = new Set([1, 3, 6, 8, 10]);
const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
/** Logic's octave numbering: MIDI note 60 is C3. */
const noteName = (n: number) => `${NAMES[n % 12]}${Math.floor(n / 12) - 2}`;

function send(data: [number, number, number]) {
  invoke("midi_event", { data }).catch(() => {});
}

export function MusicalTyping() {
  const target = useStore((s) => s.snap?.midi_target ?? null);
  const [base, setBase] = useState(60);
  const [velocity, setVelocity] = useState(100);
  const [sustain, setSustain] = useState(false);
  const [active, setActive] = useState<Set<number>>(new Set());
  const [pos, setPos] = useState(() => ({ x: Math.max(20, window.innerWidth / 2 - 380), y: Math.max(80, window.innerHeight - 300) }));
  const live = useRef({ base, velocity });
  live.current = { base, velocity };
  // Notes held per source, so note-offs match even if the octave changed meanwhile.
  const heldKeys = useRef(new Map<string, number>());
  const mouseNote = useRef<number | null>(null);

  const noteOn = (n: number, vel: number) => {
    if (n < 0 || n > 127) return;
    send([0x90, n, Math.max(1, Math.min(127, Math.round(vel)))]);
    setActive((a) => new Set(a).add(n));
  };
  const noteOff = (n: number) => {
    send([0x80, n, 0]);
    setActive((a) => {
      const b = new Set(a);
      b.delete(n);
      return b;
    });
  };
  const allOff = () => {
    heldKeys.current.forEach((n) => send([0x80, n, 0]));
    heldKeys.current.clear();
    if (mouseNote.current !== null) send([0x80, mouseNote.current, 0]);
    mouseNote.current = null;
    send([0xb0, 64, 0]);
    setSustain(false);
    setActive(new Set());
  };

  useEffect(() => {
    const isField = (t: EventTarget | null) => t instanceof HTMLElement && ["INPUT", "SELECT", "TEXTAREA"].includes(t.tagName);
    // Capture phase: these keys play notes instead of reaching the app's shortcuts.
    const down = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || isField(e.target)) return;
      const { base, velocity } = live.current;
      if (e.code in NOTE_KEYS) {
        if (!e.repeat && !heldKeys.current.has(e.code)) {
          const n = base + NOTE_KEYS[e.code];
          heldKeys.current.set(e.code, n);
          noteOn(n, velocity);
        }
      } else if (e.code === "KeyZ") setBase((b) => Math.max(0, b - 12));
      else if (e.code === "KeyX") setBase((b) => Math.min(108, b + 12));
      else if (e.code === "KeyC") setVelocity((v) => Math.max(1, v - 10));
      else if (e.code === "KeyV") setVelocity((v) => Math.min(127, v + 10));
      else if (e.code === "Tab") {
        if (!e.repeat) {
          send([0xb0, 64, 127]);
          setSustain(true);
        }
      } else return;
      e.preventDefault();
      e.stopPropagation();
    };
    const up = (e: KeyboardEvent) => {
      if (e.code in NOTE_KEYS) {
        const n = heldKeys.current.get(e.code);
        if (n !== undefined) {
          heldKeys.current.delete(e.code);
          noteOff(n);
        }
      } else if (e.code === "Tab") {
        send([0xb0, 64, 0]);
        setSustain(false);
      } else return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    window.addEventListener("blur", allOff);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
      window.removeEventListener("blur", allOff);
      allOff();
    };
  }, []);

  // Mouse playing: click a key, drag across keys for a glissando.
  const pressAt = (n: number, e: React.PointerEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    // Lower on the key plays louder, like a real key.
    const vel = 30 + ((e.clientY - r.top) / r.height) * 97;
    if (mouseNote.current !== null) noteOff(mouseNote.current);
    mouseNote.current = n;
    noteOn(n, vel);
  };
  useEffect(() => {
    const release = () => {
      if (mouseNote.current !== null) {
        noteOff(mouseNote.current);
        mouseNote.current = null;
      }
    };
    window.addEventListener("pointerup", release);
    return () => window.removeEventListener("pointerup", release);
  }, []);

  const startMove = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest("button")) return;
    const sx = e.clientX - pos.x;
    const sy = e.clientY - pos.y;
    const move = (ev: MouseEvent) => setPos({ x: Math.max(0, ev.clientX - sx), y: Math.max(0, ev.clientY - sy) });
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  // Four octaves from one below the typing octave.
  const lo = Math.max(0, base - 12);
  const hi = Math.min(127, lo + 48);
  const whites: number[] = [];
  const blacks: number[] = [];
  for (let n = lo; n <= hi; n++) (BLACK.has(n % 12) ? blacks : whites).push(n);
  const W = 24;
  const whiteIndex = (n: number) => whites.filter((w) => w < n).length;

  const key = (n: number, black: boolean) => {
    const off = n - base;
    const label = off >= 0 && off <= 17 ? LABEL_FOR_OFFSET[off] : undefined;
    const style = black ? { left: whiteIndex(n) * W - 7, width: 14 } : { left: whiteIndex(n) * W, width: W };
    return (
      <div
        key={n}
        className={(black ? "mt-black" : "mt-white") + (active.has(n) ? " on" : "") + (label ? " typed" : "")}
        style={style}
        onPointerDown={(e) => {
          e.preventDefault();
          (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
          pressAt(n, e);
        }}
        onPointerEnter={(e) => {
          if (mouseNote.current !== null && e.buttons & 1) pressAt(n, e);
        }}
        title={noteName(n)}
      >
        {label && <span className="mt-key">{label}</span>}
        {!black && n % 12 === 0 && <span className="mt-c">{noteName(n)}</span>}
      </div>
    );
  };

  return (
    <div className="mt-window" style={{ left: pos.x, top: pos.y }} onMouseDown={(e) => e.stopPropagation()}>
      <div className="eq-titlebar" onMouseDown={startMove}>
        <span className="eq-title">Musical Typing</span>
        <span className="eq-track">{target ? `→ ${target}` : "Select a software instrument track"}</span>
        <button className="eq-close" title={`Close (${fmt("Mod+K")})`} onClick={() => setState({ showTyping: false })}>
          ×
        </button>
      </div>
      <div className="mt-toolbar">
        <div className="mt-group">
          <span className="dim">Octave</span>
          <button className="mt-step" title="Octave down (Z)" onClick={() => setBase((b) => Math.max(0, b - 12))}>
            −
          </button>
          <span className="mt-value">{noteName(base)}</span>
          <button className="mt-step" title="Octave up (X)" onClick={() => setBase((b) => Math.min(108, b + 12))}>
            +
          </button>
          <span className="mt-hint">Z X</span>
        </div>
        <div className="mt-group">
          <span className="dim">Velocity</span>
          <button className="mt-step" title="Softer (C)" onClick={() => setVelocity((v) => Math.max(1, v - 10))}>
            −
          </button>
          <span className="mt-value">{velocity}</span>
          <button className="mt-step" title="Louder (V)" onClick={() => setVelocity((v) => Math.min(127, v + 10))}>
            +
          </button>
          <span className="mt-hint">C V</span>
        </div>
        <button
          className={"mt-sustain" + (sustain ? " on" : "")}
          title="Sustain pedal (hold Tab)"
          onPointerDown={() => {
            const on = !sustain;
            send([0xb0, 64, on ? 127 : 0]);
            setSustain(on);
          }}
        >
          Sustain <span className="mt-hint">Tab</span>
        </button>
      </div>
      <div className="mt-keys" style={{ width: whites.length * W }}>
        {whites.map((n) => key(n, false))}
        {blacks.map((n) => key(n, true))}
      </div>
    </div>
  );
}
