# Developing Fermata

How to build, run and test Fermata, how to debug it, the conventions the code follows, and
step-by-step recipes for the most common changes. Read [ARCHITECTURE.md](ARCHITECTURE.md)
first for how the pieces fit together.

The repository is at <https://github.com/JackMechem/fermata>; the main branch is `main`.

Contents:

- [Build and run](#build-and-run)
- [Tests](#tests)
- [Debugging](#debugging)
- [Conventions](#conventions)
- [Recipes](#recipes)
- [Packaging](#packaging)
- [Versioning and releases](#versioning-and-releases)

## Build and run

The workspace has three crates: `vst3-host`, `daw-engine` (`crates/engine`) and `daw`
(`src-tauri`, the application). The frontend is in `src/` and builds with Vite.

### NixOS and Nix (flake)

The flake provides the package, an app, a dev shell, an overlay and a check, for
`x86_64-linux` and `aarch64-linux`.

```sh
nix develop               # or `nix-shell`: Rust, Node, WebKitGTK, ALSA/JACK/PipeWire
pnpm install              # or `npm install`
pnpm tauri dev            # or `npm run tauri dev`
pnpm tauri dev -- -- path/to/Song.fermata   # open a project at launch
```

- `nix/shell.nix` takes its inputs from the package (`inputsFrom = [ fermata ]`), so the shell
  and `nix build` use the same libraries. It adds cargo, rustc, clippy, rustfmt, rust-analyzer
  and Node, sets `GIO_MODULE_DIR` and the GSettings schemas for GTK file dialogs, and puts ALSA,
  JACK, PipeWire and D-Bus on `LD_LIBRARY_PATH` for dev builds.
- `tauri.conf.json` runs `npm run dev` (Vite on port 1420) before a dev build and
  `npm run build` before a release build, whichever package manager you use to start Tauri.
- Both `package-lock.json` and `pnpm-lock.yaml` are kept. The Nix package installs npm
  dependencies from `package-lock.json`, so keep it current when you change dependencies (see
  [Packaging](#packaging)).
- For real-time audio scheduling, rtkit must be available (`security.rtkit.enable = true;`
  on NixOS; PipeWire setups usually have it).

Other flake outputs:

```sh
nix run .                 # build and launch Fermata
nix build .               # result/bin/fermata (and result/bin/daw), desktop entry and icons
nix profile install .     # install it for your user
nix flake check           # builds the package, which runs the engine's unit tests
```

To use it in a NixOS or home-manager configuration, take the flake as an input and use
`fermata.packages.${system}.default`, or add `fermata.overlays.default` to
`nixpkgs.overlays` and use `pkgs.fermata`.

Flakes only see files that git tracks, so `git add` new files before `nix build`.

### Other Linux distributions

Install Rust (stable), Node.js (22 or newer; see [Tests](#frontend-tests)), the Tauri 2
prerequisites (`webkit2gtk-4.1`, `gtk3`, `libsoup-3`, `librsvg`, `openssl`), and `alsa-lib`,
`libpipewire-0.3`, `jack` (libjack2), `dbus`, `pkg-config` and `libclang` (bindgen uses it to
generate the PipeWire bindings). Then:

```sh
pnpm install && pnpm tauri dev
```

### Windows and macOS

The audio engine (cpal: WASAPI on Windows, Core Audio on macOS), MIDI (midir), VST3 module
loading, the built-in effects and the UI are cross-platform, and `pnpm tauri build` produces
native bundles. Install Rust, Node.js and the Tauri 2 prerequisites for the platform (the
WebView2 runtime and the MSVC build tools on Windows, the Xcode command line tools on macOS).
Plug-in editor windows are implemented for Linux/X11 and macOS so far
(`crates/vst3-host/src/editor_other.rs` is the Windows placeholder), and the input level check
(`sysvol.rs`) is Linux-only. The Nix package is Linux-only. CI runs the unit tests on all
three platforms and the release workflow builds bundles for them (see
[Versioning and releases](#versioning-and-releases)), but the app gets far less hands-on testing
on Windows and macOS.

### Useful cargo commands

Run them from the repository root (inside `nix develop` on NixOS).

```sh
cargo build -p daw                       # the whole app, including the Tauri crate
cargo test -p daw-engine --lib           # engine unit tests
cargo test -p daw                        # Tauri crate tests (settings parsing)
cargo run -q -p daw-engine --example <name> -- <args>   # headless checks, below
cargo clippy --workspace
npx tsc --noEmit -p .                    # type-check the frontend
```

The dev profile (root `Cargo.toml`) builds the app crate at `opt-level = 1`, dependencies at
`opt-level = 2`, and `daw-engine` and `vst3-host` at `opt-level = 3`, so `tauri dev` has the
same CPU headroom for audio as a release build.

## Tests

### Rust unit tests

`cargo test -p daw-engine --lib` runs the engine's unit tests. They need no audio device.
They cover, among others: loops and real copies, routing order and loop pruning (`model.rs`);
the click's levels, accent, divisions and delay compensation (`dsp.rs`); delay line taps and the
PDC solver; take folder comping, crossfades and recording plans (`takes.rs`); fade curves,
crossfade gain sums and placement (`fades.rs`, `session/crossfades.rs`); automation
evaluation and thinning; Channel EQ filters; resampling; transient detection; Standard MIDI
Files; bounce trimming, normalizing and 16-bit dither; range edits; autosave and recovery,
undo limits and recording placement (`session/options.rs`); crash-recovery journals,
recovering interrupted takes, atomic saves with a backup and falling back from a damaged
project.json (`session/recovery.rs`); repairing the WAV headers a crash leaves, for every format
the recorder writes (`wav_repair.rs`); preferences parsing; the recorder's bit depths; the tempo map; `contain()`; preset files and names (`presets.rs`);
parameter normalization (`dsp/builtin.rs`); and each built-in effect's DSP (`fx/*.rs`: the
Limiter's ceiling and latency, the Reverb's stability, the Tuner's accuracy, the Loudness
Meter's BS.1770 weighting and true peak, and more).

`cargo test -p daw` runs the Tauri crate's tests (old and partly malformed `settings.json`
files load with defaults).

### Headless examples

`crates/engine/examples` holds integration checks that run a real `Session` with the ALSA
`null` output device (silent, no clock, so it runs as fast as it can). The `*_check` examples
print one `PASS <what>` or `FAIL <what>` line per check, and all of them except `tempo_check`
exit with status 1 when a check fails; for `tempo_check`, look for `FAIL` in the output
(`... | grep -c FAIL`). `undo_check` prints the project state after each step for you to read.

Most take an audio file and a scratch directory, which must be an absolute path and is used as
the project folder (never point it at your real projects):

```sh
cargo run -q -p daw-engine --example stack_check -- /path/to/tone.wav /tmp/fermata-scratch
```

`scripts/headless-checks.sh` builds the examples once and runs every check that needs no
third-party plug-ins, each in its own folder under a scratch directory, with a test tone from
the `test_tone` example. It's what CI runs, so run it the same way before sending an engine
change. It also runs on macOS: where there is no ALSA, the engine runs on a silent stand-in
device at the configured rate and buffer size instead of ALSA's `null` output:

```sh
scripts/headless-checks.sh                       # all of them, in a temporary folder
scripts/headless-checks.sh takes_check fx_check  # just these
scripts/headless-checks.sh --scratch /tmp/fermata-checks   # keep the projects and logs there
```

It runs the checks as on a fresh CI runner: `HOME` and the `XDG_*` folders point into the
scratch folder, and the PipeWire, PulseAudio, JACK and D-Bus session variables are cleared,
so the checks find no audio server, no settings and no plug-in cache, and never touch yours.
That makes the plug-in parts of `fx_check`, `automation_check` and `bounce_check` print `SKIP`;
`pdc_check` and `freeze_check` need a real plug-in and aren't in the script, so run those (and
the plug-in parts) by hand. A check counts as failed when it exits non-zero or prints a line
starting with `FAIL`; each one's output is in `<scratch>/<check>/output.log`, and the scratch
folder is kept when anything failed.

| Example | Arguments | What it checks |
|---|---|---|
| `fx_check` | audio file, scratch dir | Every built-in effect in the registry: it loads and processes, parameters set, save, reload, undo and duplicate, automation drives it, latency reaches PDC, side-chains key it (live and in isolated renders), meters reach the tick, freezing bakes it in; plug-in and Channel EQ copy and paste, presets. The VST3 paste checks need **ZaMaximX2**, and print `SKIP` without it. |
| `insert_order_check` | audio file, scratch dir | Reordering Audio FX inserts: the engine runs the new order, instances keep running (same ids, nothing reloaded), undo and redo restore it, moving or copying to another strip makes room there. |
| `stack_check` | audio file, scratch dir | Track stacks (folder and summing), groups and region edits, by the levels the meters report. |
| `loop_check` | audio file, scratch dir | Looping regions (playback, trim, move, convert, split, join) and hidden tracks. |
| `sends_check` | audio file, scratch dir | Sends pre and post fader, aux buses, output routing, solo through routing, loop refusal. |
| `send_auto_check` | audio file, scratch dir | A send-level automation lane changes what reaches the aux. |
| `bounce_check` | audio file, scratch dir | Bounce, Export All Tracks, Export Region and Bounce in Place: ranges, tails, formats, normalize, placement. |
| `export_check` | audio file, scratch dir | AIFF and FLAC bounces decode to the same samples as the WAV bounce (and within quantization of a float one); FLAC refuses float; Export Selected Tracks writes just the selection, folder stacks standing for their subtracks. |
| `crossfade_check` | audio file, scratch dir | Fade curves, Speed Up and Slow Down, and X, EqP and X S crossfades, measured in offline renders of synthetic files (X sums to unity, EqP to constant power); butt fades and gaps; the X-Fade drag mode with undo and redo; and that playback, Bounce, Export, Bounce in Place, Freeze, Join and save/reopen all sound the same. |
| `range_edit_check` | audio file, scratch dir | Audio Track Editor range edits and Split at Transients: what plays after each edit, and that one undo reverts it. |
| `automation_check` | audio file, scratch dir, optional plug-in name and parameter name | Reading volume and pan lanes, Off mode, writing in Write and Touch, undo; with a plug-in from the cache, one of its parameters too (`SKIP` when it isn't in the cache). |
| `pdc_check` | audio file, scratch dir | Plug-in delay compensation with a real latent plug-in, measured by cross-correlating offline renders. Needs **ZaMaximX2** (zam-plugins) in the plug-in cache. |
| `freeze_check` | audio file, scratch dir | Freeze: a frozen instrument track with a latent plug-in sounds as it did live, plug-ins are unloaded, edits are refused, undo, redo, unfreeze and reopening restore the right state. Needs **ZaMaximX2**. |
| `takes_check` | audio file, scratch dir | Take folders, comping, cycle and Autopunch recording layout (the null device has no input, so takes are silent), and markers. |
| `recovery_check` | audio file (unused), scratch dir | Crash recovery: a child process records (24-bit plain with two tracks, then 32-bit cycle) and is killed with SIGKILL mid-take; reopening offers and restores the takes at the right position and length (a take folder for the cycle), repairs the WAVs and clears the journal. A normal stop autosaves at once without touching project.json. |
| `countin_check` | scratch dir | Recording with a count-in from bar 1, where the transport runs at negative positions, three times without an engine error. |
| `tempo_check` | scratch dir | The tempo map and signature changes: the click across changes, plug-in transport positions, bar numbering, Move MIDI Regions with Tempo, quantize, a MIDI file round trip, undo, and that a project without changes keeps the old timing exactly. |
| `midi_check` | scratch dir | Piano Roll edits, muted notes, controller playback and chasing, split/join/trim with controllers, MIDI file export and import. |
| `region_params_check` | audio file, scratch dir | Region playback parameters in offline renders of the built-in synth and the tone: quantize, transpose, velocity, dynamics and gate play exactly the notes worked out by hand; delays in ms and ticks (across a tempo change, and before a region's start); quantize after a tempo change; an audio region's delay; Apply Region Parameters plays the same and undoes in one step; coalesced undo; saving. |
| `undo_check` | audio file | Undo and redo through a sequence of edits. |
| `import_check` | project dir, files... | Imports files onto new tracks and reports what was created. |
| `test_tone` | output `.wav` path | Not a check: writes the test tone the checks play (3 s of 440 Hz at -21.5 dBFS, stereo, 32-bit float, 48 kHz). |
| `demo_project` | output `.fermata` path | Writes a small demo project (synthesized drums, a plucked part, a MIDI region) for trying the UI. |
| `fx_bench` | optional block size (default 512) | Not a check: times every built-in effect per block at 48 kHz (worst and average) against the block's real-time budget. Run it with `--release`. |
| `hosts` | none | Lists every audio host and its devices. |
| `input_level` | `[--reset] [device-id]` | Prints (or resets to 100%) the system mixer volume of a recording device (Linux). |
| `smoke` | none | Lists devices, records two seconds from the default input through a plug-in and reports the region. Uses real devices. |

`crates/vst3-host/examples`:

| Example | Arguments | What it does |
|---|---|---|
| `probe` | optional name filter | Scans installed VST3 plug-ins, instantiates each and runs a few blocks through it. |
| `editor` | name substring, seconds | Opens one plug-in's editor with a small `poll()` run loop (no GTK), for debugging editor embedding. |

When writing a new headless check, copy the pattern in `stack_check.rs`: a `Session` on ALSA
`alsa:null` output, `s.dir` pointed at the scratch folder, the cycle turned on so playback
loops, and about 60 ms of sleep after `stop()` before the next `play()`.

### Frontend tests

```sh
npm test                  # node --test tests/*.test.ts
npx tsc --noEmit -p .     # type check (strict, no unused locals or parameters)
```

The tests use the built-in `node:test` runner on the pure frontend modules:
`tests/midiEdit.test.ts` (the Piano Roll functions in `src/midiEdit.ts`: grid lengths, snapping,
quantize with strength, swing and ends, and the rest), `eventList.test.ts` (the Event List's
positions across tempo and signature changes, typed values and edits), `audioEdit.test.ts`
(`src/audioRange.ts`), `fxFormat.test.ts` (built-in effect readouts and control mapping),
`keyFormat.test.ts`
(shortcuts on each platform) and `tempo.test.ts` (`src/tempo.ts`). Node 23.6 or newer runs
TypeScript directly; on Node 22 add `--experimental-strip-types`.

### What to run before sending a change

- `cargo test -p daw-engine --lib` and `cargo test -p daw`
- every headless example that touches your area (for engine changes,
  `scripts/headless-checks.sh` and the plug-in checks `pdc_check` and `freeze_check`); for a
  built-in effect, `fx_check` and `fx_bench`
- `npm test` and `npx tsc --noEmit -p .`
- a visual check in the running app for UI changes

## Debugging

- **Logs**: the app uses `env_logger` with the default filter `info,jack=off`, printed to the
  terminal that started it. Set `RUST_LOG=debug` (or `RUST_LOG=daw_engine=debug`) for more.
  Plug-in load failures and missing audio files are logged as errors; autosaves at info level;
  plug-in restart flags at debug level.
- **Frontend errors**: a failed command shows its error as a toast (`report` in `api.ts`). The
  app replaces the webview's right-click menu with its own.
- **Blank window on Linux**: some GPU drivers need `WEBKIT_DISABLE_DMABUF_RENDERER=1`. Leave it
  unset otherwise, because WebKit then paints on the CPU at much higher cost.
- **Wayland**: `main.rs` sets `GDK_BACKEND=x11` unless you set it yourself, because plug-in
  editors are X11 windows and GLX-based ones fail when GTK runs natively on Wayland in the same
  process. Set `GDK_BACKEND=wayland` to override (plug-in GUIs may then fail to open).
- **Audio problems**: the status line shows the device, rate and buffer; Settings › Audio shows
  the measured latencies. `cargo run -p daw-engine --example hosts` lists what cpal sees.
  The tick carries an `overruns` counter that grows when a recording ring buffer overflowed.
- **Engine errors**: a panic inside `Engine::process` is caught in the audio callback, the
  block is silenced, `EngineStatus::engine_errors` counts it, and a toast asks for a report;
  the panic message and backtrace are in the terminal (`RUST_BACKTRACE=1`). A plug-in putting
  out NaNs or runaway levels is contained and named in a toast (`bad_audio_warning`).
- **"System overload"**: the callbacks ran late for more than 60 ms in a row, so the engine shed
  load and the session stopped the transport (`EngineStatus::overloads`). The CPU figure in the
  control bar shows how close you are. `fx_bench --release` times the built-in effects; the audio
  thread runs with flush-to-zero, so denormals are not the cause.
- **Plug-ins**: `cargo run -p vst3-host --example probe -- <name>` loads and runs plug-ins
  outside the app. Settings › Plug-ins shows the last scan's report, including bundles that
  crashed or timed out. The cache is `plugins.json` in the config folder; delete it (or use
  Rescan) to force a new scan.
- **Settings and projects**: `settings.json` and `recent.json` are in `~/.config/fermata`
  (`$XDG_CONFIG_HOME/fermata`, `%APPDATA%\fermata` on Windows). A project is plain JSON in
  `project.json`; it is safe to read, and editing it by hand works if the app is closed.
- **Engine state**: everything the UI knows is in the `Snapshot` (`getState().snap` in
  `src/store.ts`) and the last `Tick` (`currentTick()`); a headless example is usually the
  quickest way to check what the engine does.
- **Never test with your real projects folder**: headless checks write into the scratch
  directory you pass; keep it away from `~/Music/Fermata`.

## Conventions

These keep the code maintainable and the merges clean.

- **Real-time safety.** Nothing on the audio thread allocates, locks, blocks or does I/O, and
  nothing waits on another thread: use `rtrb` rings and atomics, not channels or mutexes. New
  engine state arrives as a `Cmd` and replaced objects leave as `Garbage`. See
  [ARCHITECTURE.md › Real-time rules](ARCHITECTURE.md#3-real-time-rules).
- **Model compatibility.** Every new field in `model.rs` or `prefs.rs` gets `#[serde(default)]`
  (or a struct-level `#[serde(default)]`), and optional fields usually also
  `skip_serializing_if = "Option::is_none"`, so older projects and settings files still load.
- **Struct literals.** Every `Region { .. }` literal needs `loop_end`, `takes` and `params`, and every
  `Note` literal needs `muted`. Tests and examples build these by hand.
- **Undo.** Every user edit goes through a checkpointed command (`edit`, `try_edit`,
  `edit_made`). Performance switches (mute, solo, arm, monitor) and transport are not undoable.
  Anything you send to the engine must also be handled in `restore()`, `build_track()` and
  `rebuild_graph()`.
- **Main thread for plug-ins.** VST3 controllers and editors are touched only on the main
  thread (synchronous commands, `idle`). Long work goes in an async command; hand edits back
  to the main thread with `run_on_main_thread` (see `bounce_in_place`, `freeze_tracks`).
- **Argument names.** Tauri maps camelCase JS argument names to snake_case Rust parameters.
  Struct fields keep their serde (snake_case) names.
- **Where things go.** Substantial new code goes into a new module (`session/<feature>.rs`,
  `src/<feature>.ts`, `src/components/<Feature>.tsx`) and is wired in with small edits. New
  struct fields go at the end of the struct, new commands at the end of `commands.rs` and of the
  `invoke_handler` list in `main.rs`, new CSS at the end of `styles.css`, new menu items next
  to related ones.
- **Comments.** Public items get doc comments. Comments explain *why*, not what. Keep the
  Logic Pro reference in mind: behaviour and labels follow Logic unless there is a reason not
  to, and the reason goes in a comment.
- **UI.** Use the existing CSS variables and components (`Form.tsx`, `Controls.tsx`,
  `Menu.tsx`). Follow the Logic-like visual language; don't introduce a new look.
- **Formatting.** `rustfmt` for Rust (long lines are accepted in this codebase); TypeScript is
  strict with no unused locals or parameters.

## Recipes

### Add a project field

Example: a per-project "Notes" field (it exists as `Project::notes`).

1. `crates/engine/src/model.rs`: add the field at the end of `Project` with
   `#[serde(default)]`, and initialise it in `Project::new`.
2. `src/api.ts`: add it to the `Project` interface.
3. Add a `Session` method that changes it and sets `self.dirty = true` (in the most fitting
   `session/` module, `options.rs` for project options).
4. Add an undoable command for it (next recipe).
5. If the engine needs the value, add a `Cmd` (recipe after next), send it from
   `sync_globals()` (for project-wide values) or `build_track()` (per track), and make sure
   `restore()` resends it when it differs.
6. Build anything that constructs a `Project` or the struct you changed by hand (tests,
   examples) and fix the literals.

### Add an undoable command end to end

1. **Session.** Write the edit as a method on `Session`. It changes `self.project`, sends what
   the engine needs (`push_regions`, `Cmd::Params` via `push_all_params`, `sync_tracks` for
   structural changes) and sets `self.dirty = true`. Return `Result` if it can fail.
2. **Command.** At the end of `src-tauri/src/commands.rs`:

   ```rust
   /// Sets the project notes (Project Settings).
   #[tauri::command]
   pub fn set_project_notes(state: State<AppState>, notes: String) -> Snapshot {
       edit(&state, "Project Notes", Some("notes".into()), false, |s| s.set_notes(notes))
   }
   ```

   Use `try_edit` when the method returns `Result`, and `edit_made` when the UI needs the ids
   of what was created. The label is what Edit › Undo shows. Give a `key` when a drag or a
   typing burst should merge into one undo step. Pass `plugins = true` when the edit can remove
   plug-ins, so undo can bring their state back.
3. **Register** it at the end of the `invoke_handler` list in `src-tauri/src/main.rs`.
4. **Call** it from the UI: `act("set_project_notes", { notes })`, or
   `actMade("cmd", { ... })` to get the new ids. Argument names are camelCase in JS.
5. **Check undo**: make the edit, undo, redo, and check that the engine plays the right thing
   (a headless example is the best way). If the engine holds state for it, `restore()` must
   resend it.

### Add an engine `Cmd`

1. Add the variant at the end of `enum Cmd` in `crates/engine/src/dsp.rs`, with a doc comment.
   Prefer small `Copy` payloads; anything allocated is built by the session and moved in
   (`Box`, `Arc`).
2. Handle it in `Engine::apply`. If it replaces an allocated object, send the old one back
   with `self.discard(Garbage::...)` (add a `Garbage` variant if needed). Never drop
   allocations on the audio thread.
3. Use the new state in `process_block` without allocating. Preallocate buffers in
   `TrackDsp::new` or `Engine::new`.
4. Send it from the session with `self.send(Cmd::...)`.
5. Make it survive everything that rebuilds or diffs the graph: `build_track()` (new tracks
   and project load), `sync_globals()` (global values, called after load and undo),
   `restore()` (undo and redo), and, if offline renders must save and restore it,
   `Engine::render` in `dsp/offline.rs`.
6. Add a unit test in `dsp.rs` (see the `rig()` helper) or a headless example.

### Add a key command

1. `src/keymap.ts`: add an entry to `KEY_COMMANDS` with a unique `id`, the label shown in
   Settings › Key Commands, a group from `KEY_GROUPS` and the default keys (`"Mod+..."` for
   ⌘/Ctrl, `"Ctrl+..."` for the macOS Control key, then `Alt`, `Shift`). Check that the keys
   don't clash with `MENU_SHORTCUTS` or another command.
2. `src/App.tsx`: add the action to `KEY_ACTIONS` under the same id. If a menu item does the
   same thing, call `handleMenu("<menu id>")` so both stay in step.
3. Settings › Key Commands lists the new command automatically, and user overrides work
   through `settings.json` › `ui.keymap`.
4. If an editor (Piano Roll, Audio Track Editor) should handle the key differently while it
   has focus, handle it in that editor's own key listener and stop propagation there.

### Add a native menu item

1. `src-tauri/src/menu.rs`: add `.item(&item("my-id", "My Command…", None)?)` next to related
   items. Use an accelerator (`Some("CmdOrCtrl+Shift+X")`) only for ⌘ shortcuts; plain keys
   belong in the keymap so they can be changed.
2. `src/App.tsx`: add `case "my-id":` to `handleMenu`.
3. If you gave it an accelerator, add it to `MENU_SHORTCUTS` in `src/keymap.ts` so Settings
   › Key Commands shows it and refuses conflicting keys.
4. If the command should also run from a key that the user can change, add a key command
   (above) whose action calls `handleMenu("my-id")`.

### Add a setting

Decide where it belongs:

- **Engine behaviour** (General, Recording, MIDI, Editing, latency): a field in the matching
  struct in `crates/engine/src/prefs.rs` (`GeneralPrefs`, `RecordingPrefs`, ...). The structs
  use `#[serde(default)]`; add the default in their `Default` impl.
- **Interface only**: `UiPrefs` in `src-tauri/src/settings.rs` (add the default to its
  `Default` impl).
- **Saved with the project**: a `Project` field instead (see above).

Then:

1. Mirror the field in `src/prefs.ts` (`Prefs` or `UiPrefs`).
2. Add the control to the right pane in `src/components/Settings.tsx`, using the `Form.tsx`
   controls, and change it with `updateSettings((s) => ({ ...s, prefs: { ... } }))`. That calls
   `set_settings`, which writes `settings.json` and calls `Session::set_prefs`.
3. Apply it in `Session::set_prefs` (`session/options.rs`) if the engine must react (send a
   `Cmd`, call `update_pdc`, ...), or read `self.prefs` where it is used.
4. Add a case to the tests in `prefs.rs` or `settings.rs` if parsing could be affected.

### Add a built-in effect

See [ARCHITECTURE.md › Built-in effects and presets](ARCHITECTURE.md#13-built-in-effects-and-presets).

1. `crates/engine/src/fx/<name>.rs`: a struct implementing `Effect`, allocating everything in
   its constructor; `PARAMS` (new ids only, never renumbered), meter names, `PRESETS`, and
   `pub fn info() -> EffectInfo` with the id `builtin:<name>` and a category.
2. List it in `fx::registry()` and `fx::factory_presets()` in `fx/mod.rs`.
3. Unit tests in the module: silence stays clean, the reported latency is the real one, the
   presets set real parameters within range, and it stays stable at extreme settings (see
   `limiter.rs` and `reverb.rs`).
4. A panel, if the generic one isn't enough: `src/components/fx/<Name>.tsx`, built from
   `common.tsx`, registered in `FX_PANELS` in `src/fx.ts`. Call the panel props' `touch(id, on)`
   around every drag so Touch and Latch automation record it.
5. Run `fx_check` (it covers every registry entry) and
   `cargo run --release -q -p daw-engine --example fx_bench` to check the cost per block.

### Add a theme colour

For a CSS colour:

1. `src/themes.ts`: add `"--my-colour": "..."` to `studioCss`. Flat, Midnight, Light and High
   Contrast inherit it by spreading, so override it only where it must differ (at least check
   `lightCss` and `contrastCss`).
2. Use `var(--my-colour)` in `src/styles.css` (new rules go at the end).

For a canvas colour:

1. `src/theme.ts`: add the field to `CanvasTheme` and a value to the default `theme` object.
2. `src/themes.ts`: give it a value in `studioCanvas`, and override it in the other canvas
   themes where needed (they spread `studioCanvas` or `flatCanvas`).
3. Draw with `theme.myColour`. Views redraw on a theme switch when they subscribe to
   `themeId`.

## Packaging

```sh
nix build .               # result/bin/fermata, result/share/applications/Fermata.desktop, icons
pnpm tauri build          # native bundles for the current platform (deb, rpm, AppImage, NSIS, dmg, ...)
```

- The Nix package (`nix/package.nix`) builds the frontend with npm in the sandbox, vendors the
  Rust crates from `Cargo.lock`, runs `cargo tauri build`, runs the engine unit tests
  (`cargo test -p daw-engine --lib`), and installs the result with a `fermata` symlink to the
  `daw` binary and a desktop entry in the audio categories.
- npm dependencies come straight from `package-lock.json` through nixpkgs' `importNpmLock`,
  so there's no npm hash to refresh. Keep `package-lock.json` in step with `package.json`
  (`npm install --package-lock-only` after changing dependencies with pnpm).

- The package source is a file set (Cargo files, `crates`, `src-tauri`, `src`, `index.html`,
  `LICENSE`, the npm and TypeScript configs), so editing docs doesn't trigger a rebuild.
- The bundles (and so the Nix package, in `lib/Fermata`) carry a copy of `LICENSE`, through
  `bundle.resources` in `src-tauri/tauri.conf.json`.
- `nix/package.nix` has its own `version` string; `scripts/set-version.sh` keeps it in step
  (see below).

## Versioning and releases

Fermata follows [semantic versioning](https://semver.org). Prereleases carry a suffix
(`0.1.0-alpha.1`, `0.1.0-alpha.2`, ...), and every release is tagged `v<version>`.
[CHANGELOG.md](../CHANGELOG.md) records each release in the Keep a Changelog format.

The version is recorded in the root `Cargo.toml` (`[workspace.package] version`, inherited by
every crate) and `Cargo.lock`, `package.json` and `package-lock.json`,
`src-tauri/tauri.conf.json`, and `nix/package.nix`. `scripts/set-version.sh <version>` rewrites
all of them (the three workspace crates in `Cargo.lock`), refuses anything that isn't a
semver version, and prints the result. It needs GNU sed: on macOS install it (`brew install
gnu-sed`) and put `gsed` first in `PATH` as `sed`.

To make a release:

1. Move the `[Unreleased]` entries in `CHANGELOG.md` under a new `## [<version>] - <date>`
   heading. The release notes are taken from that section.
2. `scripts/set-version.sh <version>`.
3. Commit, for example `git commit -am "Release <version>"`.
4. Tag and push the tag: `git tag v<version> && git push origin main v<version>`.

Pushing a `v*` tag runs `.github/workflows/release.yml`:

1. `create-release` makes a draft GitHub release named "Fermata v<version>", with the version's
   CHANGELOG section plus download and unsigned-build notes. A version with a `-` suffix is
   marked as a prerelease.
2. `build` runs `tauri-action` on five runners and uploads the bundles to the draft:
   Linux x86_64 (`ubuntu-24.04`) and aarch64 (`ubuntu-24.04-arm`): `.deb` and `.AppImage`,
   plus `.rpm` for versions without a prerelease suffix (RPM versions can't contain `-`); Windows (`windows-latest`): the NSIS installer (`_x64-setup.exe`); macOS
   (`macos-latest`): `.app` and `.dmg` for `aarch64-apple-darwin` and `x86_64-apple-darwin`.
3. `publish` takes the release out of draft once every build has finished.

To try a change to the workflow without releasing anything, push it to the `release-check`
branch: the same builds run, and nothing is created or published. The bundles are not signed
or notarized yet.

`.github/workflows/ci.yml` runs on pushes to `main`, on pull requests and on demand, on
`ubuntu-24.04`, `windows-latest` and `macos-latest`: `npm ci`, `npm run build` (type check and
frontend build), `npm test`, `cargo test -p daw-engine --lib`, `cargo test -p daw` and
`cargo check --workspace --all-targets` (so the examples must compile). On Linux it then runs
`scripts/headless-checks.sh`: the headless checks that need no third-party plug-ins, on ALSA's
`null` device (the runner has no sound card or audio server) with a generated test tone. A
separate `nix` job runs `nix flake check`, which builds the Nix package. `pdc_check`,
`freeze_check` and the plug-in parts of the other checks need real plug-ins, so they don't run
in CI; run them locally.

The main branch is `main` on <https://github.com/JackMechem/fermata>.
