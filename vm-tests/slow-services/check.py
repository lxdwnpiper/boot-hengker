def console_level():
    return machine.succeed("cut -f1 /proc/sys/kernel/printk").strip()


def unit_state(unit):
    return machine.succeed(f"systemctl is-active {unit} || true").strip()


def foreground_console_text():
    terminal_number = foreground_terminal().removeprefix("tty")
    return machine.succeed(f"tr -cd '[:print:]' < /dev/vcs{terminal_number}")


def assert_unit_start_stays_off_console(description):
    machine.succeed(f"systemd-run --wait --description='{description}' /run/current-system/sw/bin/true")
    console_text = foreground_console_text()
    assert description not in console_text, f"systemd status reached the login screen:\n{console_text}"


recording = ScreenRecording(machine)
machine.start()
recording.start()

wait_for_screen("Initrd", "Slow hardware probe")
assert_bootvine_in_front()
assert_text_visible(font_has_512_glyphs=True)
assert console_level() == "1", f"the kernel console was not quieted: {console_level()}"
machine.succeed("touch /run/release-initrd-hardware-probe")
machine.switch_root()

retry(lambda _: unit_state("sample-database") == "activating", timeout_seconds=60)
wait_for_screen("Stage 2", "sample-database")
assert_bootvine_in_front()
assert_text_visible(font_has_512_glyphs=True)
assert console_level() == "1", f"stage 2 sysctl reopened the kernel console: {console_level()}"

machine.wait_for_unit("greetd.service")
retry(lambda _: "another program took the console" in bootvine_journal(), timeout_seconds=60)
assert unit_state("late-sensor-probe") == "activating", "the boot finished before the display manager took over"
assert foreground_terminal() != f"tty{BOOTVINE_TERMINAL}", "bootvine took the console back from the display manager"
machine.wait_until_tty_matches("1", "Username:")
assert_unit_start_stays_off_console("Mid boot status probe")
show_status = machine.succeed("systemctl show -p ShowStatus --value").strip()
assert show_status == "no", f"systemd status output came back before the boot finished: {show_status}"
assert unit_state("bootvine") == "active", "bootvine left before the boot finished"
machine.succeed("touch /run/release-late-sensor-probe")
machine.wait_for_unit("graphical.target")
retry(lambda _: unit_state("bootvine") == "inactive", timeout_seconds=60)

journal = bootvine_journal()
print(journal)
watched = [line for line in journal.splitlines() if line.startswith("bootvine: watching")]
assert any(" in Initrd " in line for line in watched), "never ran in the initrd"
assert any(" in Stage 2 " in line for line in watched), "never reconnected after the root switch"
pids = {line.split(" as pid ")[1].split()[0] for line in watched}
assert len(pids) == 1, f"the initrd process did not survive the root switch: {pids}"
assert "handing the console over" in journal, "never handed the console over"

machine.succeed('test "$(systemctl show -p Result --value bootvine)" = success')
assert console_level() != "1", f"kernel console level was not restored: {console_level()}"
show_status = machine.succeed("systemctl show -p ShowStatus --value").strip()
assert show_status == "yes", f"systemd status output was not restored: {show_status}"
assert_unit_start_stays_off_console("Post boot status probe")

machine.wait_until_tty_matches("1", "Username:")
machine.send_chars("sample-user\n")
machine.wait_until_tty_matches("1", "Password:")
machine.send_chars("sample-user\n")
machine.wait_until_succeeds("pgrep -u sample-user bash", timeout=30)

foreground = foreground_terminal()
assert foreground != f"tty{BOOTVINE_TERMINAL}", "bootvine kept the console after handing it over"
machine.succeed("systemctl restart bootvine")
retry(lambda _: unit_state("bootvine") == "inactive", timeout_seconds=10)
assert "boot already finished" in bootvine_journal(), "a restart after boot did not stand down"
assert foreground_terminal() == foreground, "a restart after boot took the console"

machine.succeed("systemctl poweroff --no-block")
retry(lambda _: unit_state("sample-database") == "deactivating", timeout_seconds=60)
wait_for_screen("Shutdown", "sample-database")
assert_bootvine_in_front()
assert_text_visible(font_has_512_glyphs=True)
machine.succeed("touch /run/release-sample-database")
machine.wait_for_shutdown()
recording.stop()
