{
  target = "bootvine-sample-services";
  initrdProbeSeconds = "6";
  services = {
    scratch-disk-prepare = {
      description = "Scratch disk preparation";
      seconds = "2";
      stopSeconds = "2";
    };
    sample-database = {
      description = "Sample database";
      seconds = "3";
      stopSeconds = "3";
      after = [ "scratch-disk-prepare" ];
    };
    memory-cache = {
      description = "In-memory cache";
      seconds = "1.5";
      stopSeconds = "1";
    };
    web-frontend = {
      description = "Web frontend";
      seconds = "2";
      stopSeconds = "2";
      after = [
        "sample-database"
        "memory-cache"
      ];
    };
    remote-content-sync = {
      description = "Remote content sync";
      seconds = "4";
    };
    flaky-health-check = {
      description = "Flaky health check";
      seconds = "1";
      fails = true;
    };
    slow-warm-up = {
      description = "Slow warm-up job";
      seconds = "8";
    };
    startup-report = {
      description = "Startup report";
      seconds = "1";
      after = [
        "web-frontend"
        "remote-content-sync"
      ];
    };
  };
}
