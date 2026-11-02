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

  machineDescriptions = {
    slow-services = "slow and failing units, a held initrd probe and a greetd login";
    text-console = "the slow units without a display manager, handing over at the default target";
    scripted-initrd = "the slow units with the scripted initrd, so bootvine starts in stage 2";
    encrypted-volume = "a LUKS volume unlocked in the initrd with the passphrase 'sample passphrase'";
    failures = "units that fail in every way systemd reports, each with journal messages";
    hibernate-resume = "a swap disk to hibernate to, showing the kernel's image loading progress on resume";
  };

  machines = lib.mapAttrs (machineName: _: machineModule machineName) machineDescriptions;
in
{
  inherit machines machineDescriptions;

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
