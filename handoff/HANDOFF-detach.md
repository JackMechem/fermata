# Handoff: detachable bottom-section panels

## Goal

Any bottom-section panel (Mixer, Editor = Piano Roll / Audio Track Editor), plus the floating
Channel EQ and built-in effect panels, can be detached into its own Tauri window (for another
monitor) and attached again. The detached window stays live (snapshots, 30 Hz ticks, shared
selection and other UI state, settings/theme/UI scale, key commands, native menu). Detached state
and window geometry persist across restarts, are clamped onto the monitors connected at the time,
windows close with the app, and a panel is never shown twice. The full task text is in the
coordinator's prompt; the brief is `../BRIEF.md`.

## State right now

- `npx tsc --noEmit -p .` passes (just run).
- `cargo build -p daw` passed (just run, with windows.rs, main.rs, menu.rs and the capability
  changes below). `cargo test -p daw`, `cargo test -p daw-engine --lib`, `npm test`, vite build
  and the headless examples have **not** been run yet.

## Done

- `src-tauri/src/windows.rs` (new): `is_panel_label`, the `PANEL_PREFIX = "panel-"` constant, and
  these commands:
  - `open_panel_window(label, title, width, height, geometry?)`: async (creating a window from a
    sync command deadlocks on Windows). It builds the window hidden, applies the physical
    `geometry {x,y,w,h}` with set_size/set_position, then shows and focuses it. If the window
    already exists it just focuses it.
  - `close_panel_window(label)`: destroys the window.
  - `focus_panel_window(label)`.
  - `broadcast_snapshot()`: takes the caller's `Webview`, serializes the snapshot once and sends
    it with `emit_str_filter("project-sync", …)` to every `WebviewWindow` target except the
    caller.
  - Also in windows.rs: `route_menu()` sends native menu ids to the focused `panel-*` window, else
    to "main", using `emit_to(webview_window(label))`. `on_window_event()` destroys the panel
    windows when "main" is destroyed, so the app exits as before and the detached state stays
    saved. There is a unit test for the label check.
- `src-tauri/src/main.rs`: `mod windows;`, `.on_window_event(windows::on_window_event)`, and the
  4 commands added at the end of `invoke_handler`.
- `src-tauri/src/menu.rs`: `on_menu_event` now calls `crate::windows::route_menu`, and the
  unused `Emitter` import is gone. The View menu gets "Mixer in Separate Window" and "Editor in
  Separate Window" after "Editors", with ids `detach:mixer` and `detach:editor` (toggles).
- `src-tauri/capabilities/default.json`: `"windows": ["main", "panel-*"]`, plus
  `core:window:allow-set-title`.
- `src/store.ts`: a new field `detached: string[]` (panel keys; the main window is the only
  writer) and an exported `subscribeState(l)`.
