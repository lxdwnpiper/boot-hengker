{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.bootvine;
  chosen = lib.filterAttrs (_: value: value != null);
  settings = lib.filterAttrs (_: table: table != { }) {
    palette = chosen cfg.palette;
    timing = chosen cfg.timing;
  };
  configFile = (pkgs.formats.toml { }).generate "bootvine.toml" settings;
  durationOption =
    purpose: default:
    lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "500ms";
      description = ''
        ${purpose}; unset keeps `${default}`. A duration such as `2s`,
        `500ms` or `1m 30s`.
      '';
    };
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

    timing = {
      keep_finished_units_for = durationOption "How long a finished unit stays in the tree, faded, before it leaves" "2s";
      hide_runs_shorter_than = durationOption "Runs shorter than this get no timer in the tree and no bar on the timeline" "1s";
      keep_final_frame_for = durationOption "How long the last frame stays on screen before the console is handed back" "1s";
    };
  };

  config = lib.mkIf cfg.enable {
    environment = {
      systemPackages = [ cfg.package ];
      etc."bootvine.toml" = lib.mkIf (settings != { }) { source = configFile; };
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
      contents."/etc/bootvine.toml" = lib.mkIf (settings != { }) { source = configFile; };
    };
  };
}
