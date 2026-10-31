# Changelog

All notable changes to Fermata are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Fermata uses
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- A project with a sampler saved on Windows now finds its samples on Linux and macOS: sample paths
  are kept with forward slashes.

## [0.1.0-alpha.4] - 2026-10-31

### Added

- **Fermata Sampler:** make instruments from your own sounds. Drop or choose audio files and play them across the
  keyboard at once, record straight into it from the audio input, or turn an audio region into one with Convert to
  Sampler Track, whole or sliced at its transients. Map samples across keys and velocities with loops and
  crossfades, one-shot, reverse, velocity layers and round robin, an amp envelope, filter with envelope, LFO, glide and
  polyphony. Several files map themselves by the note in their names, or chromatically. Save an instrument with its
  samples to use in any project; it appears in the preset menu, from `~/Music/Fermata Instruments`. A useful subset of SFZ loads, and what it
  doesn't do is listed. Samples are copied into the project, so it survives being moved or the originals deleted, and a
  missing sample shows plainly while the rest play. Samples are held in memory, up to 4 GB. It hasn't been heard or seen yet.
- **Built-in instruments and Fermata Tre:** a framework for instruments with settings, on the same
  footing as the built-in effects: a panel with sections and a small keyboard, docked or floating, factory
  and user presets, automation, MIDI learn, undo, saving, Freeze, Bounce and copy and paste. The first is
  Fermata Tre, a three-oscillator monophonic synth with a mixer that overdrives, a 24 dB transistor-ladder
  filter that self-oscillates in tune, two contours with a Decay switch, glide, oscillator drift, and osc 3 and noise as
  modulators, plus Unison and Poly. Modelled on the Minimoog Model D's documented behaviour (no affiliation), with
  19 factory presets; it hasn't been compared with the hardware. Fermata Synth stays the default for new instrument
  tracks, and its projects render exactly as before.
