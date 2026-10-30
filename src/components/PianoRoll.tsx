// The Piano Roll: Logic's MIDI editor for a software instrument track. It shows every MIDI
// region on the track in project time (the selected one highlighted, the others dimmed but
// editable), with a keyboard, a ruler, and a velocity / controller lane underneath.
//
// Editing logic lives in midiEdit.ts and drawing in pianoRollDraw.ts. This component handles
// the pointer and keys: a drag builds a preview of the edited list, and releasing sends the
// result with `set_midi_content` as one undo step.
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useLayoutEffect, useReducer, useRef, useState, useSyncExternalStore } from "react";
import { act, actMade, type CcKind, type Id, type Project, type Region, type Track } from "../api";
import { undoRedo } from "../files";
import { coalesced, framePos, onFrame } from "../frame";
import { Icon } from "../icons";
import * as me from "../midiEdit";
import { exportMidi } from "../midiFiles";
import { commandForEvent, fmt, keyHint } from "../keymap";
import { isEditorSelect, selectEvents } from "../noteSelect";
import {
  BLACK,
  CYCLE_H,
  drawGrid,
  drawKeys,
  drawLane,
  drawLaneScale,
  drawNotes,
  drawRuler,
  drawRubber,
  KEY_W,
  laneValue,
  laneY,
  noteName,
  pitchAt,
  PR_RULER_H,
  setupCanvas,
  timeAt,
  velocityColor,
  xOf,
  type Grid,
  type PrView,
} from "../pianoRollDraw";
import { currentTick, getState, setState, useStore, type MidiSelection } from "../store";
import { detachMenuItems, ownPanel } from "../detach";
import { DetachButton } from "./DetachButton";
import { EditorTabs } from "./EventList";
import { lengthBbt, samplesPerBeat, toBbt } from "../time";
import { barLines, musicalWarp, tempoMap, type Warp } from "../tempo";
import { closeMenu, menuOpen, openMenu, openMenuAt, type MenuItem } from "./Menu";
import { askName } from "./NameSheet";
import { getSettings } from "../prefs";
import { onRevealPlayhead, revealLeft } from "../view";
import { timeWheel } from "../wheel";
import { canRamp, createRamps, CURVE_SHAPES, densityStep, DENSITIES, drawCcCurve, nearestEvent, thinOut, type CurveShape, type Density } from "../ccCurves";

type PNote = me.PNote;
type PCc = me.PCc;

// ---- preferences (kept across editor switches and sessions) --------------------------------

export type Tool = "pointer" | "pencil" | "eraser" | "velocity" | "mute" | "scissors" | "glue" | "line";

interface Prefs {
  /** Horizontal zoom, pixels per beat. */
  pxPerBeat: number;
  noteH: number;
  laneH: number;
  showLane: boolean;
  /** "velocity", "cc:<n>", "pb" or "at". */
  lane: string;
  tool: Tool;
  grid: me.GridValue;
  snap: boolean;
  qValue: me.GridValue;
  /** Percentages. */
  qStrength: number;
  qSwing: number;
  qEnds: boolean;
  color: "velocity" | "region";
  scale: boolean;
  midiOut: boolean;
  catchPlayhead: boolean;
  /** The controller lane's Line Curve shape and Ramp Density (ccCurves.ts). */
  curve: CurveShape;
  density: Density;
}

const PREFS_KEY = "fermata.pianoRoll";
const DEFAULT_PREFS: Prefs = {
  pxPerBeat: 80,
  noteH: 12,
  laneH: 110,
  showLane: true,
  lane: "velocity",
  tool: "pointer",
  grid: "1/16",
  snap: true,
  qValue: "1/16",
  qStrength: 100,
  qSwing: 0,
  qEnds: false,
  color: "velocity",
  scale: false,
  midiOut: true,
  catchPlayhead: true,
  curve: "linear",
  density: "1/32",
};

let prefs: Prefs = (() => {
  try {
    return { ...DEFAULT_PREFS, ...JSON.parse(localStorage.getItem(PREFS_KEY) ?? "{}") };
  } catch {
    return DEFAULT_PREFS;
  }
})();
const prefSubs = new Set<() => void>();

function setPrefs(p: Partial<Prefs>) {
  prefs = { ...prefs, ...p };
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // Private mode: settings just don't persist.
  }
  prefSubs.forEach((f) => f());
}

function usePrefs(): Prefs {
  return useSyncExternalStore(
    (f) => {
      prefSubs.add(f);
      return () => prefSubs.delete(f);
    },
    () => prefs,
  );
}

// ---- lanes and tools ------------------------------------------------------------------------

const LANES: { id: string; label: string; short: string }[] = [
  { id: "velocity", label: "Velocity", short: "Velocity" },
  { id: "cc:1", label: "Modulation (CC 1)", short: "Modulation" },
  { id: "cc:11", label: "Expression (CC 11)", short: "Expression" },
  { id: "cc:64", label: "Sustain (CC 64)", short: "Sustain" },
  { id: "pb", label: "Pitch Bend", short: "Pitch Bend" },
  { id: "at", label: "Aftertouch", short: "Aftertouch" },
  { id: "cc:7", label: "Volume (CC 7)", short: "Volume" },
  { id: "cc:10", label: "Pan (CC 10)", short: "Pan" },
];

function laneKind(id: string): CcKind | null {
  if (id === "pb") return { type: "pitch_bend" };
  if (id === "at") return { type: "aftertouch" };
  const m = /^cc:(\d+)$/.exec(id);
  return m ? { type: "controller", cc: Math.min(127, parseInt(m[1])) } : null;
}

const laneShort = (id: string) => LANES.find((l) => l.id === id)?.short ?? `CC ${id.slice(3)}`;

const TOOLS: { id: Tool; label: string; key: string }[] = [
  { id: "pointer", label: "Pointer", key: "T" },
  { id: "pencil", label: "Pencil", key: "P" },
  { id: "eraser", label: "Eraser", key: "E" },
  { id: "velocity", label: "Velocity", key: "V" },
  { id: "mute", label: "Mute", key: "M" },
  { id: "scissors", label: "Scissors", key: "I" },
  { id: "glue", label: "Glue", key: "G" },
  { id: "line", label: "Line (lanes)", key: "L" },
];

/** A tool's icon (the Tracks area's tool pop-ups use the ones they share). */
export const ToolIcon = ({ tool }: { tool: Tool }) => {
  const p = { width: 14, height: 14, viewBox: "0 0 14 14", fill: "none", stroke: "currentColor", strokeWidth: 1.3, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  switch (tool) {
    case "pointer":
      return (
        <svg {...p}>
          <path d="M3.5 1.8v9.4l2.4-2.2 1.7 3.6 1.5-.7-1.7-3.5h3.2z" />
        </svg>
      );
    case "pencil":
      return (
        <svg {...p}>
          <path d="M2.5 11.5l.6-2.6 6.8-6.8 2 2-6.8 6.8zM8.8 3.2l2 2" />
        </svg>
      );
    case "eraser":
      return (
        <svg {...p}>
          <path d="M5.2 11.8 2 8.6l6-6 4.4 4.4-4.8 4.8zM4.6 6 9 10.4M5.2 11.8H12" />
        </svg>
      );
    case "velocity":
      return (
        <svg {...p}>
          <path d="M3 12V8M6 12V4.5M9 12V6.5M12 12V2.5" />
        </svg>
      );
    case "mute":
      return (
        <svg {...p}>
          <rect x="2" y="5" width="10" height="4" rx="1" />
          <path d="M2.5 12 11.5 2" />
        </svg>
      );
    case "scissors":
      return (
        <svg {...p}>
          <circle cx="3.6" cy="10.4" r="1.7" />
          <circle cx="10.4" cy="10.4" r="1.7" />
          <path d="M4.8 9.2 10.5 2M9.2 9.2 3.5 2" />
        </svg>
      );
    case "glue":
      return (
        <svg {...p}>
          <path d="M5 2.2h4l-.6 3.3h-2.8zM5.6 5.5 4.4 11.8h5.2L8.4 5.5" />
        </svg>
      );
    case "line":
      return (
        <svg {...p}>
          <path d="M2 11.5 12 2.5" />
          <circle cx="2.5" cy="11" r="1.2" fill="currentColor" />
          <circle cx="11.5" cy="3" r="1.2" fill="currentColor" />
        </svg>
      );
  }
};

// ---- the native menu bar and the editor's focus --------------------------------------------

let menuHandler: ((id: string) => boolean) | null = null;

/**
 * Lets the Piano Roll take Edit menu commands (Select All, Copy, Duplicate…) while it has
 * keyboard focus. Returns true when it handled `id`.
 */
export function pianoRollMenu(id: string): boolean {
  return menuHandler?.(id) ?? false;
}

/** Notes copied in the Piano Roll (separate from the region clipboard). */
let noteClipboard: me.ClipNote[] = [];
/** Controller events copied from the lane. One clipboard at a time: copying either clears the
 *  other, so Paste puts back what was copied last. */
let ccClipboard: me.ClipCc[] = [];

const timeBase = (p: Project): me.TimeBase => ({ sampleRate: p.sample_rate, tempo: p.tempo, sigNum: p.sig_num, sigDen: p.sig_den });
const isField = (t: EventTarget | null) => {
  const el = t as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "SELECT" || el.tagName === "TEXTAREA");
};

/** A press-and-drag on the window, with the handlers removed on release. */
function dragWith(move: (e: MouseEvent) => void, up: (e: MouseEvent) => void) {
  const m = (e: MouseEvent) => move(e);
  const u = (e: MouseEvent) => {
    window.removeEventListener("mousemove", m);
    window.removeEventListener("mouseup", u);
    up(e);
  };
  window.addEventListener("mousemove", m);
  window.addEventListener("mouseup", u);
}

interface Work {
  /** The snapshot's region list this was built from (identity). */
  source: Region[];
  /** The MIDI regions as they are after our last edit (ahead of the snapshot until it arrives). */
  regions: Region[];
  notes: PNote[];
  cc: PCc[];
  selNotes: Set<string>;
  selCc: Set<string>;
}

// A pattern region plays its steps, not its notes: the Step Sequencer edits it.
const midiRegions = (t: Track) => t.regions.filter((r) => r.content.type === "midi" && !r.pattern);

