{
  boot.loader.grub.enable = false;

  services.getty.autologinUser = "root";  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
  };

  virtualisation.vmVariant.virtualisation = {
    memorySize = 2048;
    cores = 2;
    graphics = true;
    qemu.options = [
      "-vga none"
      "-device virtio-vga,xres=1920,yres=1080"
    ];
  };
}
