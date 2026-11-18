"""Model-specific screen repair, shared by the reproducible Blender preparer.

Coordinates are measured against the hash-checked OBJ, not physical dimensions.
The source atlas is repaired even beneath the independent display surface.
"""
import importlib
import math

import numpy as np

bpy = importlib.import_module("bpy")
Vector = importlib.import_module("mathutils").Vector

UP = np.array([0.0, 0.605380237, -0.795936388])
NORMAL = np.array([0.0, 0.795936388, 0.605380237])
LEFT, RIGHT = -0.307, 0.450
HEIGHT = (RIGHT - LEFT) * 160 / 296
TOP = 0.570
BOTTOM = TOP - HEIGHT
DEPTH = 0.15420
PAPER = (245 / 255, 243 / 255, 233 / 255, 1.0)


def clip(poly, normal, boundary):
    """Clip 3D positions plus interpolated UVs to a half-space."""
    result = []
    for start, end in zip(poly, poly[1:] + poly[:1], strict=True):
        a = float(np.dot(start[:3], normal) - boundary)
        b = float(np.dot(end[:3], normal) - boundary)
        if a <= 0:
            result.append(start)
        if (a <= 0) != (b <= 0):
            result.append(start + (end - start) * (a / (a - b)))
    return result


def screen_mask(obj_bytes, size):
    vertices, uvs, faces = [], [], []
    current = None
    for line in obj_bytes.splitlines():
        fields = line.split()
        if not fields:
            continue
        if fields[0] == b"v":
            vertices.append(tuple(map(float, fields[1:4])))
        elif fields[0] == b"vt":
            uvs.append(tuple(map(float, fields[1:3])))
        elif fields[0] == b"o":
            current = fields[1]
        elif fields[0] == b"f" and current == b"mesh_5_1":
            faces.append([tuple(int(v) - 1 for v in token.split(b"/")[:2]) for token in fields[1:]])
    vertices, uvs = np.asarray(vertices), np.asarray(uvs)
    mask = np.zeros((size, size), dtype=bool)
    planes = [(np.array([1, 0, 0]), RIGHT + 0.006),
              (np.array([-1, 0, 0]), -LEFT + 0.006),
              (UP, TOP + 0.006), (-UP, -BOTTOM + 0.006),
              (NORMAL, DEPTH + 0.016), (-NORMAL, -DEPTH + 0.016)]
    for face in faces:
        points = vertices[[v for v, _ in face]]
        normal = np.cross(points[1] - points[0], points[2] - points[0])
        length = np.linalg.norm(normal)
        if length == 0 or abs(np.dot(normal / length, NORMAL)) < 0.9:
            continue
        poly = [np.concatenate([vertices[v], uvs[t]]) for v, t in face]
        for direction, boundary in planes:
            poly = clip(poly, direction, boundary)
            if len(poly) < 3:
                break
        if len(poly) < 3:
            continue
        for i in range(1, len(poly) - 1):
            triangle = np.asarray([poly[0][3:], poly[i][3:], poly[i + 1][3:]]) * size
            low = np.maximum(np.floor(triangle.min(0)).astype(int), 0)
            high = np.minimum(np.ceil(triangle.max(0)).astype(int), size - 1)
            if np.any(high < low):
                continue
            yy, xx = np.mgrid[low[1]:high[1] + 1, low[0]:high[0] + 1]
            a, b, c = triangle
            denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
            if abs(denominator) < 1e-8:
                continue
            u = ((b[1] - c[1]) * (xx + 0.5 - c[0]) + (c[0] - b[0]) * (yy + 0.5 - c[1])) / denominator
            v = ((c[1] - a[1]) * (xx + 0.5 - c[0]) + (a[0] - c[0]) * (yy + 0.5 - c[1])) / denominator
            mask[low[1]:high[1] + 1, low[0]:high[0] + 1] |= (u >= -0.001) & (v >= -0.001) & (u + v <= 1.001)
    if mask.sum() < size * size * 0.001:
        raise ValueError("Screen atlas mask unexpectedly small")
    # One-texel gutter prevents bilinear sampling of the previous artwork.
    padded = np.pad(mask, 1)
    mask = np.logical_or.reduce([padded[y:y + size, x:x + size] for y in range(3) for x in range(3)])
    return mask


