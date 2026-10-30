// The Event List (D): Logic's list editor for MIDI regions. Each note and controller event of
// the selected regions is a row; every value can be dragged up or down, or double-clicked and
// typed. The maths lives in eventList.ts. Edits work on the Piano Roll's working list
// (midiEdit.ts) and go back with `set_midi_content`, one undo step each; a drag previews and
// sends its result on release. The note selection is the store's `midiSelection`, which the
// Piano Roll shares.
//
// Only the rows in view are rendered, so a region with thousands of events scrolls freely.
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useLayoutEffect, useReducer, useRef, useState, useSyncExternalStore } from "react";
import { act, actMade, type Id, type Project, type Region, type Track } from "../api";
import { detachMenuItems, ownPanel } from "../detach";
import * as el from "../eventList";
import { framePos, onFrame } from "../frame";
import { Icon } from "../icons";
import { commandForEvent, fmt } from "../keymap";
import { isEditorSelect, selectEvents } from "../noteSelect";
import * as me from "../midiEdit";
import { currentTick, getState, setState, useStore, type MidiSelection, type State } from "../store";
import { barLines, tempoMap } from "../tempo";
import { DetachButton } from "./DetachButton";
import { openMenuAt, type MenuItem } from "./Menu";

/** Row height in CSS pixels (the rows are laid out at multiples of it). */
const ROW_H = 18;
/** Pixels of vertical drag per step of a value. */
const DRAG_PX = 2;

// ---- preferences ----------------------------------------------------------------------------

interface Prefs {
  /** Event types shown (the filter buttons). */
  show: Record<el.EventType, boolean>;
  /** What + adds. */
  addType: el.EventType;
  catchPlayhead: boolean;
  /** Play notes when they are clicked or their pitch or velocity is changed. */
  midiOut: boolean;
}

const PREFS_KEY = "fermata.eventList";
const DEFAULT_PREFS: Prefs = { show: { note: true, bend: true, control: true, pressure: true }, addType: "note", catchPlayhead: true, midiOut: true };

