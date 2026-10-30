{ lib, bootvineModule }:
let
  machineModule = machineName: {
    imports = [
      bootvineModule
      ./shared/common.nix
      ./${machineName}/machine.nix
    ];
    networking.hostName = "bootvine-${machineName}";
  };

  machineNames = [
    "slow-services"
    "text-console"
    "scripted-initrd"
    "encrypted-volume"
    "failures"
  ];

  machines = lib.genAttrs machineNames machineModule;
in
{
  inherit machines;

  sampleUnits = import ./slow-services/sample-units.nix;

  nixosSystemFor =
    system: machineName:
    lib.nixosSystem {
      inherit system;
      modules = [
        machines.${machineName}
        (import ./shared/interactive-vm.nix machineName)
      ];
    };

  checksFor =
    pkgs:
    lib.mapAttrs (
      machineName: machine:
      pkgs.testers.runNixOSTest (import ./${machineName}/check.nix { inherit machine pkgs; })
    ) machines;
}
