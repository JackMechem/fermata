import type { Id, Track } from "./api";
import { getState, setState, useStore } from "./store";
import { takeRowsHeight } from "./takes";
import { OUTPUT_ID, subLaneHeight } from "./autoLanes";

/** Smallest track height: one line of text. Headers lay out in tiers by height (see
 *  trackHeader.ts), and regions slim their name bar (regionBar.ts). */
export const MIN_TRACK_H = 22;
export const MAX_TRACK_H = 260;

/** Vertical position of every track: per-track heights fall back to the global height. Tracks
 *  inside a collapsed stack, and hidden tracks outside hide mode, get no height. `depth` is how
 *  many stacks each track is in. A track with an open take folder grows by its take rows
 *  (`takeRows` has that extra height, already included in `heights`). */
export interface Layout {
  tops: number[];
  heights: number[];
  depth: number[];
  total: number;
  takeRows: number[];
  /** With automation shown: each track's extra automation lanes sit below it (after
   *  `heights`), `autoRows` tall in all, `subLane` each. `total` includes them. */
  autoRows: number[];
  subLane: number[];
  /** The Output track (Stereo Out), below the last track and outside `total`. */
  output: OutputRow | null;
}

export interface OutputRow {
  top: number;
  /** Its main lane; its sub-lanes follow, `autoRows` in all. */
  height: number;
  autoRows: number;
  subLane: number;
}

/** What the automation view adds: sub-lanes (while shown), and the Output track. */
export interface AutoLayout {
  shown: boolean;
  output: boolean;
  /** The Output track's sub-lanes. */
  outputLanes: number;
}

const NO_AUTO: AutoLayout = { shown: false, output: false, outputLanes: 0 };

export function trackLayout(tracks: Track[], base: number, heights: Record<Id, number>, showHidden = false, openFolders: Id[] = [], auto: AutoLayout = NO_AUTO): Layout {
  const byId = new Map(tracks.map((t) => [t.id, t]));
  const tops: number[] = [];
  const hs: number[] = [];
  const depth: number[] = [];
  const takeRows: number[] = [];
  const autoRows: number[] = [];
  const subLane: number[] = [];
  let y = 0;
  for (const t of tracks) {
    let d = 0;
    let hidden = t.hidden && !showHidden;
    for (let p = t.parent != null ? byId.get(t.parent) : undefined; p && d < 32; p = p.parent != null ? byId.get(p.parent) : undefined) {
      d++;
      if (p.collapsed || (p.hidden && !showHidden)) hidden = true;
    }
    const rows = hidden || !openFolders.length ? 0 : takeRowsHeight(t, openFolders);
    const h = hidden ? 0 : (heights[t.id] ?? base) + rows;
    const lane = subLaneHeight(heights[t.id] ?? base);
    const subs = hidden || !auto.shown ? 0 : (t.auto_lanes?.length ?? 0) * lane;
    tops.push(y);
    hs.push(h);
    depth.push(d);
    takeRows.push(rows);
    autoRows.push(subs);
    subLane.push(lane);
    y += h + subs;
  }
  let output: OutputRow | null = null;
  if (auto.output) {
    const h = heights[OUTPUT_ID] ?? base;
    const lane = subLaneHeight(h);
    output = { top: y, height: h, subLane: lane, autoRows: auto.shown ? auto.outputLanes * lane : 0 };
  }
  return { tops, heights: hs, depth, total: y, takeRows, autoRows, subLane, output };
}

/** Index of the track at content y, clamped to the last track (or -1 with no tracks). */
export function indexAt(l: Layout, y: number): number {
  if (!l.tops.length) return -1;
  let i = 0;
  while (i + 1 < l.tops.length && y >= l.tops[i + 1]) i++;
  // Never land on a hidden track (they share the top of the next visible one).
  while (i > 0 && l.heights[i] === 0) i--;
  return y < 0 ? 0 : i;
}

export function useLayout(): Layout {
  const tracks = useStore((s) => s.snap?.project.tracks);
  const base = useStore((s) => s.trackHeight);
  const heights = useStore((s) => s.trackHeights);
  const showHidden = useStore((s) => s.showHidden);
  const openFolders = useStore((s) => s.openFolders);
  const shown = useStore((s) => s.showAutomation);
  const output = useStore((s) => s.showOutput);
  const outputLanes = useStore((s) => s.snap?.project.master.auto_lanes?.length ?? 0);
  return trackLayout(tracks ?? [], base, heights, showHidden, openFolders, { shown, output, outputLanes });
}

/** The layout as it is now, for code outside React. */
export function currentLayout(): Layout {
  const s = getState();
  return trackLayout(s.snap?.project.tracks ?? [], s.trackHeight, s.trackHeights, s.showHidden, s.openFolders, autoLayout());
}

/** The automation view's part of the layout, as the store has it now. */
export function autoLayout(): AutoLayout {
  const s = getState();
  return { shown: s.showAutomation, output: s.showOutput, outputLanes: s.snap?.project.master.auto_lanes?.length ?? 0 };
}

/** The nearest track with height `d` steps away from `index` (skipping collapsed and hidden
 *  tracks), or `index` itself when there's none. */
export function stepVisible(l: Layout, index: number, d: number): number {
  let i = index;
  for (let left = Math.abs(d); left > 0; left--) {
    let k = i + Math.sign(d);
    while (k >= 0 && k < l.heights.length && l.heights[k] === 0) k += Math.sign(d);
    if (k < 0 || k >= l.heights.length) break;
    i = k;
  }
  return i;
}

/** The signed number of tracks with height passed going from `from` to `to`: how far a
 *  vertical drag went in visible steps, to apply with `stepVisible`. */
export function visibleSteps(l: Layout, from: number, to: number): number {
  const dir = Math.sign(to - from);
  let n = 0;
  for (let k = from + dir; dir && k !== to + dir; k += dir) if (l.heights[k] > 0) n += dir;
  return n;
}

export function clampHeight(h: number): number {
  return Math.round(Math.min(MAX_TRACK_H, Math.max(MIN_TRACK_H, h)));
}

/** Sets one track's height (null resets it to the global height). */
export function setTrackHeight(id: Id, h: number | null) {
  const heights = { ...getState().trackHeights };
  if (h === null) delete heights[id];
  else heights[id] = clampHeight(h);
  setState({ trackHeights: heights });
}

/** Scales every track (Logic's vertical zoom): the global height and any individual sizes. */
export function zoomTracks(factor: number) {
  const s = getState();
  const heights: Record<Id, number> = {};
  for (const [id, h] of Object.entries(s.trackHeights)) heights[Number(id)] = clampHeight(h * factor);
  setState({ trackHeight: clampHeight(s.trackHeight * factor), trackHeights: heights });
}

// ---- persistence: heights (and the Output track) are view preferences, remembered per project folder

const key = (path: string) => `fermata.trackHeights:${path}`;

export function loadHeights(path: string) {
  try {
    const raw = localStorage.getItem(key(path));
    const v = raw ? JSON.parse(raw) : null;
    setState({ trackHeight: clampHeight(v?.base ?? 64), trackHeights: v?.heights ?? {}, showOutput: !!v?.output });
  } catch {
    setState({ trackHeight: 64, trackHeights: {}, showOutput: false });
  }
}

export function saveHeights(path: string) {
  try {
    const s = getState();
    localStorage.setItem(key(path), JSON.stringify({ base: s.trackHeight, heights: s.trackHeights, output: s.showOutput }));
  } catch {
    // Not critical: heights fall back to the default next time.
  }
}
