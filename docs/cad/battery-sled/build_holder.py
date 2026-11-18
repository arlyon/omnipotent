"""Generate the mechanical 21700/pogo fit prototype using FreeCAD's Python API.

Run with the system Python that matches FreeCAD (see README). Parameters are in
parameters.json; rerun to regenerate. Contacts are external envelope models,
not a manufacturer STEP model or a finished electrical/PCB design.
"""
import importlib
import importlib.util
import json
import os
import sys
from pathlib import Path

# FreeCAD ships native modules outside this project's Python environment.
if importlib.util.find_spec("FreeCAD") is None:
    sys.path.insert(0, "/usr/lib64/freecad/lib64")
App = importlib.import_module("FreeCAD")
Part = importlib.import_module("Part")
MeshPart = importlib.import_module("MeshPart")

ROOT = Path(__file__).resolve().parent
V = App.Vector
BLACK = (0.12, 0.13, 0.15)
RED = (0.75, 0.12, 0.10)
GOLD = (0.92, 0.65, 0.16)
GREEN = (0.12, 0.40, 0.25)


def box(x, y, z, dx, dy, dz):
    return Part.makeBox(dx, dy, dz, V(x, y, z))


def cylinder(x, y, z, radius, height):
    return Part.makeCylinder(radius, height, V(x, y, z))


def union(shapes):
    shape = shapes[0]
    for other in shapes[1:]:
        shape = shape.fuse(other)
    return shape.removeSplitter()


def feature(doc, name, shape, colour, group, label=None):
    obj = doc.addObject("PartDesign::Feature", name)
    obj.Label = label or name
    obj.Shape = shape.removeSplitter()
    if App.GuiUp:
        obj.ViewObject.ShapeColor = colour
        obj.ViewObject.LineColor = (0.18, 0.18, 0.18)
    group.addObject(obj)
    return obj


def wire_path(points, radius):
    segments = []
    for start, end in zip(points, points[1:], strict=False):
        direction = V(*end) - V(*start)
        segments.append(Part.makeCylinder(radius, direction.Length, V(*start), direction))
    segments.extend(Part.makeSphere(radius, V(*point)) for point in points[1:-1])
    return union(segments)


