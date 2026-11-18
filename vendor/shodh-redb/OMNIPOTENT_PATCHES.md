# Omnipotent storage patches

## Provenance

Vendored from [varun29ankuS/shodh-redb](https://github.com/varun29ankuS/shodh-redb) at commit `24f0c558d2e53b427c47b99832a252251aa91000` (upstream package version 0.6.0, Apache-2.0). The upstream LICENSE and authorship are retained. Local publishing is disabled.

This is a curated source checkout: core, tests, examples, benchmarks, docs, and the derive test dependency are retained. The unrelated benchmark application, CLI, Python package, fuzz workspace, and repository automation/configuration are not included. The nested workspace is narrowed accordingly. It requires Rust 1.89; Omnipotent's existing format crate retains its own Rust 1.85 contract.

## Functional changes

### Lazy, transaction-bound BlobReader

- `BlobReader<'txn>` borrows a read or write transaction instead of owning the entire blob body. Opening reads metadata only.
- `read_into(&mut [u8])` and `seek_to(u64)` work without std. Reads copy directly from guarded database chunks into caller memory, without an output-sized intermediate Vec.
- Physical chunk positions are computed explicitly. Missing, truncated, overflowing, or unaddressable chunk ranges fail rather than silently shifting later bytes.
- std Read/Seek remain available. `read_range` remains an allocating convenience for callers that deliberately request an owned range.
- Cursor advancement occurs only after a successful read; the caller buffer may have been partially modified on an I/O error. The engine poisons a DB after I/O failure, so retrying through that same handle is not recovery; reopening is required.
- Read transactions pin the source snapshot. Modifying/deleting the same blob while reading through a write transaction is unsupported.
- Whole-blob checksum verification is not performed by range/stream reads. This preserves the existing range-read integrity semantics, not the verified `get_blob` semantics. Database caches/pages still allocate; this is not kernel zero-copy or proof of total peak heap on hardware.

The upstream BlobWriter already maintains streaming xxh3/SHA state; it is unchanged.

### no_std incremental core

- Logical export/import, IncrementalSnapshot, IncrementalImportReport, and in-memory snapshot encode/decode now work with no_std + alloc.
- File backup/apply and elapsed-time reports remain std-only.
- The read-transaction table-tree accessor used by root-checksum comparison no longer requires std.
- Decoding rejects impossible table/entry counts before count-driven allocation, checks conversion into usize, and rejects nonzero reserved padding, empty table names, and trailing payload bytes. Truncation diagnostics retain the upstream error wording contract. This does not impose an application heap budget on a large otherwise valid snapshot.

The serialized delta version remains upstream v1. No application release/manifest format is introduced by these patches.

### Portable read-only backend opening

- `Builder::open_read_only_with_backend` exposes the existing read-only open path without std/filesystem APIs. The path-based opener delegates to it and retains platform file locking.
- The internal read-only backend is available without std and rejects writes/resizes. Custom-backend callers must exclude external writers and keep the storage immutable; this API cannot acquire a platform lock for them.
- Conformance tests in both feature modes cover clean table/blob reads, unchanged backend length, no write/resize calls, and refusal of empty/corrupt/failing backends.

### RISC-V dispatch seam

- RISC-V distance operations explicitly dispatch to aliases of the existing scalar kernels. This is not SIMD or ESP32 acceleration.
- A TODO records the need for verified ESP32-S31 kernels and feature detection; a host unit test exercises the scalar hook. No RISC-V hardware or cross-target performance claim is made.

## Deliberate limitations

The current incremental implementation is **not yet Omnipotent synchronization**:

- Export still materializes old/new contents of changed tables; decode/import use owned changes.
- Import targets normal byte-key/byte-value tables. Existing typed tables are not handled; regression tests verify failure rolls back prior changes atomically.
- Coverage of internal blob/vector system tables is not established. Do not use this API as a complete database replication mechanism.
- Base/target release lineage, wrong-base rejection, replay policy, resumable transport, authenticity, and bounded streaming application remain future work.
- No firmware, device SD/flash backend, ESP32 heap measurement, or vector-search latency/recall claim has been validated.

## Tests and checks

Project conformance tests live in `crates/omnipotent-storage-tests/`, not in the dependency's production API. They run with std and with the dependency's std feature disabled. They include cold backend read accounting for a 4 MiB blob, cross-chunk reads, EOF/seeking, dedup, old snapshots after deletion, injected I/O failure, byte-table delta roundtrips, corruption/truncation, impossible counts, trailing data, and typed-import rollback.

```bash
bash scripts/check.sh
cargo test --locked -p omnipotent-storage-tests --no-default-features
cargo check --locked --manifest-path vendor/shodh-redb/Cargo.toml -p shodh-redb --no-default-features --lib
```

The canonical check also runs private reader-helper regressions and the existing upstream incremental unit tests. Existing upstream blob and no_std-surface integration suites were run separately during development.

Workspace conformance code is subject to strict Clippy in both feature modes. Directly linting the entire upstream dependency with Rust 1.99 exposes pre-existing Clippy-deny failures outside these patches and an unused AssignmentValue warning. These have not been suppressed or swept into an unrelated cleanup. They remain an upstream maintenance issue; root conformance linting is not a claim that the whole vendored codebase is lint-clean.

Modified existing source files were rustfmt-formatted with the installed toolchain; formatting-only differences may accompany the functional changes above.
