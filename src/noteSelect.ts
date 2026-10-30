// Edit › Select in the MIDI editors: with the Piano Roll or the Event List focused, Logic applies
// the Select commands to its notes and events rather than to the regions. Those that make sense
// for notes are here; the rest (Overlapped, Similar, All Following of Same Track) stay with the
// regions. tests/noteSelect.test.ts covers this file.

/** An event as the Select commands see it: a note (pitch, velocity, muted) or a controller. */
export interface SelectItem {
  start: number;
  sel: boolean;
  /** Events that are "equal" share this: a note's pitch, a controller's kind and value. */
  equal: string;
  /** A note's velocity, which the Piano Roll colours it by. */
  velocity?: number;
  muted?: boolean;
}

/** The Select commands a MIDI editor takes (keymap.ts and menu.rs ids). */
export const EDITOR_SELECT_COMMANDS = ["sel-following", "sel-inside-locators", "sel-muted", "sel-same-color", "sel-equal", "sel-invert"] as const;

export function isEditorSelect(id: string): boolean {
  return (EDITOR_SELECT_COMMANDS as readonly string[]).includes(id);
}

/**
 * Which events command `id` selects, one flag per item, or null for a command the editor
 * doesn't take. `playhead` is where All Following starts with nothing selected; `locators` are
 * the cycle's ends (null when not set).
 *
 * - All Following: every event from the first selected one on, or from the playhead.
 * - Inside Locators: every event starting between the locators.
 * - Muted: the muted notes.
 * - Same Colored: the notes with a selected note's velocity (the Piano Roll's colour).
 * - Equal: the events with a selected event's pitch, or controller and value.
 * - Invert Selection.
 *
 * Same Colored and Equal need a selection; without one, nothing changes.
 */
export function selectEvents(id: string, items: SelectItem[], ctx: { playhead: number; locators: { start: number; end: number } | null }): boolean[] | null {
  const selected = items.filter((n) => n.sel);
  switch (id) {
    case "sel-following": {
      const from = selected.length ? Math.min(...selected.map((n) => n.start)) : ctx.playhead;
      return items.map((n) => n.start >= from);
    }
    case "sel-inside-locators": {
      const l = ctx.locators;
      if (!l || l.end <= l.start) return items.map((n) => n.sel);
      return items.map((n) => n.start >= l.start && n.start < l.end);
    }
    case "sel-muted":
      return items.map((n) => !!n.muted);
    case "sel-same-color": {
      const v = new Set(selected.flatMap((n) => (n.velocity === undefined ? [] : [n.velocity])));
      return v.size ? items.map((n) => n.velocity !== undefined && v.has(n.velocity)) : items.map((n) => n.sel);
    }
    case "sel-equal": {
      const eq = new Set(selected.map((n) => n.equal));
      return eq.size ? items.map((n) => eq.has(n.equal)) : items.map((n) => n.sel);
    }
    case "sel-invert":
      return items.map((n) => !n.sel);
    default:
      return null;
  }
}
