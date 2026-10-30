# Changelog

All notable changes to Fermata are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Fermata uses
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Step Sequencer:** pattern regions on software instrument tracks (Track › New Pattern
  Region, or Create Pattern Region in an instrument track's right-click menu), edited as rows
  of steps in a new editor tab beside the Piano Roll and Event List (also in its own window).
  Note On, Velocity, Gate and Tie edit modes with click-and-drag painting, per-row pitch (with
  the General MIDI drum names and Learn), mute, solo, step rate and length (polymeter),
  pattern length, rate, swing and loop, and the playing step lit during playback. Pattern
  regions show their steps in the Tracks area, play in every render, and convert to and from
  MIDI regions.
- **Comps and MIDI take folders:** a take folder keeps several comps (Create New Comp,
  Duplicate Comp, Rename Comp, Delete Comp and the comp list in its menu; the name bar shows
  the comp heard). Settings › Recording › MIDI › Create take folders makes each MIDI cycle pass
  a take, and MIDI recordings over regions take folders; MIDI folders play whole takes. Take
  folders crossfade with their neighbours, loops crossfade at their loop end and optionally at
  every repetition (Crossfade Loop Repetitions), and Settings › Editing › Crossfade Time and
  Crossfade Curve set the take folder, X-Fade and Crossfade menu crossfades (comp boundaries
  are now 20 ms by default instead of 10 ms).
- **Linear Phase EQ** (EQ category): the Channel EQ's eight bands with linear phase, in the
  Channel EQ's own editor, with a **Quality** pop-up (Low, Medium, High: 21, 43 or 85 ms of
  latency at 48 kHz, made up by delay compensation). Settings change without clicks, follow
  automation, and render identically in bounces and freezes.
- **Delay compensation changes glide:** inserting or removing a latent plug-in, changing a
  plug-in's latency or switching Low Latency Mode while playing now crossfades each compensated
  path (track outputs, sends and auxes) to its new delay over 15 ms instead of clicking.
- **Arranging commands** from Logic's Edit menu (also in the Tracks area's Edit and Functions
  menus, with Logic's keys where free): **Select** All Following (Shift+F), Inside Locators,
  Muted, Overlapped, Same Colored, Equal or Similar Regions, Invert Selection (Shift+I);
  **Section Between Locators**: Insert Silence, Cut Section, Repeat Section and Insert Cut
  Section across every track (regions cut at the locators, automation, markers and arrangement
  sections move with their time), or within the marquee; **Trim**: region start or end to the
  playhead, end to the next region, Fill Within Locators, Remove Overlaps, Resize to Same
  Length, Crop Outside Locators or Marquee; **Move Region to Playhead** (;), Remove Gaps, Delete
  and Shuffle, Paste at Original Position, Paste Replace, Join Regions per Track, and naming or
  colouring regions by their track and tracks by their region.
- **Strip Silence** (Edit › Strip Silence…): splits audio regions around the parts above a
  threshold, with Logic's minimum time, pre-attack and post-release settings and a preview of
  the cuts on the region's waveform.
- **VCA faders:** Mixer › Options › Create New VCA, or a strip's new **VCA** slot. A VCA strip's
  fader offsets the level of every strip assigned to it without moving their faders (an
  outlined cap on each fader shows where it really plays), its Mute mutes them, and VCAs can be
  nested. Volume automation, bounces, exports and frozen tracks follow the VCA.
- **Pre-fader side-chains:** a **Pre / Post** switch beside the Side Chain pop-up of the
  Compressor, Noise Gate and De-Esser keys the effect from the source before its fader, so a
  trigger track pulled all the way down still works. Existing projects stay post-fader.
- **Arrangement track** (the top global track, View › Show Global Tracks): named sections
  (Intro, Verse, Chorus…) that tile the timeline, added with +, renamed from Logic's name
  pop-up, coloured by name, and resized by their right edge. Dragging a section moves it with
  its content (regions on every track cut at its edges, automation and markers) and Alt-drag
  copies it; a section can be deleted with or without its content. Clicking one selects its
  regions.
- **Markers with their own lengths:** set from the marker menu's Length submenu, the Marker
  List's new Length column, or by dragging the end of a marker's bar. Clicking a marker selects
  it (Delete removes it); with Cycle on, or dragged into the cycle strip, it sets the locators.
