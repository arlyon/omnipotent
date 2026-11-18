"""Offline structural checks for the prepared, self-contained GLB assets."""

import hashlib
import json
import math
import struct
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets/models/omnipotent"


def load_glb(path):
    data = path.read_bytes()
    magic, version, length = struct.unpack_from("<4sII", data)
    assert (magic, version, length) == (b"glTF", 2, len(data))
    cursor = 12
    chunks = {}
    while cursor < len(data):
        size, kind = struct.unpack_from("<I4s", data, cursor)
        cursor += 8
        assert size % 4 == 0
        assert cursor + size <= len(data)
        assert kind not in chunks
        chunks[kind] = data[cursor : cursor + size]
        cursor += size
    assert cursor == len(data)
    return json.loads(chunks[b"JSON"]), chunks[b"BIN\x00"]


def accessor(document, binary, index):
    spec = document["accessors"][index]
    assert "sparse" not in spec
    view = document["bufferViews"][spec["bufferView"]]
    formats = {5121: "B", 5123: "H", 5125: "I", 5126: "f"}
    widths = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}
    fmt = "<" + formats[spec["componentType"]] * widths[spec["type"]]
    size = struct.calcsize(fmt)
    stride = view.get("byteStride", size)
    offset = view.get("byteOffset", 0) + spec.get("byteOffset", 0)
    end = offset + (spec["count"] - 1) * stride + size
    assert stride >= size and end <= view.get("byteOffset", 0) + view["byteLength"]
    return [struct.unpack_from(fmt, binary, offset + i * stride) for i in range(spec["count"])]


