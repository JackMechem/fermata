# Fermata roadmap

What's left, in the order it matters for a usable release. Each stage builds on the one before.
The full Logic Pro feature scope is in [LOGIC_FEATURES.md](LOGIC_FEATURES.md); known limitations
of what exists are in the [User Guide](docs/USER_GUIDE.md#20-known-limitations).

## Open items (2026-10-30)

Left over from the round of 2026-10-30 (Track On/Off, drag to a new track, VST3 side-chain inputs,
multi-output instruments, surviving SIGXCPU). Checked in the running app on a virtual display: the
new-track lane and drop, undo, the Off look, the Side Chain menu and SC badge, the "+", an output
aux's name and Input slot, and TONE3000's editor opening on a copy of a real project.

- **Not heard yet:** all of the above ran stopped or on the null device. Listen for clicks when
  choosing a side-chain source and pressing "+" during playback, on a real interface.
- **Opening TONE3000's editor can blow the 10.7 ms soft real-time limit** (SIGXCPU on two of three
  first opens on a copy of a real project, ignored now, so it should only cost a dropout). What
  makes that block slow isn't known.
- Undo in a VST3's own editor was checked by hand on TONE3000 with a model: the knob follows Undo
  and Redo, with no late blocks over three rounds while stopped. Not listened to during playback.
  The CPU meter's red state hasn't been seen, since nothing ran late.
- In Low Latency Mode a monitored instrument skips its compensation delay but its output auxes
  keep theirs, so an aux can lag while monitoring.
- An LSP lookahead change didn't reach delay compensation after a render until the next
  `update_pdc`.
- Freeze and Bounce in Place refuse an instrument with output auxes; Export Region renders only
  its main output.
- **LSP's Multi-Sampler can lose a sample for good while loading:** one instrument stays silent, and
  a new instance or reopening the project brings it back. Measured with two samples and our plug-in
  host only, no engine: 1 load in 300 at audio-device pace, 9 in 300 calling `process` flat-out. It
  hasn't been tried in another host, so whether the fault is LSP's or in how we call it is unknown.
  `multiout_check` retries past it and prints a line when it happens.
