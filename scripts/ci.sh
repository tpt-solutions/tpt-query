#!/usr/bin/env bash
# Runs the checks that gate a change: formatting, lints, tests, benches, and the
# no_std and wasm builds and tests.
#
# Requires the wasm32-unknown-unknown and thumbv7em-none-eabihf targets:
#   rustup target add wasm32-unknown-unknown thumbv7em-none-eabihf
# The wasm tests also need a wasm-bindgen-cli that matches the lockfile:
#   cargo install -f wasm-bindgen-cli --version <version in Cargo.lock>
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> fmt"
cargo fmt --all -- --check

echo "==> clippy"
cargo clippy --all-features --all-targets -- -D warnings

echo "==> test (all features)"
cargo test --all-features

echo "==> test (no features)"
cargo test --no-default-features

echo "==> bench (compile only)"
cargo bench --no-run

echo "==> no_std build (thumbv7em-none-eabihf)"
cargo build --no-default-features --target thumbv7em-none-eabihf

echo "==> wasm test (wasm32-unknown-unknown, under node)"
cargo test --target wasm32-unknown-unknown --no-default-features --test wasm

echo "All checks passed."
