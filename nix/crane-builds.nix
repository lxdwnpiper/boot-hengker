{
  lib,
  pkgs,
  craneLib,
}:
let
  bootvineCrate = craneLib.crateNameFromCargoToml { cargoToml = ../bootvine/Cargo.toml; };

  commonArgs = {
    inherit (bootvineCrate) pname version;
    src = lib.fileset.toSource {
      root = ../.;
      fileset = lib.fileset.unions [
        (craneLib.fileset.commonCargoSources ../.)
        ../bootvine/system-files
        ../.rustfmt.toml
      ];
    };
    strictDeps = true;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [
      pkgs.dbus
      pkgs.systemd
    ];
  };

  cargoArtifacts = craneLib.buildDepsOnly (commonArgs // { pname = "bootvine-workspace"; });

  buildWorkspaceMember =
    args: craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; } // args);
in
{
  rustfmt-formatting = craneLib.cargoFmt {
    inherit (commonArgs) src pname version;
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
        install -Dm644 bootvine/system-files/systemd/$unit $out/lib/systemd/system/$unit
        substituteInPlace $out/lib/systemd/system/$unit \
          --replace-fail /usr/bin/bootvine $out/bin/bootvine
      done
      resume_drop_in=systemd-hibernate-resume.service.d/bootvine.conf
      install -Dm644 bootvine/system-files/systemd/$resume_drop_in $out/lib/systemd/system/$resume_drop_in
      substituteInPlace $out/lib/systemd/system/$resume_drop_in \
        --replace-fail /usr/bin/bootvine $out/bin/bootvine
      install -Dm755 bootvine/system-files/dracut/90bootvine/module-setup.sh \
        $out/lib/dracut/modules.d/90bootvine/module-setup.sh
      install -Dm644 bootvine/system-files/mkinitcpio/install/sd-bootvine \
        $out/lib/initcpio/install/sd-bootvine
    '';
    meta = {
      mainProgram = "bootvine";
      platforms = lib.platforms.linux;
      license = lib.getLicenseFromSpdxId (lib.importTOML ../bootvine/Cargo.toml).package.license;
    };
  };

  scripted-demos = buildWorkspaceMember {
    inherit (craneLib.crateNameFromCargoToml { cargoToml = ../scripted-demos/Cargo.toml; })
      pname
      version
      ;
    cargoExtraArgs = "-p scripted-demos";
    doCheck = false;
  };

  repo-tasks = buildWorkspaceMember {
    inherit (craneLib.crateNameFromCargoToml { cargoToml = ../xtask/Cargo.toml; }) pname version;
    cargoExtraArgs = "-p xtask";
    doCheck = false;
    meta.mainProgram = "xtask";
  };
}
