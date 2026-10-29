# Fermata

A cross-platform DAW modelled on Logic Pro's workflow and layout, written in Rust with a
Tauri + React interface.

This is the first iteration. It covers the foundation: recording from an audio interface,
hosting VST3 plug-ins, MIDI input for software instruments, and a main window laid out like
Logic Pro 11's.

## Running

### NixOS / Nix (flake)

```sh
nix run .                 # build and launch Fermata
nix build .               # result/bin/fermata, plus the desktop entry and icons
nix profile install .     # install it for your user
nix flake check           # build and run the engine's unit tests
```

For development:

```sh
nix develop               # or `nix-shell`: Rust, Node, WebKitGTK, ALSA/JACK/PipeWire
npm install
npm run tauri dev         # or: npm run tauri dev -- -- path/to/Song.fermata
```

To add it to a NixOS or home-manager configuration, take the flake as an input and use
`fermata.packages.${system}.default`, or apply `fermata.overlays.default` and add `pkgs.fermata`.
For real-time audio scheduling, enable rtkit (`security.rtkit.enable = true;`), which PipeWire
setups usually already do.

The packaging lives in `nix/package.nix` and the shell in `nix/shell.nix`. After changing npm
dependencies, refresh `npmDeps.hash` with `nix run nixpkgs#prefetch-npm-deps -- package-lock.json`.
Rust dependencies are read straight from `Cargo.lock`. Flakes only see files git tracks, so
`git add` new files before building.

### Other Linux distributions

Install the Tauri prerequisites (`webkit2gtk-4.1`, `gtk3`, `libsoup-3`) plus `alsa-lib`,
`libpipewire-0.3`, `dbus`, `jack` and `libclang` (used to generate the PipeWire bindings), then
run `npm install && npm run tauri dev`.

### Windows / macOS

The audio engine (cpal), MIDI (midir), VST3 module loading and the UI are cross-platform. Plug-in
editor windows are only implemented for Linux/X11 so far (see *Known limitations*).

`npm run tauri build` produces native bundles.

## What works

**Audio I/O.** Settings › Audio (`Ctrl/Cmd-,`) picks the driver, the input and output devices,
the sample rate and the I/O buffer size. Input and output can be different devices.

- On Linux the drivers are PipeWire (the default and recommended one), PulseAudio, JACK (when a
  server is running) and ALSA for direct hardware access.
- The audio thread runs with real-time priority via rtkit.

**Monitoring and latency.**
- A track's **I** button plays its input through Fermata and its plug-ins.
- Record-enabled tracks don't monitor automatically unless you turn that on in Settings. This
  avoids the hollow, doubled sound you get when an interface or pedal already monitors directly.
- Input and output latency are measured live, and recordings are shifted earlier by the
  round-trip amount so they line up with what you heard. MIDI is shifted by the output latency.
- A manual Recording Delay adds a further offset.

**Recording.**
- Arm a track (**R**), then press **R** to record. Recording also punches in on the fly if
  you're already playing.
- Inputs are mono or stereo, chosen per track from the channel strip's Input slot.
- Takes are written to the project's `Audio Files/` folder as 32-bit float WAV.
- Regions draw live while recording.
- Count-in and metronome are available.
- A newer region hides the older audio it overlaps during playback, as in Logic. The older
  regions are not deleted.
- With Cycle on, recording starts at the left locator and loops: every pass becomes a take in
  a take folder (see below). MIDI passes are merged into one region, as Logic does by default.
- Autopunch records only between the punch-in and punch-out points (see below).