let prefs: Prefs = (() => {
  try {
    const p = JSON.parse(localStorage.getItem(PREFS_KEY) ?? "{}");
    return { ...DEFAULT_PREFS, ...p, show: { ...DEFAULT_PREFS.show, ...p.show } };
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

const usePrefs = () =>
  useSyncExternalStore(
    (f) => {
      prefSubs.add(f);
      return () => prefSubs.delete(f);
    },
    () => prefs,
  );

// ---- what is listed -------------------------------------------------------------------------

// A pattern region plays its steps, not its notes: the Step Sequencer edits it.
const isMidi = (r: Region) => r.content.type === "midi" && !r.pattern;

interface Scope {
  track: Track;
  regions: Region[];
  /** Every MIDI region of the track (none was selected). */
  all: boolean;
}

/** The selected MIDI regions (on the track of the first), or else every MIDI region of the
 *  selected instrument track. */
function scopeOf(project: Project | undefined, selected: Id[], selectedTrack: Id | null): Scope | null {
  for (const t of project?.tracks ?? []) {
    const regions = t.regions.filter((r) => isMidi(r) && selected.includes(r.id));
    if (regions.length) return { track: t, regions, all: false };
  }
  const track = project?.tracks.find((t) => t.id === selectedTrack);
  return track?.kind === "instrument" ? { track, regions: track.regions.filter(isMidi), all: true } : null;
}

/** What the list shows, for its window's title. */
export function eventListTitle(s: State): string {
  const sc = scopeOf(s.snap?.project, s.selectedRegions, s.snap?.selected ?? null);
  const name = sc && (sc.regions.length === 1 && !sc.all ? sc.regions[0].name : sc.track.name);
  return name ? `Event List — ${name}` : "Event List";
}

// ---- the editors' switch and the menu bar ---------------------------------------------------

/** Piano Roll / Event List / Step Sequencer, on the MIDI editors' bars. A panel's own window
 *  keeps its editor. */
export function EditorTabs({ current }: { current: "editor" | "list" | "steps" }) {
  if (ownPanel) return null;
  return (
    <div className="editor-tabs">
      <button className={"filter" + (current === "editor" ? " on" : "")} onClick={() => setState({ bottom: "editor" })}>
        Piano Roll
      </button>
      <button className={"filter" + (current === "list" ? " on" : "")} onClick={() => setState({ bottom: "list" })}>
        Event List
      </button>
      <button className={"filter" + (current === "steps" ? " on" : "")} onClick={() => setState({ bottom: "steps" })}>
        Step Sequencer
      </button>
    </div>
  );
}

let menuHandler: ((id: string) => boolean) | null = null;

/** Edit menu commands (Select All, Delete) for the Event List while it has focus. */
export function eventListMenu(id: string): boolean {
  return menuHandler?.(id) ?? false;
}

// ---- the list -------------------------------------------------------------------------------

interface Work {
  /** The track's regions this was built from (identity), the listed region ids, and the
   *  selection it was built with. */
  source: Region[];
  key: string;
  sel: MidiSelection | null;
  /** The listed regions as they are after our last edit (ahead of the snapshot until it comes). */
  regions: Region[];
  lists: el.Lists;
}

interface Editing {
  key: string;
  field: el.Field;
  text: string;
}

const EMPTY: el.Lists = { notes: [], cc: [] };

export function EventList() {
  const project = useStore((s) => s.snap?.project);
  const selectedRegions = useStore((s) => s.selectedRegions);
  const selectedTrack = useStore((s) => s.snap?.selected ?? null);
  const midiSel = useStore((s) => s.midiSelection);
  const p = usePrefs();
  const [, bump] = useReducer((x: number) => x + 1, 0);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewH, setViewH] = useState(200);
  const [playIdx, setPlayIdx] = useState(-1);

  const scope = scopeOf(project, selectedRegions, selectedTrack);
  const scopeKey = scope ? `${scope.track.id}:${scope.regions.map((r) => r.id).join(",")}` : "";

  // The working list follows the snapshot and the shared selection; edits replace it ahead of
  // the next snapshot, which isn't used until every edit sent has been answered.
  const work = useRef<Work>({ source: [], key: "", sel: null, regions: [], lists: EMPTY });
  const inflight = useRef(0);
  const rebuild = (sc: Scope | null, key: string, sel: MidiSelection) => {
    const w = sc ? me.fromRegions(sc.regions, new Set(sel.notes), new Set(sel.cc)) : EMPTY;
    work.current = { source: sc?.track.regions ?? [], key, sel, regions: sc?.regions ?? [], lists: w };
  };
  const wk = work.current;
  if (inflight.current === 0 && (wk.source !== (scope?.track.regions ?? wk.source) || wk.key !== scopeKey || wk.sel !== midiSel)) rebuild(scope, scopeKey, midiSel);

  /** What a drag in progress makes the list, shown in its place. The rows keep their order
   *  until it is sent. */
  const preview = useRef<el.Lists | null>(null);
  const anchor = useRef<number | null>(null);
  const reveal = useRef(false);
  // The cell being typed in; the ref stops Enter and the blur that follows both committing.
  const editRef = useRef<Editing | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const rowCache = useRef<{ lists: el.Lists; show: Prefs["show"]; rows: el.Row[] } | null>(null);
  const listed = work.current.lists;
  if (rowCache.current?.lists !== listed || rowCache.current.show !== p.show) rowCache.current = { lists: listed, show: p.show, rows: el.buildRows(listed, (t) => p.show[t]) };
  const rows = rowCache.current.rows;
  const shown = preview.current ?? listed;

  const live = useRef({ project, scope, rows });
  live.current = { project, scope, rows };

  // ---- selection and edits ----------------------------------------------------------------

  /** Shares the selection: this list's keys replace those of the listed regions. */
  const publish = (lists: el.Lists) => {
    const ids = new Set(work.current.regions.map((r) => r.id));
    const keep = (k: string) => !ids.has(Number(k.split(":")[0]));
    const prev = getState().midiSelection;
    const sel = { notes: [...prev.notes.filter(keep), ...me.selectionKeys(lists.notes)], cc: [...prev.cc.filter(keep), ...me.selectionKeys(lists.cc)] };
    work.current = { ...work.current, sel };
    setState({ midiSelection: sel });
  };

  const setSelection = (lists: el.Lists) => {
    work.current = { ...work.current, lists };
    publish(lists);
    bump();
  };

  /** Sends edited lists as one undo step (`key` joins it to the step with that key). */
  const commit = (lists: el.Lists, label: string, key?: string) => {
    preview.current = null;
    const pr = live.current.project;
    if (!pr) return;
    const { edits, selNotes, selCc } = me.toEdits(lists.notes, lists.cc, work.current.regions, barLines(tempoMap(pr)));
    if (!edits.length) {
      setSelection(lists);
      return;
    }
    const next = me.applyEdits(work.current.regions, edits);
    const w = me.fromRegions(next, selNotes, selCc);
    work.current = { ...work.current, regions: next, lists: w };
    publish(w);
    reveal.current = true;
    bump();
    inflight.current++;
    act("set_midi_content", { edits, label, key: key ?? null }).finally(() => {
      if (--inflight.current === 0) {
        // Take the snapshot (and anything it refused) from here on.
        work.current = { ...work.current, source: [] };
        bump();
      }
    });
  };

  const selectRow = (i: number, mods: { toggle: boolean; range: boolean }) => {
    const l = work.current.lists;
    const keys = el.clickSelect(live.current.rows, el.selectedKeys(l), i, anchor.current, mods);
    if (!mods.range) anchor.current = i;
    setSelection(el.selectKeys(l, keys));
  };

  const selectAll = () => setSelection(el.selectKeys(work.current.lists, new Set(live.current.rows.map((r) => r.key))));
  const selectNone = () => setSelection(el.selectKeys(work.current.lists, new Set()));
  const anySelected = () => live.current.rows.some((r) => el.isSelected(work.current.lists, r));
  /** Edit › Select commands (Invert Selection, All Following…) act on the events listed while
   *  the Event List has focus, as in Logic; false for those that stay with the regions. */
  const selectCommand = (id: string): boolean => {
    if (!isEditorSelect(id)) return false;
    const l = work.current.lists;
    const rows = live.current.rows;
    const items = rows.map((r) => {
      if (r.note) {
        const n = l.notes[r.idx];
        return { start: r.start, sel: !!n.sel, equal: `n${n.pitch}`, velocity: n.velocity, muted: n.muted };
      }
      const c = l.cc[r.idx];
      return { start: r.start, sel: !!c.sel, equal: `c${JSON.stringify(c.kind)}:${c.value}` };
    });
    const c = live.current.project?.cycle;
    const sel = selectEvents(id, items, { playhead: currentTick()?.pos ?? 0, locators: c && c.end > c.start ? c : null });
    if (!sel) return false;
    setSelection(el.selectKeys(l, new Set(rows.filter((_, i) => sel[i]).map((r) => r.key))));
    return true;
  };
  /** Deletes the selected events shown (not those of types filtered out). */
  const deleteEvents = () => {
    const l = work.current.lists;
    const keys = new Set(live.current.rows.filter((r) => el.isSelected(l, r)).map((r) => r.key));
    if (keys.size) commit(el.deleteSelected(el.selectKeys(l, keys)), "Delete Events");
  };

  // ---- sound ------------------------------------------------------------------------------

  const sounding = useRef<number | null>(null);
  const sound = (pitch: number | null, velocity = 100) => {
    if (sounding.current !== null) invoke("midi_event", { data: [0x80, sounding.current, 0] }).catch(() => {});
    sounding.current = null;
    const t = live.current.scope?.track;
    if (pitch === null || !prefs.midiOut || !t) return;
    // Live MIDI plays on the selected track.
    if (getState().snap?.selected !== t.id) act("select_track", { id: t.id });
    sounding.current = pitch;
    invoke("midi_event", { data: [0x90, pitch, Math.max(1, Math.min(127, velocity))] }).catch(() => {});
  };
  useEffect(() => () => sound(null), []);

  // ---- new events -------------------------------------------------------------------------

  /** The listed region an event at `t` goes in: one holding it, else one within a bar, else
   *  (with regions selected) the nearest, which grows. Null when a region must be made. */
  const regionFor = (t: number, pr: Project): Id | null => {
    const regs = work.current.regions;
    const covering = regs.find((r) => t >= r.start && t < r.start + r.length);
    if (covering) return covering.id;
    const dist = (r: Region) => Math.min(Math.abs(t - r.start), Math.abs(t - (r.start + r.length)));
    const near = [...regs].sort((a, b) => dist(a) - dist(b))[0];
    if (!near) return null;
    const m = tempoMap(pr);
    return dist(near) <= m.samplesPerBarAt(t) || !live.current.scope?.all ? near.id : null;
  };

  const addEvent = async (type: el.EventType = prefs.addType) => {
    const { project: pr, scope: sc } = live.current;
    if (!pr || !sc || sc.track.kind !== "instrument") return;
    const m = tempoMap(pr);
    const at = Math.max(0, Math.round(framePos()));
    // Logic's Create copies the selected event of that type, if any.
    const l0 = work.current.lists;
    const model = live.current.rows.find((r) => r.type === type && el.isSelected(l0, r));
    const like = model ? (model.note ? l0.notes[model.idx] : l0.cc[model.idx]) : null;
    let region = regionFor(at, pr);
    let key: string | undefined;
    if (region === null) {
      // A one-bar region at the playhead, in the same undo step as the event.
      const bar = m.barAt(at);
      const start = Math.round(m.barStart(bar));
      key = `new-region:${Date.now()}`;
      const ids = await actMade("create_midi_region", { track: sc.track.id, start, length: Math.round(m.barStart(bar + 1)) - start, key });
      if (!ids?.length) return;
      setState({ selectedRegions: ids, focus: "regions" });
      const s = getState();
      const next = scopeOf(s.snap?.project, s.selectedRegions, s.snap?.selected ?? null);
      rebuild(next, next ? `${next.track.id}:${next.regions.map((r) => r.id).join(",")}` : "", s.midiSelection);
      region = ids[0];
    }
    const ev = el.newEvent(m, type, at, region, like);
    const base = el.selectKeys(work.current.lists, new Set());
    if (!prefs.show[type]) setPrefs({ show: { ...prefs.show, [type]: true } });
    commit({ notes: ev.note ? [...base.notes, ev.note] : base.notes, cc: ev.cc ? [...base.cc, ev.cc] : base.cc }, `Create ${typeName(type)}`, key);
  };

  // ---- pointer ----------------------------------------------------------------------------

  const focusList = () => rootRef.current?.focus({ preventScroll: true });

  const onRowDown = (e: React.MouseEvent, i: number) => {
    if (e.button !== 0) return;
    // A click while a value is being typed just ends that (focus leaving the field commits it).
    const typing = !!editRef.current;
    focusList();
    if (typing) return;
    selectRow(i, { toggle: e.metaKey || e.ctrlKey, range: e.shiftKey });
    const r = live.current.rows[i];
    if (r?.note && !e.shiftKey && !e.metaKey && !e.ctrlKey) {
      const n = work.current.lists.notes[r.idx];
      sound(n.pitch, n.velocity);
      window.addEventListener("mouseup", () => sound(null), { once: true });
    }
  };

  /** A press on a value: selects its row, then a vertical drag changes the value of every
   *  selected event of that type by the same amount. */
  const onCellDown = (e: React.MouseEvent, i: number, field: el.Field, part = 3) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const typing = !!editRef.current;
    focusList();
    if (typing) return;
    const toggle = e.metaKey || e.ctrlKey;
    if (toggle || e.shiftKey) {
      selectRow(i, { toggle, range: e.shiftKey });
      return;
    }
    const all = live.current.rows;
    const row = all[i];
    let lists = work.current.lists;
    const wasSelected = el.isSelected(lists, row);
    if (!wasSelected) {
      lists = el.selectKeys(lists, new Set([row.key]));
      anchor.current = i;
      setSelection(lists);
    }
    const pr = live.current.project;
    if (!pr || !el.hasField(row.type, field)) return;
    const m = tempoMap(pr);
    const targets = all.filter((r) => r.type === row.type && el.isSelected(lists, r));
    const y0 = e.clientY;
    let steps = 0;
    let moved = false;
    const hear = (l: el.Lists) => {
      if (row.note) sound(l.notes[row.idx].pitch, l.notes[row.idx].velocity);
    };
    if (row.note && (field === "num" || field === "val")) hear(lists);
    const move = (ev: MouseEvent) => {
      const s = Math.trunc((y0 - ev.clientY) / DRAG_PX);
      if (s === steps) return;
      steps = s;
      moved = true;
      preview.current = s ? el.applyChange(m, lists, targets, { field, part, delta: s }) : null;
      if (row.note && field === "num") hear(preview.current ?? lists);
      bump();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      sound(null);
      const result = preview.current;
      preview.current = null;
      if (result && steps) commit(result, el.changeLabel(row.type, field));
      // A click on one of several selected rows selects just it.
      else if (!moved && wasSelected && targets.length > 1) {
        anchor.current = i;
        setSelection(el.selectKeys(lists, new Set([row.key])));
      } else bump();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const startEdit = (i: number, field: el.Field) => {
    const pr = live.current.project;
    const row = live.current.rows[i];
    if (!pr || !row || !el.hasField(row.type, field)) return;
    editRef.current = { key: row.key, field, text: el.cellText(tempoMap(pr), work.current.lists, row, field) };
    setEditing(editRef.current);
  };

  const finishEdit = (text: string | null) => {
    const ed = editRef.current;
    editRef.current = null;
    setEditing(null);
    if (!ed) return;
    focusList();
    const pr = live.current.project;
    const row = live.current.rows.find((r) => r.key === ed?.key);
    if (text === null || !pr || !row) return;
    const m = tempoMap(pr);
    const l = work.current.lists;
    if (text.trim() === el.cellText(m, l, row, ed.field)) return;
    const ch = el.parseCell(m, l, row, ed.field, text);
    if (!ch) {
      setState({ toast: { text: `“${text}” isn't a ${ed.field === "pos" ? "position" : ed.field === "len" ? "length" : row.note && ed.field === "num" ? "note" : "value"}`, at: Date.now(), ms: 2000 } });
      return;
    }
    commit(el.applyChange(m, l, [row], ch), el.changeLabel(row.type, ed.field));
  };

  // ---- keys and the menu bar --------------------------------------------------------------

  const onKeyDown = (e: React.KeyboardEvent) => {
    const tgt = e.target as HTMLElement;
    if (tgt.tagName === "INPUT") return;
    const cmd = commandForEvent(e.nativeEvent);
    if (cmd && selectCommand(cmd)) {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    const mod = e.ctrlKey || e.metaKey;
    let handled = true;
    if (mod || e.altKey) handled = false;
    else if (e.key === "Delete" || e.key === "Backspace") deleteEvents();
    else if (e.key === "Escape") selectNone();
    else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      const rs = live.current.rows;
      const l = work.current.lists;
      const sel = rs.map((r, i) => (el.isSelected(l, r) ? i : -1)).filter((i) => i >= 0);
      const d = e.key === "ArrowUp" ? -1 : 1;
      const i = sel.length ? Math.max(0, Math.min(rs.length - 1, (d < 0 ? sel[0] : sel[sel.length - 1]) + d)) : d < 0 ? rs.length - 1 : 0;
      if (rs.length) {
        if (e.shiftKey) setSelection(el.selectKeys(l, new Set([...el.selectedKeys(l), rs[i].key])));
        else {
          anchor.current = i;
          setSelection(el.selectKeys(l, new Set([rs[i].key])));
        }
        reveal.current = true;
      }
    } else if (e.key === "Enter") {
      const rs = live.current.rows;
      const i = rs.findIndex((r) => el.isSelected(work.current.lists, r));
      if (i >= 0) startEdit(i, "pos");
    } else handled = false;
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  useEffect(() => {
    const h = (id: string) => {
      if (!rootRef.current?.contains(document.activeElement)) return false;
      if (selectCommand(id)) return true;
      const f: Record<string, () => void> = { "select-all": selectAll, delete: deleteEvents };
      if (!f[id]) return false;
      f[id]();
      return true;
    };
    menuHandler = h;
    return () => {
      if (menuHandler === h) menuHandler = null;
    };
  });

  const typeName = (t: el.EventType) => el.EVENT_TYPES.find((x) => x.id === t)!.name;
  const typeItems = (): MenuItem[] => el.EVENT_TYPES.map((t) => ({ label: typeName(t.id), checked: prefs.addType === t.id, onClick: () => setPrefs({ addType: t.id }) }));
  const editItems = (): MenuItem[] => [
    { label: "Select All", hint: fmt("Mod+A"), onClick: selectAll },
    { label: "Deselect All", hint: "Esc", onClick: selectNone },
    { separator: true },
    { label: `Add ${typeName(prefs.addType)} at Playhead`, onClick: () => addEvent() },
    { label: "Delete", hint: fmt("Backspace"), disabled: !anySelected(), onClick: deleteEvents },
  ];
  const viewItems = (): MenuItem[] => [
    ...el.EVENT_TYPES.map((t) => ({ label: t.label, checked: prefs.show[t.id], onClick: () => setPrefs({ show: { ...prefs.show, [t.id]: !prefs.show[t.id] } }) })),
    { separator: true },
    { label: "Catch Playhead", checked: prefs.catchPlayhead, onClick: () => setPrefs({ catchPlayhead: !prefs.catchPlayhead }) },
    { label: "MIDI Out (Hear Edited Notes)", checked: prefs.midiOut, onClick: () => setPrefs({ midiOut: !prefs.midiOut }) },
    ...detachMenuItems("list"),
  ];

  // ---- scrolling and the playhead ---------------------------------------------------------

  const scrollToRow = (i: number) => {
    const box = scrollRef.current;
    if (!box || i < 0) return;
    const top = i * ROW_H;
    if (top < box.scrollTop || top + ROW_H > box.scrollTop + box.clientHeight) box.scrollTop = Math.max(0, top - box.clientHeight / 3);
  };

  useLayoutEffect(() => {
    if (!reveal.current) return;
    reveal.current = false;
    const l = work.current.lists;
    scrollToRow(rows.findIndex((r) => el.isSelected(l, r)));
  });

  // Back to the top for another region or track.
  useLayoutEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
    anchor.current = null;
  }, [scopeKey]);

  useEffect(() => {
    const sc = scrollRef.current;
    if (!sc) return;
    const ro = new ResizeObserver(() => setViewH(sc.clientHeight));
    ro.observe(sc);
    let last = -2;
    let lastRows: el.Row[] = [];
    const off = onFrame((now) => {
      const i = el.rowAt(live.current.rows, framePos(now));
      if (i === last && live.current.rows === lastRows) return;
      last = i;
      lastRows = live.current.rows;
      setPlayIdx(i);
      if (currentTick()?.playing && prefs.catchPlayhead) scrollToRow(i);
    });
    return () => {
      ro.disconnect();
      off();
    };
  }, []);

  // ---- layout -------------------------------------------------------------------------------

  const title = scope ? (scope.regions.length === 1 && !scope.all ? scope.regions[0].name : scope.track.name) : "";
  const map = project ? tempoMap(project) : null;
  const first = Math.max(0, Math.floor(scrollTop / ROW_H) - 4);
  const last = Math.min(rows.length, Math.ceil((scrollTop + viewH) / ROW_H) + 4);
  const hidden = el.EVENT_TYPES.some((t) => !p.show[t.id]);

  /** A position or length as four draggable parts. */
  const parts = (b: { bar: number; beat: number; div: number; tick: number }, i: number, field: "pos" | "len") =>
    [b.bar, b.beat, b.div, b.tick].map((v, k) => (
      <span key={k} className={`el-part p${k}`} onMouseDown={(e) => onCellDown(e, i, field, k)} onDoubleClick={() => startEdit(i, field)}>
        {v}
      </span>
    ));

  const cell = (i: number, field: el.Field, cls: string, content: React.ReactNode) => {
    const r = rows[i];
    if (editing?.key === r.key && editing.field === field)
      return (
        <span className={`el-cell ${cls}`}>
          <input
            className="el-input"
            autoFocus
            defaultValue={editing.text}
            onFocus={(e) => e.currentTarget.select()}
            onMouseDown={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") finishEdit(e.currentTarget.value);
              else if (e.key === "Escape") finishEdit(null);
            }}
            onBlur={(e) => finishEdit(e.currentTarget.value)}
          />
        </span>
      );
    if (field === "pos" || field === "len") return <span className={`el-cell ${cls} el-edit`}>{content}</span>;
    return (
      <span className={`el-cell ${cls} el-edit`} onMouseDown={(e) => onCellDown(e, i, field)} onDoubleClick={() => startEdit(i, field)}>
        {content}
      </span>
    );
  };

  const rowView = (i: number) => {
    const r = rows[i];
    if (!map) return null;
    const n = r.note ? shown.notes[r.idx] : null;
    const c = r.note ? null : shown.cc[r.idx];
    if (!n && !c) return null;
    const start = n?.start ?? c!.start;
    let info: React.ReactNode = null;
    if (n) info = cell(i, "len", "el-len", parts(el.lengthBbt(map, n.start, n.length), i, "len"));
    else if (c!.kind.type === "controller") info = <span className="el-cell el-info">{el.ccName(c!.kind.cc)}</span>;
    else if (c!.kind.type === "pitch_bend") info = <span className="el-cell el-info">{`${c!.value >= 8192 ? "+" : "−"}${Math.abs(c!.value - 8192)}`}</span>;
    else info = <span className="el-cell el-info" />;
    const cls = ["el-row", i % 2 ? "odd" : "", n?.sel || c?.sel ? "sel" : "", i === playIdx ? "playing" : "", n?.muted ? "muted" : ""].filter(Boolean).join(" ");
    return (
      <div key={r.key} className={cls} style={{ transform: `translateY(${i * ROW_H}px)` }} onMouseDown={(e) => onRowDown(e, i)}>
        {cell(i, "pos", "el-pos", parts(el.positionBbt(map, start), i, "pos"))}
        <span className="el-cell el-status">{el.EVENT_TYPES.find((t) => t.id === r.type)!.status}</span>
        <span className="el-cell el-ch">1</span>
        {el.hasField(r.type, "num") ? cell(i, "num", "el-num", r.note ? el.noteName(el.numOf(shown, r)) : el.numOf(shown, r)) : <span className="el-cell el-num" />}
        {cell(i, "val", "el-val", el.valOf(shown, r))}
        {info}
      </div>
    );
  };

  let empty: string | null = null;
  if (!scope) empty = "Select a MIDI region or a software instrument track to list its events.";
  else if (!scope.regions.length) empty = `No MIDI regions on this track yet. Click + to add an event at the playhead.`;
  else if (!rows.length) empty = hidden && listed.notes.length + listed.cc.length ? "No events of the types shown." : "No events. Click + to add one at the playhead.";

  return (
    <div className="editor event-list" ref={rootRef} tabIndex={-1} onKeyDown={onKeyDown}>
      <div className="local-bar">
        <EditorTabs current="list" />
        <div className="local-menus">
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, editItems())}>
            Edit
          </button>
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, viewItems())}>
            View
          </button>
          <span className="local-title">{title}</span>
        </div>
        <div className="local-right">
          <button className={"lb-btn" + (p.midiOut ? " on" : "")} title="MIDI Out (hear edited notes)" onClick={() => setPrefs({ midiOut: !p.midiOut })}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round">
              <path d="M2 5.2h2.4L7.6 2.5v9L4.4 8.8H2zM9.8 4.6a3.4 3.4 0 0 1 0 4.8M11.4 3a5.6 5.6 0 0 1 0 8" />
            </svg>
          </button>
          <button className={"lb-btn catch" + (p.catchPlayhead ? " on" : "")} title="Catch Playhead" onClick={() => setPrefs({ catchPlayhead: !p.catchPlayhead })}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M3 2v10M3 7h8M8 4l3 3-3 3" />
            </svg>
          </button>
          <DetachButton panel="list" />
        </div>
      </div>
      <div className="el-tools">
        <button className="lb-btn" title={`Add ${typeName(p.addType)} at Playhead`} disabled={!scope} onClick={() => addEvent()}>
          <Icon.plus />
        </button>
        <button className="popup el-add-type" title="What + adds" onMouseDown={(e) => openMenuAt(e.currentTarget, typeItems())}>
          {typeName(p.addType)} <Icon.chevron />
        </button>
        <div className="el-filters">
          {el.EVENT_TYPES.map((t) => (
            <button key={t.id} className={"filter" + (p.show[t.id] ? " on" : "")} title={`Show ${t.label}`} onClick={() => setPrefs({ show: { ...p.show, [t.id]: !p.show[t.id] } })}>
              {t.label}
            </button>
          ))}
        </div>
        <span className="el-count">{rows.length === 1 ? "1 event" : `${rows.length} events`}</span>
      </div>
      <div className="el-row el-head">
        <span className="el-cell el-pos">Position</span>
        <span className="el-cell el-status">Status</span>
        <span className="el-cell el-ch">Ch</span>
        <span className="el-cell el-num">Num</span>
        <span className="el-cell el-val">Val</span>
        <span className="el-cell el-len">Length/Info</span>
      </div>
      <div className="el-scroll" ref={scrollRef} onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}>
        {empty ? (
          <div className="editor-empty">{empty}</div>
        ) : (
          <div className="el-space" style={{ height: rows.length * ROW_H }}>
            {Array.from({ length: Math.max(0, last - first) }, (_, k) => rowView(first + k))}
          </div>
        )}
      </div>
    </div>
  );
}
