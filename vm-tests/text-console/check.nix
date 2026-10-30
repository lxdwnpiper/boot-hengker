{ machine, ... }:
{
  name = "bootvine-text-console";

  nodes.machine =
    { lib, ... }:
    let
      waitFor = flag: "/bin/sh -c 'until [ -e /run/${flag} ]; do sleep 0.1; done'";
    in
    {
      imports = [
        machine
        ../shared/recorded-display.nix
      ];
      testing.initrdBackdoor = true;
      boot.initrd.systemd.services.initrd-hardware-probe.serviceConfig.ExecStart = lib.mkForce (
        waitFor "release-initrd-hardware-probe"
      );
      systemd.services = {
        sample-database.serviceConfig.ExecStop = lib.mkForce (waitFor "release-sample-database");
        backdoor.before = [ "sample-database.service" ];
      };
    };

  testScript =
    builtins.readFile ../shared/console.py
    + builtins.readFile ../shared/screen_recording.py
    + builtins.readFile ./check.py;
}
