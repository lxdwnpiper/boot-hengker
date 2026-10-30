{ machine }:
{
  name = "bootvine-failures";

  nodes.machine =
    { pkgs, ... }:
    let
      holdUntilReleased =
        marker: "${pkgs.bash}/bin/bash -c 'until [ -e /run/${marker} ]; do sleep 0.1; done'";
    in
    {
      imports = [ machine ];
      bootvineSample.startFailuresByHand = true;
      testing.initrdBackdoor = true;

      boot.initrd.systemd = {
        storePaths = [ "${pkgs.bash}/bin/bash" ];
        services.initrd-failures-hold = {
          description = "Initrd held for the check";
          wantedBy = [ "initrd.target" ];
          before = [ "initrd.target" ];
          unitConfig.DefaultDependencies = false;
          serviceConfig = {
            Type = "oneshot";
            ExecStart = holdUntilReleased "release-initrd-failures-hold";
          };
        };
      };

      systemd.services.boot-failures-hold = {
        description = "Boot held for the check";
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          Type = "oneshot";
          ExecStart = holdUntilReleased "release-boot-failures-hold";
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
