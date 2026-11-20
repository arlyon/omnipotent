#!/usr/bin/env bash
# No tool installation, browser download, or generated model rebuild.
# Cargo uses the committed lockfile; the first run may fetch Rust dependencies.
set -euo pipefail
cd "$(dirname "$0")/.."
python -B -m unittest discover -s tests -v
python -B scripts/prepare-sample-book.py --check
node --test tests/site-state.test.mjs
for file in js/*.mjs tests/site.browser.cjs; do
  node --check "$file"
done
cargo fmt --all -- --check
rustfmt --edition 2024 --check firmware/esp32c6/src/main.rs
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo check --locked -p omnipotent-format --lib
cargo check --locked -p omnipotent-serve --lib
