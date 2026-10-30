// The Step Input Keyboard (Logic's Window › Show Step Input Keyboard): a floating panel that
// enters notes one step at a time at the playhead, into the Piano Roll's track. Notes come from
// its own keyboard, from MIDI inputs (with the engine's ticks) and from Musical Typing; its key
// commands are in the keymap's Step Input group. The state machine and the edits are in
// stepInput.ts; this panel feeds it and carries out what it returns, one edit after another.
import { useEffect, useReducer, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { act, actMade, type Project, type Track } from "../api";
import { commandForEvent, keyHint } from "../keymap";
import * as si from "../stepInput";
import { currentTick, getState, onTick, setState, useStore } from "../store";
import { tempoMap } from "../tempo";

const BLACK = new Set([1, 3, 6, 8, 10]);
const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
/** Logic's octave numbering: MIDI note 60 is C3. */
const noteName = (n: number) => `${NAMES[n % 12]}${Math.floor(n / 12) - 2}`;

/** The panel's settings outlive it, as Logic's window keeps them. */
let saved = { opts: si.DEFAULT_OPTS, dynamic: 4, asPlayed: true, base: 48 };

/** The track the Piano Roll shows (the selected region's, else the selected track). */
function editorTrack(project: Project | undefined, selectedRegions: number[], selected: number | null | undefined): Track | null {
  const first = selectedRegions[0];
  const t = project?.tracks.find((t) => t.regions.some((r) => r.id === first)) ?? project?.tracks.find((t) => t.id === selected);
  return t?.kind === "instrument" ? t : null;
}

const midiRegions = (t: Track) => t.regions.filter((r) => r.content.type === "midi");

export function StepInput() {
  const project = useStore((s) => s.snap?.project);
  const selectedRegions = useStore((s) => s.selectedRegions);
  const selectedTrack = useStore((s) => s.snap?.selected);
  const [opts, setOptsState] = useState(saved.opts);
  const [dynamic, setDynamic] = useState(saved.dynamic);
  const [asPlayed, setAsPlayed] = useState(saved.asPlayed);
  const [base, setBase] = useState(saved.base);
  const [playing, setPlaying] = useState(false);
  const [, bump] = useReducer((x: number) => x + 1, 0);
  const [pos, setPos] = useState(() => ({ x: Math.max(20, window.innerWidth / 2 - 330), y: Math.max(80, window.innerHeight - 380) }));
  const track = editorTrack(project, selectedRegions, selectedTrack);

  const machine = useRef(si.initialState());
  const live = useRef({ opts, dynamic, asPlayed });
  live.current = { opts, dynamic, asPlayed };
  saved = { opts, dynamic, asPlayed, base };
  // Edits go out one at a time, each reading the snapshot the one before left.
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const session = useRef(Date.now());
  const mouseNote = useRef<number | null>(null);

  const setOpts = (patch: Partial<si.StepOpts>) => {
    const next = { ...live.current.opts, ...patch };
    live.current = { ...live.current, opts: next };
    setOptsState(next);
    return next;
  };

  /** Carries out one effect of the state machine on `trackId`. */
  const perform = async (e: si.Effect, trackId: number) => {
    if (e.type === "locate") {
      await act("locate", { pos: e.pos });
      return;
    }
    const pr = getState().snap?.project;
    const t = pr?.tracks.find((x) => x.id === trackId);
    if (!pr || !t) return;
    const map = tempoMap(pr);
    if (e.type === "insert") {
      const key = `step-input:${session.current}:${e.key}`;
      let region = si.regionFor(t.regions, e.note.start, map);
      if (!region) {
        // A bar-long region where the step is, in the same undo step as its notes.
        const [start, length] = si.newRegionSpan(map, e.note.start);
        const ids = await actMade("create_midi_region", { track: trackId, start, length, key });
        region = getState().snap?.project.tracks.find((x) => x.id === trackId)?.regions.find((r) => r.id === ids?.[0]) ?? null;
        if (!region) return;
      }
      const { edits, selNotes } = si.insertEdits(region, [e.note], map);
      if (!edits.length) return;
      await act("set_midi_content", { edits, label: "Step Input", key });
      // The notes just entered are selected, as in Logic.
      setState({ midiSelection: { notes: [...selNotes], cc: [] } });
    } else if (e.type === "remove") {
      const edits = si.removeEdits(midiRegions(t), e.notes, map);
      if (edits.length) await act("set_midi_content", { edits, label: "Step Input Back", key: null });
    } else {
      const edits = si.resizeEdits(midiRegions(t), e.notes, map);
      if (edits.length) await act("set_midi_content", { edits, label: "Sustain Inserted Notes", key: `step-input:${session.current}:${e.key}` });
    }
  };

  const dispatch = (ev: si.StepEvent, o = live.current.opts) => {
    const tick = currentTick();
    // Steps go in with the transport stopped; while it plays, notes are just heard.
    if (tick?.playing || tick?.recording) return;
    const s = getState();
    const pr = s.snap?.project;
    const t = editorTrack(pr, s.selectedRegions, s.snap?.selected);
    if (!pr || !t) {
      if (ev.type === "down" || ev.type === "rest") setState({ toast: { text: "Select a software instrument track (or one of its regions) for Step Input.", at: Date.now() } });
      return;
    }
    const { state, effects } = si.reduce(machine.current, ev, tempoMap(pr), Math.round(tick?.pos ?? 0), o);
    machine.current = state;
    for (const e of effects) queue.current = queue.current.then(() => perform(e, t.id)).catch(() => {});
    bump();
  };

  /** A note-on or note-off from a MIDI input or Musical Typing. */
  const fromMidi = (d: [number, number, number]) => {
    const status = d[0] & 0xf0;
    const { asPlayed, dynamic } = live.current;
    if (status === 0x90 && d[2] > 0) dispatch({ type: "down", pitch: d[1] & 127, velocity: asPlayed ? d[2] : si.DYNAMICS[dynamic].velocity });
    else if (status === 0x80 || status === 0x90) dispatch({ type: "up", pitch: d[1] & 127 });
  };

  const toggleChord = () => {
    const o = setOpts({ chord: !live.current.opts.chord });
    if (!o.chord) dispatch({ type: "chord-off" }, o);
  };

  const commands: Record<string, () => void> = {
    "step-rest": () => dispatch({ type: "rest" }),
    "step-back": () => dispatch({ type: "back" }),
    "step-sustain": () => dispatch({ type: "sustain" }),
    "step-chord": toggleChord,
    "step-quantize": () => setOpts({ quantize: !live.current.opts.quantize }),
    "step-dotted": () => setOpts({ dotted: !live.current.opts.dotted, triplet: false }),
    "step-triplet": () => setOpts({ triplet: !live.current.opts.triplet, dotted: false }),
    ...Object.fromEntries(si.NOTE_VALUES.map((v) => [`step-${v}`, () => setOpts({ value: v })])),
  };
  const cmds = useRef(commands);
  cmds.current = commands;

  useEffect(() => {
    let started = false;
    const offTick = onTick((t) => {
      if (!t.playing) machine.current = si.observe(machine.current, t.pos);
      // Keys let go of while playing aren't seen: start over with none held.
      else if (machine.current.down.length || machine.current.open) machine.current = { ...machine.current, down: [], open: null };
      setPlaying(t.playing || t.recording);
      // The tick handed over on subscribing was already seen.
      if (started) for (const d of t.midi_in ?? []) fromMidi(d);
    });
    started = true;
    const offLocal = si.onLocalMidi(fromMidi);
    // Capture phase, like Musical Typing: Step Input's keys win over the focused editor's.
    const key = (e: KeyboardEvent) => {
      const tgt = e.target as HTMLElement | null;
      if (tgt && ["INPUT", "SELECT", "TEXTAREA"].includes(tgt.tagName)) return;
      if (getState().dialog) return;
      const id = commandForEvent(e);
      const run = id ? cmds.current[id] : undefined;
      if (!run) return;
      if (!e.repeat || id === "step-rest" || id === "step-back") run();
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", key, true);
    return () => {
      offTick();
      offLocal();
      window.removeEventListener("keydown", key, true);
    };
  }, []);

  // The on-screen keyboard plays (on the live MIDI track) and enters at the panel's velocity.
  const press = (n: number) => {
    release();
    mouseNote.current = n;
    const velocity = si.DYNAMICS[live.current.dynamic].velocity;
    invoke("midi_event", { data: [0x90, n, velocity] }).catch(() => {});
    dispatch({ type: "down", pitch: n, velocity });
  };
  const release = () => {
    const n = mouseNote.current;
    if (n === null) return;
    mouseNote.current = null;
    invoke("midi_event", { data: [0x80, n, 0] }).catch(() => {});
    dispatch({ type: "up", pitch: n });
  };
  useEffect(() => {
    const up = () => release();
    window.addEventListener("pointerup", up);
    window.addEventListener("blur", up);
    return () => {
      window.removeEventListener("pointerup", up);
      window.removeEventListener("blur", up);
      up();
    };
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

  const hint = (id: string, label: string) => {
    const k = keyHint(id);
    return k ? `${label} (${k})` : label;
  };
  const toggle = (on: boolean) => "si-btn" + (on ? " on" : "");

  // Four octaves from the panel's octave.
  const held = new Set(machine.current.down);
  const lo = base;
  const hi = Math.min(127, lo + 48);
  const whites: number[] = [];
  const blacks: number[] = [];
  for (let n = lo; n <= hi; n++) (BLACK.has(n % 12) ? blacks : whites).push(n);
  const W = 18;
  const whiteIndex = (n: number) => whites.filter((w) => w < n).length;
  const key = (n: number, black: boolean) => (
    <div
      key={n}
      className={(black ? "mt-black" : "mt-white") + (held.has(n) ? " on" : "")}
      style={black ? { left: whiteIndex(n) * W - 5, width: 10 } : { left: whiteIndex(n) * W, width: W }}
      onPointerDown={(e) => {
        e.preventDefault();
        press(n);
      }}
      title={noteName(n)}
    >
      {!black && n % 12 === 0 && <span className="mt-c">{noteName(n)}</span>}
    </div>
  );

  return (
    <div className="mt-window si-window" style={{ left: pos.x, top: pos.y }} onMouseDown={(e) => e.stopPropagation()}>
      <div className="eq-titlebar" onMouseDown={startMove}>
        <span className="eq-title">Step Input</span>
        <span className="eq-track">{!track ? "Select a software instrument track" : playing ? "Stop playback to enter notes" : `→ ${track.name}`}</span>
        <button className="eq-close" title={hint("step-input", "Close")} onClick={() => setState({ showStepInput: false })}>
          ×
        </button>
      </div>
      <div className="si-rows">
        <div className="si-row">
          <div className="si-seg">
            {si.NOTE_VALUES.map((v) => (
              <button key={v} className={toggle(opts.value === v)} title={hint(`step-${v}`, `1/${v} note`)} onClick={() => setOpts({ value: v })}>
                1/{v}
              </button>
            ))}
          </div>
          <div className="si-seg">
            <button className={toggle(opts.dotted)} title={hint("step-dotted", "Dotted: half as long again")} onClick={() => commands["step-dotted"]()}>
              Dotted
            </button>
            <button className={toggle(opts.triplet)} title={hint("step-triplet", "Triplet: two thirds as long")} onClick={() => commands["step-triplet"]()}>
              Triplet
            </button>
          </div>
        </div>
        <div className="si-row">
          <div className="si-seg">
            {si.DYNAMICS.map((d, i) => (
              <button key={d.name} className={toggle(dynamic === i) + " si-dyn"} title={`Velocity ${d.velocity}`} onClick={() => setDynamic(i)}>
                {d.name}
              </button>
            ))}
          </div>
          <button className={toggle(asPlayed)} title="Notes from a MIDI keyboard or Musical Typing keep the velocity they were played with; off, they all get the velocity chosen here" onClick={() => setAsPlayed(!asPlayed)}>
            As Played
          </button>
        </div>
        <div className="si-row">
          <div className="si-seg">
            <button className="si-btn" title={hint("step-rest", "Rest: move on a step")} onClick={() => commands["step-rest"]()}>
              Rest
            </button>
            <button className="si-btn" title={hint("step-back", "Back: delete the last step and step back")} onClick={() => commands["step-back"]()}>
              Back
            </button>
          </div>
          <div className="si-seg">
            <button className={toggle(opts.chord)} title={hint("step-chord", "Chord: notes go in at the same position until this is turned off (or Rest)")} onClick={toggleChord}>
              Chord
            </button>
            <button className="si-btn" title={hint("step-sustain", "Sustain inserted notes: lengthen the last notes by a step")} onClick={() => commands["step-sustain"]()}>
              Sustain
            </button>
            <button className={toggle(opts.quantize)} title={hint("step-quantize", "Quantize: snap the playhead to the step grid first")} onClick={() => commands["step-quantize"]()}>
              Quantize
            </button>
          </div>
          <div className="mt-group si-octave">
            <button className="mt-step" title="Octave down" onClick={() => setBase((b) => Math.max(0, b - 12))}>
              −
            </button>
            <span className="mt-value">{noteName(base)}</span>
            <button className="mt-step" title="Octave up" onClick={() => setBase((b) => Math.min(84, b + 12))}>
              +
            </button>
          </div>
        </div>
      </div>
      <div className="mt-keys si-keys" style={{ width: whites.length * W }}>
        {whites.map((n) => key(n, false))}
        {blacks.map((n) => key(n, true))}
      </div>
    </div>
  );
}
