#!/bin/sh
# Sets Fermata's version everywhere it is recorded: the Cargo workspace (and Cargo.lock),
# package.json and package-lock.json, src-tauri/tauri.conf.json and nix/package.nix.
# Needs GNU sed (Linux, or gsed on macOS).
# usage: scripts/set-version.sh 0.2.0      (or a prerelease such as 0.2.0-alpha.1)
set -eu
v=${1:?usage: scripts/set-version.sh <version>}
if ! printf '%s' "$v" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$'; then
  echo "not a semver version: $v" >&2
  exit 1
fi
cd "$(dirname "$0")/.."

# The workspace version is the only `version = ` line in the [workspace.package] table.
sed -i.bak "/^\[workspace.package\]/,/^\[/ s/^version = \".*\"/version = \"$v\"/" Cargo.toml
# Cargo.lock: the three workspace crates.
awk -v v="$v" '
  /^name = "(daw|daw-engine|vst3-host)"$/ { mine = 1; print; next }
  mine && /^version = / { print "version = \"" v "\""; mine = 0; next }
  { mine = 0; print }
' Cargo.lock > Cargo.lock.new && mv Cargo.lock.new Cargo.lock
# The first "version" in each JSON file is the package's own; package-lock.json repeats it
# for the root package entry.
sed -i.bak "0,/\"version\": \".*\"/ s//\"version\": \"$v\"/" package.json src-tauri/tauri.conf.json
awk -v v="$v" '
  n < 2 && /"version": "/ { sub(/"version": "[^"]*"/, "\"version\": \"" v "\""); n++ }
  { print }
' package-lock.json > package-lock.json.new && mv package-lock.json.new package-lock.json
sed -i.bak "s/^  version = \".*\";/  version = \"$v\";/" nix/package.nix
rm -f Cargo.toml.bak package.json.bak src-tauri/tauri.conf.json.bak nix/package.nix.bak

grep -H '^version = ' Cargo.toml
grep -H -m1 '"version"' package.json src-tauri/tauri.conf.json
grep -H '^  version = ' nix/package.nix
