# Logic Pro feature scope for Fermata

A list of the features in Logic Pro, grouped into the stages in which Fermata could build them,
with each one checked against what Fermata has today. It is a planning document: it shows how
far Fermata has come and what is left. It is not a promise to clone everything. Some items are
macOS-specific and are marked as such.

- **Date:** 2026-10-28
- **Source:** *Logic Pro User Guide for Mac*, covering **Logic Pro 12.3** (its "What's new" pages
  cover 12.0, 12.1 and 12.3). Page references such as "(p. 512)" point to that guide. The
  bundled instruments and effects are documented in Apple's separate *Logic Pro Instruments* and
  *Logic Pro Effects* guides, so Stage 5 summarises them by category.
- **Fermata status** was checked against the source code of the current merged tree (`src/`,
  `crates/engine`, `crates/vst3-host`, `src-tauri`), not only the README. Items marked "in
  progress" are being built on branches that haven't been merged yet.

## Legend

| Mark | Meaning |
|---|---|
| `[x]` | Implemented |
| `[~]` | Partial: Fermata has a simpler version (the note says what's missing) |
| `[ ]` | Not started |
| `[ ] 🚧` | In progress: being built now, not merged yet |

## Summary

Counted from the checkboxes below. Stage 5 summarises the bundled instruments and effects by
category, one line each, so its count understates how much content that is.

| Stage | Done | Partial | In progress | Not started | Total |
|---|---:|---:|---:|---:|---:|
| 1. Core | 180 | 33 | 0 | 50 | 263 |
| 2. Professional editing and mixing | 21 | 18 | 0 | 190 | 229 |
| 3. Advanced production | 0 | 0 | 0 | 111 | 111 |
| 4. Studio integration | 0 | 0 | 0 | 61 | 61 |
| 5. Content and AI | 0 | 6 | 0 | 34 | 40 |
| 6. Platform | 5 | 3 | 0 | 18 | 26 |
| **All** | **206** | **60** | **0** | **464** | **730** |

---

## Stage 1: Core

The foundation: projects, transport, tracks, regions, basic recording, the mixer, plug-in
hosting, automation and undo. This stage is mostly done.

### Logic Pro basics: the interface (p. 15–41)

- [x] Single main window with Tracks area, control bar, inspector, Library, editors, Mixer and browsers as panes (p. 15)
- [x] Show or hide panes with Logic's keys: Inspector (I), Library (Y), Mixer (X), Editors (E), Browsers (F) (p. 16–41)
- [~] Control bar (p. 87): view buttons, transport, LCD, Cycle, Count-in, Metronome, master volume, CPU. *Fixed layout; no Customize Control Bar.*
- [x] LCD showing position, tempo, time signature and key; drag or double-click to edit (p. 93)
- [~] LCD display modes (p. 93). *Beats and Time only; no Custom mode or large LCD.*
- [x] MIDI input activity shown live (Settings › MIDI monitor) (p. 93)
- [x] CPU/load meter (p. 92)
- [x] Inspector with Region and Track sections and two channel strips (p. 19–21)
- [~] Right inspector strip follows the left strip's output or aux (p. 21). *Always shows Stereo Out.*
- [x] Mixer pane (p. 22)
- [x] Editors pane: Piano Roll for instrument tracks, Audio Track Editor for audio tracks (p. 24)
- [x] All Files Browser (p. 41)
- [~] Library pane (p. 17). *Lists plug-ins to load, with search; no patches (see Stage 2).*
- [x] Musical Typing window with on-screen keyboard (⌘K) (p. 201)
- [~] Project notes (p. 30). *Free-form notes in Project Settings; no Note Pad pane.*
- [ ] Track notes (p. 31)
- [ ] Toolbar (p. 298)
- [ ] Giant beats or time display (p. 91)
- [ ] Region inspector as a floating window (p. 21)

### Windows, zoom and catch (p. 45–54)

- [x] Horizontal zoom (⌘←/⌘→, ⌘-scroll) and track-height zoom (⌘↑/⌘↓, Alt-scroll) (p. 47, 132)
- [x] Resize one track or all tracks, reset height, heights saved per project (p. 132)
- [~] Zoom to fit selection or all content (Z) (p. 47). *In the Piano Roll and Audio Track Editor; not in the Tracks area.*
- [x] Scale the whole interface (90–200%) (p. 48)
- [x] Catch playhead (`) (p. 49)
- [~] Open, close and focus windows (p. 45). *One main window with panes; the Mixer, Editors, effect panels and Channel EQ can detach into windows of their own.*

### Undo and redo (p. 60–62)

- [x] Undo and redo every edit: regions, tracks, names, colours, volume, pan, EQ, plug-in slots, recordings, imports (p. 60)
- [x] Edit menu shows what will be undone
- [x] A run of drags on one control is one undo step
- [x] Mute, solo, arm and monitoring are excluded from undo, as in Logic
- [x] Number of undo steps is configurable (p. 61)
- [~] Mixer and plug-in edits in the undo history (p. 61, 619). *Built-in effect and Channel EQ changes are undoable; parameter changes made in a VST3's own editor aren't.*

### Connect external devices (p. 68–77)

- [x] Choose audio input and output devices, which can be different devices (p. 72, 1041)
- [x] Sample rate and I/O buffer size (p. 1041)
- [x] Live input and output latency measurement (p. 1046)
- [x] Reset the audio engine (reopen devices) (p. 1041)
- [x] Connect USB/MIDI keyboards; every input routed to the selected instrument track (p. 73)
- [x] Turn individual MIDI inputs on or off (p. 1061)
- [~] Receive MIDI from other apps (p. 76). *Through system MIDI ports; Fermata doesn't create a virtual port of its own.*
- [ ] Local Off: stop a keyboard's own sound generation (p. 75)

### Work with projects (p. 78–115)

- [~] Project Chooser (p. 79). *New, Recent and Open, with four built-in templates; no user templates, demo projects or tutorials.*
- [x] New project with name, location, tempo, time signature and key (p. 79)
- [x] Create an empty project (p. 81)
- [x] Open a project; Open Recent with project details and missing-file flags (p. 81–82)
- [x] Save and Save As (p. 84)
- [x] Warn before discarding unsaved changes
- [x] Autosave and crash recovery (p. 1034)
- [x] Project sample rate, with a warning when existing audio would play at the wrong speed (p. 102)
- [x] Project tempo, time signature and key (p. 99–101)
- [x] Master volume slider (p. 91, 105)
- [ ] Open several projects at once and switch between them (p. 82)
- [ ] Save a copy including all assets (p. 84)
- [ ] Revert to the last saved version (p. 85)
- [ ] Delete a project from the chooser (p. 85)
- [ ] Project start and end points (p. 104)
- [ ] Close Project without quitting (p. 113)
- [ ] Project Information window (p. 114)

### Play and navigate projects (p. 85–98)

- [x] Play/Stop (Space), Record (R), Go to Beginning (Return), back and forward one bar (, .) (p. 85–88)
- [x] Set the playhead by clicking the ruler (p. 86)
- [x] Set the playhead in the LCD (p. 86)
- [x] Cycle mode (C); create, move and resize the cycle area in the ruler (p. 94–96)
- [x] Set locators by regions (U) (p. 95)
- [x] Musical time and clock time display (p. 92)
- [~] Chase events (p. 97). *Controllers, pitch bend and sustain are chased; notes that started earlier aren't.*
- [~] Stop hung notes (p. 94). *Stopping releases the sustain pedal and re-centres pitch bend; no explicit panic command.*
- [ ] Pause, fast forward and rewind, Play from Left Window Edge, Play from Selection (p. 88)
- [ ] Transport shortcut menus on Play and Stop (p. 89)
- [ ] Go to Position dialog (/)
- [ ] Set locators numerically in the LCD (p. 95)
- [ ] Skip cycle; move, double or halve the cycle by its length (key commands, p. 1121)
- [ ] Forward and rewind by transient (key commands, p. 1121)

### Work with tracks (p. 116–149)

- [x] New Tracks dialog: audio and software instrument tracks (p. 117–118)
- [x] Create tracks by dropping audio or MIDI files on the headers or below the tracks (p. 121–123)
- [x] New Track with Same Settings (p. 125)
- [x] Move selected regions to a new track (p. 125)
- [x] Duplicate a track with its plug-ins and content (p. 128–129)
- [x] Select tracks: click, ⌘-click, Shift-click range (p. 127)
- [x] Select previous or next track (↑/↓) (p. 128)
- [x] Rename tracks (p. 129)
- [x] Track colours (p. 131)
- [x] Reorder tracks by dragging (p. 131)
- [x] Hide tracks, Show Hidden Tracks (H), Unhide All (p. 146–148)
- [x] Delete tracks; Delete Unused Tracks (p. 148)
- [~] Change header controls on several tracks (p. 149). *Mute, solo and arm apply to the selection; other controls only through groups.*
- [ ] New track for overlapped regions (p. 125)
- [ ] Several tracks sharing one channel strip (New Track with Same Channel Strip / Next Channel) (p. 125–126)
- [ ] Search for and select a track (p. 128)
- [ ] Rename several tracks at once (p. 129)
- [ ] Track icons, including custom icons (p. 130)
- [ ] Sort tracks (p. 131)
- [ ] Automatically zoom the focused track (p. 132)
- [ ] Hide tracks through groups (p. 147)

### Track header controls (p. 135–144)

- [x] Mute, Solo, Record Enable and Input Monitoring buttons (p. 137–141)
- [x] Volume slider with level meter (p. 139)
- [ ] Pan knob in the header (p. 139)
- [ ] Configure Track Header: choose which controls show (p. 136)
- [ ] Resize the header column (p. 136)

### Record audio (p. 191–199)

- [x] Record on armed audio tracks (p. 192)
- [x] Record to several audio tracks at once (p. 195)
- [x] Mono or stereo input per track (p. 554)
- [x] Quick Punch-In: start recording while playing (p. 197)
- [x] Input monitoring (I) and automatic input monitoring setting (p. 141, 197)
- [x] Recording latency compensation plus a manual Recording Delay (p. 1041)
- [x] Takes written as 24-bit or 32-bit float WAV in the project's Audio Files folder (p. 1054, 1082)
- [x] Regions draw live while recording
- [x] Take naming options
- [~] Overlapping recordings (p. 1057). *On top, new track or replace; take folders come only from cycle recording, not as an overlap option.*
- [x] A newer region hides the older audio it overlaps during playback

### Record software instruments (p. 200–215)

- [x] Record MIDI on an armed instrument track (p. 202)
- [x] Notes, controllers, pitch bend and aftertouch are recorded and played back
- [x] Play with Musical Typing or the on-screen keyboard (p. 201)
- [x] Overdub: merge into an overlapped region (p. 204)
- [x] Record audio and a software instrument at the same time (p. 209)
- [x] MIDI Thru on or off; velocity scaling (as played, soft, hard, fixed) (p. 1057, 1087)
- [x] Built-in Fermata Synth, so instrument tracks make sound without a plug-in

### Metronome (p. 216–218)

- [x] Metronome on or off (K), Count-in (⇧K) (p. 217)
- [x] Count-in length in bars, pre-roll when Count-in is off (p. 218)
- [x] Click sound, volume, accent, bar and beat pitch, subdivisions (p. 1113)
- [x] Click while playing, while recording, or only during the count-in (p. 217)
- [x] Metronome settings saved per project with "Use for New Projects"
- [ ] Route the click to its own output, or send it as MIDI (p. 1113–1115)

### Arranging in the Tracks area (p. 288–332)

- [x] Ruler with bars and beats (p. 289)
- [x] Grid lines (p. 291)
- [~] Snap to grid (p. 290). *Smart, Bar, Beat and Off; no Division, Ticks, Frames or Samples, and no relative snap.*
- [x] Horizontal and vertical scrolling and zoom (p. 297)
- [x] Select regions (p. 299). *Click, Shift-click and rubber-band selection.*
- [x] Select all regions on a track, and all following (p. 300)
- [x] Cut, copy and paste; paste at the playhead keeps the spacing of several regions (p. 303)
- [x] Duplicate (⌘D) and Repeat Regions 2–16 times (p. 304, 313)
- [x] Move regions, including to other tracks (p. 305)
- [x] Nudge regions by the nudge value (⌥←/⌥→) (p. 305–306)
- [x] Loop regions (L) and the loop pointer at the top-right corner (p. 311–312)
- [~] Loop continuously (p. 312). *Loops to the next region, or four times the length when nothing follows.*
- [x] Convert Loops to Real Copies (p. 313)
- [x] Trim region start and end (p. 314)
- [x] Mute regions (⌃M) (p. 316)
- [x] Reverse audio regions (into a new file, so the original stays intact) (p. 318)
- [x] Split at the playhead (⌘T) (p. 319)
- [x] Join regions (⌘J): MIDI notes merge, audio is mixed into a new file (p. 322)
- [x] Create an empty MIDI region (p. 325)
- [x] Audio region gain and Normalize (p. 325–326)
- [x] Region colours (p. 328)
- [x] Rename regions (p. 331)
- [x] Delete regions (p. 332)
- [~] Pointer tool (p. 58). *Moves, trims, loops and sets fades; no Tool menu in the Tracks area (Stage 2).*

### Fades (p. 348–353)

- [x] Fade-in and fade-out from the handles at the top corners, presets or the inspector (p. 348–349)
- [x] Edit fade length (p. 350)

### Import, media and MIDI files (p. 268–287)

- [x] Import audio by dragging from the file manager or browser, or with Import Audio File (⇧⌘I) (p. 268–269)
- [x] WAV, AIFF, FLAC, MP3, OGG Vorbis, M4A/AAC and CAF (p. 282)
- [x] High-quality sample-rate conversion on import (p. 103)
- [x] Import Standard MIDI Files (format 0 and 1) at the playhead or drop position (p. 285)
- [x] An empty project takes a MIDI file's tempo and signature (p. 285)
- [x] Export a region, track or selection as a MIDI file (p. 187, 286)
- [~] All Files Browser (p. 268–273). *Browses folders; no search conditions or bookmarks.*
- [ ] Open a MIDI file as a new project (p. 285)

### Piano Roll Editor (p. 378–402)

- [x] Shows every MIDI region on the track in project time; the others dimmed but editable (p. 398–399)
- [x] Add, select, move, copy (⌥-drag) and resize notes (p. 380–389)
- [x] Snap grid 1/1 to 1/64 with triplets; override with a modifier (p. 383–384)
- [x] Transpose with the arrow keys; nudge by the grid or a bar (p. 385)
- [x] Velocity editing: drag stalks, ramp with the Line tool, set a value (p. 390–392)
- [x] Time Quantize with strength, swing and note ends (p. 392)
- [x] Scale Quantize to the project key (p. 393)
- [x] Legato, Remove Overlaps, fixed and grid note lengths (p. 389–390)
- [x] Mute notes (p. 395)
- [x] Delete duplicates (p. 397)
- [x] Select same pitch, highest or lowest notes of chords, muted notes, notes inside the locators; invert selection (p. 382, 399)
- [x] Velocity and controller lane: velocity, mod wheel, expression, sustain, pitch bend, aftertouch, any CC (p. 400–401)
- [x] Colour notes by velocity or by region (p. 396)
- [x] Highlight the rows of the project key's scale
- [x] MIDI Out: hear notes while editing (p. 1121)
- [x] Humanize timing and velocity
- [x] Help tag showing pitch, position and length
- [~] Piano Roll tools (p. 54–60). *Pointer, Pencil, Eraser, Velocity, Mute, Scissors, Glue, Line; no Brush, Quantize, Solo or Zoom tools.*
- [~] Controller editing (p. 400). *Points and ramps; no curve shapes.*
- [~] Delete events (p. 397). *No Delete Outside Region or Delete Unselected Within Selection.*
- [ ] Brush tool and brush patterns (p. 381)
- [ ] Add notes constrained to a scale (p. 381)
- [ ] Sustain pedal events to note lengths (p. 390)
- [ ] Time handles: stretch selected notes (p. 398)
- [ ] Note labels on notes (p. 396)
- [ ] Drum names and Collapse Mode views (p. 379)
- [~] Piano Roll as a separate window, with Link mode (p. 379–380). *The Editor detaches and follows the selection; no Link modes.*
- [ ] Lock event positions (p. 395)
- [ ] Assign notes to MIDI channels by pitch (p. 399)
- [ ] Track and region automation shown in the Piano Roll (p. 401)

### Audio Track Editor (p. 372–377)

- [x] Shows all of a track's regions large on the project timeline (p. 372)
- [x] Horizontal zoom and waveform amplitude zoom (p. 373)
- [x] Select, cut, copy, paste, move and trim regions (p. 373–375)
- [x] Time-range selection inside regions (p. 374)
- [x] Split at the selection or playhead (p. 375)
- [x] Fades and gain in the editor
- [x] 0 dBFS markers and dB scale
- [ ] Snap edits to zero crossings (p. 376)
- [ ] Show automation in the Audio Track Editor (p. 377)

### Mixer (p. 539–620)

- [x] A channel strip for every track plus Stereo Out (p. 539)
- [~] Channel strip types (p. 541). *Audio, instrument, aux, summing stack and Stereo Out; no MIDI, output, master or VCA strips.*
- [x] Filter strips by type (Audio, Inst, Aux) (p. 542)
- [x] Volume fader with Logic's taper (p. 552)
- [x] Level meters with a 2 s peak hold (p. 551–553)
- [~] Peak level display and clipping (p. 551). *No clip indicators to reset.*
- [x] Input format mono or stereo (p. 554)
- [x] Pan, with a −3 dB compensated pan law (p. 555)
- [x] Mute and solo (p. 557–558)
- [~] Reorder strips (p. 559). *Follows the track order.*
- [x] Audio FX insert slots: add, replace, remove and bypass plug-ins (p. 560–563)
- [x] Instrument slot (p. 561)
- [~] Search for plug-ins (p. 563). *The Library has search; slot menus are grouped by vendor and category, without search.*
- [x] Channel EQ on every strip: 8 bands, 12/24/48 dB/oct cuts, analyzer, output gain, thumbnail on the strip (p. 566)
- [~] Plug-in windows (p. 567). *Built-in effect panels everywhere; a VST3's own editor on Linux/X11 and macOS so far.*
- [x] Sends: eight per strip, pre or post fader, mute, level knob (p. 583–589)
- [x] Aux channel strips fed by buses; new aux from any send or output menu (p. 590)
- [x] Subgroups through auxes (p. 591)
- [x] Output routing to Stereo Out or an aux (p. 594–596)
- [x] Solo follows routing: soloing a track keeps its auxes audible, and the reverse
- [x] Feedback loops refused
- [x] Rename strips (p. 616)
- [ ] Mixer modes: Single, Tracks, All (p. 613)
- [ ] Select several strips, or strips by type (p. 543)
- [x] Move or copy plug-ins by dragging between slots (p. 562)

### Plug-in hosting (p. 559–580)

- [x] Third-party plug-ins: Audio Units in Logic, VST3 in Fermata (p. 559)
- [x] Scan in a separate process so a crashing plug-in can't take the app down; results cached
- [x] Instrument and effect plug-ins; separate component/controller plug-ins
- [x] Plug-in state saved in the project
- [x] Tempo and position passed to plug-ins
- [x] MIDI CC and pitch bend to instruments through the plug-in's MIDI mapping
- [~] Plug-in Manager (p. 578–580). *Hide plug-ins, add VST3 folders, rescan, view the scan report; no categories or custom names.*

### Automation (p. 621–644)

- [x] Track automation of volume, pan, mute, send levels and any plug-in parameter (p. 621)
- [x] Show Automation (A) (p. 622)
- [x] Parameter picker, with automated parameters marked (p. 623)
- [~] Automation lanes (p. 626–627). *One parameter shown per track at a time; no extra lanes.*
- [x] Modes: Off, Read, Touch, Latch, Write (p. 627–628)
- [x] Write switches to Latch afterwards, as in Logic (p. 1064)
- [x] Add, move and delete points; rubber-band select (p. 631–636)
- [x] Record live automation, including moves in a plug-in's own editor (p. 633)
- [x] Each pass is one undo step, thinned to the points the curve needs
- [x] Snap automation to the grid (p. 640)
- [x] Fader, pan knob and plug-in editors follow during playback

### Bounce and export (p. 175–187, 909–918)

- [x] Bounce Project or Section offline, faster than real time (⌘B) (p. 909)
- [x] Bounce range: whole project, cycle or a span of bars (p. 914)
- [x] Audio tail, with trailing silence trimmed
- [~] Bounce file formats (p. 914–918). *WAV and AIFF at 16, 24 or 32-bit float, FLAC at 16 or 24-bit; no CAF, MP3 or M4A.*
- [~] Dither (p. 914). *TPDF only; no POW-r.*
- [x] Bounce sample-rate conversion to 44.1–96 kHz
- [x] Normalize on bounce (p. 914)
- [x] Export All Tracks as Audio Files (⇧⌘E) (p. 184)
- [x] Export a region as an audio file (p. 185)
- [x] Bounce tracks in place, with Audio FX, volume/pan, tail and source options (p. 175–176)
- [x] Bounce regions in place (p. 178)
- [~] Bounce all tracks in place (p. 177). *Works on the selected tracks; no dedicated command.*
- [x] Export the selected track as an audio file (⌘E) (p. 183). *File › Export Selected Tracks as Audio Files…, one file per selected track.*
- [ ] Real-time bounce (p. 914)

### Settings (p. 1033–1076)

- [x] General: what happens at launch, project folder, autosave, undo steps, quit prompt (p. 1034)
- [x] Audio: driver, devices, rate, buffer, monitoring, recording delay, latency, input level check (p. 1041–1046)
- [x] Recording: bit depth, folder, take names, count-in, pre-roll, overlap behaviour (p. 1054–1057)
- [x] MIDI: inputs, MIDI Thru, velocity, activity monitor (p. 1057–1063)
- [x] Editing: default snap mode, nudge value, anti-click fades (p. 1035)
- [x] Plug-ins (Plug-in Manager) and Key Commands panes
- [x] Appearance: interface size, waveform style, theme (p. 1070–1076)
- [~] Project Settings (p. 1076–1120). *Tempo, signature, key, sample rate, bit depth, metronome and notes; most of Logic's project panes are missing.*
- [ ] Reset all settings (p. 1033)
- [ ] Notifications: reset "don't show again" warnings (p. 1039)

### Key commands (p. 1121–1158)

- [x] Key Commands window: search by name or key, add, remove, restore defaults (p. 1121–1124)
- [~] Customisable keys (p. 1121). *Window keys only; menu shortcuts are fixed.*
- [ ] Save, import and choose key command sets (p. 1124)

---

## Stage 2: Professional editing and mixing

What's being built now, plus the near-term gaps that working engineers expect: comping, markers,
the tempo map, full Tracks-area tools, the list editors, VCAs, presets and Smart Controls.

### Tools (p. 54–60)

- [ ] Tool menu (T) in the Tracks area (p. 54)
- [ ] Assign the left-click and ⌘-click tools; right-click behaviour (p. 55)
- [ ] Pencil tool: create regions (p. 58)
- [ ] Eraser tool (p. 58)
- [ ] Text tool: rename regions (p. 58)
- [ ] Scissors tool (p. 58)
- [ ] Join (Glue) tool (p. 58)
- [ ] Solo tool (p. 58)
- [ ] Zoom tool (p. 59)
- [ ] Fade tool (p. 59)
- [ ] Automation Select and Automation Curve tools (p. 59)
- [ ] Marquee tool (p. 59)
- [ ] Gain tool (p. 325)
- [ ] Slip and Rotate tools (p. 59, 1159)

### Arranging: advanced editing (p. 288–332)

- [ ] Marquee selection; marquee ruler (p. 301–302)
- [ ] Split at the marquee or locators (p. 320)
- [x] Rubber-band region selection in the Tracks area (p. 299)
- [ ] Select by criteria: equal, similar, muted, same colour, overlapped, inside locators (p. 300)
- [x] Drag modes (p. 295): Overlap, No Overlap, X-Fade, Shuffle R and Shuffle L.
- [ ] Alignment guides (p. 290)
- [ ] Full snap modes and relative snap (p. 290–292)
- [ ] Horizontal auto zoom (p. 292)
- [ ] Scrubbing (p. 296)
- [~] Waveform vertical zoom (p. 297). *In the Audio Track Editor only.*
- [ ] Paste at the original position; Paste Replace (p. 303)
- [ ] Move to playhead (pickup clock) (p. 305)
- [ ] Move to recorded position (p. 305)
- [ ] Slip and rotate by the nudge value (p. 306)
- [ ] Nudge value in ticks, frames and samples (p. 306)
- [ ] Insert silence, cut, insert or repeat a section between the locators across all tracks (p. 307–310)
- [ ] Remove gaps between regions (p. 308)
- [ ] Region anchor / shift playback position (p. 311)
- [ ] Convert repetitions to loops (p. 313, 328)
- [ ] Resize adjacent regions together (junction) (p. 314)
- [ ] Resize to the same length or to the playhead (p. 314)
- [ ] Remove overlaps, trim to the next region, fill within locators (p. 315)
- [ ] Solo regions; lock solo status (p. 316–317)
- [ ] Time-stretch MIDI regions (p. 317)
- [ ] Split into equal portions (p. 320)
- [ ] Demix MIDI by channel or note pitch (p. 321–322)
- [ ] Join per track (p. 323)
- [~] Mixdown options for joined audio, with crossfades (p. 324). *Join mixes through the playback path, so crossfades are kept; no options.*
- [ ] MIDI aliases (p. 326–328)
- [ ] Name regions by track, tracks by region, colour tracks by region and the reverse (key commands)
- [ ] Delete and shuffle the following regions (p. 332)
- [ ] Crop regions outside the locators or marquee (key commands)
- [ ] Folders: pack MIDI regions into a folder region, unpack (p. 358–360)
- [ ] Remove Silence (strip silence) (p. 355–357)

### Fades and crossfades (p. 348–353)

- [x] Fade curve shape (p. 351)
- [x] Fade types: crossfade, equal-power crossfade, speed-up and slow-down fades (p. 351–352)
- [~] Crossfades between adjacent regions (p. 352). *Not for take folders, looped or muted regions.*
- [~] Automatic crossfades on edits (p. 352). *The X-Fade drag mode; no Crossfade Time or Curve settings.*
- [ ] Apply the last fade to a marquee selection (p. 350)

### Region parameters (p. 363–371)

- [~] Region inspector (p. 363). *Mute, position, length, loop, playback parameters, gain and fades; no default region settings.*
- [x] Non-destructive MIDI quantize in the inspector, with Q-Swing (p. 366, 369). *Straight and triplet values; no swing presets (8F Swing A–F).*
- [ ] Advanced quantize: Q-Strength, Q-Range, Q-Flam, Q-Velocity, Q-Length (p. 371)
- [x] MIDI region transpose, velocity, dynamics and gate time (p. 366)
- [x] Region delay in ticks or ms (p. 364, 366). *For audio regions too.*
- [ ] Audio region transpose and fine tune (p. 364)
- [x] Apply region parameters permanently (p. 369). *One command, Apply Region Parameters, for all of them.*
- [ ] Dequantize (key commands)
- [ ] Default region settings (p. 363)

### Record: advanced (p. 191–247)

- [x] Autopunch mode with punch-in and punch-out locators (p. 196–198)
- [ ] Set punch locators from the playhead (key commands)
- [~] Replace mode (p. 199). *Audio "replace" is a setting; there's no Replace button, and MIDI can't replace.*
- [x] Cycle recording into take folders (p. 193)
- [ ] Record several audio takes to new tracks (p. 195)
- [ ] Record to several software instrument tracks at once (p. 207)
- [ ] Several MIDI devices to several instrument tracks (p. 208)
- [ ] Record MIDI over an existing region (Replace) (p. 206)
- [ ] MIDI cycle recording into take folders (p. 203)
- [ ] Spot erase notes while recording (p. 204)
- [ ] Note Repeat (p. 205)
- [ ] Step input: musical typing, Step Input keyboard, MIDI keyboard, rests (p. 210–211)
- [ ] Discard recording and return to the last play position (⌘.)
- [ ] Colour takes automatically; mark a take while recording (p. 218–220)
- [ ] Flashback Capture for MIDI and audio (p. 220–222)
- [x] Low Latency Monitoring mode (p. 575)
- [ ] Delete a recording together with its audio file (p. 199)

### Take folders and comping (p. 222–247)

- [x] Take folders from cycle recording (p. 222)
- [x] Quick Swipe Comping (p. 222)
- [x] Open a take folder and preview or choose takes (p. 223–224)
- [~] Create, save, rename and delete comps (p. 224–233). *One comp per folder; takes can be renamed and deleted.*
- [ ] Edit comp sections: move, alternate between lanes, remove a take (p. 227–229)
- [ ] Edit take regions: trim, cut, slice at comp borders, move between lanes (p. 234–237)
- [ ] Export the active take to a new track (p. 238)
- [x] Flatten, and Flatten and Merge (p. 241)
- [~] Unpack to existing tracks, new tracks or track alternatives (p. 243–246). *Unpack to New Tracks only.*
- [x] Pack regions into a take folder (p. 247)

### Tracks: professional (p. 141–148, 161–167)

- [x] Folder stacks: collapsible main track whose fader, mute and solo apply to its subtracks (p. 161–162)
- [x] Summing stacks: subtracks play through the stack's own channel strip (p. 161–163)
- [x] Create, nest and flatten stacks; add and remove subtracks; show or hide subtracks (p. 164–166)
- [ ] Convert a folder stack to a summing stack (p. 166)
- [ ] Track alternatives: add, duplicate, switch, rename, delete inactive, hear an inactive one (p. 141–143)
- [ ] Turn a track off, making its plug-ins inactive (p. 144)
- [ ] Protect tracks (p. 144)
- [x] Freeze tracks (p. 145)

### Global tracks, markers, tempo and signatures (p. 732–801)

- [x] Global Tracks area; show or hide it and its tracks (p. 733–735). *Signature and Tempo tracks (G).*
- [~] Configure, resize and reorder global tracks (p. 734–735). *The Tempo track resizes; no configuring or reordering.*
- [x] Marker track (p. 737)
- [x] Marker List (p. 737)
- [x] Create markers at the playhead (p. 737–738)
- [x] Go to the next or previous marker (p. 745)
- [ ] Markers from the cycle area or the selected regions (p. 738–739)
- [~] Move, resize, colour and protect markers (p. 740–742). *Move and colour; a marker always reaches the next one, and none can be protected.*
- [~] Rename markers in the track, list or ruler; Marker Text window (p. 743–744). *In the marker lane and the Marker List; no Marker Text window.*
- [~] Go to marker by number; set the cycle to a marker (p. 745). *Set Locators to Marker; no go to marker by number.*
- [ ] Marker sets (p. 746)
- [ ] Import and export marker information in audio files (p. 747)
- [x] Tempo track with tempo changes (a tempo map) (p. 759–762)
- [~] Tempo curves between tempo points (p. 763). *Linear ramps only.*
- [ ] Tempo point operations: constant, smoothing, average, snap (p. 761)
- [ ] Tempo List (p. 764)
- [ ] Tempo sets (p. 765)
- [ ] Record tempo changes (p. 790)
- [x] Signature track with time signature changes (p. 751–752)
- [ ] Key signature changes (p. 753)
- [ ] Edit, move, split and merge signatures; Signature List (p. 751–755)
- [ ] Signature sets (p. 756)

### Advanced editors (p. 460–538)

Event List (p. 460–480)
- [x] Event List as a pane or window (p. 461)
- [~] View the events in a region, or the regions in a project (p. 462). *Events of the selected MIDI regions on one track; no region list.*
- [x] Filter by event type (p. 463). *For the types Fermata stores.*
- [~] Create, duplicate and edit events numerically (p. 467–472). *Create at the playhead and edit by dragging or typing; no Duplicate, Copy or Paste in the list.*
- [~] Note events, including release velocity (p. 474–475). *No release velocity.*
- [~] Controller, pitch bend, program change, aftertouch and poly aftertouch events (p. 475–478). *Controllers, pitch bend and channel aftertouch; no program change or poly aftertouch.*
- [ ] SysEx and meta events (p. 478–479)
- [ ] Event Float window (p. 480)

Step Editor (p. 481–499)
- [ ] Lanes of steps for notes and controllers (p. 481–490)
- [ ] Lane parameters and the Lane inspector (p. 490–495)
- [ ] Lane Sets, including GM drum kits and hi-hat groups (p. 496–499)

MIDI Transform (p. 523–538)
- [ ] Transform window with presets (Humanize, Quantize, Crescendo, Double Speed and so on) (p. 523–527)
- [ ] User transform sets: conditions, operations and the map (p. 526–534)

Audio File Editor (p. 499–523)
- [~] Non-destructive range edits (p. 511). *Fermata's Audio Track Editor has split, trim, silence, delete, normalize, reverse and fade on a selection, and transient detection with Split at Transients. Destructive file editing is missing.*
- [ ] Destructive editing of the audio file, with backup and revert (p. 511, 521)
- [ ] Sample-level view and amplitude scales (p. 502–503)
- [ ] Navigate to the loudest point or to silences (p. 506)
- [ ] Pencil tool to repair clicks (p. 515)
- [ ] Remove DC offset; invert phase (p. 516, 520)
- [ ] Set file levels and loop points (p. 516, 520)
- [ ] Transient editing mode: move, add and delete markers (p. 512–514)
- [ ] Set the project tempo from a selection (p. 510–511)
- [ ] Save the selection as a new file (p. 522)
- [ ] Open in an external sample editor (p. 522–523)

### Project Audio Browser (p. 274–281)

- [ ] Project Audio Browser: list files and regions, play them, add them to tracks (p. 274–278)
- [ ] Delete unused files and unused sections of files (p. 277)
- [ ] Sort, group and rename audio files (p. 278–279)
- [ ] Move, copy or convert files; convert regions to independent files (p. 279–280)
- [ ] Import and export region information in audio files (p. 281)

### Selection-based processing (p. 180–182)

- [ ] Apply effect plug-ins to selected regions or a marquee selection, with preview (p. 180–182)

### Mixing: professional (p. 539–620)

- [x] Plug-in delay compensation (p. 573–575)
- [~] Side-chain inputs for plug-ins (p. 570). *For the built-in Compressor, Noise Gate and De-Esser, keyed post-fader from any track; VST3 side-chain buses aren't connected.*
- [ ] Multi-output instruments (p. 592)
- [ ] VCA faders and VCA groups (p. 603–604)
- [~] Groups (p. 598–603). *Volume, pan, mute, solo, record, selection and editing links, plus an on/off clutch; missing are several groups per strip, a Groups window, automation, send and colour links, and copy/paste of group settings.*
- [ ] Enable or disable all groups (⇧G) (p. 602)
- [ ] Solo safe (p. 558)
- [ ] Pre-fader metering (p. 554)
- [ ] Clip indicators with reset (p. 551–552)
- [ ] Switch between two volume levels (p. 552)
- [ ] Stereo Pan mode, with width (p. 556)
- [ ] Independent send pan (p. 587–588)
- [ ] Copy fader level or pan to a send (p. 588)
- [ ] Sends on Faders (p. 589)
- [ ] Highlight a send's destination; select strips with the same destination (p. 586)
- [ ] Create tracks for aux strips (p. 597)
- [ ] Narrow and wide strips, long faders, show or hide strip components (p. 613–614)
- [ ] Enter and exit folder stacks in the Mixer (p. 614–615)
- [ ] Track notes in the Mixer (p. 617)
- [ ] I/O Labels window (p. 618)
- [ ] Gain-reduction meter on the strip
- [ ] Show or hide all plug-in windows (V)

### Plug-in windows and settings (p. 567–582)

- [~] Plug-in window header (p. 568). *Built-in effect panels have bypass, presets and the side-chain pop-up; VST3 editor windows have no header.*
- [~] Load, save, step through and delete plug-in settings (presets) (p. 571–572). *Factory and user presets for the built-in effects and the Channel EQ, shared by every project; none for VST3s.*
- [~] Compare edits with the saved setting (p. 571). *The preset pop-up shows "(edited)" and offers Revert; no Compare switch.*
- [~] Copy and paste plug-in settings; reset; default setting (p. 572). *Copy and paste for every plug-in (VST3 state included) and the Channel EQ; Default resets built-in effects only.*
- [ ] Link or unlink plug-in windows (p. 569)
- [ ] Plug-in undo and redo (p. 568)
- [ ] Controls view: a generic parameter view (p. 1039)
- [ ] Channel strip settings: save, load, step through, paste only plug-ins or sends (p. 581–582)
- [ ] Copy and paste a whole channel strip (key commands)
- [ ] Remove all plug-ins, empty slots, bypassed plug-ins or sends (p. 582)
- [ ] Plug-in Manager categories and custom names (p. 579–580)

### Library and patches (p. 149–160)

- [ ] Choose a patch (track, channel strip and Smart Controls) for the selected track (p. 150)
- [ ] Browse plug-in presets in the Library (p. 151)
- [ ] Search patches (p. 151)
- [ ] Patch merging: choose which parts of a patch replace the current ones (p. 152)
- [ ] Save, reset and delete patches (p. 152–153)
- [ ] Performance patches (p. 153–154)

### Automation: advanced (p. 621–644)

- [ ] Several automation lanes per track (p. 626–627)
- [ ] Region automation, and converting between region and track automation (p. 621–623)
- [ ] Trim mode (p. 629)
- [ ] Relative automation (p. 629–630)
- [ ] Automation curves (p. 59)
- [ ] Copy and move automation, within and between tracks (p. 637)
- [~] Move automation with regions (p. 638). *Never, Always and Ask, along a track; not onto
  another track or for copies.*
- [ ] Stepped automation and vertical lines (p. 638–639)
- [ ] Points at region, marquee and selection borders (p. 632)
- [ ] Delete redundant, visible, orphaned or all automation (key commands, p. 640)
- [ ] Set modes for selected or all tracks (p. 628)
- [ ] Automation Event List (p. 642)
- [ ] Automation Quick Access (p. 643)
- [ ] Automation for folder stacks and Stereo Out
- [ ] Automation settings: ramp times, which parameters Write overwrites (p. 1064)

### Smart Controls (p. 645–657)

- [ ] Smart Controls pane (B) (p. 645–646)
- [ ] Layouts (p. 647)
- [ ] Automatic mapping to the track's plug-ins (p. 649)
- [ ] Map screen controls with Learn or a menu; several mappings per control (p. 649–652)
- [ ] Parameter mapping graphs (p. 652–653)
- [ ] Assign hardware controls to screen controls (p. 655)
- [ ] Arpeggiator button (p. 656)
- [ ] Automate screen controls (p. 657)

### Project management (p. 105–113)

- [ ] Project alternatives (p. 106–107)
- [ ] Backups of each alternative (p. 107)
- [ ] Clean up project files (p. 107)
- [ ] Consolidate project assets (p. 108)
- [ ] Rename, copy and move projects (p. 109)
- [ ] Import tracks, channel strips and settings from another project (p. 110–113)
- [ ] User project templates

### Bounce and export: professional (p. 183–187, 909–918)

- [~] AIFF, CAF and FLAC bounce (p. 914). *AIFF and FLAC; no CAF.*
- [ ] MP3 and M4A/AAC bounce (p. 916–918)
- [ ] POW-r dither (p. 914)
- [ ] Bounce several outputs at once (p. 914)
- [ ] Add the bounce to the project (p. 914)
- [ ] Export tracks with options: bypass effects, include volume and pan automation (p. 183)

### Windows and workspace (p. 45–54)

- [~] Detach editors, Mixer and lists into separate windows (p. 22–39). *Mixer, Editors, effect panels and Channel EQ; no lists yet.*
- [ ] Screensets: create, recall, lock, copy and import (p. 51–54)
- [ ] Save and recall zoom settings; navigation snapshots (p. 48)
- [ ] Link and Content Catch modes (p. 50)
- [ ] Undo History window (p. 61)

---

## Stage 3: Advanced production

The features that make Logic a production environment rather than a recorder: flexible audio,
smart tempo, samplers, virtual players, pattern tools, Live Loops and notation.

### Flex Time (p. 436–449)

- [ ] Flex on or off per track and region (p. 436–437)
- [ ] Algorithms: Monophonic, Slicing, Rhythmic, Polyphonic, Tempophone, Speed (p. 438–440)
- [ ] Flex markers: add, move, delete, reset; snap to transients in other regions (p. 443–449)
- [ ] Marquee and Flex tool time-stretching (p. 446–447)
- [ ] Quantize the timing of audio tracks (p. 441, 455)
- [ ] Time-stretch audio regions to the locators or nearest bar (p. 317)
- [ ] Choose the time-stretching algorithm (p. 318)

### Flex Pitch (p. 449–457)

- [ ] Flex Pitch editing in the Audio Track Editor (p. 450)
- [ ] Move, resize and merge notes (p. 451–452)
- [ ] Hotspots: pitch drift, fine pitch, gain, vibrato, formant (p. 452–453)
- [ ] Set notes to perfect pitch; reset (p. 454)
- [ ] Quantize pitch to a key or scale (p. 455–456)
- [ ] Create MIDI from audio (p. 456)
- [ ] Per-note gain (p. 457)

### Varispeed (p. 458)

- [ ] Varispeed: speed only, or speed and pitch, in percent or semitones (p. 458)

### Smart Tempo and beat mapping (p. 766–800)

- [ ] Free tempo recording (p. 767, 769–770)
- [ ] Project Tempo modes: Keep, Adapt, Automatic (p. 767–771)
- [ ] Smart Tempo setting per audio region; file tempo (p. 773)
- [ ] Apply a region's tempo to the project; align the detected downbeat (p. 772)
- [ ] Smart Tempo multitrack sets (p. 775)
- [ ] Smart Tempo Editor: hints, tempo ranges, beat markers, locked ranges, double or halve (p. 776–786)
- [ ] Make recordings follow the project tempo; match the project to a region (p. 788)
- [ ] Import and export tempo information in audio files (p. 789)
- [ ] Tempo Operations: scale, stretch, thin out, randomise (p. 790–793)
- [ ] Tempo Interpreter (manual tap tempo sync) (p. 793)
- [ ] Beat Mapping track for MIDI and audio (p. 796–800)

### Groove (p. 167, 361–363)

- [ ] Groove track: match other tracks to one track's timing (p. 167)
- [ ] Groove templates: create, share, and use from the quantize menu (p. 361–363)

### Sampler, Quick Sampler and Drum Machine Designer (p. 122, 329, 564)

- [ ] Sampler instrument (multi-zone, mapping, modulation)
- [ ] Quick Sampler: Classic, One Shot, Slice and Recorder modes
- [ ] Create sampler tracks by dropping files on Sampler or Quick Sampler zones, Original or Optimized (p. 122)
- [ ] Convert audio regions to a new Sampler track, sliced at transients or markers (p. 329)
- [ ] Drum Machine Designer: a pad grid of sub-tracks with per-pad channel strips (p. 122, 697)
- [ ] Sample Alchemy (p. 122)
- [ ] Drum replacement and doubling from a recorded drum track (p. 133)

### Session Players and Drummer (p. 403–435)

- [ ] Session Player tracks and regions (p. 404–405)
- [ ] Session Player Editor (p. 406)
- [ ] Player types: Drummer, Keyboard Player, Bass Player; styles and players (p. 407)
- [ ] Synth Player styles: Modulated Pad, Rhythmic Chords, Pump Bass, 808 Bass, Sequenced Bass (p. 419–428)
- [ ] Presets (p. 409–410)
- [ ] Complexity and intensity XY pad, patterns, fills, swing (p. 411–412)
- [ ] Keyboard Player hands, voicings and movement (p. 413–415)
- [ ] Bass Player notes, octaves, muting, slides (p. 416–418)
- [ ] Drummer kit-piece mutes, brush technique, custom patterns (p. 428–432)
- [ ] Follow the rhythm of another track or the chords (p. 433)
- [ ] Regenerate a performance (p. 432)
- [ ] Multi-channel drum kits (p. 434)
- [ ] Convert a Session Player region to MIDI or a pattern region (p. 435)

### Chords and the Chord track (p. 332–348)

- [ ] Chord track and region chords (p. 333)
- [ ] Add, delete, select, cut, copy and paste chords (p. 334–337)
- [ ] Move, resize, loop and transpose chords (p. 337–339)
- [ ] Edit chords, including from a MIDI controller or as text (p. 339–341)
- [ ] Chord groups and chord progressions (p. 342–345)
- [ ] Halve or double the chord rhythm (p. 345–346)
- [ ] Analyse the key of a range of chords (p. 347)
- [ ] Chords from Apple Loops (p. 260)

### Arrangement track (p. 748–750)

- [ ] Arrangement track with section markers (Intro, Verse, Chorus…) (p. 748)
- [ ] Move, copy, replace, split and delete sections together with their regions (p. 749–750)
- [ ] Cut the sections of selected arrangement markers; insert scenes (key commands)

### Transposition (p. 757)

- [ ] Transposition track / Pitch Source: transpose regions and Apple Loops with the project (p. 757)

### Step Sequencer (p. 695–731)

- [ ] Pattern regions and pattern cells (p. 696)
- [ ] Turn steps on or off; add rows; Learn mode (p. 699–701)
- [ ] Pattern length and step rate, including 5/4 and 7/8 (p. 702–703)
- [ ] Edit modes per row: velocity, gate, note repeat, offset, chance, octave and more; subrows (p. 704–708)
- [ ] Randomise patterns and rows, with probability and range (p. 708–709)
- [ ] Rows as chord degrees; follow the key and chords (p. 711–713)
- [ ] Live and step recording of patterns (p. 713–715)
- [ ] Pattern Browser: load, save, templates (p. 715–717)
- [ ] Convert between MIDI and pattern regions (p. 717)
- [ ] Mute, solo and rotate rows; playback modes (p. 718–719)
- [ ] Step Sequencer with Drum Machine Designer (p. 697–698)
- [ ] Step Sequencer inspector: pattern, row and step settings (p. 723–727)

### Live Loops (p. 658–694)

- [ ] Live Loops grid alongside the Tracks area (p. 658–659)
- [ ] Play, stop, queue and retrigger cells and scenes (p. 660–662)
- [ ] Quantize Start, Smart Pickup, trigger modes (p. 663–665)
- [ ] Add regions, files and loops to cells; copy between grid and Tracks area (p. 666–669)
- [ ] Record into cells, with take folders (p. 670–676)
- [ ] Cell loop settings: start, loop start, length, speed, reverse (p. 676–684)
- [ ] Scenes: select, reorder, duplicate, rename (p. 684–687)
- [ ] Cell Editor (p. 687)
- [ ] Bounce cells in place (p. 689)
- [ ] Record a Live Loops performance to the Tracks area (p. 691)
- [ ] Control Live Loops from controllers (p. 693)

### MIDI effects (p. 560)

- [ ] MIDI FX slots on instrument strips (p. 560)
- [ ] Hosting third-party MIDI effect plug-ins
- [ ] Bundled MIDI FX: Arpeggiator, Chord Trigger, Modifier, Modulator, Note Repeater, Randomizer, Scripter (JavaScript), Transposer, Velocity Processor
- [ ] Arpeggiator presets (p. 656)

### Articulations and MPE (p. 168–175, 577)

- [ ] Articulation sets and the Articulation Set Editor (p. 168–175)
- [ ] Keyswitches view (p. 170)
- [ ] Articulation IDs on notes in the editors (p. 394, 472)
- [ ] MPE controllers with software instruments (p. 577)

### Score Editor (p. 802–907)

- [ ] Score Editor: Linear, Wrapped and Page views (p. 803–806)
- [ ] Enter notes with the pointer, step input or Part box (p. 813–818)
- [ ] Rests, multi-bar rests (p. 818)
- [ ] Key, time signature and clef changes (p. 819–821)
- [ ] Dynamics, slurs, crescendi, accents, trills, ornaments, pedal markings (p. 821–826)
- [ ] Chord symbols, chord grids and tablature (p. 826–828, 851–868)
- [ ] Bar lines, repeats, codas, endings; line and page breaks (p. 828–829)
- [ ] Note attributes: heads, accidentals, stems, beaming, ties, syncopation (p. 832–840)
- [ ] Tuplets, grace notes, independent notes (p. 840–843)
- [ ] Lyrics, text and text styles (p. 844–850)
- [ ] Display quantisation and score region parameters (p. 869–875)
- [ ] Staff styles, including drum and transposing instruments (p. 876–893)
- [ ] Voice separation (p. 886–887)
- [ ] Score sets and part extraction (p. 893–899)
- [ ] Page layout, margins, page numbers (p. 899–902)
- [ ] Print and export to graphics or PDF (p. 903)
- [ ] MusicXML import (p. 806–807)
- [ ] Score project settings (p. 1090–1105)

### Tuning (p. 1116–1119)

- [ ] Project tuning offset (p. 1118)
- [ ] Alternative temperaments, Hermode Tuning and user tuning tables (p. 1116–1119)

---

## Stage 4: Studio integration

Connecting Fermata to the rest of a studio: control surfaces, remotes, sync, video, surround and
external hardware.

### Control surfaces (p. 1182–1201)

- [ ] Add control surfaces by scanning or manually (p. 1185)
- [ ] Mackie Control / Logic Control protocol (p. 1192)
- [ ] Automatic assignment for USB MIDI controllers (p. 1186)
- [ ] Control surface groups (p. 1187)
- [ ] Control Surface inspector and group parameters (p. 1189–1199)
- [ ] OSC devices (p. 1191)
- [ ] Control surface bars in the Mixer (p. 617–618)

### Controller assignments (p. 1201–1222)

- [ ] Learn a controller assignment (⌘L) (p. 1203)
- [ ] Easy view (p. 1202–1204)
- [ ] Expert view: zones, modes, value scaling, feedback (p. 1204–1221)
- [ ] OSC message paths; game controllers (p. 1207–1208, 1219)
- [ ] Assign hardware buttons to key commands (p. 1222)
- [ ] Write automation from external controllers (p. 643)

### Remote control (p. 98)

- [ ] A Logic Remote equivalent: transport, mixer, Smart Controls, Live Loops from a tablet or phone (p. 98)
- [ ] Touch instruments on the remote (p. 98)
- [ ] Mirror Loop Browser filters on the remote (p. 258)

### Synchronisation (p. 1025–1032)

- [ ] Send MIDI clock (p. 1026, 1106)
- [ ] Sync to external MIDI clock or MIDI Time Code (p. 1026–1029)
- [ ] Send MIDI Time Code (p. 1106)
- [ ] Frame rates and MTC interpretation (p. 1028–1029, 1107)
- [ ] MIDI Machine Control (p. 1029–1030)
- [ ] Ableton Link (p. 1027–1028)
- [ ] Synchronisation project settings: bar position vs SMPTE, audio sync (p. 1106–1112)

### Video and movies (p. 1016–1025)

- [ ] Open a movie; Movie window (p. 1016–1018)
- [ ] Movie track with thumbnails (p. 1019)
- [ ] Detect scene cuts; scene markers (p. 1020–1021)
- [ ] Import movie audio; export audio to a movie (p. 1021–1022)
- [ ] SMPTE-lock regions and events; move a region to a frame (p. 1023–1024)
- [ ] Sync audio to a video hit point (p. 789)
- [ ] Movie project settings (p. 1105)

### Surround (p. 931–949, 955–974, 1005–1015)

- [ ] Surround project formats: quad, LCRS, 5.1, 7.1 and others (p. 934, 1005)
- [ ] Surround input and output formats on strips (p. 943–946)
- [ ] Surround Panner and Surround Balancer (p. 955–964)
- [ ] Multichannel and multi-mono effects, with linked instances (p. 969–974)
- [ ] Surround I/O assignments and bounce extensions (p. 937–940)
- [ ] Surround bounce (p. 991)
- [ ] Convert between surround and stereo projects (p. 943, 997)
- [ ] Binaural panner on stereo strips (p. 605–610)

### Spatial Audio with Dolby Atmos (p. 946–1004)

- [ ] Spatial Audio projects; create from stereo or from an ADM file (p. 946–949)
- [ ] Bed tracks and object tracks (p. 952–953)
- [ ] 3D Object Panner (p. 965–968)
- [ ] Dolby Atmos plug-in: object view, monitoring formats (p. 975–982)
- [ ] Binaural rendering, render modes, personalised profiles (p. 982–984)
- [ ] Downmix and trim controls (p. 985–988)
- [ ] Export ADM BWF masters and MP4 (Dolby Digital Plus / AC-4) (p. 994–996)
- [ ] Import QTA files and bed/object tracks from other projects (p. 998–999)

### External hardware and the Environment (p. 117, 594–597, 611–613, 1224–1295)

- [ ] External MIDI tracks (p. 117)
- [ ] External Instrument plug-in: hardware synth in and out as a channel strip (p. 68, 928)
- [ ] I/O plug-in for hardware inserts, with latency detection
- [ ] MIDI channel strips (p. 611–613)
- [ ] Route strips to any hardware output pair or mono output (p. 595–596)
- [ ] Output channel strips (p. 594)
- [ ] Program change, bank select and SysEx to external devices (p. 476–478)
- [ ] Environment: layers, objects, cabling (p. 1224–1241)
- [ ] Environment objects: instruments, multi-instruments, mapped instruments, transformers, arpeggiators, faders, touch tracks (p. 1243–1295)
- [ ] MIDI to Track and Instrument Output routing between tracks (p. 212–214)

### Project interchange (p. 282–287, 929–930)

- [ ] Open GarageBand projects (p. 283–284)
- [ ] Final Cut Pro XML import and export (p. 284, 929)
- [ ] AAF import and export (p. 286–287, 929)
- [ ] MusicXML export (p. 930)
- [ ] Spatial Audio export (p. 930)

---

## Stage 5: Content and AI

Bundled instruments and effects, sound content, and the machine-learning tools. Session Players
and Drummer are listed in Stage 3.

### Bundled instruments (Logic Pro Instruments guide)

Fermata has one: **Fermata Synth**, a small polyphonic synth that responds to pitch bend,
sustain, modulation and expression.

- [~] Synthesisers: Alchemy, Retro Synth, ES1, ES2, ES E, ES M, ES P, EFM1, Sculpture, Vocoder (EVOC 20 PolySynth). *Fermata Synth only.*
- [ ] Granular synthesis, including Alchemy granular sync (p. 10)
- [ ] Sample-based instruments: Sampler, Quick Sampler, Sample Alchemy (engines in Stage 3)
- [ ] Keyboards: Studio Piano, Vintage B3 organ, Vintage Electric Piano, Vintage Clav, Vintage Mellotron
- [ ] Orchestral: Studio Strings, Studio Horns
- [ ] Bass: Studio Bass
- [ ] Drums: Drum Kit Designer, Drum Machine Designer, Drum Synth, Ultrabeat
- [ ] Utility: Klopfgeist (metronome), Test Oscillator
- [ ] Session Player sound content (Synth Session Players pack and others)

### Bundled audio effects (Logic Pro Effects guide)

Fermata has a built-in eight-band **Channel EQ** with analyzer, and eight built-in effects (Compressor, Limiter, Noise Gate, De-Esser, Reverb, Delay, Tuner, Loudness Meter); everything else comes from VST3.

- [~] EQ: Channel EQ, Linear Phase EQ, Match EQ, Single Band EQ, Vintage Console, Graphic and Tube EQs. *Channel EQ only.*
- [~] Dynamics: Compressor (with circuit models), Limiter, Adaptive Limiter, Multipressor, Noise Gate, Expander, Enveloper, DeEsser 2. *Compressor with five circuits, Limiter (with an Adaptive mode), Noise Gate and De-Esser; no Multipressor, Expander or Enveloper.*
- [~] Reverb: ChromaVerb, Space Designer (convolution), Quantec Room Simulator, SilverVerb. *One algorithmic Reverb (plate and five room types); no convolution.*
- [~] Delay: Delay Designer, Echo, Stereo Delay, Tape Delay, Sample Delay. *One Delay with Stereo, Ping-Pong, Tape and Digital modes; no Delay Designer or Sample Delay.*
- [ ] Modulation: Chorus, Ensemble, Flanger, Phaser, Microphaser, Modulation Delay, Ringshifter, Rotor Cabinet, Scanner Vibrato, Spreader, Tremolo
- [ ] Distortion and saturation: ChromaGlow, Bitcrusher, Clip Distortion, Distortion, Distortion II, Overdrive, Phase Distortion
- [ ] Amps and pedals: Amp Designer, Bass Amp Designer, Pedalboard
- [ ] Filters: AutoFilter, EVOC 20 Filterbank and TrackOscillator, Fuzz-Wah, Spectral Gate, Step FX
- [ ] Pitch: Pitch Correction, Pitch Shifter, Vocal Transformer
- [ ] Imaging: Stereo Spread, Direction Mixer, Binaural Post-Processing
- [~] Metering: Level Meter, Loudness Meter, MultiMeter, Correlation Meter, BPM Counter, Tuner. *Loudness Meter and Tuner; no Level Meter, MultiMeter, Correlation Meter or BPM Counter.*
- [ ] Specialised: Exciter, SubBass, Denoiser, Grooveshifter, Phat FX
- [ ] Performance effects: Remix FX, Beat Breaker (with Cutoff, Resonance and Pan modes and randomise, p. 10)
- [ ] Utility: Gain, Multichannel Gain, Test Oscillator, I/O
- [ ] Multichannel versions for surround (p. 1002–1003)

### Sound content and loops (p. 62–64, 248–267)

- [ ] Sound Library: browse, download, delete and relocate sound packs (p. 62–64, 1040)
- [ ] Loop Browser (O) (p. 250)
- [ ] Search and browse loops by keyword, type, scale, signature (p. 252–265)
- [ ] Preview loops at the project tempo and key (p. 257, 267)
- [ ] Favourite loops (p. 259)
- [ ] Loop families: swap a loop for another in the family (p. 261)
- [ ] Create your own loops; add a region to the loop library (p. 261)
- [ ] Index and add untagged loops (p. 263, 266)
- [ ] Apple Loops metadata: follow tempo and pitch (Flex Time and Flex Pitch separately, p. 11)
- [ ] Patch, channel strip and plug-in setting libraries (see Stage 2 for the system)
- [ ] Demo projects (p. 82)

### Machine-learning tools

- [ ] Stem Splitter: separate vocals, drums, bass and other parts from an audio region or cell (p. 353, 670)
- [ ] Mastering Assistant on the Stereo Out strip (p. 1049)
- [ ] Chord ID: detect chords in audio or MIDI regions and add them to the Chord track (p. 11, 348)
- [ ] Session Players' generated performances (Stage 3)
- [ ] Enhanced loop detection: set the file tempo and downbeat on import (p. 10)

---

## Stage 6: Platform

Accessibility, localisation, help and other operating systems. Logic is macOS-only, so most of
this stage is about doing the equivalent on Linux, Windows and macOS.

### Accessibility (p. 1039)

- [x] Readable interface: High Contrast theme and 90–200% interface scaling
- [ ] Screen reader support (VoiceOver in Logic; AT-SPI/Orca on Linux, Narrator on Windows) (p. 1039)
- [ ] Announce the playhead position when playing, recording or scrubbing (p. 1039)
- [ ] Open plug-ins in Controls view for screen readers (p. 1039)
- [~] Full keyboard operation. *Most commands have keys; complete keyboard navigation hasn't been checked.*

### Localisation

- [ ] Translated interface (Logic ships in many languages)
- [ ] Locale-aware number and time formats

### Help and documentation (p. 66–67)

- [ ] Quick Help: describes the element under the pointer (p. 66)
- [~] Help tags (p. 1068). *Some controls have tooltips; no Help Tags setting.*
- [x] User guide (USER_GUIDE.md)
- [x] Developer documentation (ARCHITECTURE.md, DEVELOPING.md)
- [x] README with feature sections and key command tables

### Operating systems and plug-in formats

- [x] Linux build with desktop entry and icons (Nix flake and other distributions)
- [~] Windows and macOS builds. *Released as an NSIS installer and .dmg bundles, but early and less tested; plug-in editor windows are not on Windows yet.*
- [~] Plug-in editor windows on Windows (HWND) and macOS (NSView) *macOS done.*
- [ ] Plug-in editors under native Wayland (they currently run through XWayland)
- [ ] CLAP plug-in hosting
- [ ] Audio Units hosting on macOS
- [ ] VST2 hosting

### Tablet, phone and sharing (p. 908–930)

- [ ] Share projects with a tablet version, and open projects made there (p. 924–928)
- [ ] Consolidate assets and save as a package for sharing (p. 925)
- [ ] Share to music apps, voice-memo apps, AirDrop or mail (Logic's Music app, Voice Memos, AirDrop, Mail) (p. 919–922)
- [ ] Burn an audio CD or DVD (p. 911)
- [ ] File-manager preview of projects (Quick Look in Logic) (p. 113)
- [ ] Privacy and analytics settings (p. 1034)
- [ ] My Info (user name for metadata) (p. 1076)

Not applicable to Fermata (listed for completeness, not counted):
- Apple Creator Studio subscription (p. 64–66)
- Touch Bar controls and shortcuts (p. 1172–1181)
- Apogee device control panel (p. 72)
- Share to GarageBand for iOS (p. 923)

---

## What Fermata has that Logic doesn't, or does differently

- **VST3 instead of Audio Units.** Plug-ins are scanned in a separate process with a cached
  scan report that shows each bundle that failed and why (crashed, timed out, no plug-ins).
- **Linux first.** Native PipeWire (the default), PulseAudio, JACK and ALSA drivers, real-time
  priority through rtkit, and a Nix flake for building, installing and testing.
- **Live latency measurement:** recordings are shifted by the measured round-trip latency.
  Logic relies on the driver's reported latency and the manual Recording Delay.
- **Waveform styles:** Smooth (true shape, showing individual cycles when zoomed in), Mirrored,
  Rectified and Blocks.
- **Themes:** Studio, Flat, Midnight, Light and High Contrast, which switch instantly. Logic's
  look is essentially fixed.
- **Overlapping recordings** can go on top, onto a new track with the same plug-ins, or replace
  what's underneath, with a MIDI merge option.
- **Piano Roll ideas from other DAWs** (Ableton, Cubase, FL Studio): Humanize, Glue Same
  Pitches, a Line tool for velocity and controller ramps, and colour by velocity or region.
- **Audio Track Editor with range tools:** Logic's non-destructive Audio Track Editor combined
  with the selection tools of its Audio File Editor (silence, normalize, reverse, fade to
  selection, split at transients) without touching the file.
- **Anti-click fades** applied automatically to new recordings and imports.
- **Open project format:** a `.fermata` folder holding readable `project.json` and plain WAV files.
- **Configurable history** of 10–1000 undo steps.

## Recommended next items

The ten gaps that would add the most value. PDC, freeze, markers, Autopunch, take folders and
comping, the tempo map and signatures, plug-in presets (built-in effects and Channel EQ),
side-chains for the built-in dynamics and the bundled effects core are done; items 4 to 6 are
partly done, as noted. [TODO.md](TODO.md) has the staged roadmap.

1. **Plug-in editor windows on Windows and macOS.** Without them, Fermata is effectively a
   Linux-only DAW for anyone using plug-ins. *macOS is done; Windows is not.*
2. **Crossfades, fade curves and drag modes** (X-Fade and Shuffle). These are the basis of
   clean audio editing and comping. *Done: fades, crossfades and all five drag modes.*
3. **Marquee tool and Tracks-area Tool menu** (Scissors, Glue, Fade, Pencil, Eraser): the
   everyday editing workflow.
4. **Plug-in presets and channel strip settings**, with compare and copy/paste. Without them,
   any sound worth keeping has to be rebuilt by hand. *Presets for the built-in effects and the
   Channel EQ, and copy and paste of any plug-in, are done; channel strip settings, Compare and
   VST3 presets are not.*
5. **Side-chain inputs and multi-output instruments.** Ducking, gating and multi-out drum
   instruments are standard in modern mixing. *Side-chains work for the built-in Compressor,
   Noise Gate and De-Esser; VST3 side-chains and multi-output instruments are missing.*
6. **A bundled effects core:** Compressor, Limiter, Noise Gate, reverb, delay and a tuner. A new
   project should be mixable without third-party plug-ins. *Done, with a De-Esser and Loudness
   Meter; modulation and distortion effects are next.*
7. **Event List and Step Input**, for precise MIDI editing and for entering notes without
   playing them.
8. **Track alternatives and project alternatives/backups**, so engineers can try ideas safely.
9. **Region playback parameters** (non-destructive quantize, transpose, velocity, delay) and
   **MIDI controller learn / controller assignments**, which also lays the groundwork for
   control surfaces. *Region playback parameters are done.*
10. **More bounce formats** (FLAC, MP3/AAC, AIFF), **export of selected tracks**, and **VCA
    faders**, to finish the mixing and delivery path.
