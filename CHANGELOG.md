# Changelog

All notable changes to Fermata are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Fermata uses
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Fades and crossfades:** a curve for each fade, Speed Up and Slow Down fades, and X, EqP
  and X S crossfades between adjacent or overlapping regions (Inspector, the region menu's
  Crossfade item, or the handle on the canvas). The X-Fade drag mode crossfades overlaps
  automatically. Bounce, export, freeze and Join render them as playback does.
- **Overview:** the Tracks area's horizontal scroll bar is now a miniature of the whole
  project (regions, playhead, cycle, markers, project end) with a box for the visible area:
  drag it to scroll, drag its edges to zoom, click elsewhere to jump there (View › Show
  Overview).
- **Scroll to Playhead** button beside Catch (Alt+\`); a second Stop also brings the playhead into view.
- **Settings › Editing › Mouse Wheel Scrolls:** Horizontally (time) makes a mouse's vertical
  wheel scroll through time in the Tracks area and Piano Roll, with Shift for vertical.
- **Autosave after recording** (Settings › General › After Recording, on by default): the
  autosave is written as soon as a take is placed, and after Bounce in Place, Freeze and
  imports.
- **Recovering an interrupted recording:** takes being recorded when Fermata crashed, was
  killed or lost power are repaired and offered in the Recover sheet, placed where they would
  have landed (cycle takes as a take folder).
- Saves are atomic and keep the previous `project.json` as `project.json.bak`; a damaged
  project file falls back to the autosave or the backup. A panic on the main thread writes an
  emergency autosave.
- **Separate windows:** the Mixer, the Editors (Piano Roll, Audio Track Editor), built-in
  effect panels and the Channel EQ can be detached into windows of their own, for example on
  another monitor (the pop-out button in their bar, or View › Mixer / Editor in Separate
  Window). They stay in sync with the main window, reopen where they were, and come back onto
  a connected monitor if theirs is gone.
- **Docked effect panels:** built-in effect panels and the Channel EQ can dock in the bottom
  section, with a tab for each, or float (Settings › General › Open Effect Panels; Floating by
  default), or open in separate OS windows. Floating panels resize from their sides and bottom corners, their graphs and meters
  grow with them, and each effect type remembers its last size.
- **VST3 editor windows on macOS:** a plug-in's own interface opens in a window that floats
  above Fermata's, resizes within the plug-in's limits and hides while another app is in front.
- **Missing plug-ins:** a project opened where a plug-in isn't installed opens and plays
  without it, says which are missing, shows their slots struck through and keeps their
  settings. A plug-in installed in another folder than on the computer that saved the project
  (another OS, say) is found by itself.
- macOS: recording asks for microphone access, and builds carry the entitlements a signed app
  needs to load third-party plug-ins.
- The headless checks run on macOS too.
- A LICENSE file (MIT), shipped in every bundle.
- CI runs the headless checks that need no third-party plug-ins on Linux
  (`scripts/headless-checks.sh`).

### Changed

- Tracks can be as short as 22 px (View › Track Height has a new Mini size). Below 52 px the
  header goes onto one line and its M, S, R, I and Freeze buttons move into a ⋯ menu; the ones
  that are on stay visible.
- The track header's volume slider and level meter are one taller control: the meter fills
  the slider's groove.
- Automation lanes move with a plug-in dragged to another track's strip; one undo reverts both.

### Fixed

- The Mixer cut off the bottom of its channel strips when it was too short, with no way to
  scroll to them; it now scrolls, and every strip ends on the same line.
- A plug-in that failed to load reported only "dlopen failed"; the report now gives the reason,
  including a plug-in built only for Intel on Apple Silicon.
- The Loudness Meter drew "LUFS" and "LU" over its values.
- The Compressor's circuit tabs and other segmented buttons didn't show the current choice.
- Opening Group Settings (including from New Group from Track) blanked the window.
- Play or Record right after moving the playhead could start from the old position.

## [0.1.0-alpha.1] - 2026-10-29

The first prerelease. Everything below is new.

### Added

- **Recording:** audio from any input, mono or stereo per track, with live waveforms and
  latency compensation from the measured round trip plus a manual Recording Delay; MIDI with
  controllers, pitch bend and aftertouch; count-in and pre-roll; cycle recording into take
  folders; Autopunch; on top, new track or replace for overlapping recordings.
- **Take folders and comping:** Quick Swipe Comping with crossfaded boundaries; Flatten,
  Flatten and Merge, Pack and Unpack.
- **Tracks area:** move, trim, split, join, loop, fade, gain, reverse, colour, copy, paste,
  repeat, nudge and Alt-drag copy; rubber-band selection; Smart snap; a click without a drag
  moves the playhead (Settings › Editing › Click Moves Playhead).
- **Tracks:** audio, instrument and aux tracks; folder and summing stacks; groups; hidden
  tracks; Track Freeze.
- **Tempo and markers:** Tempo and Signature global tracks with tempo changes, linear ramps and
  signature changes, followed by the grid, click, quantize, plug-ins and MIDI files; Move MIDI
  Regions with Tempo; a Marker track and Marker List.
- **Editors:** Piano Roll with eight tools, Time Quantize with strength and swing, scale
  quantize, humanize and a velocity/controller lane; Audio Track Editor with range edits,
  normalize, fades to selection and Split at Transients; Standard MIDI File import and export.
- **Mixing:** channel strips with an eight-band Channel EQ and analyzer, eight Audio FX slots,
  eight pre- or post-fader sends, aux buses and output routing; plug-in delay compensation on
  every path, with Low Latency Mode.
- **Automation:** volume, pan, mute, send levels and plug-in parameters, in Off, Read, Touch,
  Latch and Write modes.
- **Plug-ins:** VST3 instruments and effects, scanned out of process with a scan report;
  Fermata Synth; copy and paste of plug-ins (VST3 state included) and Channel EQ settings from
  the right-click menus; dragging plug-ins between Audio FX slots to reorder them, move them to
  another strip, or copy them with Alt.
- **Built-in effects:** Compressor (Platinum Digital, Studio VCA, Studio FET, Classic Opto and
  Vintage Vari-Mu circuits), Limiter, Noise Gate, De-Esser, Reverb, Delay, Tuner and Loudness
  Meter, with their own panels, automation, side-chain input from any track (Compressor, Noise
  Gate, De-Esser), latency reported to delay compensation, and freeze support.
- **Presets:** factory and user presets for every built-in effect and the Channel EQ. User
  presets are files in the config folder's `presets/` and are shared by every project.
- **Bounce and export:** offline Bounce of the project or a section, Export All Tracks, Export
  Region, Bounce in Place; 16-bit dithered, 24-bit or 32-bit float WAV.
- **Projects:** `.fermata` folders with readable `project.json`; Project Chooser with templates;
  autosave and crash recovery; undo for every edit (100 steps by default).
- **Interface:** Logic Pro 11's layout; five themes; interface scaling from 90 to 200 %; four
  waveform styles; rebindable key commands, shown the way each platform writes them; Musical
  Typing.
- **Real-time safety:** wait-free `rtrb` queues between the session and the audio thread; a
  panic in the engine silences one block instead of crashing; every plug-in's output is
  checked, NaNs are silenced, runaway levels are clamped, and a message names the plug-in.
- **Denormals:** the audio thread runs with flush-to-zero (and denormals-are-zero on x86_64),
  so decaying signals no longer slow DSP down about a hundredfold.
- **Overload protection:** when callbacks run late back to back for 60 ms, the engine outputs
  silence for 250 ms without processing, stops playback or recording, and shows "System
  overload…". Before, the kernel killed the app (SIGXCPU, `RLIMIT_RTTIME`). Offline renders are
  exempt.
- **Tuner:** gates on its DC-blocked level, so an interface's DC offset doesn't run the
  detector.
- **About Fermata** in the Fermata menu, with the version.
- **Platforms and packaging:** Linux x86_64 and aarch64 (AppImage and .deb; .rpm from the first
  stable release, since RPM versions can't carry a prerelease suffix) and a Nix
  flake, Windows (NSIS installer, x64) and macOS (.dmg for Apple Silicon and Intel), built and
  published by GitHub Actions; CI on all three; `scripts/set-version.sh` sets the version
  everywhere.

### Known issues

- VST3 editor windows open only on Linux (X11, through XWayland on Wayland). On Windows and
  macOS plug-ins load, play and can be automated, but show no editor.
- Windows and macOS builds are early and less tested. Only the system's default audio driver is
  available there (WASAPI, Core Audio; no ASIO).
- The builds are unsigned: macOS needs right-click › Open (or `xattr -cr`), Windows
  SmartScreen needs More info › Run anyway.
- Automation lanes of a plug-in dragged to another track stay on the old track and no longer
  drive it.
- No crossfades, fade curve shapes, drag modes (X-Fade, Shuffle) or Marquee tool.
- Regions stay at fixed sample positions when the tempo changes; only MIDI can follow (Move MIDI
  Regions with Tempo).
- A built-in effect's side-chain key is always post-fader; VST3 side-chain buses are not
  connected.
- AAC/M4A imports can start about 40 ms late (encoder delay not fully trimmed).
- The `takes_check` headless check "autopunch trims the take" fails about one run in five,
  from the null audio device's timing.

[Unreleased]: https://github.com/JackMechem/fermata/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/JackMechem/fermata/releases/tag/v0.1.0-alpha.1
