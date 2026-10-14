{
  target = "bootvine-demo";
  initrdProbeSeconds = "6";
  services = {
    demo-disk = {
      description = "Scratch disk preparation";
      seconds = "2";
      stopSeconds = "2";
    };
    demo-db = {
      description = "Demo database";
      seconds = "3";
      stopSeconds = "3";
      after = [ "demo-disk" ];
    };
    demo-cache = {
      description = "In-memory cache";
      seconds = "1.5";
      stopSeconds = "1";
    };
    demo-web = {
      description = "Web frontend";
      seconds = "2";
      stopSeconds = "2";
      after = [
        "demo-db"
        "demo-cache"
      ];
    };
    demo-fetch = {
      description = "Remote content sync";
      seconds = "4";
    };
    demo-flaky = {
      description = "Flaky health check";
      seconds = "1";
      fails = true;
    };
    demo-slow = {
      description = "Slow warm-up job";
      seconds = "8";
    };
    demo-report = {
      description = "Startup report";
      seconds = "1";
      after = [
        "demo-web"
        "demo-fetch"
      ];
    };
  };
}
