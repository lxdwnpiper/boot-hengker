{
  demoUnits,
  lib,
  pkgs,
  ...
}:
let
  demoService = spec: {
    after = map (dependency: "${dependency}.service") (spec.after or [ ]);
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      ExecStart = [
        "${pkgs.coreutils}/bin/sleep ${spec.seconds}"
      ]
      ++ lib.optional (spec.fails or false) "${pkgs.coreutils}/bin/false";
    };
  };
in
{
  networking.hostName = "bootvine-demo";
  system.stateVersion = "26.05";

  boot.initrd.systemd = {
    enable = true;
    services.demo-initrd-probe = {
      description = "bootvine demo: slow initrd step";
      wantedBy = [ "initrd.target" ];
      before = [ "initrd.target" ];
      unitConfig.DefaultDependencies = false;
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "/bin/sleep ${demoUnits.initrdProbeSeconds}";
      };
    };
  };

  services.bootvine = {
    enable = true;
    palette.running = "light-yellow";
  };

  console = {
    colors = [
      "1d2021"
      "fb4934"
      "b8bb26"
      "fabd2f"
      "83a598"
      "d3869b"
      "8ec07c"
      "d5c4a1"
      "665c54"
      "fb4934"
      "b8bb26"
      "fabd2f"
      "83a598"
      "d3869b"
      "8ec07c"
      "fbf1c7"
    ];
    earlySetup = true;
    font = "${pkgs.terminus_font}/share/consolefonts/ter-v20n.psf.gz";
  };

  systemd.services = lib.mapAttrs (_: demoService) demoUnits.services // {
    display-manager.after = [ "multi-user.target" ];
  };

  services.greetd = {
    enable = true;
    settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd ${pkgs.bashInteractive}/bin/bash";
  };

  users.users.demo = {
    isNormalUser = true;
    password = "demo";
  };
}
