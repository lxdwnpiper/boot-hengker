# Contributing

## Workflow

Work inside the dev shell (`direnv allow` once). Run `cargo clippy
--all-targets` before committing (the crate warns on clippy pedantic plus
`absolute_paths`) and `cargo fmt`, which needs the nightly toolchain for
`rustfmt.toml`. Version control is jj, not the `git` CLI.

## Testing

- `cargo test`: model and render snapshot tests.
- `nix run .#demo-units -- start`, then
  `cargo run -- --user --target bootvine-demo.target --hold 3s`: live against
  your user manager.
- `nix build .#checks.x86_64-linux.<name>`: each `tests/<name>.nix` boots a VM
  and runs `tests/console.py` followed by `tests/<name>.py`. `boot` follows
  the initrd, stage 2 and a poweroff; `luks` answers a passphrase prompt in
  the initrd. They read tty12 as text from `/dev/vcsu12`, hold the boot with
  the initrd backdoor and gate files under `/run` instead of sleeping. Run one
  by hand with `nix run .#checks.x86_64-linux.boot.driverInteractive`, then
  `test_script()`.
- `nix run .#docs`, from the repository root: renders `docs/boot.tape` and
  `docs/shutdown.tape` with VHS against the scripted examples into the README
  GIFs, and copies the passphrase screenshot from the `luks` check.

Prefix the VM commands with `env -u LD_LIBRARY_PATH` when the shell exports
libraries that clash with QEMU's.

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
