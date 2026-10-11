{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.boot-tree;

  unit = {
    description = "Live dependency tree of boot jobs on ${cfg.tty}";
    wantedBy = [ "sysinit.target" ];
    after = [ "systemd-vconsole-setup.service" ];
    before = [
      "display-manager.service"
      "shutdown.target"
    ];
    conflicts = [ "shutdown.target" ];
    unitConfig = {
      DefaultDependencies = false;
      IgnoreOnIsolate = true;
    };
    serviceConfig = {
      Type = "simple";
      ExecStart = "@${lib.getExe cfg.package} @boot-tree --hold-ms ${toString cfg.holdMs}";
      StandardInput = "tty";
      StandardOutput = "tty";
      StandardError = "journal";
      TTYPath = cfg.tty;
      TTYReset = true;
      TTYVTDisallocate = false;
    };
  };
in
{
  options.services.boot-tree = {
    enable = lib.mkEnableOption ''
      a live dependency tree of systemd jobs on the boot console. It starts in
      the systemd initrd, survives the switch to the real root, and hands the
      console over once the display manager starts or the default target is
      reached
    '';

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "boot-tree.packages.\${system}.default";
      description = "The boot-tree package.";
    };

    tty = lib.mkOption {
      type = lib.types.str;
      default = "/dev/tty12";
      description = ''
        Virtual terminal the tree is drawn on. boot-tree switches to it while
        it runs, so gettys, greeters and kernel output on other terminals
        cannot draw over the tree, and switches back to the previously active
        terminal when it hands the console over.
      '';
    };

    holdMs = lib.mkOption {
      type = lib.types.ints.unsigned;
      default = 0;
      description = "Milliseconds the final frame stays on screen before the console is handed over.";
    };

    quietConsole = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Silence kernel, udev and systemd status output on the console so it
        does not draw over the tree, and load a large Terminus console font in
        the initrd.
      '';
    };

    consoleFont = lib.mkOption {
      type = lib.types.str;
      default = "ter-v32n";
      example = "ter-v20n";
      description = "Terminus console font loaded when {option}`services.boot-tree.quietConsole` is set.";
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = config.boot.initrd.systemd.enable;
            message = "services.boot-tree needs boot.initrd.systemd.enable to start before the root switch.";
          }
        ];

        environment.systemPackages = [ cfg.package ];

        systemd.services.boot-tree = unit;

        boot.initrd.systemd = {
          services.boot-tree = unit;
          storePaths = [ (lib.getExe cfg.package) ];
        };
      }

      (lib.mkIf cfg.quietConsole {
        boot = {
          consoleLogLevel = lib.mkDefault 3;
          kernelParams = [
            "quiet"
            "systemd.show_status=error"
            "rd.systemd.show_status=error"
            "udev.log_level=3"
            "rd.udev.log_level=3"
          ];
        };

        console = {
          earlySetup = true;
          font = lib.mkDefault "${pkgs.terminus_font}/share/consolefonts/${cfg.consoleFont}.psf.gz";
        };
      })
    ]
  );
}
