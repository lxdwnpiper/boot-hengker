{ lib, ... }:
{
  imports = [ ../slow-services/machine.nix ];

  services.greetd.enable = lib.mkForce false;
}
