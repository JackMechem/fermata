// The Arrangement track (Logic's Global Tracks › Arrangement): named sections that tile the
// timeline. The engine does the edits (crates/engine/src/arrangement.rs); this is what the
// lane needs to draw them and to preview a drag. Pure, so tests can import it.
import type { ArrangementMarker, Id, Project } from "./api";

/** The names in the section's name pop-up, as in Logic (the engine's `arrangement::NAMES`). */
export const ARRANGEMENT_NAMES = ["Intro", "Verse", "Pre-Chorus", "Chorus", "Bridge", "Solo", "Break", "Interlude", "Outro"];

/** Each name's own colour, so a song's verses and choruses match at a glance. */
const NAME_COLORS: Record<string, string> = {
  Intro: "#37a6c9",
  Verse: "#4fae4c",
  "Pre-Chorus": "#c9cf3a",
  Chorus: "#ef7d2f",
  Bridge: "#8a54d6",
  Solo: "#d24c9d",
  Break: "#8e8e93",
  Interlude: "#a0745a",
  Outro: "#4f6fe0",
};

/** For other names: picked by the name, so equal names get equal colours. */
const OTHER_COLORS = ["#35a8a0", "#3b8fd9", "#6a5be0", "#b14fc8", "#d9546e", "#f2a63b", "#3aa776", "#6b7a8f"];

/** A section's colour: its own, else its name's. "Verse 2" counts as a verse. */
export function sectionColor(s: Pick<ArrangementMarker, "name" | "color">): string {
  if (s.color) return s.color;
  const base = s.name.replace(/\s*\d+$/, "");
  if (NAME_COLORS[base]) return NAME_COLORS[base];
  let h = 0;
  for (const c of s.name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return OTHER_COLORS[h % OTHER_COLORS.length];
}

export const sectionEnd = (s: Pick<ArrangementMarker, "start" | "length">) => s.start + s.length;

/** The sections in order (the project keeps them so; old or hand-edited files may not). */
export function sections(p: Pick<Project, "arrangement">): ArrangementMarker[] {
  return [...(p.arrangement ?? [])].sort((a, b) => a.start - b.start);
}

/** The section playing at `pos`. */
export function sectionAt(list: ArrangementMarker[], pos: number): ArrangementMarker | null {
  return list.find((s) => pos >= s.start && pos < sectionEnd(s)) ?? null;
}

/** The sections while one is resized: the later ones move along, as the engine will. */
export function resizePreview(list: ArrangementMarker[], id: Id, length: number): ArrangementMarker[] {
  let at = list[0]?.start ?? 0;
  return list.map((s) => {
    const out = { ...s, start: at, length: s.id === id ? Math.max(1, length) : s.length };
    at += out.length;
    return out;
  });
}

/** Where a dragged section drops: before section `before` (null: after the last), drawn at
 *  timeline `at`. */
export interface Drop {
  before: Id | null;
  at: number;
}

/** The drop for section `id` dragged so that it would start at `pos`: the section boundary
 *  nearest to it. Null when a move would leave it where it is (a copy can go anywhere). */
export function dropSlot(list: ArrangementMarker[], id: Id, pos: number, copy: boolean): Drop | null {
  if (!list.length) return null;
  const gaps: Drop[] = list.map((s) => ({ before: s.id, at: s.start }));
  gaps.push({ before: null, at: sectionEnd(list[list.length - 1]) });
  let best = gaps[0];
  for (const g of gaps) if (Math.abs(g.at - pos) < Math.abs(best.at - pos)) best = g;
  if (!copy) {
    const i = list.findIndex((s) => s.id === id);
    const next = list[i + 1]?.id ?? null;
    if (best.before === id || best.before === next) return null;
  }
  return best;
}

/** The regions on visible tracks that sound in the section: clicking it selects them. */
export function regionsInSection(p: Pick<Project, "tracks">, s: Pick<ArrangementMarker, "start" | "length">): Id[] {
  const end = sectionEnd(s);
  return p.tracks
    .filter((t) => !t.hidden)
    .flatMap((t) => t.regions)
    .filter((r) => r.start < end && Math.max(r.start + r.length, r.loop_end ?? 0) > s.start)
    .map((r) => r.id);
}
