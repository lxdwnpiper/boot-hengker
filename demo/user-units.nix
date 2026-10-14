{
  demoUnits,
  lib,
  coreutils,
  linkFarm,
  nushell,
  writeShellApplication,
  writeText,
}:
let
  toUnitFile = lib.generators.toINI { listsAsDuplicateKeys = true; };

  serviceFile =
    name: spec:
    writeText "${name}.service" (toUnitFile {
      Unit = {
        Description = spec.description;
        After = map (dependency: "${dependency}.service") (spec.after or [ ]);
      };
      Service = {
        Type = "oneshot";
        ExecStart = [
          "${coreutils}/bin/sleep ${spec.seconds}"
        ]
        ++ lib.optional (spec.fails or false) "${coreutils}/bin/false";
      }
      // lib.optionalAttrs (spec ? stopSeconds) {
        RemainAfterExit = true;
        ExecStop = "${coreutils}/bin/sleep ${spec.stopSeconds}";
      };
    });

  serviceNames = map (name: "${name}.service") (lib.attrNames demoUnits.services);

  targetFile = writeText "${demoUnits.target}.target" (toUnitFile {
    Unit = {
      Description = "bootvine demo services";
      Wants = serviceNames;
      After = serviceNames;
    };
  });

  units = linkFarm "bootvine-demo-units" (
    lib.mapAttrsToList (name: spec: {
      name = "${name}.service";
      path = serviceFile name spec;
    }) demoUnits.services
    ++ [
      {
        name = "${demoUnits.target}.target";
        path = targetFile;
      }
    ]
  );
in
writeShellApplication {
  name = "bootvine-demo-units";
  text = ''exec ${lib.getExe nushell} ${./demo-units.nu} "$@" --units ${units} --target ${demoUnits.target}.target'';
}
