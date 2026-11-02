RESUME_MARKER = "/run/bootvine/resume-console-open"
BOOT_CONSOLE_LEVEL = "3"
SENTINEL_TEXT = "held only in memory"


def console_level():
    return machine.succeed("cut -f1 /proc/sys/kernel/printk").strip()


def assert_console_returned():
    machine.wait_until_fails("systemctl is-active bootvine")
    machine.fail(f"test -e {RESUME_MARKER}")
    assert console_level() == BOOT_CONSOLE_LEVEL, f"kernel console level was not restored: {console_level()}"


recording = ScreenRecording(machine)
machine.start()
recording.start()

machine.wait_for_unit("default.target")
assert_console_returned()
machine.succeed("swapon --show=NAME --noheadings | grep -q .")
machine.succeed(
    "mkdir /run/test",
    "mount -t ramfs ramfs /run/test",
    f"echo '{SENTINEL_TEXT}' > /run/test/sentinel",
    "dd if=/dev/urandom of=/run/test/ballast bs=1M count=500",
)

machine.execute("systemctl hibernate >&2 &", check_return=False)
machine.wait_for_shutdown()
recording.stop()

resume_recording = ScreenRecording(machine)
resume_recording.frames_directory = os.path.join(machine.out_dir, "resume-recording")
resume_recording.frame_seconds = 0.1
machine.start()
resume_recording.start()
machine.wait_for_console_text("Image loading progress")
machine.wait_for_console_text("Image loading done")
machine.succeed(f"grep '{SENTINEL_TEXT}' /run/test/sentinel")
assert console_level() == BOOT_CONSOLE_LEVEL, f"the restored system kept the opened level: {console_level()}"
assert foreground_terminal() != f"tty{BOOTVINE_TERMINAL}", "the restored system came back on bootvine's terminal"
machine.fail("systemctl is-active bootvine")

resume_recording.stop()

machine.crash()
machine.start()
machine.wait_for_unit("default.target")
machine.fail("test -e /run/test/sentinel")
assert_console_returned()
journal = bootvine_journal()
print(journal)
assert " in Initrd " in journal, "bootvine did not run in the initrd of the boot without an image"
machine.succeed('test "$(systemctl show -p Result --value bootvine)" = success')
