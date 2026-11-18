# Prepared Omnipotent model

Browser-oriented derivatives of `~/Downloads/model-obj.zip`, the lower-face-count split export. Original downloads are untouched. This is concept geometry, not engineering CAD.

## Files

| File | Purpose |
| --- | --- |
| `omnipotent-1k.glb` | Browser version, 1024 px textures, approximately 5.1 MiB |
| `omnipotent-2k.glb` | Higher-resolution version, 2048 px textures, approximately 12.2 MiB |
| `omnipotent.blend` | Editable assembly with packed 2K textures |
| `poster.png` | Transparent, front-facing open/blank fallback render |
| `manifest.json` | Source hashes, geometry counts, screen repair, and export details |

Both GLBs have identical geometry and three embedded shell maps. The independent screen material is untextured matte paper until the browser supplies a canvas. No external texture URL, script, animation, camera, light, compression decoder, or network dependency is embedded in the GLBs.

## Scene contract

```text
omnipotent
├── enclosure_with_antenna
├── sled_assembly
├── epaper_surface
└── unidentified_fragments
    ├── unidentified_fragment_1
    ├── unidentified_fragment_2
    ├── unidentified_fragment_3
    └── unidentified_fragment_4
```

The original assembly is centered with its longest original axis normalized to 2 presentation units. GLB uses glTF Y-up coordinates. These are not physical millimetres or metres. The source's open pose is preserved in the file, and the mesh origins are centered on their own bounds.

Root metadata includes `sled_seat_offset`, a three-component glTF-space translation for seating the sled. The browser moves the sled and nearby retained fragments together, leaving the enclosure stationary. This is a visually fitted presentation offset, not a mechanically verified travel distance or collision-free CAD assembly.

The antenna remains fused to the enclosure. No independent antenna animation is claimed.

## Screen cleanup

The original display was baked into a fragmented atlas and the shell's surface maps. `scripts/model_screen.py` repairs the measured screen region before texture downsampling:

- Base colour is replaced with uniform paper-white.
- Tangent-space normal is replaced with a neutral normal.
- Surface data is replaced with uniform high roughness and zero metallic.
- A one-texel gutter avoids sampling old artwork at atlas edges.

The repaired 4096 px atlas mask covers 86,344 pixels. The original source files are not modified; repair operates in a temporary copy during preparation.

A new rounded, planar `epaper_surface` fits the existing bezel. Its aspect ratio is **296:160** with UVs dedicated to the live UI. It has a matte material with no texture, normal, bump, or occlusion map. Analytic flat normals prevent shading variation from small fan triangles. The browser replaces only this material with its shared four-colour canvas renderer.

The bezel and remaining shell maps are retained. The source still has uneven generated geometry and baked lighting outside the repaired screen. The display fitting is approximate concept work, not a specification for a manufactured panel.

## Geometry

No decimation is applied. The source has 25,798 triangular faces; Blender imports 25,796, omitting two enclosure faces from a source containing zero-area triangles. The new screen adds 28 triangles, for **25,824 triangles across seven meshes**. Both texture variants retain that same geometry. The model is not validated for fabrication, watertightness, physical dimensions, or electrical safety.

## Rebuild

With Blender installed, from the repository root:

```bash
blender --background --threads 4 --python scripts/prepare-model.py -- \
  --source ~/Downloads/model-obj.zip \
  --output-dir assets/models/omnipotent-rebuilt
```

The script validates the exact source OBJ hash, extracts only the five expected data files into a temporary directory, repairs the screen, and refuses to overwrite a nonempty output directory. Use a new output directory. Blender 5.2.2 LTS and its bundled Python/NumPy were used; no pip package or additional executable was installed.

The root showcase also uses `assets/images/model-poster.webp` for the front-facing open state and `model-ready.webp` for a seated, readable reduced-motion fallback. Those are rendered derivatives, not additional model versions.

## Validation and limits

Run `bash scripts/check.sh`. The model tests check buffer bounds, embedded maps, geometry/UVs/normals, material connections, exact screen aspect, analytic planarity, absence of screen normal/bump inputs, metadata, hierarchy, and texture-variant geometry equality. Browser checks cover the live display and assembly; see the root README.

Additional Blender inspection hid the added screen and rendered the underlying atlas. Its old lettering was gone. Numerical checks of the cleaned interior confirmed uniform colour, neutral normal, and matte/nonmetallic data within PNG precision. Front-facing and separated views were inspected, and the exported GLB was loaded in the browser.

The local exporter reports an unavailable optional MeshOptimizer bridge; compression is not requested or used. It also emits shared-image sampler and future Blender API notices. No renderer sandbox or permission control was weakened. A formal Khronos validator run, physical panel colour calibration, manufacturing fit, and publication rights remain unverified.
