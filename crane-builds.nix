{
  lib,
  pkgs,
  craneLib,
}:
let
  commonArgs = {
    src = lib.fileset.toSource {
      root = ./.;
      fileset = lib.fileset.unions [
        (craneLib.fileset.commonCargoSources ./.)
        ./system-files
        ./.rustfmt.toml
      ];
    };
    strictDeps = true;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.dbus ];
  };

  cargoArtifacts = craneLib.buildDepsOnly (commonArgs // { pname = "bootvine-workspace"; });

  buildWorkspaceMember =
    args: craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; } // args);
in
{
  rustfmt-formatting = craneLib.cargoFmt {
    inherit (commonArgs) src;
    cargoExtraArgs = "--all";
  };

  clippy-lints = craneLib.cargoClippy (
    commonArgs
    // {
      inherit cargoArtifacts;
      cargoClippyExtraArgs = "--workspace --all-targets -- --deny warnings";
    }
  );

  bootvine = buildWorkspaceMember {
    cargoExtraArgs = "-p bootvine";
    postInstall = ''
      for unit in bootvine.service bootvine-shutdown.service; do
        install -Dm644 system-files/systemd/$unit $out/lib/systemd/system/$unit
        substituteInPlace $out/lib/systemd/system/$unit \
          --replace-fail /usr/bin/bootvine $out/bin/bootvine
      done
      install -Dm755 system-files/dracut/90bootvine/module-setup.sh \
        $out/lib/dracut/modules.d/90bootvine/module-setup.sh
      install -Dm644 system-files/mkinitcpio/install/sd-bootvine \
        $out/lib/initcpio/install/sd-bootvine
    '';
    meta = {
      mainProgram = "bootvine";
      platforms = lib.platforms.linux;
      license = lib.getLicenseFromSpdxId (lib.importTOML ./Cargo.toml).package.license;
    };
  };

  scripted-boot-examples = buildWorkspaceMember {
    pname = "bootvine-scripted-boot-examples";
    cargoExtraArgs = "-p bootvine --examples";
    doCheck = false;
    installPhaseCommand = ''
      for example in boot shutdown; do
        install -Dm755 target/release/examples/$example $out/bin/bootvine-$example-example
      done
    '';
  };

  repo-tasks = buildWorkspaceMember {
    pname = "xtask";
    cargoExtraArgs = "-p xtask";
    doCheck = false;
    meta.mainProgram = "xtask";
  };
}
