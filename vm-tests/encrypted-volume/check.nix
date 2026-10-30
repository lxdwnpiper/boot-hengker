{ machine, pkgs }:
let
  secretDisk = import ./secret-disk.nix { inherit pkgs; };
in
{
  name = "bootvine-luks-passphrase";
  nodes.machine = {
    imports = [ machine ];

    testing.initrdBackdoor = true;

    virtualisation.qemu.options = [
      "-vga none"
      "-device virtio-vga,xres=1280,yres=800"
    ];
    virtualisation.qemu.drives = [ secretDisk.qemuDrive ];
  };

  testScript = ''
    PASSPHRASE = "${secretDisk.passphrase}"
  ''
  + builtins.readFile ../shared/console.py
  + builtins.readFile ./check.py;
}