- **Automation lanes, curves, stacks and Stereo Out:** the **+** on a lane header opens more
  lanes below a track, each with its own parameter; **Show Used Automation** (Shift+A) shows
  every lane with data. Ctrl+Shift-drag (Control-Shift on macOS, or Alt+Shift) bends a segment
  into a curve; double-click straightens it. Folder stacks automate their volume and mute,
  and **Track › Show Output Track** shows Stereo Out with lanes for its volume and plug-ins.
  Bounces and exports render all of it.

- **Channel strip settings:** the **Setting** button at the top of each strip (and the strip
  name's right-click menu) copies and pastes a strip's instrument, Audio FX (VST3 state
  included) and Channel EQ, resets the strip, and saves settings to load in any project.
  Paste, Reset and loading apply to all selected strips as one undo step.
- **Clip indicators:** each strip's peak readout holds its highest peak until reset, turns red
  over 0 dBFS and lights the top of the meter (and the end of the track header's meter); click
  to reset, Alt-click (or the key command Reset All Clip Indicators) to reset all.
- **Gain-reduction meter** beside a strip's level meter when it has a Compressor, Limiter,
  Noise Gate or De-Esser.
- **Tracks-area tools:** Left-click and Ctrl-click (⌘-click) Tool pop-ups in the Tracks
  area's bar and Logic's Tool menu (T, then the tool's letter), with the Pointer, Pencil
  (empty MIDI regions), Eraser, Scissors (Alt+click for equal pieces), Glue, Mute, Fade (fades
  and crossfades by dragging) and Marquee tools, each with its own pointer.
- **Marquee selection:** a time range across tracks (the Ctrl-click tool by default). Delete,
  Cut, Copy and Paste act on the part of the regions inside it, and a Pointer click inside
  splits the regions at its edges.
- **MIDI Learn and controller assignments** (Fermata › Control Surfaces): Learn Assignment
  for Last Touched Parameter (Ctrl+L) or a control's right-click Learn MIDI Assignment assigns
  the next control change or pitch bend to a fader, pan, send, Mute, Solo, Record, Stereo Out,
  Channel EQ setting, built-in effect or VST3 parameter, or a transport button. Assigned
  controllers move their parameter like the mouse (written as automation in Touch, Latch and
  Write) and don't reach the instrument. The Controller Assignments window (Ctrl+Shift+L) lists,
  edits (Jump or Pickup, range) and deletes them; they're saved with the settings.
- **Step Input Keyboard** (View › Step Input Keyboard, Ctrl+Alt+Shift+K, or the Piano Roll's
  bar): enter notes one step at a time at the playhead, from its own keyboard, a MIDI keyboard
  or Musical Typing, with note values 1/1 to 1/128 (dotted, triplet), velocities ppp to fff or
  as played, Rest, Back, Chord, Sustain and Quantize, and key commands for them. Notes go into
  the Piano Roll's region under the playhead, which grows or is made as needed; each step is
  one undo step.
- **Record MIDI in Replace mode:** Settings › Recording › Overlapping Recordings › MIDI ›
  Replace clears the notes and controllers in the recorded range of the regions underneath,
  and a **Replace** button in the control bar makes every recording, audio and MIDI, replace.
  Merge now merges recorded controllers too, not only notes.
- **Chase: Notes:** starting playback, jumping or cycling back in the middle of a long note
  starts it, as in Logic. Settings › MIDI › Chase switches note and controller chase.
- **Project alternatives** (File › Project Alternatives): versions of a project in one
  folder, sharing its audio files: New Alternative…, switching (the current one is saved
  first), and Edit Alternatives… to rename or delete them.
- **Backups and File › Revert to:** each save keeps the version it replaces, per alternative
  (Settings › General › Backups, 10 by default), listed by date and time; reverting keeps the
  current state as a backup too. **File › Revert to Saved.**
- **User project templates:** File › Save as Template… saves the tracks, routing, plug-ins and
  settings without regions; they appear under My Templates in the Project Chooser, where a
  right-click renames or deletes them.
- **Track alternatives** (Track › Track Alternatives and a pop-up in the track header): New,
  Duplicate, Rename and Delete sets of regions for a track, and switch between them, with undo.

- **AIFF and FLAC bounces:** Bounce, Export All Tracks and Export Region can write AIFF (16,
  24-bit or 32-bit float as AIFF-C) and lossless FLAC (16 or 24-bit) as well as WAV; the
  format is remembered with the other bounce options.
- **File › Export Selected Tracks as Audio Files…** (Ctrl+E): Export All Tracks for just the
  selected tracks.
- **Region playback parameters** in the Inspector's Region section, as in Logic: Quantize
  (straight and triplet values) with Q-Swing, Transpose, Velocity, Dynamics, Gate Time and
  Delay (ticks or ms) for MIDI regions, and Delay for audio regions. They apply at playback
  and in every render, the Tracks area draws notes as they play, and region menu › Apply
  Region Parameters writes them into the regions.
- **Event List** (D, View › List Editors, or the Event List tab beside the Piano Roll): the
  notes, controllers, pitch bend and channel pressure of the selected MIDI regions as a list,
  with Position and Length in bars, beats, divisions and ticks through the tempo map. Drag any
  value or double-click to type it, filter by event type, add events at the playhead with +,
  delete with Backspace; the event at the playhead is highlighted and Catch follows it. The
  note selection is shared with the Piano Roll, and the list can have a window of its own.
- **More built-in effects:** **Chorus**, **Flanger**, **Phaser** and **Tremolo** (Modulation;
  the Tremolo syncs to the tempo, locked to the bar), **Overdrive** and **Distortion**
  (Distortion; Soft, Hard, Tube and Fuzz curves, 4× oversampled, level-compensated drive) and
  **Gain** (Utility: gain, phase invert, swap, mono, balance). Each has its panel, automation,
  factory presets and freeze and bounce support.
- **Fades and crossfades:** a curve for each fade, Speed Up and Slow Down fades, and X, EqP
  and X S crossfades between adjacent or overlapping regions (Inspector, the region menu's
  Crossfade item, or the handle on the canvas). The X-Fade drag mode crossfades overlaps
  automatically. Bounce, export, freeze and Join render them as playback does.
- **Overview:** the Tracks area's horizontal scroll bar is now a miniature of the whole
  project (regions, playhead, cycle, markers, project end) with a box for the visible area:
  drag it to scroll, drag its edges to zoom, click elsewhere to jump there (View › Show
  Overview).
- **Drag modes No Overlap, Shuffle R and Shuffle L** (the Drag pop-up in the Tracks area):
  No Overlap shortens a region that an edit overlaps; the Shuffle modes keep a track's regions
  butted together, closing gaps on delete and trim and swapping neighbours on a drag. Drags
  are drawn where they will land.
- **Settings › Editing › Move Automation with Regions** (Never, Always, Ask): a region moved
  along its track takes the automation under it along, in the same undo step.
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
  Keys the plug-in doesn't use (Space, key commands, Musical Typing) reach Fermata.
- A plug-in whose window can't open when it's inserted now says why, instead of only logging it.
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
- **Region gain handle:** hovering over an audio region shows its gain at the top centre of
  its name bar; drag it up or down (Shift for fine steps) to change the gain of every selected
  audio region by the same amount, with the waveform following, or double-click it for 0 dB.
  A drag is one undo step.
- **Snap Edits to Zero Crossings** (Edit menu, and a key command): splits, trims and range
  edits of audio regions move to the nearest zero crossing within 5 ms. Saved with the project.
- **Controller curves** in the Piano Roll's controller lane: the Line tool draws ramps along a
  Linear, Exponential, Logarithmic or S-Curve shape at a chosen Ramp Density, joins the
  points it starts and ends on, and Create Ramp and Thin Out fill and thin the selected points.

### Changed

- Tracks can be as short as 22 px (View › Track Height has a new Mini size). Below 52 px the
  header goes onto one line and its M, S, R, I and Freeze buttons move into a ⋯ menu; the ones
  that are on stay visible.
- The track header's volume slider and level meter are one taller control: the meter fills
  the slider's groove.
- Automation lanes move with a plug-in dragged to another track's strip; one undo reverts both.
- Regions moved across tempo changes keep their bars and beats: with Snap on, the other
  regions of a drag (and Alt-drag copies) follow the grabbed one in bars and beats rather than
  in samples, and nudges by a bar, beat or note value count through the tempo map.
- Waveforms in the tracks area are drawn at their region's gain.

### Fixed

- The Mixer cut off the bottom of its channel strips when it was too short, with no way to
  scroll to them; it now scrolls, and every strip ends on the same line.
- A plug-in that failed to load reported only "dlopen failed"; the report now gives the reason,
  including a plug-in built only for Intel on Apple Silicon.
- The Loudness Meter drew "LUFS" and "LU" over its values.
- The Compressor's circuit tabs and other segmented buttons didn't show the current choice.
- Opening Group Settings (including from New Group from Track) blanked the window.
- Play or Record right after moving the playhead could start from the old position.
- M4A/MP4 imports started about 40 ms late: the encoder delay and padding are now trimmed
  exactly, from the iTunes gapless tag or the file's edit list.

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
