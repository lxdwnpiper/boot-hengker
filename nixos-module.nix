{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.boot-tree;
in
{
  options.services.boot-tree = {
    enable = lib.mkEnableOption ''
      a live dependency tree of systemd jobs on the boot console. With a
      systemd initrd it starts there and survives the switch to the real root;
      otherwise it starts in stage 2. It hands the console back once the
      display manager starts or the default target is reached
    '';

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "boot-tree.packages.\${system}.default";
      description = "The boot-tree package, which ships the systemd unit.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    systemd = {
      packages = [ cfg.package ];
      services.boot-tree.wantedBy = [ "sysinit.target" ];
    };

    boot.initrd.systemd = lib.mkIf config.boot.initrd.systemd.enable {
      packages = [ cfg.package ];
      storePaths = [ (lib.getExe cfg.package) ];
      services.boot-tree.wantedBy = [ "sysinit.target" ];
    };
  };
}
