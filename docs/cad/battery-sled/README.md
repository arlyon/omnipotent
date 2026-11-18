# Three-cell 21700 pogo holder — mechanical fit prototype

Open **`generated/21700-pogo-holder.FCStd`** in FreeCAD. The retaining side panel
is initially hidden so the cells are visible. Toggle `PrintedParts / SidePanel`
to see the closed cartridge. `ReferenceOnly` includes nominal cells, liner and
wire envelopes, and the two fit-coupon objects; these are not all production
parts. Preview images are in `generated/`.

## What is modelled

- Three side-loading nominal 21 × 70 mm cells on 22.6 mm centres.
- 74.2 × 29 × 80 mm capsule-shaped structural frame, before external enclosure.
- Removable front panel with four M2 clearance holes and matching 1.6 mm pilot
  holes. Pilot holes are prototype features, not qualified repeatedly serviceable
  threads; inserts/metal retention need a later detail design.
- Two two-contact pairs in the rear-side valleys, oriented perpendicular to the
  long edge. Left pair is BAT+/GND; right pair is SDA/SCL when viewed from the
  front with positive Y towards the wiring side. Match by net, not by an apparent
  left/right position after turning the part over.
- Top-insert two-pin carrier boards, locating shelves, top keeper and four pogo
  envelope models. Two M2 keeper screws use pilot holes in the frame.
- Bottom-insert target boards trapped by a bottom keeper; 2.2 mm gold pad
  envelopes are recessed 1 mm behind the structural mating plane.
- Rear wiring chases, slide-in covers and four separate 1 mm outside-diameter
  wire-routing envelopes. Solder fillets, actual board traces, battery branches,
  spot-welded nickel tabs, protection and charger circuits are **not** designed.
- Structural mating shoulders at z=0 and z=80. They take stack load. The positive
  terminal vent and the cells do not carry that load. The external frame/cap
  fastenings and final module alignment/keying are not included.
- Illustrative 0.3 mm split liner envelopes support the nominal cells around
  their circumference. Liner material, compression and cell tolerances remain
  to be selected; these are not ASA print parts.

## Selected contact

**Mill-Max `0906-0-15-20-76-14-11-0`**, short-stroke through-hole solder-tail pin.

| Dimension | Nominal value |
| --- | ---: |
| Maximum body diameter | 1.6256 mm |
| Flange diameter | 1.8288 mm |
| Tail diameter / length | 0.4318 / 1.7272 mm |
| Free height above flange underside | 3.4798 mm |
| Nominal working compression | 0.4953 mm |
| Full stroke | 0.9906 mm |
| Pair pitch in this prototype | 2.6 mm |

The manufacturer's dimensioned drawing is included as
`0906-0-manufacturer-drawing.svg`; sources and electrical specifications are in
`contact-source.json`. The manufacturer lists this variant active, with
multiple distributors. Verify current availability before buying.

This is **not** a press-fit or solder-cup pin. Its small flange is a poor choice
for relying on a tiny printed retaining lip. Instead, solder each pair into a
conceptual 4 × 6.4 × 0.8 mm carrier PCB with suitable wire pads, then insert the
complete assembly tail-first from the top. The keeper bears on component-free
PCB edges. Use solder fixtures and the manufacturer's soldering limits. Do not
solder to moving plungers.

The CAD carrier boards are mounting envelopes, **not Gerbers**. They show tail
holes but do not define finished pad sizes, plated-hole allowances, traces,
wire pads or solder-joint clearances. Gold target finish/wear specification also
needs selecting. Pin bodies use the manufacturer's maximum diameter envelope;
minor shell steps, tip curvature and the internal spring are not reproduced.

The pin flange sits at z=78.0155 mm. Free tips reach z=81.4953; the next module's
pads sit at z=81.0 when structural shoulders meet. Thus nominal compression is
0.4953 mm. This is a nominal geometry check, not a tolerance-stack qualification.

## Print and assemble the fit coupon first

1. Print `FitCoupon.stl` and `CouponKeeper.stl` in the intended ASA/process.
2. Check the PCB pocket, keeper access, 1.9 mm keeper bores and actual pin fit.
   Small printed bores may require calibration/reaming; do not force the pin.
3. Check a real carrier, solder joints and wires before printing the whole frame.
   The coupon is a sliced one-valley fixture, not a complete standalone battery
   holder or a full mechanical strength test.
4. Print the frame, panel, two keepers and two covers only after the fit is proven.
   STL parts have been moved to the print bed but are not optimised print
   orientations. End plates/bores may require a different orientation or support.
5. Bench-test assembly using inert cell-size/mass dummies. Do not put live cells
   in a first-fit print or test drop resistance with live cells.

Channels are accessible with the covers off. Solder wiring before inserting the
carrier boards and closing the keepers. The wire models show routing volume,
not electrically complete joints; use appropriately sized stranded insulation,
strain relief, termination pads and protection in the actual design.

## Editing / regeneration

The model is **generator-parametric**, not a fully constrained Sketcher assembly.
Edit `parameters.json` and regenerate. The FCStd contains named editable solids
and a parameter reference sheet; changing that sheet alone does not rebuild the
geometry. The chosen contact's dimensions must continue to match the actual part.

From FreeCAD: open and run **`BuildHolder.FCMacro`** through the Macro menu. Run
from this directory, and keep the macro beside the Python script and JSON file.
This gives the intended visibility and initial camera setup.

Headless generation on this machine (FreeCAD 1.1.4 native modules are installed):

```sh
/usr/bin/python3 docs/cad/battery-sled/build_holder.py
```

No new packages are required. The script uses the native FreeCAD module path
`/usr/lib64/freecad/lib64` if FreeCAD is not already importable. Adapt that path
for another installation. Headless generation does not render new previews.
Outputs go to `generated/`; set `BATTERY_CAD_OUTPUT` to use another directory.
Regeneration replaces generated outputs; preserve manual edits elsewhere.

## Validation and outstanding engineering

The generator fails on invalid solids, disconnected printable parts, cell
interference, pin/frame collisions, obstructed side/top insertion, conflicting
wire envelopes or an incompatible nominal stacked interface. Results are in
`generated/validation.json`. All eight exported STL meshes were also reopened
and checked watertight; the FCStd and STEP were reopened and validated.

These checks do **not** prove printability, drop strength, heat resistance,
contact life, I²C signal integrity or electrical safety. Before live use, resolve:

- Exact cell datasheet and dimensions, insulation and positive-terminal venting.
- Liner/preload selection; material creep and impact testing.
- Finished pin/PCB/housing tolerances and compression over the whole stack.
- Final fasteners, anti-shear/alignment features and outer frame/cap retention.
- Wire gauge, solder joints, fault protection, BMS/charging architecture and
  current/temperature measurements in the enclosure.
- Safe parallel-pack connection. Pogo pins do not limit equalisation current;
  generic protection and fuses do not make arbitrary-charge hot insertion safe.
- Native barrel/pad contacts are battery-powered and need short-circuit guarding.

The 21 × 63 mm ESP32 board, exterior shell and charging dock are outside this
first holder model. No battery or mains power is needed to inspect these files.
