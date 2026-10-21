# Contributing

## Setup

Work inside the dev shell (`direnv allow` once). Version control is jj, not
the `git` CLI.

## Before committing

```sh
cargo clippy --workspace --all-targets
cargo fmt --all
```

Clippy runs with the pedantic lints and forbids absolute paths; formatting
needs the nightly toolchain the dev shell provides.

## Testing

| What | Command |
| --- | --- |
| Model and render tests | `cargo test --workspace` |
| Scripted demos | `cargo run --example boot` (also `shutdown`, `luks`) |
| Sample machine in a window | `nix run .#sample-machine-vm` |
| Boot and shutdown in a VM | `nix build .#checks.x86_64-linux.vm-boot-and-shutdown` |
| Passphrase prompt in a VM | `nix build .#checks.x86_64-linux.vm-luks-passphrase` |

To watch your own user manager start the sample units:

```sh
units=$(nix build --no-link --print-out-paths .#sample-unit-files)
systemctl --user link --runtime $units/*
systemctl --user start --no-block bootvine-demo.target
cargo run -- --user --target bootvine-demo.target --hold 3s
systemctl --user stop $(ls $units)
systemctl --user disable --runtime $(ls $units)
```

The VM checks read the console as text, hold the boot at fixed points and wait
on marker files instead of sleeping. To step through one by hand, run
`nix run .#checks.x86_64-linux.vm-boot-and-shutdown.driverInteractive` and
call `test_script()`.

Prefix VM and recording commands with `env -u LD_LIBRARY_PATH` when your shell
exports libraries that clash with QEMU or Chromium.

## Repository tasks

- `nix run .#record-screencasts` re-records the README animations from the
  scripted demos and copies the passphrase screenshot from the VM check.
- `cargo xtask install --destdir <dir> --initrd dracut` stages an install
  without touching the system; `cargo xtask uninstall --destdir <dir>` removes
  it again.

## Code style

- No comments; names and call flow carry the intent.
- Public fields over getters, durations over raw integers, existing crates over
  hand-written formatting.
- Iterator and stream combinators over loops.

## Design notes

### One event loop

Everything runs on one thread. The service manager, the keyboard, the frame
timer, password prompts and termination each produce a stream of inputs; the
streams are merged, applied in batches to a single screen state, and the screen
is drawn once per batch. The live boot and the scripted demos share this loop
and differ only in where their inputs come from.

### systemd's private socket

- zbus rejects it, because PID 1 sends under its well-known name rather than a
  unique one. libdbus works.
- libdbus sends its authentication handshake together with the first message,
  which systemd leaves unread, so a no-reply ping goes out right after
  connecting.
- Surviving the switch to the real root needs a process name starting with `@`,
  a unit that is not stopped on isolate, and a chroot through a file descriptor
  to the new root taken as soon as the switch is queued.
