{
  sampleUnits,
  lib,
  pkgs,
  ...
}:
let
  sampleService = spec: {
    inherit (spec) description;
    after = map (dependency: "${dependency}.service") (spec.after or [ ]);
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      ExecStart = [
        "${pkgs.coreutils}/bin/sleep ${spec.seconds}"
      ]
      ++ lib.optional (spec.fails or false) "${pkgs.coreutils}/bin/false";
    }
    // lib.optionalAttrs (spec ? stopSeconds) {
      RemainAfterExit = true;
      ExecStop = "${pkgs.coreutils}/bin/sleep ${spec.stopSeconds}";
    };
  };
in
{
  networking.hostName = "bootvine-demo";
  system.stateVersion = "26.05";

  boot.consoleLogLevel = lib.mkForce 3;

  boot.initrd.systemd = {
    services.demo-initrd-probe = {
      description = "Slow hardware probe";
      wantedBy = [ "initrd.target" ];
      before = [ "initrd.target" ];
      unitConfig.DefaultDependencies = false;
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "/bin/sleep ${sampleUnits.initrdProbeSeconds}";
      };
    };
  };

  services.bootvine = {
    enable = true;
    settings.colors = {
      tree.current-stage = "light-yellow";
      tree.running-marker = "light-yellow";
      timeline.running = "light-yellow";
      footer.running = "light-yellow";
      password.frame = "light-yellow";
    };
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

  systemd.services = lib.mapAttrs (_: sampleService) sampleUnits.services // {
    greetd.after = [ "multi-user.target" ];
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