**Markers.** The marker lane at the bottom of the ruler is Logic's Marker track.
- Create Marker (⌥') adds a marker at the playhead, rounded to the nearest bar. Double-clicking
  the empty lane creates one there; Navigate › Delete Marker removes the one at or before the
  playhead.
- Each marker shows its name in its colour, with a faint band reaching the next marker. Click a
  marker to go to it, drag it to move it (snapped), double-click it to rename it. Right-click
  for Go to Marker, Set Locators to Marker (the cycle covers its section), Rename, Color and
  Delete.
- Go to Previous / Next Marker are ⌥, and ⌥. (Navigate menu).
- The Marker List (⌥⌘K, Navigate › Open Marker List) lists every marker with its position, an
  editable name, Go To and Delete.
- Markers are saved with the project, and every marker edit can be undone.

**Autopunch.** As in Logic, the punch range sets where recording happens, so you can start
playback early and only replace a phrase.
- The Autopunch button next to Cycle (⌃⌘P) switches it on. With no range yet, it takes the
  cycle's range, or the two bars from the playhead.
- The range is the red strip below the bar numbers: drag in the strip to set it, drag it or
  its edges to move or resize it, and click it to switch Autopunch off and on.
- While recording, input is only captured between punch in and punch out. Record-enabled
  tracks keep playing their regions before and after the range, so you hear the old take
  until the punch, and input monitoring follows the usual rules. The take is trimmed exactly
  to the range, after the usual latency compensation (the engine keeps capturing for the
  latency's length past the punch-out, so the compensated take still reaches it). MIDI notes
  that start outside the range are dropped, and none outlasts the punch-out.
- With Cycle on as well, every pass is punched, and the takes go into a take folder over the
  punch range.

**Take folders and comping**, modelled on Logic's Quick Swipe Comping:
- Recording with Cycle on loops the transport. When it stops, the passes become the takes of a
  *take folder* on the track: "Take 1", "Take 2"… A last pass stopped part-way is kept if it
  got at least a beat in. Recording the same cycle again adds its passes to the folder.
  A single pass is a normal region.
- Select audio regions on one track and use Pack Take Folder (⌃⌘F, or right-click) to make a
  folder from them, one take each.
- The folder plays its *comp*: which take is heard where. A new folder starts with the newest
  take, older takes filling in where it has no audio. Where one take hands over to another,
  they crossfade over 10 ms (equal power), so boundaries don't click.
- The disclosure triangle at the start of the folder's name bar opens it: the track grows by a
  row per take below its lane. The badge next to it shows the take heard (or C for a comp).
  - Drag across a take row to put that stretch of the take into the comp (snapped to the
    grid). Comp parts are drawn bright and the rest dimmed, and the closed folder draws the
    comp's waveform.
  - Click a take row to hear that take throughout.
- The take folder menu (click the badge, right-click a take row, or Take Folder in the region
  menu): pick the take to hear, Open / Close, Rename Take, Delete Take, Delete Unused Takes,
  Flatten (the comp becomes ordinary regions, one per stretch), Flatten and Merge (the comp is
  rendered to one new audio file), and Unpack to New Tracks (every take on its own new track,
  muted, and the comp flattened in place).
- A folder moves, trims, splits, loops, fades and changes gain like any audio region. Every
  comp edit can be undone, folders are saved with the project, and bounces, exports and Bounce
  in Place render the comp.

**VST3 hosting.**
- Bundles are scanned out-of-process, so a crashing plug-in can't take the app down, and the
  results are cached.
- Plug-ins load into Audio FX slots or the instrument slot.
- Each slot has bypass and opens the plug-in's own editor window.
- Plug-in state is saved in the project.
- Supports separate component/controller plug-ins, host messages, parameter round-tripping,
  and MIDI CC / pitch bend through `IMidiMapping`.

**MIDI.**
- Every MIDI input is connected and routed to the selected software-instrument track.
- MIDI is recorded into regions on armed instrument tracks and played back, notes together
  with controllers (CC), pitch bend and aftertouch.
- **Fermata Synth** is a built-in polyphonic synth, so instrument tracks make sound without a
  VST3 instrument installed.
- **Musical Typing (⌘K)** plays the selected instrument track without a MIDI keyboard.
  - A–' play the white keys and W E T Y U O P the black ones. Keys are mapped by position, so it
    works on any keyboard layout.
  - Z / X shift the octave, C / V change velocity, and holding Tab is the sustain pedal.
  - Notes can also be clicked or swept on the four-octave on-screen piano. Clicking lower on a key
    plays louder.
  - Notes played this way are recorded like those from a real keyboard.

**Main window, laid out after Logic Pro 11.**
- **Control bar:** view buttons, transport, the LCD (bar/beat, tempo, signature and key; drag or
  double-click to edit), Cycle, Autopunch, Count-in, Metronome, master volume, and CPU.
- **Tracks area:**
  - Local Edit/Functions/View menus, Snap, Catch and zoom.
  - Ruler with the yellow cycle strip: drag to create, move or resize it; click to toggle.
    Below the bar numbers are the red Autopunch strip and the marker lane.
  - Track headers with M/S/R/I, a volume slider with level meter, rename, colour, and
    drag-to-reorder.
  - Resizable tracks:
    - Drag a header's bottom edge to resize that track; Alt-drag resizes every track, and
      double-clicking the edge resets it.
    - ⌘↓ / ⌘↑ or Alt-scroll make all tracks taller or shorter, and there's a height slider in the
      local menu bar.
    - Heights are remembered per project.
  - Regions with name bars, waveforms and MIDI note dashes. Regions can be moved (also across
    tracks), trimmed, split, muted, deleted and renamed.
  - Track selection: click, ⌘/Ctrl-click to add or remove, Shift-click for a range.
- **Track operations** (right-click a track header, or the Track menu):
  - Duplicate Track (⌘D) copies settings, plug-ins with their current settings, and regions.
    New Track with Same Settings copies everything except the regions.
  - Rename, colour, mute/solo/record-enable the selection, select all regions on the track.
  - Hide Track (⌃H) hides the selected tracks and selects the next one, as in Logic. Hidden
    tracks keep playing, and hiding a stack hides its subtracks. View › Show Hidden Tracks (H,
    or the H button above the headers) shows them dimmed and puts an H button on every header
    for hiding and showing tracks one by one. Unhide All Tracks brings them all back. The mixer
    leaves hidden tracks out too.
  - Delete Track(s) (⌘⌫) and Delete Unused Tracks.
- **Track Stacks** (⇧⌘D, or right-click › Create Track Stack), as in Logic:
  - A *Folder Stack* groups tracks under a collapsible main track. Its fader moves every
    subtrack by the same amount, and its mute and solo apply to all of them.
  - A *Summing Stack* is also a bus: its subtracks play through its own EQ, plug-ins, pan and
    fader. Soloing a subtrack keeps its stack audible.
  - The disclosure triangle shows or hides the subtracks. A stack's lane shows where its
    subtracks have regions. Stacks can be nested.
  - Drag a track under a stack's header or between its subtracks to put it in. Use Remove from
    Stack to take it out, or Flatten Stack to remove the stack and keep its tracks. Deleting a
    stack deletes its subtracks (undoable).
- **Sends and aux buses**, as in Logic:
  - An *aux* is a channel strip with no regions of its own: EQ, Audio FX, sends, pan, fader,
    mute/solo and an output. New auxes are named "Aux N", are fed by "Bus N" (shown in their
    Input slot) and go at the end of the track list. Create one from Track › New Aux, the track
    menu, or "New Aux" in any send or output menu.
  - Every strip except folders and Stereo Out has eight send slots under Audio FX. Click an
    empty slot to pick an aux ("Bus 1 → Aux 1") or make a new one. A filled slot shows the bus
    (or the aux's name once renamed) and a level knob: drag it (Shift for fine), double-click
    for 0 dB. Click the name for Pre Fader / Post Fader, Mute Send, Send to, and Remove Send.
  - Post-fader sends follow the fader and pan. Pre-fader sends tap the signal after the plug-ins
    and keep sending with the fader down. Muting or soloing-out a track silences both kinds.
  - The Output slot routes a strip to Stereo Out or an aux. A summing stack's subtracks play
    through the stack by default; an explicit output overrides that (their menu also offers
    Stereo Out, to skip the stack).
  - Soloing a track keeps the auxes it feeds audible, and soloing an aux keeps the tracks
    feeding it audible. Muting an aux silences everything arriving there.
  - Feedback loops are refused with a message. Deleting an aux clears the sends and outputs
    pointing at it (undoable). Delete Unused Tracks keeps auxes that something still feeds.
  - Auxes appear in the mixer after the tracks, with an Aux filter button. They stay out of
    track stacks.
- **Track Groups** (⌘G, or right-click › Group):
  - Members can share volume and pan (as relative moves), mute, solo, record enable, track
    selection, and region selection (clicking a region selects the ones at the same place on
    the other members).
  - Group Settings (click the G badge on a header) sets the name, colour and links, and turns
    the group off without dissolving it, like Logic's group clutch.
- **Region operations** (right-click a region, the Edit menu, or keys):
  - Cut, copy and paste (⌘X/C/V; paste goes to the playhead on the selected track, keeping the
    spacing of several regions), Duplicate (⌘D) and Repeat 2–16 times.
  - Split at playhead (⌘T) and Join (⌘J). Joining MIDI merges the notes. Joining audio mixes
    the regions, as they play, into a new file.
  - Mute (⌃M), rename, and region colours.
  - Audio: gain (±1/3/6 dB, reset, Normalize to 0 dBFS), fade in and out (presets, the
    inspector, or the handles at the top corners of the region), and Reverse (a new file, so the
    original stays intact).
  - MIDI: quantize to 1/4–1/32 notes and transpose by semitones or octaves.
  - Loop (L), as in Logic: a looping region repeats up to its loop end, drawn lighter with a
    notch at each repetition. Turning Loop on loops up to the next region on the track (or four
    times the region's length when nothing follows); the inspector's Loop box does the same.
    - Point at the top right corner of a region to get the loop pointer, then drag to set where
      the loop stops (snapped). Dragging back to the region's end removes the loop.
    - Click a repetition to select the region, drag it to move the region (the loop moves
      along). Trimming changes what repeats and keeps the loop end.
    - The fade-in shapes the start of the region and the fade-out the end of the loop. Loops
      play beneath other regions on the track, so a region over a loop always wins.
    - Convert Loops to Real Copies turns the repetitions into regions. Split, Join, Repeat and
      Paste work on the whole looped length; splitting a loop converts it first.
  - Move to New Track, Set Locators by Regions (U), and select all / all following on the track.
- **Empty lane** (right-click): paste here, create an empty MIDI region, or import an audio
  file at that spot.
- **Inspector:** region and track sections, with the selected track's channel strip next to
  Stereo Out.
- **Mixer (X):** a channel strip for every track plus Stereo Out.
- **Channel EQ:** every strip, including Stereo Out, has a built-in eight-band EQ modelled on
  Logic's Channel EQ.
  - A thumbnail of the curve sits above the Audio FX slots; click it to open the editor.
  - Bands: low cut, low shelf, four bell bands, high shelf and high cut. The cuts have 12, 24 or
    48 dB/oct slopes.
  - Drag a point to set frequency and gain, scroll to change Q, and double-click a point to switch
    its band off. Each band's values can also be dragged in the strip below the graph.
  - A live post-EQ spectrum analyzer and an output gain control.
  - It processes before the Audio FX inserts, and costs nothing while flat.
- **Library (Y):** lists the plug-ins for the selected track.
- **All Files browser (F):** drag audio files into the tracks area.
- **Importing audio:** drag files from the desktop file manager or the browser.
  - Dropping on the track headers creates new tracks at that point, starting at bar 1.
  - Dropping on an audio track's lane places the file there at the snapped position.
  - Dropping below the tracks creates a new track at that position.
  - Several files become consecutive regions on a lane, or one track each on the headers.
  - Accepts WAV, AIFF, FLAC, MP3, OGG Vorbis, M4A/AAC and CAF. Anything that isn't WAV at the
    project's sample rate is converted, with high-quality resampling, into `Audio Files/`.
- **Editor (E):** follows the selection: the Piano Roll for a software instrument track and the
  Audio Track Editor for an audio track (both described below).
- **MIDI files:** File › Import MIDI File… puts a Standard MIDI File (format 0 or 1) on the
  selected instrument track at the playhead's bar, and further parts of the file on new
  instrument tracks. You can also drop `.mid` files on an instrument lane, the track headers or
  the empty area below the tracks, or right-click an instrument lane › Import MIDI File Here….
  An empty project takes the file's tempo and time signature. Export a region (right-click ›
  Export as MIDI File…), a whole track (track header menu), or the selection (File › Export
  Selection as MIDI File…).
- **Native menu bar:** File, Edit, Track, Record, Navigate, View.

**Audio Track Editor.** With an audio track selected, the editor (E) shows all of its regions on
the project timeline, drawn large, like Logic's Audio Track Editor, with the non-destructive
range tools of Logic's Audio File Editor.
- **View:** a bars/beats ruler (click or drag it to locate), the playhead (it follows Catch), the
  selected region highlighted, and a dB scale beside the waveform. The waveform is drawn with
  the region's gain applied; red marks at its top and bottom show where the audio reaches
  0 dBFS after that gain.
- **Zoom and scroll:** ⌘-scroll or the slider zooms in time, Z zooms to the selection (or the
  region, or all the track's regions) and back. ⌥-scroll, the waveform slider or ⌘↓ / ⌘↑ zoom
  the waveform's amplitude, to see quiet passages. The wheel scrolls along the timeline. The
  editor keeps its own zoom, separate from the arrange area's.
- **Selecting:** drag across the waveform to select a time range. It follows the Snap setting
  (Smart lets go below sixteenths, and ⌥ while dragging always does). Shift-click or Shift-drag
  extends the selection; double-click a region to select all of it; Esc deselects. A plain click
  selects the region under it.
- **Region editing:** drag the name bar to move a region, its lower edges to trim it, and the
  handles at the top of the waveform to set fades, as in the arrange. The Gain field in the
  local bar sets the region's gain (drag, scroll, or double-click for 0 dB).
- **Range actions** (Functions menu, right-click inside the selection, or keys) act on every
  region under the selection, each as one undo step:
  - Split at Selection splits at both ends. Trim Region to Selection keeps only the selection.
  - Silence Selection splits and mutes the middle part, so it can be brought back by unmuting
    it. Delete Selection removes the middle part and leaves a gap.
  - Normalize Selection splits and sets the middle part's gain so its peak reaches 0 dBFS.
    Reverse Selection splits and reverses the middle part (into a new file).
  - Fade In / Fade Out to Selection fade over exactly the selected range, splitting the region
    there when the range doesn't start (or end) at its edge.
  - Copy and Cut put the selected audio on the clipboard as a region (Cut also deletes it); ⌘V
    pastes it at the playhead.
  - Play Selection plays from its start and stops at its end. With Cycle on, it sets the cycle to
    the selection and loops it. Set Locators by Selection only sets the cycle.
- **Transients:** the transient button (or View › Show Transients) marks the onsets found in the
  audio, at a sensitivity set in View › Transient Sensitivity. Split at Transients chops the
  selection (or the selected regions) at every marker in one step, for slicing drums. Onsets
  are found in the engine from the audio itself: a fast peak level jumping well above a slower
  average, moved back to the start of the attack.
- **Info:** the bar under the menus shows the region's name, start, length (in bars and time),
  gain, and its file's sample rate, channels and name, plus the selection's range.

| Key | Action | Key | Action |
|---|---|---|---|
| ⌘T | Split at selection (at the playhead without one) | ⇧⌘T | Split at transients |
| ⌫ | Delete selection | ⌥⌫ | Silence selection |
| ⌥T | Trim region to selection | ⌥N | Normalize selection |
| ⌥R | Reverse selection | ⌥I / ⌥O | Fade in / out to selection |
| ⌘C / ⌘X | Copy / cut selection | ⇧Space | Play selection |
| Z | Zoom to selection or region, and back | Esc | Deselect |
| ⌘← / ⌘→ | Zoom in time | ⌘↓ / ⌘↑ | Waveform zoom in / out |

These keys apply while the editor has focus (after clicking in it); elsewhere the usual ones do.

**Automation.** Volume, pan, mute and any plug-in parameter can be automated per track, as in
Logic.
- **Show Automation (A**, or either View menu) draws each track's automation over its dimmed
  regions. Region editing is off while it's shown.
  - The header gets a parameter picker (Volume, Pan, Mute, and the parameters of the track's
    instrument and Audio FX plug-ins; automated ones are marked ●) and the mode pop-up.
  - Click empty lane space to add a point (snapped in time) and drag a point to move it. Shift
    while dragging keeps it in time. Double-click or Alt-click deletes a point, a drag on empty
    space selects points with a rubber band, and ⌫ deletes the selection. Hovering shows the
    value. Right-click for Delete Automation.
  - Volume is drawn on the fader's scale. Lanes are straight lines between points, holding the
    first value before them and the last one after.
- **Modes** (in the header, and above the pan knob on every channel strip):
  - **Off** ignores the lanes and **Read** (the default, green) plays them. The fader, pan
    knob and plug-in editors follow while playing.
  - **Touch**, **Latch** and **Write** (orange/red) record what you do while playing. Touch
    writes while you hold a control and glides back to the existing curve when you let go.
    Latch writes from the first touch until Stop, holding the last value. Write overwrites
    volume, pan and every existing lane from where playback started; afterwards the track
    switches to Latch, as Logic does by default.
  - Moves in a plug-in's own editor are written too (through the plug-in's begin/perform/end
    edit calls).
  - Each pass is one undo step, *Write Automation*. Written passes are thinned to the points
    the curve needs.
- The engine evaluates lanes every block. Volume keeps the fader's smoothing, and plug-in
  parameters reach the plug-in through the VST3 parameter queue, ramped across each block.

**Piano Roll.** The Editor (E) for a software instrument track, modelled on Logic's Piano Roll
with ideas from Ableton's MIDI editor, Cubase's Key Editor and FL Studio's piano roll.
- **View:** every MIDI region on the track in project time, with bars and beats. The selected
  region is highlighted and the others are dimmed, but still editable. The playhead and cycle
  are shown; click the ruler to locate, or drag in its top strip to set the cycle. ⌘-scroll or
  the slider zooms horizontally, Alt-scroll or the row slider vertically; Z zooms to fit the
  region. Catch follows the playhead.
- **Keyboard:** C3 is middle C (MIDI 60), as in Logic. Click a key to hear it (further right is
  louder) and to select every note of that pitch; Shift adds.
- **View menu:** colour notes by velocity (Logic's violet-to-red scale) or by region, highlight
  the rows of the project key's scale, and turn MIDI Out (hear notes as you edit) on or off.
- **Tools** (the tool pop-up, or T then the tool's letter): Pointer (T), Pencil (P), Eraser (E),
  Velocity (V), Mute (M), Scissors (I), Glue (G) and Line (L, for the lanes). ⌘/Ctrl-click with
  the Pointer is the Pencil.
- **Editing with the Pointer:** click a note to select it, Shift-click to add, drag on empty
  space to rubber-band select, click empty space to deselect. Drag notes to move them (snapped
  to the grid; hold ⌘/Ctrl to move freely), Alt-drag to copy. Drag a note's right edge to change
  its length and its left edge to trim its start (Alt or ⌘/Ctrl for no snap). Double-click empty
  space to create a note of the grid length. The help tag shows pitch, position or length.
- **Pencil:** click to create a note, drag right to set its length. Notes drawn or moved past a
  region's edges grow it to the next bar line; on empty space far from any region a new region
  is made.
- **Grid:** 1/1 to 1/64 and triplets; it sets the snap and the Pencil's note length.
- **Time Quantize** (the pop-up in the local bar): value, strength, swing and whether note ends
  are quantized too. Q quantizes the selected notes (or all of them when none are selected), as
  do Functions › Quantize and Quantize to.
- **Functions:** Scale Quantize to the project key; Transpose (octave, semitone or any amount);
  Velocity (set, scale by a percentage); Humanize (light, medium, strong or custom timing and
  velocity amounts); Note Length (Legato, Remove Overlaps, set to the grid or a fixed value);
  Mute / Unmute; Delete Duplicates; Reverse; Split at Playhead; Glue Same Pitches.
- **Edit menu:** cut, copy, paste at the playhead, duplicate, delete, select all, invert,
  same pitch, highest or lowest notes of each chord, muted notes, notes inside the locators, and
  Export Region as MIDI File.
- **Velocity and controller lane** (drag the bar above it to resize): pick Velocity, Modulation
  (CC 1), Expression (CC 11), Sustain (CC 64), Pitch Bend, Aftertouch, Volume (CC 7), Pan (CC 10)
  or any other controller number; lanes holding data are marked with a dot.
  - Velocity: drag a stalk to set that note's velocity (the rest of the selection moves with it).
    Drag on empty space, or use the Line tool, to ramp the selection's velocities along a line;
    the Pencil draws freehand. The Vel slider in the local bar sets the selection's velocity.
  - Controllers: double-click to add a point, drag points to move them (Shift-click to add to
    the selection, rubber-band on empty space), the Pencil draws freehand, Shift-Pencil or the
    Line tool draws a ramp, and the Eraser or Alt-click removes points. ⌫ deletes the selected
    points.
- **Playback:** muted notes are skipped. Controllers play with the notes, and the values in
  effect are sent when playback starts or jumps, so a region played from the middle sounds as
  it does from its start. Stopping releases the sustain pedal and re-centres pitch bend. Split,
  join and trim keep controllers consistent: the later part of a split starts with the values in
  effect at the cut. The built-in synth responds to pitch bend, sustain, modulation (vibrato) and
  expression; VST3 instruments get controllers through their MIDI mapping.
- Each edit is one undo step (arrow-key nudges in a row merge into one).

| Key (Piano Roll focused) | Action | Key | Action |
|---|---|---|---|
| ← / → | Nudge by the grid (⇧: by a bar) | ↑ / ↓ | Transpose a semitone (⇧: an octave) |
| ⌥← / ⌥→ | Select previous / next note | ⌫ | Delete notes (or selected controller points) |
| Q | Quantize | T | Tool menu (then T P E V M I G L) |
| ⌘A | Select all | Esc | Deselect all |
| ⌘C / ⌘X / ⌘V | Copy / cut / paste at playhead | ⌘D | Duplicate after the selection |
| ⌃M | Mute / unmute notes | Z | Zoom to fit the region |
| ⌘← / ⌘→ | Zoom out / in | ⌘↑ / ⌘↓ | Shorter / taller rows |

**Themes.** Settings › Appearance (or View › Theme) switches between Studio (the default),
Flat, Midnight, Light and High Contrast. The choice applies immediately and is remembered.
Themes live in `src/themes.ts` as CSS variables plus colours for the canvas views.

**Interface size.** Settings › Appearance offers 90%–200%, also available from View › Larger
/ Smaller Interface (⌘= / ⌘−, and ⌘0 for actual size). The whole window scales and the canvas
views render at the matching resolution, so they stay sharp.

**Waveforms.** Settings › Appearance › Waveforms picks how audio is drawn:
- **Smooth** (the default) traces the true shape above and below the centre line. Zoomed in, it
  shows the individual cycles of the wave.
- **Mirrored** shows loudness mirrored around the centre line.
- **Rectified** shows loudness rising from the bottom of the region.
- **Blocks** is the classic bar-per-bucket overview.

The overview holds a signed minimum and maximum for every 32 frames, sent to the UI as binary.

**Project Chooser.** When Fermata starts without a project, a chooser opens. It also opens from
File › New… (⌘N) and File › Open Recent… (⇧⌘O).
- **New Project:** four templates (Empty, Record Audio, Software Instrument, Songwriter), plus name,
  location, tempo, time signature and key. The project is saved to its folder straight away, so
  recordings land in the right place.
- **Recent:** the last 20 projects you saved or opened, with track count, tempo and how long ago
  each was saved. Missing projects are flagged and can be removed from the list.
- **Open…:** browse for any `.fermata` folder.
- Untick **Show at launch** to start straight into an empty project instead.

**Undo.** ⌘Z / ⇧⌘Z undo and redo every edit: regions, tracks, names, colours, volume, pan, EQ,
plug-in slots, recordings and imports. The Edit menus show what will be undone. A run of drags on
one control counts as a single step. Mute, solo, arm and monitoring aren't undoable, as in Logic.
The history holds 100 steps by default (Settings › General) and is cleared when another project is
opened.

**Bounce and export.** Everything renders offline: the real graph runs faster than real time,
so plug-ins, stacks and channel strips sound exactly as they play. The audio device is silent
until the render finishes, as with Logic's offline bounce. Plug-ins see the transport playing
from the start of the range, and every instrument gets its note-offs at the end.
- **File › Bounce Project or Section…** (⌘B) writes the full mix through Stereo Out.
  - The range is the whole project (bar 1 to the end of the last region), the cycle range (the
    default when Cycle is on), or a span of bars.
  - The audio tail (2 s by default) lets reverbs and releases ring out. Silence at its end is
    trimmed.
  - Files are WAV in 16-bit (with TPDF dither), 24-bit or 32-bit float, at the project's sample
    rate or 44.1, 48, 88.2 or 96 kHz (converted with the high-quality resampler).
  - Normalize puts the peak at −0.1 dBFS.
  - The file name defaults to the project's, next to the project folder. The settings are
    remembered.
- **File › Export All Tracks as Audio Files…** (⇧⌘E) writes one file per track into a folder,
  named `<number> <track name>.wav` after the track's place in the list.
  - Each audio and instrument track plays alone through its own channel strip, fader and pan
    included and Stereo Out left out, so the files add up to the mix.
  - A track's own mute and the project's solos are ignored.
  - A summing stack is also exported as a whole, through its own strip, alongside its subtracks.
    Folder stacks have no channel strip, so they get no file.
  - Tracks without regions are skipped.
  - Every file covers the same range plus the full tail, so they line up when imported elsewhere.
- **Track › Bounce in Place…** (⌃B on macOS, Ctrl+Alt+B elsewhere, since Ctrl+B is Bounce), also
  in the track and region menus.
  - Renders the selected tracks through their channel strips, over the span of their regions, into
    the project's `Audio Files/`.
  - From the region menu, it renders just the selected regions, over their span.
  - Each bounce goes onto a new stereo audio track below the source, with the same colour and in
    the same stack, named "<name> (bounced)", as one region at the start of the range.
  - Options:
    - **Audio FX** (Channel EQ and plug-ins).
    - **Volume/Pan**. Left off, the new track takes over the source's fader and pan instead.
    - **Audio Tail**, up to 2 s.
    - **Source**: Leave, Mute (the track, or the regions when bouncing regions) or Delete.
  - Instrument tracks bounce their MIDI to audio. A summing stack bounces its output, its
    subtracks included. A folder stack bounces the mix of its subtracks, with its fader printed.
  - When regions were the last thing selected, the menu command bounces those regions.
  - It is all one undo step, plug-ins included when the source is deleted.
- **Export as Audio File…** in the region menu renders one region alone, muted or not, through its
  track's Channel EQ and plug-ins. It is pre-fader, so the track's level doesn't colour the file.
  It uses the same format options as Bounce.

**Plug-in delay compensation.** Plug-ins that look ahead (limiters, linear-phase EQs, some
amp models) put their output late. Fermata compensates every path, as Logic's default "All"
mode does, so tracks, summing stacks, auxes and sends all line up at Stereo Out.
- Each plug-in's latency is read from the plug-in (`getLatencySamples`) when it loads, is
  bypassed or removed, on undo and project load, and whenever it reports a change.
- A bus waits for the latest of its inputs, and every other path into it is delayed to match.
  Stereo Out waits for the slowest path; the whole mix plays that much behind the playhead,
  like extra output latency. Its own plug-ins delay everything equally.
- Sends tap the same delay line as the track's output, each at its own delay, so a send into an
  aux with a latent reverb leaves early and meets the dry signal again at Stereo Out. This
  holds pre- and post-fader.
- Bypassed plug-ins still count, as in Logic: a delay stands in for them, so switching a plug-in
  off doesn't shift the rest of the mix.
- Automation follows the audio: plug-in lanes are read as late as the strip's input, fader, pan
  and send lanes as late as its output. Plug-ins downstream also see the transport that late.
- The metronome is delayed by the same total, so it clicks with the music.
- Recordings (audio and MIDI) are moved earlier by the total too, since that is how late the
  performer heard the song. An armed audio track that is recording skips its own compensation
  delay, so it is monitored with only its plug-ins' latency.
- Bounces, exports, Bounce in Place and Freeze render the extra latency and drop it from the
  start, so files line up with the timeline.
- Settings › Audio › **Plug-in Delay Compensation**: All (default) or Off.
- **Low Latency Mode** (Settings › Audio, or Record › Low Latency Mode): plug-ins with more
  latency than the limit (5 ms by default) are switched off on record-enabled tracks and on
  audio tracks monitoring their input, and those tracks skip their compensation delay, so
  playing live feels immediate. Renders ignore it.
- The Audio FX label of a strip shows its plug-in latency; each slot's tooltip has its own, and
  the status line shows the total ("PDC 10.0 ms").

**Track Freeze.** The snowflake button on audio and instrument track headers (or Track › Freeze
Track, or the track menu) renders the track's instrument, Channel EQ and plug-ins into a file in
the project's `Freeze Files/` folder and plays that instead, with the plug-ins unloaded. That is
the point: a heavy instrument or effect chain stops using CPU.
- The render is pre-fader, so volume, pan, mute, sends and their automation stay live.
  Plug-in automation is baked in.
- The plug-ins' settings are kept in the project, and Unfreeze (the button again, or the menus)
  loads them back exactly, then deletes the freeze file.
- While frozen, the regions, plug-ins and Channel EQ can't be edited, and the track can't be
  record-enabled. Its regions are veiled, and edits are refused with a message (the session
  puts back anything that slips through).
- Freeze and Unfreeze are undoable; undoing an Unfreeze writes the freeze file back. Frozen
  tracks stay frozen when the project is saved and reopened. A duplicate of a frozen track is
  live.

**Projects.** New, Open, Save and Save As, as `.fermata` folders (`project.json` plus
`Audio Files/`). You're warned before unsaved changes are discarded.

## Settings

Fermata › Settings… (⌘,) has a tab per area, like Logic's Settings. Changes apply at once and
are kept in `~/.config/fermata/settings.json`; settings files from older versions load with
defaults for anything new.

- **General:** what happens at launch (Project Chooser, the most recent project, or an empty
  project); the folder for new projects (default `~/Music/Fermata`); **autosave** every 1–15
  minutes or off; the number of undo steps (10–1000, default 100); whether to ask before
  quitting or closing over unsaved changes.
- **Audio:** driver, devices, sample rate, buffer size, auto input monitoring, recording delay,
  plug-in delay compensation (All or Off) and Low Latency Mode with its limit, the measured
  latencies, the system input level check, and **Reset Audio Engine** to reopen the devices.
- **Recording:** bit depth of new recordings (24-bit integer or 32-bit float); the recording
  folder (default: the project's Audio Files); take names (`Vocals#03`, `Vocals` or
  `Song - Vocals#03`); count-in length (1–4 bars); pre-roll in seconds when Count-in is off;
  what happens when a recording overlaps existing regions: for audio, put the take **on top**
  (older audio stays underneath), **create a new track** below with the same settings and
  plug-ins, or **replace** (trim) what's underneath; for MIDI, a new region or **merge** into the
  region it overlaps.
- **Metronome:** the click is a project setting, as in Logic: sound (Classic, Click, Woodblock,
  Beep, all synthesized in the engine), volume, accent on the first beat, bar and beat pitch in
  semitones, subdivisions (eighths, triplets, sixteenths), click while playing, while recording,
  or only during the count-in. The count-in always clicks. **Use for New Projects** makes the
  current settings the default.
- **MIDI:** switch each MIDI input on or off; MIDI Thru (off: live MIDI is recorded but not
  heard); velocity as played, soft, hard or fixed; a live activity monitor of incoming messages.
- **Editing:** the snap mode new sessions start with; the Nudge value (bar, beat, 1/8, 1/16,
  1/32, 10 ms, 1 ms) for ⌥← / ⌥→; anti-click fades (ms) given to new recordings and imports.
- **Plug-ins** (Logic's Plug-in Manager): every scanned plug-in with manufacturer, type and
  version; untick to hide one from the Library and the slot menus (projects using it still
  load it); extra VST3 folders; Rescan; and the last scan's report, including each bundle that
  failed and why (crashed, timed out, no plug-ins).
- **Key Commands:** every window shortcut, searchable by name or key. Click + and press a key to
  add one, × to remove it, Default to restore it; taking a key from another command moves it.
  The menu bar's ⌘ shortcuts are listed but fixed.
- **Appearance:** interface size, waveform style and theme.

**Project Settings** (File › Project Settings…, ⌥P, or the LCD's pop-up menu) holds what is
saved with the project: tempo, time signature, key, sample rate (with a warning when existing
audio would play at the wrong speed), a bit-depth override for its recordings, the metronome
settings, and free-form notes. These edits can be undone.

**Autosave and crash recovery.** While a project has unsaved changes, Fermata writes
`project.autosave.json` into its folder every few minutes (the plug-in states are read on the main
thread, the file is written on a worker thread). Saving removes it, and so does choosing Don't
Save. At launch, Fermata looks in the recent projects and the projects folder for autosaves newer
than their `project.json` and offers to recover them; opening such a project asks the same.

## Key commands

| Key | Action | Key | Action |
|---|---|---|---|
| Space | Play / Stop | R | Record |
| Return | Go to beginning | , / . | Back / forward one bar |
| C | Cycle | K / ⇧K | Metronome / Count-in |
| X | Mixer | I | Inspector |
| Y | Library | E | Editor |
| F | Browsers | ` | Catch playhead |
| M / S | Mute / solo selected track | ⌃R | Record-enable selected track |
| ↑ / ↓ | Select previous / next track | ⌫ | Delete selected regions |
| ⌥⌘N | New tracks | ⌘T | Split at playhead |
| L | Loop selected regions | U | Set locators by regions |
| ⌃H | Hide selected tracks | H | Show hidden tracks |
| ⌘S / ⇧⌘S | Save / Save As | ⌘O / ⌘N | Open / New |
| ⌘← / ⌘→, ⌘-wheel | Zoom | ⇧⌘I | Import audio file |
| ⌘Z / ⇧⌘Z | Undo / Redo | ⌘K | Musical Typing |
| ⌘B | Bounce project or section | ⇧⌘E | Export all tracks as audio files |
| ⌃B (macOS), Ctrl+Alt+B | Bounce in place | | |
| A | Show / hide automation | | |
| ⌥← / ⌥→ | Nudge regions | ⌥P | Project Settings |
| ⌥K | Key Commands | ⌘, | Settings |
| ⌥' | Create marker | ⌥, / ⌥. | Previous / next marker |
| ⌥⌘K | Marker List | ⌃⌘P | Autopunch |
| ⌃⌘F | Pack take folder | | |

On Linux and Windows, ⌘ is Ctrl. The keys without ⌘ (and the ⌘-arrow zooms) can be changed in
Settings › Key Commands; they live in `src/keymap.ts`.

## Architecture

```
crates/vst3-host   VST3 hosting on the `vst3` bindings: module loading, scanning,
                   host COM objects, the RT-safe processing wrapper, X11 editor windows
                   and the IRunLoop bridge.
crates/engine      The audio engine.
  dsp.rs           Real-time graph. Lock-free command queue in, garbage queue out;
                   no allocation on the audio thread.
  audio_io.rs      cpal duplex I/O. The output stream is the clock; input arrives over
                   a ring buffer with drift control.
  session.rs       Non-RT controller: the project document, recording takes, plug-in
                   instances, save/load.
  session/routing.rs  Sends, auxes and track outputs. The processing order (every
                   sender before its receivers) and the loop checks are in model.rs.
  dsp/offline.rs   Offline rendering: runs the graph on the calling thread, whole or
                   with one track isolated.
  dsp/pdc.rs       Delay lines with a read tap per output and send, for plug-in
                   delay compensation.
  session/pdc.rs   Plug-in latency and the compensation solver.
  session/freeze.rs  Track Freeze: render, unload the plug-ins, refuse edits.
  session/bounce.rs  Bounce, Export All Tracks, Export Region, Bounce in Place, WAV
                   writing with dither.
  session/options.rs  Settings applied to the session: recording placement, autosave
                   and crash recovery, project options.
  takes.rs         Take folders: comp editing, the comp's playback pieces and
                   crossfades, and how cycle and punch recordings are cut into takes.
  session/takes.rs  Recording into take folders and Autopunch trimming; Flatten, Merge,
                   Pack, Unpack and the other take folder edits.
  session/markers.rs  The Marker track and the Autopunch range.
  prefs.rs         App preferences and the metronome/bit-depth types saved in projects.
  recorder.rs      Disk-writer thread for takes.
  midi_io.rs       MIDI inputs.
  midi.rs          MIDI region content helpers: split, trim and join with controller
                   chasing.
  smf.rs           Standard MIDI File reading and writing.
  plugin_db.rs     Out-of-process plug-in scanning and the cache.
src-tauri          Tauri app: commands, native menu, main-thread plug-in idle, GTK run
                   loop integration, 30 Hz meter/playhead ticks.
src                React UI. Canvas for the arrange area, ruler and meters; ticks
                   bypass React state.
  midiEdit.ts      The Piano Roll's editing functions (quantize, humanize, legato…),
                   pure and tested in tests/ (`npm test`).
  takes.ts         Take folders in the arrange area: take rows, comp drawing, swipes.
  markers.ts       The marker lane and marker commands.
  frame.ts         One requestAnimationFrame loop drives the playhead, meters and LCD
                   at display rate, interpolating between the engine's 30 Hz reports.
docs/logic-reference.md   The Logic Pro UI/feature research used for the design.
```

Synchronous Tauri commands run on the main thread, which is where VST3 controllers and
editors must live. The audio thread only sees prepared objects.

## Known limitations (planned next)

- Plug-in editor windows are Linux/X11 only. Windows (HWND) and macOS (NSView) embedding are
  next. On Linux the UI runs through XWayland by default, because GLX-based plug-in GUIs fail
  when GTK is native Wayland in the same process. Set `GDK_BACKEND=wayland` to override.
- No tempo map (one tempo per project).
- Plug-in delay compensation: the mix plays behind the playhead by the total latency rather
  than reading regions early, so live input on a monitored track that isn't recording is
  delayed with everything else (Low Latency Mode avoids that). Low Latency Mode only switches
  off plug-ins on the live track itself, not on the auxes or stacks it plays through, and a
  track that skips its compensation is early against the rest while it plays regions. Fader
  automation written live is stored at the playhead, not compensated. Delay changes while
  playing (bypass, a plug-in reporting new latency) can click.
- Track Freeze renders from the track's first region to its last plus up to 4 s of tail; a
  frozen track's plug-in automation lanes stay editable but do nothing until it is unfrozen.
- Imported AAC/M4A files may start about 40 ms late, because the encoder delay isn't fully
  trimmed. MP3 is trimmed exactly.
- Take folders have one comp (Logic keeps several, Comp A, B…). Recording without Cycle over a
  folder doesn't add a take to it. A folder's own fades shape only its first and last comp
  stretch.
- With Autopunch inside the cycle, a punch-out closer to the cycle end than the recording
  latency makes each take that much shorter (the latency tail would be past the loop point).
- Markers have no length editing yet (a marker reaches the next one); there is no Arrangement
  track.
- Automation: folder stacks and Stereo Out have no automation of their own, and lanes don't
  move with regions.
- The Piano Roll has no Step Input, Event List or Score yet, and controllers have no curve shapes.
- CLAP, AU and VST2 aren't supported.

## Roadmap

`LOGIC_FEATURES.md` lists every Logic Pro feature by stage and marks what Fermata already has.
Next up, in rough order:

1. Tempo changes and time-signature changes (a tempo map with Tempo and Signature global tracks).
2. Plug-in editor windows on Windows (HWND) and macOS (NSView).
3. Crossfades, fade curve shapes and drag modes; a Marquee tool and a Tool menu in the tracks
   area; region copy-drag and rubber-band selection.
4. Plug-in presets and channel strip settings; side-chain inputs; multi-output instruments.
5. Built-in effects (compressor, limiter, gate, reverb, delay, tuner).
6. Event List and Step Input; non-destructive region quantize/transpose; MIDI learn.
7. CLAP hosting.

Design notes for the Logic-like layout are in `docs/logic-reference.md`.