- `controller_check` failed once in a full run, counting two MIDI messages too many ("other messages
  still play the instrument (2 -> 6)"), and passed 15 of 15 alone. Cause unknown.
- An undo that turns a latent track back on during a take can shift where the take lands.

## Open items (2026-10-29)

Left over from the feature round of 2026-10-29 and the stabilisation pass after it. Nothing
here has been checked by eye in the app yet unless it says so.

- **Look at in the running app (this round's features):** the Arrangement track (move, copy,
  delete with content); File › Revert to and Project Alternatives submenus updating on Linux;
  automation curve bending (Ctrl+Shift-drag) and extra lanes; the Output track; a looped region's
  loop-end crossfade; the region gain handle; the Tool menu (T) and Marquee; the Mixer's Setting
  button, peak readouts, gain-reduction bar, VCA slot and VCA strips; the Inspector's Region rows;
  the new effect panels and Linear Phase EQ; Event List, Step Sequencer and Step Input; Strip
  Silence; Controller Assignments; the name sheet replacing prompts; sheets holding back keys.
- **Look at in the running app (earlier):**
  - Effect panels: the Loudness Meter's units at −100.0 and −∞; resizing floating panels from
    every edge and corner; the dock's tabs, float/dock, the preset sheet not being clipped; Esc
    order (frontmost floating panel, then the docked one).
  - Separate windows: Mixer and Editor pop-out and back; key commands and ⌘A/⌘C/⌘V in a
    detached Editor; an effect slot clicked in a detached Mixer floats there; positions
    remembered across a relaunch and on a second monitor (Retina next to a 1× screen).
  - The Mixer scrolling when short, with every strip ending on the same line.
  - Open Effect Panels › Separate windows, and each effect type remembering its window size.
  - macOS VST3 editors in the app (Surge XT, Keyscape): open, raise, close, resize, cascade,
    hiding while another app is in front; Space, R and Musical Typing with a plug-in window in
    front; typing into a plug-in's text field; ⌘S running once; a project with a missing
    plug-in showing it struck through.
  - From before the move: the overview strip in a few themes; track headers at 22–51 px and the
    meter-in-slider in the Light and Contrast themes; crossfade and fade-curve drawing and the
    Inspector's fade rows; the Drag pop-up; the Scroll to Playhead button; the recovery sheet
    after `kill -9` during a take.
- **Windows/macOS: verify by hand.** `dev` cross-checks and lints clean for both, and the
  engine tests and headless checks pass on the silent stand-in device they use there (simulated
  on Linux), but none of this has run on a real machine:
  - Both: audio devices listed in Settings › Audio, playback, recording and input monitoring
    through an interface; MIDI input and MIDI Learn; plug-in scanning; the file dialogs (open,
    save, import, bounce, export); detached Mixer, Editor and Channel EQ windows opening,
    closing, re-attaching and coming back in place after a relaunch, on two monitors with
    different scaling; native menu commands reaching the focused detached window; renaming a
    template or alternative by case only (both file systems ignore case); recovery after the
    app is killed during a take.
  - Windows: a detached window opening doesn't hang the app; Alt+` and other Alt key commands
    don't leave the focus in the menu bar; the NSIS installer and uninstaller, with LICENSE in
    the install folder; recording with Settings › Privacy › Microphone off and on.
  - macOS: the first recording asks for microphone access with the Info.plist text, in
    `tauri dev` and in the .app; ⌘ and ⌥ key commands; the unsigned .dmg opening with
    right-click › Open; `bash scripts/headless-checks.sh` (bash 3.2, no `timeout`).
- **Small fixes:**
  - Pattern regions restart their pattern where they're cut, so an Arrangement move or a section
    edit that cuts across one changes what plays (splitting by hand does the same, by design).
  - Arrangement moves and section edits don't move the regions of a track's inactive
    alternatives.
  - On a live Stop with delay compensation, the audio still in the compensation delay lines plays
    out at static levels (renders keep reading automation through it; playback doesn't).
  - MIDI Learn has no VCA level target.
  - The Compressor's "Lookahead" and the Classic Opto's "Peak Reduction" labels may overlap
    their knobs by a pixel or two.
- **Larger:**
  - The settings folder is `~/.config/fermata` on macOS too; `~/Library/Application Support`
    is the native place, but moving it needs a migration.
  - VST3 editor windows on Linux don't pass unused keys to Fermata, so Ctrl+Z and Space do nothing
    with focus in one; macOS does (`src/pluginKeys.ts`). Tried on 2026-10-30, not implemented:
    "forward unless X focus is in a plug-in child" would steal typing from LSP's text fields, which
    don't take X focus (from reading sources, unmeasured). Likely mechanism: a sync `XGrabKey` on the
    frame with `XAllowEvents` replay, on its own thread. Rule to be chosen by measuring whether the
    I-beam cursor is readable on JUCE and LSP; the fallbacks are chords only, or a per-window toggle.
    It needs a test under XWayland on a real display.
  - Only one Channel EQ analyzer per session: a detached EQ and another strip's EQ take turns.
  - CI: run `scripts/headless-checks.sh` on macos-latest too (it runs on macOS now), and open a
    draft PR from `dev` so the Linux headless step runs on GitHub's runners.
  - Signing and notarization on macOS (the entitlements are ready in `src-tauri/`).
  - Code style: about 60 clippy style warnings in the engine (`needless_range_loop`,
    `manual_is_multiple_of`, argument counts) and no rustfmt configuration (the code uses long
    lines, so plain `cargo fmt` would rewrite most files). A `rustfmt.toml` with a wide
    `max_width`, one formatting pass and a CI `cargo fmt --check` step would settle it.

## Stage 1: Release hygiene

Goal: prereleases that install cleanly and that people can trust with their projects.

- [ ] Code signing on Windows (Authenticode) and signing plus notarization on macOS
- [x] A LICENSE file (MIT, shipped in every bundle)
- [ ] Hands-on testing of the Windows and macOS builds: devices, MIDI, plug-in scanning,
      installers, file dialogs
- [x] Automation lanes follow a plug-in moved to another track
- [x] Make `takes_check` "autopunch trims the take" deterministic (it was a real race: a
      position read right after a locate could return the old playhead)
- [x] Run the headless checks that need no plug-ins in CI on Linux (`scripts/headless-checks.sh`)

## Stage 2: Plug-ins on every platform

Goal: a VST3 user on Windows or macOS gets the same workflow as on Linux.

- [ ] VST3 editor windows on Windows (HWND embedding)
- [x] VST3 editor windows on macOS (NSView embedding)
- [ ] VST3 editors under native Wayland (they run through XWayland now)
- [ ] HiDPI scaling for plug-in editors (`IPlugViewContentScaleSupport`)
- [ ] ASIO on Windows
- [x] Side-chain inputs for VST3 plug-ins
- [x] Multi-output instruments

## Stage 3: Audio editing

Goal: clean, fast audio editing and comping without workarounds.

- [x] Crossfades between adjacent regions, and automatic crossfades on edits
- [x] Fade curve shapes and fade types (equal power, speed-up, slow-down)
- [x] Drag modes: No Overlap, Shuffle Left and Right (Overlap and X-Fade are done)
- [x] Crossfades for take folders and looped regions; Logic's Crossfade Time and Curve settings
- [x] Marquee tool, a Tool menu in the tracks area, and Scissors, Glue, Fade, Pencil and Eraser
- [x] Region gain drag; snap edits to zero crossings
- [x] Region drags in bars and beats across tempo changes
- [x] Several comps per take folder; MIDI cycle recording into take folders
- [x] Move automation with regions
- [x] Trim AAC/M4A encoder delay exactly on import
- [x] Logic's arranging commands: select by criteria, section edits between the locators, Trim,
      Remove Gaps, Paste Replace, Strip Silence

## Stage 4: MIDI and control

Goal: precise MIDI editing and hands-on control from hardware.

- [x] MIDI learn and controller assignments for plug-in and mixer parameters
- [x] Event List
- [x] Step Input (Musical Typing, MIDI keyboard, rests)
- [x] Region playback parameters: non-destructive quantize, transpose, velocity, delay
- [x] Record MIDI in Replace mode
- [x] Chase notes that started before the playhead
- [x] Controller curve shapes

## Stage 5: Mixing and delivery

Goal: finish a mix and deliver it without leaving Fermata.

- [x] Channel strip settings; copy and paste a whole strip
- [x] Several automation lanes per track, automation curves, automation for folder stacks and
      Stereo Out
- [x] Undo for parameter changes made in a VST3's own editor
- [x] VCA faders
- [x] A pre-fader side-chain option for built-in effects
- [x] Clip indicators and a gain-reduction meter on the strip
- [x] FLAC and AIFF bounce; export the selected tracks
- [ ] MP3 and AAC bounce (needs an encoder whose licence suits)
- [x] Delay compensation changes while playing without clicks
- [x] More built-in effects: modulation (chorus, flanger, phaser, tremolo), distortion, a Gain
      utility
- [x] A linear-phase EQ

## Stage 6: Advanced production

Goal: the larger Logic features that set a full DAW apart.

- [ ] Flex Time
- [ ] Flex Pitch
- [x] Track alternatives; project alternatives and backups; user project templates
- [x] Arrangement track; markers with their own lengths
- [x] Step Sequencer: pattern regions, rows of steps, edit modes, polymeter, swing
- [ ] Score Editor

## Stage 7: Formats and reach

Goal: more plug-ins, more people.

- [ ] CLAP hosting
- [ ] Audio Units on macOS
- [ ] Screen reader support
- [ ] Translated interface
