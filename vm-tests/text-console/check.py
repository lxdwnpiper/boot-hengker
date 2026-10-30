def unit_state(unit):
    return machine.succeed(f"systemctl is-active {unit} || true").strip()


recording = ScreenRecording(machine)
machine.start()
recording.start()

wait_for_screen("Initrd", "Slow hardware probe")
assert_bootvine_in_front()
machine.succeed("touch /run/release-initrd-hardware-probe")
machine.switch_root()

wait_for_screen("Stage 2", "slow-warm-up")
assert_bootvine_in_front()
assert_text_visible(font_has_512_glyphs=True)

retry(lambda _: unit_state("bootvine") == "inactive", timeout_seconds=60)
journal = bootvine_journal()
print(journal)
assert "handing the console over" in journal, "never handed the console over"
assert foreground_terminal() != f"tty{BOOTVINE_TERMINAL}", "bootvine kept the console without a display manager"
machine.wait_until_tty_matches("1", "login: ")

machine.succeed("systemctl poweroff --no-block")
retry(lambda _: unit_state("sample-database") == "deactivating", timeout_seconds=60)
wait_for_screen("Shutdown", "sample-database")
assert_bootvine_in_front()
machine.succeed("touch /run/release-sample-database")
machine.wait_for_shutdown()
recording.stop()
