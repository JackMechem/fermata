// Edit › Snap Edits to Zero Crossings (Logic): a project setting. With it on, the engine moves
// splits and trims of audio regions to the nearest zero crossing (session/zero_crossings.rs).
import { act } from "./api";
import type { MenuItem } from "./components/Menu";
import { keyHint } from "./keymap";
import { getState, setState } from "./store";

export function toggleZeroCrossings() {
  const on = !getState().snap?.project.snap_zero_crossings;
  act("set_snap_zero_crossings", { on });
  setState({ toast: { text: `Snap Edits to Zero Crossings ${on ? "on" : "off"}`, at: Date.now(), ms: 1500 } });
}

/** The Edit menu item, with its tick. */
export function zeroCrossingsItem(): MenuItem {
  return { label: "Snap Edits to Zero Crossings", hint: keyHint("zero-crossings"), checked: !!getState().snap?.project.snap_zero_crossings, onClick: toggleZeroCrossings };
}
