# Architecture

How bootvine follows a boot and a shutdown, and what it changes on the console
while it does. None of this is needed to install or configure it.

## Requirements

- **systemd ≥ 246 as PID 1.** bootvine reads the jobs and units from the
  service manager over D-Bus.
- **[libdbus](https://www.freedesktop.org/wiki/Software/dbus/).** It talks to
  PID 1 over systemd's private socket, which
  [zbus](https://github.com/dbus2/zbus) cannot use; see [systemd's private socket](#systemds-private-socket).
- **A virtual terminal.** It draws on `tty12` and switches the console to it.

## Lifecycle

### Boot

1. **Start.** `bootvine.service` is wanted by `sysinit.target` and has no
   default dependencies, so it starts early. With a systemd initrd it starts
   there, about a second after the kernel; without one it starts in stage 2.
2. **Switch to the real root.** In the initrd, bootvine survives the switch
   and keeps drawing; the tree moves the initrd into the finished stages.
3. **Stage 2.** The tree shows the jobs still pending under the default
   target, nested under the units waiting on them.
4. **Handover.** bootvine hands the console back once the default target is
   reached and the final frame has stayed for `keep-final-frame-for`. When a
   display manager is starting, it waits instead for the display manager to
   switch to its own terminal, up to `wait-for-display-manager`.

### Shutdown

`shutdown.target` conflicts with `bootvine.service` and wants
`bootvine-shutdown.service`, which runs `bootvine --shutdown`. It shows the
units being stopped, nested under the units that have to stop first, until
systemd hands over to `systemd-shutdown`.

## The console

While it runs, bootvine:

- **brings `tty12` to the foreground**, and takes it back if something else
  switches away during the boot;
- **quiets the kernel console** by setting the console log level in
  `/proc/sys/kernel/printk` to 1, so only emergency messages reach the
  screen, and sets it again if something raises it;
- **silences systemd's status output** with the manager's `SetShowStatus`
  call.

It restores the log level and the status output when it hands the console
back. Kernel messages from before it starts are already on screen; see
[early kernel messages](configuration.md#early-kernel-messages).

### Passphrase prompts

bootvine acts as a systemd
[password agent](https://systemd.io/PASSWORD_AGENTS/): it watches
`/run/systemd/ask-password` and answers requests, such as a
[LUKS](https://gitlab.com/cryptsetup/cryptsetup) unlock, in a
popup over the tree. If another agent's prompt is already showing when it
starts, it stays behind it and comes forward once the prompt is answered.

## The units

| Unit | Wanted by | Notable settings |
| --- | --- | --- |
| `bootvine.service` | `sysinit.target` | `IgnoreOnIsolate=yes`, `Before=display-manager.service`, process name `@bootvine` |
| `bootvine-shutdown.service` | `shutdown.target` | `After=bootvine.service display-manager.service` |

Both draw on `/dev/tty12` with `TTYReset=yes` and log to the journal.

## One event loop

Everything runs on one thread. The service manager, the keyboard, the frame
timer, password prompts and termination each produce a stream of inputs; the
streams are merged, applied in batches to a single screen state, and the screen
is drawn once per batch. The live boot and the scripted demos share this loop
and differ only in where their inputs come from.

## systemd's private socket

- zbus rejects it, because PID 1 sends under its well-known name rather than a
  unique one. libdbus works.
- libdbus sends its authentication handshake together with the first message,
  which systemd leaves unread, so a no-reply ping goes out right after
  connecting.

## Surviving the switch to the real root

Three things keep bootvine alive from the initrd into stage 2:

- a process name starting with `@`, which systemd's switch-root spares from
  being killed;
- a unit that is not stopped on isolate (`IgnoreOnIsolate=yes`), since the
  initrd isolates `initrd-switch-root.target`;
- a chroot through a file descriptor to the new root, taken as soon as the
  switch is queued.