def clean_atlas(texture_dir, obj_bytes):
    size = 4096
    mask = screen_mask(obj_bytes, size)
    for filename, replacement, space in (
        ("model_baseColor.png", PAPER, "sRGB"),
        ("model_normal.png", (0.5, 0.5, 1, 1), "Non-Color"),
        ("model_metallicRoughness.png", (1, 0.95, 0, 1), "Non-Color"),
    ):
        path = texture_dir / filename
        image = bpy.data.images.load(str(path), check_existing=False)
        image.colorspace_settings.name = space
        if tuple(image.size) != (size, size):
            raise ValueError("Unexpected source atlas dimensions")
        pixels = np.empty(size * size * 4, dtype=np.float32)
        image.pixels.foreach_get(pixels)
        pixels.reshape(size, size, 4)[mask] = replacement
        image.pixels.foreach_set(pixels)
        image.filepath_raw = str(path)
        image.file_format = "PNG"
        image.save()
        bpy.data.images.remove(image)
    return int(mask.sum())


def add_screen(center, scale):
    """Flat, rounded 296:160 surface inside the existing bezel, with clean UVs."""
    width = RIGHT - LEFT
    radius = 0.024
    mid_x, mid_y = (LEFT + RIGHT) / 2, (TOP + BOTTOM) / 2
    perimeter = []
    for cx, cy, start in ((RIGHT - radius, TOP - radius, 0),
                          (LEFT + radius, TOP - radius, 90),
                          (LEFT + radius, BOTTOM + radius, 180),
                          (RIGHT - radius, BOTTOM + radius, 270)):
        for i in range(7):
            angle = math.radians(start + i * 90 / 6)
            perimeter.append((cx + math.cos(angle) * radius, cy + math.sin(angle) * radius))
    local = [(mid_x, mid_y), *perimeter]
    points = []
    for x, y in local:
        raw = np.array([x, 0, 0]) + UP * y + NORMAL * (DEPTH + 0.004)
        world = Vector((float(raw[0]), float(-raw[2]), float(raw[1])))
        points.append((world - center) * scale)
    faces = [(0, i + 1, (i + 1) % len(perimeter) + 1) for i in range(len(perimeter))]
    mesh = bpy.data.meshes.new("epaper_flat_surface")
    mesh.from_pydata(points, [], faces)
    mesh.update()
    # Pin the analytic plane normal. Tiny rounded fan triangles otherwise
    # amplify float32 rounding into slightly inconsistent exported normals.
    flat_normal = Vector((float(NORMAL[0]), float(-NORMAL[2]), float(NORMAL[1]))).normalized()
    mesh.normals_split_custom_set_from_vertices([flat_normal] * len(points))
    uv = mesh.uv_layers.new(name="DisplayUV")
    for poly in mesh.polygons:
        for loop_index in poly.loop_indices:
            x, y = local[mesh.loops[loop_index].vertex_index]
            uv.data[loop_index].uv = ((x - LEFT) / width, (y - BOTTOM) / HEIGHT)
    obj = bpy.data.objects.new("epaper_surface", mesh)
    bpy.context.scene.collection.objects.link(obj)
    material = bpy.data.materials.new("epaper_matte")
    material.use_nodes = True
    shader = material.node_tree.nodes.get("Principled BSDF")
    shader.inputs["Base Color"].default_value = tuple(((v + 0.055) / 1.055) ** 2.4 for v in PAPER[:3]) + (1,)
    shader.inputs["Roughness"].default_value = 0.95
    shader.inputs["Metallic"].default_value = 0
    # No image, bump, or normal node exists on the independently textured screen.
    obj.data.materials.append(material)
    obj["resolution"] = [296, 160]
    obj["surface"] = "flat matte paper; no normal/bump map"
    obj["source"] = "fitted to existing concept bezel, not physical CAD"
    return obj
