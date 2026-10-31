# Fermata as a Nix package: the frontend is built with npm in the sandbox, Rust crates are
# vendored from Cargo.lock, and `cargo tauri build` produces a .deb whose contents land in $out.
{
  lib,
  rustPlatform,
  importNpmLock,
  nodejs,
  cargo-tauri,
  pkg-config,
  wrapGAppsHook3,
  alsa-lib,
  libjack2,
  pipewire,
  dbus,
  webkitgtk_4_1,
  gtk3,
  glib,
  glib-networking,
  libsoup_3,
  librsvg,
  openssl,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "fermata";
  version = "0.1.0-alpha.4";

  # Only what the build reads, so editing docs doesn't trigger a rebuild.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../src-tauri
      ../src
      ../index.html
      # bundled with the app (Tauri's bundle.resources)
      ../LICENSE
      ../package.json
      ../package-lock.json
      ../tsconfig.json
      ../vite.config.ts
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  # Read straight from package-lock.json (each package is fetched by its integrity hash), so
  # there's no npmDepsHash to update whenever the lockfile or the version changes.
  npmDeps = importNpmLock { npmRoot = finalAttrs.src; };

  nativeBuildInputs = [
    cargo-tauri.hook
    nodejs
    importNpmLock.npmConfigHook
    pkg-config
    wrapGAppsHook3
    # libclang for the PipeWire bindings (pipewire-sys runs bindgen)
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    # audio and MIDI: ALSA, JACK, PipeWire; D-Bus for rtkit real-time scheduling
    alsa-lib
    libjack2
    pipewire
    dbus
    # Tauri / WebKitGTK
    webkitgtk_4_1
    gtk3
    glib
    glib-networking
    libsoup_3
    librsvg
    openssl
  ];

  buildAndTestSubdir = "src-tauri";

  # Run the engine's DSP unit tests (Channel EQ filters, analyzer). They need no audio devices.
  cargoTestFlags = [
    "--package"
    "daw-engine"
    "--lib"
  ];

  postInstall = ''
    # The Cargo binary is called `daw`; offer the product name as the command too.
    ln -s daw $out/bin/fermata

    # Tauri leaves the categories empty; list it with the audio apps.
    substituteInPlace $out/share/applications/Fermata.desktop \
      --replace-fail "Categories=" "Categories=AudioVideo;Audio;Music;Recorder;" \
      --replace-fail "Type=Application" "Type=Application
    Comment=Record, mix and produce music with VST3 plug-ins
    Keywords=DAW;audio;MIDI;recording;VST;mixer;"
  '';

  meta = {
    description = "Cross-platform DAW with VST3 hosting, modelled on Logic Pro's workflow";
    license = lib.licenses.mit;
    mainProgram = "fermata";
    platforms = lib.platforms.linux;
  };
})
