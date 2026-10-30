{ lib, pkgs, ... }:
let
  sampleUnits = import ./sample-units.nix;

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
  boot.initrd.systemd.services.initrd-hardware-probe = {
    description = "Slow hardware probe";
    wantedBy = [ "initrd.target" ];
    before = [ "initrd.target" ];
    unitConfig.DefaultDependencies = false;
    serviceConfig = {
      Type = "oneshot";
      ExecStart = "/bin/sleep ${sampleUnits.initrdProbeSeconds}";
    };
  };

  systemd.services = lib.mapAttrs (_: sampleService) sampleUnits.services // {
    greetd.after = [ "multi-user.target" ];
  };

  services.greetd = {
    enable = true;
    settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd ${pkgs.bashInteractive}/bin/bash";
  };
}
