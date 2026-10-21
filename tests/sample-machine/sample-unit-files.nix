{
  sampleUnits,
  lib,
  coreutils,
  runCommand,
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

  serviceNames = map (name: "${name}.service") (lib.attrNames sampleUnits.services);

  targetFile = writeText "${sampleUnits.target}.target" (toUnitFile {
    Unit = {
      Description = "bootvine demo services";
      Wants = serviceNames;
      After = serviceNames;
    };
  });
  unitFiles =
    lib.mapAttrsToList (name: spec: {
      name = "${name}.service";
      path = serviceFile name spec;
    }) sampleUnits.services
    ++ [
      {
        name = "${sampleUnits.target}.target";
        path = targetFile;
      }
    ];
in
runCommand "bootvine-sample-unit-files" { } (
  lib.concatMapStringsSep "\n" (unit: "install -Dm644 ${unit.path} $out/${unit.name}") unitFiles
)
