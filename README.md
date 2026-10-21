# bootvine

A live tree of systemd jobs on the boot console, in the spirit of
nix-output-monitor. It replaces the scrolling kernel and systemd lines with the
units still pending and, in the room the tree leaves, a timeline of the slowest
ones that ran.

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/main/recordings/boot.gif" width="691" alt="A boot through the initrd into stage 2, with one failed unit">

It needs systemd ≥ 246 as PID 1, libdbus and a virtual terminal (it draws on
tty12). With a systemd initrd it starts about a second after the kernel and
follows the boot through the switch to the real root; without one it starts in
stage 2. While it runs it quiets the kernel console and systemd's status
output, and restores both when it hands the console back. Kernel messages from
before it starts still reach the screen; boot with `quiet`, or set
`boot.consoleLogLevel = 3` on NixOS, to keep those to errors.

It also follows shutdowns and reboots: the units being stopped, nested under
the units that have to stop first, until systemd hands over to
`systemd-shutdown`.

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/main/recordings/shutdown.gif" width="691" alt="A poweroff: units stopping under the targets waiting on them">

Passphrase prompts, such as a LUKS unlock in the initrd, are answered in place
over the tree.

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/main/recordings/passphrase.png" width="640" alt="A LUKS passphrase prompt floating over the initrd tree">

## Try it without installing

Neither of these touches your system's boot.

### Scripted boots in your terminal

From a clone, with Rust installed (see step 1 of the Ubuntu install below):

```sh
git clone https://codeberg.org/wvhulle/bootvine.git
cd bootvine
cargo run --example boot
cargo run --example shutdown
cargo run --example luks
```

These play a scripted boot, a poweroff and a boot from an encrypted root, with
no systemd involved. `q` quits. The encrypted root unlocks with the passphrase
`demo`; `Esc` quits while it asks.

### A sample machine in a VM

With Nix, and no clone needed:

```sh
nix run git+https://codeberg.org/wvhulle/bootvine#sample-machine-vm
```

This boots a NixOS machine with bootvine and a set of slow and failing sample
units in a QEMU window, from the initrd through stage 2. It logs in as root;
`poweroff` shows the shutdown. Its disk image is kept in
`~/.cache/bootvine-sample-machine.qcow2`.

## Install

### NixOS

1. Add the flake to your system flake's inputs and pass its module in:

   ```nix
   {
     inputs.bootvine = {
       url = "git+https://codeberg.org/wvhulle/bootvine";
       inputs.nixpkgs.follows = "nixpkgs";
     };

     outputs = { nixpkgs, bootvine, ... }: {
       nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
         modules = [
           bootvine.nixosModules.default
           ./configuration.nix
         ];
       };
     };
   }
   ```

2. Enable it in `configuration.nix`:

   ```nix
   {
     services.bootvine.enable = true;
   }
   ```

3. Optionally, follow the boot from the initrd and keep early kernel messages
   to errors:

   ```nix
   {
     boot.initrd.systemd.enable = true;
     boot.consoleLogLevel = 3;
   }
   ```

4. Rebuild and reboot:

   ```sh
   sudo nixos-rebuild switch --flake .#my-host
   ```

### Ubuntu

1. Install the build dependencies and Rust. The repository pins a nightly
   toolchain in `rust-toolchain.toml`, which rustup fetches on the first
   build:

   ```sh
   sudo apt install build-essential pkg-config libdbus-1-dev git curl
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. Clone, build and install the binary and the systemd units, and enable
   them. `cargo xtask install` builds as you and asks `sudo` only for the
   copying and `systemctl enable`:

   ```sh
   git clone https://codeberg.org/wvhulle/bootvine.git
   cd bootvine
   cargo xtask install
   ```

   This is enough for bootvine to show stage 2 and shutdown. It installs
   `/usr/bin/bootvine` and `bootvine.service` and `bootvine-shutdown.service`
   in `/usr/lib/systemd/system`.

3. Optionally, follow the boot from the initrd. This needs a dracut initramfs
   with systemd in it; initramfs-tools has no systemd, so on releases that use
   it bootvine starts in stage 2 only.

   ```sh
   cargo xtask install --initrd dracut
   sudo dracut --force
   ```

   This adds the dracut module in `/usr/lib/dracut/modules.d/90bootvine` and
   enables it in `/etc/dracut.conf.d/bootvine.conf`.

4. Reboot.

`cargo xtask uninstall` removes all of it again.

### Other distributions

Install as in the Ubuntu steps. For the initrd, use the dracut module as above,
or on mkinitcpio run `cargo xtask install --initrd mkinitcpio`, which installs
`/usr/lib/initcpio/install/sd-bootvine`, then add `sd-bootvine` after `systemd`
in `HOOKS` in `/etc/mkinitcpio.conf` and run `sudo mkinitcpio -P`.
`--destdir <dir>` stages the files under `<dir>` instead, for packaging.

## Configure

`/etc/bootvine.toml`. Every key is optional; `bootvine defaults` prints them
all with their defaults, and `bootvine defaults --format nix` prints the same as
a Nix attribute set. A file that changes two of them:

```toml
[colors.tree]
running-marker = "light-yellow"

[timing]
keep-final-frame-for = "500ms"
```

Each `[colors.*]` table covers one part of the screen: `header`, the finished
`stages`, the unit `tree`, the `timeline`, the `footer` tallies and the
`password` popup. Values are the 16 console colour names. To change how those look, set the console palette: `console.colors` on
NixOS, or the `vt.default_red`, `_grn` and `_blu` kernel parameters elsewhere.

`keep-finished-units-for` is how long a finished unit stays in the tree, faded,
before it leaves. Runs shorter than `hide-runs-shorter-than` get no timer and no
timeline bar. `keep-final-frame-for` is how long the last frame stays before the
console is handed back; `--hold` overrides it. When a display manager is
starting, bootvine instead waits for it to switch to its own terminal, up to
`wait-for-display-manager`, so the console's status lines never show in
between.

`bootvine check /etc/bootvine.toml` reports unknown keys and bad values; at
boot, bootvine ignores an invalid file and uses the defaults. The initrd hooks
copy the file into the initramfs, so rebuild it after editing.

On NixOS, set the same tables under `services.bootvine.settings`; the rebuild
fails if `bootvine check` rejects them:

```nix
services.bootvine.settings = {
  colors.tree.running-marker = "light-yellow";
  timing.keep-final-frame-for = "500ms";
};
```

## License

See [LICENSE](LICENSE).
