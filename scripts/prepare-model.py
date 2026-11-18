"""Prepare the low-poly split OBJ as self-contained browser assets.

Run with Blender, not system Python:
  blender --background --threads 4 --python scripts/prepare-model.py -- \
    --source ~/Downloads/model-obj.zip --output-dir assets/models/omnipotent

Requires only Blender's bundled modules. Never executes archive content.
"""

import argparse
import hashlib
import importlib
import json
import sys
import tempfile
import zipfile
from pathlib import Path

# Blender supplies these modules in its own interpreter. Resolve them at
# runtime so ordinary Python tooling can inspect the preparation script.
bpy = importlib.import_module("bpy")
Vector = importlib.import_module("mathutils").Vector


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])


def bounds(objects):
    points = [o.matrix_world @ Vector(v) for o in objects for v in o.bound_box]
    return (
        Vector(tuple(min(p[i] for p in points) for i in range(3))),
        Vector(tuple(max(p[i] for p in points) for i in range(3))),
    )


def parent_preserving_transform(obj, parent):
    world = obj.matrix_world.copy()
    obj.parent = parent
    obj.matrix_world = world


def empty(name, location=(0, 0, 0)):
    obj = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(obj)
    obj.location = location
    return obj


def material(texture_dir, size):
    mat = bpy.data.materials.new(f"shell_atlas_{size}")
    mat.use_nodes = True
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    shader = nodes.get("Principled BSDF")
    # Preserve the authored metallic/roughness map, not an invented gloss value.
    shader.inputs["Metallic"].default_value = 1
    shader.inputs["Roughness"].default_value = 1
    images = {}
    for role, filename, color_space in (
        ("base", "model_baseColor.png", "sRGB"),
        ("normal", "model_normal.png", "Non-Color"),
        ("surface", "model_metallicRoughness.png", "Non-Color"),
    ):
        image = bpy.data.images.load(str(texture_dir / filename), check_existing=False)
        image.name = f"{role}_{size}"
        image.colorspace_settings.name = color_space
        image.scale(size, size)
        image.pack()
        tex = nodes.new("ShaderNodeTexImage")
        tex.image = image
        tex.label = role
        images[role] = tex
    links.new(images["base"].outputs["Color"], shader.inputs["Base Color"])
    normal = nodes.new("ShaderNodeNormalMap")
    links.new(images["normal"].outputs["Color"], normal.inputs["Color"])
    links.new(normal.outputs["Normal"], shader.inputs["Normal"])
    channels = nodes.new("ShaderNodeSeparateColor")
    channels.mode = "RGB"
    links.new(images["surface"].outputs["Color"], channels.inputs["Color"])
    # glTF metallic-roughness packing: green = roughness, blue = metallic.
    links.new(channels.outputs["Green"], shader.inputs["Roughness"])
    links.new(channels.outputs["Blue"], shader.inputs["Metallic"])
    return mat


def export(path, objects):
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_texcoords=True,
        export_normals=True,
        export_tangents=True,
        export_materials="EXPORT",
        export_extras=True,
        export_animations=False,
        export_cameras=False,
        export_lights=False,
        export_image_format="AUTO",
    )


def preview(path, meshes):
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 24
    scene.cycles.use_denoising = True
    scene.render.resolution_x = 1000
    scene.render.resolution_y = 1200
    scene.render.resolution_percentage = 100
    scene.render.film_transparent = True
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    scene.world = bpy.data.worlds.new("Preview studio")
    scene.world.use_nodes = True
    scene.world.node_tree.nodes.get("Background").inputs["Strength"].default_value = 0.45
    scene.view_settings.view_transform = "AgX"
    for location, energy, size in (
        ((-3, -4, 4), 450, 4),
        ((3, -1, 2), 250, 3),
        ((0, 4, 3), 500, 3),
    ):
        bpy.ops.object.light_add(type="AREA", location=location)
        light = bpy.context.object
        light.data.energy = energy
        light.data.shape = "DISK"
        light.data.size = size
        light.rotation_euler = (-light.location).to_track_quat("-Z", "Y").to_euler()
    target = Vector((0, 0.795936388, 0.605380237)) * 0.2
    normal = Vector((0, -0.605380237, 0.795936388))
    bpy.ops.object.camera_add(location=target + normal * 5)
    camera = bpy.context.object
    camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
    camera.data.type = "ORTHO"
    camera.data.ortho_scale = 2.5
    scene.camera = camera
    scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)


