// The Tracks area's tools at work: the Tool menu (T), what a press with each tool does, the
// marquee's commands (Delete, Cut, Copy, Paste and the Pointer's click inside it), the tool
// cursors and what the tools draw over the lanes. The tool list and the geometry are in
// tools.ts and marquee.ts; Arrange.tsx hooks these in.
import { act, actMade, call, type Id, type Project, type Region, type Track } from "./api";
import { getState, setState } from "./store";
import { closeMenu, menuOpen, openMenu, type MenuItem } from "./components/Menu";
import { IS_MAC } from "./keyFormat";
import { equalPieces, fadeToolEdit, nearerEnd, toolFor, toolForKey, TRACK_TOOLS, type FadeEdit, type FadeRegion, type TrackTool } from "./tools";
import { dragMarquee, inMarquee, marqueeRows, type Marquee } from "./marquee";
import { snapPos } from "./time";
import { tempoMap } from "./tempo";
import { crossfadesByRegion, followingRegion } from "./fades";
import { regionEnd } from "./loops";
import { refuseFrozen } from "./freeze";
import { indexAt, type Layout } from "./layout";
import { theme } from "./theme";
import { deleteRegions, joinRegions, linkedRegions, pasteRegions, toggleRegionMute } from "./ops";

// ---- choosing tools -------------------------------------------------------------------------

export function setTool(which: "left" | "cmd", tool: TrackTool) {
  setState((s) => ({ tools: { ...s.tools, [which]: tool } }));
}

/** The Left-click or Command-click Tool pop-up's menu. */
export function toolMenuItems(which: "left" | "cmd"): MenuItem[] {
  const cur = getState().tools[which];
  return TRACK_TOOLS.map((t) => ({ label: t.label, hint: t.key, checked: cur === t.id, onClick: () => setTool(which, t.id) }));
}

/** The tool a press or hover uses: the Command-click Tool while ⌘ (Ctrl elsewhere) is held. */
export function activeTool(e: { ctrlKey: boolean; metaKey: boolean }): TrackTool {
  return toolFor(getState().tools, IS_MAC ? e.metaKey : e.ctrlKey || e.metaKey);
}

/** Where the pointer last was over the tracks, for the Tool menu; null once it left them. */
let pointer: { x: number; y: number } | null = null;

export function notePointer(at: { x: number; y: number } | null) {
  pointer = at;
}

/**
 * T: Logic's Tool menu, at the pointer when it is over the tracks (otherwise under the
 * Left-click Tool pop-up). While it is open a tool's key makes that tool the Left-click Tool,
 * and T again or Esc goes back to the Pointer; other letters do nothing, so the Tracks area's
 * own key commands stay out of the way.
 */
export function openToolMenu() {
  const anchor = document.querySelector(".tool-popup.left")?.getBoundingClientRect();
  const at = pointer ?? (anchor ? { x: anchor.left, y: anchor.bottom + 2 } : null);
  if (!at) return;
  const done = () => {
    window.removeEventListener("keydown", key, true);
    window.removeEventListener("mousedown", done, true);
    window.removeEventListener("blur", done);
  };
  // Capture phase, ahead of the menu's own Esc and the main window's key commands.
  const key = (e: KeyboardEvent) => {
    if (!menuOpen()) return done();
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const tool = e.repeat ? null : toolForKey(e.key);
    if (tool || e.key.length === 1) {
      e.preventDefault();
      e.stopImmediatePropagation();
    }
    if (!tool) return;
    setTool("left", tool);
    closeMenu();
    done();
  };
  window.addEventListener("keydown", key, true);
  window.addEventListener("mousedown", done, true);
  window.addEventListener("blur", done);
  openMenu(at.x, at.y, toolMenuItems("left"));
}

// ---- pressing with a tool -------------------------------------------------------------------

/** What Arrange's hit test found under the pointer. */
export interface ToolHit {
  x: number;
  y: number;
  ti: number;
  track?: Track;
  region?: Region;
  sample: number;
}

