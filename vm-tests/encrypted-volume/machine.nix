{ lib, pkgs, ... }:
let
  secretDisk = import ./secret-disk.nix { inherit pkgs; };
in
{
  boot.initrd.luks.devices = lib.mkVMOverride {
    secret.device = "/dev/disk/by-uuid/${secretDisk.uuid}";
  };

  virtualisation.vmVariant.virtualisation.qemu.drives = [ secretDisk.qemuDrive ];
}
