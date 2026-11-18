# Offline package pipeline

This replaces the earlier federated-worker/spool design. The conversation's
simplifications and selection of `embeddinggemma2-rust` supersede that design.

## Responsibilities

- `omnipotent-format`: existing shared article/resource and package structures.
- `omnipotent-wikipedia`: synchronous Parquet → typed source records → `Article`
  iterator. No model or storage code; no `serde_json::Value` conversion.
- `omnipotent-embed`: pillar dispatch, `chunkers/article.rs`, token-aware splitting,
  batch planning, and the pinned `embeddinggemma2-rust` backend.
- `omnipotent-pack`: thin CLI and package assembly operations. redb stores articles,
  located passages, temporary token rows and full raw vectors.

Only article chunking is implemented. Publication extraction will use xberg;
publication, map, media and data chunkers remain TODOs. Article chunking visits
abstracts and paragraph nodes, including paragraphs nested in lists/infobox fields.
Tables, references and media remain preserved but are not separately embedded:
the current search-location schema does not address table cells or reference text.

## Preparation and inference

1. Convert every article in the worker's selected shards.
2. Chunk with title/heading context. Final model inputs contain at most 384 tokens,
   including special tokens. Preserve UTF-8 byte locations; no overlap.
3. Commit article content, search entries and little-endian u32 token rows to redb.
4. Finish preparation for the entire worker dataset.
5. Load only `(token count, passage ID)` descriptors and globally sort them.
6. Greedily batch adjacent lengths, with both `count <= --batch-size` (default 128)
   and `count * longest_input <= 2048`.
7. Read tokens on demand and embed with the upstream library's isolated-sequence
   packed forward. No tokenisation during inference. Commit raw 768d vectors and
   remove their temporary tokens in the same transaction.

No hashing, verification subsystem, separate spool format, coordinator, buckets,
windowed sorting or handwritten model implementation. Original safetensors are
loaded into FP32. CPU and CUDA use the same upstream architecture. Linux/Windows
CUDA libraries are external and loaded dynamically. CPU mode never opens CUDA;
auto can fall back during device initialisation, not after an inference error.
Metal is not supported. macOS releases are CPU-only.

Interrupted embedding can continue with `embed`; committed vectors are retained.
Interrupted source preparation or merge must restart into a new output. Outputs
are never overwritten implicitly. Model files and source caches are not deleted.

## Assembly

Merge copies completed worker databases without a model or re-embedding. It
remaps package-local resource, payload and passage IDs, retains logical identities,
and copies vectors in bounded transactions. Duplicate logical resource identities
and different embedding profiles are errors. The operator assigns disjoint inputs;
there is no deduplication policy or general asset installation pipeline.

Finalisation is independent of sources, token rows and model files. It trains one
search index from a bounded global sample, inserts all vectors, compacts the file,
and marks it ready. The index uses normalised 256d Matryoshka prefixes; raw 768d
vectors remain available. Non-finalised worker files are not serving-ready.

```sh
omnipotent-pack wikipedia --shards 0..10 --output a.redb --no-finalize
omnipotent-pack wikipedia --shards 10..20 --output b.redb --no-finalize
omnipotent-pack merge --input a.redb --input b.redb --output all.redb --no-finalize
omnipotent-pack finalize --output all.redb
```

`--input PATH` accepts local Parquet shards instead of downloading. `--limit N`
enables a bounded real run. `--model-dir` uses original local model assets;
otherwise pinned public assets are downloaded under `--cache`/`OMNIPOTENT_CACHE`.

## Releases

release-plz owns versions, changelogs, git tags and GitHub releases, not crates.io
publication. The manually authorised Release workflow opens release PRs or releases.
Packer binaries builds native Linux x86-64, Windows x86-64 and macOS ARM64 assets
on hosted runners. A published release triggers asset builds when the release token
permits workflow chaining; otherwise manually dispatch Packer binaries with its tag.
No separate release orchestrator, installers, signing or checksums.

Actual remote releases require a Git repository/remote and GitHub permissions.
This checkout currently has no Git metadata. A configured matrix is not evidence
that Windows/macOS binaries have run; report local and remote evidence separately.