def main():
    args = arguments()
    source = args.source.expanduser().resolve(strict=True)
    output = args.output_dir.resolve()
    if output.exists() and any(output.iterdir()):
        raise FileExistsError(f"Refusing to overwrite nonempty directory: {output}")
    # This mapping was visually inspected for this exact export, not inferred
    # from generic mesh numbering in arbitrary OBJ files.
    expected_obj_hash = "893ea1a212ce9c1ffee5b0adb333dcaad3e6aee78909d7ff3839c31c9cd6377d"
    with tempfile.TemporaryDirectory(prefix="omnipotent-obj-") as tmp:
        temp = Path(tmp)
        with zipfile.ZipFile(source) as archive:
            obj_bytes = archive.read("model/model.obj")
            if hashlib.sha256(obj_bytes).hexdigest() != expected_obj_hash:
                raise ValueError("Not the inspected six-part low-poly export")
            source_faces = {}
            current = None
            for line in obj_bytes.splitlines():
                fields = line.split()
                if fields and fields[0] == b"o":
                    current = fields[1].decode()
                    source_faces[current] = 0
                elif fields and fields[0] == b"f":
                    source_faces[current] += 1
            for name in (
                "model/model.obj", "model/model.mtl",
                "model/textures/model_baseColor.png",
                "model/textures/model_normal.png",
                "model/textures/model_metallicRoughness.png",
            ):
                target = temp / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(archive.read(name))
        bpy.ops.wm.read_factory_settings(use_empty=True)
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        screen_tools = importlib.import_module("model_screen")
        cleaned_pixels = screen_tools.clean_atlas(temp / "model/textures", obj_bytes)
        bpy.ops.wm.obj_import(filepath=str(temp / "model/model.obj"))
        meshes = sorted((o for o in bpy.context.scene.objects if o.type == "MESH"), key=lambda o: o.name)
        if [o.name for o in meshes] != [f"mesh_{i}_1" for i in range(1, 7)]:
            raise ValueError("Unexpected part list")
        low, high = bounds(meshes)
        center = (low + high) / 2
        scale = 2 / max(high - low)
        records = []
        for obj in meshes:
            original = obj.name
            # Bake OBJ import orientation, center the assembly, and uniformly
            # normalize without asserting any physical dimensions.
            world = obj.matrix_world.copy()
            for vertex in obj.data.vertices:
                vertex.co = (world @ vertex.co - center) * scale
            obj.matrix_world.identity()
            obj.data.update()
            lo, hi = bounds([obj])
            pivot = (lo + hi) / 2
            for vertex in obj.data.vertices:
                vertex.co -= pivot
            obj.location = pivot
            obj.data.update()
            obj.name = (
                "enclosure_with_antenna" if original == "mesh_5_1"
                else "sled_assembly" if original == "mesh_6_1"
                else f"unidentified_fragment_{original.split('_')[1]}"
            )
            obj["source_object"] = original
            obj["identification"] = "visual inspection; fragments not assigned to a mechanical part"
            records.append({"source": original, "node": obj.name, "vertices": len(obj.data.vertices),
                            "source_faces": source_faces[original],
                            "faces": len(obj.data.polygons),
                            "triangles": sum(len(p.vertices) - 2 for p in obj.data.polygons)})
        bpy.context.view_layer.update()
        root = empty("omnipotent")
        root["units"] = "normalized presentation units, not CAD dimensions"
        root["display_status"] = "independent flat epaper_surface; cleaned colour/normal/roughness atlas"
        root["sled_seat_offset"] = [0, 0.55 * 0.605380237 * scale, -0.55 * 0.795936388 * scale]
        root["antenna_status"] = "part of enclosure geometry"
        static = empty("unidentified_fragments")
        parent_preserving_transform(static, root)
        for obj in meshes:
            parent_preserving_transform(obj, static if obj.name.startswith("unidentified") else root)
        screen = screen_tools.add_screen(center, scale)
        parent_preserving_transform(screen, root)
        objects = [root, static, *meshes, screen]
        records.append({"source": "generated", "node": screen.name,
                        "vertices": len(screen.data.vertices), "source_faces": 0,
                        "faces": len(screen.data.polygons), "triangles": len(screen.data.polygons)})
        output.mkdir(parents=True, exist_ok=True)
        variants = []
        for size, name in ((1024, "omnipotent-1k.glb"), (2048, "omnipotent-2k.glb")):
            mat = material(temp / "model/textures", size)
            for obj in meshes:
                obj.data.materials.clear()
                obj.data.materials.append(mat)
            export(output / name, objects)
            variants.append({"file": name, "texture_size": size, "bytes": (output / name).stat().st_size})
        # Save only the prepared assembly and packed materials, before adding
        # preview cameras/lights. Remove unused imported images referencing temp.
        for mat in list(bpy.data.materials):
            if mat.users == 0:
                bpy.data.materials.remove(mat)
        for image in list(bpy.data.images):
            if image.users == 0:
                bpy.data.images.remove(image)
        bpy.ops.wm.save_as_mainfile(filepath=str(output / "omnipotent.blend"))
        preview(output / "poster.png", [*meshes, screen])
        manifest = {
            "source_archive": source.name,
            "source_archive_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "source_obj_sha256": hashlib.sha256(obj_bytes).hexdigest(),
            "blender": bpy.app.version_string,
            "normalization": {"source_center_blender_z_up": list(center), "scale": scale,
                              "longest_axis": 2, "glb_axes": "glTF right-handed Y-up"},
            "parts": records, "variants": variants,
            "geometry_note": "No decimation applied. Blender's OBJ importer omits two enclosure faces from the source; the source contains zero-area triangles.",
            "material_note": "Screen atlas regions repainted paper, neutral tangent normal, matte/nonmetallic. Independent screen material has no texture or bump/normal map. Surrounding shell maps preserved.",
            "screen": {"resolution": [296, 160], "cleaned_atlas_pixels": cleaned_pixels,
                       "aspect": 296 / 160, "source_bounds": [screen_tools.LEFT, screen_tools.RIGHT, screen_tools.BOTTOM, screen_tools.TOP]},
            "limitations": ["antenna remains part of enclosure mesh", "screen fit is approximate concept geometry, not physical CAD",
                            "four small unidentified fragments are retained without mechanical animation assignments",
                            "source geometry is a visual concept, not dimensionally validated engineering CAD"],
        }
        (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        print("PREPARED", json.dumps(manifest))


if __name__ == "__main__":
    main()
