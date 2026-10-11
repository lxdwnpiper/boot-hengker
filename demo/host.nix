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
  networking.hostName = "boot-tree-demo";
  system.stateVersion = "26.05";

  boot.initrd.systemd = {
    enable = true;
    services.demo-initrd-probe = {
      description = "boot-tree demo: slow initrd step";
      wantedBy = [ "initrd.target" ];
      before = [ "initrd.target" ];
      unitConfig.DefaultDependencies = false;
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "/bin/sleep ${demoUnits.initrdProbeSeconds}";
      };
    };
  };

  services.boot-tree = {
    enable = true;
    consoleFont = "ter-v20n";
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
