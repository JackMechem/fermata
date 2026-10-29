# Fermata: shared brief for feature agents

Fermata is a cross-platform DAW modelled on Logic Pro. It uses Rust (engine + VST3 host), Tauri 2
and React 19 + TypeScript (Vite). Several agents are working **in parallel**, each on its own
copy of the same baseline. The coordinator 3-way merges every agent's copy back into the main
tree, so keep your diff easy to merge.

## Your sandbox

- Work **only** inside your own directory, `scratchpad/agents/<name>/`; your prompt gives the
  absolute path. Do not touch `/home/jack/Projects/daw` (the user's live repo, with a dev server
  watching it), other agents' directories, `agents/base`, or `scratchpad/wt`.
- There is no usable git history. Don't run git.
- Build and check from your directory:
  - `nix develop /home/jack/Projects/daw -c sh -c 'CARGO_TARGET_DIR=<your dir>-target CARGO_BUILD_JOBS=6 cargo build -p daw'`
    builds the whole app, including the Tauri crate.
  - `... cargo test -p daw-engine --lib` runs the unit tests.
  - `... cargo run -q -p daw-engine --example <name> -- <args>` runs the headless examples.
  - `npx tsc --noEmit -p .` type-checks the frontend (node_modules is symlinked).
  - Prefix the nix command with `2>/dev/null` if its "dirty tree" warning gets in the way.
- **Never launch the GUI app.** It shares the user's desktop, and a compositor crash already
  happened once today. Verify with unit tests, headless examples and `tsc` instead. The
  coordinator does the visual checks.
- Headless engine tests use the pattern in `crates/engine/examples/stack_check.rs`: a `Session`
  with ALSA `alsa:null` output, `s.dir` pointed at a scratch folder (never `~/Music`), audio
  from `/tmp/claude-1000/-home-jack-Projects-daw/bba4e424-725b-47eb-bdc1-d6a68ab39484/scratchpad/ImportTest.fermata/Audio Files/tone44.wav`, and the cycle turned on so
  playback loops. The null device has no clock and runs flat out. Sleep about 60 ms after
  `stop()` before calling `play()` again.

## Architecture, in short

- `crates/engine/src/model.rs` holds the saved project document (serde). **Every new field gets
  `#[serde(default)]`** so old projects still load.
- `crates/engine/src/dsp.rs` is the real-time engine. `Engine::process` runs on the audio
  thread: it must **not allocate, lock or block**. Changes arrive as `Cmd`s, and replaced
  objects go back as `Garbage` to be dropped elsewhere. Tracks are processed in the engine's
  list order, and subtracks write into a summing stack's `sum_l/sum_r` through `out_idx`, which
  `resolve_routes()` keeps up to date.
- `crates/engine/src/session.rs` is the non-real-time controller. It owns the `Project` and
  turns edits into `Cmd`s.
  - `regions_dsp()` builds a track's playback list. Newer regions hide the older audio they
    overlap.
  - `track_params()` works out what the engine plays for a track, with its stacks applied.
    `push_all_params()` and `sync_tracks()` (normalize the tree, send `Order(process_order())`,
    then the params) resend it.
  - Undo: `checkpoint()` snapshots the project and `restore()` diffs it back into the engine.
    **Anything new you send to the engine must also be restored by `restore()`**, and by
    `build_track()` / `rebuild_graph()` for project load.
- `src-tauri/src/commands.rs` holds the Tauri commands. The `edit()`, `try_edit()` and
  `edit_made()` wrappers record an undo step. **JS passes arguments in camelCase**
  (`fadeIn` reaches Rust as `fade_in`). Register new commands in `src-tauri/src/main.rs`.
  The native menu is `src-tauri/src/menu.rs`, which emits ids that `handleMenu` in
  `src/App.tsx` handles.
- Frontend:
  - `src/api.ts` has the types mirroring the model, plus `act()` (stores the returned
    snapshot) and `actMade()`.
  - `src/store.ts` is the global UI state. `src/ops.ts` has the track and region operations and
    the right-click menus (`trackMenu`, `regionMenu`, `laneMenu`).
  - `src/layout.ts` computes track heights and depth. `src/components/Arrange.tsx` is the
    canvas arrange area; `TrackHeaders.tsx`, `ChannelStrip.tsx`, `Panels.tsx` (Inspector,
    Mixer, Library, Browser), `Dialogs.tsx` and `Editor.tsx` (bottom pane) do what their names
    say.
  - Themes are CSS variables in `src/themes.ts`, plus `src/theme.ts` for the canvas. Styles
    are in `src/styles.css`.
