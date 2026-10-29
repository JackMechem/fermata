// Keys typed into a plug-in's editor window that the plug-in doesn't use arrive here (from
// vst3-host's key forwarder, src-tauri/src/windows.rs; macOS only for now) and are dispatched
// in the main window as if pressed there, as Logic does: key commands follow the user's keymap
// and Musical Typing plays. ⌘ shortcuts of the native menu never come: the menu takes them first.
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

/** vst3_host::keys::KeyInfo. */
export interface ForwardedKey {
  up: boolean;
  key: string;
  code: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  repeat: boolean;
}

/** The KeyboardEvent a forwarded key becomes. */
export function keyEventInit(k: ForwardedKey): [type: "keydown" | "keyup", init: KeyboardEventInit] {
  const { up, key, code, shiftKey, ctrlKey, altKey, metaKey, repeat } = k;
  return [up ? "keyup" : "keydown", { key, code, shiftKey, ctrlKey, altKey, metaKey, repeat, bubbles: true, cancelable: true }];
}

/** Listens for forwarded keys (main window, once). */
export function installPluginKeys(): Promise<() => void> {
  // On this window only, like the "menu" event.
  return getCurrentWebviewWindow().listen<ForwardedKey>("plugin-key", (e) => {
    const [type, init] = keyEventInit(e.payload);
    // At the element with the main window's focus, so its own key handlers (an editor's) run
    // first, as for a key pressed there. A focused text field ignores it, as onKey does.
    (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent(type, init));
  });
}
