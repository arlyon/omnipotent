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
cargo clippy --locked -p omnipotent-storage-tests --no-default-features --all-targets -- -D warnings
cargo test --locked -p omnipotent-storage-tests --no-default-features
storage_manifest=vendor/shodh-redb/Cargo.toml
rustfmt --edition 2024 --config skip_children=true --check \
  vendor/shodh-redb/src/blob_store/reader.rs vendor/shodh-redb/src/incremental.rs \
  vendor/shodh-redb/src/transactions.rs vendor/shodh-redb/src/db.rs vendor/shodh-redb/src/lib.rs \
  vendor/shodh-redb/src/vector_ops.rs vendor/shodh-redb/src/tree_store/mod.rs \
  vendor/shodh-redb/src/tree_store/page_store/mod.rs vendor/shodh-redb/src/tree_store/page_store/backends.rs
cargo check --locked --manifest-path "$storage_manifest" -p shodh-redb --no-default-features --lib
cargo test --locked --manifest-path "$storage_manifest" -p shodh-redb --lib blob_store::reader::tests
cargo test --locked --manifest-path "$storage_manifest" -p shodh-redb --lib incremental::tests
cargo test --locked --manifest-path "$storage_manifest" -p shodh-redb --lib vector_ops::riscv_scalar_dispatch
