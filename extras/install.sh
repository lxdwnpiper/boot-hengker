#!/bin/sh
# Build and install this fork with the piperCs theme.
#
#   extras/install.sh
#
# Needs a C toolchain, pkg-config and the libdbus and libsystemd headers:
#   Arch:   sudo pacman -S --needed base-devel git dbus
#   Ubuntu: sudo apt install build-essential pkg-config libdbus-1-dev libsystemd-dev git
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(dirname "$here")

cargo build --release --manifest-path "$root/Cargo.toml" --package bootvine

if [ -e /etc/bootvine.toml ]; then
	sudo cp /etc/bootvine.toml "/etc/bootvine.toml.bak.$(date +%Y%m%d%H%M%S)"
fi

sudo "$root/target/release/bootvine" install
sudo install -Dm644 "$here/bootvine.toml" /etc/bootvine.toml
bootvine check /etc/bootvine.toml

echo
echo "Installed. Reboot to see the tree, and press Enter at the end to reach the"
echo "display manager."
echo
echo "For the boot from the initrd as well, on mkinitcpio:"
echo "  sudo bootvine install --initrd mkinitcpio"
echo "  add sd-bootvine after systemd in HOOKS in /etc/mkinitcpio.conf"
echo "  sudo mkinitcpio -P"
