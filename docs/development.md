# Development

## Layout

| Folder | Holds |
| --- | --- |
| `bootvine/` | the published crate: the library, the `bootvine` binary and the unit and initrd files it installs |
| `scripted-demos/` | the scripted boot, shutdown and passphrase demos, and the VHS tapes that record them |
| `xtask/` | repository tasks run with `cargo xtask` |
| `vm-tests/` | the sample NixOS machines, one folder each with its VM check |
| `nix/` | the crane builds and the NixOS module |
| `recordings/` | ignored output of `record-screencasts`: the scripted demo GIFs and the sample machine WebP recordings, published on the `recordings` branch for the README and the [guide](guide.md) |

`cargo build` and `cargo test` act on every crate. `cargo run` needs the binary:
`--bin bootvine`, `--bin scripted-boot` and so on.

## Setup

Work inside the dev shell (`direnv allow` once, with
[direnv](https://direnv.net)). Version control is
[jj](https://github.com/jj-vcs/jj), not the `git` CLI.

## Before committing

```sh
cargo clippy --workspace --all-targets
cargo fmt --all
```

Clippy runs with the pedantic lints and forbids absolute paths; formatting
needs the nightly toolchain the dev shell provides.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org).
jj does not run git hooks, so check the commits you are about to push with:

```sh
nix run .#check-conventional-commits -- origin/main..HEAD
```

Git users can install the same commit-message check and a formatting check as
hooks with `prek install` ([prek](https://github.com/j178/prek), from the dev
shell) or `pre-commit install` ([pre-commit](https://pre-commit.com)).

## Continuous integration

`.woodpecker/rust-and-flake-checks.yaml` runs on Codeberg's
[Woodpecker CI](https://woodpecker-ci.org) for every push and pull request: the bootvine build with its tests, the xtask build, clippy,
rustfmt, an evaluation of every flake output, and the commit-message check on
pull requests. Builds on `main` are pushed to the `wvhulle` [Cachix](https://www.cachix.org) cache, which
`nixConfig` in `flake.nix` offers as a substituter. The two VM checks need KVM,
which Codeberg's runners lack, so they only run locally.

## Releases

`.woodpecker/release-pr-and-publish.yaml` runs
[release-plz](https://release-plz.dev) after the checks pass on `main`. It
keeps a pull request labelled `release` open on Codeberg that bumps the version
in `Cargo.toml` and adds the new commits to `CHANGELOG.md`. Only `feat`, `fix`
and `perf` commits open one; a `!` or a `BREAKING CHANGE` footer bumps the
minor version while bootvine is below 1.0. Merging that pull request publishes
the crate to crates.io, tags the merge `v<version>` and creates a Codeberg
release with the changelog. Settings are in `release-plz.toml`.

The pipeline needs two Woodpecker secrets: `codeberg_token`, a Codeberg access
token with repository write access, and `cargo_registry_token`, a crates.io
token that may publish new crates and updates. Preview the next release
locally from the dev shell with `release-plz update`.

## Testing

| What | Command |
| --- | --- |
| Model and render tests | `cargo test --workspace` |
| Scripted demos | `cargo run --bin scripted-boot` (also `scripted-shutdown`, `scripted-luks-unlock`) |
| Sample machines in a window | `nix run .#sample-vm-<machine>`, see the [guide](guide.md#sample-machines) |
| A sample machine's check | `nix build .#checks.x86_64-linux.vm-<machine>` |

To watch your own user manager start the sample units:

```sh
units=$(nix build --no-link --print-out-paths .#sample-unit-files)
systemctl --user link --runtime $units/*
systemctl --user start --no-block bootvine-sample-services.target
cargo run --bin bootvine -- --user --target bootvine-sample-services.target --hold 3s
systemctl --user stop $(ls $units)
systemctl --user disable --runtime $(ls $units)
```

Everything a sample machine needs sits in its own folder under `vm-tests/`:
`machine.nix`, `check.nix` and `check.py`. `vm-tests/shared/` holds the
settings every machine shares, the window setup for `nix run`, the console
helpers for the checks and the screen recorder. A new machine is a new folder
plus its name in `vm-tests/default.nix`.

The VM checks read the console as text, hold the boot at fixed points and wait
on marker files instead of sleeping. The failures check starts each failing
unit by hand, so it knows which one is newest in the panel. While a check runs,
`ScreenRecording` takes a frame every half second through a second QEMU
monitor socket, so it never competes with the driver's own monitor; the frames
land in the check's output under `recording/`. To step through a check by hand, run
`nix run .#checks.x86_64-linux.vm-slow-services.driverInteractive` and
call `test_script()`.

Prefix VM and recording commands with `env -u LD_LIBRARY_PATH` when your shell
exports libraries that clash with [QEMU](https://www.qemu.org) or Chromium.

## Repository tasks

- `nix run .#record-screencasts` re-records the animations from the scripted
  demos and turns the frames each VM check captured into
  `recordings/<machine>.webp`, for the README and the [guide](guide.md).
- `nix run .#publish-recordings` force-pushes `recordings/` as the single,
  parentless commit of the `recordings` branch, which the README and the guide
  link to. Keeping the recordings off `main` keeps them out of every clone and
  flake fetch.
- `cargo xtask sync-default-config` rewrites the defaults block in
  [configuration](configuration.md#defaults) from `bootvine defaults`.
- `cargo run --bin bootvine -- install --destdir <dir> --initrd dracut` stages an install
  without touching the system; `cargo run --bin bootvine -- uninstall --destdir <dir>` removes
  it again.

## Code style and design

- [Code style](style.md).
- [Architecture](architecture.md): the event loop, the console
  takeover and the workarounds for systemd's private socket.
