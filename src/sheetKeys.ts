// Key commands while a sheet is open. Every sheet (Dialogs.tsx `Sheet`, Settings' frame, the
// name sheet, the Project Chooser) registers here with `useSheetKeys`, and one keydown listener,
// installed before anything else listens (main.tsx), applies the rule:
//
// - Keys typed inside a sheet stay in it: its fields get them, and `sheetKey` (Form.tsx) stops
//   them before the window's key commands.
// - Keys pressed with the focus outside the open sheets (on the page, or still on the editor
//   that had it, since clicking a slider or a button doesn't always move the focus) never reach
//   the app: Esc closes the top sheet, Return presses its default button, Tab still moves the
//   focus, and a modal sheet drops everything else, the focused editor's own keys included.
// - Controller Assignments is non-modal, like Logic's window: Learn Assignment (⌘L), its own
//   key and the transport keys still work while it is open; editing keys (Delete, Cut, Paste…)
//   are dropped. The pure rule is sheetKeysModel.ts.
//
// Keys passed on from a panel's window, and capture-phase listeners that see a key before this
// one can stop it, ask `sheetBlocks`.
import { useEffect, useRef, type RefObject } from "react";
import { commandForEvent } from "./keymap";
import { heldBack, outsideKey, type SheetMode } from "./sheetKeysModel";

interface OpenSheet {
  el: RefObject<HTMLElement | null>;
  mode: SheetMode;
  close: () => void;
}

/** The open sheets, the top one last. */
const open: OpenSheet[] = [];

/** Whether a sheet is open in this window. */
export const sheetOpen = () => open.length > 0;

/** Whether key command `id` is held back by the sheets open in this window. */
export function sheetBlocks(id: string | null): boolean {
  return heldBack(
    open.map((s) => s.mode),
    id,
  );
}

/** Registers a sheet while it is shown; `ref` is the sheet's box (not its backdrop). */
export function useSheetKeys(ref: RefObject<HTMLElement | null>, onClose: () => void, mode: SheetMode = "modal") {
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });
  useEffect(() => {
    const s: OpenSheet = { el: ref, mode, close: () => close.current() };
    open.push(s);
    return () => {
      const i = open.indexOf(s);
      if (i >= 0) open.splice(i, 1);
    };
  }, [ref, mode]);
}

/** The sheet's default button (`push default` in its foot), if it can be pressed. */
export function defaultButton(sheet: HTMLElement | null): HTMLButtonElement | null {
  const all = sheet?.querySelectorAll<HTMLButtonElement>(".sheet-foot button.push.default");
  const b = all?.length ? all[all.length - 1] : null;
  return b && !b.disabled ? b : null;
}

let grab: ((e: KeyboardEvent) => void) | null = null;

/** Hands every key press to `f`, ahead of everything else, until the returned function is
 *  called: Settings › Key Commands taking a new key. */
export function grabKeys(f: (e: KeyboardEvent) => void): () => void {
  grab = f;
  return () => {
    if (grab === f) grab = null;
  };
}

function onKey(e: KeyboardEvent) {
  if (grab) {
    grab(e);
    e.stopImmediatePropagation();
    return;
  }
  // An open menu (a sheet's pop-up) has the keys.
  if (!open.length || document.querySelector(".menu-layer")) return;
  const t = e.target instanceof Node ? e.target : null;
  if (t && open.some((s) => s.el.current?.contains(t))) return;
  const top = open[open.length - 1];
  const what = outsideKey(
    open.map((s) => s.mode),
    e.key,
    commandForEvent(e),
  );
  if (what === "pass") return;
  e.preventDefault();
  e.stopImmediatePropagation();
  if (what === "close") top.close();
  else if (what === "default") defaultButton(top.el.current)?.click();
}

/** Installs the listener; main.tsx calls it first, so it sees keys before any other. */
export function installSheetKeys() {
  window.addEventListener("keydown", onKey, true);
}
