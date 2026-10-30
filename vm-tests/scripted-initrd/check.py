def unit_state(unit):
    return machine.succeed(f"systemctl is-active {unit} || true").strip()


recording = ScreenRecording(machine)
machine.start()
recording.start()

wait_for_screen("Stage 2", "sample-database")
assert_bootvine_in_front()
assert_text_visible(font_has_512_glyphs=True)

machine.wait_for_unit("greetd.service")
retry(lambda _: unit_state("bootvine") == "inactive", timeout_seconds=60)
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
