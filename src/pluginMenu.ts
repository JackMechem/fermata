import { BUILTIN_SYNTH, type PluginDesc } from "./api";
import type { MenuItem } from "./components/Menu";
import type { FxInfo, InstInfo } from "./fx";
import { getState } from "./store";

/** The built-in effects as menu items, one submenu per category (Dynamics, Reverb, ...). */
export function builtinFxItems(list: FxInfo[], pick: (id: string) => void, current?: string): MenuItem[] {
  const cats = new Map<string, FxInfo[]>();
  for (const f of list) {
    if (!cats.has(f.category)) cats.set(f.category, []);
    cats.get(f.category)!.push(f);
  }
  return [...cats.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([cat, fs]) => ({ label: cat, children: fs.sort((a, b) => a.name.localeCompare(b.name)).map((f) => ({ label: f.name, checked: f.id === current, onClick: () => pick(f.id) })) }));
}

/**
 * The built-in instruments as menu items: the house ones ("Fermata") in the menu itself, others in
 * a submenu per group (Vintage Synths, ...).
 */
export function builtinInstrumentItems(list: InstInfo[], pick: (id: string) => void, current?: string): MenuItem[] {
  const item = (i: InstInfo): MenuItem => ({ label: i.name, checked: i.id === current, onClick: () => pick(i.id) });
  const groups = new Map<string, InstInfo[]>();
  for (const i of list) {
    if (!groups.has(i.category)) groups.set(i.category, []);
    groups.get(i.category)!.push(i);
  }
  const house = list.filter((i) => i.category === "Fermata").map(item);
  const others = [...groups.entries()].filter(([g]) => g !== "Fermata").sort((a, b) => a[0].localeCompare(b[0]));
  return [...house, ...others.map(([label, is]) => ({ label, children: is.map(item) }))];
}

function category(p: PluginDesc): string {
  const parts = p.subcategories.split("|").filter((s) => s && s !== "Fx" && s !== "Instrument" && s !== "Mono" && s !== "Stereo");
  return parts[0] ?? (p.is_instrument ? "Synth" : "Other");
}

function byName(a: PluginDesc, b: PluginDesc) {
  return a.name.localeCompare(b.name, undefined, { numeric: true });
}

function vendorTree(list: PluginDesc[], pick: (id: string) => void, current?: string): MenuItem[] {
  const vendors = new Map<string, PluginDesc[]>();
  for (const p of list) {
    const v = p.vendor || "Unknown";
    if (!vendors.has(v)) vendors.set(v, []);
    vendors.get(v)!.push(p);
  }
  return [...vendors.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([vendor, ps]) => ({
      label: vendor,
      children: ps.sort(byName).map((p) => ({ label: p.name, checked: p.id === current, onClick: () => pick(p.id) })),
    }));
}

/**
 * Builds the slot pop-up menu: "No Plug-in", then Logic-style category submenus, then the raw
 * VST3 list grouped by manufacturer.
 */
export function pluginMenu(
  plugins: PluginDesc[],
  opts: { instrument: boolean; current?: string; pick: (id: string) => void; remove: () => void },
): MenuItem[] {
  const pool = plugins.filter((p) => p.is_instrument === opts.instrument);
  const items: MenuItem[] = [{ label: "No Plug-in", onClick: opts.remove, checked: !opts.current }, { separator: true }];
  if (opts.instrument) {
    // Before the list arrives, the synth every track can always have.
    const list = getState().instList;
    const fallback: MenuItem[] = [{ label: "Fermata Synth", checked: opts.current === BUILTIN_SYNTH, onClick: () => opts.pick(BUILTIN_SYNTH) }];
    items.push(...(list.length ? builtinInstrumentItems(list, opts.pick, opts.current) : fallback));
    items.push({ separator: true });
  } else if (getState().fxList.length) {
    items.push({ label: "Fermata", header: true }, ...builtinFxItems(getState().fxList, opts.pick, opts.current), { separator: true });
  }
  const cats = new Map<string, PluginDesc[]>();
  for (const p of pool) {
    const c = category(p);
    if (!cats.has(c)) cats.set(c, []);
    cats.get(c)!.push(p);
  }
  for (const [cat, ps] of [...cats.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
    const children =
      ps.length > 30
        ? vendorTree(ps, opts.pick, opts.current)
        : ps.sort(byName).map((p) => ({ label: p.name, hint: p.vendor, checked: p.id === opts.current, onClick: () => opts.pick(p.id) }));
    items.push({ label: cat, children });
  }
  if (pool.length > 0) {
    items.push({ separator: true });
    items.push({ label: "VST3", children: vendorTree(pool, opts.pick, opts.current) });
  } else {
    items.push({ label: opts.instrument ? "No VST3 instruments found" : "No VST3 effects found", disabled: true });
  }
  return items;
}