/** Piano Roll for a software instrument track; `region` is the one selected in the tracks area. */
export function PianoRoll({ track, region }: { track: Track; region: Region | null }) {
  const project = useStore((s) => s.snap?.project) as Project;
  const themeId = useStore((s) => s.themeId);
  const showStepInput = useStore((s) => s.showStepInput);
  useStore((s) => s.uiScale);
  const pr = usePrefs();
  const [, bump] = useReducer((x: number) => x + 1, 0);
  const [qOpen, setQOpen] = useState(false);

  // The working list follows the snapshot; edits replace it ahead of the next snapshot.
  const work = useRef<Work>({ source: [], regions: [], notes: [], cc: [], selNotes: new Set(), selCc: new Set() });
  // Edits sent but not yet answered. Snapshots arriving meanwhile are older than the working
  // list, so it isn't rebuilt from them until the last answer is in.
  const inflight = useRef(0);
  if (work.current.source !== track.regions && inflight.current === 0) {
    const regions = midiRegions(track);
    const w = me.fromRegions(regions, work.current.selNotes, work.current.selCc);
    work.current = { ...work.current, source: track.regions, regions, notes: w.notes, cc: w.cc };
  }
  // The note selection is shared with the Event List (the store's midiSelection).
  const sharedSel = useStore((s) => s.midiSelection);
  const seenSel = useRef<MidiSelection | null>(null);
  if (sharedSel !== seenSel.current) {
    seenSel.current = sharedSel;
    const selNotes = new Set(sharedSel.notes);
    const selCc = new Set(sharedSel.cc);
    const w = me.fromRegions(work.current.regions, selNotes, selCc);
    work.current = { ...work.current, notes: w.notes, cc: w.cc, selNotes, selCc };
  }
  const regions = work.current.regions;
  const focus = region && regions.some((r) => r.id === region.id) ? region.id : null;

  /** What a drag in progress would make the list; drawn instead of the working list. */
  const preview = useRef<{ notes: PNote[]; cc: PCc[] } | null>(null);
  const rubber = useRef<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const laneLine = useRef<{ x0: number; y0: number; x1: number; y1: number; shape?: CurveShape } | null>(null);
  const laneRubber = useRef<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const cyclePreview = useRef<{ start: number; end: number } | null>(null);
  const pressedKey = useRef<number | null>(null);
  const hoverPitch = useRef<number | null>(null);
  const lastArea = useRef<"notes" | "lane">("notes");
  const lastVelocity = useRef(100);
  const toolArmed = useRef(0);
  const pendingScroll = useRef<{ left?: number; top?: number } | null>(null);

  const rootRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const notesCv = useRef<HTMLCanvasElement>(null);
  const keysCv = useRef<HTMLCanvasElement>(null);
  const rulerCv = useRef<HTMLCanvasElement>(null);
  const laneCv = useRef<HTMLCanvasElement>(null);
  const scaleCv = useRef<HTMLCanvasElement>(null);
  const laneWrap = useRef<HTMLDivElement>(null);
  const playheads = [useRef<HTMLDivElement>(null), useRef<HTMLDivElement>(null), useRef<HTMLDivElement>(null)];
  const tagRef = useRef<HTMLDivElement>(null);
  const toolBtn = useRef<HTMLButtonElement>(null);
  const qBtn = useRef<HTMLButtonElement>(null);

  const live = useRef({ project, track, focus, pr, themeId });
  live.current = { project, track, focus, pr, themeId };

  // ---- geometry ---------------------------------------------------------------------------

  const view = (): PrView => {
    const el = scrollRef.current;
    const { project, pr } = live.current;
    return {
      pps: pr.pxPerBeat / samplesPerBeat(project),
      noteH: pr.noteH,
      left: el?.scrollLeft ?? 0,
      top: el?.scrollTop ?? 0,
      W: el?.clientWidth ?? 0,
      H: el?.clientHeight ?? 0,
    };
  };
  const grid = (): Grid => {
    const { project, pr } = live.current;
    const tb = timeBase(project);
    const len = me.gridLength(tb, pr.grid);
    return { bar: me.barLength(tb), beat: samplesPerBeat(project), grid: len, map: tempoMap(project), gridQ: len / me.quarterLength(tb) };
  };
  /** With tempo or signature changes, grid maths runs in musical time (see `musicalWarp`):
   *  grid lengths above are at the project's starting tempo, and warped positions keep
   *  their bar and beat. Null when one tempo and signature hold throughout. */
  const warp = (): Warp | null => {
    const p = live.current.project;
    const m = tempoMap(p);
    return m.isConstant ? null : musicalWarp(m, me.quarterLength(timeBase(p)));
  };
  /** Bar lines for grid maths on warped positions (a plain bar length without changes). */
  const warpedBars = (w: Warp | null) => w?.bars ?? grid().bar;
  /** Runs `f` on the notes in musical time, so grid-based edits follow the tempo map. */
  const musical = (notes: PNote[], f: (n: PNote[]) => PNote[]): PNote[] => {
    const w = warp();
    if (!w) return f(notes);
    const v = notes.map((n) => {
      const s = w.toV(n.start);
      return { ...n, start: s, length: w.toV(n.start + n.length) - s };
    });
    return f(v).map((n) => {
      const s = Math.round(w.fromV(n.start));
      return { ...n, start: s, length: Math.max(1, Math.round(w.fromV(n.start + n.length)) - s) };
    });
  };
  /** A grid-length span starting at `t`, in samples there. */
  const gridSpan = (t: number, len = grid().grid) => {
    const w = warp();
    return w ? Math.round(w.fromV(w.toV(t) + len) - t) : Math.round(len);
  };
  /** Snaps to the grid unless snapping is off or `free` (a modifier) is held. */
  const snapT = (t: number, free = false) => {
    const g = grid();
    if (!live.current.pr.snap || free) return Math.round(t);
    const w = warp();
    return w ? Math.round(w.fromV(me.snapTime(w.toV(t), g.grid, w.bars))) : me.snapTime(t, g.grid, g.bar);
  };
  /** The grid line at or before `t`, where the pencil puts a note. */
  const floorT = (t: number, free = false) => {
    const g = grid();
    if (!live.current.pr.snap || free) return Math.round(t);
    const w = warp();
    const v = w ? w.toV(t) : t;
    const b = me.barFloor(v, warpedBars(w));
    const at = b + Math.floor((v - b) / g.grid + 1e-6) * g.grid;
    return w ? Math.round(w.fromV(at)) : at;
  };

  const shown = () => preview.current ?? work.current;

  const noteColor = (n: PNote) => {
    if (live.current.pr.color === "velocity") return velocityColor(n.velocity);
    const r = work.current.regions.find((x) => x.id === n.region);
    return r?.color ?? live.current.track.color;
  };

  // ---- drawing ----------------------------------------------------------------------------

  const draw = () => {
    const el = scrollRef.current;
    const { project, track, focus, pr, themeId } = live.current;
    if (!el || !project) return;
    const v = view();
    const g = grid();
    const { notes, cc } = shown();
    const scene = { project, regions: work.current.regions, focus, track, scale: pr.scale ? me.scaleOf(project.key) : null };
    if (notesCv.current) {
      const ctx = setupCanvas(notesCv.current, v.W, v.H);
      drawGrid(ctx, v, g, scene);
      const muted = new Set(work.current.regions.filter((r) => r.muted).map((r) => r.id));
      drawNotes(ctx, v, notes, { color: noteColor, focus, mutedRegions: muted, themeId });
      if (rubber.current) drawRubber(ctx, rubber.current, themeId);
    }
    if (keysCv.current) drawKeys(setupCanvas(keysCv.current, KEY_W, v.H), v, pressedKey.current, hoverPitch.current, themeId);
    if (rulerCv.current) drawRuler(setupCanvas(rulerCv.current, v.W, PR_RULER_H), v, g, scene, cyclePreview.current);
    const lh = laneWrap.current?.clientHeight ?? 0;
    if (pr.showLane && laneCv.current && lh > 0) {
      drawLane(setupCanvas(laneCv.current, v.W, lh), v, g, lh, {
        kind: laneKind(pr.lane),
        notes,
        cc,
        color: noteColor,
        focus,
        line: laneLine.current,
        rubber: laneRubber.current,
        themeId,
        trackColor: track.color,
        ends: new Map(work.current.regions.map((r) => [r.id, r.start + r.length])),
      });
      if (scaleCv.current) drawLaneScale(setupCanvas(scaleCv.current, KEY_W, lh), lh, laneKind(pr.lane));
    }
    lastPh.current = "";
    placePlayhead();
  };
  const drawRef = useRef(draw);
  drawRef.current = draw;
  const [requestDraw] = useState(() => coalesced(() => drawRef.current()));

  const lastPh = useRef("");
  const placePlayhead = (pos: number = framePos()) => {
    const v = view();
    const x = xOf(v, pos);
    const vis = x >= -1 && x <= v.W + 1;
    const key = vis ? x.toFixed(1) : "hidden";
    if (key === lastPh.current) return;
    lastPh.current = key;
    for (const r of playheads) {
      if (!r.current) continue;
      r.current.style.transform = `translate3d(${x.toFixed(1)}px,0,0)`;
      r.current.style.display = vis ? "block" : "none";
    }
  };

  useLayoutEffect(() => {
    const ps = pendingScroll.current;
    const el = scrollRef.current;
    if (ps && el) {
      if (ps.left !== undefined) el.scrollLeft = Math.max(0, ps.left);
      if (ps.top !== undefined) el.scrollTop = Math.max(0, ps.top);
      pendingScroll.current = null;
    }
    requestDraw();
  });

  useEffect(() => {
    const el = scrollRef.current!;
    const ro = new ResizeObserver(() => requestDraw());
    ro.observe(el);
    if (laneWrap.current) ro.observe(laneWrap.current);
    const off = onFrame((now) => {
      const pos = framePos(now);
      placePlayhead(pos);
      const t = currentTick();
      if (t?.playing && live.current.pr.catchPlayhead) {
        const v = view();
        const x = xOf(v, pos);
        // Page along like Logic: jump when the playhead reaches the right edge.
        if (x > v.W - 24 || x < 0) el.scrollLeft = Math.max(0, pos * v.pps - v.W * 0.08);
      }
    });
    // Go to Beginning and a second Stop bring the playhead into view, as in the Tracks area.
    const offReveal = onRevealPlayhead((pos) => {
      const v = view();
      const left = revealLeft(pos * v.pps, v.left, v.W, v.W * 0.08);
      if (left !== null) el.scrollLeft = left;
    });
    return () => {
      ro.disconnect();
      off();
      offReveal();
    };
  }, []);

  // Scroll to the edited region when the track changes, or when another region is picked and
  // it's out of view.
  const scrolledFor = useRef<{ track: Id; focus: Id | null } | null>(null);
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const prev = scrolledFor.current;
    if (prev && prev.track === track.id && prev.focus === focus) return;
    scrolledFor.current = { track: track.id, focus };
    const r = regions.find((x) => x.id === focus) ?? regions[0];
    const v = view();
    if (prev && prev.track === track.id && r) {
      const x0 = xOf(v, r.start);
      if (x0 >= 0 && x0 < v.W - 40) return;
    }
    const notes = work.current.notes.filter((n) => !r || n.region === r.id);
    const mid = notes.length ? notes.reduce((a, n) => a + n.pitch, 0) / notes.length : 60;
    el.scrollLeft = r ? Math.max(0, r.start * v.pps - 24) : el.scrollLeft;
    el.scrollTop = Math.max(0, (127 - mid) * v.noteH - v.H / 2);
    requestDraw();
  }, [track.id, focus]);

  // ---- committing edits -------------------------------------------------------------------

  /** Shares the selection with the Event List. */
  const publishSelection = () => {
    const { selNotes, selCc } = work.current;
    const prev = getState().midiSelection;
    if (prev.notes.length === selNotes.size && prev.cc.length === selCc.size && prev.notes.every((k) => selNotes.has(k)) && prev.cc.every((k) => selCc.has(k))) return;
    seenSel.current = { notes: [...selNotes], cc: [...selCc] };
    setState({ midiSelection: seenSel.current });
  };

  const setSelection = (notes: PNote[], cc: PCc[] = work.current.cc) => {
    work.current = { ...work.current, notes, cc, selNotes: me.selectionKeys(notes), selCc: me.selectionKeys(cc) };
    publishSelection();
    preview.current = null;
    requestDraw();
    bump();
  };

  /** Sends an edited list to the engine as one undo step. */
  const commit = (notes: PNote[], cc: PCc[], label: string, key?: string) => {
    preview.current = null;
    const m = tempoMap(live.current.project);
    const { edits, selNotes, selCc } = me.toEdits(notes, cc, work.current.regions, m.isConstant ? grid().bar : barLines(m));
    if (!edits.length) {
      setSelection(notes, cc);
      return;
    }
    const next = me.applyEdits(work.current.regions, edits);
    const w = me.fromRegions(next, selNotes, selCc);
    work.current = { ...work.current, regions: next, notes: w.notes, cc: w.cc, selNotes, selCc };
    publishSelection();
    requestDraw();
    bump();
    inflight.current++;
    act("set_midi_content", { edits, label, key: key ?? null }).finally(() => {
      if (--inflight.current === 0) {
        resync();
        requestDraw();
        bump();
      }
    });
  };

  /** Rebuilds the working list from the store, after an edit that went through another command. */
  const resync = () => {
    const t = getState().snap?.project.tracks.find((x) => x.id === live.current.track.id);
    if (!t) return;
    const regs = midiRegions(t);
    const w = me.fromRegions(regs, work.current.selNotes, work.current.selCc);
    work.current = { ...work.current, source: t.regions, regions: regs, notes: w.notes, cc: w.cc };
  };

  /** The region a note or event at `t` goes in: one covering it (the edited one first), else
   *  one ending or starting within a bar of it, which then grows to hold it. Null when there
   *  is none, so a new region is made there. */
  const regionFor = (t: number): Id | null => {
    const regs = work.current.regions;
    const f = live.current.focus;
    const covers = (r: Region) => t >= r.start && t < r.start + r.length;
    const fr = regs.find((r) => r.id === f);
    if (fr && covers(fr)) return fr.id;
    const c = [...regs].reverse().find(covers);
    if (c) return c.id;
    const dist = (r: Region) => Math.min(Math.abs(t - r.start), Math.abs(t - (r.start + r.length)));
    const bar = grid().bar;
    if (fr && dist(fr) <= bar) return fr.id;
    let best: Region | null = null;
    let d = bar;
    for (const r of regs) if (dist(r) <= d) [best, d] = [r, dist(r)];
    return best?.id ?? null;
  };

  /** Commits notes that may sit in a region that doesn't exist yet (id -1): makes a one-bar
   *  region at `t` first. Both go in one undo step (a shared checkpoint key). */
  const commitWithRegion = async (notes: PNote[], t: number, label: string) => {
    if (!notes.some((n) => n.region === -1)) return commit(notes, work.current.cc, label);
    const m = tempoMap(live.current.project);
    const bar = m.barAt(Math.max(0, t));
    const start = Math.round(m.barStart(bar));
    const key = `new-region:${Date.now()}`;
    const ids = await actMade("create_midi_region", { track: live.current.track.id, start, length: Math.round(m.barStart(bar + 1)) - start, key });
    if (!ids?.length) {
      // Nothing was made (a frozen track, say): drop the note drawn in advance.
      preview.current = null;
      setSelection(work.current.notes);
      return;
    }
    setState({ selectedRegions: ids, focus: "regions" });
    resync();
    commit(
      notes.map((n) => (n.region === -1 ? { ...n, region: ids[0] } : n)),
      work.current.cc,
      label,
      key,
    );
  };

  // ---- sound ------------------------------------------------------------------------------

  const sounding = useRef<number | null>(null);
  const play = (pitch: number | null, velocity = 100) => {
    // MIDI Out off silences edits, but not the keyboard column (a pressed key), which is
    // played on purpose. Note-offs always go out, so nothing hangs.
    if (!live.current.pr.midiOut && pitch !== null && pressedKey.current === null) return;
    if (sounding.current !== null) invoke("midi_event", { data: [0x80, sounding.current, 0] }).catch(() => {});
    sounding.current = pitch;
    if (pitch !== null) invoke("midi_event", { data: [0x90, pitch, Math.max(1, Math.min(127, Math.round(velocity)))] }).catch(() => {});
  };

  const ensureSelectedTrack = () => {
    if (getState().snap?.selected !== live.current.track.id) act("select_track", { id: live.current.track.id });
  };

  // ---- help tag ---------------------------------------------------------------------------

  const showTag = (text: string | null, x = 0, y = 0) => {
    const el = tagRef.current;
    if (!el) return;
    if (text === null) {
      el.style.display = "none";
      return;
    }
    el.textContent = text;
    el.style.display = "block";
    el.style.transform = `translate(${Math.round(x + 14)}px, ${Math.round(y + 14)}px)`;
  };
  const posLabel = (t: number) => {
    const b = toBbt(live.current.project, t);
    return `${b.bar} ${b.beat} ${b.div} ${b.tick}`;
  };
  /** A length from `start`, counted through the tempo map. */
  const lenLabel = (len: number, start = 0) => {
    const b = lengthBbt(live.current.project, start, len);
    return `${b.bar} ${b.beat} ${b.div} ${b.tick}`;
  };

  // ---- note area --------------------------------------------------------------------------

  const local = (e: { clientX: number; clientY: number }, el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  /** The note under a point, and which part of it: an edge to resize or the body. */
  const hitNote = (notes: PNote[], x: number, y: number): { n: PNote; zone: "left" | "right" | "body" } | null => {
    const v = view();
    const p = pitchAt(v, y);
    let found: PNote | null = null;
    // Selected notes are drawn on top, so they're hit first.
    for (let pass = 0; pass < 2 && !found; pass++) {
      for (let i = notes.length - 1; i >= 0; i--) {
        const n = notes[i];
        if (n.pitch !== p || n.sel !== (pass === 0)) continue;
        const x0 = xOf(v, n.start);
        const x1 = x0 + Math.max(3, n.length * v.pps);
        if (x >= x0 - 1 && x <= x1 + 1) {
          found = n;
          break;
        }
      }
    }
    if (!found) return null;
    const x0 = xOf(v, found.start);
    const w = Math.max(3, found.length * v.pps);
    const edge = Math.min(6, w / 3);
    if (x > x0 + w - edge) return { n: found, zone: "right" };
    if (x < x0 + edge && w > 14) return { n: found, zone: "left" };
    return { n: found, zone: "body" };
  };

  const selectRegionOf = (n: PNote) => {
    if (n.region !== live.current.focus) setState({ selectedRegions: [n.region], focus: "regions" });
  };

  const onNotesDown = (e: React.MouseEvent) => {
    rootRef.current?.focus({ preventScroll: true });
    lastArea.current = "notes";
    const el = scrollRef.current!;
    const { x, y } = local(e, el);
    if (x > el.clientWidth || y > el.clientHeight) return; // the scrollbars
    if (e.button !== 0) return;
    e.preventDefault();
    ensureSelectedTrack();
    const v = view();
    let tool = live.current.pr.tool;
    // ⌘/Ctrl-click is the Pencil, like Logic's secondary tool.
    if ((e.ctrlKey || e.metaKey) && (tool === "pointer" || tool === "line")) tool = "pencil";
    if (tool === "line") tool = "pointer";
    const notes = work.current.notes;
    const hit = hitNote(notes, x, y);

    if (tool === "eraser") return sweep(() => null, "Delete Notes", hit?.n);
    if (tool === "mute") {
      const muted = hit ? !hit.n.muted : true;
      // On a selected note the whole selection toggles: muted unless all of it already is.
      if (hit?.n.sel) return commit(me.toggleMute(notes), work.current.cc, notes.every((n) => !n.sel || n.muted) ? "Unmute Notes" : "Mute Notes");
      return sweep((n) => ({ ...n, muted }), muted ? "Mute Notes" : "Unmute Notes", hit?.n);
    }
    if (hit && tool === "scissors") {
      const only = new Set(hit.n.sel ? notes.filter((n) => n.sel) : [hit.n]);
      const at = snapT(timeAt(v, x), e.altKey);
      return commit(me.splitAt(notes, at, only), work.current.cc, "Split Notes");
    }
    if (hit && tool === "glue") {
      const base = hit.n.sel ? notes : notes.map((n) => (n === hit.n ? { ...n, sel: true } : n));
      return commit(me.glue(base), work.current.cc, "Glue Notes");
    }

    if (hit) {
      selectRegionOf(hit.n);
      const wasSelected = hit.n.sel;
      let base: PNote[];
      if (e.shiftKey) base = notes.map((n) => (n === hit.n ? { ...n, sel: !n.sel } : n));
      else if (!hit.n.sel) base = notes.map((n) => (n === hit.n ? (n.sel ? n : { ...n, sel: true }) : n.sel ? { ...n, sel: false } : n));
      else base = notes;
      const grabbed = base[notes.indexOf(hit.n)];
      setSelection(base);
      if (!grabbed.sel) return;
      lastVelocity.current = grabbed.velocity;
      if (tool === "velocity") return velocityDrag(e, base, grabbed);
      play(grabbed.pitch, grabbed.velocity);
      return noteDrag(e, hit.zone === "body" ? "move" : hit.zone, base, grabbed, wasSelected && !e.shiftKey);
    }

    // Empty space.
    if (tool === "pencil" || (tool === "pointer" && e.detail === 2)) return pencil(e, x, y, tool === "pencil");
    if (tool === "pointer" || tool === "velocity" || tool === "scissors" || tool === "glue") return rubberBand(e, x, y);
  };

  /** Moving, copying (Alt) or resizing the selected notes. */
  const noteDrag = (e0: React.MouseEvent, kind: "move" | "left" | "right", base: PNote[], grabbed: PNote, reselectOnClick: boolean) => {
    const sx = e0.clientX;
    const sy = e0.clientY;
    const v = view();
    const g = grid();
    let moved = false;
    let copying = false;
    let lastPitch = grabbed.pitch;
    const el = scrollRef.current!;
    dragWith(
      (ev) => {
        if (!moved && Math.abs(ev.clientX - sx) < 3 && Math.abs(ev.clientY - sy) < 3) return;
        moved = true;
        const dxT = (ev.clientX - sx) / v.pps;
        const { x, y } = local(ev, el);
        let notes: PNote[];
        if (kind === "move") {
          const free = ev.ctrlKey || ev.metaKey;
          const target = snapT(grabbed.start + dxT, free);
          const dp = -Math.round((ev.clientY - sy) / v.noteH);
          // Across tempo changes the selection moves in musical time, keeping its rhythm.
          const w = warp();
          const shifted = w ? musical(base, (n) => me.nudge(n, w.toV(target) - w.toV(grabbed.start), dp)) : me.nudge(base, Math.round(target - grabbed.start), dp);
          copying = ev.altKey;
          notes = copying ? [...base.map((n) => (n.sel ? { ...n, sel: false } : n)), ...shifted.filter((n) => n.sel)] : shifted;
          const g2 = shifted[base.indexOf(grabbed)];
          if (g2.pitch !== lastPitch) {
            lastPitch = g2.pitch;
            play(g2.pitch, g2.velocity);
          }
          showTag(`${noteName(g2.pitch)}   ${posLabel(g2.start)}${copying ? "   copy" : ""}`, x, y);
        } else {
          const free = ev.ctrlKey || ev.metaKey || ev.altKey;
          const min = free ? 1 : Math.max(1, Math.min(g.grid, grabbed.length));
          if (kind === "right") {
            const end = snapT(grabbed.start + grabbed.length + dxT, free);
            const dl = end - (grabbed.start + grabbed.length);
            notes = base.map((n) => (n.sel ? { ...n, length: Math.max(Math.min(min, n.length), Math.round(n.length + dl)) } : n));
          } else {
            const ds = snapT(grabbed.start + dxT, free) - grabbed.start;
            notes = base.map((n) => {
              if (!n.sel) return n;
              const end = n.start + n.length;
              const s = Math.round(Math.max(0, Math.min(end - Math.min(min, n.length), n.start + ds)));
              return { ...n, start: s, length: end - s };
            });
          }
          const g2 = notes[base.indexOf(grabbed)];
          showTag(`${kind === "right" ? "Length" : "Start"}   ${kind === "right" ? lenLabel(g2.length, g2.start) : posLabel(g2.start)}`, x, y);
        }
        preview.current = { notes, cc: work.current.cc };
        requestDraw();
      },
      () => {
        play(null);
        showTag(null);
        if (!moved) {
          preview.current = null;
          // A click on a note in a selection selects just that note.
          if (reselectOnClick) setSelection(base.map((n) => (n === grabbed ? n : n.sel ? { ...n, sel: false } : n)));
          requestDraw();
          return;
        }
        const notes = preview.current?.notes ?? base;
        commit(notes, work.current.cc, kind === "move" ? (copying ? "Copy Notes" : "Move Notes") : "Resize Notes");
      },
    );
  };

  /** The Velocity tool: drag up or down on a note to change the selection's velocities. */
  const velocityDrag = (e0: React.MouseEvent, base: PNote[], grabbed: PNote) => {
    const sy = e0.clientY;
    const el = scrollRef.current!;
    let moved = false;
    dragWith(
      (ev) => {
        const delta = Math.round((sy - ev.clientY) / 2);
        if (!moved && delta === 0) return;
        moved = true;
        const notes = me.offsetVelocity(base, delta);
        const g2 = notes[base.indexOf(grabbed)];
        const { x, y } = local(ev, el);
        showTag(`Velocity ${g2.velocity}`, x, y);
        preview.current = { notes, cc: work.current.cc };
        requestDraw();
      },
      () => {
        showTag(null);
        if (!moved || !preview.current) return;
        const g2 = preview.current.notes[base.indexOf(grabbed)];
        lastVelocity.current = g2.velocity;
        play(g2.pitch, g2.velocity);
        window.setTimeout(() => play(null), 180);
        commit(preview.current.notes, work.current.cc, "Note Velocity");
      },
    );
  };

  /** Eraser and Mute: apply `f` to every note the pointer passes over (null deletes). */
  const sweep = (f: (n: PNote) => PNote | null, label: string, first?: PNote) => {
    const el = scrollRef.current!;
    const base = work.current.notes;
    const hits = new Set<PNote>(first ? [first] : []);
    const update = () => {
      const notes: PNote[] = [];
      for (const n of base) {
        if (!hits.has(n)) notes.push(n);
        else {
          const r = f(n);
          if (r) notes.push(r);
        }
      }
      preview.current = { notes, cc: work.current.cc };
      requestDraw();
    };
    update();
    dragWith(
      (ev) => {
        const { x, y } = local(ev, el);
        const h = hitNote(base, x, y);
        if (h && !hits.has(h.n)) {
          hits.add(h.n);
          update();
        }
      },
      () => {
        if (!hits.size || !preview.current) {
          preview.current = null;
          requestDraw();
          return;
        }
        commit(preview.current.notes, work.current.cc, label);
      },
    );
  };

  /** Creates a note of the grid length at the press; dragging right sets its length. */
  const pencil = (e0: React.MouseEvent, x: number, y: number, drag: boolean) => {
    const v = view();
    const start = Math.max(0, floorT(timeAt(v, x), e0.altKey));
    const pitch = pitchAt(v, y);
    const rid = regionFor(start) ?? -1;
    const note: PNote = { region: rid, start, length: gridSpan(start), pitch, velocity: lastVelocity.current, muted: false, sel: true };
    const base = me.selectNone(work.current.notes);
    preview.current = { notes: [...base, note], cc: work.current.cc };
    requestDraw();
    play(pitch, note.velocity);
    const sx = e0.clientX;
    const el = scrollRef.current!;
    dragWith(
      (ev) => {
        if (!drag) return;
        const free = ev.altKey || ev.ctrlKey || ev.metaKey;
        const end = snapT(start + gridSpan(start) + (ev.clientX - sx) / v.pps, free);
        note.length = Math.round(Math.max(free ? 1 : gridSpan(start), end - start));
        preview.current = { notes: [...base, { ...note }], cc: work.current.cc };
        const { x: lx, y: ly } = local(ev, el);
        showTag(`Length   ${lenLabel(note.length, start)}`, lx, ly);
        requestDraw();
      },
      () => {
        play(null);
        showTag(null);
        commitWithRegion([...base, { ...note }], start, "Create Note");
      },
    );
  };

  const rubberBand = (e0: React.MouseEvent, x0: number, y0: number) => {
    const el = scrollRef.current!;
    const add = e0.shiftKey;
    const base = work.current.notes;
    const v0 = view();
    // Positions in content coordinates, so scrolling during the drag keeps the anchor.
    const ax = x0 + v0.left;
    const ay = y0 + v0.top;
    let moved = false;
    dragWith(
      (ev) => {
        const { x, y } = local(ev, el);
        const v = view();
        if (!moved && Math.hypot(x + v.left - ax, y + v.top - ay) < 3) return;
        moved = true;
        rubber.current = { x0: ax - v.left, y0: ay - v.top, x1: x, y1: y };
        const notes = me.selectRect(base, timeAt(v, ax - v.left), timeAt(v, x), pitchAt(v, ay - v.top), pitchAt(v, y), add);
        preview.current = { notes, cc: work.current.cc };
        requestDraw();
      },
      () => {
        rubber.current = null;
        if (moved && preview.current) setSelection(preview.current.notes);
        else if (!add) setSelection(me.selectNone(base));
        else requestDraw();
      },
    );
  };

  const onNotesHover = (e: React.MouseEvent) => {
    if (e.buttons) return;
    const el = scrollRef.current!;
    const { x, y } = local(e, el);
    const p = pitchAt(view(), y);
    if (p !== hoverPitch.current) {
      hoverPitch.current = p;
      requestDraw();
    }
    const tool = live.current.pr.tool;
    if (tool !== "pointer" && tool !== "line" && tool !== "pencil") {
      el.style.cursor = "";
      return;
    }
    const h = hitNote(work.current.notes, x, y);
    el.style.cursor = h && h.zone !== "body" ? "ew-resize" : "";
  };

  // ---- note context menu ------------------------------------------------------------------

  const onNotesContext = (e: React.MouseEvent) => {
    e.preventDefault();
    rootRef.current?.focus({ preventScroll: true });
    const el = scrollRef.current!;
    const { x, y } = local(e, el);
    const hit = hitNote(work.current.notes, x, y);
    if (hit && !hit.n.sel) setSelection(work.current.notes.map((n) => (n === hit.n ? { ...n, sel: true } : n.sel ? { ...n, sel: false } : n)));
    const any = work.current.notes.some((n) => n.sel);
    openMenu(e.clientX, e.clientY, [
      { label: "Cut", hint: fmt("Mod+X"), disabled: !any, onClick: () => cut() },
      { label: "Copy", hint: fmt("Mod+C"), disabled: !any, onClick: () => copy() },
      { label: "Paste", hint: fmt("Mod+V"), disabled: !noteClipboard.length && !ccClipboard.length, onClick: () => paste() },
      { label: "Duplicate", hint: fmt("Mod+D"), disabled: !any, onClick: () => duplicate() },
      { label: "Delete", hint: fmt("Backspace"), disabled: !any, onClick: () => deleteSelected() },
      { separator: true },
      ...functionItems(),
    ]);
  };

  // ---- keyboard (piano) ---------------------------------------------------------------------

  const onKeysDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    rootRef.current?.focus({ preventScroll: true });
    ensureSelectedTrack();
    const cv = keysCv.current!;
    const keyAt = (ev: { clientX: number; clientY: number }) => {
      const { x, y } = local(ev, cv);
      const v = view();
      let p = pitchAt(v, y);
      // Right of a black key, the row belongs to the white keys either side.
      if (BLACK.has(p % 12) && x > KEY_W * 0.6) p += y + v.top - (127 - p) * v.noteH < v.noteH / 2 ? 1 : -1;
      // Further right plays louder, as on Logic's keyboard.
      return { p: Math.max(0, Math.min(127, p)), vel: Math.round(40 + (87 * Math.min(KEY_W, Math.max(0, x))) / KEY_W) };
    };
    const first = keyAt(e);
    // A click on a key also selects that pitch's notes (Shift adds), as in Logic.
    const notes = work.current.notes;
    setSelection(notes.map((n) => ({ ...n, sel: n.pitch === first.p || (e.shiftKey && n.sel) })));
    const press = (k: { p: number; vel: number }) => {
      pressedKey.current = k.p;
      play(k.p, k.vel);
      requestDraw();
    };
    press(first);
    dragWith(
      (ev) => {
        const k = keyAt(ev);
        if (k.p !== pressedKey.current) press(k);
      },
      () => {
        pressedKey.current = null;
        play(null);
        requestDraw();
      },
    );
  };

  // ---- ruler ------------------------------------------------------------------------------

  const onRulerDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    rootRef.current?.focus({ preventScroll: true });
    const cv = rulerCv.current!;
    const { x, y } = local(e, cv);
    const v = view();
    const project = live.current.project;
    const at = timeAt(v, x);
    if (y > CYCLE_H) {
      const locate = (t: number) => act("locate", { pos: Math.max(0, Math.round(snapT(t))) });
      locate(at);
      dragWith(
        (ev) => locate(timeAt(view(), local(ev, cv).x)),
        () => {},
      );
      return;
    }
    // The cycle strip: drag to set the locators, drag the range or its ends, click to toggle.
    const c = project.cycle;
    const has = c.end > c.start;
    const sx = xOf(v, c.start);
    const ex = xOf(v, c.end);
    let mode: "new" | "move" | "start" | "end" = "new";
    if (has && Math.abs(x - sx) < 6) mode = "start";
    else if (has && Math.abs(x - ex) < 6) mode = "end";
    else if (has && x > sx && x < ex) mode = "move";
    const g = grid();
    let moved = false;
    dragWith(
      (ev) => {
        const cx = local(ev, cv).x;
        if (!moved && Math.abs(cx - x) < 3) return;
        moved = true;
        const cur = timeAt(view(), cx);
        let s = c.start;
        let en = c.end;
        if (mode === "new") {
          s = snapT(Math.min(at, cur));
          en = snapT(Math.max(at, cur));
          if (en <= s) en = s + g.beat;
        } else if (mode === "move") {
          const d = snapT(c.start + (cur - at)) - c.start;
          s = c.start + d;
          en = c.end + d;
        } else if (mode === "start") s = Math.min(snapT(cur), c.end - g.grid);
        else en = Math.max(snapT(cur), c.start + g.grid);
        cyclePreview.current = { start: Math.max(0, s), end: Math.max(0, en) };
        requestDraw();
      },
      () => {
        const p = cyclePreview.current;
        cyclePreview.current = null;
        if (moved && p) act("set_cycle", { enabled: true, start: Math.round(p.start), end: Math.round(p.end) });
        else if (mode !== "new") act("set_cycle", { enabled: !c.enabled, start: c.start, end: c.end });
        requestDraw();
      },
    );
  };

  // ---- velocity / controller lane ------------------------------------------------------------

  const onLaneDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    rootRef.current?.focus({ preventScroll: true });
    lastArea.current = "lane";
    ensureSelectedTrack();
    const cv = laneCv.current!;
    const H = cv.clientHeight;
    const { x, y } = local(e, cv);
    const v = view();
    const kind = laneKind(live.current.pr.lane);
    let tool = live.current.pr.tool;
    // As in the note area, Mod-click is the Pencil, but here the Line tool is kept (it draws
    // controller ramps, while over notes it acts as the Pointer) and Shift turns the Pencil
    // into a line.
    if ((e.ctrlKey || e.metaKey) && tool === "pointer") tool = "pencil";
    if (tool === "pencil" && e.shiftKey) tool = "line";
    if (!kind) return velocityLaneDown(x, y, H, v, tool);
    return ccLaneDown(e, x, y, H, v, tool, kind);
  };

  const velocityLaneDown = (x: number, y: number, H: number, v: PrView, tool: Tool) => {
    const cv = laneCv.current!;
    const notes = work.current.notes;
    // The nearest stalk within a few pixels, selected notes first.
    let hit: PNote | null = null;
    let best = 5;
    for (const n of notes) {
      const d = Math.abs(xOf(v, n.start) - x) - (n.sel ? 0.5 : 0);
      if (d < best) [hit, best] = [n, d];
    }
    if (hit && tool !== "pencil" && tool !== "line") {
      const h = hit;
      const base = h.sel ? notes : notes.map((n) => (n === h ? { ...n, sel: true } : n.sel ? { ...n, sel: false } : n));
      const grabbed = base[notes.indexOf(h)];
      setSelection(base);
      selectRegionOf(grabbed);
      let moved = false;
      dragWith(
        (ev) => {
          moved = true;
          const ly = local(ev, cv).y;
          const val = laneValue(H, ly, 127);
          const next = me.offsetVelocity(base, val - grabbed.velocity);
          const g2 = next[base.indexOf(grabbed)];
          showLaneTag(`Velocity ${g2.velocity}`, ev);
          preview.current = { notes: next, cc: work.current.cc };
          requestDraw();
        },
        () => {
          showTag(null);
          if (!moved || !preview.current) return;
          lastVelocity.current = preview.current.notes[base.indexOf(grabbed)].velocity;
          commit(preview.current.notes, work.current.cc, "Note Velocity");
        },
      );
      return;
    }
    if (tool === "pencil") {
      // Freehand: notes the pointer passes get the value under it.
      let last = { x, y };
      let cur = notes;
      const apply = (a: { x: number; y: number }, b: { x: number; y: number }) => {
        const vv = view();
        const [l, r] = a.x < b.x ? [a, b] : [b, a];
        cur = me.rampVelocity(cur, timeAt(vv, l.x - 2), laneValue(H, l.y, 127), timeAt(vv, r.x + 2), laneValue(H, r.y, 127));
        preview.current = { notes: cur, cc: work.current.cc };
        requestDraw();
      };
      apply(last, last);
      dragWith(
        (ev) => {
          const p = local(ev, cv);
          apply(last, p);
          last = p;
        },
        () => preview.current && commit(preview.current.notes, work.current.cc, "Draw Velocity"),
      );
      return;
    }
    // Pointer or Line on empty space: draw a line to ramp the velocities under it.
    lineDrag(x, y, (l) => {
      const vv = view();
      const next = me.rampVelocity(work.current.notes, timeAt(vv, l.x0), laneValue(H, l.y0, 127), timeAt(vv, l.x1), laneValue(H, l.y1, 127));
      commit(next, work.current.cc, "Velocity Ramp");
    });
  };

  const lineDrag = (x: number, y: number, done: (l: { x0: number; y0: number; x1: number; y1: number }) => void) => {
    const cv = laneCv.current!;
    let moved = false;
    dragWith(
      (ev) => {
        const p = local(ev, cv);
        if (!moved && Math.hypot(p.x - x, p.y - y) < 3) return;
        moved = true;
        // A controller line shows the curve it will draw.
        const shape = laneKind(live.current.pr.lane) ? live.current.pr.curve : undefined;
        laneLine.current = { x0: x, y0: y, x1: p.x, y1: Math.max(0, Math.min(cv.clientHeight, p.y)), shape };
        requestDraw();
      },
      () => {
        const l = laneLine.current;
        laneLine.current = null;
        requestDraw();
        if (moved && l) done(l);
      },
    );
  };

  const showLaneTag = (text: string, ev: MouseEvent) => {
    const el = scrollRef.current!;
    const { x } = local(ev, el);
    showTag(text, x, el.clientHeight - 30);
  };

  /** Controller events live in regions; say so when one is drawn where there's none. */
  const noRegionHere = () => setState({ toast: { text: "Controller events go in a MIDI region: draw over a region, or a note first to make one", at: Date.now(), ms: 4000 } });

  const ccLaneDown = (e0: React.MouseEvent, x: number, y: number, H: number, v: PrView, tool: Tool, kind: CcKind) => {
    const cv = laneCv.current!;
    const max = me.ccMax(kind);
    const all = work.current.cc;
    // The event dot under the pointer.
    let hit: PCc | null = null;
    let best = 7;
    for (const c of all) {
      if (!me.sameKind(c.kind, kind)) continue;
      const d = Math.hypot(xOf(v, c.start) - x, laneY(H, c.value, max) - y);
      if (d < best) [hit, best] = [c, d];
    }
    const t0 = timeAt(v, x);

    if (tool === "eraser" || (hit && e0.altKey && tool === "pointer")) {
      // Everything of this kind between the press and the pointer goes.
      const pad = 4 / v.pps;
      // Only in the region the press is in, as the Pencil draws into one region.
      const inRegion = regionFor(t0) ?? undefined;
      const erase = (b: number) => {
        const next = me.eraseCc(all, kind, Math.min(t0, b) - pad, Math.max(t0, b) + pad, inRegion);
        preview.current = { notes: work.current.notes, cc: next };
        requestDraw();
      };
      erase(t0);
      dragWith(
        (ev) => erase(timeAt(view(), local(ev, cv).x)),
        () => {
          const next = preview.current?.cc;
          if (next && next.length !== all.length) commit(work.current.notes, next, "Delete Controller Events");
          else {
            preview.current = null;
            requestDraw();
          }
        },
      );
      return;
    }

    if (tool === "pencil") {
      const rid = regionFor(t0);
      if (rid === null) return noRegionHere();
      let last = { t: t0, v: laneValue(H, y, max) };
      let cur = me.drawCcLine(all, rid, kind, last.t, last.v, last.t, last.v, 1);
      preview.current = { notes: work.current.notes, cc: cur };
      requestDraw();
      // About one point per 1/64 note keeps freehand curves smooth but light.
      const step = me.quarterLength(timeBase(live.current.project)) / 16;
      dragWith(
        (ev) => {
          const p = local(ev, cv);
          const t = timeAt(view(), p.x);
          const val = laneValue(H, p.y, max);
          cur = me.drawCcLine(cur, rid, kind, last.t, last.v, t, val, step);
          last = { t, v: val };
          preview.current = { notes: work.current.notes, cc: cur };
          showLaneTag(`${laneShort(live.current.pr.lane)} ${val}`, ev);
          requestDraw();
        },
        () => {
          showTag(null);
          commit(work.current.notes, cur, "Draw Controller");
        },
      );
      return;
    }

    if (tool === "line") {
      lineDrag(x, y, (l) => {
        const vv = view();
        // An end on an existing event joins it: the line starts or stops at its time and value.
        const at = (px: number, py: number) => {
          const e = nearestEvent(work.current.cc, kind, px, py, (t) => xOf(vv, t), (val) => laneY(H, val, max));
          return e ? { t: e.start, v: e.value } : { t: timeAt(vv, px), v: laneValue(H, py, max) };
        };
        const [a, b] = [at(l.x0, l.y0), at(l.x1, l.y1)];
        const rid = regionFor(Math.min(a.t, b.t));
        if (rid === null) return noRegionHere();
        const { pr, project } = live.current;
        const step = densityStep(me.quarterLength(timeBase(project)), pr.density);
        commit(work.current.notes, drawCcCurve(work.current.cc, rid, kind, a.t, a.v, b.t, b.v, step, pr.curve), "Controller Line");
      });
      return;
    }

    if (hit) {
      // Select (Shift toggles) and drag the selected events.
      const h = hit;
      let base: PCc[];
      if (e0.shiftKey) base = all.map((c) => (c === h ? { ...c, sel: !c.sel } : c));
      else if (!h.sel) base = all.map((c) => (c === h ? { ...c, sel: true } : c.sel ? { ...c, sel: false } : c));
      else base = all;
      const grabbed = base[all.indexOf(h)];
      setSelection(work.current.notes, base);
      if (!grabbed.sel) return;
      const sx = e0.clientX;
      const sy = e0.clientY;
      let moved = false;
      dragWith(
        (ev) => {
          if (!moved && Math.abs(ev.clientX - sx) < 3 && Math.abs(ev.clientY - sy) < 3) return;
          moved = true;
          const free = ev.ctrlKey || ev.metaKey;
          const dt = Math.round(snapT(grabbed.start + (ev.clientX - sx) / v.pps, free) - grabbed.start);
          // The lane's value range spans its height less 6 px margins top and bottom (laneY).
          const dv = Math.round(((sy - ev.clientY) / (H - 12)) * max);
          const next = base.map((c) => (c.sel ? { ...c, start: Math.max(0, c.start + dt), value: me.clamp(c.value + dv, 0, max) } : c));
          const g2 = next[base.indexOf(grabbed)];
          showLaneTag(`${laneShort(live.current.pr.lane)} ${g2.value}   ${posLabel(g2.start)}`, ev);
          preview.current = { notes: work.current.notes, cc: next };
          requestDraw();
        },
        () => {
          showTag(null);
          if (moved && preview.current) commit(work.current.notes, preview.current.cc, "Move Controller Events");
        },
      );
      return;
    }

    if (e0.detail === 2) {
      const t = Math.max(0, snapT(t0, e0.altKey));
      const rid = regionFor(t);
      if (rid === null) return noRegionHere();
      const next = [...all.map((c) => (c.sel ? { ...c, sel: false } : c)), { region: rid, start: t, kind, value: laneValue(H, y, max), sel: true }];
      commit(work.current.notes, next.sort((a, b) => a.start - b.start), "Create Controller Event");
      return;
    }

    // Rubber band over the events.
    const add = e0.shiftKey;
    let moved = false;
    dragWith(
      (ev) => {
        const p = local(ev, cv);
        if (!moved && Math.hypot(p.x - x, p.y - y) < 3) return;
        moved = true;
        laneRubber.current = { x0: x, y0: y, x1: p.x, y1: p.y };
        const vv = view();
        const [ta, tb] = [timeAt(vv, Math.min(x, p.x)), timeAt(vv, Math.max(x, p.x))];
        const [va, vb] = [laneValue(H, Math.max(y, p.y), max), laneValue(H, Math.min(y, p.y), max)];
        const next = all.map((c) => {
          const inside = me.sameKind(c.kind, kind) && c.start >= ta && c.start <= tb && c.value >= va && c.value <= vb;
          const sel = add ? c.sel || inside : inside;
          return sel === c.sel ? c : { ...c, sel };
        });
        preview.current = { notes: work.current.notes, cc: next };
        requestDraw();
      },
      () => {
        laneRubber.current = null;
        if (moved && preview.current) setSelection(work.current.notes, preview.current.cc);
        else if (!add) setSelection(work.current.notes, all.map((c) => (c.sel ? { ...c, sel: false } : c)));
        else requestDraw();
      },
    );
  };

  const onSplitDown = (e: React.MouseEvent) => {
    e.preventDefault();
    const sy = e.clientY;
    const h0 = live.current.pr.laneH;
    dragWith(
      (ev) => setPrefs({ laneH: Math.round(Math.max(40, Math.min(420, h0 + sy - ev.clientY))) }),
      () => {},
    );
  };

  // ---- wheel: zoom and scroll ---------------------------------------------------------------

  const zoomX = (factor: number, anchorX?: number) => {
    const v = view();
    const ax = anchorX ?? v.W / 2;
    const t = timeAt(v, ax);
    const next = Math.max(6, Math.min(1600, live.current.pr.pxPerBeat * factor));
    const pps = next / samplesPerBeat(live.current.project);
    pendingScroll.current = { ...pendingScroll.current, left: t * pps - ax };
    setPrefs({ pxPerBeat: next });
  };
  const zoomY = (factor: number, anchorY?: number) => {
    const v = view();
    const ay = anchorY ?? v.H / 2;
    const rows = (ay + v.top) / v.noteH;
    const next = Math.max(5, Math.min(32, Math.round(live.current.pr.noteH * factor * 2) / 2));
    if (next === v.noteH) return;
    pendingScroll.current = { ...pendingScroll.current, top: rows * next - ay };
    setPrefs({ noteH: next });
  };

  useEffect(() => {
    const el = scrollRef.current!;
    const lane = laneWrap.current;
    const ruler = rulerCv.current;
    const keys = keysCv.current;
    const onNotes = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        zoomX(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - el.getBoundingClientRect().left);
      } else if (e.altKey) {
        e.preventDefault();
        zoomY(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientY - el.getBoundingClientRect().top);
      } else if (getSettings().ui.wheel_scrolls === "horizontal") {
        // Settings › Editing › Mouse Wheel Scrolls: the wheel moves through time, Shift+wheel
        // through the pitches. The default is left to native scrolling.
        e.preventDefault();
        const { dx, dy } = timeWheel(e, el.clientWidth);
        el.scrollLeft += dx;
        el.scrollTop += dy;
      }
    };
    // Over the ruler and the lane the wheel scrolls (or zooms) in time.
    const onTime = (e: WheelEvent) => {
      e.preventDefault();
      if (e.ctrlKey || e.metaKey) zoomX(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - el.getBoundingClientRect().left);
      else el.scrollLeft += e.deltaX || e.deltaY;
    };
    const onKeys = (e: WheelEvent) => {
      e.preventDefault();
      if (e.altKey || e.ctrlKey || e.metaKey) zoomY(e.deltaY < 0 ? 1.1 : 1 / 1.1, e.clientY - el.getBoundingClientRect().top);
      else el.scrollTop += e.deltaY;
    };
    el.addEventListener("wheel", onNotes, { passive: false });
    lane?.addEventListener("wheel", onTime, { passive: false });
    ruler?.addEventListener("wheel", onTime, { passive: false });
    keys?.addEventListener("wheel", onKeys, { passive: false });
    return () => {
      el.removeEventListener("wheel", onNotes);
      lane?.removeEventListener("wheel", onTime);
      ruler?.removeEventListener("wheel", onTime);
      keys?.removeEventListener("wheel", onKeys);
    };
  }, [pr.showLane]);

  // ---- commands -------------------------------------------------------------------------------

  const notesNow = () => work.current.notes;
  const anySelected = () => work.current.notes.some((n) => n.sel);
  const playhead = () => currentTick()?.pos ?? 0;

  /** Whether Copy, Cut and Delete act on controller events: some are selected in the lane
   *  shown, and the lane was clicked last or no notes are selected. */
  const ccTarget = () => !!laneKind(live.current.pr.lane) && work.current.cc.some((c) => c.sel) && (lastArea.current === "lane" || !anySelected());
  const canEdit = () => anySelected() || ccTarget();
  const copy = () => {
    if (ccTarget()) {
      ccClipboard = me.copyCc(work.current.cc);
      noteClipboard = [];
      return ccClipboard.length;
    }
    const c = me.copyNotes(notesNow());
    if (c.length) [noteClipboard, ccClipboard] = [c, []];
    return c.length;
  };
  const deleteSelected = () => {
    if (ccTarget()) {
      commit(notesNow(), work.current.cc.filter((c) => !c.sel), "Delete Controller Events");
      return;
    }
    if (!anySelected()) return;
    commit(
      notesNow().filter((n) => !n.sel),
      work.current.cc,
      "Delete Notes",
    );
  };
  const cut = () => {
    const cc = ccTarget();
    if (!copy()) return;
    if (cc) return commit(notesNow(), work.current.cc.filter((c) => !c.sel), "Cut Controller Events");
    commit(
      notesNow().filter((n) => !n.sel),
      work.current.cc,
      "Cut Notes",
    );
  };
  const paste = () => {
    const at = Math.round(playhead());
    if (ccClipboard.length) {
      const rid = regionFor(at);
      if (rid === null) return noRegionHere();
      commit(notesNow(), me.pasteCc(work.current.cc, ccClipboard, at, rid), "Paste Controller Events");
      return;
    }
    if (!noteClipboard.length) return;
    const rid = regionFor(at) ?? -1;
    commitWithRegion(me.pasteNotes(notesNow(), noteClipboard, at, rid), at, "Paste Notes");
  };
  const duplicate = () => {
    if (anySelected()) commit(musical(notesNow(), (n) => me.duplicate(n, grid().grid)), work.current.cc, "Duplicate Notes");
  };
  const selectAll = () => setSelection(me.selectAll(notesNow()));
  /** Edit › Select commands (Invert Selection, All Following…) act on the notes while the
   *  Piano Roll has focus, as in Logic; false for those that stay with the regions. */
  const selectCommand = (id: string): boolean => {
    if (!isEditorSelect(id)) return false;
    const c = live.current.project.cycle;
    const notes = notesNow();
    const items = notes.map((n) => ({ start: n.start, sel: n.sel, equal: String(n.pitch), velocity: n.velocity, muted: n.muted }));
    const sel = selectEvents(id, items, { playhead: playhead(), locators: c.end > c.start ? c : null });
    if (!sel) return false;
    setSelection(
      notes.map((n, i) => (n.sel === sel[i] ? n : { ...n, sel: sel[i] })),
      work.current.cc.map((x) => (x.sel ? { ...x, sel: false } : x)),
    );
    return true;
  };
  const apply = (f: (n: PNote[]) => PNote[], label: string, key?: string) => commit(f(notesNow()), work.current.cc, label, key);
  const quantizeNow = () => {
    const { project, pr } = live.current;
    const tb = timeBase(project);
    const bar = warpedBars(warp());
    apply((n) => musical(n, (v) => me.quantize(v, { grid: me.gridLength(tb, pr.qValue), bar, strength: pr.qStrength / 100, swing: pr.qSwing / 100, ends: pr.qEnds })), "Quantize");
  };
  const msToSamples = (ms: number) => (ms / 1000) * live.current.project.sample_rate;
  const humanize = (ms: number, vel: number) => apply((n) => me.humanize(n, { time: msToSamples(ms), velocity: vel, length: 0 }), "Humanize");
  /** Asks for one or more numbers (Transpose by…, Set Velocity to…) in the name sheet. */
  const ask = async (title: string, label: string, def: string, f: (v: number[]) => void) => {
    const numbers = (text: string) => text.split(/[,\s]+/).filter(Boolean).map(Number).filter((x) => Number.isFinite(x));
    const a = await askName({ title, label, value: def, check: (t) => (numbers(t).length ? null : "Type a number.") });
    if (a !== null) f(numbers(a));
  };
  const exportRegion = () => {
    const r = work.current.regions.find((x) => x.id === live.current.focus) ?? work.current.regions[0];
    if (r) exportMidi([r.id], r.name);
  };
  const nudgeKey = () => `nudge-notes:${live.current.track.id}`;

  /** The Functions menu, also used by the note context menu. */
  const functionItems = (): MenuItem[] => {
    const { project, pr } = live.current;
    const any = anySelected();
    const scope = any ? "" : " (All)";
    return [
      { label: `Quantize${scope}`, hint: "Q", onClick: quantizeNow },
      {
        label: "Quantize to",
        children: me.GRID_VALUES.map((g) => ({
          label: g,
          checked: g === pr.qValue,
          onClick: () => {
            setPrefs({ qValue: g });
            quantizeNow();
          },
        })),
      },
      { label: `Scale Quantize to ${project.key}`, onClick: () => apply((n) => me.scaleQuantize(n, me.scaleOf(project.key)), "Scale Quantize") },
      { separator: true },
      {
        label: "Transpose",
        children: [
          ...(
            [
              ["Octave Up", 12],
              ["Semitone Up", 1],
              ["Semitone Down", -1],
              ["Octave Down", -12],
            ] as [string, number][]
          ).map(([label, d]) => ({ label, onClick: () => apply((n) => me.transpose(n, d), "Transpose") })),
          { separator: true },
          { label: "Other…", onClick: () => ask("Transpose", "Semitones:", "7", ([d]) => apply((n) => me.transpose(n, Math.round(d)), "Transpose")) },
        ],
      },
      {
        label: "Velocity",
        children: [
          ...[32, 64, 80, 100, 127].map((v) => ({ label: `Set to ${v}`, onClick: () => apply((n) => me.setVelocity(n, v), "Set Velocity") })),
          { separator: true },
          ...[50, 80, 90, 110, 120, 150].map((p) => ({ label: `Scale ${p}%`, onClick: () => apply((n) => me.scaleVelocity(n, p), "Scale Velocity") })),
          { separator: true },
          { label: "Set to…", onClick: () => ask("Set Velocity", "Velocity (1–127):", "100", ([v]) => apply((n) => me.setVelocity(n, v), "Set Velocity")) },
          { label: "Scale by…", onClick: () => ask("Scale Velocity", "Percent:", "110", ([p]) => apply((n) => me.scaleVelocity(n, p), "Scale Velocity")) },
        ],
      },
      {
        label: "Humanize",
        children: [
          { label: "Light (±5 ms, ±5 velocity)", onClick: () => humanize(5, 5) },
          { label: "Medium (±10 ms, ±10 velocity)", onClick: () => humanize(10, 10) },
          { label: "Strong (±20 ms, ±20 velocity)", onClick: () => humanize(20, 20) },
          { label: "Custom…", onClick: () => ask("Humanize", "Timing (ms), velocity:", "10, 10", ([ms, v]) => humanize(ms, v ?? 0)) },
        ],
      },
      { separator: true },
      {
        label: "Note Length",
        children: [
          { label: "Legato", onClick: () => apply(me.legato, "Legato") },
          { label: "Remove Overlaps", onClick: () => apply(me.removeOverlaps, "Remove Overlaps") },
          { label: `Set to Grid (${pr.grid})`, onClick: () => apply((n) => musical(n, (v) => me.setLength(v, grid().grid)), "Set Note Length") },
          { separator: true },
          ...(["1/1", "1/2", "1/4", "1/8", "1/16", "1/32"] as const).map((g) => ({ label: `Set to ${g}`, onClick: () => apply((n) => musical(n, (v) => me.setLength(v, me.gridLength(timeBase(project), g))), "Set Note Length") })),
        ],
      },
      { label: "Mute / Unmute Notes", hint: fmt("Ctrl+M"), onClick: () => apply(me.toggleMute, "Mute Notes") },
      { label: "Delete Duplicates", onClick: () => apply(me.deleteDuplicates, "Delete Duplicates") },
      { label: "Reverse", onClick: () => apply(me.reverse, "Reverse Notes") },
      { label: "Split at Playhead", onClick: () => apply((n) => me.splitAt(n, playhead(), new Set(me.targets(n))), "Split Notes") },
      { label: "Glue Same Pitches", disabled: !any, onClick: () => apply(me.glue, "Glue Notes") },
    ];
  };

  const editItems = (): MenuItem[] => {
    const snap = getState().snap;
    const any = anySelected();
    return [
      { label: snap?.undo ? `Undo ${snap.undo}` : "Undo", hint: fmt("Mod+Z"), disabled: !snap?.undo, onClick: () => undoRedo("undo") },
      { label: snap?.redo ? `Redo ${snap.redo}` : "Redo", hint: fmt("Mod+Shift+Z"), disabled: !snap?.redo, onClick: () => undoRedo("redo") },
      { separator: true },
      { label: "Cut", hint: fmt("Mod+X"), disabled: !canEdit(), onClick: cut },
      { label: "Copy", hint: fmt("Mod+C"), disabled: !canEdit(), onClick: copy },
      { label: "Paste at Playhead", hint: fmt("Mod+V"), disabled: !noteClipboard.length && !ccClipboard.length, onClick: paste },
      { label: "Duplicate", hint: fmt("Mod+D"), disabled: !any, onClick: duplicate },
      { label: "Delete", hint: fmt("Backspace"), disabled: !canEdit(), onClick: deleteSelected },
      { separator: true },
      { label: "Select All", hint: fmt("Mod+A"), onClick: selectAll },
      { label: "Deselect All", hint: "Esc", onClick: () => setSelection(me.selectNone(notesNow())) },
      { label: "Invert Selection", onClick: () => setSelection(me.invertSelection(notesNow())) },
      { label: "Select Same Pitch", disabled: !any, onClick: () => setSelection(me.selectSamePitch(notesNow())) },
      { label: "Select Highest Notes", onClick: () => setSelection(me.selectTop(notesNow(), true, msToSamples(15))) },
      { label: "Select Lowest Notes", onClick: () => setSelection(me.selectTop(notesNow(), false, msToSamples(15))) },
      {
        label: "Select Muted Notes",
        onClick: () => setSelection(notesNow().map((n) => ({ ...n, sel: n.muted }))),
      },
      {
        label: "Select Inside Locators",
        disabled: !(live.current.project.cycle.end > live.current.project.cycle.start),
        onClick: () => {
          const c = live.current.project.cycle;
          setSelection(notesNow().map((n) => ({ ...n, sel: n.start >= c.start && n.start < c.end })));
        },
      },
      { separator: true },
      { label: "Export Region as MIDI File…", disabled: !work.current.regions.length, onClick: exportRegion },
    ];
  };

  const laneItems = (): MenuItem[] => {
    const cc = work.current.cc;
    const used = (id: string) => {
      const k = laneKind(id);
      return k ? cc.some((c) => me.sameKind(c.kind, k)) : false;
    };
    const custom = cc.filter((c) => c.kind.type === "controller" && !LANES.some((l) => l.id === `cc:${(c.kind as { cc: number }).cc}`)).map((c) => `cc:${(c.kind as { cc: number }).cc}`);
    const ids = [...LANES.map((l) => l.id), ...new Set(custom)];
    const cur = live.current.pr.lane;
    if (!ids.includes(cur)) ids.push(cur);
    return [
      ...ids.map((id) => ({
        label: LANES.find((l) => l.id === id)?.label ?? `Controller ${id.slice(3)}`,
        checked: cur === id,
        hint: used(id) ? "●" : undefined,
        onClick: () => setPrefs({ lane: id, showLane: true }),
      })),
      { separator: true },
      {
        label: "Other Controller…",
        onClick: () => ask("Show Controller", "Controller (0–127):", "74", ([n]) => n >= 0 && n <= 127 && setPrefs({ lane: `cc:${Math.round(n)}`, showLane: true })),
      },
      ...curveItems(),
    ];
  };

  /** The controller lane's curve settings and ramp functions (ccCurves.ts). */
  const curveItems = (): MenuItem[] => {
    const { pr, project } = live.current;
    const kind = laneKind(pr.lane);
    if (!kind) return [];
    const cc = work.current.cc;
    const step = densityStep(me.quarterLength(timeBase(project)), pr.density);
    return [
      { separator: true },
      { label: "Line Curve", children: CURVE_SHAPES.map(([id, label]) => ({ label, checked: pr.curve === id, onClick: () => setPrefs({ curve: id }) })) },
      { label: "Ramp Density", children: DENSITIES.map((d) => ({ label: d, checked: pr.density === d, onClick: () => setPrefs({ density: d }) })) },
      { label: "Create Ramp", disabled: !canRamp(cc, kind), onClick: () => commit(work.current.notes, createRamps(work.current.cc, kind, pr.curve, step), "Create Ramp") },
      {
        label: "Thin Out",
        onClick: () => {
          const next = thinOut(work.current.cc, kind, Math.max(1, Math.round(me.ccMax(kind) / 127)));
          if (next !== work.current.cc) commit(work.current.notes, next, "Thin Out Controller Events");
        },
      },
    ];
  };

  const viewItems = (): MenuItem[] => {
    const p = live.current.pr;
    return [
      { label: "Color by Velocity", checked: p.color === "velocity", onClick: () => setPrefs({ color: "velocity" }) },
      { label: "Color by Region", checked: p.color === "region", onClick: () => setPrefs({ color: "region" }) },
      { label: `Highlight Scale (${live.current.project.key})`, checked: p.scale, onClick: () => setPrefs({ scale: !p.scale }) },
      { separator: true },
      { label: "Show Velocity / Controller Lane", checked: p.showLane, onClick: () => setPrefs({ showLane: !p.showLane }) },
      { label: "Lane", children: laneItems() },
      { separator: true },
      { label: "Catch Playhead", checked: p.catchPlayhead, onClick: () => setPrefs({ catchPlayhead: !p.catchPlayhead }) },
      { label: "MIDI Out (Hear Edited Notes)", checked: p.midiOut, onClick: () => setPrefs({ midiOut: !p.midiOut }) },
      { separator: true },
      { label: "Zoom In", hint: fmt("Mod+ArrowRight"), onClick: () => zoomX(1.4) },
      { label: "Zoom Out", hint: fmt("Mod+ArrowLeft"), onClick: () => zoomX(1 / 1.4) },
      { label: "Taller Rows", hint: fmt("Mod+ArrowDown"), onClick: () => zoomY(1.2) },
      { label: "Shorter Rows", hint: fmt("Mod+ArrowUp"), onClick: () => zoomY(1 / 1.2) },
      { label: "Zoom to Fit Region", hint: "Z", onClick: zoomToFit },
      ...detachMenuItems("editor"),
    ];
  };

  const toolItems = (): MenuItem[] =>
    TOOLS.map((t) => ({ label: t.label, hint: t.key, checked: live.current.pr.tool === t.id, onClick: () => setPrefs({ tool: t.id }) }));

  /** Fits the edited region (or every region) and its notes into view. */
  const zoomToFit = () => {
    const el = scrollRef.current!;
    const regs = work.current.regions.filter((r) => live.current.focus === null || r.id === live.current.focus);
    if (!regs.length) return;
    const lo = me.minOf(regs, (r) => r.start);
    const hi = me.maxOf(regs, (r) => r.start + r.length);
    const spb = samplesPerBeat(live.current.project);
    const W = el.clientWidth - 40;
    const pxPerBeat = Math.max(6, Math.min(1600, (W / (hi - lo)) * spb));
    const notes = work.current.notes.filter((n) => regs.some((r) => r.id === n.region));
    const pLo = notes.length ? me.minOf(notes, (n) => n.pitch) : 48;
    const pHi = notes.length ? me.maxOf(notes, (n) => n.pitch) : 72;
    const noteH = Math.max(5, Math.min(24, Math.floor(el.clientHeight / (pHi - pLo + 5))));
    pendingScroll.current = { left: lo * (pxPerBeat / spb) - 20, top: (127 - pHi - 2) * noteH };
    setPrefs({ pxPerBeat, noteH });
  };

  // Native menu bar: Edit commands go to the Piano Roll while it has focus.
  useEffect(() => {
    const h = (id: string) => {
      if (!rootRef.current?.contains(document.activeElement)) return false;
      if (selectCommand(id)) return true;
      const f: Record<string, () => void> = { "select-all": selectAll, copy, cut, paste, duplicate, delete: deleteSelected };
      if (!f[id]) return false;
      f[id]();
      return true;
    };
    menuHandler = h;
    return () => {
      if (menuHandler === h) menuHandler = null;
    };
  });

  const onKeyDown = (e: React.KeyboardEvent) => {
    // An open menu (the tool menu after T aside) has the keys.
    if (isField(e.target) || (menuOpen() && toolArmed.current <= performance.now())) return;
    const mod = e.ctrlKey || e.metaKey;
    const k = e.key;
    const g = grid();
    let handled = true;
    // After T, a letter picks a tool, as in Logic's tool menu.
    if (toolArmed.current > performance.now() && !mod) {
      const t = TOOLS.find((x) => x.key.toLowerCase() === k.toLowerCase());
      toolArmed.current = 0;
      if (t) {
        setPrefs({ tool: t.id });
        closeMenu();
        e.preventDefault();
        e.stopPropagation();
        return;
      }
    }
    const cmd = commandForEvent(e.nativeEvent);
    if (cmd && selectCommand(cmd)) {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    if (mod && !e.altKey) {
      // ⌘A and ⌘D are native menu accelerators; they arrive through pianoRollMenu().
      switch (k.toLowerCase()) {
        case "c":
          copy();
          break;
        case "x":
          cut();
          break;
        case "v":
          paste();
          break;
        case "m":
          apply(me.toggleMute, "Mute Notes");
          break;
        case "arrowright":
          zoomX(1.4);
          break;
        case "arrowleft":
          zoomX(1 / 1.4);
          break;
        case "arrowdown":
          zoomY(1.2);
          break;
        case "arrowup":
          zoomY(1 / 1.2);
          break;
        default:
          handled = false;
      }
    } else if (!mod) {
      switch (k) {
        case "Delete":
        case "Backspace":
          deleteSelected();
          break;
        case "ArrowLeft":
        case "ArrowRight": {
          const notes = notesNow();
          if (e.altKey) {
            // Select the previous / next note in time.
            const sorted = notes.map((n, i) => [n, i] as const).sort((a, b) => a[0].start - b[0].start || a[0].pitch - b[0].pitch);
            const cur = sorted.findIndex(([n]) => n.sel);
            const next = sorted[Math.max(0, Math.min(sorted.length - 1, cur < 0 ? 0 : cur + (k === "ArrowRight" ? 1 : -1)))];
            if (next) {
              setSelection(notes.map((n, i) => (i === next[1] ? { ...n, sel: true } : n.sel ? { ...n, sel: false } : n)));
              play(next[0].pitch, next[0].velocity);
              window.setTimeout(() => play(null), 200);
            }
            break;
          }
          if (!anySelected()) break;
          const step = e.shiftKey ? g.bar : g.grid;
          apply((n) => musical(n, (v) => me.nudge(v, k === "ArrowRight" ? step : -step, 0)), "Nudge Notes", nudgeKey());
          break;
        }
        case "ArrowUp":
        case "ArrowDown": {
          if (!anySelected()) break;
          const d = (k === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 12 : 1);
          apply((n) => me.nudge(n, 0, d), "Transpose Notes", nudgeKey());
          const first = notesNow().find((n) => n.sel);
          if (first) {
            play(first.pitch, first.velocity);
            window.setTimeout(() => play(null), 200);
          }
          break;
        }
        case "q":
        case "Q":
          quantizeNow();
          break;
        case "t":
        case "T":
          toolArmed.current = performance.now() + 2000;
          if (toolBtn.current) openMenuAt(toolBtn.current, toolItems());
          break;
        case "z":
        case "Z":
          zoomToFit();
          break;
        case "Escape":
          setSelection(me.selectNone(notesNow()), work.current.cc.map((c) => (c.sel ? { ...c, sel: false } : c)));
          break;
        default:
          handled = false;
      }
    } else handled = false;
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  // ---- layout -------------------------------------------------------------------------------

  const v = view();
  const pps = pr.pxPerBeat / samplesPerBeat(project);
  const bar = me.barLength(timeBase(project));
  const lastEnd = Math.max(0, ...project.tracks.flatMap((t) => t.regions.map((r) => r.start + r.length)), project.cycle.end);
  const contentW = Math.max(v.W + 1, Math.ceil((lastEnd + 16 * bar) * pps));
  const contentH = 128 * pr.noteH;
  const selNotes = work.current.notes.filter((n) => n.sel);
  const selVel = selNotes.length ? Math.round(selNotes.reduce((a, n) => a + n.velocity, 0) / selNotes.length) : null;
  const focusRegion = regions.find((r) => r.id === focus);
  const title = focusRegion?.name ?? track.name;
  const hZoom = Math.log(pr.pxPerBeat / 6) / Math.log(1600 / 6);
  const toolClass = `pr-cursor-${pr.tool}`;

  // Velocity slider: previews while dragging, commits on release.
  const velPreview = (val: number) => {
    preview.current = { notes: me.setVelocity(work.current.notes, val), cc: work.current.cc };
    requestDraw();
    bump();
  };
  const velCommit = () => {
    if (preview.current) commit(preview.current.notes, work.current.cc, "Set Velocity", `velocity:${track.id}`);
  };
  const shownVel = preview.current ? (() => {
    const s = preview.current.notes.filter((n) => n.sel);
    return s.length ? Math.round(s.reduce((a, n) => a + n.velocity, 0) / s.length) : selVel;
  })() : selVel;

  return (
    <div className="editor piano-roll" ref={rootRef} tabIndex={-1} onKeyDown={onKeyDown}>
      <div className="local-bar">
        <EditorTabs current="editor" />
        <div className="local-menus">
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, editItems())}>
            Edit
          </button>
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, functionItems())}>
            Functions
          </button>
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, viewItems())}>
            View
          </button>
          <span className="local-title">{title}</span>
        </div>
        <div className="local-right">
          <button ref={toolBtn} className="popup pr-tool" title="Tool (T)" onMouseDown={(e) => openMenuAt(e.currentTarget, toolItems())}>
            <ToolIcon tool={pr.tool} /> <Icon.chevron />
          </button>
          <span className="lb-label">Quantize</span>
          <span className="pr-qgroup">
            <button ref={qBtn} className="popup" title="Time Quantize settings" onMouseDown={() => setQOpen(!qOpen)}>
              {pr.qValue}
              {pr.qSwing ? ` · ${pr.qSwing}%` : ""} <Icon.chevron />
            </button>
            <button className="lb-btn pr-q" title={`Quantize ${selNotes.length ? "selected notes" : "all notes"} (Q)`} onClick={quantizeNow}>
              Q
            </button>
          </span>
          <span className="lb-label">Grid</span>
          <button
            className="popup"
            title="Grid and note length for the Pencil"
            onMouseDown={(e) => openMenuAt(e.currentTarget, me.GRID_VALUES.map((g) => ({ label: g, checked: g === pr.grid, onClick: () => setPrefs({ grid: g }) })))}
          >
            {pr.grid} <Icon.chevron />
          </button>
          <button className={"lb-btn" + (pr.snap ? " on" : "")} title={`Snap to grid (hold ${fmt("Mod")} while dragging to move freely)`} onClick={() => setPrefs({ snap: !pr.snap })}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M3 2v5a4 4 0 0 0 8 0V2M3 4.5h2.4M8.6 4.5H11M5.4 2v5a1.6 1.6 0 0 0 3.2 0V2" />
            </svg>
          </button>
          <span className="lb-label" title="Velocity of the selected notes">
            Vel
          </span>
          <input
            className="zoom pr-vel"
            type="range"
            min={1}
            max={127}
            step={1}
            disabled={shownVel === null}
            value={shownVel ?? 100}
            title="Velocity of the selected notes"
            onChange={(e) => velPreview(parseInt(e.target.value))}
            onPointerUp={velCommit}
            onKeyUp={velCommit}
          />
          <span className="pr-vel-val">{shownVel ?? "–"}</span>
          <button className={"lb-btn" + (pr.midiOut ? " on" : "")} title="MIDI Out: hear notes as you edit them" onClick={() => setPrefs({ midiOut: !pr.midiOut })}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M2 5.2h2.4L7.6 2.5v9L4.4 8.8H2zM9.8 4.6a3.4 3.4 0 0 1 0 4.8M11.4 3a5.6 5.6 0 0 1 0 8" />
            </svg>
          </button>
          {/* The Step Input Keyboard lives in the main window. */}
          {!ownPanel && (
            <button className={"lb-btn" + (showStepInput ? " on" : "")} title={`Step Input Keyboard (${keyHint("step-input")})`} onClick={() => setState({ showStepInput: !showStepInput })}>
              <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
                <path d="M4.5 10.5V2.5h5" />
                <ellipse cx="3" cy="10.5" rx="1.8" ry="1.4" fill="currentColor" stroke="none" />
                <path d="M8 7.5h4.5M10.5 5.5l2 2-2 2" />
              </svg>
            </button>
          )}
          <button className={"lb-btn catch" + (pr.catchPlayhead ? " on" : "")} title="Catch Playhead" onClick={() => setPrefs({ catchPlayhead: !pr.catchPlayhead })}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M3 2v10M3 7h8M8 4l3 3-3 3" />
            </svg>
          </button>
          <span className="lb-label" title="Row height">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M6 1v10M3.5 3.5 6 1l2.5 2.5M3.5 8.5 6 11l2.5-2.5" />
            </svg>
          </span>
          <input className="zoom vzoom" type="range" min={5} max={32} step={0.5} value={pr.noteH} title={`Row height (${fmt("Alt")}-scroll, ${fmt("Mod+ArrowUp")} / ${fmt("Mod+ArrowDown")})`} onChange={(e) => zoomY(parseFloat(e.target.value) / pr.noteH)} />
          <span className="lb-label" title="Horizontal zoom">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M1 6h10M3.5 3.5 1 6l2.5 2.5M8.5 3.5 11 6 8.5 8.5" />
            </svg>
          </span>
          <input
            className="zoom"
            type="range"
            min={0}
            max={1}
            step={0.001}
            value={hZoom}
            title={`Horizontal zoom (${fmt("Mod")}-scroll, ${fmt("Mod+ArrowLeft")} / ${fmt("Mod+ArrowRight")})`}
            onChange={(e) => zoomX((6 * Math.pow(1600 / 6, parseFloat(e.target.value))) / pr.pxPerBeat)}
          />
          <DetachButton panel="editor" />
        </div>
      </div>
      {qOpen && (
        <QuantizePanel
          anchor={qBtn.current}
          prefs={pr}
          onClose={() => setQOpen(false)}
          onQuantize={() => {
            quantizeNow();
            setQOpen(false);
          }}
        />
      )}
      <div className="pr-body" style={{ gridTemplateColumns: `${KEY_W}px 1fr`, gridTemplateRows: `${PR_RULER_H}px 1fr ${pr.showLane ? `5px ${pr.laneH}px` : "0 0"}` }}>
        <div className="pr-corner" />
        <div className="pr-ruler">
          <canvas ref={rulerCv} onMouseDown={onRulerDown} />
          <div className="pr-playhead" ref={playheads[0]} />
        </div>
        <div className="pr-keys">
          <canvas
            ref={keysCv}
            onMouseDown={onKeysDown}
            onMouseLeave={() => {
              hoverPitch.current = null;
              requestDraw();
            }}
          />
        </div>
        <div className="pr-notes">
          <canvas ref={notesCv} />
          <div className="pr-playhead" ref={playheads[1]} />
          {!regions.length && <div className="pr-hint">No MIDI regions on this track yet. Double-click or draw with the Pencil to add notes.</div>}
          <div
            ref={scrollRef}
            className={"pr-scroll " + toolClass}
            onScroll={requestDraw}
            onMouseDown={onNotesDown}
            onMouseMove={onNotesHover}
            onContextMenu={onNotesContext}
            onMouseLeave={() => {
              hoverPitch.current = null;
              requestDraw();
            }}
          >
            <div style={{ width: contentW, height: contentH }} />
          </div>
          <div className="pr-tag" ref={tagRef} />
        </div>
        {pr.showLane && (
          <>
            <div className="pr-split" onMouseDown={onSplitDown} title="Drag to resize the lane" />
            <div className="pr-lane-head">
              <canvas ref={scaleCv} />
              <button className="pr-lane-pick" title="Choose what the lane shows" onMouseDown={(e) => openMenuAt(e.currentTarget, laneItems())}>
                {laneShort(pr.lane)} <Icon.chevron />
              </button>
            </div>
            <div className="pr-lane" ref={laneWrap}>
              <canvas
                ref={laneCv}
                className={toolClass}
                onMouseDown={onLaneDown}
                onContextMenu={(e) => {
                  // A controller lane's right-click menu: the curve settings and ramp functions.
                  e.preventDefault();
                  const items = curveItems().slice(1);
                  if (items.length) openMenu(e.clientX, e.clientY, items);
                }}
              />
              <div className="pr-playhead" ref={playheads[2]} />
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/** Logic's Time Quantize settings: value, strength, swing and whether ends are quantized. */
function QuantizePanel({ anchor, prefs: p, onClose, onQuantize }: { anchor: HTMLElement | null; prefs: Prefs; onClose: () => void; onQuantize: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const down = (e: MouseEvent) => {
      if (ref.current?.contains(e.target as Node) || anchor?.contains(e.target as Node)) return;
      onClose();
    };
    // Esc closes just the panel: capture phase, ahead of the editor's Esc (deselect all).
    const key = (e: KeyboardEvent) => {
      // A menu over the panel takes the Esc first (it marks it handled).
      if (e.key !== "Escape" || e.defaultPrevented || menuOpen()) return;
      e.stopPropagation();
      onClose();
    };
    window.addEventListener("mousedown", down);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("mousedown", down);
      window.removeEventListener("keydown", key, true);
    };
  }, [anchor, onClose]);
  const r = anchor?.getBoundingClientRect();
  const left = Math.min((r?.left ?? 0) - 40, window.innerWidth - 270);
  return (
    <div className="pr-qpanel" ref={ref} style={{ left: Math.max(4, left), top: (r?.bottom ?? 0) + 4 }}>
      <div className="pr-qtitle">Time Quantize</div>
      <div className="pr-qgrid">
        {me.GRID_VALUES.map((g) => (
          <button key={g} className={"pr-qval" + (g === p.qValue ? " on" : "")} onClick={() => setPrefs({ qValue: g })}>
            {g}
          </button>
        ))}
      </div>
      <label className="pr-qrow">
        <span>Strength</span>
        <input type="range" min={0} max={100} value={p.qStrength} onChange={(e) => setPrefs({ qStrength: parseInt(e.target.value) })} />
        <span className="pr-qnum">{p.qStrength}%</span>
      </label>
      <label className="pr-qrow">
        <span>Swing</span>
        <input type="range" min={0} max={100} value={p.qSwing} onChange={(e) => setPrefs({ qSwing: parseInt(e.target.value) })} />
        <span className="pr-qnum">{p.qSwing}%</span>
      </label>
      <label className="pr-qcheck">
        <input type="checkbox" checked={p.qEnds} onChange={(e) => setPrefs({ qEnds: e.target.checked })} />
        Quantize note ends
      </label>
      <div className="pr-qfoot">
        <button className="pr-qbtn" onClick={onQuantize}>
          Quantize
        </button>
      </div>
    </div>
  );
}
