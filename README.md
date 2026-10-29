# Fermata

A digital audio workstation modelled on Logic Pro's workflow and layout. The audio engine and
VST3 host are written in Rust; the interface is Tauri 2 with React and TypeScript.

Fermata is in alpha. It is developed and tested on Linux; the Windows and macOS builds are
early and less tested (see [Platform support](#platform-support)).

## Features

- **Recording:** audio from any input (mono or stereo per track) with live waveforms, latency
  compensation and a manual recording delay; MIDI with controllers, pitch bend and aftertouch;
  count-in and pre-roll; cycle recording into take folders; Autopunch; a choice of what happens
  to overlapped regions.
- **Take folders and comping**, after Logic's Quick Swipe Comping: swipe across take rows to
  build a comp, with crossfaded boundaries; Flatten, Flatten and Merge, Pack and Unpack.
- **Tracks area:** move, trim, split, join, loop, fade, gain, reverse, colour, copy, paste,
  repeat, nudge, Alt-drag copy and rubber-band select regions with Smart snap; fade curves,
  Speed Up and Slow Down fades, and X, EqP and X S crossfades between adjacent or overlapping
  regions, made automatically by edits in the X-Fade drag mode; track stacks
  (folder and summing), groups, hidden tracks, Track Freeze, and a Marker track.
- **Editors:** a Piano Roll with eight tools, Time Quantize with strength and swing, scale
  quantize, humanize, note-length functions and a velocity / controller lane; an Audio Track
  Editor with range edits, normalizing, fades to selection and Split at Transients; Standard
  MIDI File import and export.
- **Mixing:** channel strips with an eight-band Channel EQ and analyzer, eight Audio FX slots,
  eight pre- or post-fader sends, aux buses and output routing; plug-in delay compensation on
  every path, with a Low Latency Mode.
- **Automation:** volume, pan, mute, send levels and plug-in parameters, with Logic's Off,
  Read, Touch, Latch and Write modes.
- **Plug-ins:** VST3 instruments and effects, scanned out of process; Fermata Synth built in.
  Copy and paste plug-ins (VST3 state included) and Channel EQ settings between strips and
  projects; drag plug-ins to reorder them or move them to another strip (Alt copies).
- **Built-in effects:** Compressor (five modelled circuits), Limiter, Noise Gate, De-Esser,
  Reverb, Delay, Tuner and Loudness Meter, in any Audio FX slot with
  Logic-style panels, factory and user presets, automatable parameters, side-chain input from
  any track, latency reported to delay compensation, and freeze support. The Channel EQ has
  presets too.
- **Bounce and export:** offline bounce of the project or a section, Export All Tracks,
  Export Region, Bounce in Place, as 16-bit (dithered), 24-bit or 32-bit float WAV.
- **Tempo and signature:** tempo changes and smooth ramps, time signature changes, in Tempo and
  Signature global tracks; the grid, click, quantize, plug-ins and MIDI files all follow them.
- **Projects:** `.fermata` folders, a Project Chooser with templates, autosave and crash
  recovery, and undo for every edit (100 steps by default).
- **Interface:** Logic Pro 11's layout, five themes, interface scaling from 90 to 200 %, four
  waveform styles, rebindable key commands shown the way your platform writes them, Musical
  Typing, and a click in the tracks area that moves the playhead (configurable).
- **Robustness:** plug-ins are scanned in a separate process; every plug-in's output is
  checked, so NaNs and runaway levels are silenced and the plug-in is named in a message; an
  internal engine error skips one block instead of crashing; the audio thread never waits on a
  lock.

The [User Guide](docs/USER_GUIDE.md) describes all of it in detail.
[LOGIC_FEATURES.md](LOGIC_FEATURES.md) lists every Logic Pro feature by stage and marks what
Fermata already has.

## Install

Prebuilt binaries are on the [GitHub Releases page](https://github.com/JackMechem/fermata/releases):

- **Linux** (x86_64 and aarch64): `.AppImage` (any distribution), `.deb` (Debian, Ubuntu) and
  `.rpm` (Fedora, openSUSE; stable releases only, since RPM can't version a prerelease).
  They're built on Ubuntu 24.04, so they need glibc 2.39 and
  PipeWire 1.0 or newer (Ubuntu 24.04, Debian 13, Fedora 40 and later).
- **Windows** (x64): the NSIS installer, `_x64-setup.exe`.
- **macOS:** `_aarch64.dmg` for Apple Silicon, `_x64.dmg` for Intel.

The builds are not signed yet, so the system warns about them:

- **macOS:** the first time, right-click the app and choose Open, or run
  `xattr -cr /Applications/Fermata.app`.
- **Windows:** in the SmartScreen prompt, choose **More info › Run anyway**.

On NixOS or with Nix, use the flake instead (below).

## Platform support

| | Linux | Windows | macOS |
|---|---|---|---|
| Status | Main platform, tested | Early, less tested | Early, less tested |
| Packages | AppImage, .deb, .rpm from stable releases (x86_64, aarch64); Nix flake | NSIS installer (x64) | .dmg (Apple Silicon, Intel) |
| Audio drivers | PipeWire, PulseAudio, JACK, ALSA | WASAPI | Core Audio |
| MIDI input | Yes | Yes | Yes |
| VST3 instruments and effects | Yes | Yes | Yes |
| VST3 editor windows | Yes (X11; through XWayland on Wayland) | Not yet | Not yet |
| Built-in effects, Fermata Synth | Yes | Yes | Yes |
| Input level check | Yes (PipeWire, PulseAudio) | No | No |

On Windows and macOS, VST3 plug-ins still load, play, save their state and can be automated;
only their own editor window is missing.

## Build from source

On NixOS or any system with Nix flakes:

```sh
git clone https://github.com/JackMechem/fermata && cd fermata
nix run .                                 # build and launch
```

To develop:

```sh
nix develop                               # Rust, Node, WebKitGTK, ALSA/JACK/PipeWire
pnpm install                              # or npm install
pnpm tauri dev                            # or npm run tauri dev
```

On other Linux distributions, install Rust, Node.js, the Tauri 2 prerequisites
(`webkit2gtk-4.1`, `gtk3`, `libsoup-3`) and `alsa-lib`, `libpipewire-0.3`, `jack`, `dbus` and
`libclang`, then run `pnpm install && pnpm tauri dev`. See [DEVELOPING.md](docs/DEVELOPING.md)
for Windows and macOS, tests and packaging.

## First steps

1. Create a project from the Project Chooser (a template such as Record Audio or Software
   Instrument).
2. Open Settings › Audio (Ctrl+, or ⌘, on macOS), choose a driver (PipeWire is recommended on
   Linux) and your devices, and click Apply Changes.
3. Record-enable a track with its R button and press R to record, Space to stop.

## Documentation

| Document | For |
|---|---|
| [docs/USER_GUIDE.md](docs/USER_GUIDE.md) | Using Fermata: recording, editing, mixing, exporting, every setting and key command, troubleshooting. |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | How the code works: threads, real-time rules, the project model, the engine graph, undo, persistence, the VST3 host, the frontend, and a map of every source file. |
| [docs/DEVELOPING.md](docs/DEVELOPING.md) | Building, testing, debugging, conventions, and recipes for common changes. |
| [docs/logic-reference.md](docs/logic-reference.md) | The Logic Pro research behind the layout and behaviour. |
| [LOGIC_FEATURES.md](LOGIC_FEATURES.md) | The feature scope: every Logic Pro feature by stage, with Fermata's status. |
| [TODO.md](TODO.md) | The roadmap, in stages. |
| [CHANGELOG.md](CHANGELOG.md) | What changed in each release. |

## Status and known limitations

Fermata is alpha software under active development; keep backups of projects that matter. The
main limitations:

- Plug-in editor windows are Linux/X11 only. On Linux the interface runs through XWayland by
  default, because GLX-based plug-in GUIs fail when GTK runs natively on Wayland.
- VST3 only (no CLAP, AU or VST2). On Windows and macOS only the system's default audio driver
  is available (no ASIO).
- The release builds are unsigned (see [Install](#install)).
- Regions stay at fixed times when the tempo changes (Move MIDI Regions with Tempo keeps MIDI
  on its bars); region drags in the tracks area move in samples.
- No Marquee tool, Fade tool, or No Overlap and Shuffle drag modes yet.
- No Step Input, Event List or Score in the Piano Roll; no MIDI learn.

The User Guide's [Known limitations](docs/USER_GUIDE.md#19-known-limitations) has the full list,
[TODO.md](TODO.md) the staged roadmap, and [CHANGELOG.md](CHANGELOG.md) what each release
contains.

## Versions

Fermata uses [semantic versioning](https://semver.org). Prereleases are tagged
`vX.Y.Z-alpha.N` (the first is `v0.1.0-alpha.1`) and published as prereleases on GitHub; see
[DEVELOPING.md](docs/DEVELOPING.md#versioning-and-releases) for how a release is made.

## Licence

Fermata is released under the MIT licence; see [LICENSE](LICENSE). The release bundles include
a copy.
