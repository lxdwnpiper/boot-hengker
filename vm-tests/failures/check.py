def start_failing_unit(unit):
    machine.succeed(f"systemctl start --no-block {unit}")


def unit_result(unit):
    return machine.succeed(f"systemctl show -p Result --value {unit}").strip()


def failure_panel():
    lines = screen().splitlines()
    start = next(index for index, line in enumerate(lines) if " Failures " in line)
    return "\n".join(lines[start:])


machine.start()

wait_for_screen("Initrd", "Initrd held for the check")
start_failing_unit("initrd-nic-firmware-load")
wait_for_screen("Failures", "initrd-nic-firmware-load  exited with status 1", "rtl8168h-2.fw not found")
assert_bootvine_in_front()
assert_text_visible(font_has_512_glyphs=True)
machine.succeed("touch /run/release-initrd-failures-hold")
machine.switch_root()

wait_for_screen("Stage 2", "Boot held for the check")
wait_for_screen("initrd-nic-firmware-load  exited with status 1")

start_failing_unit("failure-report")
wait_for_screen("app-config-check  exited with status 2", "missing key 'listen'")
retry(lambda _: unit_result("failure-report") != "", timeout_seconds=10)
assert "× failure-report" not in failure_panel(), f"a dependency failure reached the panel:\n{screen()}"
assert "failure-report" in screen(), f"the dependency failure left the tree:\n{screen()}"

start_failing_unit("worker-pool")
wait_for_screen("worker-pool", "SIGSEGV", "starting worker pool with 4 threads")

start_failing_unit("license-server-wait")
wait_for_screen("license-server-wait  timed out", "waiting for license server at 10.0.0.9:27000")

start_failing_unit("metrics-agent")
wait_for_screen("metrics-agent  exited with status 203")

start_failing_unit("http-listener")
wait_for_screen("http-listener  restarted too often", "address already in use")

start_failing_unit("database-client")
wait_for_screen("database-client  exited with status 1: Cannot reach database", "connection refused")

start_failing_unit("upstream-uplink")
wait_for_screen("upstream-uplink  exited with status 1", "lost connection to upstream, giving up")

wait_for_screen("× 9 failed", "connection refused")
assert "Started Upstream uplink" not in failure_panel(), f"systemd's own start line reached the panel:\n{screen()}"
print(screen())
machine.screenshot("failures")

machine.succeed("touch /run/release-boot-failures-hold")
retry(lambda _: "handing the console over" in bootvine_journal(), timeout_seconds=60)
print(bootvine_journal())
