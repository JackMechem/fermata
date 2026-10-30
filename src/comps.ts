// Take folder comps and MIDI take folders: what the folder menu lists and what the folder's
// name bar says. The comp logic itself is in the engine (crates/engine/src/takes.rs).
import type { Region, TakeFolder } from "./api";

/** A MIDI take folder: whole takes only, no swipe comping (as in Logic). */
export const isMidiFolder = (r: Region): boolean => !!r.takes && r.content.type === "midi";

/** The folder's comps by name. One that never had a second comp has "Comp 1". */
export function compNames(f: TakeFolder): string[] {
  return f.comps?.length ? f.comps.map((c) => c.name) : ["Comp 1"];
}

/** Index of the comp being heard. */
export function currentComp(f: TakeFolder): number {
  return Math.min(f.current ?? 0, compNames(f).length - 1);
}

/** The take heard throughout when the comp is one whole take, else null. */
function wholeTakeName(f: TakeFolder): string | null {
  if (f.comp.length !== 1) return null;
  const s = f.comp[0];
  const t = f.takes.find((x) => x.id === s.take);
  return t && t.start === s.start && t.end === s.end ? t.name : null;
}

/** What the folder's name bar says after its name: the take heard throughout, or the comp
 *  (Logic's "Audio 1: Comp 2"). A MIDI folder names its take. */
export function folderLabel(r: Region): string | null {
  const f = r.takes;
  if (!f) return null;
  if (isMidiFolder(r)) {
    const id = f.comp[0]?.take ?? f.takes[f.takes.length - 1]?.id;
    return f.takes.find((t) => t.id === id)?.name ?? null;
  }
  return wholeTakeName(f) ?? compNames(f)[currentComp(f)];
}
