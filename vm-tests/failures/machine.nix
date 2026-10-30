{
  config,
  lib,
  pkgs,
  ...
}:
let
  startedByHand = config.bootvineSample.startFailuresByHand;
  wantedAtBoot = target: lib.optional (!startedByHand) target;

  failingService =
    {
      description,
      script,
      serviceConfig ? { },
      unitConfig ? { },
      extraAttributes ? { },
    }:
    {
      inherit description unitConfig;
      wantedBy = wantedAtBoot "multi-user.target";
      serviceConfig = {
        Type = "oneshot";
        ExecStart = pkgs.writeShellScript "failing-service" script;
      }
      // serviceConfig;
    }
    // extraAttributes;

  initrdFirmwareScript = pkgs.writeShellScript "initrd-nic-firmware-load" ''
    echo "loading firmware for enp1s0 (r8169)"
    echo "firmware: rtl_nic/rtl8168h-2.fw not found" >&2
    exit 1
  '';
in
{
  options.bootvineSample.startFailuresByHand = lib.mkEnableOption ''
    leaving the failing sample units out of the boot targets, so a check can
    start them one at a time
  '';

  config = {
    boot.initrd.systemd = {
      storePaths = [
        initrdFirmwareScript
        "${pkgs.bash}/bin/bash"
      ];
      services.initrd-nic-firmware-load = {
        description = "Network card firmware";
        wantedBy = wantedAtBoot "initrd.target";
        before = [ "initrd.target" ];
        unitConfig.DefaultDependencies = false;
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${pkgs.bash}/bin/bash ${initrdFirmwareScript}";
        };
      };
    };

    systemd.services = {
      app-config-check = failingService {
        description = "Application configuration check";
        script = ''
          echo "reading /etc/app/app.toml"
          echo "config: /etc/app/app.toml: missing key 'listen'" >&2
          exit 2
        '';
      };

      worker-pool = failingService {
        description = "Worker pool";
        script = ''
          echo "starting worker pool with 4 threads"
          kill -SEGV $$
        '';
      };

      license-server-wait = failingService {
        description = "License server check";
        script = ''
          echo "waiting for license server at 10.0.0.9:27000"
          sleep infinity
        '';
        serviceConfig.TimeoutStartSec = 4;
      };

      metrics-agent = {
        description = "Metrics agent";
        wantedBy = wantedAtBoot "multi-user.target";
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "/usr/bin/metrics-agent --listen :9100";
        };
      };

      http-listener = failingService {
        description = "HTTP listener";
        script = ''
          echo "bind 0.0.0.0:8080: address already in use" >&2
          exit 1
        '';
        serviceConfig = {
          Restart = "on-failure";
          RestartSec = "200ms";
        };
        unitConfig = {
          StartLimitBurst = 3;
          StartLimitIntervalSec = 30;
        };
      };

      database-client = failingService {
        description = "Database client";
        script = ''
          systemd-notify --status="Cannot reach database at db.internal:5432"
          echo "connect db.internal:5432: connection refused" >&2
          exit 1
        '';
        serviceConfig = {
          Type = "notify";
          NotifyAccess = "all";
        };
      };

      upstream-uplink = failingService {
        description = "Upstream uplink";
        script = ''
          echo "connected to upstream"
          sleep 3
          echo "lost connection to upstream, giving up" >&2
          exit 1
        '';
        serviceConfig.Type = "simple";
      };

      failure-report = failingService {
        description = "Failure report";
        script = ''
          echo "all checks passed"
        '';
        extraAttributes = {
          requires = [ "app-config-check.service" ];
          after = [ "app-config-check.service" ];
        };
      };
    };
  };
}
