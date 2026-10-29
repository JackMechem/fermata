// A plug-in moved to another track takes its automation lanes along (the engine re-keys them
// to the new instance there). This keeps the automation view in step: a lane of the plug-in
// that its old track was showing is shown on the new track instead.
import type { AutoParam, Id } from "./api";

/** The shown lane per track (`autoLane` in the store) after plug-in `oldUid` on track `from`
 *  moved to track `to`, where it is instance `newUid`. The old track goes back to its default
 *  lane. Returns `shown` itself when nothing changes. */
export function followMovedPlugin(shown: Record<Id, AutoParam>, from: Id, oldUid: Id, to: Id, newUid: Id): Record<Id, AutoParam> {
  const p = shown[from];
  if (from === to || p?.type !== "plugin" || p.value.uid !== oldUid) return shown;
  const next = { ...shown, [to]: { type: "plugin", value: { ...p.value, uid: newUid } } as AutoParam };
  delete next[from];
  return next;
}