- `src/prefs.ts`: exported `onSettingsChange(l)` and `receiveSettings(s)` (they take another
  window's settings without saving).
- `src/detachModel.ts` (new, pure, no runtime imports):
  - keys and labels: `panelKey`, `parsePanelKey` ("eq:12" → kind eq, arg 12), `panelLabel` (the
    label is "panel-" + key; ':' is legal in labels), `keyOfLabel`;
  - the detached list: `withDetached`, `withoutDetached`, `detachedKinds`, and
    `bottomAfterHide(kind, before, detached)`, which gives back what was in the bottom section
    before the panel;
  - shared state: `SYNCED_FIELDS`, `syncPatch(prev, next)`, `syncedState`, `cleanPatch`;
  - geometry: `Rect`, `PanelMemory`, `parseMemory`, `fitToMonitors(g, monitors)` (fits the window
    onto the monitor it overlaps most, or the nearest one if none, shrinking it to fit and moving
    it inside) and `cascade`.

## Left to do (exact next steps)

1. **`tests/detach.test.ts`** (node:test, import `../src/detachModel.ts`). Test:
   - key and label round-trips;
   - `bottomAfterHide` cases: before = the same kind, "none", a detached kind, and another pane;
   - `syncPatch`: returns null when nothing changed, and only the changed fields otherwise;
   - `cleanPatch`;
   - `parseMemory` with junk and with partial data;
   - `fitToMonitors`: on a monitor, monitor gone (goes to the nearest), larger than the monitor,
     spanning two monitors, and no monitors;
   - `cascade`.
2. **`src/detach.ts`** (runtime; it must not import components, so there are no import cycles).
   It needs:
   - `ownPanel = keyOfLabel(getCurrentWindow().label)` (null in main) and
     `isMain = ownPanel === null`.
   - A `PanelKind` registry:
     ```ts
     interface PanelKind {
       title(arg, s): string;
       render(arg): ReactNode;
       shown(arg, s): boolean;
       hide(arg, s): Partial<State>;
       show(arg, s): Partial<State>;
       alive?(arg, s): boolean;
       restore?: boolean;
       size: { w; h };
     }
     ```
     Export `registerPanelKind(kind, k)`, `panelKind(kind)`, and
     `bottomPanel(pane, {title, render, size})`. `bottomPanel` gives `restore: true`,
     `shown = s.bottom === pane`, `hide = {bottom: bottomAfterHide(pane, bottomBefore, s.detached)}`
     and `show = {bottom: pane}`.
   - Module state: `remote` (a counter), `prev` (the last state seen), `bottomBefore` (the value
     `bottom` had before its latest change) and a broadcast timer.
   - `onStateChange` (passed to `subscribeState` in every window):
     - update `bottomBefore`;
     - if `remote === 0` and there are peers (main: `detached.length > 0`; a panel: always),
       `emit("ui-sync", {from: label, patch: syncPatch(prev, s)})`;
     - if the snap changed, call `invoke("broadcast_snapshot")`, throttled to one call per
       ~30 ms (trailing timer);
     - main only: when the snap changed, close panels whose `alive()` is false;
     - **guard**: for the first detached key (other than `ownPanel`) whose kind shows it in this
       window's state, `setState(hide)`, call `focus_panel_window` and return (a nested
       setState re-runs the guard; always read `getState()` fresh);
     - panel window only: once booted, if its own kind is no longer `shown` (EQ ×/Esc, FX
       closed), send `close`.
   - `receiveSnapshot(snap)` and `applySync({from, patch})`. Both run under `remote++`, and both
     ignore messages whose `from` is their own label. `applySync` uses `cleanPatch`; main drops
     `detached` from the patches it receives. It applies `themeId` with `applyTheme(id, false)`
     and `uiScale` with `applyUiScale(s, false)`, and `setState`s the rest.
   - Settings: `onSettingsChange`, then, if `remote === 0` and there are peers,
     `emit("ui-settings", {from, settings})`. The receiver calls `receiveSettings` under
     `remote++`.
   - Messages to the main window go as `emitTo("main", "panel-msg", {from, type, …})`. The
     types:
     - `hello`: main replies `emitTo(from, "ui-sync", {from:"main", patch: syncedState(state)})`
       plus `ui-settings`;
     - `attach` / `detach` / `close` with a `key`;
     - `key` with an `id`: main runs `KEY_ACTIONS[id]` through a callback from App;
     - `geometry` with a `key` and a `Rect`: main stores `memory.geometry[kind]`.
   - Main functions:
     - `detachPanel(key)`: if already detached, focus it. Otherwise `setState({...hide if shown,
       detached: withDetached})`, `saveMemory()`, then `openWindow`. `openWindow` takes the saved
       geometry for the kind, applies `cascade` by the number of open panels of the same kind,
       fits it with `fitToMonitors` against `availableMonitors()` (use `workArea`), and calls
       `invoke("open_panel_window", …)`, with try/catch.
     - `attachPanel(key)`: `setState({detached: without, ...show})`, then `close_panel_window`
       and `saveMemory`.
     - `closePanel(key)`: the same without `show`.
     - `togglePanel(key)`.
     - From a panel window, these hide the panel locally if it is shown there, then send the
       message to main.
   - Memory goes in localStorage under `fermata.detachedPanels` as
     `{open: detached keys whose kind has restore, geometry}`. Main owns it; panels send their
     geometry to main and don't write it, so this works even if localStorage weren't shared.
   - `installDetach({key})` (main) adds the subscriptions, then these scoped listeners
     (`getCurrentWebviewWindow().listen`): `panel-msg`, `ui-sync`, `ui-settings`,
     `project-sync`. Then it restores `memory.open`, and emits a full ui-sync if panels exist.
   - `installPanelWindow({menu})` (panel) adds:
     - the subscriptions;
     - `listen("tick")` → `pushTick` with the warning stripped (main already toasts it);
     - `listen("project")` → `receiveSnapshot`;
     - these scoped listeners:
       - `project-sync`, `ui-sync` and `ui-settings`;
       - `menu`, which runs `menu(id)` and otherwise does `emitTo("main", "menu", id)`;
     - keydown in the bubble phase: skip if defaultPrevented, a field has focus or a local dialog
       is open; otherwise run `commandForEvent`, preventDefault, and send `{type:"key", id}`;
     - `onCloseRequested` → await a `send({type:"attach"})`;
     - `onMoved` / `onResized` → after 400 ms, skip if minimized or maximized, else send
       `outerPosition` and `innerSize`.
     After that it `setState(kind.show(arg))` under remote, marks the window as booted, waits
     until the listeners are registered, then sends hello and does `call("get_state")` →
     `receiveSnapshot`. Use `call`, not `act`, so the snapshot isn't broadcast back.
   - `useDetached(key)` and `detachMenuItems(key)`: a separator plus "Open in Separate Window",
     or "Attach to Main Window" in the panel's own window.
