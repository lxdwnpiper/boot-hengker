{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.bootvine;
  chosenColors = lib.filterAttrs (_: color: color != null) cfg.palette;
  configFile = (pkgs.formats.toml { }).generate "bootvine.toml" { palette = chosenColors; };
  colorOption =
    role: default:
    lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "light-yellow";
      description = ''
        Colour for ${role}; unset keeps `${default}`. One of the 16 console
        colour names (`red`, `light-red`, `dark-gray`, …). What each name
        looks like comes from the console palette, {option}`console.colors`.
      '';
    };
in
{
  options.services.bootvine = {
    enable = lib.mkEnableOption ''
      a live dependency tree of systemd jobs on the boot console. With a
      systemd initrd it starts there and survives the switch to the real root;
      otherwise it starts in stage 2. It hands the console back once the
      display manager starts or the default target is reached, and comes back
      for shutdown until systemd hands over to systemd-shutdown
    '';

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "bootvine.packages.\${system}.default";
      description = "The bootvine package, which ships the systemd unit.";
    };

    palette = {
      text = colorOption "running units and headings" "white";
      muted = colorOption "rules, tree lines and the time axis" "dark-gray";
      waiting = colorOption "queued units and finished-unit labels" "gray";
      running = colorOption "running units and their bars" "yellow";
      done = colorOption "finished units, stages and milestones" "green";
      failed = colorOption "failed units" "light-red";
      target = colorOption "targets in the tree" "cyan";
    };
  };

  config = lib.mkIf cfg.enable {
    environment = {
      systemPackages = [ cfg.package ];
      etc."bootvine.toml" = lib.mkIf (chosenColors != { }) { source = configFile; };
    };

    systemd = {
      packages = [ cfg.package ];
      services.bootvine.wantedBy = [ "sysinit.target" ];
      services.bootvine-shutdown.wantedBy = [ "shutdown.target" ];
    };

    boot.initrd.systemd = lib.mkIf config.boot.initrd.systemd.enable {
      packages = [ cfg.package ];
      storePaths = [ (lib.getExe cfg.package) ];
      services.bootvine.wantedBy = [ "sysinit.target" ];
      contents."/etc/bootvine.toml" = lib.mkIf (chosenColors != { }) { source = configFile; };
    };
  };
}
