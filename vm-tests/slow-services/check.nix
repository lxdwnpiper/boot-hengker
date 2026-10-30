{ machine }:
{
  name = "bootvine-boot-and-shutdown";

  nodes.machine =
    { lib, ... }:
    let
      waitFor = flag: "/bin/sh -c 'until [ -e /run/${flag} ]; do sleep 0.1; done'";
    in
    {
      imports = [ machine ];
      testing.initrdBackdoor = true;
      boot.initrd.systemd.services.initrd-hardware-probe.serviceConfig.ExecStart = lib.mkForce (
        waitFor "release-initrd-hardware-probe"
      );
      systemd = {
        settings.Manager.ShowStatus = lib.mkForce true;
        services.sample-database.serviceConfig.ExecStop = lib.mkForce (waitFor "release-sample-database");
        services.backdoor.before = [ "sample-database.service" ];
        services.late-sensor-probe = {
          wantedBy = [ "graphical.target" ];
          after = [ "greetd.service" ];
          serviceConfig = {
            Type = "oneshot";
            ExecStart = waitFor "release-late-sensor-probe";
          };
        };
      };
      virtualisation = {
        memorySize = 2048;
        qemu.options = [
          "-vga none"
          "-device virtio-vga,xres=1280,yres=800"
        ];
      };
    };

  testScript = builtins.readFile ../shared/console.py + builtins.readFile ./check.py;
}
