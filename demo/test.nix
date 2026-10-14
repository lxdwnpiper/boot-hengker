{ demoHost }:
{
  name = "bootvine";
  enableOCR = true;

  nodes.machine =
    { lib, pkgs, ... }:
    let
      lingerFor = seconds: lib.mkForce "${pkgs.coreutils}/bin/sleep ${seconds}";
    in
    {
      imports = [ demoHost ];
      systemd.settings.Manager.ShowStatus = lib.mkForce true;
      systemd.services.demo-db.serviceConfig.ExecStop = lingerFor "20";
      virtualisation = {
        memorySize = 2048;
        qemu.options = [
          "-vga none"
          "-device virtio-vga,xres=1280,yres=800"
        ];
      };
    };

  testScript = ''
    machine.start()

    machine.wait_for_text("Initrd")
    machine.screenshot("initrd")

    machine.wait_for_text("Stage 2")
    machine.screenshot("stage-2")
    quieted = machine.succeed("cut -f1 /proc/sys/kernel/printk").strip()
    assert quieted == "1", f"stage 2 sysctl reopened the kernel console: {quieted}"

    machine.wait_for_unit("greetd.service")
    machine.wait_until_succeeds("systemctl show -p ActiveState --value bootvine | grep -qx inactive")

    journal = machine.succeed("journalctl -b -u bootvine -o cat")
    print(journal)
    watched = [line for line in journal.splitlines() if line.startswith("bootvine: watching")]
    assert any(" in initrd " in line for line in watched), "never ran in the initrd"
    assert any(" in stage 2 " in line for line in watched), "never reconnected after the root switch"
    pids = {line.split(" as pid ")[1].split()[0] for line in watched}
    assert len(pids) == 1, f"the initrd process did not survive the root switch: {pids}"
    assert "handing the console over" in journal, "never handed the console over"

    machine.succeed("test \"$(systemctl show -p Result --value bootvine)\" = success")
    console_level = machine.succeed("cut -f1 /proc/sys/kernel/printk").strip()
    assert console_level != "1", f"kernel console level was not restored: {console_level}"
    show_status = machine.succeed("systemctl show -p ShowStatus --value").strip()
    assert show_status == "yes", f"systemd status output was not restored: {show_status}"
    machine.screenshot("handed-over")

    foreground = machine.succeed("cat /sys/class/tty/tty0/active").strip()
    machine.succeed("systemctl restart bootvine")
    machine.wait_until_succeeds("systemctl show -p ActiveState --value bootvine | grep -qx inactive", timeout=10)
    assert "boot already finished" in machine.succeed("journalctl -u bootvine -o cat"), "a restart after boot did not stand down"
    still_foreground = machine.succeed("cat /sys/class/tty/tty0/active").strip()
    assert still_foreground == foreground, f"a restart after boot took the console: {foreground} -> {still_foreground}"

    machine.succeed("systemctl poweroff --no-block")
    machine.wait_for_text("demo-db")
    machine.screenshot("shutdown")
    machine.wait_for_shutdown()
  '';
}
