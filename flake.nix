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
            builds = import ./nix/crane-builds.nix { inherit lib pkgs craneLib; };
          }
        );

      vmTests = import ./vm-tests {
        inherit lib;
        bootvineModule = self.nixosModules.bootvine;
      };
    in
    {
      packages = forEachSystem (
        { pkgs, builds, ... }:
        {
          default = builds.bootvine;
          inherit (builds) bootvine scripted-demos repo-tasks;
          workspace-dependencies = builds.cargoArtifacts;
          sample-unit-files = pkgs.callPackage ./vm-tests/slow-services/sample-unit-files.nix {
            inherit (vmTests) sampleUnits;
          };
        }
      );

      nixosModules = {
        default = self.nixosModules.bootvine;
        bootvine = import ./nix/nixos-module.nix { inherit self; };
      };

      nixosConfigurations = lib.mapAttrs' (
        machineName: _:
        lib.nameValuePair "sample-${machineName}" (vmTests.nixosSystemFor "x86_64-linux" machineName)
      ) vmTests.machines;

      apps = forEachSystem (
        {
          pkgs,
          system,
          builds,
          rustToolchain,
          ...
        }:
        let
          mkApp = drv: {
            type = "app";
            program = lib.getExe drv;
          };

          repoTaskApp =
            name: description: xtaskArguments: runtimeInputs:
            mkApp (
              pkgs.runCommand name
                {
                  nativeBuildInputs = [ pkgs.makeBinaryWrapper ];
                  meta.mainProgram = name;
                }
                ''
                  makeWrapper ${lib.getExe builds.repo-tasks} $out/bin/${name} \
                    --add-flags ${lib.escapeShellArg xtaskArguments} \
                    --prefix PATH : ${lib.makeBinPath runtimeInputs}
                ''
            )
            // {
              meta.description = description;
            };

          sampleMachineApps = lib.mapAttrs' (
            machineName: machineDescription:
            lib.nameValuePair "sample-vm-${machineName}" {
              type = "app";
              meta.description = "Boot the ${machineName} sample machine in QEMU: ${machineDescription}";
              program = lib.getExe (vmTests.nixosSystemFor system machineName).config.system.build.vm;
            }
          ) vmTests.machineDescriptions;

          ciTaskApps = import ./nix/ci-tasks.nix {
            inherit pkgs mkApp;
            cargoToolchain = rustToolchain;
            preCommitInputs = [ rustToolchain ];
            extraReleaseInputs = [
              pkgs.pkg-config
              pkgs.stdenv.cc
            ];
            releaseWrapperArgs = [
              "--set"
              "PKG_CONFIG_PATH"
              (lib.makeSearchPathOutput "dev" "lib/pkgconfig" builds.bootvine.buildInputs)
            ];
          };
        in
        sampleMachineApps
        // ciTaskApps
        // {
          record-screencasts =
            repoTaskApp "record-screencasts"
              "Re-record the scripted demo animations and the sample machine recordings into recordings/; publish them with publish-recordings"
              "record-screencasts --vm-check-outputs ${
                pkgs.linkFarm "vm-check-outputs" (
                  lib.mapAttrs (machineName: _: self.checks.${system}."vm-${machineName}") vmTests.machines
                )
              }"
              [
                pkgs.vhs
                pkgs.libwebp
                builds.scripted-demos
              ];

          publish-recordings =
            repoTaskApp "publish-recordings"
              "Replace the recordings branch on origin with a single commit holding recordings/"
              "publish-recordings"
              [ pkgs.gitMinimal ];
        }
      );

      checks = forEachSystem (
        { pkgs, builds, ... }:
        {
          inherit (builds) clippy-lints;
          bootvine-build = builds.bootvine;
          repo-tasks-build = builds.repo-tasks;
        }
        // lib.mapAttrs' (machineName: check: lib.nameValuePair "vm-${machineName}" check) (
          vmTests.checksFor pkgs
        )
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
