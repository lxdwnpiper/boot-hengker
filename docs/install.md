# Install

bootvine always shows stage 2 and shutdown. To show the boot from the initrd
as well, the initrd has to run systemd.

## NixOS

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

   The initrd is a systemd initrd by default since NixOS 26.05, and the only
   kind after it. On NixOS 25.11 and earlier, also set
   `boot.initrd.systemd.enable = true` to show the boot from the initrd.

3. Rebuild and reboot:

   ```sh
   sudo nixos-rebuild switch --flake .#my-host
   ```

## Ubuntu

1. Install the build dependencies and Rust:

   ```sh
   sudo apt install build-essential pkg-config libdbus-1-dev git curl
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. Build bootvine into `~/.cargo/bin`:

   ```sh
   cargo install --locked --git https://codeberg.org/wvhulle/bootvine bootvine
   ```

3. Install it for the system and enable its units. It asks `sudo` itself for
   the copying and `systemctl enable`:

   ```sh
   bootvine install
   ```

   This copies the binary to `/usr/bin/bootvine`, and `bootvine.service` and
   `bootvine-shutdown.service` to `/usr/lib/systemd/system`, which is enough
   for stage 2 and shutdown. The copy in `~/.cargo/bin` is no longer needed;
   `cargo uninstall bootvine` removes it.

4. Optionally, show the boot from the initrd. This needs a
   [dracut](https://github.com/dracut-ng/dracut-ng) initramfs with systemd in
   it; initramfs-tools has no systemd, so on releases that use
   it bootvine starts in stage 2 only.

   ```sh
   bootvine install --initrd dracut
   sudo dracut --force
   ```

   This adds the dracut module in `/usr/lib/dracut/modules.d/90bootvine` and
   enables it in `/etc/dracut.conf.d/bootvine.conf`.

5. Reboot.

## Other distributions

Install as in the Ubuntu steps, with your distribution's packages for a C
toolchain, `pkg-config` and the libdbus headers.

For the initrd, use the dracut module as above, or on
[mkinitcpio](https://wiki.archlinux.org/title/Mkinitcpio):

1. Install the hook in `/usr/lib/initcpio/install/sd-bootvine`:

   ```sh
   bootvine install --initrd mkinitcpio
   ```

2. Add `sd-bootvine` after `systemd` in `HOOKS` in `/etc/mkinitcpio.conf`.

3. Rebuild the initramfs:

   ```sh
   sudo mkinitcpio -P
   ```

## Packaging

`bootvine install --destdir <dir>` stages the files under `<dir>` instead of
the root, without `sudo` or `systemctl`, and combines with `--initrd` and
`--prefix`. The units' `ExecStart` follows `--prefix`. The initrd hooks expect
the default `/usr`.

## Uninstall

`bootvine uninstall` disables and removes everything `bootvine install` put in
place, with the same `--prefix` and `--destdir`. On NixOS, remove
`services.bootvine.enable` and rebuild.
