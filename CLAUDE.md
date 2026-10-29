# Fermata

A Logic Pro–style DAW in Rust, Tauri 2 and React. Start with `docs/ARCHITECTURE.md` and
`docs/DEVELOPING.md`. The roadmap is in `TODO.md`; its "Open items" section lists what is left
from the latest round of work, including what still needs a look in the running app.

## How the user likes to work

- Run independent features as parallel agents, each in its own git worktree with its own
  `CARGO_TARGET_DIR`. Never switch branches in the main checkout while `pnpm tauri dev` runs
  from it: overlapping rebuilds corrupt the incremental cache.
- Agents never launch the GUI; the coordinator does the visual checks.
- Deploy finished, verified work straight into the repo the user runs `pnpm tauri dev` from,
  and say when the app restarts. Report progress as each piece lands.
- Commit and push only when asked, to `dev`, one commit per feature with a descriptive body.
  Commits are signed with the user's YubiKey, so leave signing and pushing to them.
- When two branches append to the end of `src/styles.css`, check the merged tail by hand and
  run the vite build before trusting the merge.
