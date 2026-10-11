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
- `nix build .#checks.x86_64-linux.boot`: boots a VM and reads the screen with
  OCR; screenshots land in `result/`. Run it by hand with
  `nix run .#checks.x86_64-linux.boot.driverInteractive`, then `test_script()`.

Prefix the VM commands with `env -u LD_LIBRARY_PATH` when the shell exports
libraries that clash with QEMU's.

## Conventions

- No comments or doc comments; intent lives in names and call flow.
- Pub fields instead of getters, `Duration` instead of raw integers, crates
  instead of hand-written formatting.
- One thread: the loop waits on the bus with a timeout, then draws.
- Ratatui views are built declaratively from iterators.

## systemd's private socket

- zbus rejects it: PID 1 sends as `org.freedesktop.systemd1`, not a unique
  name. libdbus works.
- libdbus pipelines `BEGIN` with the first message and sd-bus leaves it unread,
  so a no-reply `Ping` goes out right after connecting.
- Surviving switch-root needs `argv[0]` starting with `@`, `IgnoreOnIsolate`,
  and a chroot through an fd to `/sysroot` taken when
  `initrd-switch-root.service` is queued.
