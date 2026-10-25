{
  description = "bootvine — live dependency tree of systemd jobs on the boot console";

  nixConfig = {
    extra-substituters = [ "https://wvhulle.cachix.org" ];
    extra-trusted-public-keys = [
      "wvhulle.cachix.org-1:heXx8DZMiRsKUx6l1TxNoF+Nmtmz66QEdsonQzc1ir0="
    ];
  };

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
            rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
            craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
          in
          f {
            inherit
              system
              pkgs
              craneLib
              rustToolchain
              ;
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
          rustToolchain,
          ...
        }:
        let
          runReleasePlzOnCodeberg = subcommand: description: {
            type = "app";
            meta.description = description;
            program = lib.getExe (
              pkgs.writeShellApplication {
                name = "release-plz-${subcommand}";
                runtimeInputs = [
                  pkgs.release-plz
                  pkgs.gitMinimal
                  pkgs.pkg-config
                  pkgs.stdenv.cc
                  rustToolchain
                ];
                text = ''
                  export PKG_CONFIG_PATH="${lib.getDev pkgs.dbus}/lib/pkgconfig"
                  exec release-plz ${subcommand} --forge gitea --git-token "$CODEBERG_TOKEN"
                '';
              }
            );
          };
        in
        {
          release-plz-release = runReleasePlzOnCodeberg "release" "Publish unpublished versions to crates.io and tag them with a Codeberg release";
          release-plz-release-pr = runReleasePlzOnCodeberg "release-pr" "Open or update the Codeberg pull request that bumps the version and changelog";
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
          build-and-push-to-cachix = {
            type = "app";
            meta.description = "Build flake outputs and push everything built to the wvhulle Cachix cache";
            program = lib.getExe (
              pkgs.writeShellApplication {
                name = "build-and-push-to-cachix";
                runtimeInputs = [ pkgs.cachix ];
                text = ''exec cachix watch-exec wvhulle -- nix build --print-build-logs "$@"'';
              }
            );
          };
          check-conventional-commits = {
            type = "app";
            meta.description = "Check that the commit messages in a git revision range are conventional commits";
            program = lib.getExe (
              pkgs.writeShellApplication {
                name = "check-conventional-commits";
                runtimeInputs = [
                  pkgs.convco
                  pkgs.gitMinimal
                ];
                text = ''exec convco check "$@"'';
              }
            );
          };
        }
      );

      checks = forEachSystem (
        { pkgs, builds, ... }:
        {
          inherit (builds) rustfmt-formatting clippy-lints;
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
        {
          pkgs,
          craneLib,
          builds,
          ...
        }:
        {
          default = craneLib.devShell {
            inputsFrom = [ builds.bootvine ];
            packages = [
              pkgs.prek
              pkgs.convco
              pkgs.release-plz
              pkgs.git-cliff
            ];
          };
        }
      );
    };
}
