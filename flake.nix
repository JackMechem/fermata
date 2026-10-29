{
  description = "Fermata, a cross-platform DAW with VST3 hosting, modelled on Logic Pro";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        fermata = pkgs.callPackage ./nix/package.nix { };
        default = fermata;
      });

      apps = forAllSystems (pkgs: rec {
        fermata = {
          type = "app";
          program = nixpkgs.lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.fermata;
        };
        default = fermata;
      });

      devShells = forAllSystems (pkgs: {
        default = import ./nix/shell.nix {
          inherit pkgs;
          inherit (self.packages.${pkgs.stdenv.hostPlatform.system}) fermata;
        };
      });

      # For NixOS / home-manager configurations: `nixpkgs.overlays = [ fermata.overlays.default ];`
      overlays.default = final: _prev: {
        fermata = final.callPackage ./nix/package.nix { };
      };

      checks = forAllSystems (pkgs: {
        # Builds the app and runs the engine's unit tests.
        fermata = self.packages.${pkgs.stdenv.hostPlatform.system}.fermata;
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
