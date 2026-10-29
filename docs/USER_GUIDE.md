# Fermata User Guide

Fermata is a digital audio workstation modelled on Logic Pro. This guide is organised like
Logic's manual: it starts with setting up and recording, then covers editing, the editors,
mixing, exporting, project management, and ends with a complete reference of settings and key
commands.

**Keyboard convention.** Fermata runs on Linux, Windows and macOS. In this guide **Ctrl means
⌘ (Command) and Alt means ⌥ (Option) on macOS**; Shift is Shift everywhere. So "Ctrl+D" is
Ctrl+D on Linux and Windows and ⌘D on macOS. A few commands use the Control key on macOS, as
Logic does; for those both forms are given, for example "Ctrl+R (Control+R on macOS)".

Fermata shows shortcuts the way your platform does: "Ctrl+Shift+Z" on Linux and Windows, "⇧⌘Z"
on macOS. That applies to menus, tooltips and Settings › Key Commands, and tooltips follow any
keys you reassign there.

Contents:

1. [Getting started](#1-getting-started)
2. [The main window](#2-the-main-window)
3. [Recording](#3-recording)
4. [Software instruments and Musical Typing](#4-software-instruments-and-musical-typing)
5. [Editing in the tracks area](#5-editing-in-the-tracks-area)
6. [Track stacks, groups, hiding and freezing](#6-track-stacks-groups-hiding-and-freezing)
7. [Take folders and comping](#7-take-folders-and-comping)
8. [Markers](#8-markers)
9. [Tempo and time signature](#9-tempo-and-time-signature)
10. [The Piano Roll](#10-the-piano-roll)
11. [The Audio Track Editor](#11-the-audio-track-editor)
12. [Mixing](#12-mixing)
13. [Automation](#13-automation)
14. [Bounce and export](#14-bounce-and-export)
15. [Projects](#15-projects)
16. [Settings reference](#16-settings-reference)
17. [Key command reference](#17-key-command-reference)
18. [Troubleshooting](#18-troubleshooting)
19. [Known limitations](#19-known-limitations)

---

## 1. Getting started

### First launch

1. On first launch Fermata scans your VST3 plug-ins. Each bundle is loaded in a separate
   process, so a plug-in that crashes can't take Fermata down. The result is cached; rescan
   from Settings › Plug-ins or the Library.
2. The **Project Chooser** opens (unless Settings › General › At Launch says otherwise). Pick a
   template, give the project a name and a location, and click **Create**. The project is saved
   to its folder straight away, so recordings land in the right place.
3. Open **Settings › Audio** (Fermata › Settings…, Ctrl+,) and choose your driver and devices
   (below).

### Choosing a driver and devices

Settings › Audio holds the device settings. The driver, devices, sample rate, buffer size,
Monitoring and Recording Delay take effect when you click **Apply Changes**; Done, switching
tabs or closing the window drops changes you haven't applied. The delay compensation options on
the same tab apply at once.

- **Driver.** On Linux the choices are **PipeWire** (recommended), PulseAudio, JACK (listed as
  "JACK (no server running)" when there is no JACK server) and **ALSA (direct hardware)**. On
  Windows and macOS the system's default driver is used (WASAPI, Core Audio). Changing the
  driver resets both devices to System Default.
- **Output Device** and **Input Device.** They can be different devices. Choose
  **None (playback only)** as the input if you only play back.
- **Sample Rate.** Only the rates both devices support are offered.
- **I/O Buffer Size.** 32 to 2048 samples; the time per buffer is shown next to it. Smaller is
  more responsive, larger is safer against crackles.
- **Reset Audio Engine** closes and reopens the devices, for example after replugging an
  interface.

The status line at the bottom of the window shows the devices, the sample rate and the buffer
size, or "Audio stopped" with the reason. Click it to open Settings › Audio.

### The input level check (Linux)

PipeWire and PulseAudio have their own volume control for each recording device, and it is
often below 100%. That lowers everything you record before Fermata sees it, and at 0% or
muted you record silence. With PipeWire or PulseAudio, Settings › Audio checks this level
(using `wpctl`) and warns when the input is muted or turned down, with a **Set to 100%** button.
Fermata also shows a notice at launch when the level is low. Set your recording level with the
gain knob on your interface instead.

The check is not available with JACK, with direct `hw:` ALSA devices, or on Windows and macOS.

### Monitoring

- A track's **I** button (Input Monitoring) plays its input through Fermata and its plug-ins.
- Settings › Audio › **Monitoring: "Hear record-enabled tracks without pressing I"** (off by
  default) makes armed audio tracks monitor automatically. Leave it off if your interface or
  pedal monitors directly, or you will hear a delayed second copy.

---

## 2. The main window

The window is laid out like Logic Pro 11:

| Area | Where | Show or hide |
|---|---|---|
| Control bar | top | always |
| Library | far left | Y |
| Inspector | left of the track headers | I |
| Tracks area | centre | always |
| Mixer or Editors | bottom (one at a time) | X (Mixer), E or P (Editors) |
| Browser (All Files) | right | F |
| Status line | bottom edge | always |

Drag the dividers between panes to resize them; double-click a divider to reset it. Pane
sizes are remembered.

### Control bar

From left to right:

- **Library, Inspector, Mixer, Editors** buttons.
- **Transport:** Rewind (`,`, to the previous bar line), Forward (`.`, to the next bar line),
  Stop, Play (Space), Record (R). Stop while stopped returns to the left locator when Cycle is
  on, otherwise to the start, and the Tracks area and the open editor scroll sideways to show
  it if it's off screen. Other jumps (Go to Beginning, Rewind, Forward, markers) leave the view
  where it is; **Scroll to Playhead** (the button beside Catch, or Alt+\`) brings the playhead into view.
- **The LCD:**
  - **BAR / BEAT:** the playhead position. Drag vertically to move the playhead by bars.
  - **TEMPO:** drag vertically to change it, or double-click to type a value (Enter applies,
    Esc cancels).
  - **Time signature:** click for 2/4, 3/4, 4/4, 5/4, 6/8, 7/8, 9/8 and 12/8.
  - **Key:** click for the major and minor keys.
  - The **chevron** offers "Beats & Project", "Time" (the position as time) and
    "Project Settings…".
- **Mode buttons:** Cycle (C), Autopunch, Count-in (the "1234" button, Shift+K), Metronome (K).
- **Master volume** slider and the **CPU** meter (the audio thread's load; amber above 50%, red
  above 80%).
- **Musical Typing** (Ctrl+K) and **Browsers** (F) buttons.

### Tracks area

- **Local menu bar:** Edit, Functions and View menus; on the right, the **Catch** button
  (\`, on by default: the view follows the playhead), **Scroll to Playhead** (shows the
  playhead without moving it), the **Snap** pop-up (Smart, Bar, Beat,
  Off; Smart follows the zoom), the track height slider and the horizontal zoom slider.
- **Ruler**, from the top:
  - the **cycle strip** (yellow): drag to make a cycle range, drag it or its edges to move or
    resize it, click it to switch Cycle on and off;
  - the **bar numbers**: click or drag to move the playhead;
  - the **Autopunch strip** (red): works like the cycle strip, for the punch range;
  - the **marker lane** (see [Markers](#8-markers)).
- **Track headers:** colour, number, icon, name, the M (Mute), S (Solo), R (Record Enable,
  audio and instrument tracks), I (Input Monitoring, audio tracks) buttons, the Freeze
  (snowflake) button, a group badge, and a volume slider whose groove is the track's level
  meter. Short tracks put it all on one line (see below). The **+** button above the headers
  opens New Tracks; the **H** button turns on hide mode.
- **Tracks:** regions on lanes. Scroll with the wheel (Shift+wheel or a sideways trackpad swipe
  scrolls through time); Ctrl+wheel zooms horizontally around the pointer, Alt+wheel changes
  track heights. With a mouse that has only a vertical wheel, set Settings › Editing › Mouse
  Wheel Scrolls to **Horizontally (time)**: the wheel then scrolls through time and Shift+wheel
  scrolls the tracks. Over the track headers the wheel always scrolls the tracks, and over the
  ruler and global tracks it always scrolls through time. A click without a drag, on a region or on empty
  space, also moves the playhead there (snapped to the grid). Settings › Editing › Click Moves
  Playhead can limit this to empty space or turn it off.
- **Overview:** the strip along the bottom, in place of a horizontal scroll bar, shows the
  whole project in miniature: a row per track (a closed stack is one row), each region as a
  bar in its colour, the cycle range and markers along the top edge, the playhead, and the end
  of the last region, with the timeline after it shaded darker. The lit box is the part shown
  in the Tracks area, both the time and the tracks scrolled into view.
  - Drag the box to scroll, across and up or down.
  - Click outside the box to centre the view there; keep the button down to go on dragging.
  - Drag the box's left or right edge to zoom: the view shows the time between the edges
    (within the usual zoom limits).
  - The wheel scrolls through time (Shift+wheel scrolls the tracks); Ctrl+wheel zooms around
    the pointer when it's over the box, otherwise around the middle of the view.
  - View › Show Overview (in the main menu bar or the Tracks area's View menu), or right-click
    the strip, hides it and brings back the scroll bar; the same menu item, or a right-click on
    the scroll bar, shows it again. The choice is remembered.

Track headers:

- **Select** a track by clicking its header; Ctrl+click adds or removes a track; Shift+click
  selects a range.
- **Rename** by double-clicking the name.
- **Reorder** by dragging the track number or icon (just the number on the shortest tracks,
  which have no icon). A track dropped among a stack's subtracks
  (or right under an open stack's header) goes into that stack.
- **Resize** by dragging the header's bottom edge. Alt-drag resizes every track, and
  double-clicking the edge resets the track. Ctrl+Down and Ctrl+Up (or Alt+wheel, or View ›
  Taller / Shorter Tracks, or View › Track Height: Mini, Compact, Small, Medium, Large, Extra
  Large) change all tracks. Tracks go from 22 to 260 px; heights are remembered per project.
- **Short tracks:** from 52 px up the name sits above the buttons. Below that the header is a
  single line with a smaller icon (32 to 51 px), or without the icon and in a smaller size
  (below 32 px). The M, S, R, I and Freeze buttons then move into the **⋯** menu, which lists
  them with check marks (and their keys), plus the lane's parameter while automation is shown.
  A switch that is on stays in the header, so a muted, soloed, armed, monitored or frozen track
  still shows it; click it to turn it off. Regions on short tracks get a slimmer name bar over a
  thin strip of waveform or notes.
- **Volume:** drag the slider (Shift for fine), double-click or Alt-click it for 0 dB; its
  tooltip shows the level. In a group with volume linked, the other members follow. The meter
  inside the groove is post-fader and uses the slider's own scale, so a full-scale signal
  reaches the thumb, and only a hotter one passes it; peaks are held for 2 s, in red above
  0 dBFS. A folder stack's slider has no meter (it moves every subtrack).

### Inspector

The Inspector shows a **Region** section (mute, position, length, loop, and for audio regions
gain and the fade rows described under [Fades and crossfades](#fades-and-crossfades)) and a
**Track** section (type, input, instrument, region or subtrack
count, group), both collapsed by default, followed by the selected track's channel strip and
the Stereo Out strip.

### Library and Browser

- **Library (Y):** the plug-ins for the selected track. For an instrument track it lists
  instruments (Fermata Synth first, then plug-ins by vendor) and loads into the instrument
  slot; for other tracks it lists Audio FX and loads into the first free insert slot. Search
  with the field at the top; **Rescan** rescans plug-ins.
- **Browser (F)**, "All Files": browse folders (double-click to open, the up button for the
  enclosing folder). Drag audio files into the tracks area, or double-click one to import it at
  the playhead on the selected audio track.

### Status line

The project name ("Edited" when there are unsaved changes), the plug-in delay compensation
status, the audio devices, and the number of MIDI inputs.

### The menu bar

| Menu | Items |
|---|---|
| Fermata | About Fermata (shows the version), Settings…, Appearance…, Key Commands…, Quit Fermata |
| File | New…, Open Recent…, Open…, Save, Save As…, Project Settings…, Import Audio File…, Bounce Project or Section…, Export All Tracks as Audio Files…, Import MIDI File…, Export Selection as MIDI File… |
| Edit | Undo, Redo, Cut / Copy / Paste Regions, Duplicate, Repeat Regions, Split Regions at Playhead, Join Regions, Loop Regions, Convert Loops to Real Copies, Pack Take Folder, Set Locators by Regions, Select All, Delete |
| Track | New Tracks…, New Aux, Duplicate Track, New Track with Same Settings, Bounce in Place…, Freeze Track, Unfreeze Track, Create Folder Stack, Create Summing Stack, Group Selected Tracks, Hide Track, Unhide All Tracks, Delete Track, Delete Unused Tracks |
| Record | Play / Stop, Record, Cycle, Autopunch, Metronome, Count-in, Low Latency Mode |
| Navigate | Create Marker, Delete Marker, Go to Previous Marker, Go to Next Marker, Open Marker List |
| View | Library, Inspector, Mixer, Editors, Mixer in Separate Window, Editor in Separate Window, Browsers, Show Automation, Musical Typing, Show Hidden Tracks, Zoom In, Zoom Out, Larger Interface, Smaller Interface, Actual Size, Theme › |

When the Piano Roll has focus, the Edit menu's Cut, Copy, Paste, Duplicate, Delete and Select
All act on notes. When the Audio Track Editor has focus and a range selected, Cut, Copy, Delete
and Split act on the range.
While a bounce is rendering, menu commands are ignored.

### Separate windows

The Mixer, the Editors, the Channel EQ and the built-in effect panels can each be opened in a
window of their own, for example to put the Mixer on a second monitor.

- **Open in Separate Window:** click the pop-out button at the right end of the panel's bar
  (the Mixer's, the Piano Roll's or the Audio Track Editor's), or in an effect panel's or
  Channel EQ's title bar, docked or floating. View › Mixer in Separate Window and View › Editor
  in Separate Window do the same, and the Piano Roll's and Audio Track Editor's View menus have
  the item too.
- **Attach to Main Window:** the same button in the separate window (pop-in), the same menu
  items, or the window's close button. The Mixer and the Editors go back to the bottom
  section; an effect panel or Channel EQ opens in the main window as Settings › General › Open
  Effect Panels says (docked or floating).
- **One place at a time:** while a panel has its own window, it isn't shown in the main window.
  X (Mixer), E (Editors), an insert slot or a Channel EQ thumbnail bring its window to the
  front instead (with Open Effect Panels set to Separate windows, the slot or thumbnail closes
  it).
- **Effect panels in separate windows by default:** Settings › General › Open Effect Panels ›
  Separate windows opens every built-in effect panel and Channel EQ in its own window.
- **Everything stays live.** Meters, the playhead, the selection (the editor follows it), the
  theme, the interface size and the settings are the same in every window. Key commands and the
  menu bar work in a separate window as in the main one; the Edit menu acts on the separate
  window's editor while it is in front.
- **Effect panels opened from a separate Mixer** (clicking an insert slot there) float inside
  the Mixer's window.
- An effect panel's or Channel EQ's own × (or Esc) closes it altogether, and its window goes
  with it. So does removing the effect or deleting the track.
- **Remembered:** each kind of window (the Mixer, the Editors, the Channel EQ, each type of
  effect) reopens where it was last, with its size, on the same monitor if it is still
  connected (otherwise on the nearest one, moved and shrunk to fit). The Mixer and the Editors
  reopen in their own windows at the next launch if they were detached when Fermata quit.
  Separate windows close with the main window.

---

## 3. Recording

### Recording audio

1. Create an audio track (Track › New Tracks…, Ctrl+Alt+N, type **Audio**), or use the Record
   Audio template.
2. Choose the input in the channel strip's **Input** slot: the mono/stereo button and the input
   pop-up ("Input 1", "Input 1–2"...).
3. Record-enable the track with **R** in its header (or Ctrl+R, Control+R on macOS, for the
   selected track).
4. Press **R** (or Shift+R, or the Record button). Press R again, or Space, to stop.

- Recording while playing punches in on the fly.
- Recordings are WAV files in the project's `Audio Files/` folder, named after the track and
  take number ("Vocals#03.wav"; see Settings › Recording › Take Names). The format is 32-bit
  float by default, or 24-bit (Settings › Recording, or per project in Project Settings).
- Regions draw live while you record.
- A record-enabled audio track is silent while recording (you hear its input only if it is
  monitored), so you don't hear the old take over the new one.
- A frozen track can't be record-enabled.

**Overlapping recordings** (Settings › Recording): a new take over existing regions is put
**on top** (the older audio stays underneath and is hidden where the new one plays), on a **new
track** below with the same settings, or **replaces** what's underneath (the older regions are
trimmed; their files are kept).

A newer region always hides the older audio it overlaps during playback, as in Logic. The
older regions are not deleted; move or trim the newer one to hear them again. Regions that
crossfade are the exception: both play across the crossfade.

### Count-in and pre-roll

- **Count-in** (the "1234" button or Shift+K) plays the click for 1 to 4 bars before recording
  (Settings › Recording › Count-in). The count-in always clicks, even with the metronome off.
- With Count-in off, **Pre-roll** (Settings › Recording, in seconds, 0 by default) starts
  playback that far before the playhead so you hear a lead-in; recording still starts at the
  playhead.

### Cycle recording

With **Cycle** on, recording starts at the left locator (or carries on if you are already
playing inside the cycle) and loops. When you stop, the passes become **takes in a take
folder** ("Take 1", "Take 2"...). A last pass stopped part-way is kept if it got at least a
beat in. Recording the same cycle range again adds its passes to the same folder. A single pass
becomes a normal region. See [Take folders and comping](#7-take-folders-and-comping).

MIDI recorded in cycle mode goes into one region over the passes.

### Autopunch

Autopunch records only between the punch-in and punch-out points, so you can start playback
early and replace just a phrase.

- Turn it on with the Autopunch button next to Cycle (Ctrl+P on Linux and Windows,
  Control+⌘P on macOS). With no range yet, it takes the cycle range, or the two bars from the
  playhead.
- The range is the red strip in the ruler: drag in the strip to set it, drag it or its edges to
  move or resize it, click it to turn Autopunch off and on.
- While recording, armed tracks keep playing their regions before and after the range, and
  input is captured only inside it. The take is trimmed exactly to the range, after latency
  compensation. MIDI notes that start outside the range are dropped, and none lasts past the
  punch-out.
- With Cycle on as well, every pass is punched and the takes go into a take folder over the
  punch range.
- Recording refuses to start when the playhead is past the punch-out, or when the punch range
  is outside the cycle.

### Recording MIDI

Arm an instrument track and record as for audio. MIDI goes to the selected instrument track
(or, if the selected track isn't one, the first armed instrument track). Notes, controllers,
pitch bend and aftertouch are recorded into a region that spans whole bars. With Settings ›
Recording › Overlapping Recordings › MIDI set to **Merge**, the notes go into the region they
overlap instead of a new one. Notes still held when you stop end at the stop position.

### Latency compensation

Fermata measures the input and output latency of the devices while running and moves every
recording earlier by the round trip, so it lines up with what you heard:

- audio is moved by input latency + output latency + the audio waiting in Fermata's input
  buffer + the manual **Recording Delay** (Settings › Audio, -500 to 500 ms, positive moves
  recordings earlier) + the plug-in delay compensation total;
- MIDI is moved by the output latency plus the plug-in delay compensation total.

Settings › Audio shows the measured values and the total shift. If recordings still land
early or late (some drivers misreport latency), record a click or a clap through a cable and
adjust Recording Delay.

---

## 4. Software instruments and Musical Typing

- Create an instrument track (New Tracks › **Software Instrument**). Its **Instrument** slot
  holds Fermata Synth by default, a built-in polyphonic synthesizer that responds to pitch bend,
  sustain, modulation (vibrato) and expression. Choose a VST3 instrument in the New Tracks
  dialog, the Library, or the strip's Instrument slot.
- **MIDI input:** every connected MIDI input is routed to the selected instrument track (if
  the selected track isn't an instrument track, to the first armed one). Devices plugged in
  later are picked up within a few seconds. Settings › MIDI switches individual inputs off,
  turns MIDI Thru off (MIDI is then recorded but not played, for keyboards with their own
  sound) and sets a velocity curve.
- **VST3 instruments** receive controllers and pitch bend through their MIDI mapping.

### Musical Typing

**Musical Typing** (View › Musical Typing, Ctrl+K, or the keyboard button in the control bar)
plays the selected instrument track from the computer keyboard. Keys are mapped by their
position, so it works on any keyboard layout:

| Keys | Plays |
|---|---|
| A S D F G H J K L ; ' | White keys from C up (A = C) |
| W E T Y U O P | Black keys (W = C♯, E = D♯, T = F♯, Y = G♯, U = A♯, O = C♯, P = D♯) |
| Z / X | Octave down / up (starts at C3) |
| C / V | Velocity down / up by 10 (starts at 100) |
| Tab (hold) | Sustain pedal |

- The four-octave on-screen piano can be clicked or swept; clicking lower on a key plays
  louder. The **Sustain** button latches the pedal.
- While the window is open these keys play notes instead of running their usual commands.
  Combinations with Ctrl, Alt (and ⌘ on macOS) still work, and typing in text fields is not
  affected.
- Notes played this way are recorded like notes from a MIDI keyboard. The MIDI velocity curve
  doesn't apply to them.

---

## 5. Editing in the tracks area

### Selecting regions

- Click a region to select it; Shift+click adds or removes regions. Clicking empty lane space
  deselects. Ctrl+A (Edit › Select All) selects all regions.
- With a group whose Editing setting is on, clicking a region also selects the regions at the
  same place on the other members.
- Right-click a region › Select › All on Track, All Following on Track, or All.

- Drag across empty lane space to draw a rubber band: every region it touches is selected.
  Shift+drag adds to the selection. A click without dragging still deselects.
- Alt+drag (Option-drag on macOS) a selection to copy it: the originals stay put, the copies
  are drawn where they'll land and become the selection, and the copy is one undo step. Some
  Linux window managers use Alt+drag to move windows; if that happens, use Copy and Paste,
  Duplicate or Repeat, or change the window manager's modifier.

### Moving, trimming and snapping

The pointer changes over a region's hot zones:

| Where | Drag to |
|---|---|
| The body or the name bar | Move the region (and the rest of the selection). Drag up or down to move it to another track of the same kind. |
| Lower left or right edge | Trim the start or end. |
| Top left or right corner, below the name bar (audio) | Set the fade-in or fade-out length. |
| Top right corner of the name bar | Set where a loop stops (see Loops). |

- Moves, trims and loop ends follow the **Snap** setting (Smart, Bar, Beat or Off). Smart
  snaps to sixteenths, beats or bars depending on the zoom. Fades are never snapped.
- A moved region comes to the front, so it plays over whatever it now overlaps.
- **Drag mode** (the **Drag** pop-up in the tracks area's bar, next to Snap): **Overlap** (the
  default) leaves overlapping regions as they are; **X-Fade** crossfades audio regions that a
  move, trim, nudge, paste or Alt-drag copy leaves overlapping, over the overlap (type X), and
  removes such a crossfade when an edit pulls the two apart. The mode is saved with the
  project and isn't an undo step.
- **Nudge:** Alt+Left / Alt+Right move the selected regions by the Nudge Value (Settings ›
  Editing; 1/16 note by default). A run of nudges is one undo step.
- Regions on a frozen track can be selected but not changed.

### Split and join

- **Split at Playhead** (Ctrl+T, Edit menu or the region menu) splits the selected regions at
  the playhead (all regions under it when none are selected). Splitting a MIDI region keeps its
  controllers consistent: the right part starts with the values in effect at the cut.
- **Join** (Ctrl+J) joins the selected regions on each track into one. MIDI notes are merged;
  audio is mixed, as it plays (newer regions winning), into a new file.

### Loops

As in Logic, a looping region repeats its own content from its end up to its **loop end**:

- **Loop** (L, the region menu, or the Inspector's Loop box) loops the selected regions up to
  the next region on the track, or for four times the region's length when nothing follows.
  Press L again to turn it off.
- Point at the top right corner of the region (the pointer becomes a circular arrow) and drag
  to set where the loop stops; dragging back to the region's end removes the loop.
- Repetitions are drawn lighter, with a notch at each pass. Click a repetition to select the
  region and drag it to move the region (the loop moves along). Trimming changes what repeats
  and keeps the loop end.
- The fade-in shapes the start of the region and the fade-out the end of the loop. Loop
  repetitions play beneath other regions on the track, so a region over a loop always wins.
- **Convert Loops to Real Copies** turns the repetitions into regions. Split, Join, Repeat and
  Paste work on the whole looped length; splitting a loop converts it first.

### Fades and crossfades

- **Fades:** drag the handles at the top corners of a selected or hovered audio region (a
  looped region's fade-out is set from the menu or the Inspector), use the
  region menu's **Fade In** and **Fade Out** submenus (None, 10 ms, 50 ms, 250 ms, 1 Beat,
  1 Bar), or type lengths in the Inspector.
- **Inspector rows**, as in Logic's Region inspector:

  | Row | What it sets |
  |---|---|
  | **Fade In** | The fade-in length in ms. Click the label to choose **Fade In** or **Speed Up**. |
  | **Curve** | The fade-in's curve, -100 to 100. |
  | **Fade Out** | The fade-out length in ms. Click the label to choose **Fade Out** or **Slow Down**. |
  | **Type** | **Out** (a plain fade-out), or a crossfade with the following region: **X** (the two levels add up to unity), **EqP** (equal power, for material that isn't the same on both sides) or **X S** (S-shaped). |
  | **Curve** | The fade-out's curve, -100 to 100 (also shapes an X crossfade). |

- **Curves:** 0 is a straight line. Positive values bow outwards (a fade-in rises quickly,
  a fade-out holds its level longer), negative values inwards. The default, 50, is the
  quarter sine every fade used before curves could be set, so older projects sound the same.
- **Speed Up and Slow Down** change the playback speed over the fade, like a tape starting or
  stopping, following the curve; the level fades with it. The audio after a Speed Up and before
  a Slow Down plays where it always did. They are drawn with a dashed curve.
- **Crossfades:** a crossfade type on a region whose end meets the next region on the track
  (touching or overlapping) crossfades the two over the Fade Out length, centred on the
  junction (or the middle of the overlap). Choosing a crossfade type for a region without a
  fade-out gives it 20 ms (or the overlap). The next region's own fade-in isn't used meanwhile;
  its Fade In rows are dimmed. Followed by a gap, the type fades out like Out.
- Region menu › **Crossfade** (None, 10 ms, 20 ms, 50 ms, 250 ms, 1 Beat) crossfades each selected
  audio region with the one that follows it, as EqP unless it already has a crossfade type;
  None removes it. It is greyed out when no selected region is followed directly.
- A crossfade plays audio from the outgoing region's file past its end and from the incoming
  one's before its start. Where a file has none to spare, the crossfade slides and shortens to
  fit; where neither has any, the two regions fade out and in at the junction, over half the
  length each (a short dip rather than a click).
- Crossfades are drawn across the junction: both regions' curves, crossing. Drag the handle at
  the crossfade's start (on the outgoing region) to lengthen or shorten it about its centre.
- Playback, Bounce, Export, Bounce in Place, Freeze and Join all render fades and crossfades
  identically.
- **Anti-click fades:** Settings › Editing › Anti-click Fades gives new recordings and imports
  short fades (0 ms by default).

### Gain and reverse (audio regions)

- **Gain:** region menu › Gain › +6, +3, +1, -1, -3, -6 dB, Reset to 0 dB, or Normalize (the
  loudest sample reaches 0 dBFS); or the Inspector's Gain field. Gain ranges from -96 to +30 dB.
- **Reverse** writes a reversed copy of the audio to a new file, so the original stays intact.

### Colours, names and mute

- Region menu › **Color** sets a region colour (Track Color resets it). Region menu ›
  **Rename…** renames one region.
- **Mute** regions with Ctrl+M (Control+M on macOS) or the region menu. Muted regions don't
  play.

### Copy, paste, duplicate and repeat

- **Cut, Copy, Paste** (Ctrl+X, Ctrl+C, Ctrl+V): Paste places the copied regions at the
  playhead on the selected track, keeping their spacing and their relative tracks. Regions that
  don't fit the track they would land on (audio on an instrument track) are skipped.
- **Duplicate** (Ctrl+D): with regions selected (and the last click on a region), puts a copy
  straight after the selection; otherwise it duplicates the selected tracks.
- **Repeat** (Edit › Repeat or the region menu): copies the selection 1 to 16 times straight
  after itself.
- Right-click empty lane space › **Paste Here** pastes at that point.

### Other region commands

From the region menu:

- **Move to New Track:** moves the regions onto a new track with the same settings.
- **Set Locators by Regions** (U): sets the cycle range to the selection.
- **MIDI:** Quantize to 1/4, 1/8, 1/16 or 1/32 notes; Transpose by an octave or a semitone;
  Export as MIDI File….
- **Bounce in Place…** and **Export as Audio File…** (see [Bounce and export](#14-bounce-and-export)).
- **Delete** (Backspace or Delete).

Right-click **empty lane space** for Paste Here, Create MIDI Region (one bar, on instrument
tracks), Import MIDI File Here… / Import Audio File Here…, Select All on Track, Select All and
New Tracks….

### Importing audio and MIDI

- **Drag files** from the desktop file manager or the Browser:
  - onto the **track headers**: new tracks inserted at that point, starting at bar 1;
  - onto an **audio track's lane**: placed there at the snapped position;
  - onto another lane: a new track after it, at that position;
  - **below the tracks**: a new track at the end.
  - Several files become consecutive regions on a lane, or one track each on the headers.
- Accepted audio: WAV, AIFF, FLAC, MP3, OGG Vorbis, M4A/AAC/MP4 and CAF. A WAV file already
  at the project's sample rate is used where it is; anything else is converted (with
  high-quality resampling) into the project's `Audio Files/`.
- **File › Import Audio File…** (Ctrl+Shift+I) imports one WAV file at the playhead onto the
  selected audio track (or the first audio track). The lane menu's Import Audio File Here…
  accepts every supported format and several files.
- MIDI files (`.mid`, `.midi`, `.smf`, `.kar`) can be dropped the same way; see
  [MIDI files](#midi-files).

### Tracks

From the Track menu, the track header's right-click menu, or keys:

- **New Tracks…** (Ctrl+Alt+N): Audio or Software Instrument, the number of tracks (1 to 64),
  the input (with **Ascending**, each extra track takes the next input), the instrument, and
  whether the new tracks are record-enabled and monitored. New tracks go below the selected
  track.
- **New Aux** (see [Sends and aux buses](#sends-and-aux-buses)).
- **Duplicate Track** copies settings, plug-ins with their current settings, automation and
  regions. **New Track with Same Settings** copies everything except the regions and
  automation.
- **Rename Track**, **Track Color**, Mute, Solo, Record Enable, Select All Regions on Track,
  Export Track as MIDI File…, Reset Track Height.
- **Delete Track** (Ctrl+Backspace) and **Delete Unused Tracks** (tracks without regions;
  stacks that still hold tracks and auxes that something feeds are kept). Deleting a stack
  deletes its subtracks.
- **Select Previous / Next Track:** Up and Down arrows. **Mute / Solo the selected track:**
  M and S.

### Undo

Ctrl+Z and Ctrl+Shift+Z undo and redo every edit: regions, tracks, names, colours, volume,
pan, EQ, plug-in slots, sends, automation, recordings, imports, markers, take folders and
Project Settings. The Edit menus show what will be undone, and a toast confirms it. A run of
drags on one control counts as one step.

Not undoable, as in Logic: mute, solo, record enable, input monitoring, the transport, the
cycle and Autopunch ranges, and the metronome and count-in switches.

The history holds 100 steps by default (Settings › General › Undo Steps) and is cleared when
another project is opened or created.

---

## 6. Track stacks, groups, hiding and freezing

### Track stacks

Select tracks and choose **Create Folder Stack** (Ctrl+Shift+D) or **Create Summing Stack**
(Track menu, or the track menu › Create Track Stack):

- A **Folder Stack** groups tracks under a collapsible main track. Its fader moves every
  subtrack by the same amount, and its mute and solo apply to all of them. It has no audio of
  its own, so no plug-ins, sends or meter.
- A **Summing Stack** is also a bus: its subtracks play through its own Channel EQ, plug-ins,
  pan and fader. Soloing a subtrack keeps its stack audible. A subtrack's output can still be
  set to Stereo Out or an aux, to skip the stack.
- The disclosure triangle shows or hides the subtracks. A collapsed stack's lane shows where
  its subtracks have regions. Stacks can be nested.
- Drag a track under a stack's header, or between its subtracks, to put it in. **Remove from
  Stack** takes it out; **Flatten Stack** removes the stack and keeps its tracks.
- Auxes can't go into stacks.

### Groups

**Group Selected Tracks** (Ctrl+G, or the track menu › Group) makes a group and opens **Group
Settings**, which you can reopen by clicking a member's group badge:

- Name, colour, and **Group on** (off makes members act alone without dissolving the group,
  like Logic's group clutch).
- What the members share: **Volume** and **Pan** (as relative moves), **Mute**, **Solo**,
  **Record Enable**, **Track Selection** (clicking one selects all), and **Editing
  (Selection)** (selecting a region selects the regions at the same place on the others).
- **Delete Group** dissolves it. A group nobody is in is removed.

### Hiding tracks

- **Hide Track** (Ctrl+H, Control+H on macOS) hides the selected tracks and selects the next
  one. Hidden tracks keep playing, and hiding a stack hides its subtracks. The mixer leaves
  hidden tracks out too.
- **Show Hidden Tracks** (H, or the H button above the headers) shows them dimmed and puts an
  H button on every header for hiding and showing tracks one by one.
- **Unhide All Tracks** brings them all back.

### Freezing tracks

Freeze renders a track's instrument, Channel EQ and plug-ins to a file and plays that instead,
with the plug-ins unloaded, so a heavy track stops using CPU.

- Click the **snowflake** on an audio or instrument track header, or use Track › Freeze Track
  or the track menu. With several tracks selected it freezes them all. A progress toast shows
  the render.
- The render is pre-fader, so volume, pan, mute, sends and their automation stay live.
  Plug-in automation is baked in. It covers the track's first region to its last plus up to 4 s
  of tail.
- While frozen, the regions, plug-ins and Channel EQ can't be changed, and the track can't be
  record-enabled. Its regions are veiled, and edits are refused with a message.
- **Unfreeze** (the snowflake again, Track › Unfreeze Track, or the track menu) loads the
  plug-ins back with exactly their settings and deletes the freeze file.
- Freeze and Unfreeze can be undone. Frozen tracks stay frozen when the project is saved and
  reopened. A duplicate of a frozen track is live.
- Freeze files are in the project's `Freeze Files/` folder.

---

## 7. Take folders and comping

Take folders follow Logic's Quick Swipe Comping.

- **Making one.** Record with Cycle on (each pass becomes a take), or select several audio
  regions on one track and choose **Pack Take Folder** (Ctrl+F on Linux and Windows,
  Control+⌘F on macOS; also in the Edit and region menus): one take per region.
- **The comp.** A folder plays its *comp*: which take is heard where. A new folder starts with
  the newest take, older takes filling in where it has no audio. Where one take hands over to
  another they crossfade over 10 ms (equal power), so boundaries don't click.
- **Opening a folder.** Click the disclosure triangle at the start of the folder's name bar.
  The track grows by one row per take below its lane. The badge next to the triangle shows the
  take heard (or **C** for a comp).
- **Comping.**
  - Drag across a take row to put that stretch of the take into the comp (snapped to the grid).
    Comp parts are drawn bright and the rest dimmed; the closed folder draws the comp's
    waveform.
  - Click a take row to hear that take throughout.
- **The take folder menu** (click the badge, right-click a take row, or region menu › Take
  Folder): the takes (pick one to hear), Open / Close Take Folder, Rename Take, Delete Take,
  Delete Unused Takes, **Flatten** (the comp becomes ordinary regions, one per stretch),
  **Flatten and Merge** (the comp is rendered to one new audio file), and **Unpack to New
  Tracks** (every take on its own new, muted track, and the comp flattened in place).
- A folder moves, trims, splits, loops, fades and changes gain like any audio region. Every
  comp edit can be undone, folders are saved with the project, and bounces, exports and Bounce
  in Place render the comp.

---

## 8. Markers

The marker lane at the bottom of the ruler is Logic's Marker track.

- **Create Marker** (Alt+', or Navigate › Create Marker) adds a marker at the playhead,
  rounded to the nearest bar. Double-clicking empty marker lane creates one there.
- Each marker shows its name in its colour, with a faint band reaching the next marker.
- Click a marker to go to it; drag it to move it (snapped); double-click it to rename it.
- Right-click a marker: Go to Marker, Set Locators to Marker (the cycle covers its section),
  Rename…, Color, Delete, Marker List…. Right-click empty lane: Create Marker Here, Marker
  List….
- **Go to Previous / Next Marker:** Alt+, and Alt+. (Navigate menu).
- **Navigate › Delete Marker** removes the marker at or before the playhead.
- **Marker List** (Ctrl+Alt+K, Navigate › Open Marker List): every marker with its colour,
  position, an editable name, Go To and Delete, plus Create Marker.
- Markers are saved with the project, and every marker edit can be undone.

---

## 9. Tempo and time signature

A project has a base tempo (5 to 990 BPM) and time signature, set in the control bar's LCD or in
File › Project Settings › General. Changes along the timeline live in the **Global Tracks**.
Show them with View › Show Global Tracks (G); they sit between the ruler and the tracks.

**Tempo track**
- Click empty space to add a tempo change at the snapped position, starting at the tempo in
  effect there, and drag straight away to set it.
- Drag a point up or down to change its tempo (Shift for 0.01 BPM steps), or sideways to move
  it. A drag locks to whichever direction you move first.
- Double-click a point, or select it and press Delete, to remove it. The first point can't be
  removed.
- Right-click a point › **Ramp to Next Tempo** to glide smoothly to the next change instead of
  jumping. Ramps are exact: beats, the click and MIDI all follow the curve.
- Drag the lane's bottom edge to resize it.

**Signature track**
- Double-click empty space to add a time signature change at that bar, or double-click a label
  such as "7/8" to edit it. Changes always start on a bar line.
- Right-click a label to delete it.

**What follows the tempo map:** the ruler's bar numbers, the grid and Snap, quantize, nudge,
the metronome (including subdivisions and accents on each bar), count-in (it uses the tempo and
signature at the punch point), the bar/beat display, markers, cycle and punch defaults, the
Bounce dialog's bar range, the Piano Roll and the Audio Track Editor, and the tempo and position
reported to plug-ins.

**The LCD** shows the tempo and signature at the playhead, live while playing. Changing the tempo
there (drag, or double-click and type) edits the tempo change in effect at the playhead; the
signature menu edits the signature of the playhead's bar.

**Regions stay where they are in time.** Recorded audio and MIDI keep their position when the
tempo changes, so audio never drifts. To keep MIDI on its bars and beats instead, turn on
right-click › **Move MIDI Regions with Tempo** in the Tempo track before editing: tempo edits
from the Tempo track or the LCD then move and stretch MIDI regions, notes and controllers to
keep their musical positions. Audio, frozen tracks, automation and markers never move.

**MIDI files** carry their tempo and signature changes: importing into a project with no
regions adopts the file's tempo map; importing into a project that already has regions places
the notes using the file's own timing. Export writes the project's tempo map and signatures
(ramps become steps every sixteenth note, with exact durations).

---

## 10. The Piano Roll

The Editors pane (E or P) follows the selection: it shows the Piano Roll for a software
instrument track and the Audio Track Editor for an audio track. The Piano Roll is modelled on
Logic's, with ideas from Ableton's MIDI editor, Cubase's Key Editor and FL Studio's piano roll.

### View

- Every MIDI region on the track is shown in project time, with bars and beats. The region
  selected in the tracks area is in focus; the others are dimmed but still editable. The area
  outside regions is shaded.
- **Keyboard:** C3 is middle C (MIDI 60), as in Logic. Click a key to hear it (further right
  is louder; drag for a glissando) and to select every note of that pitch; Shift+click adds to
  the selection.
- **Ruler:** click or drag below the top strip to move the playhead (snapped to the grid when
  Snap is on). In the top strip, drag to create a cycle range, drag inside it to move it or near
  its ends to resize it; click it to switch Cycle on and off.
- **Zoom:** Ctrl+wheel or the horizontal slider zooms in time, Alt+wheel or the row slider
  changes the row height; Ctrl+Right / Ctrl+Left and Ctrl+Down / Ctrl+Up do the same from the
  keyboard. **Z** (View › Zoom to Fit Region) fits the focused region and its pitch range.
- **Scroll:** over the notes the wheel scrolls the pitches and Shift+wheel scrolls through time;
  with Settings › Editing › Mouse Wheel Scrolls set to Horizontally it's the other way round.
  Over the ruler and the controller lane the wheel always scrolls through time.
- **Catch Playhead** (button and View menu) pages along with the playhead.
- **View menu:** Color by Velocity (Logic's violet-to-red scale) or Color by Region; Highlight
  Scale (shades the rows of the project key's scale); Show Velocity / Controller Lane and Lane ›;
  Catch Playhead; **MIDI Out (Hear Edited Notes)**; the zoom commands.
- Note names appear on notes when they are large enough. The view settings are remembered.

### Tools

Choose a tool from the tool pop-up in the local bar, or press **T** and then the tool's
letter within two seconds (T T is the Pointer).

| Tool | Letter | In the note area |
|---|---|---|
| Pointer | T | Select, move, copy and resize notes; double-click empty space to create a note. |
| Pencil | P | Click empty space to create a note, drag right to set its length. On a note it moves or resizes like the Pointer. |
| Eraser | E | Deletes the note clicked and every note the drag passes over. |
| Velocity | V | Drag up or down on a note to change the velocity of the selection. |
| Mute | M | Click a note to mute or unmute it (the whole selection if it is selected); drag across notes to mute or unmute them all. |
| Scissors | I | Click a note to split it at the pointer (snapped; Alt for free). Splits the whole selection when the note is selected. |
| Glue | G | Click a note to join the selected notes of the same pitch into one. |
| Line | L | Draws lines in the velocity and controller lane; behaves as the Pointer in the note area. |

Ctrl+click with the Pointer (or Line) works as the Pencil, like Logic's secondary tool.

### Editing notes

- **Select:** click a note; Shift+click adds or removes it. Drag on empty space (with the Pointer,
  Velocity, Scissors or Glue tool) for a rubber band; Shift adds. Click empty space to deselect.
- **Move:** drag notes. The grabbed note snaps to the grid; hold Ctrl to move freely. Drag up
  or down to change pitch (you hear each new pitch with MIDI Out on). Hold **Alt** during the drag
  to copy instead.
- **Resize:** drag a note's right edge to change its length, or its left edge to move its start.
  Alt or Ctrl for no snapping.
- **Create:** with the Pencil, click (or drag to set the length); with the Pointer, double-click
  empty space. New notes are one grid step long and use the last velocity you set. A help tag
  shows the pitch, position or length while you drag.
- **Regions grow:** notes drawn or moved past a region's edges grow it to the next bar line. A
  note drawn on empty space within a bar of a region goes into that region; farther away, a new
  one-bar region is made.
- **Right-click** a note for Cut, Copy, Paste, Duplicate, Delete and the Functions menu.
- Each edit is one undo step; a run of arrow-key nudges or transposes merges into one.

### Grid and Time Quantize

- **Grid:** 1/1, 1/2, 1/4, 1/8, 1/16 (default), 1/32, 1/64 and the triplets 1/2T to 1/32T. It
  sets the snap and the Pencil's note length. Snapping counts from each bar line, so odd meters
  line up. The **Snap** button turns snapping off.
- **Time Quantize:** the Quantize pop-up in the local bar opens a panel with the value, **Strength**
  (0 to 100%, default 100), **Swing** (0 to 100%, default 0; it delays every second grid step) and
  **Quantize note ends** (off by default). **Q** (or the Q button, or Functions › Quantize)
  quantizes the selected notes, or all of them when none are selected. Functions › **Quantize
  to** › picks a value and quantizes at once.

### Functions menu

All functions act on the selected notes, or on every note when none are selected.

| Item | What it does |
|---|---|
| Quantize, Quantize to › | Time quantize (above). |
| Scale Quantize to *key* | Moves notes outside the project key's scale to the nearest scale note (natural minor for minor keys). |
| Transpose › | Octave Up, Semitone Up, Semitone Down, Octave Down, Other… (any number of semitones). |
| Velocity › | Set to 32, 64, 80, 100 or 127; Scale 50% to 150%; Set to…; Scale by…. |
| Humanize › | Light (±5 ms, ±5 velocity), Medium (±10), Strong (±20), Custom…. Lengths are not changed. |
| Note Length › | Legato (each note extends to the next one), Remove Overlaps (same-pitch overlaps), Set to Grid, Set to 1/1 to 1/32. |
| Mute / Unmute Notes | Ctrl+M. Muted notes stay in the region but don't play. |
| Delete Duplicates | Notes with the same pitch and start; the louder one stays. |
| Reverse | Plays the notes backwards within their span. |
| Split at Playhead | Splits the notes under the playhead. |
| Glue Same Pitches | Joins the selected notes of the same pitch. |

### Edit menu

Undo and Redo; Cut, Copy, **Paste at Playhead**, Duplicate (a copy straight after the
selection), Delete; Select All, Deselect All (Esc), Invert Selection, Select Same Pitch, Select
Highest Notes, Select Lowest Notes (of each chord), Select Muted Notes, Select Inside Locators;
Export Region as MIDI File…. The Piano Roll has its own note clipboard, separate from the region
clipboard.

### Velocity and controller lane

The lane below the notes shows velocity or a controller. Drag the bar above it to resize it
(40 to 420 px); choose what it shows with the lane button: **Velocity**, **Modulation (CC 1)**,
**Expression (CC 11)**, **Sustain (CC 64)**, **Pitch Bend**, **Aftertouch**, **Volume (CC 7)**,
**Pan (CC 10)**, any other controller already used, or **Other Controller…** (any number 0 to
127). Lanes that hold data are marked with a dot.

**Velocity:**

- Drag a note's stalk to set its velocity; the rest of the selection moves by the same amount.
- Drag on empty space (or use the Line tool) to ramp the velocities of the selection (or all
  notes) along a line.
- The Pencil draws velocities freehand.
- The **Vel** slider in the local bar sets the selection's velocity (it shows their average).

**Controllers, pitch bend and aftertouch:**

- Double-click to add a point (snapped; Alt for free).
- Click a point to select it (Shift adds); drag selected points to move them in time and value
  (Ctrl for no snapping). Drag on empty space for a rubber band.
- The Pencil draws freehand; Shift+Pencil or the Line tool draws a straight ramp.
- The Eraser, or Alt+click with the Pointer, removes points.
- Backspace or Delete removes the selected points (after clicking in the lane).

### Playback of MIDI regions

- Muted notes are skipped. Controllers play with the notes, and the values in effect are sent
  when playback starts or jumps, so a region played from the middle sounds as it does from its
  start.
- Stopping releases the sustain pedal and re-centres pitch bend.
- Split, join and trim keep controllers consistent: the later part of a split starts with the
  values in effect at the cut.
- The Piano Roll has no Step Input, Event List or Score yet, and controller points have no
  curve shapes.

---

## 11. The Audio Track Editor

With an audio track selected, the Editors pane (E) shows all of the track's regions on the
project timeline, drawn large, like Logic's Audio Track Editor, with the non-destructive range
tools of Logic's Audio File Editor.

### View and zoom

- A bars/beats ruler (click or drag it to move the playhead; its top strip shows the cycle), the
  playhead (it follows Catch), the selected region highlighted, and a dB scale beside the
  waveform.
- The waveform is drawn with the region's gain applied; red marks at its top and bottom show
  where the audio reaches 0 dBFS after that gain.
- **Zoom:** Ctrl+wheel or the slider zooms in time; the wheel scrolls along the timeline.
  Alt+wheel, the waveform slider (double-click to reset) or Ctrl+Down / Ctrl+Up zoom the
  waveform's amplitude (1× to 32×) to see quiet passages.
- **Z** zooms to the selection (or the region, or all the track's regions); press it again to
  go back. The editor keeps its own zoom, separate from the tracks area's.

### Selecting

- Drag across the waveform to select a time range. It follows the Snap setting (Smart snapping
  stops at sixteenths, and holding **Alt** while dragging is always free).
- Shift+click or Shift+drag extends the selection from its nearer end.
- Double-click a region to select all of it. A plain click selects the region under it.
- Esc deselects.

### Editing regions

Drag a region's name bar to move it, its lower edges to trim it, and the handles at the top of
the waveform to set fades, as in the tracks area (fade curves and crossfades are drawn the same
way, and a crossfade's handle works the same). The **Gain** field in the local bar sets the
selected region's gain: drag it (Shift for fine), scroll it (±0.5 dB), or double-click for 0 dB.

### Range actions

The Functions menu, right-click inside the selection, or keys. Each acts on every region under
the selection, as one undo step:

| Action | Key | What it does |
|---|---|---|
| Split at Selection | Ctrl+T | Splits at both ends (at the playhead without a selection). |
| Split at Transients | Ctrl+Shift+T | Splits at every transient in the selection (or the selected regions). |
| Trim Region to Selection | Alt+T | Keeps only the selection. |
| Silence Selection | Alt+Backspace | Splits and mutes the middle part (unmute it to bring it back). |
| Delete Selection | Backspace | Removes the middle part and leaves a gap. |
| Normalize Selection | Alt+N | Splits and sets the middle part's gain so its peak reaches 0 dBFS. |
| Reverse Selection | Alt+R | Splits and reverses the middle part (into a new file). |
| Fade In to Selection | Alt+I | Fades in over exactly the range, splitting the region where the range doesn't start at its edge. |
| Fade Out to Selection | Alt+O | The same at the end. |

- **Copy** and **Cut** (Ctrl+C, Ctrl+X) put the selected audio on the clipboard as a region
  (Cut also deletes it); **Paste at Playhead** (Ctrl+V) pastes it.
- **Play Selection** (Shift+Space) plays from the selection's start and stops at its end. With
  Cycle on, it sets the cycle to the selection and loops it. **Set Locators by Selection** only
  sets the cycle.
- Looped regions are converted to real copies first. Only Reverse writes a new file; the
  original stays intact.

### Transients

The transient button (or View › Show Transients) marks the onsets found in the audio. View ›
**Transient Sensitivity** sets how readily they are found: Low, Medium (default), High or Very
High. **Split at Transients** chops the selection (or the selected regions) at every marker in
one step, for slicing drums. Onsets are found from the audio itself: a fast peak level jumping
well above a slower average, moved back to the start of the attack.

### Info

The bar under the menus shows the region's name, start, length (in bars and as time), gain,
and its file's sample rate, channels and name, plus the selection's range.

The editor's own keys apply after you click in it; elsewhere the usual ones do. See the
[key reference](#audio-track-editor-keys).

---

## 12. Mixing

Open the Mixer with X. It shows a channel strip for every track (stacks included), then the
auxes, then Stereo Out. The **Audio**, **Inst** and **Aux** buttons filter the strips. Hidden
tracks appear only in hide mode. The Inspector shows the selected track's strip and Stereo
Out.

### The channel strip

From top to bottom:

| Part | What it does |
|---|---|
| **Input** | Audio tracks: the mono/stereo button and the input ("No Input", "Input 1"..., "Input 1–2"...). Instrument tracks: the **instrument slot** (No Plug-in, Fermata Synth, or a VST3 instrument by category or vendor). Auxes show their bus ("Bus 1"); summing stacks show "Subtracks". |
| **EQ thumbnail** | The Channel EQ curve; click to open the editor. |
| **Audio FX** | Insert slots: the filled ones plus one empty slot (up to 8). The label shows the strip's plug-in latency. |
| **Sends** | Up to 8 send slots. |
| **Output** | Stereo Out, the summing stack, or an aux. |
| **Automation mode** | Off, Read, Touch, Latch, Write (see [Automation](#13-automation)). |
| **Pan** | Drag vertically (Shift for fine); double-click to centre. |
| **Readouts** | Volume in dB and the peak level (red above 0 dB; click to reset). |
| **Fader and meter** | Drag the fader (Shift for fine), double-click for 0 dB. -96 to +6 dB. The meter holds peaks for 2 s. |
| **M S R I** | Mute, Solo, Record Enable, Input Monitoring. |
| **Name** | With the track colour. |

A folder stack's strip shows only its fader and buttons ("A folder has no audio of its own").
Stereo Out has Channel EQ, Audio FX, its fader and meter.

Mono audio tracks pan with a -3 dB compensated pan law (unity in the centre); stereo tracks
and buses use balance.

### Channel EQ

Every strip, Stereo Out included, has a built-in eight-band EQ modelled on Logic's Channel EQ.
It processes before the Audio FX inserts and costs nothing while flat.

- Click the thumbnail above the Audio FX slots to open the editor. It opens like a built-in
  effect's panel (see [Built-in effects](#built-in-effects)): floating by default, or docked
  in the bottom section, and it can be moved between the two and resized. Click the thumbnail
  again, press Esc or click × to close it. One strip's Channel EQ is open at a time.
- Bands: **Low Cut** (off by default, 30 Hz), **Low Shelf** (80 Hz), **Bell 1–4** (160, 500,
  1500, 5000 Hz), **High Shelf** (10 kHz) and **High Cut** (off by default, 18 kHz). The cuts
  have 12, 24 (default) or 48 dB/oct slopes; click the slope to change it.
- **On the graph:** drag a point to set frequency and gain (Shift for fine; the cuts move only in
  frequency), scroll to change Q, double-click a point to switch its band off or on, Alt-click
  to reset its gain.
- **Below the graph:** each band's on/off switch, frequency, gain and Q can be dragged (Shift
  for fine) and double-clicked to reset.
- **Toolbar:** On / Bypassed, **Analyzer** (a live post-EQ spectrum, on by default), **Reset**,
  the **preset** pop-up (see [Presets](#presets)), **Copy** and **Paste**, and **Output** gain
  (±24 dB).
- **Copy and paste:** right-click the thumbnail for **Copy Channel EQ** and **Paste Channel EQ**
  (with several tracks selected, also **Paste Channel EQ to Selected Tracks**). Pasting replaces
  the strip's EQ settings, Stereo Out included, as one undo step. The copy survives opening
  another project.

### Plug-ins

- **Loading:** click an empty Audio FX slot, or the arrow on a filled one, for a menu of plug-ins
  by category and vendor; or click a plug-in in the Library. The instrument slot works the same
  way.
- **Editing:** click the plug-in's name in its slot to open its editor window.
- **Bypass:** the power button on the slot.
- **Reordering:** drag a plug-in's name onto another Audio FX slot. On the same strip it moves
  there and the plug-ins in between shift over; it keeps running, so nothing reloads and a
  reverb tail carries on. Dropped on another strip's slot, it moves to that strip (whatever is
  in that slot shifts down) as a new instance with the same settings, and its automation lanes
  go with it (see [Automation](#13-automation)). Hold Alt as you let go to copy it instead.
  Esc cancels the drag; each move is one undo step, plug-in and lanes together.
- **Removing:** "No Plug-in" in the slot's menu.
- **Copy and paste:** right-click a slot for **Copy Plug-in**, and any slot (empty ones too) for
  **Paste Plug-in**, which loads a new instance with exactly the copied settings: a VST3's whole
  state, a built-in effect's values and side-chain (when its source exists here). With several
  tracks selected, **Paste to Selected Tracks** does them all. Each paste is one undo step, and
  the copy survives opening another project. Instruments paste only into instrument slots,
  effects only into Audio FX slots.
- Plug-in settings are saved in the project and restored on undo.
- Settings › Plug-ins (the Plug-in Manager) lists every scanned plug-in and lets you switch
  some off, so they don't appear in the Library and slot menus (projects that use them still
  load them). It also takes extra VST3 folders and shows the last scan's report, including each
  bundle that failed and why.

Fermata hosts **VST3** plug-ins only. Plug-in editor windows are available on Linux (X11;
see [Troubleshooting](#plug-in-windows-dont-open-on-wayland)) and macOS, where they float
above Fermata's windows like Logic's and hide while another app is in front.

### Built-in effects

Fermata's own effects load in any Audio FX slot like a plug-in: **Compressor**, **Limiter**,
**Noise Gate** and **De-Esser** (Dynamics), **Reverb**, **Delay**, and **Tuner** and **Loudness
Meter** (Metering). They are listed first, by category, in the slot menu and the Library.

- **The panel:** click the effect's name in its slot to open its panel; click again, press Esc
  or click × to close it. There is one panel per slot. The panel's header has the power (bypass)
  button, the effect and track names, the preset pop-up and, for effects with a side-chain, the
  **Side Chain** pop-up.
  - **Docked** (see [General](#general) settings): panels open in the bottom
    section, where the Mixer and the Editors show, with a tab for each open panel. Drag the
    divider above to make the section taller; graphs and meters grow with it. Showing the Mixer
    or an Editor hides the panels but keeps them open: click a slot again to bring them back.
    Closing the last tab gives the section back to what it showed before.
  - **Floating** (the default): the float button at the right of the dock's bar takes a panel out into a
    window of its own (and the dock button in a window's header puts it back). Drag a window by
    its title; drag its sides or bottom corners to resize it. A window can't be made smaller
    than its controls; Fermata remembers the last size for each kind of effect.
  - **Separate windows:** each panel opens in an OS window of its own, which you can move to
    another monitor (see [Separate windows](#separate-windows)). Clicking the slot again closes
    it; the pop-in button in its bar puts it in the main window, floating.
  - **Esc** closes the frontmost floating panel first, then the docked one on screen.
- **Knobs:** drag up and down (Shift for fine); double-click or Alt-click for the default;
  double-click the value under a knob to type one ("2.5k", "-6 dB", "1.2 s"). Every change is
  undoable, and a drag is one undo step.
- **Meters** show the effect's levels; gain reduction meters hang down from the top.
- **Automation:** every parameter appears in the track's automation lane menu under the effect's
  name, and Touch, Latch and Write record moves of the panel's knobs. While automation plays,
  the panel follows it.
- **Latency:** an effect with lookahead reports it to [delay compensation](#plug-in-delay-compensation),
  so the track stays in time; the slot's tooltip shows it.
- **Freeze** renders built-in effects like any plug-in; their settings come back on unfreeze.
- **Side Chain:** pick a track in the Side Chain pop-up and its post-fader output, instead of
  the effect's own input, drives the detector: a kick opening a gate on a bass, a vocal ducking
  a pad. Pick **None** to go back. The source plays before the effect's track, loops are refused,
  and muting the source (or pulling its fader down) silences the key, as in Logic. Bounces and
  freezes of the effect's track play the source along, unheard, so the key is right. The key is
  taken as the source leaves its plug-ins; if the source's own plug-ins add latency, the key
  arrives that much late. The Compressor, Noise Gate and De-Esser have a side-chain.
- **Copy and paste** and **dragging between slots** work as for any plug-in (see
  [Plug-ins](#plug-ins)).

#### Compressor

One set of controls in front of five **circuit** types, after Logic's Compressor:

- **Platinum Digital:** clean and exact; Threshold, Ratio, Knee, Attack and Release mean
  exactly what they say.
- **Studio VCA** (SSL bus, dbx 160 style): stepped attack and release, and **Auto Release**,
  which recovers fast after transients and slowly after sustained compression.
- **Studio FET** (1176 style): a feedback design with a very fast attack; the ratio buttons are
  4, 8, 12 and 20:1 plus **All** ("all buttons in"), which over-compresses loud peaks. Adds
  odd-harmonic colour as it works harder.
- **Classic Opto** (LA-2A style): a slow optical attack and a two-stage release that depends on
  how hard and long it was driven. The threshold shows as **Peak Reduction**; **Compress** and
  **Limit** replace the ratio.
- **Vintage Vari-Mu** (Fairchild style): a very wide knee whose ratio rises with level, six
  **Time Constant** positions (5 and 6 are program-dependent) and even-harmonic tube colour.

Around every circuit: **Make Up** gain and **Auto Gain** (Off, 0 dB, -12 dB, -24 dB),
**Detection** (Peak or RMS), **Stereo Link**, **Lookahead** (up to 20 ms, reported to delay
compensation), **Distortion** (Off, Soft, Hard, Clip), **Mix** (dry/wet, for parallel
compression) and **Output**. The **Side Chain Filter** (Off, HP, LP, BP, with a frequency)
shapes what the detector hears, and **Listen** plays it. The panel shows input, output and gain
reduction meters and the transfer curve. There are 14 factory presets, from Vocal Leveler
(Opto) to Mix Bus Glue (VCA) and Side-chain Duck.

#### Limiter

A lookahead brickwall limiter, after Logic's Limiter and Adaptive Limiter: nothing passes the
**Out Ceiling** (-20 to 0 dB, default -0.1 dB).

- **Gain** (0 to 24 dB) drives the signal into the limiter.
- **Release** (1 ms to 1 s) and **Lookahead** (0 to 20 ms, default 2 ms). The lookahead is
  reported to delay compensation.
- **Mode:** **Precise** (transparent), **Smooth** (a soft knee from 6 dB under the ceiling and a
  program-dependent release) or **Adaptive** (rounds peaks with a soft saturator first, for
  loudness).
- **True Peak** (on by default) limits the 4× oversampled peak, so the ceiling holds between
  samples too.
- Meters: Input, Output and Gain Reduction. Factory presets include Master -1 dBTP Streaming
  and Master -0.1 dB CD.

#### Noise Gate

Turns the signal down by **Reduction** whenever it falls under **Threshold**, and back up when
it rises above it.

- **Attack** and **Release:** how long opening and closing take. The gain always glides, so the
  gate never clicks.
- **Hold:** how long the gate stays open after the level falls.
- **Hysteresis:** how far under the threshold the level must fall before the gate closes, so a
  signal hovering at the threshold doesn't make it chatter.
- **Lookahead** (up to 20 ms): the gate sees the signal coming and is already open when a hit
  arrives. It adds that much latency, which delay compensation makes up.
- **Side Chain Filter:** **High Cut** and **Low Cut** filter what the detector hears (not the
  audio), to key off a kick and not the hi-hat. **Monitor** plays what the detector hears.
- Meters: Input, Key (with the threshold marked) and Gain reduction, plus an Open / Closed light.
- Factory presets: Tight Kick, Snare, Tom Rumble Killer, Vocal Breath, Guitar Amp Hiss and
  Gentle Cleanup.

#### De-Esser

Turns sibilance down when the band above **Frequency** gets louder than **Threshold**, by up to
**Range**.

- **Mode:** **Split** reduces only the band above Frequency; **Wide** turns the whole signal
  down, which keeps the voice's tone but ducks it more audibly.
- **Smoothing:** how fast the reduction lets go.
- **Listen to Detector** plays the band the detector hears, to find the "s".
- The split is a Linkwitz-Riley crossover, so the de-esser is transparent while it isn't
  reducing.
- Meters: Sibilance (the detected level, with the threshold marked) and Reduction.
- Factory presets: Female Vocal, Male Vocal, Gentle, Aggressive, Wide and Smooth, Hi-Hat Tamer
  and Acoustic Guitar Squeak.

#### Reverb

An algorithmic reverb in the spirit of Logic's ChromaVerb and SilverVerb.

- **Room Type:** Plate, Room, Hall (default), Chamber, Cathedral or Ambience. Changing it fades
  out the old tail.
- **Pre-delay** (0 to 500 ms, or a note value with **Sync**), **Size**, **Decay** (0.1 to 20 s),
  **Density**, **Diffusion**, **Damping** (high-frequency decay) and **Low Decay** (low
  frequencies decay 0.5× to 2× as long).
- **Early/Late** balances the early reflections against the tail; **Mod Rate** and **Mod
  Depth** move the tail gently against metallic ringing; **Width**, **Low Cut** and **High
  Cut** shape the result.
- **Freeze** holds the current tail indefinitely and stops new input.
- **Dry** and **Wet** levels (Dry 100 %, Wet 30 % by default; turn Dry down on an aux).
- 14 factory presets, from Vocal Plate and Drum Room to Concert Hall and Frozen Pad.

#### Delay

Logic's Stereo Delay and Tape Delay in one effect.

- **Mode:** **Stereo** (separate left and right times and feedback, with **Cross-feed**),
  **Ping-Pong** (repeats bounce left and right), **Tape** (wow, flutter, saturation and a
  darkening loop; feedback can pass 100 % and self-oscillate) or **Digital** (clean, one time).
- **Sync** (on by default) sets the times as note values (1/1 to 1/32, dotted and triplet),
  following tempo changes; off, **Time L** and **Time R** are 1 ms to 4 s. **Link** makes the
  right side follow the left.
- **Feedback L/R**, **Low Cut** and **High Cut** in the loop, **Mod Rate** and **Mod Depth**
  (chorus on the repeats), and in Tape mode **Wow**, **Flutter**, **Saturation** and **Smooth**
  (how slowly a time change glides, like a varispeed tape).
- **Freeze** loops what's in the delay; **Width**, **Dry** and **Wet** set the output.
- Changing the time never clicks: the clean modes crossfade, Tape glides.
- 11 factory presets, including Quarter Note, Dotted Eighth, Slapback, Tape Echo and Dub
  Delay.

#### Tuner

A chromatic tuner for guitar, bass and voice. The audio passes through untouched.

- The panel shows the note, how many cents off it is (a needle, or a strobe with **Display**)
  and the frequency.
- **Reference** (400 to 480 Hz, default 440), **Notation** (sharps or flats), **Transpose**
  (±24 semitones, for transposing instruments) and **Threshold** (the level below which it
  stops detecting).
- **Mute** silences the track's output while you tune.
- The level it gates on is measured after a DC blocker, so an interface's DC offset alone
  doesn't make it detect anything.
- Factory presets: Guitar (A440), Bass, Baroque A415, Orchestra A442, B♭ Instrument,
  E♭ Instrument, Mute While Tuning and Strobe.

#### Loudness Meter

EBU R128 / ITU-R BS.1770 loudness, after Logic's Loudness Meter. The audio passes through
untouched; put it last on Stereo Out to measure the mix.

- **Momentary** (400 ms), **Short-term** (3 s) and **Integrated** loudness in LUFS, **Loudness
  Range** (LU) and **True Peak** (dBTP), plus a scrolling short-term history.
- **Target:** -23 LUFS (EBU R128), -16 (Apple Music), -14 (Spotify, YouTube), -24 (ATSC A/85) or
  a custom value; **Scale** is EBU +9 or EBU +18.
- Integrated loudness and range are measured only while the transport plays. **Reset** clears
  them; **Auto-Reset on Play** (on by default) clears them each time playback starts.
- Factory presets for each delivery target, plus Podcast (-16) and CD Master (-9).

#### Presets

Every built-in effect has a preset pop-up in its panel's header, and the Channel EQ has one in
its toolbar. It shows the preset loaded last, with "(edited)" once something changed ("Default" or "Manual" before
any preset). The arrows step to the previous and next preset.

- **Factory** presets come with Fermata (the Channel EQ has Flat, Low Cut 80 Hz, Vocal
  Presence, Vocal Air, Kick Punch, Snare Crack, Bass Clarity, Acoustic Guitar, Telephone and
  Master Polish). **User**
  presets are yours, and every project sees them: they are files in the `presets/<effect>/`
  folder of the settings folder (`~/.config/fermata/presets` on Linux and macOS,
  `%APPDATA%\fermata\presets` on Windows), one `<name>.json` each.
- **Save Preset…** names and saves the current settings (or updates the user preset that is
  loaded); **Save Preset As…** saves a copy under a new name. Saving over an existing name asks
  first. **Rename Preset…** and **Delete Preset** work on the user preset that is loaded.
- **Revert** goes back to the loaded preset; **Default** (Flat for the EQ) resets everything.
- Loading a preset is one undo step.
- VST3 plug-ins use their own preset browsers; Fermata saves their state with the project and
  copies it with Copy Plug-in, but has no preset files for them.

### Sends and aux buses

An **aux** is a channel strip with no regions of its own: EQ, Audio FX, sends, pan, fader,
mute/solo and an output. It plays whatever its bus receives.

- Create one with Track › New Aux, the track menu, or **New Aux** in any send or output menu.
  New auxes are named "Aux N", are fed by "Bus N", and go at the end of the track list.
- **Sends:** every strip except folders and Stereo Out has eight send slots under Audio FX.
  Click an empty slot to pick an aux ("Bus 1 → Aux 1") or make a new one. A filled slot shows
  the bus (or the aux's name) and a level knob: drag it (Shift for fine), double-click for
  0 dB. Click the name for **Pre Fader / Post Fader**, **Mute Send**, **Send to**, and **Remove
  Send**.
- Post-fader sends follow the fader and pan. Pre-fader sends tap the signal after the plug-ins
  and keep sending with the fader down. Muting or soloing-out a track silences both kinds.
- **Output:** routes a strip to Stereo Out, its summing stack, or an aux.
- Soloing a track keeps the auxes it feeds audible, and soloing an aux keeps the tracks feeding
  it audible. Muting an aux silences everything arriving there.
- Feedback loops are refused with a message (sends that would make one are greyed out).
  Deleting an aux clears the sends and outputs that pointed at it (undoable).

### Plug-in delay compensation

Plug-ins that look ahead (limiters, linear-phase EQs, some amp models) output late. Fermata
compensates every path, as Logic's default "All" mode does, so tracks, summing stacks, auxes
and sends line up at Stereo Out.

- A bus waits for the latest of its inputs, and every other path into it is delayed to match.
  The whole mix plays that much behind the playhead, like extra output latency.
- Sends are compensated too, pre- and post-fader.
- Bypassed plug-ins still count, so switching a plug-in off doesn't shift the rest of the mix.
- Automation, the metronome and recordings follow the compensation.
- The Audio FX label of a strip shows its plug-in latency (each slot's tooltip has its own),
  and the status line shows the total ("PDC 10.0 ms").
- Settings › Audio › **Plug-in Delay Compensation**: All (default) or Off.

**Low Latency Mode** (Settings › Audio, or Record › Low Latency Mode): while you play live,
plug-ins with more latency than the limit (5 ms by default) are switched off on record-enabled
tracks and on monitored audio tracks, and those tracks skip their compensation delay. An armed
audio track that is recording always skips its own compensation delay. Bounces and exports
ignore Low Latency Mode.

---

## 13. Automation

Volume, pan, mute, send levels and any plug-in parameter can be automated per track, as in
Logic. Folder stacks and Stereo Out have no automation of their own.

### Showing and editing automation

**Show Automation** (A, or either View menu) draws each track's automation over its dimmed
regions. Region editing is off while it is shown.

- The track header gets a **parameter pop-up** (Volume, Pan, Mute, each used send as
  "Send N → bus", and a submenu per plug-in with its parameters; lanes holding points are marked
  ●) and the **mode pop-up**. On a one-line (short) header the mode pop-up stays and the
  parameters are in the **⋯** menu. The pop-up also has Delete *parameter* Automation and Delete All
  Automation on Track.
- **Add a point:** click empty lane space (snapped in time; the value is where you click).
- **Select:** click a point (Shift adds or removes); drag on empty lane space for a rubber band.
- **Move:** drag selected points in time and value. Hold Shift while dragging to change only the
  value.
- **Delete:** double-click or Alt+click a point, or press Backspace or Delete for the selected
  points. Right-click for Delete Selected Points, Delete *parameter* Automation, Delete All
  Automation on Track and Hide Automation.
- Hovering shows the value. Volume is drawn on the fader's scale. Lanes are straight lines
  between points, holding the first value before them and the last one after.
- A lane is drawn faded when the track's mode is Off.

### Automation modes

Set the mode in the track header (with automation shown) or above the pan knob on every channel
strip.

| Mode | Behaviour |
|---|---|
| **Off** | Lanes are ignored. |
| **Read** (default, green) | Lanes play. The fader, pan knob and plug-in editors follow while playing. |
| **Touch** | Plays the lanes, and writes while you hold a control during playback. When you let go, the parameter glides back to the existing curve (over 0.2 s). |
| **Latch** | Like Touch, but once you have touched a control it keeps writing, holding the last value, until playback stops. |
| **Write** | From where playback started, overwrites volume, pan and every existing lane (except send lanes) with the current values. After you stop, the track switches to Latch, as Logic does by default. |

- Moves in a plug-in's own editor are written too (through the plug-in's begin/perform/end edit
  calls).
- Group members that share volume, pan or mute write together.
- Each pass is one undo step, **Write Automation**. Written passes are thinned to the points the
  curve needs.
- Lanes are evaluated every audio block. Volume keeps the fader's smoothing, and plug-in
  parameters are ramped across each block. With plug-in delay compensation, automation follows the
  delayed audio.
- Lanes don't move with regions.
- **Lanes follow their plug-in.** Moving a plug-in within its strip keeps its lanes; dragging it
  to another track's strip moves its lanes to that track, with their points, so they keep
  driving the same parameters there (the track showing one of them shows it on the new track).
  They play in the new track's automation mode. A copy (Alt-drag, or Copy and Paste Plug-in)
  starts without automation, and the original keeps its lanes. Stereo Out has no automation,
  so a plug-in moved there leaves its lanes on the old track, where they no longer drive
  anything (as when a plug-in is deleted).

---

## 14. Bounce and export

Everything renders **offline**: the real mix runs faster than real time, so plug-ins, stacks
and channel strips sound exactly as they play. Playback is silent until the render finishes.
Plug-ins see the transport playing from the start of the range, and every instrument gets its
note-offs at the end. Delay compensation is removed from the start, so files line up with the
timeline.

### Bounce Project or Section

**File › Bounce Project or Section…** (Ctrl+B) writes the full mix through Stereo Out.

| Option | Choices |
|---|---|
| Range | Whole Project (bar 1 to the end of the last region), Cycle Range (the default when Cycle is on), or Bars from–to. |
| Audio Tail | 0 to 30 s (2 s by default), for reverbs and releases. Silence at the end is trimmed. |
| File Format | WAVE. |
| Resolution | 16-bit (with TPDF dither), 24-bit (default), 32-bit float. |
| Sample Rate | The project's (default), 44.1, 48, 88.2 or 96 kHz (converted with the high-quality resampler). |
| Normalize | Puts the peak at -0.1 dBFS (off by default). |

The file name defaults to the project's, next to the project folder. The options are
remembered.

### Export All Tracks as Audio Files

**File › Export All Tracks as Audio Files…** (Ctrl+Shift+E) writes one file per track into a
folder, named `<number> <track name>.wav` after the track's place in the list.

- Each audio and instrument track plays alone through its own channel strip, fader and pan
  included and Stereo Out left out, so the files add up to the mix.
- A track's own mute and the project's solos are ignored.
- A summing stack is also exported as a whole, through its own strip, next to its subtracks.
  Folder stacks have no strip and get no file.
- Tracks without (unmuted) regions are skipped.
- Every file covers the same range plus the full tail, so they line up when imported elsewhere.

### Bounce in Place

**Track › Bounce in Place…** (Ctrl+Alt+B on Linux and Windows, Control+B on macOS), also in
the track and region menus:

- Renders the selected tracks through their channel strips, over the span of their regions,
  into the project's `Audio Files/`. From the region menu (or when regions were the last thing
  you selected) it renders just the selected regions.
- Each result goes onto a new stereo audio track below the source, with the same colour and in
  the same stack, named "<name> (bounced)".
- Options: **Audio FX** (Channel EQ and plug-ins, on by default); **Volume/Pan** (off by
  default: the new track takes over the source's fader and pan instead); **Audio Tail** (up to
  2 s); **Source**: Leave, Mute (default; the track, or the regions when bouncing regions) or
  Delete.
- Instrument tracks bounce their MIDI to audio. A summing stack bounces its output, subtracks
  included. A folder stack bounces the mix of its subtracks, with its fader printed.
- It is all one undo step, plug-ins included when the source is deleted.

### Export Region as Audio File

**Export as Audio File…** in the region menu renders one region alone, muted or not, through its
track's Channel EQ and plug-ins, before the fader. It uses the same format options as Bounce.

### MIDI files

- **File › Import MIDI File…** puts a Standard MIDI File on the selected instrument track at the
  playhead's bar, and further parts of the file on new instrument tracks. You can also drop MIDI
  files on an instrument lane, the track headers or the empty area below the tracks, or
  right-click an instrument lane › Import MIDI File Here…. An empty project takes the file's
  tempo and time signature.
- **Export:** a region (region menu › Export as MIDI File…), a whole track (track menu ›
  Export Track as MIDI File…), or the selection (File › Export Selection as MIDI File…).

---

## 15. Projects

A project is a folder ending in `.fermata`, holding `project.json` and an `Audio Files/` folder
(and `Freeze Files/` once a track is frozen).

### The Project Chooser

Opens at launch (by default) and from **File › New…** (Ctrl+N) and **File › Open Recent…**
(Ctrl+Shift+O).

- **New Project:** four templates:
  - **Empty Project**;
  - **Record Audio**: one audio track on input 1, record-enabled;
  - **Software Instrument**: one instrument track with Fermata Synth, record-enabled;
  - **Songwriter**: Vocals and Guitar audio tracks on inputs 1 and 2, plus a Keys instrument
    track, all record-enabled.

  Set the name, location (default `~/Music/Fermata`, or Settings › General › Project Folder),
  tempo (20 to 400 BPM), time signature and key. Double-click a template to create it at once.
- **Recent:** the last 20 projects you saved or opened, with track count, tempo and how long
  ago each was saved. Missing projects are marked and can be removed from the list (×).
- **Open…:** choose any `.fermata` folder.
- **Show at launch:** untick it to start straight into an empty project.

### Open, save and close

- **File › Open…** (Ctrl+O): choose a `.fermata` folder. **Save** (Ctrl+S) and **Save As…**
  (Ctrl+Shift+S). Save As to a new folder copies the audio files that were in the old project
  folder.
- You're asked before unsaved changes are discarded (when quitting, opening or creating
  another project), unless you turn that off in Settings › General. **Don't Save** also
  deletes the autosave.
- A project whose sample rate differs from the audio device switches the device to the
  project's rate when opened.
- You can open a project from the command line: `fermata path/to/Song.fermata`.

### Autosave and recovery

- While a project has unsaved changes, Fermata writes `project.autosave.json` into its folder
  every 2 minutes (Settings › General › Autosave: Off, or every 1, 2, 5, 10 or 15 minutes). It
  isn't written while recording. Saving removes it.
- **Every take is autosaved the moment you stop recording**, as is the result of Bounce in
  Place, Freeze and importing audio files (Settings › General › After Recording, on by default;
  it works with the interval autosave off too). This writes the autosave, not your project:
  `project.json` only changes when you save, so **Don't Save** still returns to the last saved
  version. Starting to record also autosaves unsaved changes, so new tracks are recoverable along
  with their takes.
- **Crashes while recording:** while you record, Fermata keeps each take's file valid on disk
  every 2 seconds and keeps a small journal (`recording-….journal.json`) in the project folder.
  If Fermata crashes, is killed or the computer loses power mid-take, the audio recorded up to
  that moment is recovered: the file is repaired and the take goes back on its track where it
  was recorded (cycle recordings as a take folder). If the track itself was never saved, it is
  recreated. The journal goes away once the take is in the autosave or the project is saved.
- At launch, Fermata looks in the recent projects and the projects folder for autosaves newer
  than their `project.json`, and for interrupted recordings, and offers to **Recover** them (or
  Discard, or decide Later; projects you don't recover are offered again next time). The sheet
  says which takes on which tracks were being recorded. Opening such a project asks the same.
  Recovering puts the takes back as one undo step (Undo: Recover Recording). Discarding keeps the
  recorded files in Audio Files, repaired, so you can still import them by hand.
- A recovered project counts as edited; save it to keep the changes.
- **Damaged files:** saves are written to a temporary file and then renamed into place, so a
  crash during a save leaves the previous version intact, and each save keeps the previous
  `project.json` as `project.json.bak`. If `project.json` is missing, cut short or damaged when
  you open a project, Fermata opens the autosave instead, or failing that the backup, and says
  so; save to repair it. A damaged autosave isn't offered for recovery.

### Project Settings

**File › Project Settings…** (Alt+P, or the LCD's chevron menu) holds what is saved with the
project:

| Tab | Settings |
|---|---|
| General | Tempo (5 to 990 BPM), Time Signature, Key, Sample Rate (with a warning when existing audio would play at the wrong speed). |
| Recording | Bit Depth: Use Settings, 24-bit integer or 32-bit float; Count-in on or off. |
| Metronome | The metronome settings (see [Settings › Metronome](#metronome)). |
| Notes | Free-form notes (lyrics, session notes, who played what). |

Tempo, time signature, key, bit depth, metronome settings and notes can be undone. The sample
rate change is applied to the audio device and is not an undo step.

---

## 16. Settings reference

Open Settings with **Fermata › Settings…** (Ctrl+,), which opens on the Audio tab;
Fermata › Appearance… and Fermata › Key Commands… (Alt+K) open those tabs. Most changes apply
at once and are kept in `settings.json` (`~/.config/fermata` on Linux and macOS,
`%APPDATA%\fermata` on Windows). The exceptions are the Audio devices (Apply Changes) and the
Metronome tab (saved with the project).

### General

| Setting | Choices | Default |
|---|---|---|
| At Launch | Show the Project Chooser; Open the most recent project; Start with an empty project | Project Chooser |
| Project Folder | Any folder (Choose…, Default) | `~/Music/Fermata` |
| Autosave | Off; every 1, 2, 5, 10 or 15 minutes | Every 2 minutes |
| After Recording | Autosave as soon as a recording, Bounce in Place, Freeze or import finishes | On |
| Undo Steps | 10 to 1000 | 100 |
| Unsaved Changes | Ask before quitting or closing a project with unsaved changes | On |
| Open Effect Panels | Floating; Docked in the bottom section; Separate windows (VST3 editors always open in their own window) | Floating |

### Audio

| Setting | Choices | Default |
|---|---|---|
| Driver | PipeWire, PulseAudio, JACK, ALSA on Linux; the system driver elsewhere | The first available, in that order (JACK only with a running server) |
| Output Device | System Default or a device | System Default |
| Input Device | None (playback only), System Default or a device | System Default |
| Sample Rate | Rates both devices support (44.1 to 192 kHz) | 48 kHz |
| I/O Buffer Size | 32, 64, 128, 256, 512, 1024, 2048 samples | 256 |
| Monitoring | Hear record-enabled tracks without pressing I | Off |
| Recording Delay | -500 to 500 ms, added to the measured latency | 0 |
| Plug-in Delay Compensation | All, Off (applies at once) | All |
| Low Latency Mode | On, with a limit of 0 to 100 ms (applies at once) | Off, 5 ms |

Below them: the measured input and output latency and how far recordings are shifted, the
input level check (Linux), and **Reset Audio Engine**. Everything except the two delay
compensation rows needs **Apply Changes**.

### Recording

| Setting | Choices | Default |
|---|---|---|
| Bit Depth | 24-bit integer, 32-bit float | 32-bit float |
| Recording Folder | A folder (recordings go into a subfolder named after the project), or the project's Audio Files | Project › Audio Files |
| Take Names | Vocals#03 (track and take number); Vocals (track name); Song - Vocals#03 (project, track and number) | Track and take number |
| Count-in | On (this project's switch, the same as Shift+K) and 1 to 4 bars | Off, 1 bar |
| Pre-roll | 0 to 30 seconds, used when Count-in is off | 0 |
| Overlapping Recordings › Audio | Put the new take on top; Create a new track; Replace what's underneath | On top |
| Overlapping Recordings › MIDI | Create a new region; Merge into the existing region | New region |

With a Recording Folder outside the project, Save As does not copy those recordings into the
new project folder; the project refers to them where they are.

### Metronome

These settings are saved with the **project**, as in Logic, and changes can be undone.

| Setting | Choices | Default |
|---|---|---|
| Click | Metronome on (K) | Off |
| Sound | Classic, Click, Woodblock, Beep | Classic |
| Volume | -40 to +6 dB | 0 dB |
| Accent | Higher and louder on the first beat of each bar | On |
| Pitch | Bar and Beat, -12 to +12 semitones | 0, 0 |
| Subdivision | Off (beats only), Eighth notes, Triplets, Sixteenth notes | Off |
| Playing | Click while playing | On |
| Recording | Click while recording; Only during the count-in | On; off |

The count-in always clicks, even with the metronome off. **Use for New Projects** makes the
current settings the default for new projects; **Revert to Default** brings back those
defaults.

### MIDI

| Setting | Choices | Default |
|---|---|---|
| MIDI Inputs | A checkbox per input, with its connection status | All on |
| MIDI Thru | Play the selected instrument track live | On |
| Velocity | As played; Soft (light touch plays louder); Hard (needs a firmer touch); Fixed velocity (1 to 127) | As played (fixed: 100) |
| Monitor | The last incoming messages (C3 = middle C) | |

The velocity curve applies to MIDI inputs, not to Musical Typing.

### Editing

| Setting | Choices | Default |
|---|---|---|
| Snap | Smart (follows the zoom), Bar, Beat, Off | Smart |
| Nudge Value | Bar, Beat, 1/8 note, 1/16 note, 1/32 note, 10 ms, 1 ms | 1/16 note |
| Click Moves Playhead | Anywhere in the tracks; Empty space only; Never (a click without a drag moves the playhead there, snapped to the grid) | Anywhere |
| Anti-click Fades | 0 to 100 ms, for new recordings and imports | 0 |
| Mouse Wheel Scrolls | Vertically (tracks); Horizontally (time), for a mouse with one wheel: the wheel scrolls through time in the Tracks area and Piano Roll, and Shift+wheel scrolls vertically | Vertically |

### Plug-ins

The Plug-in Manager: every scanned plug-in with its manufacturer, type and version; untick
**Use** to hide one from the Library and slot menus (Use All / Use None act on the filtered
list); **VST3 Folders** (the standard locations are always searched; Add Folder… adds more);
**Last Scan** (bundles, plug-ins and failures, with the reason for each failure: crashed, timed
out, no plug-ins); **Rescan Plug-ins**.

### Key Commands

Every window shortcut, searchable by name or key, in the groups Transport, Navigation, View,
Tracks, Editing and Windows.

- Click **+** and press a key to add a shortcut; Esc cancels. **×** removes one.
- Taking a key from another command moves it (a message says so). Keys that belong to the menu
  bar can't be used.
- **Default** restores a command's own keys; **Reset All** restores everything.
- The menu bar's shortcuts are listed at the end, read-only ("Menu Bar (fixed)").
- Shortcuts work in the main window, not while typing in a text field.

### Appearance

| Setting | Choices | Default |
|---|---|---|
| Interface Size | 90%, 100%, 110%, 125%, 150%, 175%, 200% (also View › Larger / Smaller Interface, Ctrl+= / Ctrl+-, Ctrl+0 for actual size) | 100% |
| Waveforms | Smooth (the true shape; individual cycles when zoomed in), Mirrored (loudness mirrored around the centre), Rectified (loudness rising from the bottom), Blocks (bar per bucket) | Smooth |
| Theme | Studio, Flat, Midnight, Light, High Contrast (also View › Theme) | Studio |

The whole window scales, and the canvas views render at the matching resolution so they stay
sharp.

---

## 17. Key command reference

Ctrl means ⌘ and Alt means ⌥ on macOS, except where a separate macOS key is given. The
commands in the first tables can be reassigned in Settings › Key Commands; the defaults are
shown.

### Transport

| Key | Command |
|---|---|
| Space | Play / Stop |
| R, Shift+R | Record (again to stop) |
| C | Cycle Mode |
| K | Metronome |
| Shift+K | Count-in |
| Enter (Return) | Go to Beginning |
| , | Rewind one bar |
| . | Forward one bar |
| \` | Catch Playhead |
| Alt+\` (⌥\` on macOS) | Scroll to Playhead |
| Ctrl+P (Control+⌘P on macOS) | Autopunch Mode |

### Navigation

| Key | Command |
|---|---|
| Alt+' | Create Marker |
| Alt+, | Go to Previous Marker |
| Alt+. | Go to Next Marker |
| Ctrl+Alt+K | Open Marker List |
| (none) | Delete Marker |

### View

| Key | Command |
|---|---|
| X | Show/Hide Mixer |
| I | Show/Hide Inspector |
| Y | Show/Hide Library |
| E, P | Show/Hide Editors |
| F | Show/Hide Browsers |
| A | Show/Hide Automation |
| H | Show Hidden Tracks |
| Ctrl+Right / Ctrl+Left | Zoom in / out horizontally |
| Ctrl+Down / Ctrl+Up | Zoom in / out vertically (taller / shorter tracks) |

### Tracks

| Key | Command |
|---|---|
| M | Mute the selected track |
| S | Solo the selected track |
| Ctrl+R (Control+R on macOS) | Record-enable the selected track |
| Up / Down | Select previous / next track |
| Ctrl+H (Control+H on macOS) | Hide the current track and select the next |

### Editing

| Key | Command |
|---|---|
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / Cut / Paste regions |
| Ctrl+M (Control+M on macOS) | Mute regions |
| Backspace, Delete | Delete regions (or selected automation points) |
| L | Loop regions |
| U | Set Locators by Regions |
| Alt+Left / Alt+Right | Nudge regions left / right |
| Ctrl+F (Control+⌘F on macOS) | Pack Take Folder |

### Windows

| Key | Command |
|---|---|
| Alt+P | Project Settings… |
| Alt+K | Key Commands… |

### Menu bar shortcuts (fixed)

| Key | Menu › Command |
|---|---|
| Ctrl+, | Fermata › Settings… |
| Ctrl+N | File › New… |
| Ctrl+Shift+O | File › Open Recent… |
| Ctrl+O | File › Open… |
| Ctrl+S | File › Save |
| Ctrl+Shift+S | File › Save As… |
| Ctrl+Shift+I | File › Import Audio File… |
| Ctrl+B | File › Bounce Project or Section… |
| Ctrl+Shift+E | File › Export All Tracks as Audio Files… |
| Ctrl+Z | Edit › Undo |
| Ctrl+Shift+Z | Edit › Redo |
| Ctrl+D | Edit › Duplicate |
| Ctrl+T | Edit › Split Regions at Playhead |
| Ctrl+J | Edit › Join Regions |
| Ctrl+A | Edit › Select All |
| Ctrl+Alt+N | Track › New Tracks… |
| Ctrl+Alt+B (Control+B on macOS) | Track › Bounce in Place… |
| Ctrl+Shift+D | Track › Create Folder Stack |
| Ctrl+G | Track › Group Selected Tracks |
| Ctrl+Backspace | Track › Delete Track |
| Ctrl+K | View › Musical Typing |
| Ctrl+= / Ctrl+- / Ctrl+0 | View › Larger Interface / Smaller Interface / Actual Size |

The Settings › Key Commands list shows Bounce in Place as ⌥⌘B; the real shortcut on macOS is
Control+B.

### Tracks area mouse modifiers

| Gesture | Action |
|---|---|
| Wheel | Scroll the tracks (or through time, see Settings › Editing › Mouse Wheel Scrolls) |
| Shift+wheel | Scroll through time (or the tracks, with Mouse Wheel Scrolls set to Horizontally) |
| Ctrl+wheel | Zoom horizontally around the pointer |
| Alt+wheel | Change track heights |
| Shift+click a region | Add it to or remove it from the selection |
| Ctrl+click a track header | Add or remove the track |
| Shift+click a track header | Select a range of tracks |
| Alt-drag a header's bottom edge | Resize every track |

### Piano Roll keys

These apply after you click in the Piano Roll, and can't be reassigned. In the Piano Roll
both Ctrl and ⌘ work on macOS.

| Key | Action |
|---|---|
| Left / Right | Nudge the selected notes by the grid (Shift: by a bar) |
| Up / Down | Transpose a semitone (Shift: an octave) |
| Alt+Left / Alt+Right | Select the previous / next note |
| Backspace, Delete | Delete notes (or the selected controller points, after clicking in the lane) |
| Q | Quantize |
| T, then T P E V M I G L | Choose a tool |
| Z | Zoom to fit the region |
| Esc | Deselect all |
| Ctrl+A | Select all |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / Cut / Paste at the playhead |
| Ctrl+D | Duplicate after the selection |
| Ctrl+M | Mute / unmute notes |
| Ctrl+Left / Ctrl+Right | Zoom out / in |
| Ctrl+Up / Ctrl+Down | Shorter / taller rows |

| Mouse | Action |
|---|---|
| Ctrl+click (Pointer) | Use the Pencil |
| Alt during a note drag | Copy the notes |
| Ctrl during a move | Move without snapping |
| Alt or Ctrl while resizing | Resize without snapping |
| Ctrl+wheel / Alt+wheel | Zoom horizontally / change row height |

### Audio Track Editor keys

These apply after you click in the editor; most need a range selection.

| Key | Action |
|---|---|
| Ctrl+T | Split at selection (at the playhead without one) |
| Ctrl+Shift+T | Split at transients |
| Backspace, Delete | Delete selection |
| Alt+Backspace | Silence selection |
| Alt+T | Trim region to selection |
| Alt+N | Normalize selection |
| Alt+R | Reverse selection |
| Alt+I / Alt+O | Fade in / out to selection |
| Ctrl+C / Ctrl+X | Copy / cut selection |
| Shift+Space | Play selection |
| Z | Zoom to selection or region, and back |
| Esc | Deselect |
| Ctrl+Left / Ctrl+Right | Zoom out / in |
| Ctrl+Down / Ctrl+Up | Waveform zoom in / out |

### Automation (with Show Automation on)

| Key or mouse | Action |
|---|---|
| A | Show / hide automation |
| Click empty lane | Add a point |
| Double-click or Alt+click a point | Delete it |
| Shift while dragging points | Keep them in time |
| Backspace, Delete | Delete the selected points |

### Musical Typing (while its window is open)

See [Musical Typing](#musical-typing): A to ' and W E T Y U O P play notes, Z / X change the
octave, C / V the velocity, Tab is the sustain pedal.

---

## 18. Troubleshooting

### Recordings are very quiet (or silent)

On Linux, the sound server (PipeWire or PulseAudio) has its own volume for each recording
device, and it often isn't at 100%. Fermata receives the signal after that volume, so a source
at 40% lowers everything you record by about 24 dB, and a muted source records silence.

- Settings › Audio shows a warning with **Set to 100%** when the level is low; Fermata also
  shows a notice at launch.
- Or set it yourself: `wpctl get-volume @DEFAULT_AUDIO_SOURCE@` shows it, and
  `wpctl set-volume @DEFAULT_AUDIO_SOURCE@ 1.0` sets it to 100% (use the node id from
  `wpctl status` for another device). pavucontrol's Input Devices tab does the same.
- Then set the recording level with the gain knob on your interface.

### Crackles, clicks or dropouts

- Raise the **I/O Buffer Size** in Settings › Audio (256 or 512 samples), then Apply Changes.
- Watch the **CPU** meter in the control bar; freeze heavy tracks (snowflake button).
- Make sure real-time scheduling is available: on Linux, rtkit must be running
  (`security.rtkit.enable = true;` on NixOS). PipeWire's own quantum can also be raised.
- Use **PipeWire** as the driver on Linux. Direct ALSA (`hw:`) devices lock the interface for
  Fermata alone.

### Plug-in windows don't open on Wayland

Plug-in editors are X11 windows, and GLX-based plug-in GUIs fail when GTK runs natively on
Wayland in the same process. Fermata therefore runs its interface through XWayland on Linux
by default (it sets `GDK_BACKEND=x11` unless you set it). If you set `GDK_BACKEND=wayland`
yourself, plug-in GUIs may fail to open; unset it. On Windows and macOS plug-in editor
windows are not implemented yet; plug-ins still load and play, and their parameters can be
automated.

### Your computer's audio is recorded, or loops back

Some USB devices (guitar pedals and multi-effects, some interfaces) loop the computer's playback
back into their recording input, so a recording picks up the backing track or system sounds,
and monitoring can feed back.

- Check the device's own settings or editor app for a **loopback**, "USB playback to input" or
  "re-amp" option and turn it off, or choose an input channel that doesn't carry the loopback.
- Make sure only one of Fermata's monitoring and the device's direct monitoring is on (Settings
  › Audio › Monitoring, and the track's I button).
- Use a different device for output than for input (Settings › Audio lets them differ), or
  turn down the device's computer-playback level.
- On PipeWire, check in `pw-top` or a patchbay (qpwgraph, Helvum) that nothing else is linked
  into the device's input.

### Recordings are early or late

Set **Recording Delay** in Settings › Audio. See [Latency compensation](#latency-compensation).

### "System overload" and playback stops

When the computer can't produce audio as fast as the device plays it for more than about 60 ms
in a row, Fermata silences the output for a quarter of a second so the audio thread can catch
up, stops playback or recording, and shows "System overload: the audio engine couldn't keep
up…". (Without this, the system would kill Fermata for holding the processor.) Raise the I/O
Buffer Size, bypass or freeze heavy plug-ins, and see
[Crackles, clicks or dropouts](#crackles-clicks-or-dropouts). Bounces and other offline renders
are not affected.

### "… put out invalid audio" and a plug-in goes quiet

Fermata checks the output of every plug-in, built-in or VST3, each block. NaNs are silenced
and levels above +60 dBFS are clamped, so one unstable plug-in can't blow up the rest of the
mix or your speakers. When that happens, a message names the plug-in and the track (at most
every few seconds while it continues). A built-in effect is also reset so it recovers. Bypass
the plug-in or reset its settings; if a VST3 keeps doing it, report it to its maker.

"The audio engine hit an internal error and skipped some audio" means the engine itself failed
for a block; the block is silenced instead of crashing. Please report it with the terminal
output.

### The playhead or meters freeze during a bounce

That is expected: renders run offline and the interface waits for them. Playback is silent
until the render finishes.

### Plug-ins are missing

Check Settings › Plug-ins: the plug-in may be switched off, its folder may not be searched (add
it under VST3 Folders), or it may have failed in the last scan (expand the report). Rescan
after installing plug-ins.

- On a Mac with Apple Silicon, a plug-in built only for Intel can't load; the report says so.
  Install its Universal or Apple Silicon version.
- A project from another computer finds a plug-in installed in a different folder there by
  itself. A plug-in that isn't installed at all shows struck through in the Mixer, and opening
  the project says which ones are missing. Its settings stay in the project, so it sounds as
  before on a computer that has it, even if you save the project here.

### A blank window on Linux

Some GPU drivers need `WEBKIT_DISABLE_DMABUF_RENDERER=1`. Set it only if the window stays
blank, because WebKit then draws on the CPU.

---

## 19. Known limitations

- **Platforms:** plug-in editor windows open on Linux (X11) and macOS; Windows (HWND)
  embedding is planned. On macOS, keys a plug-in's window doesn't use itself (Space, R, other
  key commands, Musical Typing) go to Fermata, while typing into the plug-in's own fields stays
  there; Esc doesn't close the window. On Linux only menu shortcuts work while a plug-in's
  window is in front. The input level check is Linux only. On Windows and macOS only the
  system's default audio driver is available (no ASIO).
- **Plug-in formats:** VST3 only; no CLAP, AU or VST2.
- **Tempo:** moving regions in the tracks area is in samples, not bars, across tempo changes;
  Move MIDI Regions with Tempo doesn't move automation or markers, isn't applied to base-tempo
  edits in Project Settings, and isn't remembered between sessions.
- **Editing:** no Marquee tool, Fade tool or region gain drag. Drag modes are Overlap and
  X-Fade only (no No Overlap or Shuffle). Apply Last Fade, fade marquee selections and the
  Crossfade Time and Curve settings aren't implemented.
- **Crossfades** are only between plain audio regions: not take folders, looping regions or
  muted regions (those fade out as Out). A crossfade's handle can't be grabbed where the
  incoming region is drawn over it (an overlap with the newer region on top); use the
  Inspector. The Inspector's fade rows edit the one selected region.
- **Plug-in delay compensation:** the mix plays behind the playhead by the total latency rather
  than reading regions early, so live input on a monitored track that isn't recording is delayed
  with everything else (Low Latency Mode avoids that). Low Latency Mode only switches off
  plug-ins on the live track itself, not on the auxes or stacks it plays through. Fader
  automation written live is stored at the playhead, not compensated. Delay changes while
  playing (bypass, a plug-in reporting new latency) can click.
- **Separate windows:** the analyzer follows one strip, so a Channel EQ in a separate window
  and another strip's Channel EQ in the main window take turns showing it. On Linux under
  native Wayland (not the default), the compositor decides where windows open.
- **Built-in effects:** the side-chain key is always post-fader (no pre-fader option yet).
  Changing a Lookahead while playing can click as the delay compensation moves. Bounces keep
  the tail length set in the Bounce dialog whatever an effect's own tail; Freeze stretches its
  tail to the longest one. VST3 user presets aren't saved by Fermata (use the plug-in's own).
- **Freeze** renders from the track's first region to its last plus up to 4 s of tail. A frozen
  track's plug-in automation lanes stay editable but do nothing until it is unfrozen.
- **Imports:** AAC/M4A files may start about 40 ms late, because the encoder delay isn't fully
  trimmed. MP3 is trimmed exactly.
- **Take folders** have one comp (Logic keeps several). Recording without Cycle over a folder
  doesn't add a take to it. A folder's own fades shape only its first and last comp stretch.
  With Autopunch inside the cycle, a punch-out closer to the cycle end than the recording
  latency makes each take that much shorter.
- **Markers** have no length editing (a marker reaches the next one); there is no Arrangement
  track.
- **Automation:** folder stacks and Stereo Out have no automation of their own (a plug-in moved
  to Stereo Out leaves its lanes behind), and lanes don't move with regions.
- **Piano Roll:** no Step Input, Event List or Score, and controllers have no curve shapes.
- **Undo:** the metronome and count-in switches and the project sample rate are not undoable,
  and switching the metronome doesn't mark the project as edited.
