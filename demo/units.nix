{
  target = "bootvine-demo";
  initrdProbeSeconds = "6";
  services = {
    demo-disk = {
      seconds = "2";
      stopSeconds = "2";
    };
    demo-db = {
      seconds = "3";
      stopSeconds = "3";
      after = [ "demo-disk" ];
    };
    demo-cache = {
      seconds = "1.5";
      stopSeconds = "1";
    };
    demo-web = {
      seconds = "2";
      stopSeconds = "2";
      after = [
        "demo-db"
        "demo-cache"
      ];
    };
    demo-fetch.seconds = "4";
    demo-flaky = {
      seconds = "1";
      fails = true;
    };
    demo-slow.seconds = "8";
    demo-report = {
      seconds = "1";
      after = [
        "demo-web"
        "demo-fetch"
      ];
    };
  };
}
