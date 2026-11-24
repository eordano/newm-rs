#!/usr/bin/env bash
# The Rust side of the proof: the replay harness on a copy of the crate outside the niri
# workspace, including the tests of the repaired findings F01 and F02.
set -euo pipefail
src=${PROOF_ROOT_CODE:?run through proofgate}
proof=$(pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -r "$src/src" "$src/tests" "$src/Cargo.toml" "$work/"
printf '\n[workspace]\n' >> "$work/Cargo.toml"
export CARGO_TARGET_DIR="$proof/_build/cargo"
run() {
  if command -v cargo >/dev/null; then (cd "$work" && cargo "$@")
  else (cd "$work" && nix shell --inputs-from "${NIX_INPUTS_FROM:-$proof/../..}" nixpkgs#cargo nixpkgs#rustc -c cargo "$@"); fi
}
run test --offline --quiet --release