/** What the tools need from the Tracks area. */
export interface ToolView {
  project: Project;
  pps: number;
  layout: Layout;
  el: HTMLElement;
  redraw: () => void;
  /** Shows a fade length on a region while the Fade tool drags; null ends it. */
  previewFade: (p: { id: Id; kind: "fadeIn" | "fadeOut"; length: number } | null) => void;
  /** A click without a drag moves the playhead (Settings › Editing › Click Moves Playhead). */
  clickLocate: (e: React.MouseEvent, onRegion: boolean) => void;
}

const snapped = (p: Project, v: number) => Math.max(0, Math.round(snapPos(p, v, getState().snapMode, getState().pxPerBeat)));

const contentX = (ev: MouseEvent, el: HTMLElement) => ev.clientX - el.getBoundingClientRect().left + el.scrollLeft;
const contentY = (ev: MouseEvent, el: HTMLElement) => ev.clientY - el.getBoundingClientRect().top + el.scrollTop;

/** A drag starts once the pointer has moved this far, as region drags do. */
const DRAG_PX = 3;

/** Runs `move` for every mouse move once the pointer has gone far enough, then `up` (told
 *  whether it moved) on release. */
function track(e: React.MouseEvent, move: (ev: MouseEvent) => void, up: (moved: boolean, ev: MouseEvent) => void) {
  let moved = false;
  const onMove = (ev: MouseEvent) => {
    if (!moved && Math.abs(ev.clientX - e.clientX) < DRAG_PX && Math.abs(ev.clientY - e.clientY) < DRAG_PX) return;
    moved = true;
    move(ev);
  };
  const onUp = (ev: MouseEvent) => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    up(moved, ev);
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}

/** The region and the selection a click on it acts on: the whole selection when the region is
 *  in it, otherwise the region (with its group's linked regions). */
function clicked(r: Region): Id[] {
  const sel = getState().selectedRegions;
  return sel.includes(r.id) ? sel : linkedRegions(r.id);
}

const select = (ids: Id[] | null) => ids && setState({ selectedRegions: ids, focus: "regions" });

/**
 * A press in the Tracks area with `tool`. Returns false when the Pointer's handling applies
 * instead: the Pointer itself, the Pencil on a region (it moves and trims as the Pointer
 * does), and the other region tools on empty space (a rubber band, as with the Pointer).
 */
export function toolMouseDown(tool: TrackTool, e: React.MouseEvent, h: ToolHit, v: ToolView): boolean {
  if (tool === "pointer") return false;
  if (tool === "marquee") {
    startMarquee(e, h, v);
    return true;
  }
  const t = h.track;
  const r = h.region;
  if (!t) return false;
  if (tool === "pencil") {
    if (r) return false;
    // Logic's Pencil makes an empty one-bar region on a MIDI track; on others it does nothing.
    if (t.kind === "instrument" && !refuseFrozen([t.id])) {
      const start = snapped(v.project, h.sample);
      const length = Math.round(tempoMap(v.project).samplesPerBarAt(start));
      actMade("create_midi_region", { track: t.id, start, length, key: null }).then(select);
    }
    return true;
  }
  if (!r) return false;
  if (refuseFrozen([t.id])) return true;
  switch (tool) {
    case "eraser":
      deleteRegions(clicked(r));
      break;
    case "mute":
      toggleRegionMute(clicked(r));
      break;
    case "scissors": {
      const at = snapped(v.project, h.sample);
      // Alt: equal pieces, as long as the part before the click.
      const ids = e.altKey ? [r.id] : clicked(r);
      const positions = e.altKey ? equalPieces(r.start, regionEnd(r), at) : [at];
      if (positions.length) actMade("split_regions_at", { ids, positions }).then(select);
      break;
    }
    case "glue": {
      // A click on the selection joins it; elsewhere the Glue selects, so Shift-clicks build up
      // what to join.
      const sel = getState().selectedRegions;
      if (sel.includes(r.id) && sel.length > 1 && !e.shiftKey) joinRegions(sel);
      else select(e.shiftKey ? (sel.includes(r.id) ? sel.filter((x) => x !== r.id) : [...sel, ...linkedRegions(r.id)]) : linkedRegions(r.id));
      break;
    }
    case "fade":
      if (r.content.type === "audio") startFade(e, h, t, r, v);
      break;
  }
  return true;
}

