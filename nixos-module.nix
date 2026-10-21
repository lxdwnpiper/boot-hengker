{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.bootvine;
  settingsFormat = pkgs.formats.toml { };
  uncheckedConfigFile = settingsFormat.generate "bootvine-unchecked.toml" cfg.settings;
  canRunPackage = pkgs.stdenv.buildPlatform.canExecute pkgs.stdenv.hostPlatform;
  configFile = pkgs.runCommand "bootvine.toml" { } ''
    ${lib.optionalString canRunPackage "${lib.getExe cfg.package} check ${uncheckedConfigFile}"}
    cp ${uncheckedConfigFile} $out
  '';
  hasSettings = cfg.settings != { };
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

    settings = lib.mkOption {
      inherit (settingsFormat) type;
      default = { };
      example = {
        colors.tree.running-marker = "light-yellow";
        timing.keep-final-frame-for = "500ms";
      };
      description = ''
        Written to {file}`/etc/bootvine.toml`, in the real root and the
        initrd. `bootvine defaults --format nix` prints every key with its
        default; the build fails if `bootvine check` rejects the result.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment = {
      systemPackages = [ cfg.package ];
      etc."bootvine.toml" = lib.mkIf hasSettings { source = configFile; };
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
      contents."/etc/bootvine.toml" = lib.mkIf hasSettings { source = configFile; };
    };
  };
}
