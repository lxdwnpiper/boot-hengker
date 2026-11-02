{ machine, pkgs, ... }:
{
  name = "bootvine-hibernate-resume";

  nodes.machine =
    { lib, ... }:
    {
      imports = [
        machine
        ../shared/recorded-display.nix
        "${pkgs.path}/nixos/tests/common/auto-format-root-device.nix"
      ];

      boot.resumeDevice = "/dev/vdc";
      swapDevices = lib.mkOverride 0 [
        {
          device = "/dev/vdc";
          options = [ "x-systemd.makefs" ];
        }
      ];

      powerManagement = {
        powerDownCommands = "systemctl --no-block stop backdoor.service";
        resumeCommands = "systemctl --no-block restart backdoor.service";
      };

      virtualisation = {
        emptyDiskImages = [ 4096 ];
        useNixStoreImage = true;
        useEFIBoot = true;
      };
    };

  testScript =
    builtins.readFile ../shared/console.py
    + builtins.readFile ../shared/screen_recording.py
    + builtins.readFile ./check.py;
}
