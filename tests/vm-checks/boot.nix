{ sampleMachine }:
{
  name = "bootvine-boot-and-shutdown";

  nodes.machine =
    { lib, ... }:
    let
      waitFor = flag: "/bin/sh -c 'until [ -e /run/${flag} ]; do sleep 0.1; done'";
    in
    {
      imports = [ sampleMachine ];
      testing.initrdBackdoor = true;
      boot.initrd.systemd.services.demo-initrd-probe.serviceConfig.ExecStart = lib.mkForce (
        waitFor "release-initrd-probe"
      );
      systemd = {
        settings.Manager.ShowStatus = lib.mkForce true;
        services.demo-db.serviceConfig.ExecStop = lib.mkForce (waitFor "release-demo-db");
        services.backdoor.before = [ "demo-db.service" ];
      };
      virtualisation = {
        memorySize = 2048;
        qemu.options = [
          "-vga none"
          "-device virtio-vga,xres=1280,yres=800"
        ];
      };
    };

  testScript = builtins.readFile ./console.py + builtins.readFile ./boot.py;
}
