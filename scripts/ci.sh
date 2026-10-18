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

echo "==> clippy (no features, then all features)"
cargo clippy --no-default-features --all-targets -- -D warnings
cargo clippy --all-features --all-targets -- -D warnings

echo "==> test (all features)"
cargo test --all-features

echo "==> test (spatial only)"
cargo test --no-default-features --features spatial

echo "==> test (no features)"
cargo test --no-default-features

echo "==> examples"
cargo run --example quickstart
cargo run --example doc_outline
cargo run --example custom_source
cargo run --example spreadsheet_range
cargo run --example cad_viewport --features spatial
echo '{"source":1,"filters":[{"Eq":{"field":"material","value":"Steel"}}]}' | cargo run -q --example json_cli --features json > /dev/null
cargo run -q --features cli --bin tpt-query -- --entities tests/fixtures/scene.json --dsl 'from 1 where material = "Steel" select name' >/dev/null

echo "==> no_std build with spatial"
cargo build --no-default-features --features spatial

echo "==> bench (compile only)"
cargo bench --no-run

echo "==> no_std build (thumbv7em-none-eabihf)"
cargo build --no-default-features --target thumbv7em-none-eabihf

echo "==> wasm test (wasm32-unknown-unknown, under node)"
cargo test --target wasm32-unknown-unknown --no-default-features --test wasm

echo "All checks passed."
