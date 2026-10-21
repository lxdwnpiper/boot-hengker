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
        let
          commonArgs = {
            inherit (buildInputsFor pkgs) nativeBuildInputs buildInputs;
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                (craneLib.fileset.commonCargoSources ./.)
                ./packaging
              ];
            };
            strictDeps = true;
          };
        in
        {
          examples = craneLib.buildPackage (
            commonArgs
            // {
              pname = "bootvine-examples";
              cargoExtraArgs = "--examples";
              doCheck = false;
              installPhaseCommand = ''
                for example in boot shutdown; do
                  install -Dm755 target/release/examples/$example $out/bin/bootvine-$example-example
                done
              '';
            }
          );
          default = craneLib.buildPackage (
            commonArgs
            // {
              postInstall = ''
                for unit in bootvine.service bootvine-shutdown.service; do
                  install -Dm644 packaging/systemd/$unit $out/lib/systemd/system/$unit
                  substituteInPlace $out/lib/systemd/system/$unit \
                    --replace-fail /usr/bin/bootvine $out/bin/bootvine
                done
                install -Dm755 packaging/dracut/90bootvine/module-setup.sh \
                  $out/lib/dracut/modules.d/90bootvine/module-setup.sh
                install -Dm644 packaging/mkinitcpio/install/sd-bootvine \
                  $out/lib/initcpio/install/sd-bootvine
              '';
              meta.mainProgram = "bootvine";
              meta.platforms = lib.platforms.linux;
              meta.license = lib.getLicenseFromSpdxId (lib.importTOML ./Cargo.toml).package.license;
            }
          );
          demo-units = pkgs.callPackage ./demo/user-units.nix { inherit demoUnits; };
        }
      );

      nixosModules.default = import ./nixos-module.nix { inherit self; };

      nixosConfigurations.demo = demoSystem "x86_64-linux";

      apps = forEachSystem (
        { pkgs, system, ... }:
        {
          vm = {
            type = "app";
            program = lib.getExe (demoSystem system).config.system.build.vm;
          };
          demo-units = {
            type = "app";
            program = lib.getExe self.packages.${system}.demo-units;
          };
          docs = {
            type = "app";
            program = lib.getExe (
              pkgs.writeShellApplication {
                name = "bootvine-docs";
                runtimeInputs = [
                  pkgs.vhs
                  self.packages.${system}.examples
                ];
                text = ''
                  if [ ! -d docs ]; then
                    echo "no docs directory here; run from the repository root" >&2
                    exit 1
                  fi
                  vhs ${./docs/boot.tape}
                  vhs ${./docs/shutdown.tape}
                  install -m644 -t docs ${self.checks.${system}.luks}/passphrase.png
                '';
              }
            );
          };
        }
      );

      checks = forEachSystem (
        { pkgs, system, ... }:
        {
          package = self.packages.${system}.default;
          boot = pkgs.testers.runNixOSTest (import ./tests/boot.nix { inherit demoHost; });
          luks = pkgs.testers.runNixOSTest (
            import ./tests/luks.nix { bootvineModule = self.nixosModules.default; }
          );
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
