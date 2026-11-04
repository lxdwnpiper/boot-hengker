def unit_state(unit):
    return machine.succeed(f"systemctl is-active {unit} || true").strip()


recording = ScreenRecording(machine)
machine.start()
recording.start()

wait_for_screen("Stage 2", "sample-database")
assert_bootvine_in_front()
assert_text_visible(font_has_512_glyphs=True)

machine.succeed("timeout 10 systemctl stop bootvine")
machine.succeed("systemctl start --no-block bootvine")
retry(lambda _: foreground_terminal() == f"tty{BOOTVINE_TERMINAL}", timeout_seconds=30)
wait_for_screen("Stage 2")

machine.send_chars("q")
retry(lambda _: unit_state("bootvine") == "inactive", timeout_seconds=10)
assert foreground_terminal() != f"tty{BOOTVINE_TERMINAL}", "the quit key left bootvine in front"
show_status = machine.succeed("systemctl show -p ShowStatus --value").strip()
assert show_status == "yes", f"the quit key did not restore the status output: {show_status}"

machine.succeed("systemctl start --no-block bootvine")
retry(lambda _: foreground_terminal() == f"tty{BOOTVINE_TERMINAL}", timeout_seconds=30)

machine.wait_for_unit("greetd.service")
retry(lambda _: "another program took the console" in bootvine_journal(), timeout_seconds=60)
assert unit_state("late-sensor-probe") == "activating", "the boot finished before the display manager took over"
assert unit_state("bootvine") == "active", "bootvine left before the boot finished"
machine.succeed("timeout 10 systemctl stop bootvine")
assert unit_state("bootvine") == "inactive", "stopping bootvine during the boot did not end it"
show_status = machine.succeed("systemctl show -p ShowStatus --value").strip()
assert show_status == "yes", f"stopping bootvine did not restore the status output: {show_status}"
machine.succeed("touch /run/release-late-sensor-probe")
machine.wait_for_unit("graphical.target")
journal = bootvine_journal()
print(journal)
watched = [line for line in journal.splitlines() if line.startswith("bootvine: watching")]
assert watched, "bootvine never started"
assert not any(" in Initrd " in line for line in watched), "bootvine ran in a scripted initrd"
assert "handing the console over" in journal, "never handed the console over"

machine.succeed("systemctl poweroff --no-block")
retry(lambda _: unit_state("sample-database") == "deactivating", timeout_seconds=60)
wait_for_screen("Shutdown", "sample-database")
assert_bootvine_in_front()
machine.succeed("touch /run/release-sample-database")
machine.wait_for_shutdown()
recording.stop()
