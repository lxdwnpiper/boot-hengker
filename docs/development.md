# Development

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
| Scripted demos | `cargo run --example boot` (also `shutdown`, `luks`) |
| Sample machine in a window | `nix run .#sample-machine-vm` |
| Boot and shutdown in a VM | `nix build .#checks.x86_64-linux.vm-boot-and-shutdown` |
| Passphrase prompt in a VM | `nix build .#checks.x86_64-linux.vm-luks-passphrase` |

To watch your own user manager start the sample units:

```sh
units=$(nix build --no-link --print-out-paths .#sample-unit-files)
systemctl --user link --runtime $units/*
systemctl --user start --no-block bootvine-demo.target
cargo run -- --user --target bootvine-demo.target --hold 3s
systemctl --user stop $(ls $units)
systemctl --user disable --runtime $(ls $units)
```

The VM checks read the console as text, hold the boot at fixed points and wait
on marker files instead of sleeping. To step through one by hand, run
`nix run .#checks.x86_64-linux.vm-boot-and-shutdown.driverInteractive` and
call `test_script()`.

Prefix VM and recording commands with `env -u LD_LIBRARY_PATH` when your shell
exports libraries that clash with [QEMU](https://www.qemu.org) or Chromium.

## Repository tasks

- `nix run .#record-screencasts` re-records the README animations from the
  scripted demos and copies the passphrase screenshot from the VM check.
- `cargo xtask sync-default-config` rewrites the defaults block in
  [configuration](configuration.md#defaults) from `bootvine defaults`.
- `cargo run -- install --destdir <dir> --initrd dracut` stages an install
  without touching the system; `cargo run -- uninstall --destdir <dir>` removes
  it again.

## Code style and design

- [Code style](style.md).
- [Architecture](architecture.md): the event loop, the console
  takeover and the workarounds for systemd's private socket.
