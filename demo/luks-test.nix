{ bootvineModule }:
{
  name = "bootvine-luks";
  enableOCR = true;

  nodes.machine =
    { lib, pkgs, ... }:
    {
      imports = [ bootvineModule ];

      services.bootvine.enable = true;

      boot.loader.systemd-boot.enable = true;
      boot.initrd.systemd.enable = true;
      environment.systemPackages = [ pkgs.cryptsetup ];

      virtualisation = {
        emptyDiskImages = [ 64 ];
        useBootLoader = true;
        useEFIBoot = true;
        mountHostNixStore = true;
      };

      specialisation.unlock.configuration = {
        boot.initrd.luks.devices = lib.mkVMOverride {
          secret.device = "/dev/vdb";
        };
      };
    };

  testScript = ''
    import time

    passphrase = "quietly unlocked"

    machine.wait_for_unit("multi-user.target")
    machine.succeed(f"echo -n '{passphrase}' | cryptsetup luksFormat -q --iter-time=1 /dev/vdb -")
    machine.succeed("bootctl set-default nixos-generation-1-specialisation-unlock.conf")
    machine.succeed("sync")
    machine.crash()

    machine.start()
    machine.wait_for_text("passphrase")
    machine.send_chars("not the passphrase\n")
    time.sleep(5)
    machine.wait_for_text("passphrase")
    machine.screenshot("passphrase")
    machine.send_chars(f"{passphrase}\n")
    machine.wait_for_unit("multi-user.target")

    machine.succeed("test -e /dev/mapper/secret")
    journal = machine.succeed("journalctl -b -u bootvine -o cat")
    print(journal)
    assert "in initrd" in journal, "never ran in the initrd"
    assert journal.count("answered a password prompt") == 2, "did not answer both attempts itself"
  '';
}
