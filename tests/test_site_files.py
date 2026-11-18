"""Validate local showcase references without starting a server or installing tools."""
import hashlib
import json
import re
import unittest
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


class Page(HTMLParser):
    def __init__(self):
        super().__init__()
        self.ids = []
        self.references = []
        self.navigation = set()
        self.h1s = 0
        self.elements = []

    def handle_starttag(self, tag, attrs):
        attributes = dict(attrs)
        self.elements.append((tag, attributes))
        if tag == "a" and attributes.get("href"):
            self.navigation.add(attributes["href"])
        if "id" in attributes:
            self.ids.append(attributes["id"])
        self.h1s += tag == "h1"
        for name in ("href", "src"):
            if name in attributes:
                self.references.append(attributes[name])
        srcset = attributes.get("srcset")
        if srcset:
            self.references.extend(part.strip().split()[0] for part in srcset.split(",") if part.strip())


class SiteFiles(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.html = (ROOT / "index.html").read_text()
        cls.page = Page()
        cls.page.feed(cls.html)

    def test_unique_ids_and_single_primary_heading(self):
        self.assertEqual(len(self.page.ids), len(set(self.page.ids)))
        self.assertEqual(self.page.h1s, 1)

    def test_local_references_exist(self):
        for reference in self.page.references:
            with self.subTest(reference=reference):
                url = urlsplit(reference)
                if url.scheme:
                    self.assertEqual(url.scheme, "https")
                    self.assertIn(reference, self.page.navigation, "Only navigation links may leave the locally hosted showcase")
                    self.assertTrue(url.netloc)
                    continue
                if url.path:
                    target = (ROOT / unquote(url.path)).resolve()
                    self.assertTrue(target.is_relative_to(ROOT))
                    self.assertTrue(target.exists())
                elif url.fragment:
                    self.assertIn(url.fragment, self.page.ids)
        css = (ROOT / "styles.css").read_text()
        for reference in re.findall(r"url\(['\"]?([^)'\"]+)", css):
            self.assertTrue((ROOT / reference).is_file(), reference)

    def test_graphics_are_passive_accessible_svg(self):
        graphics = list((ROOT / "assets/graphics").glob("*.svg"))
        self.assertEqual(len(graphics), 6)
        for path in graphics:
            with self.subTest(file=path.name):
                # Inspect markup without an XML parser or entity expansion.
                parsed = Page()
                parsed.feed(path.read_text())
                tags = [tag for tag, _ in parsed.elements]
                self.assertEqual(tags[0], "svg")
                self.assertIn("title", tags)
                self.assertIn("viewbox", parsed.elements[0][1])
                for tag, attributes in parsed.elements:
                    self.assertNotIn(tag, ("script", "foreignobject"))
                    self.assertTrue(all(not key.lower().startswith("on") for key in attributes))

    def test_import_map_is_local(self):
        match = re.search(r'<script type="importmap">(.*?)</script>', self.html, re.S)
        if match is None:
            self.fail("Missing import map")
        imports = json.loads(match.group(1))["imports"]
        self.assertEqual(set(imports), {"three", "three/addons/"})
        for value in imports.values():
            self.assertTrue(value.startswith("./assets/vendor/three/"))
            self.assertTrue((ROOT / value).exists())

    def test_requested_product_features_are_described(self):
        self.assertIn("Semantic search", self.html)
        self.assertIn("across the entire corpus", self.html)
        self.assertIn("Designed to be fully weather-resistant", self.html)
        self.assertIn("Modular three-cell base", self.html)
        self.assertIn("future extensions", self.html)

    def test_battery_example_exposes_assumptions_and_calculation(self):
        nominal_wh = 3 * 3.5 * 3.6
        usable_wh = nominal_wh * 0.8
        self.assertIn("Three 3,500 mAh cells at 3.6 V", self.html)
        self.assertIn(f"{nominal_wh:.1f} Wh nominal", self.html)
        self.assertIn(f"{usable_wh:.1f} Wh usable", self.html)
        self.assertIn("Designed for a year between charges", self.html)
        for hours in (16, 8, 4):
            shortest = round(usable_wh / (0.005 * 3.3 * hours) / 30.4375)
            longest = round(usable_wh / (0.002 * 3.3 * hours) / 30.4375)
            self.assertIn(f"<td>{hours} hours", self.html)
            self.assertIn(f"{shortest} to {longest} months", self.html)
        self.assertIn("a connected average of 2 to 5 mA at 3.3 V", self.html)
        self.assertIn("not measured board runtime", self.html)
        self.assertNotIn("four active minutes", self.html)
        self.assertIn("Wi-Fi Target Wake Time (TWT)", self.html)
        self.assertIn("full power-off between reed-sensor wakes", self.html)
        self.assertNotIn("Estimated runtime: 30 hours", self.html)
        self.assertNotIn("10 mW asleep", self.html)
        self.assertIn("battery-estimate", self.page.ids)

    def test_dialog_library_is_local_pinned_and_labelled(self):
        vendor = ROOT / "assets/vendor/a11y-dialog"
        self.assertEqual(hashlib.sha256((vendor / "a11y-dialog.esm.js").read_bytes()).hexdigest(),
                         "e384db98aac859596899f60c16c589604e8f92b2adea1652bb0645ad4339b83c")
        for filename in ("LICENSE", "LICENSE-focusable-selectors"):
            self.assertIn("The MIT License", (vendor / filename).read_text())
        self.assertIn("../assets/vendor/a11y-dialog/a11y-dialog.esm.js", (ROOT / "js/site.mjs").read_text())
        dialog = next(attributes for _, attributes in self.page.elements if attributes.get("id") == "details-dialog")
        disclosures = {attributes["id"] for tag, attributes in self.page.elements
                       if tag == "details" and "data-dialog-label" in attributes}
        self.assertEqual(disclosures, {"preview-details", "medical-details", "battery-estimate", "kindle-details", "build-details", "format-details", "transport-details"})
        self.assertEqual(dialog.get("role"), "dialog")
        self.assertEqual(dialog.get("aria-modal"), "true")
        self.assertIn(dialog.get("aria-labelledby"), self.page.ids)
        self.assertEqual(dialog.get("aria-hidden"), "true")

    def test_complete_xberg_catalogue_is_pinned_and_includes_mime_only_formats(self):
        path = ROOT / "assets/data/xberg-formats.json"
        self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), "9eb9ac6f6d65295ca75a767bf73426f8874a52411e35433dc3525fefe727a0f4")
        catalogue = json.loads(path.read_text())
        self.assertEqual(catalogue["schema"], 1)
        formats = catalogue["formats"]
        self.assertEqual(len(formats), 107)
        self.assertEqual(len({item["mime"] for item in formats}), 107)
        self.assertEqual(len({ext for item in formats for ext in item["extensions"]}), 141)
        self.assertEqual(len({alias for item in formats for alias in item["aliases"]}), 65)
        self.assertEqual({item["mime"] for item in formats if not item["extensions"]},
                         {"text/x-gfm", "text/x-markdown-extra", "text/x-multimarkdown", "text/x-pandoc", "application/csl+json", "text/x-source-code"})
        self.assertEqual(len(catalogue["languages"]), 371)
        self.assertEqual(len({item["id"] for item in catalogue["languages"]}), 371)
        self.assertEqual(catalogue["provenance"]["commit"], "246c33b12444014de04cdf39150db0597e8d168c")
        self.assertEqual(catalogue["provenance"]["language_pack_version"], "1.25.2")
        for item in formats + catalogue["languages"]:
            self.assertTrue(item["name"])
            self.assertIsInstance(item["extensions"], list)

    def test_protocol_sections_sources_and_capability_boundaries(self):
        for section in ("protocols", "file-formats", "sources", "transmissions"):
            self.assertIn(section, self.page.ids)
        for source in ("Wikipedia", "OpenStreetMap", "PubMed Central", "arXiv", "Project Gutenberg", "Standard Ebooks", "Your own files"):
            self.assertIn(source, self.html)
        for protocol in ("HTTP/1.1", "OPDS 2.0", "WebDAV", "mDNS", "DNS-SD", "DHCP", "RSS", "Atom", "OAI-PMH", "GATT", "LoRa", "microSD"):
            self.assertIn(protocol, self.html)
        self.assertNotIn("OPDS 1.2", self.html)
        self.assertIn('Reader catalogues<span class="support-state">Linux sample</span>', self.html)
        self.assertIn('Resume &amp; seek<span class="support-state">Planned</span>', self.html)
        self.assertIn("Application framing is still to be selected", self.html)
        self.assertIn("separately from Xberg extraction", self.html)

    def test_no_generated_publications_or_capacity_claims(self):
        self.assertIn("example collections for the device", self.html)
        self.assertEqual(self.html.count('class="collection-step"'), 6)
        self.assertIn("Six collections.", self.html)
        self.assertNotIn("not a measured claim yet", self.html)
        self.assertIn('<epaper-display id="epaper" state="sleep">', self.html)
        self.assertNotIn('href="README.md"', self.html)
        self.assertNotIn("firmware feasibility task", self.html)


if __name__ == "__main__":
    unittest.main()
