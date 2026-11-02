# Guide

What bootvine shows on the console, part by part, with the recordings that go
with each. The GIFs come from the [scripted demos](#scripted-demos); the WebP
animations are real boots of the [sample machines](#sample-machines), recorded
by their [VM checks](development.md#testing).

## Reading the screen

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/recordings/boot.gif" width="691" alt="A boot through the initrd into stage 2, with one failed unit">

From top to bottom:

1. **The header**: the host name, the system and its generation on NixOS
   (otherwise the `PRETTY_NAME` from `os-release`), and the kernel. Once a
   stage has finished, the total time since the kernel started sits on the
   right.
2. **The stages**: each finished stage, such as `Initrd`, with how long it
   took, how many units finished in it and how many of them failed. The stage
   still running is the last line, ending in `Running`.
3. **The tree** of units with a pending job. Each unit is nested under the unit
   waiting on it, and the milestone targets (`sysinit.target`, `basic.target`,
   `multi-user.target`, `graphical.target`) open the levels:
   - `◆` is the stage being worked on, `◇` one still to come, `√` one reached
     (`*`, `o` and `+` on console fonts without those glyphs);
   - `[|]` with a spinner is running, `[ ]` is waiting, `[√]` finished in the
     last couple of seconds (`keep-finished-units-for`);
   - `[×]` in bold red broke or timed out; dimmed red failed only because a
     unit it needed did;
   - the `Running` column counts up while a unit runs, and after 5 seconds the
     `Timeout` column shows the limit systemd will stop it at, red once it
     is passed;
   - a unit waiting on exactly one other, which waits on exactly one other,
     folds into one row joined by `─`; mounts and devices show their
     description instead of their unit name.

   When the tree is taller than the screen, finished and waiting rows give way
   first, and a last line counts the units left out.
4. **The failure panel**, only once a unit broke or timed out; see
   [failures](#failures).
5. **The timeline**, in the room left below the tree: the milestones reached,
   with how many units they took, the slowest finished units (up to five, and
   none shorter than `hide-runs-shorter-than`) and the units still running,
   as bars against seconds since the kernel started. A `│` marks the switch
   from the initrd to the real root.
6. **The footer**: the units done, running, waiting and failed so far.

## Booting through the initrd

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/recordings/slow-services.webp" width="640" alt="A NixOS machine booting through the initrd into stage 2, handing over to a greetd login, then powering off">

The `slow-services` machine boots with a held hardware probe in the initrd,
slow units in stage 2 and one failing unit, hands over to its greetd login
and powers off. With a systemd initrd, bootvine starts about a second after
the kernel:

- **The initrd stage**: udev settling, the root device and its file systems
  under `initrd.target`.
- **The switch to the real root**: the initrd moves into the stages at the
  top, with its time and unit count, and the tree starts again under the
  default target. Until the new manager answers, the stage line says it is
  waiting for it. The timeline keeps the initrd's runs, left of the `│`.
- **Stage 2** up to `graphical.target`.
- **The handover**: bootvine hands the console back once the default target is
  reached, or earlier, once the display manager has switched to its own
  terminal.

How bootvine gets in that early and survives the switch is in
[architecture](architecture.md#starting-in-the-initrd).

### Without a display manager

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/recordings/text-console.webp" width="640" alt="The same boot on a machine without a display manager, ending at a text login">

The `text-console` machine boots the same units without greetd. With no
display manager to wait for, bootvine hands the console back as soon as the
default target is reached, and the text login on `tty1` comes forward.

### Without a systemd initrd

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/recordings/scripted-initrd.webp" width="640" alt="The same boot with NixOS's scripted initrd: the kernel and initrd lines scroll past before bootvine starts in stage 2">

The `scripted-initrd` machine uses NixOS's older scripted initrd, which has no
systemd. The initrd's lines scroll past as usual, and bootvine starts in
stage 2, from `sysinit.target` on.

## Resuming from hibernation

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/recordings/hibernate-resume.webp" width="640" alt="The tree held still during a resume, with the kernel's image loading progress in a band across the middle of the screen">

Resuming from hibernation looks different from a boot, because the kernel
freezes every process before it loads the image, bootvine included. The tree
stops moving, and a band across the middle of the screen shows the kernel's own
progress:

```
─── Looking for a hibernated session ─────────────────────────────
PM: Image loading progress:  60%

──────────────────────────────────────────────────────────────────
```

The text comes straight from the kernel, which is why it is a line of text and
not a bar. The screen stays like this until the restored system takes over.
Bootvine does not draw again, because the restore replaces it along with the
rest of the boot kernel's memory.

The last lines show how the time was spent. `Read ... in N seconds` is the
image load. `Suspending console(s)` means the load has finished and the kernel
is about to jump into the restored system. If the screen stays on that line for
a long time, the wait is the restored kernel bringing devices back up, not the
disk.

With no image to restore, `systemd-hibernate-resume` finishes at once and the
band disappears. Some kernel console setups
switch to a blank terminal for the load; the same lines then appear there.

## Shutdown

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/recordings/shutdown.gif" width="691" alt="A poweroff: units stopping under the targets waiting on them">

On a poweroff, reboot or halt the tree turns around:

- its root is the target being reached, such as `poweroff.target`;
- each unit being stopped sits under the units that have to stop before it,
  so the unit holding up the shutdown is at the bottom of its branch;
- `shutdown.target`, `umount.target` and `final.target` open the levels;
- the screen stays until systemd hands over to `systemd-shutdown`.

## Passphrase prompts

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/recordings/encrypted-volume.webp" width="640" alt="A LUKS passphrase prompt floating over the initrd tree, a wrong answer, then the unlock">

The `encrypted-volume` machine asks for a LUKS passphrase in the initrd; its
recording shows a wrong answer, then the right one.


When a unit asks for a passphrase, such as `systemd-cryptsetup@….service`
unlocking a [LUKS](https://gitlab.com/cryptsetup/cryptsetup) volume, the
question floats over a dimmed tree:

- typing shows one mark per character;
- `Enter` sends it, and the popup says it is checking;
- a wrong passphrase brings the prompt back with "Wrong passphrase, try
  again";
- once unlocked, the popup closes and the boot carries on.

If another password agent, such as Plymouth's, was already asking when
bootvine started, bootvine waits behind it.

## Failures

<img src="https://codeberg.org/wvhulle/bootvine/raw/branch/recordings/failures.webp" width="640" alt="Units failing one after another, each landing in the failure panel with its reason and last journal lines">

A unit that broke or timed out gets an entry in the failure panel, newest
first:

- **The headline**: the unit and why it failed, such as `exited with status 2`,
  `killed by SIGSEGV`, `timed out`, `restarted too often` or
  `exited with status 203` (systemd could not run the command). A service that
  reported a status with `sd_notify` adds it, as in
  `exited with status 1: Cannot reach database`.
- **The journal lines**: up to six of the unit's last messages from this boot,
  the same ones `journalctl -u` shows, without systemd's own "Starting…" and
  "Failed with result…" lines.
- **Out of room**: the newest failure keeps its lines, older ones shrink to
  their headline, and a last line counts the rest.
- **What stays out**: a unit that failed only because one it needed did stays
  dimmed in the tree, so the panel points at the unit that actually broke.
  The footer counts both.

Failures from the initrd stay in the panel after the switch to the real root.
The recording is the `failures` machine, where every kind of failure systemd
reports happens one after another.

## Trying it

Neither of these touches your system's boot.

### Scripted demos

Play a scripted boot, poweroff and passphrase prompt in your terminal, with
[Rust](https://www.rust-lang.org/tools/install) installed:

```sh
git clone https://codeberg.org/wvhulle/bootvine.git
cd bootvine
cargo run --bin scripted-boot
cargo run --bin scripted-shutdown
cargo run --bin scripted-luks-unlock
```

`q` quits. The encrypted root unlocks with the passphrase `demo`; `Esc` quits
while it asks. These are the scenarios the animations above were recorded
from.

### Sample machines

Each sample machine boots in a [QEMU](https://www.qemu.org) window with
[Nix](https://nixos.org/download/), and logs in as root at the end. `poweroff`
shows the shutdown.

| Run | Shows | Recording |
| --- | --- | --- |
| `nix run .#sample-vm-slow-services` | slow units in the initrd and stage 2, one failing unit, and the handover to a greetd login | [booting through the initrd](#booting-through-the-initrd) |
| `nix run .#sample-vm-text-console` | the same units without a display manager, handing over at the default target | [without a display manager](#without-a-display-manager) |
| `nix run .#sample-vm-scripted-initrd` | the same units with NixOS's scripted initrd, so bootvine starts in stage 2 only | [without a systemd initrd](#without-a-systemd-initrd) |
| `nix run .#sample-vm-encrypted-volume` | a LUKS volume unlocked in the initrd; the passphrase is `sample passphrase` | [passphrase prompts](#passphrase-prompts) |
| `nix run .#sample-vm-hibernate-resume` | a swap disk to hibernate to; run `systemctl hibernate`, then start the machine again | [resuming from hibernation](#resuming-from-hibernation) |
| `nix run .#sample-vm-failures` | a firmware failure in the initrd, then units that exit with an error, crash, time out, cannot be run, restart too often, report a status, die after starting, and one that fails only through its dependency | [failures](#failures) |

Each machine keeps its disk in `~/.cache/bootvine-<machine>.qcow2`; delete it
to start fresh. The machines live in `vm-tests/`, one folder each, next to the
check that drives and records it.
