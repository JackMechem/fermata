# Non-flake entry point (`nix-shell`); the same environment as `nix develop`.
{
  pkgs ? import <nixpkgs> { },
}:

import ./nix/shell.nix {
  inherit pkgs;
  fermata = pkgs.callPackage ./nix/package.nix { };
}
