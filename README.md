# bootvine

A live dependency tree of systemd jobs on the boot console, in the spirit of
nix-output-monitor. It replaces the scrolling kernel and systemd lines with a
tree of pending units and a timeline bar per row.

## Requirements

- Linux with **systemd ≥ 246 as PID 1** (private socket `/run/systemd/private`,
  `Manager.SetShowStatus`).
- Runs as **root**, from the shipped `bootvine.service`.
- **libdbus-1** at runtime.
- A **virtual terminal** (`CONFIG_VT`); the unit draws on `/dev/tty12`.
- Optional: a **systemd-based initrd** (NixOS `boot.initrd.systemd`, dracut, or
  mkinitcpio's `systemd` hook). Without one the tree starts in stage 2, once the
  real root is mounted.
- Optional: a large console font on HiDPI screens, e.g. Terminus `ter-v32n`
  through the distro's console font setting.

No kernel parameters are needed: while it runs, bootvine lowers the kernel
console log level and turns systemd's status output off, and restores both when
it hands the console over.

## How it works

- Starts as an initrd unit when the initrd runs systemd, about a second after
  the kernel.
- Talks to PID 1 over the private socket `/run/systemd/private` with libdbus:
  `Subscribe`, `ListJobs`, unit properties, and the `JobNew`/`JobRemoved`/
  `PropertiesChanged` signals.
- Survives the switch to the real root: its `argv[0]` starts with `@`, the unit
  ignores isolate, and it chroots into `/sysroot` before PID 1 re-executes, then
  reconnects. Plymouth does the same with `plymouth update-root-fs`.
- Draws on its own virtual terminal (tty12 by default) and switches back to the
  previous one when the display manager starts or the default target is
  reached, so gettys and greeters on tty1 cannot draw over it.

## Installing

The package ships everything a system needs under `lib/`:

| File | Purpose |
| --- | --- |
| `lib/systemd/system/bootvine.service` | the unit, used in both the initrd and stage 2 |
| `lib/dracut/modules.d/90bootvine/` | dracut module |
| `lib/initcpio/install/sd-bootvine` | mkinitcpio hook |

### NixOS

```nix
inputs.bootvine.url = "git+https://codeberg.org/wvhulle/bootvine";
# in the host's modules:
bootvine.nixosModules.default
{ services.bootvine.enable = true; }
```

With `boot.initrd.systemd.enable` the unit is added to the initrd as well.

### Other distributions

1. Install `bootvine` to `/usr/bin` and the files above under `/usr/lib`.
2. `systemctl enable bootvine.service`.
3. To start it in the initrd:
   - dracut: `add_dracutmodules+=" bootvine "` in `/etc/dracut.conf.d/bootvine.conf`,
     then `dracut --force`;
   - mkinitcpio: add `sd-bootvine` after `systemd` in `HOOKS`, then
     `mkinitcpio -P`.

To draw on another terminal, override `TTYPath=` with
`systemctl edit bootvine.service`.

## Configuration

Colours come from `/etc/bootvine.toml` (another path with `--config`). Every
key is optional; these are the defaults:

```toml
[palette]
text = "white"        # running units, headings
muted = "dark-gray"   # rules, tree lines, axis
waiting = "gray"      # queued units, finished-unit labels
running = "yellow"
done = "green"
failed = "light-red"
target = "cyan"
```

Values are the 16 console colour names (`red`, `light-red`, `dark-gray`, …).
The Linux console cannot show other colours, so to change what the names look
like, set the console palette itself. That recolours every virtual terminal from
the start of boot:

- NixOS: `console.colors = [ "1d2021" "fb4934" … ];` (16 hex values);
- elsewhere: the kernel parameters `vt.default_red=`, `vt.default_grn=` and
  `vt.default_blu=`.

On NixOS set the roles as options; unset ones keep their defaults:

```nix
services.bootvine.palette = {
  running = "light-yellow";
  done = "light-green";
};
```

On dracut and mkinitcpio systems the hooks copy `/etc/bootvine.toml` into the
initramfs, so rebuild it after editing.

## Testing

Commands assume the dev shell is active (`direnv allow` once; `.envrc` runs
`use flake`).

| What | Command |
| --- | --- |
| Unit and render snapshot tests | `cargo test` |
| Scripted boot animation, no systemd needed | `cargo run --example demo` (`q` quits) |
| Live against your user manager | `nix run .#demo-units -- start`, then `nix run . -- --user --target bootvine-demo.target --hold 3s` |
| Boot a demo VM in a window | `env -u LD_LIBRARY_PATH nix run .#vm` |
| Automated boot test with OCR | `nix build .#checks.x86_64-linux.boot` (screenshots in `result/`) |
| Same test, interactively | `nix run .#checks.x86_64-linux.boot.driverInteractive`, then `test_script()` |

`env -u LD_LIBRARY_PATH` is only needed when the shell exports libraries that
clash with QEMU's.
