# Code style

- No comments; names and call flow carry the intent.
- Public fields over getters, durations over raw integers, existing crates over
  hand-written formatting.
- Iterator and stream combinators over loops.

Clippy runs with the pedantic lints and forbids absolute paths; formatting
needs the nightly toolchain the dev shell provides. See
[Development](development.md#before-committing) for the commands.
