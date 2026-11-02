# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://codeberg.org/wvhulle/bootvine/compare/v0.1.1...v0.1.2) - 2026-11-02

### Added

- show the kernel's image loading progress while resuming from hibernation
- say when the screen is waiting for the system manager

### Changed

- reduce screen spacing to a small named set and declarative layouts

### Fixed

- repaint the whole screen while the root switches
- finish console setup before resuming from hibernation

## [0.1.1](https://codeberg.org/wvhulle/bootvine/compare/v0.1.0...v0.1.1) - 2026-10-30

### Changed

- read the release environment from the crate build and generate the sample machine apps
- keep recordings on an orphan branch

## [0.1.0](https://codeberg.org/wvhulle/bootvine/releases/tag/v0.1.0) - 2026-10-25

### Added

- follow shutdowns on the boot console
- font-aware glyphs, quieter tree and timeline
- support luks
- better tree view
- show the demo passphrase and say when an answer was wrong
- print the version with --version
- animate at 60 fps with timers in hundredths and bars that grow by eighths
- move install commands from xtask into clap subcommands
- show timeout timer

### Build

- raise MSRV to 1.91 for const Duration::from_mins
- shrink the release binary from 2.82 MB to 1.74 MB
- add tokio and futures for the stream loop

### Changed

- reduce polling
- build graph first and then render
- give modules descriptive names
- build the waiting graph once with petgraph
- fit and fold the waiting forest in the view
- split model and view into files under 500 lines
- decide phase, milestones and quit keys once
- keep unit timestamps as one Activity
- derive errors with thiserror
- group modules by data-flow stage behind a thin main
- parse systemd state strings at the bus boundary
- parse systemd strings with serde_plain
- reorganise flake, tests and repo tasks
- drive the spinner by time instead of frame count
- drive the watch loop with async streams in the Elm style

### Documentation

- compact README, move workflow to CONTRIBUTING
- add boot screenshots
- GIFs and install steps
- automate gif generation
- improve documentation of settings using a subcommand
- add metadata for crates.io

### Fixed

- stand down when started after boot finished
- separation of tree and timeline
- arrow glyph
- add luks prompt to fix broken on external keyboard
- shutdown
- make prompt LUKS more visible
- passing control to display manager too early
- add license
- animation timeline loops
- handover too early
- yield the console once the display manager starts
- deal with scripted initrd changes in nixos unstable
- Ubuntu bugs

### Miscellaneous

- upgrade inputs
- set initial version 0.1.0

### Other

- live systemd boot dependency tree on a dedicated VT
- libdbus feed thread, dedicated VT, stage sections, bead timeline, demo example
- single-threaded feed, Duration timestamps, fields over getters, jiff formatting
- portable unit, dracut and mkinitcpio hooks, self-quieting console, minimal NixOS module
- palette options, finished-run timeline, readable unit names, repaint on reconnect, handover frame
- rename boot-tree to bootvine

### Performance

- small improvements
- reach each sibling once when nesting waiting units
- await systemd replies instead of blocking the screen
- rebuild the waiting tree only when the boot changed
- repaint the whole console only when something disturbed it

### Testing

- shutdown demo
- share boot fixtures between model and view tests
- measure the cost of one boot frame