- **Five more synth models:** under Vintage Synths, **Fermata Sei** (six voices of one DCO with saw, pulse and sub, a 24 dB
  filter, one envelope with VCA gate, a four-position high-pass and a stereo bucket-brigade chorus with modes I, II and I+II;
  modelled on the Juno-106 family), **Fermata Cinque** (five voices of two oscillators with Poly Mod from the filter envelope and Osc B to
  Osc A's pitch and width and the filter; modelled on the Prophet-5), **Fermata Uno** (one oscillator, a diode ladder, accent with its sweep, and slide; from a MIDI region overlapping notes slide and high velocity accents;
  modelled on the TB-303) and **Fermata Sedici** (six-operator FM with the 32 algorithms, four-rate envelopes, keyboard scaling, 16 voices, at the original's 49.096 kHz with an optional converter character; modelled on the DX7). Under
  Modern Synths, **Fermata Onda**: two wavetable oscillators with position scan, band-limited tables generated in code, unison, seven filters, three envelopes,
  two LFOs and eight modulation slots. 14 to 16 factory presets each. Fermata isn't affiliated with any of those makers, and none has been compared with the hardware or heard yet.
- **Multi-core track processing:** independent tracks play at the same time on a small pool of
  real-time threads, with the audio thread working too, so a heavy project finishes each buffer
  sooner. Settings › Audio › Processing Threads chooses Automatic (the default) or a number; 1 keeps
  everything on the audio thread, and the change applies without restarting audio. The output is the
  same sample for sample however many threads play it. A light project stays on the audio thread alone,
  and bounces are unchanged. The CPU meter still shows how much of each buffer's time it takes to
  finish the buffer, and its tooltip now says how many threads are playing.
- **Undo for changes made in a VST3's own editor:** a knob moved in a plug-in's window is an undo
  step, labelled with the plug-in and the parameter ("TONE3000: Gain"), and Undo and Redo put the
  plug-in and its open window back. A plug-in that doesn't announce its drags gets a step per
  burst of edits; a preset loaded inside a plug-in that reports it is one step. Moves captured by
  automation being written stay in that pass's step, and neither automation playing, a controller
  assignment nor a plug-in moving its own parameters adds any. An edit on a bypassed plug-in now
  reaches it at once rather than when it's next switched on.
- **Dragging regions below the last track** creates a new track, as in Logic: the region is
  drawn on a new lane there while you drag, and the drop makes a track with the source track's
  settings (audio or software instrument, as the region). A selection spanning tracks gets one
  new track per source lane past the end. Alt-drag copies do the same, and one undo removes it
  all.
- **Track On/Off** (Logic's power button): in the track header, the Mixer, the track menu and
  Track › Toggle Track On/Off. An Off track makes no sound and runs nothing: no regions, no
  instrument, no EQ, no plug-ins, no sends, no delay compensation, so it frees the CPU that a
  muted track keeps using. Its plug-ins stay loaded and carry on when it's back on, without a
  tail or a click. A stack's Off reaches its subtracks. An Off track can't record, and doesn't
  appear in bounces or exports. It is saved with the project.
- **Side-chain inputs for VST3 plug-ins:** a plug-in with an aux input (LSP's Sidechain
  Compressor and Gate, say) gets a **Side Chain** item in its slot's menus, with the same choice
  of source and Pre / Post as a built-in effect's pop-up. With a source its aux input turns on
  and hears that track; with none it stays off and the plug-in uses its internal key. It works
  through Off tracks, Freeze, bounces, undo, duplicating and channel strip settings, and is
  saved with the project.
- **Multi-output instruments**, Logic's way: the strip of a VST3 instrument with extra outputs
  (a drum sampler's direct outputs, say) has a **+** that adds an aux for the next output, named
  after the track and the output, with its own fader, pan, plug-ins and sends. The aux stays in
  step with the main output through delay compensation, and bounces and exports include it. An
  output only runs while an aux uses it. Freeze and Bounce in Place refuse an instrument with such
  auxes for now.

### Changed

- **The CPU meter says what it measures and shows what the average hides.** Its tooltip explains
  that it's the audio thread's load, not the system monitor's figure, with the average and recent
  peak. A thin tick marks the worst single block for two seconds, and the label stays red once
  a block has gone over 100%, with a count in the tooltip, until you click the meter.
- **A System Overload message says what happened:** how long the buffer took and which track took
  longest, or which track a stuck processing thread is in.
- **Built-in instrument panels open in the bottom section,** with their sections in one row that the
  mouse wheel moves along. Row and Grid beside the panel's keyboard choose the layout; a floating panel
  starts in Grid, and each place remembers its own choice.

### Fixed

- Undoing or redoing the bypass of an instrument now reaches the running one, and a built-in instrument
  that gets bypassed lets go of the notes it was holding, rather than keeping them when it comes back.

- Holding a key no longer repeats toggles and one-shot commands: holding Space started and
  stopped the transport over and over. Moving, zooming and nudging still repeat.
- Dragging a region onto a frozen track lost it, and dragging one off a frozen track doubled
  it. Both now leave the region where it was, with the existing "is frozen" message.
- On Linux, one audio block taking longer than the buffer's length ended the app with SIGXCPU.
  Opening a heavy plug-in's editor could do it. It's now a dropout at worst.
- Editing during playback no longer restarts held notes. Adding, deleting or reordering a track,
  adding an aux, changing a send, moving a fader, selecting a track, switching the metronome or
  moving the cycle each released every instrument's notes and chased them again, which a sampler
  that ignores note-offs played doubled. Now a track's notes only restart when its own instrument
  or regions change, or when a key held on it loses its way: moving the MIDI target or turning
  MIDI Thru off with a key down releases that key, which with a VST3 instrument used to stick.
- Changing or removing an instrument during playback now releases the held notes and starts them
  on the new one, and so does editing a region, where the note used to stop until the next one.

## [0.1.0-alpha.3] - 2026-10-30

### Fixed

- With snapping on, moving a region failed with "invalid args `moves` for command
  `move_regions`" at most tempos other than 120 BPM, where a beat isn't a whole number of
  samples. Trimming regions, dragging a loop's end or an Arrangement section's end, setting
  the cycle in the ruler and adding a marker at the playhead could fail the same way. Snapped
  positions are now whole samples.

## [0.1.0-alpha.2] - 2026-10-30

### Added

- **Autosave after recording** (Settings › General › After Recording, on by default): the
  autosave is written as soon as a take is placed, and after Bounce in Place, Freeze and
  imports.
- **Recovering an interrupted recording:** takes being recorded when Fermata crashed, was
  killed or lost power are repaired and offered in the Recover sheet, placed where they would
  have landed (cycle takes as a take folder).
- Saves are atomic; a damaged project file falls back to the autosave, then the newest backup
  (or `project.json.bak` when Backups is set to 0). A panic on the main thread writes an
  emergency autosave.
- **Comps and MIDI take folders:** a take folder keeps several comps (Create New Comp,
  Duplicate Comp, Rename Comp, Delete Comp and the comp list in its menu; the name bar shows
  the comp heard). Settings › Recording › MIDI › Create take folders makes each MIDI cycle pass
  a take, and MIDI recordings over regions take folders; MIDI folders play whole takes. Take
  folders crossfade with their neighbours, loops crossfade at their loop end and optionally at
  every repetition (Crossfade Loop Repetitions), and Settings › Editing › Crossfade Time and
  Crossfade Curve set the take folder, X-Fade and Crossfade menu crossfades (comp boundaries
  are now 20 ms by default instead of 10 ms).
- **Record MIDI in Replace mode:** Settings › Recording › Overlapping Recordings › MIDI ›
  Replace clears the notes and controllers in the recorded range of the regions underneath,
  and a **Replace** button in the control bar makes every recording, audio and MIDI, replace.
  Merge now merges recorded controllers too, not only notes.
- **Chase: Notes:** starting playback, jumping or cycling back in the middle of a long note
  starts it, as in Logic. Settings › MIDI › Chase switches note and controller chase.
- **Tracks-area tools:** Left-click and Ctrl-click (⌘-click) Tool pop-ups in the Tracks
  area's bar and Logic's Tool menu (T, then the tool's letter), with the Pointer, Pencil
  (empty MIDI regions), Eraser, Scissors (Alt+click for equal pieces), Glue, Mute, Fade (fades
  and crossfades by dragging) and Marquee tools, each with its own pointer.
- **Marquee selection:** a time range across tracks (the Ctrl-click tool by default). Delete,
  Cut, Copy and Paste act on the part of the regions inside it, and a Pointer click inside
  splits the regions at its edges.
- **Drag modes No Overlap, Shuffle R and Shuffle L** (the Drag pop-up in the Tracks area):
  No Overlap shortens a region that an edit overlaps; the Shuffle modes keep a track's regions
  butted together, closing gaps on delete and trim and swapping neighbours on a drag. Drags
  are drawn where they will land.
- **Settings › Editing › Move Automation with Regions** (Never, Always, Ask): a region moved
  along its track takes the automation under it along, in the same undo step.
- **Fades and crossfades:** a curve for each fade, Speed Up and Slow Down fades, and X, EqP
  and X S crossfades between adjacent or overlapping regions (Inspector, the region menu's
  Crossfade item, or the handle on the canvas). The X-Fade drag mode crossfades overlaps
  automatically. Bounce, export, freeze and Join render them as playback does.
- **Region gain handle:** hovering over an audio region shows its gain at the top centre of
  its name bar; drag it up or down (Shift for fine steps) to change the gain of every selected
  audio region by the same amount, with the waveform following, or double-click it for 0 dB.
  A drag is one undo step.
- **Snap Edits to Zero Crossings** (Edit menu, and a key command): splits, trims and range
  edits of audio regions move to the nearest zero crossing within 5 ms. Saved with the project.
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
- **Arrangement track** (the top global track, View › Show Global Tracks): named sections
  (Intro, Verse, Chorus…) that tile the timeline, added with +, renamed from Logic's name
  pop-up, coloured by name, and resized by their right edge. Dragging a section moves it with
  its content (regions on every track cut at its edges, automation and markers) and Alt-drag
  copies it; a section can be deleted with or without its content. Clicking one selects its
  regions.
- **Markers with their own lengths:** set from the marker menu's Length submenu, the Marker
  List's new Length column, or by dragging the end of a marker's bar. Clicking a marker selects
  it (Delete removes it); with Cycle on, or dragged into the cycle strip, it sets the locators.
- **Overview:** the Tracks area's horizontal scroll bar is now a miniature of the whole
  project (regions, playhead, cycle, markers, project end) with a box for the visible area:
  drag it to scroll, drag its edges to zoom, click elsewhere to jump there (View › Show
  Overview).
- **Scroll to Playhead** button beside Catch (Alt+\`); a second Stop also brings the playhead into view.
- **Settings › Editing › Mouse Wheel Scrolls:** Horizontally (time) makes a mouse's vertical
  wheel scroll through time in the Tracks area and Piano Roll, with Shift for vertical.
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
- **Step Sequencer:** pattern regions on software instrument tracks (Track › New Pattern
  Region, or Create Pattern Region in an instrument track's right-click menu), edited as rows
  of steps in a new editor tab beside the Piano Roll and Event List (also in its own window).
  Note On, Velocity, Gate and Tie edit modes with click-and-drag painting, per-row pitch (with
  the General MIDI drum names and Learn), mute, solo, step rate and length (polymeter),
  pattern length, rate, swing and loop, and the playing step lit during playback. Pattern
  regions show their steps in the Tracks area, play in every render, and convert to and from
  MIDI regions.
- **Step Input Keyboard** (View › Step Input Keyboard, Ctrl+Alt+Shift+K, or the Piano Roll's
  bar): enter notes one step at a time at the playhead, from its own keyboard, a MIDI keyboard
  or Musical Typing, with note values 1/1 to 1/128 (dotted, triplet), velocities ppp to fff or
  as played, Rest, Back, Chord, Sustain and Quantize, and key commands for them. Notes go into
  the Piano Roll's region under the playhead, which grows or is made as needed; each step is
  one undo step.
- **Controller curves** in the Piano Roll's controller lane: the Line tool draws ramps along a
  Linear, Exponential, Logarithmic or S-Curve shape at a chosen Ramp Density, joins the
  points it starts and ends on, and Create Ramp and Thin Out fill and thin the selected points.
- **MIDI Learn and controller assignments** (Fermata › Control Surfaces): Learn Assignment
  for Last Touched Parameter (Ctrl+L) or a control's right-click Learn MIDI Assignment assigns
  the next control change or pitch bend to a fader, pan, send, Mute, Solo, Record, Stereo Out,
  Channel EQ setting, built-in effect or VST3 parameter, or a transport button. Assigned
  controllers move their parameter like the mouse (written as automation in Touch, Latch and
  Write) and don't reach the instrument. The Controller Assignments window (Ctrl+Shift+L) lists,
  edits (Jump or Pickup, range) and deletes them; they're saved with the settings.
- **More built-in effects:** **Chorus**, **Flanger**, **Phaser** and **Tremolo** (Modulation;
  the Tremolo syncs to the tempo, locked to the bar), **Overdrive** and **Distortion**
  (Distortion; Soft, Hard, Tube and Fuzz curves, 4× oversampled, level-compensated drive) and
  **Gain** (Utility: gain, phase invert, swap, mono, balance). Each has its panel, automation,
  factory presets and freeze and bounce support.
- **Linear Phase EQ** (EQ category): the Channel EQ's eight bands with linear phase, in the
  Channel EQ's own editor, with a **Quality** pop-up (Low, Medium, High: 21, 43 or 85 ms of
  latency at 48 kHz, made up by delay compensation). Settings change without clicks, follow
  automation, and render identically in bounces and freezes.
- **Delay compensation changes glide:** inserting or removing a latent plug-in, changing a
  plug-in's latency or switching Low Latency Mode while playing now crossfades each compensated
  path (track outputs, sends and auxes) to its new delay over 15 ms instead of clicking.
- **Channel strip settings:** the **Setting** button at the top of each strip (and the strip
  name's right-click menu) copies and pastes a strip's instrument, Audio FX (VST3 state
  included) and Channel EQ, resets the strip, and saves settings to load in any project.
  Paste, Reset and loading apply to all selected strips as one undo step.
- **Clip indicators:** each strip's peak readout holds its highest peak until reset, turns red
  over 0 dBFS and lights the top of the meter (and the end of the track header's meter); click
  to reset, Alt-click (or the key command Reset All Clip Indicators) to reset all.
- **Gain-reduction meter** beside a strip's level meter when it has a Compressor, Limiter,
  Noise Gate or De-Esser.
- **VCA faders:** Mixer › Options › Create New VCA, or a strip's new **VCA** slot. A VCA strip's
  fader offsets the level of every strip assigned to it without moving their faders (an
  outlined cap on each fader shows where it really plays), its Mute mutes them, and VCAs can be
  nested. Volume automation, bounces, exports and frozen tracks follow the VCA.
- **Pre-fader side-chains:** a **Pre / Post** switch beside the Side Chain pop-up of the
  Compressor, Noise Gate and De-Esser keys the effect from the source before its fader, so a
  trigger track pulled all the way down still works. Existing projects stay post-fader.
- **Automation lanes, curves, stacks and Stereo Out:** the **+** on a lane header opens more
  lanes below a track, each with its own parameter; **Show Used Automation** (Shift+A) shows
  every lane with data. Ctrl+Shift-drag (Control-Shift on macOS, or Alt+Shift) bends a segment
  into a curve; double-click straightens it. Folder stacks automate their volume and mute,
  and **Track › Show Output Track** shows Stereo Out with lanes for its volume and plug-ins.
  Bounces and exports render all of it.
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
- With Musical Typing on, Shift with a note key played the note instead of running its key
  command (Shift+F, Shift+S and the other Select commands, Shift+A, Shift+K, Shift+R).
- Step Input's keys (note values, Rest, Back…) did nothing in a detached Piano Roll's window.
- Controller Assignments… in a detached Mixer's or effect panel's right-click menu did nothing;
  the sheet now opens in the main window.
- Step Input, the Piano Roll and the Event List put notes into pattern regions, which play their
  steps instead, so the notes were never heard; they now leave pattern regions alone.
- Clicking the current project alternative in File › Project Alternatives, or cancelling a
  switch, left the menu's tick on the wrong alternative.
- Edit › Delete removed the selected regions even when automation points, a tempo change or a
  marker were selected; it now deletes those first, as the Delete key does.
- On macOS, renaming a region, take, comp, marker or arrangement marker, Save Channel Strip
  Setting As… and the Piano Roll's Transpose by…, Set Velocity…, Scale Velocity…, Custom
  Humanize… and Other Controller… did nothing: macOS's web view has no `window.prompt`. They now
  ask in Fermata's own name sheet (which detached windows show too).
- Key commands reached the app behind an open sheet once the focus was outside its text fields:
  Backspace deleted regions while Strip Silence was open after clicking its slider. Sheets
  (Strip Silence, Group Settings, the name sheet, Settings, Bounce…) now hold back key
  commands, Esc closes them and Return presses the default button. Controller Assignments
  stays non-modal, as in Logic: Learn Assignment and the transport keys still work.
- Copy Channel Strip Settings, Copy Plug-in and Copy Channel EQ in a detached Mixer couldn't be
  pasted in the main window (or the other way round): the clipboards are now shared.
- Edit › Select commands (Shift+I, Shift+F, Shift+M…) selected regions while the Piano Roll or
  Event List had focus; Invert Selection, All Following, Inside Locators, Muted, Same Colored and
  Equal now select its notes or events.
- Delete or Backspace with the Step Sequencer focused deleted the pattern region being edited;
  they now do nothing there.
- A VCA on a track inside a folder stack was ignored (level and mute) while the folder's
  volume or mute automation played.
- A folder stack's automation played early by the delay compensation (up to 85 ms with the
  Linear Phase EQ on High) instead of in line with its subtracks.
- With delay compensation, the last moments of a bounce or export (the plug-in latency) ignored
  automation and Stereo Out's lanes, so a fade to silence could end with a blip.
- Bounces and exports could play a plug-in parameter at a value set by hand instead of its
  automation in Read, on tracks and on Stereo Out; a plug-in put back on Stereo Out by undo
  waited for its lane to change. Bounces now also chase notes and controllers held over their
  start, as playback does.
- After undo, a plug-in could inherit the latency of a Linear Phase EQ (or other built-in
  effect) that had been undone, delaying the rest of the mix to compensate for nothing.
- Duplicate Track (and Move Regions to New Track) shared the original's track alternatives:
  the copy's alternatives held the same regions, so editing one changed the other. The copy now
  gets its own (or none, without content).
- Moving, copying or deleting an Arrangement section left Stereo Out's automation behind.
- Low Latency Mode turned off the wrong plug-in after inserts were reordered.
- Switching a track's or Stereo Out's automation to Off or Write left a built-in effect's
  automated parameter at the lane's last value, so the project sounded different once reopened;
  it now goes back to its own value.

### Known issues

- VST3 editor windows open on Linux (X11, through XWayland on Wayland) and macOS; on Windows
  plug-ins load, play and can be automated, but show no editor.
- Windows and macOS builds are early and have had little hands-on testing. Only the system's
  default audio driver is available there (WASAPI, Core Audio; no ASIO).
- The builds are unsigned: macOS needs right-click › Open (or `xattr -cr`), Windows
  SmartScreen needs More info › Run anyway.
- A region's length stays in samples when it is moved across a tempo change; only its start
  follows the bars and beats.
- VST3 side-chain buses are not connected, and VCA levels can't be automated or assigned to a
  controller.
- Pattern regions restart their pattern where they're cut, so an Arrangement move or section
  edit across one changes what plays. Arrangement edits don't move the regions of a track's
  inactive alternatives.
- The `takes_check` headless check "autopunch trims the take" fails about one run in five,
  from the null audio device's timing.

The full list is in the User Guide, [Known limitations](https://github.com/JackMechem/fermata/blob/main/docs/USER_GUIDE.md#20-known-limitations).

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

[Unreleased]: https://github.com/JackMechem/fermata/compare/v0.1.0-alpha.4...HEAD
[0.1.0-alpha.4]: https://github.com/JackMechem/fermata/compare/v0.1.0-alpha.3...v0.1.0-alpha.4
[0.1.0-alpha.3]: https://github.com/JackMechem/fermata/compare/v0.1.0-alpha.2...v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/JackMechem/fermata/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/JackMechem/fermata/releases/tag/v0.1.0-alpha.1