/** The Fade tool's drag in progress: the lane and the range dragged over. */
let fadeBand: { ti: number; from: number; to: number } | null = null;

const asFadeRegion = (r: Region): FadeRegion => ({
  id: r.id,
  start: r.start,
  length: r.length,
  fadeIn: r.content.type === "audio" ? r.content.fade_in : 0,
  fadeOut: r.content.type === "audio" ? r.content.fade_out : 0,
});

/** The Fade tool on an audio region: a drag over an end sets that fade, a drag across where
 *  it meets its neighbour crossfades the two, and an Alt-click removes the nearer fade. */
function startFade(e: React.MouseEvent, h: ToolHit, t: Track, r: Region, v: ToolView) {
  const next = followingRegion(t.regions, r);
  const prev = t.regions.find((p) => p.id !== r.id && followingRegion(t.regions, p)?.id === r.id);
  const from = h.sample;
  let edit: FadeEdit | null = null;
  track(
    e,
    (ev) => {
      const to = contentX(ev, v.el) / v.pps;
      edit = fadeToolEdit(asFadeRegion(r), prev && asFadeRegion(prev), next && asFadeRegion(next), from, to);
      fadeBand = { ti: h.ti, from, to };
      v.previewFade(edit && edit.kind !== "xfade" ? { id: edit.id, kind: edit.kind === "in" ? "fadeIn" : "fadeOut", length: edit.length } : null);
      v.redraw();
    },
    (moved) => {
      fadeBand = null;
      v.previewFade(null);
      v.redraw();
      if (!moved) {
        if (e.altKey) clearFade(r, from);
        return;
      }
      if (!edit) return;
      if (edit.kind === "xfade") act("crossfade_regions", { ids: [edit.id], length: edit.length, kind: null });
      else act("set_region_fades", { ids: [edit.id], fadeIn: edit.kind === "in" ? edit.length : null, fadeOut: edit.kind === "out" ? edit.length : null });
    },
  );
}

/** Removes the fade (or crossfade) at the end of `r` nearer to `at`. */
function clearFade(r: Region, at: number) {
  const xf = crossfadesByRegion(getState().snap?.crossfades).get(r.id);
  const side = nearerEnd(r.start, r.length, at);
  if (side === "out" && xf?.out) act("crossfade_regions", { ids: [r.id], length: 0, kind: null });
  else if (side === "in" && xf?.in) act("crossfade_regions", { ids: [xf.in.out_region], length: 0, kind: null });
  else act("set_region_fades", { ids: [r.id], fadeIn: side === "in" ? 0 : null, fadeOut: side === "out" ? 0 : null });
}

// ---- the marquee ----------------------------------------------------------------------------

/** The Marquee tool: drag a time range (snapped) across tracks. Shift-drag grows the marquee
 *  there is; a click without a drag removes it (and moves the playhead, as clicks do). */
function startMarquee(e: React.MouseEvent, h: ToolHit, v: ToolView) {
  const { project, layout } = v;
  const ids = project.tracks.map((t) => t.id);
  const laneH = layout.heights.map((x, i) => x - layout.takeRows[i]);
  const base = e.shiftKey ? getState().marquee : null;
  const i0 = indexAt(layout, h.y);
  const a = snapped(project, h.sample);
  if (i0 < 0) return;
  track(
    e,
    (ev) => {
      const i1 = indexAt(layout, contentY(ev, v.el));
      const m = dragMarquee(ids, laneH, a, i0, snapped(project, contentX(ev, v.el) / v.pps), i1, base);
      setState({ marquee: m, selectedRegions: m ? [] : getState().selectedRegions });
    },
    (moved) => {
      if (moved) return;
      if (!e.shiftKey) setState({ marquee: null });
      v.clickLocate(e, !!h.region);
    },
  );
}

/**
 * A press with a tool other than the Marquee while there is a marquee. With the Pointer,
 * inside it, the regions are split at its edges and the pieces inside selected (Logic); the
 * press is then used up and true is returned. Anywhere else the marquee goes.
 */
