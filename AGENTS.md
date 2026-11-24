# AGENTS.md — newm-rs

Shared guide for coding agents; `CLAUDE.md` includes it.

Maintainer machine rules live outside this repository in `~/.config/newm-dev/AGENTS.md`; if that
file exists, read it before building or testing.

## Build and test

Build and test on a machine with room for a full Rust build, one build at a time.
- Bar, run with `--features canvas` and again with `--features canvas,telemetry`: zero warnings,
  `cargo clippy --all-targets --features <set> -- -D warnings` clean, every test green
  (`cargo test --features <set>`). The canvas, session and usage code builds only with
  `canvas`; without it, a test filter such as `session::` matches nothing. The usage log and its
  tests build only with `telemetry`, which no default build has.
- A newm-canvas change keeps its Rocq proof's gate green (`newm-canvas/proof/README.md`).
- Format with `cargo +nightly fmt --all`.
- Never use npm, npx or pip.

## Commits

- One coherent change per commit, so each one reverts alone.
- The subject names the area and what changed, `area: what changed` (for example
  `canvas: zoom-reset returns the camera to 1:1`); the body says why.
- No attribution trailers.
