// Hosting built-in effects in the UI: the registry, their panel windows, the plug-in and Channel
// EQ clipboard, and the slot right-click menu.
import { act, BUILTIN_SYNTH, call, EQ_SLOT, INSTRUMENT_SLOT, MASTER_ID, type ChannelEq, type Id, type PluginSlot, type Project } from "./api";
import type { FxInfo } from "./fx";
import type { MenuItem } from "./components/Menu";
import { getState, setState } from "./store";

/** The preset id of the Channel EQ. */
export const CHANNEL_EQ = "builtin:channel-eq";

export async function loadFxList() {
  const list = await call<FxInfo[]>("list_builtin_effects");
  if (list) setState({ fxList: list });
}

export function fxInfo(pluginId: string | undefined): FxInfo | undefined {
  return pluginId ? getState().fxList.find((f) => f.id === pluginId) : undefined;
}

/** Built-in effects (not the built-in synth) have a panel. */
export const isBuiltinFx = (pluginId: string | undefined) => !!fxInfo(pluginId);

/** Where a built-in effect instance sits: its strip (MASTER_ID for Stereo Out) and slot. */
export function findFxSlot(project: Project | undefined, uid: Id): { track: Id; slot: number; plugin: PluginSlot } | null {
  if (!project) return null;
  for (const t of project.tracks) {
    const i = t.inserts.findIndex((s) => s?.uid === uid);
    if (i >= 0) return { track: t.id, slot: i, plugin: t.inserts[i]! };
  }
  const i = project.master.inserts.findIndex((s) => s?.uid === uid);
  return i >= 0 ? { track: MASTER_ID, slot: i, plugin: project.master.inserts[i]! } : null;
}

export function openFxPanel(uid: Id) {
  setState((s) => ({ fxPanels: [...s.fxPanels.filter((u) => u !== uid), uid] }));
}

export function closeFxPanel(uid: Id) {
  setState((s) => ({ fxPanels: s.fxPanels.filter((u) => u !== uid) }));
}

export function toggleFxPanel(uid: Id) {
  if (getState().fxPanels.includes(uid)) closeFxPanel(uid);
  else openFxPanel(uid);
}

/** Loads a plug-in into a slot; a built-in effect then opens its panel (VST3s their editor). */
export async function loadPlugin(track: Id, slot: number, pluginId: string, open = true) {
  const snap = await act("set_plugin", { track, slot, pluginId, open });
  if (!snap || !open || !isBuiltinFx(pluginId)) return;
  const inserts = track === MASTER_ID ? snap.project.master.inserts : snap.project.tracks.find((t) => t.id === track)?.inserts;
  const uid = inserts?.[slot]?.uid;
  if (uid !== undefined) openFxPanel(uid);
}

// ---- clipboard --------------------------------------------------------------------------------
// Kept here rather than in the store's project state, so it survives opening another project.

let pluginClip: PluginSlot | null = null;
let eqClip: ChannelEq | null = null;

export const pluginClipboard = () => pluginClip;
export const eqClipboard = () => eqClip;

export async function copyPlugin(track: Id, slot: number) {
  const s = await call<PluginSlot>("copy_plugin", { track, slot });
  if (s) pluginClip = s;
}

export function pastePlugin(tracks: Id[], slot: number) {
  if (pluginClip) return act("paste_plugin", { tracks, slot, plugin: pluginClip });
}

export function copyEq(eq: ChannelEq) {
  eqClip = structuredClone(eq);
}

export function pasteEq(tracks: Id[]) {
  if (eqClip) return act("paste_eq", { tracks, eq: eqClip });
}

/** The tracks a paste goes to: every selected track when `track` is among several selected. */
function pasteTargets(track: Id): Id[] {
  const sel = getState().selectedTracks;
  return sel.length > 1 && sel.includes(track) ? sel : [track];
}

/** The right-click menu of an insert or instrument slot. */
export function slotMenu(track: Id, slot: number, plugin: PluginSlot | null): MenuItem[] {
  const instrument = slot === INSTRUMENT_SLOT;
  const clip = pluginClip;
  // An instrument only pastes into an instrument slot, an effect only into an Audio FX slot.
  const isInstrument = (id: string) => id === BUILTIN_SYNTH || !!getState().plugins.find((p) => p.id === id)?.is_instrument;
  const fits = !!clip && isInstrument(clip.plugin_id) === instrument;
  const targets = pasteTargets(track);
  const items: MenuItem[] = [
    { label: "Copy Plug-in", disabled: !plugin, onClick: () => copyPlugin(track, slot) },
    { label: clip ? `Paste ${clip.name}` : "Paste Plug-in", disabled: !fits, onClick: () => pastePlugin([track], slot) },
  ];
  if (targets.length > 1) items.push({ label: "Paste to Selected Tracks", disabled: !fits, onClick: () => pastePlugin(targets, slot) });
  return items;
}

/** The Channel EQ thumbnail's right-click menu. */
export function eqMenu(track: Id, eq: ChannelEq | undefined): MenuItem[] {
  const targets = pasteTargets(track);
  const items: MenuItem[] = [
    { label: "Copy Channel EQ", disabled: !eq, onClick: () => eq && copyEq(eq) },
    { label: "Paste Channel EQ", disabled: !eqClip, onClick: () => pasteEq([track]) },
  ];
  if (targets.length > 1) items.push({ label: "Paste Channel EQ to Selected Tracks", disabled: !eqClip, onClick: () => pasteEq(targets) });
  return items;
}

export { EQ_SLOT };
