# Development

The tree's layout, the Cargo features and the packaging renames are in
[SPEC §2.1](SPEC.md#21-a-fork-of-niri-that-tracks-upstream); the tests are in
[SPEC §11](SPEC.md#11-testing).

## Building

With Nix:

```sh
nix build                  # packages.newm-rs: bin/newm-rs, bin/newm-rs-lock, the session files
nix build .#newm-rs-debug  # the same, unoptimized and unstripped
nix run . -- --version
nix develop                # a shell with the toolchain and the libraries for cargo
```

The package builds with `canvas`, `dbus`, `systemd` and `xdp-gnome-screencast`, and without
`text` (newm-rs draws stand-ins for niri's text panels) and `telemetry`. Override
`withText = true` to render text with pango, `withTelemetry = true` to write the
[usage log](session.md#the-usage-log), or `withCanvas = false` for niri's scrolling layout.

With cargo, on Linux, with Rust and the libraries niri needs (libinput, libxkbcommon, libgbm,
libdisplay-info 0.3, libseat, Mesa's EGL, Wayland, cairo and pango, PipeWire, D-Bus, systemd's
libudev):

```sh
cargo build --release --features canvas             # add ,telemetry for the usage log
cargo build --release -p newm-rs-lock               # also needs PAM
```

Without `--features canvas` the build is niri with newm-rs's other changes and the scrolling
layout. `dbus`, `systemd`, `xdp-gnome-screencast` and `text` are on by default; `telemetry`,
`dinit` and the Tracy profiling features are off.

## The bar

Every change keeps all of these green, with zero warnings:

```sh
cargo clippy --all-targets --features canvas -- -D warnings
cargo test --features canvas
cargo test -p newm-canvas
cargo test -p newm-canvas --release --test bench -- --ignored --nocapture   # timings, by hand
cargo +nightly fmt --all
```

The canvas, session and usage-log tests build only with `--features canvas`; without it, a filter
such as `session::` matches nothing.

A change to `newm-canvas` also keeps its Rocq proof's gate green: the gate compiles the model in
`newm-canvas/proof/`, re-checks every claim, compares claims and anchored source lines with
`proof.lock.json`, and runs the Rust replay harness. See
[newm-canvas/proof/README.md](../../newm-canvas/proof/README.md) and
[SPEC §3.15](SPEC.md#315-proof-obligations).

The compat VM suite runs real apps against the packaged session:
`nix build .#checks.x86_64-linux.compat-newm-rs -L` runs its smoke suite, `compat-newm-rs-<suite>`
the others ([protocols.md](protocols.md#the-compat-vm-suite)).

## Tracking upstream niri

newm-rs follows niri's `main` branch by merging it:

```sh
git remote add upstream https://github.com/niri-wm/niri.git
git fetch upstream
git merge-base HEAD upstream/main
```

Most of newm-rs lives in its own files and behind the `canvas` feature, which keeps upstream
merges small. `docs/wiki/` stays as upstream has it: niri-config's `tests/wiki-parses.rs`
parses its KDL examples.
