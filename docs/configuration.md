# Configuration

Every setting has a default, so bootvine needs no configuration. To change
some, write only those keys:

- **On [NixOS](https://nixos.org)**, under `services.bootvine.settings`. The module writes them to
  `/etc/bootvine.toml` in the real root and the initrd, and the rebuild fails
  if `bootvine check` rejects them.

  ```nix
  services.bootvine.settings = {
    colors.tree.running-marker = "light-yellow";
    timing.keep-final-frame-for = "500ms";
  };
  ```

- **On other distributions**, in `/etc/bootvine.toml`:

  ```toml
  [colors.tree]
  running-marker = "light-yellow"

  [timing]
  keep-final-frame-for = "500ms"
  ```

  Then check the file and, if bootvine runs in your initrd, rebuild the
  initramfs, since the [dracut](https://github.com/dracut-ng/dracut-ng) and
  [mkinitcpio](https://wiki.archlinux.org/title/Mkinitcpio) hooks copy the file into it:

  ```sh
  bootvine check /etc/bootvine.toml
  sudo dracut --force  # or: sudo mkinitcpio -P
  ```

  At boot, bootvine ignores an invalid file and uses the defaults.

## Defaults

`bootvine defaults` prints every key with its default as TOML, and
`bootvine defaults --format nix` prints the same as a Nix attribute set to
paste into `services.bootvine.settings`:

```toml
[colors.header]
origin = "white"
separators = "dark-gray"
total-time = "white"

[colors.stages]
branches = "dark-gray"
finished-label = "gray"
finished-time = "green"
unit-count = "gray"
failed-count = "light-red"
current = "white"

[colors.tree]
branches = "dark-gray"
columns = "dark-gray"
target = "cyan"
current-stage = "yellow"
reached-stage = "green"
running-unit = "white"
running-marker = "yellow"
waiting-unit = "gray"
finished-unit = "dark-gray"
finished-marker = "green"
failed-unit = "light-red"
overflow = "gray"

[colors.timeline]
rule = "dark-gray"
title = "gray"
axis = "dark-gray"
milestone = "green"
finished = "green"
finished-label = "gray"
failed = "light-red"
queued-bar = "gray"
running = "yellow"
running-label = "white"

[colors.footer]
rule = "dark-gray"
done = "green"
running = "yellow"
waiting = "gray"
failed = "light-red"
no-failures = "gray"

[colors.password]
frame = "yellow"
message = "white"
hint = "dark-gray"
checking = "gray"
rejected = "light-red"
backdrop = "dark-gray"

[timing]
keep-finished-units-for = "2s"
hide-runs-shorter-than = "1s"
keep-final-frame-for = "1s"
wait-for-display-manager = "10s"
```

## Colours

Each `[colors.*]` table covers one part of the screen:

| Table | Part |
| --- | --- |
| `header` | the top line with the origin and total time |
| `stages` | the finished stages, such as the initrd |
| `tree` | the unit tree |
| `timeline` | the timeline of the slowest units |
| `footer` | the tallies of done, running, waiting and failed units |
| `password` | the passphrase popup |

Values are the 16 console colour names, such as `yellow`, `light-yellow`,
`gray` and `dark-gray`. To change how those 16 colours look, set the console
palette instead: `console.colors` on NixOS, or the `vt.default_red`,
`vt.default_grn` and `vt.default_blu` kernel parameters elsewhere.

## Timing

| Key | Meaning |
| --- | --- |
| `keep-finished-units-for` | How long a finished unit stays in the tree, faded, before it leaves. |
| `hide-runs-shorter-than` | Runs shorter than this get no timer and no timeline bar. |
| `keep-final-frame-for` | How long the last frame stays before the console is handed back. |
| `wait-for-display-manager` | When a display manager is starting, how long to wait for it to switch to its own terminal instead, so the console's status lines never show in between. |

## Early kernel messages

bootvine silences the kernel console while it runs, but kernel messages from
before it starts still reach the screen. To keep those to errors, boot with the
`quiet` kernel parameter, or on NixOS set:

```nix
boot.consoleLogLevel = 3;
```

## Running it by hand

The units run bootvine with its defaults. Run by hand, it takes these options:

| Option | Meaning |
| --- | --- |
| `--config <path>` | Read the settings from another file than `/etc/bootvine.toml`. |
| `--hold <duration>` | Override `timing.keep-final-frame-for`, e.g. `3s`. |
| `--target <unit>` | Root the tree at this unit instead of the default target. |
| `--user` | Watch your user service manager instead of PID 1. |
| `--shutdown` | Follow a shutdown instead of a boot. |

[Development](development.md#testing) shows how to watch your user manager
start a set of sample units this way.
