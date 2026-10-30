def answered_prompts():
    return bootvine_journal().count("answered a password prompt")


def pending_prompt():
    listing = machine.succeed("ls /run/systemd/ask-password").split()
    return next((name for name in listing if name.startswith("ask.")), "")


recording = ScreenRecording(machine)
machine.start()
recording.start()

retry(lambda _: pending_prompt() != "", timeout_seconds=60)
wait_for_screen("passphrase")
assert_text_visible(font_has_512_glyphs=True)
first_prompt = pending_prompt()
machine.send_chars("not the passphrase\n")
retry(lambda _: answered_prompts() == 1, timeout_seconds=30)
retry(lambda _: pending_prompt() not in ("", first_prompt), timeout_seconds=30)
wait_for_screen("passphrase")
assert_text_visible(font_has_512_glyphs=True)
machine.send_chars(f"{PASSPHRASE}\n")
machine.wait_for_unit("systemd-cryptsetup@secret.service")
machine.switch_root()
machine.wait_for_unit("multi-user.target")

machine.succeed("test -e /dev/mapper/secret")
journal = bootvine_journal()
print(journal)
assert " in Initrd " in journal, "never ran in the initrd"
assert journal.count("answered a password prompt") == 2, "did not answer both attempts itself"
recording.stop()