- Read the files you'll change before designing anything. `README.md` describes the features.

## Rules that make merging work

- Keep changes focused on your feature. Don't reformat, reorder, rename or "clean up" code you
  don't need to touch.
- Add **new struct fields at the end** of a struct, **new commands at the end** of
  `commands.rs` and of the `invoke_handler` list, new CSS at the end of `styles.css`, and new
  menu entries next to related ones.
- Prefer putting substantial new code in **new files**: Rust modules, `src/<feature>.ts`,
  `src/components/<Feature>.tsx`. Then wire them in with small edits.
- Match the surrounding style. Doc comments go on public items, and comments explain *why*.
  The code must stay maintainable by humans.
- The UI must not look AI-generated. Follow the existing Logic-like visual language and use the
  existing CSS variables.
- Undo: every user edit goes through a checkpointed command, so it can be undone.

## When you finish

Reply with:
1. the files changed and added;
2. a design summary: model fields, `Cmd`s, commands and their argument names, and UI entry
   points with their keys;
3. what you tested and how, with results;
4. known limitations, and anything the coordinator must wire up or check visually.

## Addendum (current state of the codebase)

Much has been added since this brief was written. Read the relevant modules before designing:

- **Engine (`crates/engine/src`):**
  - `dsp.rs`, the real-time graph. Its child modules are `dsp/offline.rs` (offline render, `Isolate`) and `dsp/pdc.rs` (delay lines).
  - Other engine modules:
    - `automation.rs`
    - `takes.rs` (take folders, `RecPlan`)
    - `midi.rs` and `smf.rs` (MIDI files)
    - `transients.rs`
    - `prefs.rs` (app settings)
    - `eq.rs`, `synth.rs`, `decode.rs`, `clip.rs`, `recorder.rs`, `midi_io.rs`, `audio_io.rs`, `plugin_db.rs`
  - `session.rs`. Its child modules live in `session/`: `routing`, `bounce`, `range_edit`, `auto_write`, `options` (prefs, autosave, recording placement), `midi_content`, `pdc`, `freeze`, `markers` and `takes`.
- **Frontend (`src`):**
  - Shared modules:
    - `ops.ts` (track/region operations and menus)
    - `keymap.ts` (`KEY_COMMANDS`, the `MENU_SHORTCUTS` read-only list, `formatKey`, `keyHint`)
    - `prefs.ts`
    - `automation.ts`, `loops.ts`, `routing.ts`, `bounce.ts`, `freeze.ts`, `takes.ts`, `markers.ts`
    - `midiEdit.ts` (pure, tested), `audioEdit.ts`, `pianoRollDraw.ts`, `midiFiles.ts`
    - `waveform.ts`, `layout.ts`, `time.ts`, `transport.ts`, `view.ts`, `frame.ts`, `dnd.ts`
  - Components:
    - Main window: `Arrange`, `Ruler`, `TrackHeaders`, `TracksArea`, `ControlBar`, `Splitter`
    - Mixing: `ChannelStrip`, `Sends`, `Eq`, `Latency`
    - Editors: `Editor` (dispatcher), `PianoRoll`, `AudioEditor`, `AutomationHeader`
    - Dialogs and panels: `Settings` (all settings panes, Project Settings, Recovery), `Dialogs`, `Bounce`, `MarkerList`, `ProjectChooser`, `Panels`
  - Keys: `KEY_ACTIONS` in `App.tsx` maps keymap ids to actions.
- **Tests:** `cargo test -p daw-engine --lib`, `cargo test -p daw` and `npm test` (`tests/*.test.ts`, node:test).
- **Headless checks**, run with `-- "<scratchpad>/ImportTest.fermata/Audio Files/tone44.wav" <absolute scratch dir>`; `midi_check` takes only the scratch dir:
  - `stack_check`, `loop_check`, `sends_check`, `bounce_check`, `range_edit_check`
  - `automation_check`, `send_auto_check`, `pdc_check`, `freeze_check`, `takes_check`, `midi_check`, `fx_check`, `insert_order_check`

  **All of them must pass when you finish.**
- **Struct literals:** every new `Region { .. }` literal needs `loop_end: None, takes: None`, and every `Note` literal needs `muted`.
- **The repo is on GitHub now.** Still never run git in your sandbox.
