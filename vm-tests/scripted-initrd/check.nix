{ machine, ... }:
{
  name = "bootvine-scripted-initrd";

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
      systemd = {
        settings.Manager.ShowStatus = lib.mkForce true;
        services = {
          sample-database.serviceConfig.ExecStop = lib.mkForce (waitFor "release-sample-database");
          backdoor.before = [ "sample-database.service" ];
          late-sensor-probe = {
            wantedBy = [ "graphical.target" ];
            after = [ "greetd.service" ];
            serviceConfig = {
              Type = "oneshot";
              ExecStart = waitFor "release-late-sensor-probe";
            };
          };
        };
      };
    };

  testScript =
    builtins.readFile ../shared/console.py
    + builtins.readFile ../shared/screen_recording.py
    + builtins.readFile ./check.py;
}