def generate(output_dir=None):
    try:
        p = json.loads((ROOT / "parameters.json").read_text())
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"Cannot load CAD parameters from {ROOT}") from exc
    out = Path(output_dir) if output_dir else ROOT / "generated"
    out.mkdir(parents=True, exist_ok=True)
    for document in list(App.listDocuments()):
        if document == "BatterySled":
            App.closeDocument(document)
    doc = App.newDocument("BatterySled")
    printed = doc.addObject("App::DocumentObjectGroup", "PrintedParts")
    purchased = doc.addObject("App::DocumentObjectGroup", "ContactAssemblies")
    reference = doc.addObject("App::DocumentObjectGroup", "ReferenceOnly")
    sheet = doc.addObject("Spreadsheet::Sheet", "Dimensions")
    sheet.set("A1", "Generator parameters (edit parameters.json and rebuild)")
    for row, (key, value) in enumerate(p.items(), 3):
        sheet.set(f"A{row}", key)
        sheet.set(f"B{row}", str(value))
    sheet.setColumnWidth("A", 310)
    sheet.setColumnWidth("B", 180)

    pitch = p["cell_pitch"]
    centres = [0, pitch, 2 * pitch]
    valleys = [pitch / 2, 1.5 * pitch]
    r = p["outer_radius"]
    bore = p["cell_diameter"] / 2 + p["cell_radial_clearance"]
    height = p["cell_length"] + 2 * p["cell_axial_clearance"] + 2 * p["end_wall"]
    cz = p["end_wall"] + p["cell_axial_clearance"]
    assert pitch > 2 * bore, "Cell bores overlap"
    assert 0 < p["pogo_compression"] < p["pogo_max_stroke"]
    # Bottom targets are recessed behind a 1 mm insulating keeper. Pin bodies
    # enter the target windows; external frame shoulders still meet at z=0/H.
    pad_plane = p["keeper_thickness"]
    flange_z = height + pad_plane + p["pogo_compression"] - p["pogo_free_height"]
    carrier_z = flange_z - p["carrier_thickness"]
    yc = sum(p["contact_pair_y"]) / 2
    points = [(x, y) for x in valleys for y in p["contact_pair_y"]]
    cw, cd = p["carrier_width"], p["carrier_depth"]
    fit = p["carrier_fit_clearance"]

    # Capsule perimeter, three cell bores and a broad side-loading aperture.
    blank = union([
        box(0, -r, 0, 2 * pitch, 2 * r, height),
        cylinder(0, 0, 0, r, height),
        cylinder(2 * pitch, 0, 0, r, height),
    ])
    bores = union([cylinder(x, 0, p["end_wall"], bore,
                           height - 2 * p["end_wall"]) for x in centres])
    shell = blank.cut(bores)
    loading = box(-r - 1, -r - 1, p["end_wall"],
                  2 * pitch + 2 * r + 2, r + 1, height - 2 * p["end_wall"])
    frame = shell.cut(loading)
    lid = shell.common(loading)

    # Side-panel screws: clearance in lid and pilot bores in the rear half.
    screw_positions = [(x, z) for x in (-12.7, 2 * pitch + 12.7)
                       for z in (18, height - 18)]
    for x, z in screw_positions:
        tool = Part.makeCylinder(1.15, 2 * r + 2, V(x, -r - 1, z), V(0, 1, 0))
        lid = lid.cut(tool)
        pilot = Part.makeCylinder(0.80, 5, V(x, 0, z), V(0, 1, 0))
        frame = frame.cut(pilot)

    # Rear wire chases. Slide-in covers run down grooves and are trapped by
    # the end keepers when assembled. Wiring is accessible before closure.
    covers = []
    for i, x in enumerate(valleys):
        frame = frame.cut(box(x - 2, 11.2, 1, 4, r - 11.2 + 1, height - 2))
        for side in (-1, 1):
            frame = frame.cut(box(x + side * 2.3 - 0.3, r - 0.9, 1,
                                  0.6, 1.0, height - 2))
        cover = union([
            box(x - 1.85, r - 0.7, 1.3, 3.7, 0.7, height - 2.6),
            box(x - 2.45, r - 0.65, 1.3, 4.9, 0.45, height - 2.6),
        ])
        covers.append(feature(doc, f"WireCover{i + 1}", cover, BLACK, printed))

    # Pin carriers drop tail-first from above onto 0.6 mm-wide side shelves.
    # Bottom target boards insert from below and are trapped by their keeper.
    for x in valleys:
        frame = frame.cut(box(x - cw / 2 - fit, yc - cd / 2 - fit,
                              carrier_z, cw + 2 * fit, cd + 2 * fit,
                              height - carrier_z + 1))
        frame = frame.cut(box(x - 1.6, yc - cd / 2 - fit, height - 5.0,
                              3.2, r - (yc - cd / 2 - fit) + 1, 5.1))
        frame = frame.cut(box(x - cw / 2 - fit, yc - cd / 2 - fit, -0.1,
                              cw + 2 * fit, cd + 2 * fit,
                              pad_plane + 0.035 + p["carrier_thickness"] + 0.1))
        frame = frame.cut(box(x - 1.6, yc - cd / 2 - fit, 0,
                              3.2, r - (yc - cd / 2 - fit) + 1, 6.0))

    # A pair of screwed end keepers traps all contact boards and wire covers.
    keeper_x = valleys[0] - 8.0
    keeper_len = valleys[1] - valleys[0] + 16.0
    keeper_y = 5.3
    keeper_depth = r - keeper_y
    top_z = height - p["keeper_thickness"]
    top = box(keeper_x, keeper_y, top_z, keeper_len, keeper_depth,
              p["keeper_thickness"])
    bottom = box(keeper_x, keeper_y, 0, keeper_len, keeper_depth,
                 p["keeper_thickness"])
    # Top keeper replaces the top millimetre of the frame locally.
    frame = frame.cut(top)
    frame = frame.cut(bottom)
    for x in valleys:
        # Feet press only component-free outer edges of the carrier boards.
        for side in (-1, 1):
            foot_x = x - cw / 2 if side < 0 else x + cw / 2 - 0.45
            top = top.fuse(box(foot_x, yc - cd / 2, flange_z,
                               0.45, cd, top_z - flange_z))
    for x, y in points:
        top = top.cut(cylinder(x, y, flange_z - 0.1,
                               p["pogo_keeper_bore"] / 2, height - flange_z + 1))
        bottom = bottom.cut(cylinder(x, y, -0.1, (p["pad_diameter"] + 0.3) / 2, 1.2))
    for x in (valleys[0] - 6, valleys[1] + 6):
        y = 12.5
        top = top.cut(cylinder(x, y, top_z - 0.1, 1.1, 1.2))
        bottom = bottom.cut(cylinder(x, y, -0.1, 1.1, 1.2))
        frame = frame.cut(cylinder(x, y, height - 4, 0.8, 4.1))
        frame = frame.cut(cylinder(x, y, 0, 0.8, 4.0))
    frame = frame.removeSplitter()
    lid = lid.removeSplitter()
    frame_obj = feature(doc, "RearFrame", frame, BLACK, printed,
                        "ASA rear cradle — side loading")
    lid_obj = feature(doc, "SidePanel", lid, BLACK, printed,
                      "ASA front retaining panel — four M2 clearance holes")
    top_obj = feature(doc, "TopKeeper", top, RED, printed,
                      "Top contact keeper — two M2 screws")
    bottom_obj = feature(doc, "BottomKeeper", bottom, RED, printed,
                         "Bottom target keeper — two M2 screws")

    cells = []
    for i, x in enumerate(centres):
        cells.append(feature(doc, f"Cell{i + 1}",
                             cylinder(x, 0, cz, p["cell_diameter"] / 2, p["cell_length"]),
                             (0.65, 0.68, 0.72), reference,
                             f"Cell {i + 1} — nominal envelope, not a selected cell"))
    # Thin split liner envelopes illustrate full circumferential support.
    # Their material/preload must be selected experimentally; these are not STL parts.
    for i, x in enumerate(centres):
        liner = cylinder(x, 0, cz + 3, bore, p["cell_length"] - 6).cut(
            cylinder(x, 0, cz + 2.9, p["cell_diameter"] / 2,
                     p["cell_length"] - 5.8))
        for name, y in (("Rear", 0), ("Front", -bore)):
            half = liner.common(box(x - bore, y, cz + 3,
                                    2 * bore, bore, p["cell_length"] - 6))
            feature(doc, f"{name}Liner{i + 1}", half, (0.25, 0.50, 0.65), reference,
                    f"Cell {i + 1}: {name.lower()} liner envelope — material TBD")
    contacts, boards, targets, wires = [], [], [], []
    for i, x in enumerate(valleys):
        board = box(x - cw / 2, yc - cd / 2, carrier_z, cw, cd,
                    p["carrier_thickness"])
        for y in p["contact_pair_y"]:
            board = board.cut(cylinder(x, y, carrier_z - 0.1, 0.254,
                                       p["carrier_thickness"] + 0.2))
        boards.append(feature(doc, f"PinCarrier{i + 1}", board, GREEN, purchased,
                              f"Pair {i + 1}: 4 × 6.4 × 0.8 mm carrier PCB — conceptual"))
        targets.append(feature(doc, f"TargetCarrier{i + 1}",
                               box(x - cw / 2, yc - cd / 2, pad_plane + 0.035,
                                   cw, cd, p["carrier_thickness"]), GREEN, purchased,
                               f"Pair {i + 1}: target PCB — conceptual"))
        for j, y in enumerate(p["contact_pair_y"]):
            net = ("BAT+", "GND")[j] if i == 0 else ("SDA", "SCL")[j]
            pin = union([
                cylinder(x, y, flange_z - p["pogo_tail_length"],
                         p["pogo_tail_diameter"] / 2, p["pogo_tail_length"]),
                cylinder(x, y, flange_z, p["pogo_flange_diameter"] / 2,
                         p["pogo_flange_thickness"]),
                cylinder(x, y, flange_z + p["pogo_flange_thickness"],
                         p["pogo_body_diameter"] / 2,
                         p["pogo_body_height"] - p["pogo_flange_thickness"]),
                cylinder(x, y, flange_z + p["pogo_body_height"],
                         p["pogo_tip_diameter"] / 2,
                         p["pogo_free_height"] - p["pogo_body_height"]),
            ])
            pin_obj = feature(doc, f"Pogo{i}{j}", pin, GOLD, purchased,
                              f"{net}: Mill-Max 0906-0-15-20-76-14-11-0 — free")
            pin_obj.addProperty("App::PropertyString", "ManufacturerPart").ManufacturerPart = \
                "0906-0-15-20-76-14-11-0"
            contacts.append(pin_obj)
            targets.append(feature(doc, f"Target{i}{j}",
                                   cylinder(x, y, pad_plane, p["pad_diameter"] / 2, 0.035),
                                   GOLD, purchased, f"{net}: fixed gold target"))
            wx = x + (-0.8 if j == 0 else 0.8)
            route = wire_path([(x, y, 2.5), (wx, 12.3, 4.5),
                               (wx, 12.3, height - 4.5),
                               (x, y, carrier_z - (1.2 if j == 0 else 0.3))],
                              p["wire_diameter"] / 2)
            wires.append(feature(doc, f"Wire{i}{j}", route,
                    ((0.8, 0.1, 0.1), (0.12, 0.12, 0.13))[j] if i == 0
                    else ((0.1, 0.5, 0.85), (0.85, 0.7, 0.1))[j], reference,
                    f"{net}: routing envelope only — no solder joints/branch wiring"))

    # A separate fit coupon reproduces the pin carrier shelf and keeper.
    coupon_region = box(valleys[0] - 7, 4.8, height - 8,
                        14, r - 4.8, 8)
    coupon = frame.common(coupon_region)
    coupon_obj = feature(doc, "FitCoupon", coupon, BLACK, reference,
                         "Print first: one-valley pocket and pin-carrier shelf")
    coupon_keeper = top.common(box(valleys[0] - 7, 4.8, 0, 14, r - 4.8, height + 1))
    coupon_keeper_obj = feature(doc, "CouponKeeper", coupon_keeper, RED, reference)

    # Programmatic checks are part of the artifact, not an assumed fit.
    checks = {}
    required = [frame_obj, lid_obj, top_obj, bottom_obj, coupon_obj, coupon_keeper_obj] + covers
    for obj in required + contacts + boards + targets + cells:
        assert obj.Shape.isValid() and not obj.Shape.isNull(), obj.Name
        assert len(obj.Shape.Solids) == 1, (obj.Name, "not one connected solid")
    checks["all_parts_valid_single_solids"] = True
    tolerance = 1e-6
    for cell in cells:
        for part in [frame_obj, lid_obj, top_obj, bottom_obj] + covers + contacts + boards + targets:
            assert cell.Shape.common(part.Shape).Volume < tolerance, (cell.Name, part.Name)
    checks["no_cell_interference"] = True
    for pin in contacts:
        for part in [frame_obj, top_obj, bottom_obj] + boards:
            assert pin.Shape.common(part.Shape).Volume < tolerance, (pin.Name, part.Name)
    checks["pins_clear_frame_keeper_and_carriers"] = True
    for a, b in [(frame_obj, lid_obj), (frame_obj, top_obj), (frame_obj, bottom_obj)]:
        assert a.Shape.common(b.Shape).Volume < tolerance, (a.Name, b.Name)
    checks["frame_and_closures_do_not_overlap"] = True
    for wire in wires:
        for part in [frame_obj, lid_obj, top_obj, bottom_obj] + covers + cells:
            assert wire.Shape.common(part.Shape).Volume < tolerance, (wire.Name, part.Name)
    for i, wire in enumerate(wires):
        for other in wires[i + 1:]:
            assert wire.Shape.common(other.Shape).Volume < tolerance, (wire.Name, other.Name)
    checks["wire_envelopes_clear_structure_cells_and_each_other"] = True
    # Check the actual next-module target recess with pins at working stroke.
    for pin in contacts:
        # Only the plunger changes length; body and tail are unchanged.
        bb = pin.Shape.BoundBox
        compressed = pin.Shape.cut(box(bb.XMin - 1, bb.YMin - 1,
                                       height + pad_plane,
                                       bb.XLength + 2, bb.YLength + 2, 2))
        for part in [frame_obj, bottom_obj] + targets:
            upper = part.Shape.copy()
            upper.translate(V(0, 0, height))
            assert compressed.common(upper).Volume < tolerance, (pin.Name, part.Name, "stack")
    checks["stacked_interface_clears_at_working_compression"] = True
    # Cell swept sideways through the opening; no structural load on contacts.
    for x in centres:
        swept = union([
            cylinder(x, 0, cz, p["cell_diameter"] / 2, p["cell_length"]),
            cylinder(x, -35, cz, p["cell_diameter"] / 2, p["cell_length"]),
            box(x - p["cell_diameter"] / 2, -35, cz,
                p["cell_diameter"], 35, p["cell_length"]),
        ])
        assert swept.common(frame).Volume < tolerance, "Side loading obstructed"
    checks["side_loading_sweeps_clear_frame"] = True
    # Contact top-insertion swept envelopes clear all shelves until PCB seating.
    for x in valleys:
        insertion = box(x - cw / 2, yc - cd / 2, carrier_z, cw, cd,
                        height + 10 - carrier_z)
        assert insertion.common(frame).Volume < tolerance, "Carrier top insertion obstructed"
    checks["top_insert_carriers_clear_frame"] = True
    # Keeper holes admit the solid body of the previous module, not just its tip.
    assert p["pad_diameter"] + 0.3 > p["pogo_body_diameter"]
    checks["nominal_compression_mm"] = p["pogo_compression"]
    checks["maximum_stroke_mm"] = p["pogo_max_stroke"]
    checks["frame_dimensions_mm"] = [2 * pitch + 2 * r, 2 * r, height]
    checks["cell_radial_clearance_mm"] = p["cell_radial_clearance"]
    checks["prototype_only"] = True

    # Restore a useful initial view; the side panel is hidden to expose cells.
    if App.GuiUp:
        lid_obj.Visibility = False
        coupon_obj.Visibility = False
        coupon_keeper_obj.Visibility = False
        for cell in cells:
            cell.ViewObject.Transparency = 35
        App.Gui.activeDocument().activeView().viewAxonometric()
        App.Gui.activeDocument().activeView().fitAll()
    doc.recompute()
    doc.saveAs(str(out / "21700-pogo-holder.FCStd"))
    export = [frame_obj, lid_obj, top_obj, bottom_obj] + covers + contacts + boards + targets + cells
    Part.export(export, str(out / "21700-pogo-holder.step"))
    for obj in required:
        # Normalise the coupon/keepers to their print bed, preserving orientation.
        shape = obj.Shape.copy()
        shape.translate(V(-shape.BoundBox.XMin, -shape.BoundBox.YMin, -shape.BoundBox.ZMin))
        mesh = MeshPart.meshFromShape(Shape=shape, LinearDeflection=0.08,
                                      AngularDeflection=0.15, Relative=False)
        mesh.write(str(out / f"{obj.Name}.stl"))
    (out / "validation.json").write_text(json.dumps(checks, indent=2) + "\n")
    if App.GuiUp:
        App.Gui.activeDocument().activeView().saveImage(str(out / "assembly.png"), 1600, 1000, "White")
    print(json.dumps(checks, indent=2))
    return doc


if __name__ == "__main__":
    generate(os.environ.get("BATTERY_CAD_OUTPUT"))
