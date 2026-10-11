{ demoHost }:
{
  name = "boot-tree";
  enableOCR = true;

  nodes.machine = {
    imports = [ demoHost ];
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

    machine.wait_for_text("initrd")
    machine.screenshot("initrd")

    machine.wait_for_text("demo-slow")
    machine.screenshot("stage-2")

    machine.wait_for_unit("greetd.service")
    machine.wait_until_succeeds("systemctl show -p ActiveState --value boot-tree | grep -qx inactive")

    journal = machine.succeed("journalctl -b -u boot-tree -o cat")
    print(journal)
    watched = [line for line in journal.splitlines() if line.startswith("boot-tree: watching")]
    assert any(" in initrd " in line for line in watched), "never ran in the initrd"
    assert any(" in stage 2 " in line for line in watched), "never reconnected after the root switch"
    pids = {line.split(" as pid ")[1].split()[0] for line in watched}
    assert len(pids) == 1, f"the initrd process did not survive the root switch: {pids}"
    assert "handing the console over" in journal, "never handed the console over"

    machine.succeed("test \"$(systemctl show -p Result --value boot-tree)\" = success")
    machine.screenshot("handed-over")
  '';
}
