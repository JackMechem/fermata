// Edit › Select in the Tracks area: Logic's ways of selecting regions by criteria. Each takes
// the tracks in view and returns the region ids to select. Pure, so tests can import it; the
// commands are wired up in arrangeCmds.ts.
import type { CcEvent, Id, Note, Region, Track } from "./api";
import { regionEnd } from "./loops.ts";

type Lanes = Pick<Track, "id" | "color" | "regions">[];

const all = (tracks: Lanes) => tracks.flatMap((t) => t.regions);
const ids = (rs: Region[]) => rs.map((r) => r.id);
const picked = (tracks: Lanes, sel: Id[]) => all(tracks).filter((r) => sel.includes(r.id));

/** Select All Following: every region starting at or after `from` (the first selected
 *  region's start, or the playhead); only on `track` for All Following of Same Track. */
export function following(tracks: Lanes, from: number, track?: Id): Id[] {
  return ids(tracks.filter((t) => track === undefined || t.id === track).flatMap((t) => t.regions.filter((r) => r.start >= from)));
}

/** Select Inside Locators: the regions that lie wholly between the locators. */
export function insideLocators(tracks: Lanes, start: number, end: number): Id[] {
  return ids(all(tracks).filter((r) => r.start >= start && regionEnd(r) <= end));
}

/** Select Muted Regions. */
export const muted = (tracks: Lanes): Id[] => ids(all(tracks).filter((r) => r.muted));

/** Select Overlapped Regions: every region that overlaps another on its track. */
export function overlapped(tracks: Lanes): Id[] {
  return ids(tracks.flatMap((t) => t.regions.filter((r) => t.regions.some((o) => o.id !== r.id && o.start < regionEnd(r) && regionEnd(o) > r.start))));
}

/** Select Same Colored Regions: the regions showing a colour one of the selected shows (their
 *  own, or their track's). */
export function sameColor(tracks: Lanes, sel: Id[]): Id[] {
  const color = (t: Lanes[number], r: Region) => (r.color ?? t.color).toLowerCase();
  const want = new Set(tracks.flatMap((t) => t.regions.filter((r) => sel.includes(r.id)).map((r) => color(t, r))));
  return ids(tracks.flatMap((t) => t.regions.filter((r) => want.has(color(t, r)))));
}

const notesKey = (notes: Note[]) => notes.map((n) => `${n.start}:${n.length}:${n.pitch}:${n.velocity}:${n.muted ? 1 : 0}`).sort().join(",");
const ccKey = (cc: CcEvent[] = []) => cc.map((c) => `${c.start}:${JSON.stringify(c.kind)}:${c.value}`).join(",");
const pitches = (notes: Note[]) => [...notes].sort((a, b) => a.start - b.start || a.pitch - b.pitch).map((n) => n.pitch).join(",");

/** Equal Regions: the same content. Audio plays the same stretch of the same file; MIDI has
 *  the same length, notes and controllers. */
export function sameContent(a: Region, b: Region): boolean {
  if (a.length !== b.length) return false;
  const x = a.content;
  const y = b.content;
  if (x.type === "audio" && y.type === "audio") return x.clip === y.clip && x.offset === y.offset;
  if (x.type === "midi" && y.type === "midi") return notesKey(x.notes) === notesKey(y.notes) && ccKey(x.cc) === ccKey(y.cc);
  return false;
}

/** Similar Regions: audio from the same file; MIDI playing the same pitches in the same
 *  order, whatever the timing, lengths and velocities. */
export function similarContent(a: Region, b: Region): boolean {
  const x = a.content;
  const y = b.content;
  if (x.type === "audio" && y.type === "audio") return x.clip === y.clip;
  if (x.type === "midi" && y.type === "midi") return x.notes.length > 0 && pitches(x.notes) === pitches(y.notes);
  return false;
}

/** The regions matching any selected one by `same` (the selected ones included). */
export function matching(tracks: Lanes, sel: Id[], same: (a: Region, b: Region) => boolean): Id[] {
  const base = picked(tracks, sel);
  return ids(all(tracks).filter((r) => sel.includes(r.id) || base.some((b) => same(b, r))));
}

/** Invert Selection. */
export const invert = (tracks: Lanes, sel: Id[]): Id[] => ids(all(tracks).filter((r) => !sel.includes(r.id)));

/** Where Select All Following starts: the earliest selected region, or the playhead. */
export function followingFrom(tracks: Lanes, sel: Id[], playhead: number): number {
  const starts = picked(tracks, sel).map((r) => r.start);
  return starts.length ? Math.min(...starts) : playhead;
}
