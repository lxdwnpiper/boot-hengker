# piperCs fork

This is [bootvine](https://codeberg.org/wvhulle/bootvine) with a personal
"hacker" look, used on my machines. Everything upstream does still works; the
additions are:

- **Hold until a key.** With `timing.hold-until-key`, the finished tree stays on
  screen (the display manager starts behind it) and Enter, Esc or q hands the
  console over, so the tree is readable on a fast SSD instead of flashing past.
- **An operator intro.** With `look.intro`, a rain of glyphs and a panel with
  `operator@host`, a watchword and a progress bar cover the first
  `look.intro-for` of the boot.
- **The operator name in the header.** `look.operator` turns the header into
  `operator@host`, so the tree is signed.
- **A green phosphor theme.** `extras/bootvine.toml` sets every colour, so the
  whole screen is green on black, with failures left in red.

## Install on a fresh system

Arch:

```sh
sudo pacman -S --needed base-devel git dbus
```

Debian or Ubuntu:

```sh
sudo apt install build-essential pkg-config libdbus-1-dev libsystemd-dev git
```

Then:

```sh
git clone git@github.com:lxdwnpiper/boot-hengker.git
cd boot-hengker
extras/install.sh
```

The script builds the release binary, runs `bootvine install` (which copies the
binary and the units to `/usr/bin` and `/usr/lib/systemd/system` and enables
them, asking for `sudo`), backs up any existing `/etc/bootvine.toml`, installs
`extras/bootvine.toml`, and checks that the config parses.

To show the boot from the initrd as well, on mkinitcpio:

```sh
sudo bootvine install --initrd mkinitcpio
# add sd-bootvine after systemd in HOOKS in /etc/mkinitcpio.conf
sudo mkinitcpio -P
```

## Tune it

`/etc/bootvine.toml`:

| Key | Effect |
| --- | --- |
| `look.operator` | Name in the header and intro, rendered as `operator@host`. |
| `look.intro` | Show the intro splash. Ignored during a shutdown. |
| `look.intro-for` | How long the splash stays, such as `3s` or `5s`. |
| `timing.hold-until-key` | Wait for Enter at the end of the tree before the display manager appears. |

Check every edit, since an invalid file is ignored whole and the defaults return:

```sh
bootvine check /etc/bootvine.toml
```

Preview the splash without rebooting:

```sh
cargo run --example preview
```

## Upstream

`git remote add upstream https://codeberg.org/wvhulle/bootvine.git` and rebase
`main` on `upstream/main` to pick up changes. The additions are a handful of
commits, mostly in `bootvine/src/console_session/live_desk.rs`,
`bootvine/src/boot_view/intro.rs`, `bootvine/src/screen_settings/config_file.rs`
and `bootvine/src/console_session/watch_screen.rs`.

Licensed under GPL-3.0-or-later, as upstream is.