export function pressOnMarquee(tool: TrackTool, h: ToolHit): boolean {
  const m = getState().marquee;
  if (!m || tool === "marquee") return false;
  if (tool === "pointer" && inMarquee(m, h.track?.id, h.sample)) {
    marqueeSplit();
    return true;
  }
  setState({ marquee: null });
  return false;
}

const marked = (): Marquee | null => {
  const m = getState().marquee;
  return m && m.end > m.start && m.tracks.length ? m : null;
};

const range = (m: Marquee) => ({ tracks: m.tracks, start: m.start, end: m.end });

/** Splits the regions at the marquee's edges and selects the pieces inside; the marquee goes. */
export function marqueeSplit(): boolean {
  const m = marked();
  if (!m) return false;
  setState({ marquee: null });
  actMade("marquee_split", range(m)).then(select);
  return true;
}

/** Delete: removes what is inside the marquee (the regions are cut at its edges). False when
 *  there is no marquee, so Delete acts on the selected regions. */
export function marqueeDelete(): boolean {
  const m = marked();
  if (!m) return false;
  act("marquee_delete", range(m));
  return true;
}

/** Copy: the parts of the regions inside the marquee go on the region clipboard. */
export function marqueeCopy(then?: () => void): boolean {
  const m = marked();
  if (!m) return false;
  call<[number, Region][]>("marquee_copy", range(m)).then((items) => {
    if (items?.length) setState({ clipboard: items.map(([index, region]) => ({ index, region })), clipboardStart: m.start });
    then?.();
  });
  return true;
}

/** Cut: Copy, then Delete. */
export const marqueeCut = (): boolean => marqueeCopy(marqueeDelete);

/** Paste with a marquee: at its start, on its top track. */
export function marqueePaste(): boolean {
  const m = marked();
  if (!m || !getState().clipboard.length) return false;
  pasteRegions({ track: m.tracks[0], pos: m.start });
  return true;
}

/** Esc removes the marquee (when nothing else, like an open menu, took the key). */
export function escapeMarquee(e: KeyboardEvent) {
  const tag = (e.target as HTMLElement | null)?.tagName;
  if (e.key !== "Escape" || e.defaultPrevented || menuOpen() || tag === "INPUT" || tag === "TEXTAREA" || !getState().marquee) return;
  setState({ marquee: null });
}

// ---- cursors and drawing --------------------------------------------------------------------

const svgCursor = (svg: string, x: number, y: number, fallback: string) => `url("data:image/svg+xml,${encodeURIComponent(svg)}") ${x} ${y}, ${fallback}`;

/** Each tool's pointer, drawn white with a dark outline like the Piano Roll's. */
const CURSORS: Record<Exclude<TrackTool, "pointer">, string> = {
  pencil: svgCursor("<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><path d='M2.5 17.5l1.2-4.6L13.6 3l3.4 3.4-9.9 9.9z' fill='white' stroke='black' stroke-width='1.2' stroke-linejoin='round'/><path d='M11.8 4.8l3.4 3.4' stroke='black' stroke-width='1.2'/></svg>", 2, 17, "crosshair"),
  eraser: svgCursor("<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><path d='M7 17 2.8 12.8l8.5-8.5 5.9 5.9L10.4 17z' fill='white' stroke='black' stroke-width='1.2' stroke-linejoin='round'/><path d='M6.2 8.9l5.9 5.9' stroke='black' stroke-width='1.2'/></svg>", 4, 16, "cell"),
  scissors: svgCursor("<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><circle cx='5.5' cy='14.5' r='2.4' fill='white' stroke='black' stroke-width='1.2'/><circle cx='14.5' cy='14.5' r='2.4' fill='white' stroke='black' stroke-width='1.2'/><path d='M7.2 12.8 15 2.5M12.8 12.8 5 2.5' stroke='black' stroke-width='1.4' stroke-linecap='round'/></svg>", 10, 2, "crosshair"),
  glue: svgCursor("<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><path d='M6 3h8l-1 5H7zM7 8 5.2 17.5h9.6L13 8' fill='white' stroke='black' stroke-width='1.2' stroke-linejoin='round'/></svg>", 10, 18, "copy"),
  mute: svgCursor("<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><rect x='2.5' y='7' width='15' height='6' rx='1.5' fill='white' stroke='black' stroke-width='1.2'/><path d='M3.5 17.5 16.5 2.5' stroke='white' stroke-width='3.2' stroke-linecap='round'/><path d='M3.5 17.5 16.5 2.5' stroke='black' stroke-width='1.4' stroke-linecap='round'/></svg>", 10, 10, "pointer"),
  fade: svgCursor("<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><path d='M2.5 17.5C9 17.5 13 11 17.5 2.5' fill='none' stroke='white' stroke-width='3.4' stroke-linecap='round'/><path d='M2.5 17.5C9 17.5 13 11 17.5 2.5' fill='none' stroke='black' stroke-width='1.4' stroke-linecap='round'/><path d='M2.5 17.5h15' stroke='black' stroke-width='1.2'/></svg>", 2, 17, "col-resize"),
  marquee: "crosshair",
};

