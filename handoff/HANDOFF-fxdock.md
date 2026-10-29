# Handoff: fxdock (built-in effect panels: overlap fix, resizable, docked by default)

## Goal
1. Fix the Loudness Meter's units overlapping the big readouts ("LUFS" over "−22.2", "LU" over "1.9"), and check the other built-in panels and the Channel EQ for the same problem.
2. Let effect panels be resized by their edges and corner. Graphs grow with the panel, controls keep their size, each panel has a sensible minimum, and each effect type's last size is remembered in localStorage.
3. Dock panels in the bottom section by default, like the Mixer: tabs, a float/dock control and a close button. Add the setting Settings › General › "Open Effect Panels: Docked in the bottom section / Floating" (default Docked). VST3 editors stay native windows.
4. Undo, presets, side-chain pop-ups, meters and Esc must still work in every mode. Update docs/USER_GUIDE.md.

Another agent is building OS-window detaching for bottom panels, so the panel content stays self-contained and keyed (see Seams).

## State right now
- `npx tsc --noEmit -p .` **passes** (it was run at handoff).
- `cargo build -p daw` and `cargo test -p daw` passed after the Rust change: 3 settings tests.
- `npm test` and `vite build` have **not** been run since the changes. `tests/fxDock.test.ts` is **not written yet**.
- **CSS for the new structure is NOT written yet**, so the UI is not visually right yet (see "Left").

## Done (file by file)
- `src-tauri/src/settings.rs`: `UiPrefs.fx_panels: String` (default `"docked"`) is added at the end of the struct, and the test asserts the default.
- `src/prefs.ts`: `UiPrefs.fx_panels?: "docked" | "floating"`.
- `src/fxDock.ts` (new, pure, no runtime imports so node:test can load it):
  - Panel keys `fx:<uid>` / `eq:<track>`, with `fxKey`, `eqKey` and `parseKey`.
  - `DockPanels { open (insertion order = tab order), floating (z-order, frontmost last), active, ret }`, and `DockState = DockPanels + bottom`.
  - Transitions: `openPanel` (only one Channel EQ at a time), `closePanel`, `togglePanel`, `floatPanel`, `dockPanel`, `focusPanel`, `selectDocked`, `prunePanels`, `checkBottom`.
  - Queries: `isShown`, `dockedPanels`, `escTarget` (the frontmost floating panel, else the docked one on screen).
  - Sizes: `clampSize`, `resizeRect(start, edge, dx, dy, min, max)` (edges e/w/s/se/sw; west edges keep the right side fixed), `keepOnScreen`, `parseSizes`, and `DEFAULT_WIDTH` (compressor 628, limiter 458, channel-eq 820) for windows that have never been resized.
- `src/fxDockState.ts` (new): the store glue.
  - `apply()` writes `fxDock` and `bottom` and keeps the legacy `fxPanels` and `eqEditor` derived from them.
  - Exports `openPanel`, `closePanel`, `togglePanel`, `floatPanel`, `dockPanel`, `focusPanel`, `selectDocked`, `prunePanels`, `escTarget`, `usePanelShown`, `toggleEqPanel`, `panelMode()` (reads the setting) and `savedPanelSize` / `savePanelSize` (localStorage `fermata.fxPanelSizes`, in try/catch).
- `src/store.ts`:
  - `BottomPane` gains `"fx"`.
  - `PaneSizes.fx` (default 420) is added at the end.
  - `State.fxDock` is added at the end, with its default.
- `src/components/Splitter.tsx`: `fx` default 420 and limits [160, 900].
- `src/fxHost.ts`: `openFxPanel`, `closeFxPanel` and `toggleFxPanel` now delegate to `fxDockState`.
- `src/components/fx/FxPanel.tsx` (new). These are the seam for the detach agent; everything works from a panel key:
  - `PanelContent({panelKey})` (fx → `FxPanelContent`, eq → `EqPanel`), `PanelLead` (power), `PanelTools` (presets + side chain).
  - `usePanelTitle(key)` gives `{name, track, type}`; `panelExists(project, key)`.
  - Also `FxPower`, `FxTools`, `FxPanelContent` (moved from FxWindow: values, meters, set/touch, `watch_builtin`), `GenericPanel` (moved here), and `DockGlyph` / `FloatGlyph`.
