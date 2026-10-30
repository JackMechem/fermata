#!/usr/bin/env bash
# Builds the engine's examples once and runs every headless check that needs no third-party
# plug-ins, on ALSA's `null` output device, the way CI runs them (.github/workflows/ci.yml).
#
# The checks run in an isolated environment: HOME and the XDG folders point into the scratch
# folder, and the audio-server variables are cleared, so they see no PipeWire, PulseAudio or
# JACK server and never read or write your settings, plug-in cache, presets or projects.
# The test tone comes from the `test_tone` example.
#
# usage: scripts/headless-checks.sh [--scratch DIR] [check ...]
#   --scratch DIR  where the projects, renders and logs go (default: a new temporary folder,
#                  removed afterwards when every check passed)
#   check ...      run only these checks (default: all of them)
# On Linux it needs the ALSA library and its `null` PCM (part of alsa-lib's standard config).
# Elsewhere (macOS) the engine stands in for that device with a silent thread of its own.
set -euo pipefail

# Each check and its arguments: "tone" is the test tone, "dir" its scratch folder.
CHECKS=(
  "stack_check tone dir"
  "loop_check tone dir"
  "sends_check tone dir"
  "send_auto_check tone dir"
  "bounce_check tone dir"
  "export_check tone dir"
  "range_edit_check tone dir"
  "automation_check tone dir"
  "insert_order_check tone dir"
  "fx_check tone dir"
  "fx_play_check tone dir"
  "takes_check tone dir"
  "recovery_check tone dir"
  "crossfade_check tone dir"
  "dragmode_check tone dir"
  "midi_check dir"
  "countin_check dir"
  "tempo_check dir"
  "region_params_check tone dir"
  "strip_check tone dir"
  "controller_check tone dir"
  "vca_check tone dir"
  "chase_check dir"
  "midi_replace_check dir"
)
# A check that hangs (a transport that never stops, say) fails instead of stalling CI.
TIMEOUT=${HEADLESS_CHECK_TIMEOUT:-600}

scratch=
only=()
while [ $# -gt 0 ]; do
  case $1 in
    --scratch) scratch=${2:?--scratch needs a folder}; shift 2 ;;
    -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) only+=("$1"); shift ;;
  esac
done

# (`${only[@]+...}`: macOS still ships bash 3.2, where an empty array is "unbound" under set -u.)
for name in ${only[@]+"${only[@]}"}; do
  case " ${CHECKS[*]%% *} " in
    *" $name "*) ;;
    *) echo "unknown check: $name" >&2; exit 2 ;;
  esac
done

made_scratch=
if [ -z "$scratch" ]; then
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/fermata-checks.XXXXXX")
  made_scratch=1
fi
mkdir -p "$scratch"
scratch=$(cd "$scratch" && pwd)

cd "$(dirname "$0")/.."

echo "== Building the engine examples"
cargo build -p daw-engine --examples
target=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
bin="$target/debug/examples"

# The environment of a fresh CI runner: an empty home, no session bus, no audio servers.
env_home="$scratch/_env"
rm -rf "$env_home"
mkdir -p "$env_home/.config" "$env_home/.local/share" "$env_home/.cache" "$env_home/.local/state" "$env_home/runtime"
chmod 700 "$env_home/runtime"
isolated=(
  env
  -u PIPEWIRE_RUNTIME_DIR -u PIPEWIRE_REMOTE -u PULSE_SERVER -u PULSE_RUNTIME_PATH
  -u JACK_DEFAULT_SERVER -u JACK_PROMISCUOUS_SERVER -u DBUS_SESSION_BUS_ADDRESS
  -u APPDATA -u USERPROFILE
  HOME="$env_home"
  XDG_CONFIG_HOME="$env_home/.config"
  XDG_DATA_HOME="$env_home/.local/share"
  XDG_CACHE_HOME="$env_home/.cache"
  XDG_STATE_HOME="$env_home/.local/state"
  XDG_RUNTIME_DIR="$env_home/runtime"
  JACK_NO_START_SERVER=1
)
if command -v timeout > /dev/null; then
  isolated+=(timeout "$TIMEOUT")
fi

tone="$scratch/tone.wav"
"${isolated[@]}" "$bin/test_tone" "$tone"

failed=()
ran=0
for entry in "${CHECKS[@]}"; do
  read -r name args <<< "$entry"
  if [ ${#only[@]} -gt 0 ]; then
    case " ${only[*]} " in
      *" $name "*) ;;
      *) continue ;;
    esac
  fi
  dir="$scratch/$name"
  rm -rf "$dir"
  mkdir -p "$dir"
  argv=()
  for a in $args; do
    case $a in
      tone) argv+=("$tone") ;;
      dir) argv+=("$dir") ;;
    esac
  done
  echo
  echo "== $name"
  ran=$((ran + 1))
  status=0
  "${isolated[@]}" "$bin/$name" "${argv[@]}" 2>&1 | tee "$dir/output.log" || status=$?
  # tempo_check reports failures only in its output, so look there too.
  if [ "$status" -ne 0 ] || grep -q '^FAIL' "$dir/output.log"; then
    failed+=("$name")
  fi
done

echo
if [ ${#failed[@]} -gt 0 ]; then
  echo "== ${#failed[@]} of $ran checks failed: ${failed[*]}"
  echo "   logs and projects: $scratch"
  exit 1
fi
echo "== All $ran checks passed"
if [ -n "$made_scratch" ]; then
  rm -rf "$scratch"
fi
