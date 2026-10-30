{ pkgs }:
let
  passphrase = "sample passphrase";
  uuid = "9d1f2a8e-0b7c-4c55-9a8e-6b2f8f3c1d00";
  image = pkgs.runCommand "bootvine-luks-disk" { nativeBuildInputs = [ pkgs.cryptsetup ]; } ''
    truncate -s 32M disk.img
    echo -n '${passphrase}' | cryptsetup luksFormat -q --disable-locks --type luks2 \
      --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --uuid ${uuid} disk.img -
    mv disk.img $out
  '';
in
{
  inherit passphrase uuid;
  qemuDrive = {
    name = "secret";
    file = "${image}";
    driveExtraOpts = {
      format = "raw";
      snapshot = "on";
    };
  };
}
