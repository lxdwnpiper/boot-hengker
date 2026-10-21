# Contributing

## Workflow

Work inside the dev shell (`direnv allow` once). Run `cargo clippy --workspace
--all-targets` before committing (the workspace warns on clippy pedantic plus
`absolute_paths`) and `cargo fmt --all`, which needs the nightly toolchain for
`.rustfmt.toml`. Version control is jj, not the `git` CLI.

The workspace has two crates: `bootvine` itself and `xtask`, the repository
tasks behind `cargo xtask` (`record-screencasts`, `install`, `uninstall`).

## Testing

Everything that boots a machine or a service manager lives under `tests/`:
`tests/sample-machine/` holds the sample host, its slow and failing sample units
and the interactive VM; `tests/vm-checks/` holds the NixOS tests that boot it.

- `cargo test --workspace`: model and render snapshot tests.
- Live against your user manager, with the sample units:

  ```sh
  units=$(nix build --no-link --print-out-paths .#sample-unit-files)
  systemctl --user link --runtime $units/*
  systemctl --user start --no-block bootvine-demo.target
  cargo run -- --user --target bootvine-demo.target --hold 3s
  systemctl --user stop $(ls $units)
  systemctl --user disable --runtime $(ls $units)
  ```

- `nix run .#sample-machine-vm`: the sample host in a QEMU window. Its disk
  image goes to `~/.cache/bootvine-sample-machine.qcow2`.
- `nix build .#checks.x86_64-linux.<name>`: each `tests/vm-checks/<test>.nix`
  boots a VM and runs `console.py` followed by `<test>.py`.
  `vm-boot-and-shutdown` follows the initrd, stage 2 and a poweroff;
  `vm-luks-passphrase` answers a passphrase prompt in the initrd. They read
  tty12 as text from `/dev/vcsu12`, hold the boot with the initrd backdoor and
  gate files under `/run` instead of sleeping. Run one by hand with
  `nix run .#checks.x86_64-linux.vm-boot-and-shutdown.driverInteractive`, then
  `test_script()`.
- `nix run .#record-screencasts`, from the repository root: runs
  `cargo xtask record-screencasts`, which records `recordings/boot.tape` and
  `recordings/shutdown.tape` with VHS against the scripted examples into the README
  GIFs and copies the passphrase screenshot from the `vm-luks-passphrase` check.
- `cargo xtask install --destdir <dir> --initrd dracut`: stages an install
  without touching the system; `cargo xtask uninstall --destdir <dir>` empties
  it again.

Prefix the VM and VHS commands with `env -u LD_LIBRARY_PATH` when the shell
exports libraries that clash with QEMU's or Chromium's.

## Conventions

- No comments or doc comments; intent lives in names and call flow.
- Pub fields instead of getters, `Duration` instead of raw integers, crates
  instead of hand-written formatting.
- One thread: the loop sleeps in one `poll(2)` on the terminal, the bus and an
  inotify watch on `/run/systemd/ask-password`, with the next frame as timeout,
  then draws.
- Ratatui views are built declaratively from iterators.

## systemd's private socket

- zbus rejects it: PID 1 sends as `org.freedesktop.systemd1`, not a unique
  name. libdbus works.
- libdbus pipelines `BEGIN` with the first message and sd-bus leaves it unread,
  so a no-reply `Ping` goes out right after connecting.
- Surviving switch-root needs `argv[0]` starting with `@`, `IgnoreOnIsolate`,
  and a chroot through an fd to `/sysroot` taken when
  `initrd-switch-root.service` is queued.
