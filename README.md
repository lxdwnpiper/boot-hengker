# boot-tree

A live dependency tree of systemd jobs on the boot console, in the spirit of
nix-output-monitor. It replaces the scrolling kernel and systemd lines with a
tree of pending units and a timeline bar per row.

## How it works

- Starts as an initrd unit (needs `boot.initrd.systemd.enable`), about a second
  after the kernel.
- Talks to PID 1 over the private socket `/run/systemd/private` with libdbus:
  `Subscribe`, `ListJobs`, unit properties, and the `JobNew`/`JobRemoved`/
  `PropertiesChanged` signals.
- Survives the switch to the real root: its `argv[0]` starts with `@`, the unit
  ignores isolate, and it chroots into `/sysroot` before PID 1 re-executes, then
  reconnects. Plymouth does the same with `plymouth update-root-fs`.
- Draws on its own virtual terminal (tty12 by default) and switches back to the
  previous one when the display manager starts or the default target is
  reached, so gettys and greeters on tty1 cannot draw over it.

## Usage

```nix
inputs.boot-tree.url = "git+file:/home/wvhulle/Code/boot-tree?ref=refs/heads/main";
# in a host's modules:
boot-tree.nixosModules.default
{ services.boot-tree.enable = true; }
```

Options: `tty`, `holdMs`, `quietConsole` (silences kernel/systemd console
output and loads a Terminus font in the initrd), `consoleFont`.

## Testing

Commands assume the dev shell is active (`direnv allow` once; `.envrc` runs
`use flake`).

| What | Command |
| --- | --- |
| Unit and render snapshot tests | `cargo test` |
| Scripted boot animation, no systemd needed | `cargo run --example demo` (`q` quits) |
| Live against your user manager | `nix run .#demo-units -- start`, then `nix run . -- --user --target boot-tree-demo.target --hold-ms 3000` |
| Boot a demo VM in a window | `env -u LD_LIBRARY_PATH nix run .#vm` |
| Automated boot test with OCR | `nix build .#checks.x86_64-linux.boot` (screenshots in `result/`) |
| Same test, interactively | `nix run .#checks.x86_64-linux.boot.driverInteractive`, then `test_script()` |

`env -u LD_LIBRARY_PATH` is only needed when the shell exports libraries that
clash with QEMU's.
