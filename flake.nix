{
  description = "bootvine — live dependency tree of systemd jobs on the boot console";

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
          let
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            };
            craneLib = (crane.mkLib pkgs).overrideToolchain (
              p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml
            );
          in
          f {
            inherit system pkgs craneLib;
            builds = import ./crane-builds.nix { inherit lib pkgs craneLib; };
          }
        );

      sampleMachine = import ./tests/sample-machine {
        inherit lib;
        bootvineModule = self.nixosModules.bootvine;
      };
    in
    {
      packages = forEachSystem (
        { pkgs, builds, ... }:
        {
          default = builds.bootvine;
          inherit (builds) bootvine scripted-boot-examples repo-tasks;
          sample-unit-files = pkgs.callPackage ./tests/sample-machine/sample-unit-files.nix {
            inherit (sampleMachine) sampleUnits;
          };
        }
      );

      nixosModules = {
        default = self.nixosModules.bootvine;
        bootvine = import ./nixos-module.nix { inherit self; };
      };

      nixosConfigurations.sample-machine = sampleMachine.nixosSystemFor "x86_64-linux";

      apps = forEachSystem (
        {
          pkgs,
          system,
          builds,
          ...
        }:
        {
          sample-machine-vm = {
            type = "app";
            meta.description = "Boot a sample machine with bootvine and slow sample units in QEMU";
            program = lib.getExe (sampleMachine.nixosSystemFor system).config.system.build.vm;
          };
          record-screencasts = {
            type = "app";
            meta.description = "Re-record the README screencasts and passphrase screenshot into recordings/";
            program = lib.getExe (
              pkgs.writeShellApplication {
                name = "record-screencasts";
                runtimeInputs = [
                  pkgs.vhs
                  builds.scripted-boot-examples
                  builds.repo-tasks
                ];
                text = "exec xtask record-screencasts --passphrase-png ${
                  self.checks.${system}.vm-luks-passphrase
                }/passphrase.png";
              }
            );
          };
        }
      );

      checks = forEachSystem (
        { pkgs, builds, ... }:
        {
          bootvine-build = builds.bootvine;
          repo-tasks-build = builds.repo-tasks;
          vm-boot-and-shutdown = pkgs.testers.runNixOSTest (
            import ./tests/vm-checks/boot.nix { sampleMachine = sampleMachine.machine; }
          );
          vm-luks-passphrase = pkgs.testers.runNixOSTest (
            import ./tests/vm-checks/luks.nix { bootvineModule = self.nixosModules.bootvine; }
          );
        }
      );

      devShells = forEachSystem (
        { craneLib, builds, ... }:
        {
          default = craneLib.devShell { inputsFrom = [ builds.bootvine ]; };
        }
      );
    };
}