- `src/components/fx/FxWindow.tsx` (rewritten):
  - `FxWindows` renders the floating panels, prunes panels whose instance is gone (for all open panels, docked ones too), and owns the single Esc handler.
  - `FloatingPanel`: drag by the title bar, resize handles `.fx-resize.{e,w,s,se,sw}` driven by pointer capture, and a dock button.
  - The minimum is measured live: width = the window at `width: min-content`, height = `height: auto` at the current width. The size is saved per effect type on mouse-up.
  - Positions are remembered per key in a module map.
- `src/components/fx/FxDock.tsx` (new): the bottom-pane component.
  - A `local-bar` with tabs (`.fx-tab`, with a × on each), power/presets/side chain for the active tab, a float button (`lb-btn`) and a close button, then `PanelContent`.
- `src/components/Eq.tsx`:
  - `EqEditor` is replaced by `EqPanel({track})` (toolbar, graph, bands, hint; no window chrome and no Esc handler).
  - The graph height comes from `wrap.clientHeight`, and mouse maths uses `rect.height`.
  - `EqThumb` uses `usePanelShown(eqKey)` and `toggleEqPanel`.
- `src/App.tsx`:
  - Removed the `EqEditor` import and its `<EqEditor />`.
  - Added the `FxDock` import and `{s.bottom === "fx" && <FxDock />}` after the Editor.
  - Added `"--fx-h"` to the pane CSS vars.
- `src/components/ChannelStrip.tsx`: the slot highlight now uses `usePanelShown(fxKey(uid))`.
- `src/components/Settings.tsx`: GeneralPane gains a Section "Plug-in Windows" with the "Open Effect Panels:" Select.
- `src/components/fx/Loudness.tsx`:
  - The readouts now use `<Reading value unit/>`, which renders `span.loud-read-num > .loud-read-value + .loud-read-unit`; Peak L/R is wrapped in `.loud-read-num` too.
  - The History canvas now measures both width and height of its parent box.

## Partly done / left
1. **CSS (none written yet). Append it at the end of `src/styles.css`.**
   - `.loud-read`: make it `display:flex; flex-direction:column; justify-content:center` instead of the grid (the grid with `align-items: baseline` is the WebKit overlap bug).
   - `.loud-read-num {display:flex; align-items:baseline; gap:5px; white-space:nowrap}`, with value and unit `flex:none` and the value `font-variant-numeric: tabular-nums`.
   - `.loud-history {flex:1 1 auto; min-height:84px; position:relative}` and its canvas `position:absolute; inset:0`.
   - Floating window:
     - `.fx-window {display:flex; flex-direction:column; max-height:calc(100vh - 16px)}`, with the titlebar `flex:none`.
     - `.fx-window > .fx-body, .fx-window > .eq-panel {flex:1 1 auto; min-height:0}`.
     - `.fx-title-gap {margin-left:auto}`, `.fx-window .eq-close {margin-left:0}`, and a `.fx-mode` button style (like `.eq-close`).
   - `.fx-body {display:flex; flex-direction:column; overflow:auto}` and `.fx-body > * {flex:1 0 auto; width:100%; margin-inline:auto}`.
   - `.eq-panel {display:flex; flex-direction:column; overflow:auto}`, `.eq-graph {flex:1 1 auto; height:auto; min-height:250px; position:relative}` and `.eq-graph canvas {position:absolute; inset:0}`.
   - Resize handles: `.fx-resize` positions and cursors. e/w are 6px strips below the 32px title bar, s is a 6px strip, se/sw are 12px corners; cursors ew/ns/nwse/nesw.
   - Dock:
     - `.fx-dock {height:var(--fx-h,420px); flex:none; display:flex; flex-direction:column; border-top:1px solid var(--edge); background:var(--bg)}`.
     - `.fx-dock > .fx-body, .fx-dock > .eq-panel {flex:1 1 auto; min-height:0}`.
     - `.fx-tabs`, `.fx-tab` (`.on` = `var(--btn-active)` + `btn-shadow`), `.fx-tab-name`, `.fx-tab-track` (faint, ellipsis), `.fx-tab-close` (visible on hover or on the active tab), `.fx-dock-tools` (border-left separator).
   - Per panel:
     - Compressor: `.dyn-comp {min-width:600px; max-width:1100px}`.
     - Limiter: `.dyn-lim {min-width:430px; max-width:820px}` and `.dyn-lim-body {flex:1 1 auto}`.
     - Gate/DeEsser/Generic: `.fx-panel, .fx-generic {max-width:1000px}`, `.fx-group {flex:1 1 auto}`, `.fx-group-body {justify-content:space-around}`, `.fx-meters {align-items:stretch}`, `.fx-meter-bar {flex:1 1 auto}`.
     - Reverb/Delay: `.sp-main {flex:1 1 auto}`, `.sp-graph {height:auto; min-height:176px}`, `.sp-group {flex:1 1 auto; justify-content:space-around}`.
     - Tuner: `.tuner-face {flex:1 1 auto; display:flex; flex-direction:column}` and `.tuner-dial-box {flex:1 1 auto; min-height:112px; position:relative}` (canvas absolute).
   - Fix the empty `.dyn-seg button.on {}` rule, which lost its body; give it `background:var(--btn-active); color:var(--text); box-shadow:var(--btn-shadow)` like `.dyn-circuits button.on`.
