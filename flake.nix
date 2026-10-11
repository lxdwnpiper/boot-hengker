{
  description = "boot-tree — live dependency tree of systemd jobs on the boot console";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
    }:
    let
      inherit (nixpkgs) lib;

      forEachSystem =
        f:
        lib.genAttrs [ "x86_64-linux" "aarch64-linux" ] (
          system:
          f rec {
            inherit system;
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            };
            craneLib = (crane.mkLib pkgs).overrideToolchain (
              p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml
            );
          }
        );

      buildInputsFor = pkgs: {
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.dbus ];
      };

      demoUnits = import ./demo/units.nix;

      demoHost = {
        imports = [
          self.nixosModules.default
          ./demo/host.nix
        ];
        _module.args = { inherit demoUnits; };
      };

      demoSystem =
        system:
        lib.nixosSystem {
          inherit system;
          modules = [
            demoHost
            ./demo/interactive.nix
          ];
        };
    in
    {
      packages = forEachSystem (
        { pkgs, craneLib, ... }:
        {
          default = craneLib.buildPackage {
            inherit (buildInputsFor pkgs) nativeBuildInputs buildInputs;
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
            meta.mainProgram = "boot-tree";
            meta.platforms = lib.platforms.linux;
          };
          demo-units = pkgs.callPackage ./demo/user-units.nix { inherit demoUnits; };
        }
      );

      nixosModules.default = import ./nixos-module.nix { inherit self; };

      nixosConfigurations.demo = demoSystem "x86_64-linux";

      apps = forEachSystem (
        { system, ... }:
        {
          vm = {
            type = "app";
            program = lib.getExe (demoSystem system).config.system.build.vm;
          };
          demo-units = {
            type = "app";
            program = lib.getExe self.packages.${system}.demo-units;
          };
        }
      );

      checks = forEachSystem (
        { pkgs, system, ... }:
        {
          package = self.packages.${system}.default;
          boot = pkgs.testers.runNixOSTest (import ./demo/test.nix { inherit demoHost; });
        }
      );

      devShells = forEachSystem (
        { pkgs, craneLib, ... }:
        {
          default = craneLib.devShell {
            packages = with buildInputsFor pkgs; nativeBuildInputs ++ buildInputs;
          };
        }
      );
    };
}