3. **`src/components/DetachButton.tsx`**: a small `lb-btn detach-btn` with an inline SVG
   pop-out or pop-in icon. In the panel's own window it shows "Attach to Main Window" and calls
   `attachPanel`; elsewhere it shows "Open in Separate Window" and calls `detachPanel`.
4. **`src/panelKinds.tsx`**: register these kinds.
   - mixer: `bottomPanel`, rendering `<Mixer/><EqEditor/><FxWindows/>`, titled
     "Mixer — <project>", size about 1100×560.
   - editor: `bottomPanel`, rendering `<Editor/>`. Title: "Piano Roll — <region or track>",
     "Audio Track Editor — <track>", or "Editor — <project>". The region/track lookup is the
     same as in Editor.tsx.
   - eq: renders `<div className="panel-host"><EqEditor/></div>`.
     - `shown = eqEditor === id`; `hide = {eqEditor: null}`; `show = {eqEditor: id}`;
     - `alive = !!eqOf(project, id)`; restore false.
   - fx: renders `<div className="panel-host"><FxWindows/></div>`.
     - `shown = fxPanels.includes(uid)`; hide and show change `fxPanels`;
     - `alive = !!findFxSlot`; the title is `<effect> — <track>`.
     - This is a stop-gap until the other agent's self-contained effect panel component lands.
5. **`src/components/DetachedWindow.tsx`**: the panel window's root.
   - It calls `installPanelWindow({menu: id => editorMenuCommand(id) || pianoRollMenu(id)})`.
   - It sets the title from `kind.title` (`getCurrentWindow().setTitle` + `document.title`) and
     prevents the default context menu.
   - It renders `kind.render(arg)`, then:
     - the dialogs `NewTrackDialog`, `SettingsDialog`, `BounceDialog`, `BounceInPlaceDialog`,
       `ProjectSettingsSheet` and `GroupSheet`, by local `dialog` and `groupSheet`;
     - `<MenuHost/>` and `<Toast/>`.
   - Before the snapshot arrives it shows the "Loading…" `.loading` text.