2. **Panel reflow code still to do:**
   - `common.tsx`:
     - FxKnob: use `minWidth` instead of `width`.
     - FxMeter: use `minHeight: height` instead of `height`.
   - `Compressor.tsx`:
     - Make `.dyn-top` grow: CSS `flex:1 1 0; min-height:142px`, plus a ResizeObserver on the row.
     - TransferGraph size = `max(136, min(h - 6, round(0.4*w)))`, with `align-self:center`.
     - LevelBar `grow` prop: the bar gets `minHeight: length` and `flex:1`, and a ResizeObserver sets `--dyn-bar-len` on the bar element. Take that variable out of the inline style when `grow` is set.
     - Lift the `.dyn-vu` max-height. Use `grow` in Compressor and Limiter.
   - `Tuner.tsx`: wrap the canvas in `.tuner-dial-box` and measure w and h. Draw with scale `k = max(1, min(H/112, w/360))`: transform `dpr*k`, logical width `w/k`, logical height 112, centred vertically.
3. `tests/fxDock.test.ts` (node:test, import `../src/fxDock.ts`). Cover:
   - open docked/floating, toggling, close switching to another tab, and the last close restoring `ret`
   - float/dock, one EQ at a time, `escTarget` order, prune, `checkBottom`
   - `resizeRect` west/east/south and the clamps, `clampSize` when min > max, `parseSizes` with bad input
4. `docs/USER_GUIDE.md`:
   - Built-in effects › "The panel" bullet: docked by default with tabs, float/dock, resize, the remembered size, and Esc closing the frontmost.
   - Channel EQ section: it opens the same way.
   - Settings › General table: add the row "Open Effect Panels | Docked in the bottom section; Floating | Docked".
5. Run `npx tsc --noEmit -p .`, `npm test`, and `npx vite build --outDir <sandbox scratch dir>` (then delete that folder). `cargo build -p daw` and `cargo test -p daw` passed already.

## Design decisions
- The bottom section shows one thing at a time. Opening a docked panel switches it to "fx" and remembers the previous pane in `ret`; closing the last docked panel restores it.
- Pressing X or E while the dock is showing just switches the pane. The docked panels stay open but hidden, and clicking their slot brings the dock back with all its tabs.
- A slot click toggles a panel: it closes it if it is on screen, and otherwise opens or shows it.
- Only one Channel EQ panel is open at a time, because the engine's analyzer follows one strip.
- Esc closes the frontmost floating panel first, then the docked one on screen.
- Growing rows use CSS-driven sizes (`flex` with `min-height` = today's fixed size), and canvases are absolutely positioned and measured with a ResizeObserver. That way content never feeds back into layout and a panel can always shrink again. An unresized floating window keeps today's natural size.
- The minimum floating size is measured from the content, not hard-coded. When the box is too small, the body scrolls instead of the content overlapping.

## Seams for the detach agent
- `PanelContent`, `PanelLead`, `PanelTools` and `usePanelTitle` in `components/fx/FxPanel.tsx` render any panel from a key and fill their container.
- The dock state lives in `fxDock.ts` / `fxDockState.ts`, in `State.fxDock` at the end of `State`.
- App changes: one `FxDock` line in the bottom pane, plus `--fx-h`.

## Visual checks still owed (coordinator)
- Each panel docked and floating.
- Resize from every edge and the corner.
- Tabs, float/dock, close.
- The preset sheet opening from the dock bar is not clipped.
- Loudness units at values like −100.0 and −∞.
