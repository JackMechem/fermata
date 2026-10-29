// Track Freeze (Logic's snowflake button): renders a track's instrument and plug-ins to a file
// and plays that instead, with the plug-ins unloaded. The engine side is
// crates/engine/src/session/freeze.rs; these are the UI's entry points and guards.
import { listen } from "@tauri-apps/api/event";
import { act, call, type Id, type Snapshot, type Track } from "./api";
import { isBouncing, setBouncing } from "./bounce";
import { getState, setState } from "./store";

const toast = (text: string, ms = 3000) => setState({ toast: { text, at: Date.now(), ms } });
const tracks = (): Track[] => getState().snap?.project.tracks ?? [];

/** Audio and software instrument tracks have a Freeze button. */
export const canFreeze = (t: Track) => t.kind === "audio" || t.kind === "instrument";

export const isFrozen = (id: Id) => !!tracks().find((t) => t.id === id)?.frozen;

/**
 * Says why an edit can't happen when it would touch a frozen track. Returns true (and
 * shows a toast) when one of `ids` is frozen. The session refuses these edits too.
 */
export function refuseFrozen(ids: Id[]): boolean {
  const t = tracks().find((x) => ids.includes(x.id) && x.frozen);
  if (!t) return false;
  toast(`${t.name} is frozen. Unfreeze it to edit its regions and plug-ins.`);
  return true;
}

/** Renders and freezes the tracks, showing progress, as one undo step. */
export async function freezeTracks(ids: Id[]) {
  const todo = tracks().filter((t) => ids.includes(t.id) && canFreeze(t) && !t.frozen);
  if (!todo.length || isBouncing()) return;
  const label = todo.length > 1 ? `${todo.length} tracks` : todo[0].name;
  setBouncing(true);
  toast(`Freezing ${label}…`, 60000);
  const off = await listen<number>("bounce-progress", (e) => toast(`Freezing ${label}… ${Math.round(e.payload * 100)}%`, 60000));
  try {
    const snap = await call<Snapshot>("freeze_tracks", { ids: todo.map((t) => t.id) });
    if (snap) {
      setState({ snap });
      toast(`Froze ${label}`, 1500);
    }
  } finally {
    off();
    setBouncing(false);
  }
}

export function unfreezeTracks(ids: Id[]) {
  const todo = tracks().filter((t) => ids.includes(t.id) && t.frozen).map((t) => t.id);
  if (todo.length) act("unfreeze_tracks", { ids: todo });
}

/** The snowflake: freezes the tracks, or unfreezes them when they're all frozen already. */
export function toggleFreeze(ids: Id[]) {
  const sel = tracks().filter((t) => ids.includes(t.id) && canFreeze(t));
  if (sel.length && sel.every((t) => t.frozen)) unfreezeTracks(ids);
  else freezeTracks(ids);
}
