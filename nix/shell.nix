# Development shell. Takes its toolchain and libraries from the package itself, so the shell
# and `nix build` can't drift apart.
{ pkgs, fermata }:

pkgs.mkShell {
  inputsFrom = [ fermata ];

  packages = with pkgs; [
    cargo
    rustc
    clippy
    rustfmt
    rust-analyzer
    nodejs
  ];

  shellHook = ''
    export GIO_MODULE_DIR=${pkgs.glib-networking}/lib/gio/modules/
    # GTK file dialogs need the gsettings schemas.
    export XDG_DATA_DIRS=${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS
    # Libraries opened at run time by dev builds.
    export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath (with pkgs; [ alsa-lib libjack2 pipewire dbus ])}:$LD_LIBRARY_PATH
    # Leave WebKitGTK's DMA-BUF (GPU) renderer on: disabling it makes WebKit paint on the CPU,
    # roughly 15x the per-frame cost. Only set WEBKIT_DISABLE_DMABUF_RENDERER=1 if a GPU driver
    # shows a blank window.
  '';
}
