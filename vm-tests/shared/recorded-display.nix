{
  virtualisation = {
    memorySize = 2048;
    qemu.options = [
      "-vga none"
      "-device virtio-vga,xres=1280,yres=800"
      "-chardev socket,id=recording-monitor,path=recording-monitor,server=on,wait=off"
      "-mon chardev=recording-monitor,mode=readline"
    ];
  };
}
