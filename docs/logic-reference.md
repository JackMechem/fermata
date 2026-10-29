# Logic Pro 11 reference (design research)

This is a condensed version of the research behind Fermata's layout and behaviour. The primary
source is Apple's Logic Pro User Guide (support.apple.com/guide/logicpro).

Colour values are estimates from screenshots, because Apple does not publish them. Items marked
[unc] were not confirmed against a live install.

## Main window

| Area | Position | Key |
|---|---|---|
| Control bar | top | — |
| Library | far left | Y |
| Inspector | left of the track headers | I |
| Tracks area | centre (local menus, ruler, headers, workspace) | — |
| Smart Controls / Mixer / Editors | shared bottom pane | B / X / E (Piano Roll P, Score N) |
| List Editors / Note Pads / Loop Browser / Browsers | shared right pane | D / ⌃⇧P / O / F |

## Control bar (left to right)

1. View buttons: Library, Inspector, Toolbar, Smart Controls, Mixer, Editors.
2. Transport: Rewind, Forward, Stop, Play, Record.
   - Stop pressed a second time goes to the start, or to the left locator when Cycle is on.
3. LCD in Beats & Project mode: BAR / BEAT, TEMPO (KEEP / ADAPT / AUTO), time signature, key.
   - Edit by dragging vertically or double-clicking.
   - Other modes: Beats & Time, Beats, Time, Custom (adds CPU/HD meters and locators).
4. Modes: Cycle (yellow), Replace, Tuner, Solo, Count-in "1234", and Metronome. Count-in and
   Metronome are purple when on. The exact default set is [unc].
5. Master volume.
6. Right side: List Editors, Note Pads, Loop Browser, Browsers.

## Tracks area

**Local bar:**
- Edit, Functions and View menus.
- Show Automation (A) and Flex (⌘F).
- Left-click and ⌘-click tool menus.
- Catch (`).
- Snap: Smart (default), Bar, Beat, Division, Ticks, Frames, Samples.
- Drag: Overlap, No Overlap, X-Fade, Shuffle R/L.
- Waveform zoom and the zoom sliders.

**Above the headers:** "+" (⌥⌘N), Duplicate Track, and Clear/Recall Solo.

**Track header:**
- Number, colour bar, icon, name.
- M (blue), S (yellow), R (red), I (orange).
- Volume and pan, with a level meter.

**Ruler:**
- Upper strip: the cycle. Drag to create it (yellow); click to toggle.
- ⌘-drag makes a skip cycle.
- The middle third holds the red Autopunch stripe.
- Clicking the lower half sets the playhead.

**Regions:**
- Rounded, with a name bar.
- Audio regions show a waveform; MIDI regions show note dashes.
- Default colours: audio blue, software instrument green, Drummer/Session Player yellow.

**New Tracks sheet (Logic 11):**
- Types: MIDI (Software Instrument / External MIDI), Pattern, Session Player, Audio (Mic or
  Line / Guitar or Bass).
- Details: Instrument; Audio Input with Ascending; Audio Output; Load Default Patch; Open
  Library; Input Monitoring; Record Enable; Number of tracks; Create.

## Inspector

From the top:
1. Region inspector: Mute, Loop, Quantize, Q-Swing, Transpose, Velocity / Fine Tune, Gain,
   plus "More".
2. Track inspector.
3. Two channel strips: the selected track, and its output (Stereo Out, or the aux after
   ⇧-clicking a send).

## Channel strip, top to bottom

1. Setting
2. Gain reduction meter
3. EQ thumbnail
4. MIDI FX
5. Input (with mono/stereo) or Instrument
6. Audio FX
7. Sends
8. Output
9. Group
10. Automation mode (Read)
11. Pan
12. Volume dB and peak readouts
13. Fader with meter
14. R and I, then M and S
15. Bounce and Dim (on output strips)
16. Name with colour bar

The mixer has Single / Tracks / All views, type filters, and Wide/Narrow strips.

## Key commands

**Transport:**
- Space play/stop; Return to beginning
- R record; ⌘. discard recording
- , and . move one bar back/forward
- C cycle; K metronome; ⇧K count-in

**Tracks:**
- M mute, S solo, ⌃R record-enable, ⌃I input monitoring
- ⌥⌘N new tracks, ⌘D duplicate, ⌘⌫ delete
- ⇧↩ rename

**Editing:**
- ⌘T split at playhead, ⌘R repeat, ⌘J join
- Q quantize, L loop, ⌃M mute regions, ⌘G snap

**Zoom:**
- ⌘← / ⌘→ horizontal, ⌘↑ / ⌘↓ vertical
- Z zoom to fit, ` catch

**Tools:** T opens the tool menu at the pointer.
- P pencil, E eraser, I scissors, J join, F finger
- S solo, M mute, Q quantize, V velocity
- R marquee, A fade, G gain

**File:** ⌘S save, ⇧⌘S save as, ⌘B bounce, ⌘E export track.

## Feature backlog

**Recording:**
- Take folders with cycle recording, and Quick Swipe Comping.
- Replace mode, Autopunch, and Quick Punch-In.
- Count-in options and pre-roll.
- Auto Input Monitoring, Low Latency Mode, Flashback Capture.

**MIDI:**
- Piano Roll, Step Sequencer, Score, Event List.
- Quantize with swing and strength, and MIDI FX.
- Musical Typing.

**Audio:**
- Flex Time and Flex Pitch.
- Audio Track/File editors.
- Fades and crossfades, gain, normalize.

**Tempo:** tempo track, Smart Tempo, beat mapping.

**Arranging:** Session Players and Drummer, Live Loops, markers, Track Alternatives, Stem Splitter.

**Routing:**
- Aux sends and buses, VCAs, groups.
- Track stacks (folder and summing).

**Automation:**
- Modes: Off, Read, Touch, Latch, Write, plus Trim and Relative.
- Region automation.

**Bounce:**
- PCM, MP3 and M4A.
- Realtime or offline, normalize, include tail.

**Settings › Audio:**
- I/O buffer 32–1024 and recording delay.
- Process buffer range.
- Plug-in latency compensation.

**Visual tokens:**

| Element | Approx. value |
|---|---|
| Base | #1E1E1E–#262626 |
| Track headers | #3A3A3A; selected #4A4A4A–#575757 |
| Cycle | #E8C120–#F2C230 |
| Record | #FF3B30 |
| Mute | #3FA2F5 |
| Solo | #F5C542 |
| Monitor | #F39A26 |
| Metronome / Count-in | #A07BFF |
| Playhead | #F0F0F0 |
| Audio regions | ~#3E8EDB |
| MIDI regions | ~#56B04C |
