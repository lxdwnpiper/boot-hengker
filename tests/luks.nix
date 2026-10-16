{ bootvineModule }:
let
  passphrase = "quietly unlocked";
in
{
  name = "bootvine-luks";
  nodes.machine =
    { lib, pkgs, ... }:
    let
      secretUuid = "9d1f2a8e-0b7c-4c55-9a8e-6b2f8f3c1d00";
      secretDisk = pkgs.runCommand "bootvine-luks-disk" { nativeBuildInputs = [ pkgs.cryptsetup ]; } ''
        truncate -s 32M disk.img
        echo -n '${passphrase}' | cryptsetup luksFormat -q --disable-locks --type luks2 \
          --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --uuid ${secretUuid} disk.img -
        mv disk.img $out
      '';
    in
    {
      imports = [ bootvineModule ];

      services.bootvine.enable = true;

      testing.initrdBackdoor = true;
      boot.initrd.systemd.enable = true;
      boot.initrd.luks.devices = lib.mkVMOverride {
        secret.device = "/dev/disk/by-uuid/${secretUuid}";
      };

      virtualisation.qemu.options = [
        "-vga none"
        "-device virtio-vga,xres=1280,yres=800"
      ];
      virtualisation.qemu.drives = [
        {
          name = "secret";
          file = "${secretDisk}";
          driveExtraOpts = {
            format = "raw";
            snapshot = "on";
          };
        }
      ];
    };

  testScript = ''
    PASSPHRASE = "${passphrase}"
  ''
  + builtins.readFile ./console.py
  + builtins.readFile ./luks.py;
}