6. **Wiring (small edits):**
   - `main.tsx`: `import "./panelKinds"`, then render `ownPanel ? <DetachedWindow/> : <App/>`.
   - `App.tsx`:
     - `export function Toast`;
     - `listen("project")` → `receiveSnapshot`;
     - `listen("menu")` → **`getCurrentWebviewWindow().listen("menu")`**. This is required:
       `emit_to` still reaches listeners with an Any target;
     - `installDetach({key: id => { if (!getState().dialog) KEY_ACTIONS[id]?.(); }})` in the
       mount effect, with cleanup;
     - in the `default` branch of `handleMenu`: `else if (id.startsWith("detach:"))
       togglePanel(id.slice(7))`.
   - Add a `DetachButton` at the end of `local-right`:
     - in Panels.tsx (the Mixer; its `local-right` is empty now);
     - in PianoRoll.tsx and AudioEditor.tsx;
     - in Editor.tsx's empty-state bar (add a `local-right` div);
     - before `.eq-close` in the EQ titlebar in Eq.tsx (`panel="eq:<id>"`).
   - Add `...detachMenuItems("editor")` at the end of PianoRoll's `viewItems()` and of
     `audioEdit.ts`'s `viewMenu()`.
   - Optional: an "Open Panel in Separate Window" item in `fxHost.ts` `slotMenu` for built-in
     effects. Note that the other agent edits fxHost.ts.
   - Append to `styles.css`:
     - `.panel-window` (100vh flex column, `var(--bg)`);
     - `.panel-window > .mixer, > .editor` with `flex: 1; height: auto; border-top: none`;
     - `.panel-host` (flex 1, overflow auto);
     - `.panel-host > .eq-window` with `position: static; width: auto; border-radius: 0;
       box-shadow: none`, and `cursor: default` on its titlebar;
     - `.detach-btn` colours, using `var(--dim)` and `var(--text)`.
7. Docs: add a Detached windows section to `docs/USER_GUIDE.md` and the multi-window design
   (events, authority, sync) to `docs/ARCHITECTURE.md`.
8. Checks:
   - `npx tsc --noEmit -p .`, `npm test`, `npx vite build --outDir <sandbox>/dist-check` (then
     delete it);
   - `cargo build -p daw`, `cargo test -p daw`, `cargo test -p daw-engine --lib`;
   - the headless examples listed in the brief's Addendum.

## Design decisions (and why)

- **Event targeting.** Tauri 2 delivers an event to any JS listener registered with an `Any`
  target, which is what plain `listen()` registers, whatever target the emitter chose. So every
  event meant for one window ("menu", "project-sync", "ui-sync", "ui-settings", "panel-msg") is
  listened for with `getCurrentWebviewWindow().listen` (a WebviewWindow target), and the
  emitters use `emit_to` / `emitTo` / filters. Global events (`tick`, `project`) still use plain
  `listen`.
- **Snapshots.**
  - `act()` updates only the window that ran the command.
  - A store subscription notices any local `snap` change (including the direct
    `setState({snap})` calls in files.ts, ops.ts, Eq.tsx and others) and calls
    `broadcast_snapshot` (throttled), which sends the snapshot to every other window. The main
    window never gets the same snapshot twice.
  - Snapshots from the engine's `project` event are marked remote, so they aren't re-broadcast.
  - With no panels detached the main window does no extra work.
- **The main window is the authority** for `detached`, the persisted memory and
  opening/closing windows. Panels ask it to.
- **Guard.** Each window hides a detached panel if its own state tries to show it (X/E keys, the
  EQ thumbnail), then focuses that panel's window. So X with a detached Mixer brings its window
  to the front.
- **Closing.** The OS close button attaches the panel back where it was (the bottom section; a
  floating EQ or effect window in main). The panel's own × or Esc closes it. Quitting destroys
  the panels from Rust without attaching, so they reopen at launch (Mixer and Editor only; EQ and
  FX are tied to a project's content and aren't restored).
- **Geometry.** It is kept per kind, in physical pixels: the outer position and inner size.
  Minimized and maximized states aren't saved. Native Wayland ignores positions. The app forces
  `GDK_BACKEND=x11` by default, so XWayland placement normally works.
- **Native menu bar.** Tauri gives new windows the app menu on Linux and Windows, so panel
  windows get a menu bar with working accelerators. `route_menu` sends items to the focused
  panel first. `w.remove_menu()` would remove the bar if that is unwanted.
- **Known limitation.** There is one analyzer (`set_analyzer`) per session, so a detached EQ and
  a floating EQ on another strip fight over it.

## Files touched so far

- New: `src-tauri/src/windows.rs`, `src/detachModel.ts`, `HANDOFF-detach.md`.
- Modified:
  - `src-tauri/src/main.rs`, `src-tauri/src/menu.rs`;
  - `src-tauri/capabilities/default.json`;
  - `src/store.ts` (the `detached` field and `subscribeState`);
  - `src/prefs.ts` (`onSettingsChange` and `receiveSettings`).
