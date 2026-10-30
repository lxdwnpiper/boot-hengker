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

  checksFor = pkgs: {
    vm-boot-and-shutdown = pkgs.testers.runNixOSTest (
      import ./slow-services/check.nix { machine = machines.slow-services; }
    );
    vm-luks-passphrase = pkgs.testers.runNixOSTest (
      import ./encrypted-volume/check.nix {
        machine = machines.encrypted-volume;
        inherit pkgs;
      }
    );
    vm-failures = pkgs.testers.runNixOSTest (
      import ./failures/check.nix { machine = machines.failures; }
    );
  };
}
