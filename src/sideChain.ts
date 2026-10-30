// Choosing a side-chain key source, kept pure so it can be tested: the pop-up of a built-in
// effect's panel and the Side Chain menu of a VST3's insert slot offer the same list.
import type { Id, PluginSlot, Track } from "./api";
import type { MenuItem } from "./components/Menu";

type Source = Pick<Track, "id" | "name" | "kind">;

/** The tracks that can key a plug-in on track `own`: all with audio of their own but itself. */
export function keySources<T extends Source>(tracks: readonly T[], own: Id): T[] {
  return tracks.filter((t) => t.id !== own && t.kind !== "folder");
}

/** The name of the source keying a slot, or null with none (or one that's gone). */
export function keyName(tracks: readonly Source[], plugin: Pick<PluginSlot, "sidechain">): string | null {
  const source = plugin.sidechain ?? null;
  return source === null ? null : tracks.find((t) => t.id === source)?.name ?? null;
}

/** None, then the sources, with the current one checked. */
export function keySourceItems(tracks: readonly Source[], own: Id, source: Id | null, set: (src: Id | null) => void): MenuItem[] {
  const items: MenuItem[] = [{ label: "None", checked: source === null, onClick: () => set(null) }, { separator: true }];
  for (const t of keySources(tracks, own)) items.push({ label: t.name, checked: t.id === source, onClick: () => set(t.id) });
  return items;
}

/**
 * The Side Chain item of a VST3's insert slot: its source, and once it has one, where the key
 * leaves it (the Pre / Post switch of a built-in effect's panel).
 */
export function sideChainMenu(
  tracks: readonly Source[],
  own: Id,
  plugin: Pick<PluginSlot, "sidechain" | "sidechain_pre">,
  set: (src: Id | null) => void,
  setPre: (pre: boolean) => void,
): MenuItem {
  const source = plugin.sidechain ?? null;
  const pre = !!plugin.sidechain_pre;
  const children = keySourceItems(tracks, own, source, set);
  if (source !== null) {
    children.push(
      { separator: true },
      { label: "Pre Fader", checked: pre, onClick: () => setPre(true) },
      { label: "Post Fader", checked: !pre, onClick: () => setPre(false) },
    );
  }
  const name = keyName(tracks, plugin);
  return { label: name ? `Side Chain: ${name}` : "Side Chain", children };
}
