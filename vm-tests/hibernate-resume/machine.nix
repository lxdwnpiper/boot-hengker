{ lib, ... }:
{
  virtualisation.vmVariant = {
    boot.resumeDevice = "/dev/vdb";
    swapDevices = lib.mkOverride 0 [
      {
        device = "/dev/vdb";
        options = [ "x-systemd.makefs" ];
      }
    ];
    virtualisation.emptyDiskImages = [ 4096 ];
  };
}
