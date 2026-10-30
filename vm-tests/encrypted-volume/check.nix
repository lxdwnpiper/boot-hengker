{ machine, pkgs }:
let
  secretDisk = import ./secret-disk.nix { inherit pkgs; };
in
{
  name = "bootvine-encrypted-volume";
  nodes.machine = {
    imports = [
        machine
        ../shared/recorded-display.nix
      ];

    testing.initrdBackdoor = true;

    virtualisation.qemu.drives = [ secretDisk.qemuDrive ];
  };

  testScript = ''
    PASSPHRASE = "${secretDisk.passphrase}"
  ''
  + builtins.readFile ../shared/console.py
    + builtins.readFile ../shared/screen_recording.py
  + builtins.readFile ./check.py;
}
