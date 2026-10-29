# Fermata roadmap

What's left, in the order it matters for a usable release. Each stage builds on the one before.
The full Logic Pro feature scope is in [LOGIC_FEATURES.md](LOGIC_FEATURES.md); known limitations
of what exists are in the [User Guide](docs/USER_GUIDE.md#19-known-limitations).

## Stage 1: Release hygiene

Goal: prereleases that install cleanly and that people can trust with their projects.

- [ ] Code signing on Windows (Authenticode) and signing plus notarization on macOS
- [ ] A LICENSE file (the Cargo and Nix metadata declare MIT, but the repository has none)
- [ ] Hands-on testing of the Windows and macOS builds: devices, MIDI, plug-in scanning,
      installers, file dialogs
- [ ] Automation lanes follow a plug-in moved to another track (today they stay behind on the
      old track, keyed to the old instance)
- [ ] Make `takes_check` "autopunch trims the take" deterministic (fails about 1 in 5 runs on the
      null device's timing)
- [ ] Run the headless checks that need no plug-ins in CI on Linux

## Stage 2: Plug-ins on every platform

Goal: a VST3 user on Windows or macOS gets the same workflow as on Linux.

- [ ] VST3 editor windows on Windows (HWND embedding)
- [ ] VST3 editor windows on macOS (NSView embedding)
- [ ] VST3 editors under native Wayland (they run through XWayland now)
- [ ] HiDPI scaling for plug-in editors (`IPlugViewContentScaleSupport`)
- [ ] ASIO on Windows
- [ ] Side-chain inputs for VST3 plug-ins (their side-chain buses stay inactive)
- [ ] Multi-output instruments

## Stage 3: Audio editing

Goal: clean, fast audio editing and comping without workarounds.

- [ ] Crossfades between adjacent regions, and automatic crossfades on edits
- [ ] Fade curve shapes and fade types (equal power, speed-up, slow-down)
- [ ] Drag modes: No Overlap, X-Fade, Shuffle Left and Right
- [ ] Marquee tool, a Tool menu in the tracks area, and Scissors, Glue, Fade, Pencil and Eraser
- [ ] Region gain drag; snap edits to zero crossings
- [ ] Region drags in bars and beats across tempo changes
- [ ] Several comps per take folder; MIDI cycle recording into take folders
- [ ] Move automation with regions
- [ ] Trim AAC/M4A encoder delay exactly on import

## Stage 4: MIDI and control

Goal: precise MIDI editing and hands-on control from hardware.

- [ ] MIDI learn and controller assignments for plug-in and mixer parameters
- [ ] Event List
- [ ] Step Input (Musical Typing, MIDI keyboard, rests)
- [ ] Region playback parameters: non-destructive quantize, transpose, velocity, delay
- [ ] Record MIDI in Replace mode
- [ ] Controller curve shapes; chase notes that started before the playhead

## Stage 5: Mixing and delivery

Goal: finish a mix and deliver it without leaving Fermata.

- [ ] Channel strip settings; copy and paste a whole strip
- [ ] Several automation lanes per track, automation curves, automation for folder stacks and
      Stereo Out
- [ ] Undo for parameter changes made in a VST3's own editor
- [ ] VCA faders
- [ ] A pre-fader side-chain option for built-in effects
- [ ] Clip indicators and a gain-reduction meter on the strip
- [ ] FLAC, AIFF, MP3 and AAC bounce; export the selected tracks
- [ ] Delay compensation changes while playing without clicks
- [ ] More built-in effects: modulation (chorus, flanger, phaser, tremolo), distortion, a Gain
      utility, a linear-phase EQ

## Stage 6: Advanced production

Goal: the larger Logic features that set a full DAW apart.

- [ ] Flex Time
- [ ] Flex Pitch
- [ ] Track alternatives; project alternatives and backups; user project templates
- [ ] Arrangement track; markers with their own lengths
- [ ] Score Editor and Step Sequencer

## Stage 7: Formats and reach

Goal: more plug-ins, more people.

- [ ] CLAP hosting
- [ ] Audio Units on macOS
- [ ] Screen reader support
- [ ] Translated interface
