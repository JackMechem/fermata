# Fermata roadmap

What's left, in the order it matters for a usable release. Each stage builds on the one before.
The full Logic Pro feature scope is in [LOGIC_FEATURES.md](LOGIC_FEATURES.md); known limitations
of what exists are in the [User Guide](docs/USER_GUIDE.md#19-known-limitations).

## Open items (2026-10-29)

Left over from finishing the effect panels, separate windows and macOS plug-in work. Nothing
here has been checked by eye in the app yet unless it says so.

- **Look at in the running app:**
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
- **Small fixes:**
  - Check a VCA on a subtrack of a folder stack whose own volume is automated: the folder's
    automation (`apply_fold`) replaces the static stack values, which is also where the VCA's
    offset is folded in (`track_params`), so the VCA may be ignored while the folder lane plays.
  - The Compressor's "Lookahead" and the Classic Opto's "Peak Reduction" labels may overlap
    their knobs by a pixel or two.
- **Larger:**
  - The settings folder is `~/.config/fermata` on macOS too; `~/Library/Application Support`
    is the native place, but moving it needs a migration.
  - VST3 editor windows on Linux don't pass unused keys to Fermata (the plug-in's child window
    takes KeyPress); macOS does (`src/pluginKeys.ts`).
  - Only one Channel EQ analyzer per session: a detached EQ and another strip's EQ take turns.
  - CI: run `scripts/headless-checks.sh` on macos-latest too (it runs on macOS now), and open a
    draft PR from `dev` so the Linux headless step runs on GitHub's runners.
  - Signing and notarization on macOS (the entitlements are ready in `src-tauri/`).

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
- [ ] Side-chain inputs for VST3 plug-ins (their side-chain buses stay inactive)
- [ ] Multi-output instruments

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
- [ ] Undo for parameter changes made in a VST3's own editor
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
