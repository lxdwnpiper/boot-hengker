{ lib, pkgs, ... }:
{
  system.stateVersion = "26.05";

  boot.consoleLogLevel = lib.mkForce 3;

  services.bootvine = {
    enable = true;
    settings.colors = {
      tree.current-stage = "light-yellow";
      tree.running-marker = "light-yellow";
      timeline.running = "light-yellow";
      footer.running = "light-yellow";
      password.frame = "light-yellow";
    };
  };

  console = {
    colors = [
      "1d2021"
      "fb4934"
      "b8bb26"
      "fabd2f"
      "83a598"
      "d3869b"
      "8ec07c"
      "d5c4a1"
      "665c54"
      "fb4934"
      "b8bb26"
      "fabd2f"
      "83a598"
      "d3869b"
      "8ec07c"
      "fbf1c7"
    ];
    earlySetup = true;
    font = "${pkgs.terminus_font}/share/consolefonts/ter-v20n.psf.gz";
  };

  users.users.sample-user = {
    isNormalUser = true;
    password = "sample-user";
  };
}
