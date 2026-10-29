# Handoff: work in progress (2026-10-29)

Two features were being built in parallel by agents when work moved to another computer. Their
unfinished work is saved here as patches, so `dev` itself stays building and working. Delete
this folder once both features are merged.

| Patch | Feature | State |
|---|---|---|
| `fxdock.patch` | Built-in effect panels: fix the Loudness Meter's unit labels overlapping the values (and the same in other panels), resizable panels, docked in the bottom section by default with a float/dock toggle, and a setting "Open Effect Panels: Docked / Floating". | Roughly 60%. Logic, store, docked pane and resizable floating windows are written; **the CSS is not**, so panels won't lay out yet. `tsc` and `cargo build -p daw` passed. See `HANDOFF-fxdock.md`. |
| `detach.patch` | Every bottom-section panel (Mixer, editors, effect panels) can be detached into its own OS window for another monitor, kept in sync, with position, size and monitor remembered. | Roughly 30%. Rust window commands, capabilities, menu routing and the pure model are written; the frontend runtime, components and wiring are not. `tsc` and `cargo build -p daw` passed. See `HANDOFF-detach.md`. |

Both patches are against commit `801f20f` ("Scroll to Playhead defaults to Alt+`"), the commit
just before this folder was added, so they apply cleanly on top of it.

## Resuming

Work on each feature in its own branch or worktree, then merge:

```sh
git switch -c fxdock dev && git apply handoff/fxdock.patch
git switch -c detach dev && git apply handoff/detach.patch
```

Then follow the numbered next steps in each `HANDOFF-*.md`. Paths in them that start with
`/tmp/claude-1000/...` point at the old sandboxes. Read them as paths relative to the repository
root.

**Order:** finish fxdock first. Detach is designed to register fxdock's docked effect panels
as one more detachable panel kind, reusing `PanelContent` / `PanelLead` / `PanelTools` /
`usePanelTitle` from fxdock's `src/components/fx/FxPanel.tsx`. Both patches touch `src/store.ts`
and `src/prefs.ts`, which will need a small merge.

**Watch out for:**

- **Menu events reach every window.** In Tauri 2 a plain `listen()` also receives events sent
  with `emit_to` to a *different* window. `App.tsx`'s `listen("menu")` must become
  `getCurrentWebviewWindow().listen("menu")`, and every per-window event needs the same.
- **Merging CSS.** When two branches both append to the end of `src/styles.css`,
  `git merge-file` can splice the blocks together and still report success. Rebuild the tail
  by hand, and run `npx vite build --outDir <scratch dir>` before trusting a merge.

## Agent brief

`AGENT-BRIEF.md` is the brief the agents worked from: the architecture, the rules that keep
merges easy, and how to test. Its sandbox and `/tmp` paths are specific to the old machine.

Before merging, run the full set of checks: `cargo test -p daw-engine --lib`,
`cargo test -p daw`, `npm test`, `npx tsc --noEmit -p .`, `bash scripts/headless-checks.sh`
(inside `nix develop` on NixOS), and a look at the running app.
