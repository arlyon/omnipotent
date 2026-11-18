# Record-envelope fixture

`payload.json` is synthetic envelope-only test data, not a valid Article or a Wikipedia extract. `payload.json.zst` encodes its exact bytes using Python zstandard 0.25.0, level 3, a maximum window log of 17, content size, and a checksum. Check the encoded bytes using `zstd -t` and compare `zstd -dc payload.json.zst` with `payload.json`.

The runtime crate only checks framing and Zstd magic. A consumer must independently verify decompression, checksums, actual decoded size, and the Article model. The fixture is committed so unit tests need no network, compressor, or external executable.
