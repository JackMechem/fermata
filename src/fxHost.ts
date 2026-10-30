// Hosting built-in effects in the UI: the registry, their panel windows, the plug-in and Channel
// EQ clipboard, and the slot right-click menu.
import { act, BUILTIN_SYNTH, call, EQ_SLOT, INSTRUMENT_SLOT, MASTER_ID, type ChannelEq, type Id, type PluginSlot, type Project } from "./api";
import type { FxInfo } from "./fx";
import type { MenuItem } from "./components/Menu";
import { getState, setState } from "./store";
import { closePanel, fxKey, openPanel, togglePanel } from "./fxDockState";
import { sideChainMenu } from "./sideChain";

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

/** Opens an effect's panel (or brings it forward), docked or floating as Settings say (fxDock.ts). */
export function openFxPanel(uid: Id) {
  openPanel(fxKey(uid));
}

export function closeFxPanel(uid: Id) {
  closePanel(fxKey(uid));
}

/** A slot click: closes the panel if it is on screen, otherwise opens or shows it. */
export function toggleFxPanel(uid: Id) {
  togglePanel(fxKey(uid));
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
// In the store, shared by every window, and outside the project, so it survives opening
// another.

export const pluginClipboard = () => getState().pluginClip;
export const eqClipboard = () => getState().eqClip;

export async function copyPlugin(track: Id, slot: number) {
  const s = await call<PluginSlot>("copy_plugin", { track, slot });
  if (s) setState({ pluginClip: s });
}

export function pastePlugin(tracks: Id[], slot: number) {
  const plugin = pluginClipboard();
  if (plugin) return act("paste_plugin", { tracks, slot, plugin });
}

export function copyEq(eq: ChannelEq) {
  setState({ eqClip: structuredClone(eq) });
}

export function pasteEq(tracks: Id[]) {
  const eq = eqClipboard();
  if (eq) return act("paste_eq", { tracks, eq });
}

/** The tracks a paste goes to: every selected track when `track` is among several selected. */
function pasteTargets(track: Id): Id[] {
  const sel = getState().selectedTracks;
  return sel.length > 1 && sel.includes(track) ? sel : [track];
}

/**
 * The Side Chain item of a VST3 insert that has a side-chain input, then a separator: the editor is
 * a native window with no room for it, so it lives in the slot's menus. Nothing for other slots.
 */
export function slotSideChainItems(track: Id, slot: number, plugin: PluginSlot | null): MenuItem[] {
  const snap = getState().snap;
  if (!snap || !plugin || !snap.key_inputs?.includes(plugin.uid)) return [];
  const set = (source: Id | null) => act("set_sidechain", { track, slot, source });
  const setPre = (pre: boolean) => act("set_sidechain_pre", { track, slot, pre });
  return [sideChainMenu(snap.project.tracks, track, plugin, set, setPre), { separator: true }];
}

/** The right-click menu of an insert or instrument slot. */
export function slotMenu(track: Id, slot: number, plugin: PluginSlot | null): MenuItem[] {
  const instrument = slot === INSTRUMENT_SLOT;
  const clip = pluginClipboard();
  // An instrument only pastes into an instrument slot, an effect only into an Audio FX slot.
  // Like Session::find_plugin: a plug-in found at another path matches by its class id.
  const cid = (id: string) => id.slice(id.lastIndexOf("|") + 1).toLowerCase();
  const isInstrument = (id: string) => {
    if (id === BUILTIN_SYNTH) return true;
    const { plugins } = getState();
    const desc = plugins.find((p) => p.id === id) ?? (id.includes("|") ? plugins.find((p) => p.id.includes("|") && cid(p.id) === cid(id)) : undefined);
    return !!desc?.is_instrument;
  };
  const fits = !!clip && isInstrument(clip.plugin_id) === instrument;
  const targets = pasteTargets(track);
  const items: MenuItem[] = [
    ...slotSideChainItems(track, slot, plugin),
    { label: "Copy Plug-in", disabled: !plugin, onClick: () => copyPlugin(track, slot) },
    { label: clip ? `Paste ${clip.name}` : "Paste Plug-in", disabled: !fits, onClick: () => pastePlugin([track], slot) },
  ];
  if (targets.length > 1) items.push({ label: "Paste to Selected Tracks", disabled: !fits, onClick: () => pastePlugin(targets, slot) });
  return items;
}

/** The Channel EQ thumbnail's right-click menu. */
export function eqMenu(track: Id, eq: ChannelEq | undefined): MenuItem[] {
  const targets = pasteTargets(track);
  const eqClip = eqClipboard();
  const items: MenuItem[] = [
    { label: "Copy Channel EQ", disabled: !eq, onClick: () => eq && copyEq(eq) },
    { label: "Paste Channel EQ", disabled: !eqClip, onClick: () => pasteEq([track]) },
  ];
  if (targets.length > 1) items.push({ label: "Paste Channel EQ to Selected Tracks", disabled: !eqClip, onClick: () => pasteEq(targets) });
  return items;
}

export { EQ_SLOT };