class ModelAssets(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest = json.loads((ASSETS / "manifest.json").read_text())
        cls.variants = [(v, *load_glb(ASSETS / v["file"])) for v in cls.manifest["variants"]]

    def test_self_contained_buffers_and_images(self):
        for variant, doc, binary in self.variants:
            with self.subTest(file=variant["file"]):
                self.assertEqual((ASSETS / variant["file"]).stat().st_size, variant["bytes"])
                self.assertEqual(len(doc["buffers"]), 1)
                self.assertNotIn("uri", doc["buffers"][0])
                self.assertLessEqual(doc["buffers"][0]["byteLength"], len(binary))
                for view in doc["bufferViews"]:
                    self.assertEqual(view["buffer"], 0)
                    self.assertLessEqual(view.get("byteOffset", 0) + view["byteLength"], len(binary))
                self.assertEqual(len(doc["images"]), 3)
                for image in doc["images"]:
                    self.assertNotIn("uri", image)
                    view = doc["bufferViews"][image["bufferView"]]
                    start = view.get("byteOffset", 0)
                    payload = binary[start : start + view["byteLength"]]
                    self.assertEqual(payload[:8], b"\x89PNG\r\n\x1a\n")
                    self.assertEqual(struct.unpack_from(">II", payload, 16), (variant["texture_size"],) * 2)

    def test_hierarchy_and_independent_sled(self):
        for variant, doc, _ in self.variants:
            with self.subTest(file=variant["file"]):
                names = {n["name"]: i for i, n in enumerate(doc["nodes"])}
                root = doc["nodes"][names["omnipotent"]]
                self.assertEqual(set(root["children"]), {
                    names["enclosure_with_antenna"], names["epaper_surface"],
                    names["sled_assembly"], names["unidentified_fragments"],
                })
                self.assertEqual(len(doc["nodes"][names["unidentified_fragments"]]["children"]), 4)
                self.assertEqual(len(doc["meshes"]), 7)
                self.assertIn("cleaned", root["extras"]["display_status"])
                offset = root["extras"]["sled_seat_offset"]
                self.assertEqual(len(offset), 3)
                self.assertTrue(all(math.isfinite(x) for x in offset))
                self.assertGreater(offset[1], 0)
                self.assertLess(offset[2], 0)
                self.assertNotIn("animations", doc)
                self.assertNotIn("cameras", doc)

    def test_pbr_maps_are_connected(self):
        for _, doc, _ in self.variants:
            self.assertEqual(len(doc["materials"]), 2)
            mat = doc["materials"][0]
            self.assertIn("normalTexture", mat)
            pbr = mat["pbrMetallicRoughness"]
            self.assertIn("baseColorTexture", pbr)
            self.assertIn("metallicRoughnessTexture", pbr)
            self.assertEqual(mat.get("alphaMode", "OPAQUE"), "OPAQUE")

    def test_screen_is_flat_matte_and_has_no_bump_map(self):
        self.assertEqual(self.manifest["screen"]["resolution"], [296, 160])
        self.assertGreater(self.manifest["screen"]["cleaned_atlas_pixels"], 80000)
        for _, doc, binary in self.variants:
            node = next(node for node in doc["nodes"] if node["name"] == "epaper_surface")
            self.assertEqual(node["extras"]["resolution"], [296, 160])
            mesh = doc["meshes"][node["mesh"]]
            for primitive in mesh["primitives"]:
                mat = doc["materials"][primitive["material"]]
                self.assertNotIn("normalTexture", mat)
                self.assertNotIn("occlusionTexture", mat)
                self.assertNotIn("baseColorTexture", mat["pbrMetallicRoughness"])
                self.assertNotIn("metallicRoughnessTexture", mat["pbrMetallicRoughness"])
                self.assertEqual(mat["pbrMetallicRoughness"]["metallicFactor"], 0)
                self.assertAlmostEqual(mat["pbrMetallicRoughness"]["roughnessFactor"], 0.95, places=5)
                attrs = primitive["attributes"]
                normals = accessor(doc, binary, attrs["NORMAL"])
                positions = accessor(doc, binary, attrs["POSITION"])
                # Blender packs custom normals before export. Check geometric
                # planarity against the analytic plane independently of that
                # quantization, then check the exported normals are uniform.
                normal = (0, 0.795936388, 0.605380237)
                offsets = [sum(p[i] * normal[i] for i in range(3)) for p in positions]
                self.assertLess(max(offsets) - min(offsets), 1e-5)
                self.assertTrue(all(max(abs(a - b) for a, b in zip(n, normal, strict=True)) < 1e-4 for n in normals))
                self.assertEqual(len(set(normals)), 1)
                width = max(p[0] for p in positions) - min(p[0] for p in positions)
                vertical = [p[1] * 0.605380237 - p[2] * 0.795936388 for p in positions]
                self.assertAlmostEqual(width / (max(vertical) - min(vertical)), 296 / 160, places=4)

    def test_geometry_is_finite_and_indexed_correctly(self):
        expected = sum(p["triangles"] for p in self.manifest["parts"])
        for variant, doc, binary in self.variants:
            count = 0
            with self.subTest(file=variant["file"]):
                for mesh in doc["meshes"]:
                    for primitive in mesh["primitives"]:
                        self.assertEqual(primitive.get("mode", 4), 4)
                        attrs = primitive["attributes"]
                        positions = accessor(doc, binary, attrs["POSITION"])
                        self.assertTrue(all(math.isfinite(x) for point in positions for x in point))
                        required = ["NORMAL", "TEXCOORD_0"]
                        if mesh["name"] != "epaper_flat_surface":
                            required.append("TANGENT")
                        for name in required:
                            values = accessor(doc, binary, attrs[name])
                            self.assertEqual(len(values), len(positions))
                            self.assertTrue(all(math.isfinite(x) for item in values for x in item))
                        indices = accessor(doc, binary, primitive["indices"])
                        self.assertEqual(len(indices) % 3, 0)
                        self.assertTrue(all(0 <= i[0] < len(positions) for i in indices))
                        count += len(indices) // 3
                self.assertEqual(count, expected)
                self.assertEqual(count, 25824)

    def test_variants_preserve_geometry(self):
        signatures = []
        for _, doc, binary in self.variants:
            parts = []
            for mesh in doc["meshes"]:
                for primitive in mesh["primitives"]:
                    attrs = primitive["attributes"]
                    parts.append((mesh["name"], accessor(doc, binary, attrs["POSITION"]),
                                  accessor(doc, binary, primitive["indices"])))
            signatures.append(hashlib.sha256(repr(parts).encode()).hexdigest())
        self.assertEqual(len(set(signatures)), 1)

    def test_poster_is_rgba(self):
        header = (ASSETS / "poster.png").read_bytes()[:33]
        self.assertEqual(header[:8], b"\x89PNG\r\n\x1a\n")
        self.assertEqual(struct.unpack_from(">II", header, 16), (1000, 1200))
        self.assertEqual(header[25], 6)  # PNG truecolour with alpha


if __name__ == "__main__":
    unittest.main()