/** The Scissors' guide: the lane and the (snapped) position a click would cut at. */
let guide: { ti: number; pos: number } | null = null;

/**
 * The pointer over the tracks with `tool`: sets the tool's cursor and the Scissors' guide.
 * Returns false when the Pointer's cursor applies (the Pointer, and the Pencil over a region).
 */
export function toolHover(tool: TrackTool, h: ToolHit | null, v: Pick<ToolView, "project" | "el" | "redraw">): boolean {
  const g = tool === "scissors" && h?.region && h.track && !h.track.frozen ? { ti: h.ti, pos: snapped(v.project, h.sample) } : null;
  if (g?.ti !== guide?.ti || g?.pos !== guide?.pos) {
    guide = g;
    v.redraw();
  }
  if (tool === "pointer" || (tool === "pencil" && h?.region)) return false;
  v.el.style.cursor = CURSORS[tool];
  return true;
}

/** The pointer left the tracks. */
export function toolLeave(redraw: () => void) {
  pointer = null;
  if (guide) {
    guide = null;
    redraw();
  }
}

/** Draws the marquee, the Scissors' guide and the Fade tool's drag over the lanes. */
export function drawToolOverlay(ctx: CanvasRenderingContext2D, v: { project: Project; layout: Layout; pps: number; vp: { left: number; top: number }; W: number; H: number }) {
  const { project, layout, pps, vp } = v;
  const m = getState().marquee;
  if (!m && !guide && !fadeBand) return;
  const select = getComputedStyle(document.documentElement).getPropertyValue("--select").trim() || "#3d8bf0";
  const laneH = layout.heights.map((x, i) => x - layout.takeRows[i]);
  const ids = project.tracks.map((t) => t.id);
  if (m) {
    const rows = marqueeRows(m, ids, layout.tops, laneH);
    if (rows) {
      const x = Math.round(m.start * pps - vp.left) + 0.5;
      const w = Math.max(1, Math.round((m.end - m.start) * pps));
      const y = Math.round(rows.top - vp.top) + 0.5;
      const h = Math.round(rows.bottom - rows.top) - 1;
      ctx.fillStyle = select;
      ctx.globalAlpha = 0.22;
      ctx.fillRect(x, y, w, h);
      ctx.globalAlpha = 1;
      ctx.strokeStyle = select;
      ctx.lineWidth = 1;
      ctx.strokeRect(x, y, w, h);
    }
  }
  if (fadeBand && laneH[fadeBand.ti] > 0) {
    const x0 = Math.min(fadeBand.from, fadeBand.to) * pps - vp.left;
    const x1 = Math.max(fadeBand.from, fadeBand.to) * pps - vp.left;
    ctx.fillStyle = select;
    ctx.globalAlpha = 0.25;
    ctx.fillRect(x0, layout.tops[fadeBand.ti] - vp.top + 1, x1 - x0, laneH[fadeBand.ti] - 3);
    ctx.globalAlpha = 1;
  }
  if (guide && laneH[guide.ti] > 0) {
    ctx.fillStyle = theme.selectOutline;
    ctx.fillRect(Math.round(guide.pos * pps - vp.left), layout.tops[guide.ti] - vp.top + 1, 1, laneH[guide.ti] - 3);
  }
}
