{ lib, ... }:
{
  imports = [ ../slow-services/machine.nix ];

  boot.initrd.systemd.enable = lib.mkForce false;
}
