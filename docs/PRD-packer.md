# Offline package pipeline

This replaces the earlier federated-worker/spool design. The conversation's
simplifications and selection of `embeddinggemma2-rust` supersede that design.

## Responsibilities

- `omnipotent-format`: existing shared article/resource and package structures.
- `omnipotent-wikipedia`: synchronous Parquet → typed source records → `Article`
  iterator. No model or storage code; no `serde_json::Value` conversion.
- `omnipotent-embed`: pillar dispatch, `chunkers/article.rs`, token-aware splitting,
  batch planning, and the pinned `embeddinggemma2-rust` backend.
- `omnipotent-pack`: thin CLI and package assembly operations. Fjall worker directories
  store article records, passage locations, temporary tokens, and model vectors.

## Stored article and passage layout

`record.rs` defines the stored article: a little-endian header, fixed 28-byte
nodes in preorder, and shared string and auxiliary data. The worker keys records
by package-local article ID and writes those bytes unchanged into the bundle.
`Bundle::article(id)?.nodes()` and node text borrow directly from the bundle bytes.

Each passage reference is exactly 16 bytes: article ID, node ID (zero for abstract),
start and end byte offsets. Passage text and headings are not stored again.
`bundle.rs` defines the flat v1 `.bin` layout: a 16-byte header, cluster boundaries,
centroids, codebooks, codes, vectors, passage references, article boundaries, and
records. The row number connects code, vector and passage. Linux serves article
records as `application/vnd.omnipotent.article+binary` and serves native files
from mounted directories. This does not implement the device's flash backend.

Only article chunking is implemented. Publication extraction will use xberg;
publication, map, media and data chunkers remain TODOs. Article chunking visits
abstracts and paragraph nodes, including paragraphs nested in lists/infobox fields.
Tables, references and media remain preserved but are not separately embedded:
the current search-location schema does not address table cells or reference text.

## Preparation and inference

1. Convert every article in the worker's selected shards.
2. Chunk with title/heading context. Final model inputs contain at most 384 tokens,
   including special tokens. Preserve UTF-8 byte locations; no overlap.
3. Commit the article record, passage locations and little-endian u32 token rows to Fjall.
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

Merge copies completed, unfinalised worker databases without a model or re-embedding. It
remaps package-local article and passage IDs, retains logical identities,
and copies vectors in bounded transactions. Duplicate logical resource identities
and different embedding profiles are errors. The operator assigns disjoint inputs;
there is no deduplication policy or general asset installation pipeline.

Finalisation is independent of sources, token rows and model files. `reductive`
trains IVF centroids and PQ codebooks from a bounded sample of 256d Matryoshka
prefixes, L2-normalised **after** truncation. The packer writes one 256d FP32
vector plus one PQ code per passage to a new `.bin`. The output is published
only after it is fully written, never overwrites another file, and contains no
worker data. A complete worker remains available for another merge or export.

```sh
omnipotent-pack wikipedia --shards 0..10 --output a.worker
omnipotent-pack wikipedia --shards 10..20 --output b.worker
omnipotent-pack merge --input a.worker --input b.worker --output all.worker
omnipotent-pack finalize --input all.worker --output all.bin
```

`--input PATH` accepts local Parquet shards instead of downloading. `--limit N`
enables a bounded real run. `--model-dir` uses original local model assets;
otherwise pinned public assets are downloaded under `--cache`/`OMNIPOTENT_CACHE`.

## Releases

Release-plz owns versions, changelogs, tags and GitHub releases, not crates.io
publication. A push to `main` opens or updates a release PR when needed. Merging
that PR is the approval step: the resulting push publishes the release and calls
Packer binaries in the same workflow to attach Linux x86-64, Windows x86-64 and
macOS ARM64 executables. This uses GitHub's workflow token, not a PAT or a release
event to start the builds.

For recovery, either workflow can be dispatched manually. Packer binaries accepts
the already-published packer tag when only the binary build needs retrying.
