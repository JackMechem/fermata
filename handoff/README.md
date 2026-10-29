# Resuming work (handoff from 2026-10-29)

This folder has everything needed to carry on from another computer. If you're Claude and were
asked to "resume", read all of this file first, then the two `HANDOFF-*.md` files.

## Where things stand

- All finished work is committed on branch **`dev`** and pushed. `main` is the
  v0.1.0-alpha.1 release. **Don't make a release or tag yet**; the user wants more changes
  first.
- Finished this round, all on `dev` and listed in `CHANGELOG.md` under Unreleased:
  - Stage 1 of `TODO.md`: LICENSE, the locate race behind the `takes_check` flake, and the
    headless checks in CI.
  - Automation lanes follow a plug-in moved to another track.
  - Fade curves and types, crossfades and the X-Fade drag mode.
  - Autosave after recording, and recovery of takes interrupted by a crash.
  - The mouse wheel setting, Scroll to Playhead (Alt+`) and the view following a second Stop.
  - Compact track headers, with a meter inside the volume slider.
  - The overview strip, and the Group Settings crash fix.
- **Two features are unfinished.** They're saved here as patches, so `dev` itself builds and
  works:

| Patch | Feature | State |
|---|---|---|
| `fxdock.patch` | Built-in effect panels | About 60% done. Logic, store, docked pane and resizable floating windows are written. The Loudness Meter's value rows are restructured, but the CSS that fixes the overlap isn't written, and **no CSS at all is written**, so panels won't lay out yet. `tsc` and `cargo build -p daw` passed. Next steps: `HANDOFF-fxdock.md`. |
| `detach.patch` | Detachable bottom-section panels | About 30% done. The Rust window commands, capabilities, menu routing and the pure model in `src/detachModel.ts` are written. The frontend runtime, components and wiring aren't. `tsc` and `cargo build -p daw` passed. Next steps: `HANDOFF-detach.md`. |

The fxdock patch covers:
- the bug in `loudness-meter-overlap.png`, where "LUFS" and "LU" are drawn over the values;
- the same check in every other built-in effect panel and the Channel EQ;
- resizable panels;
- panels docked in the bottom section by default, with a float/dock toggle;
- a setting "Open Effect Panels: Docked / Floating".

The detach patch covers:
- detaching the Mixer, editors and effect panels into their own OS windows for another monitor;
- keeping them in sync;
- remembering their position, size and monitor.

Both patches are against commit `801f20f` and apply cleanly to it. The only later commit is
this `handoff/` folder.

## What the user asked for, in their words

- On the effect panels, with the screenshot: "also fix this. these menus should also be
  resizable and also by default I want them to be on the bottom section like where the mixer
  is. I want all the panels that are on the bottom section to be detachable as separate
  windows, so I can detach them if I want and put them on another monitor."

## How to resume

1. **Finish fxdock first, then detach.**
   - Detach is designed to register fxdock's docked effect panels as one more detachable
     panel kind, reusing `PanelContent`, `PanelLead`, `PanelTools` and `usePanelTitle` from
     fxdock's `src/components/fx/FxPanel.tsx`.
   - Both patches touch `src/store.ts` and `src/prefs.ts`, so expect a small merge.
2. **Start each feature on its own branch**, then merge both back into `dev`:
   ```sh
   git switch -c fxdock dev && git apply handoff/fxdock.patch
   ```
   Paths in the `HANDOFF-*.md` files that start with `/tmp/claude-1000/...` point at the old
   sandboxes; read them as paths relative to the repository root.
3. **Follow the numbered next steps** in `HANDOFF-fxdock.md`, then in `HANDOFF-detach.md`.
4. **Test before merging:**
   - `cargo test -p daw-engine --lib` and `cargo test -p daw`
   - `npm test` and `npx tsc --noEmit -p .`
   - `npx vite build --outDir <scratch dir>`
   - `bash scripts/headless-checks.sh`, inside `nix develop` on NixOS
   - a look at the running app (`pnpm tauri dev`)
5. When both are in, update `CHANGELOG.md` (Unreleased), `docs/USER_GUIDE.md` and, if
   relevant, `LOGIC_FEATURES.md` and `TODO.md`. **Delete this `handoff/` folder** and its
   pointer in `CLAUDE.md`.

### Pitfalls the agents found

- **Menu events reach every window.** In Tauri 2, a plain `listen()` also receives events sent
  with `emit_to` to a *different* window. `App.tsx`'s `listen("menu")` must become
  `getCurrentWebviewWindow().listen("menu")`, and every per-window event needs the same.
- **Merging CSS.** When two branches both append to the end of `src/styles.css`, a 3-way merge
  can splice the blocks together and still report success. Check the tail by hand, and run
  the vite build before trusting a merge.
- **Struct literals.** Every new `RegionContent::Audio { .. }` literal needs
  `fade_shape: Default::default()`, and every `RegionDsp::Audio` needs
  `laws: Default::default()`.

## How the user likes to work

- **Use agents.** Run independent features as parallel agents. Each works in its own copy of
  the tree: a git worktree or a copied folder, with its own `CARGO_TARGET_DIR`. `AGENT-BRIEF.md`
  is the brief they worked from: the architecture, the rules that keep merges easy (new fields
  and commands at the end, new code in new files, `#[serde(default)]` on every new model
  field), and how to test. Its sandbox paths are for the old machine.
- **Never let an agent launch the GUI.** The coordinator does the visual checks.
- **Deploy live.** The user runs `pnpm tauri dev` from the repo. As soon as a piece is merged
  and verified, put it in the repo, even though the app restarts; don't wait for the user to
  save. Say that the app restarted.
- **Report progress.** The user wants every requested feature finished and to hear about
  progress, with a short note when each piece lands.
- **Git.**
  - Commit and push only when asked, to `dev`.
  - Make one commit per feature, with a descriptive body.
  - The old machine signed commits with a dedicated SSH key, because the user's YubiKey needs
    a touch. Use whatever signing setup this machine has, or ask.
- **Windows/macOS.** The user will do the Windows/macOS VST work (editor embedding) themselves
  later. Code signing for Windows and macOS also needs the user.
- **CI.** It runs only on pushes to `main` and on pull requests, so `dev` hasn't been through
  CI. The new headless-checks step is unproven on GitHub's runners; a draft PR from `dev` into
  `main` would run it.

## Release lessons (for later)

- Linux CI must be Ubuntu 24.04 or newer, because 22.04's PipeWire is too old for libspa 0.10.
- RPM rejects a `-` in the version, so prereleases ship no `.rpm`.
- GitHub job logs need admin rights. Read failures through annotations, or ask the user for a
  screenshot.
- Pushing a `v*` tag publishes a GitHub release (see `docs/DEVELOPING.md`).

## Still to check by eye in the app

No agent could see these, and the user hasn't confirmed them yet:
- The overview strip, in a few themes.
- Track headers at 22–51 px, and the meter-in-slider in the Light and Contrast themes.
- The crossfade and fade-curve drawing, and the Inspector's fade rows.
- The Drag pop-up next to Snap.
- The Scroll to Playhead button, and a right-click on the scroll bar with the overview hidden.
- The recovery sheet after `kill -9` during a take.
