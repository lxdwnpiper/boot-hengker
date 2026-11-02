{ lib, ... }:
{
  boot.initrd.availableKernelModules = [ "bochs" ];

  virtualisation.vmVariant = {
    boot.resumeDevice = "/dev/vdb";
    swapDevices = lib.mkOverride 0 [
      {
        device = "/dev/vdb";
        options = [ "x-systemd.makefs" ];
      }
    ];
    virtualisation = {
      emptyDiskImages = [ 4096 ];
      qemu.options = lib.mkForce [
        "-vga none"
        "-device VGA,xres=1920,yres=1080"
      ];
    };
  };
}
