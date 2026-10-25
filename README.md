# bootvine

A live tree of systemd jobs on the boot console, in the spirit of
nix-output-monitor. It replaces the scrolling kernel and systemd lines with the
units still pending and, in the room the tree leaves, a timeline of the slowest
ones that ran.

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/main/recordings/boot.gif" width="691" alt="A boot through the initrd into stage 2, with one failed unit">

It follows shutdowns and reboots too: the units being stopped, nested under the
units that have to stop first.

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/main/recordings/shutdown.gif" width="691" alt="A poweroff: units stopping under the targets waiting on them">

Passphrase prompts, such as a LUKS unlock in the initrd, are answered in place
over the tree.

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/main/recordings/passphrase.png" width="640" alt="A LUKS passphrase prompt floating over the initrd tree">

It needs systemd ≥ 246 as PID 1, libdbus and a virtual terminal. With a
systemd initrd it shows the boot from about a second after the kernel starts.

## Try it without installing

Neither of these touches your system's boot.

Play a scripted boot, poweroff and encrypted-root boot in your terminal, with
Rust installed:

```sh
git clone https://codeberg.org/wvhulle/bootvine.git
cd bootvine
cargo run --example boot
cargo run --example shutdown
cargo run --example luks
```

`q` quits. The encrypted root unlocks with the passphrase `demo`; `Esc` quits
while it asks.

Or, with Nix, boot a sample NixOS machine with slow and failing units in a
QEMU window:

```sh
nix run git+https://codeberg.org/wvhulle/bootvine#sample-machine-vm
```

It logs in as root; `poweroff` shows the shutdown.

## Documentation

- [Install](https://codeberg.org/wvhulle/bootvine/src/branch/main/docs/install.md)
  on NixOS, Ubuntu and other distributions.
- [Configuration](https://codeberg.org/wvhulle/bootvine/src/branch/main/docs/configuration.md)
  of colours, timing and the console.
- [Architecture](https://codeberg.org/wvhulle/bootvine/src/branch/main/docs/architecture.md):
  how bootvine follows the boot and takes the console.
- [Development](https://codeberg.org/wvhulle/bootvine/src/branch/main/docs/development.md)
  and the [code style](https://codeberg.org/wvhulle/bootvine/src/branch/main/docs/style.md).

## License

[GPL-3.0-or-later](https://codeberg.org/wvhulle/bootvine/src/branch/main/LICENSE).
