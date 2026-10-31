// How an instrument panel lays out its sections: in one row, or in a grid that fills the panel. The
// choice is kept apart for the bottom section and for a floating window, as they suit different ones.

export type SectionLayout = "row" | "grid";
export type PanelPlace = "docked" | "floating";
export type SectionLayouts = Partial<Record<PanelPlace, SectionLayout>>;

const KEY = "fermata.instSectionLayout";

/** What a place shows until it's been chosen: the bottom section is wide and short. */
export const defaultLayout = (place: PanelPlace): SectionLayout => (place === "docked" ? "row" : "grid");

export function layoutFor(saved: SectionLayouts, place: PanelPlace): SectionLayout {
  return saved[place] ?? defaultLayout(place);
}

/** Reads the remembered choices, dropping anything malformed. */
export function parseLayouts(text: string | null): SectionLayouts {
  const out: SectionLayouts = {};
  if (!text) return out;
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return out;
  }
  if (!v || typeof v !== "object" || Array.isArray(v)) return out;
  for (const place of ["docked", "floating"] as const) {
    const l = (v as Record<string, unknown>)[place];
    if (l === "row" || l === "grid") out[place] = l;
  }
  return out;
}

export function savedLayout(place: PanelPlace): SectionLayout {
  try {
    return layoutFor(parseLayouts(localStorage.getItem(KEY)), place);
  } catch {
    return defaultLayout(place);
  }
}

export function saveLayout(place: PanelPlace, layout: SectionLayout) {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...parseLayouts(localStorage.getItem(KEY)), [place]: layout }));
  } catch {
    // Not critical: the panel opens with the default next time.
  }
}
