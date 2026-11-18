"""Static archive checks, including the complete no-JavaScript navigation."""
import re
import unittest
from pathlib import Path
from urllib.parse import unquote, urlsplit

from PIL import Image
from test_site_files import Page

ROOT = Path(__file__).resolve().parents[1]


class Gallery(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.html = (ROOT / "assets.html").read_text()
        cls.page = Page()
        cls.page.feed(cls.html)

    def test_semantic_structure_and_collections(self):
        self.assertEqual(self.page.h1s, 1)
        self.assertEqual(len(self.page.ids), len(set(self.page.ids)))
        self.assertTrue({"renders", "cutouts", "screens", "drawings", "models"}.issubset(self.page.ids))
        self.assertEqual(sum(tag == "figure" for tag, _ in self.page.elements), 30)
        self.assertIn('href="assets.html"', (ROOT / "index.html").read_text())

    def test_all_local_references_and_fragments_exist(self):
        for reference in self.page.references:
            with self.subTest(reference=reference):
                url = urlsplit(reference)
                self.assertFalse(url.scheme or url.netloc)
                if url.path:
                    target = (ROOT / unquote(url.path)).resolve()
                    self.assertTrue(target.is_relative_to(ROOT))
                    self.assertTrue(target.is_file())
                elif url.fragment:
                    self.assertIn(url.fragment, self.page.ids)
        for reference in re.findall(r"from ['\"]([^'\"]+)", (ROOT / "js/gallery.mjs").read_text()):
            self.assertTrue((ROOT / "js" / reference).is_file())

    def test_image_dimensions_alt_text_and_transparency(self):
        for tag, attributes in self.page.elements:
            if tag != "img":
                continue
            with self.subTest(image=attributes.get("src", "dialog preview")):
                self.assertTrue(attributes.get("alt") or attributes.get("id") == "asset-dialog-image")
                if "src" not in attributes:
                    continue
                path = ROOT / attributes["src"]
                if path.suffix == ".svg":
                    box = re.search(r'viewBox="0 0 (\d+) (\d+)"', path.read_text())
                    if box is None:
                        self.fail(f"Missing SVG viewBox: {path}")
                    self.assertEqual((attributes["width"], attributes["height"]), box.groups())
                else:
                    with Image.open(path) as image:
                        self.assertEqual(image.size, (int(attributes["width"]), int(attributes["height"])))
                        if path.stem in {p.stem for p in (ROOT / "docs/assets/field-archive/transparent/cutouts").glob("*.png")}:
                            self.assertEqual(image.mode, "RGBA")
                            self.assertEqual(image.getchannel("A").getextrema(), (0, 255))

    def test_previews_are_lightweight_and_downloads_are_originals(self):
        previews = list((ROOT / "assets/gallery").glob("*.webp"))
        self.assertEqual(len(previews), 18)
        self.assertLess(sum(path.stat().st_size for path in previews), 3_000_000)
        downloads = [attrs["href"] for tag, attrs in self.page.elements if tag == "a" and "download" in attrs and "href" in attrs]
        self.assertEqual(len(downloads), 39)
        self.assertTrue(all(not href.startswith("assets/gallery/") for href in downloads))
        self.assertIn("not suitable for manufacturing", self.html)
        self.assertNotIn('src="js/site.mjs"', self.html)
        self.assertNotIn('type="importmap"', self.html)

    def test_original_links_survive_without_javascript(self):
        previews = [attrs for tag, attrs in self.page.elements if tag == "a" and "asset-preview" in attrs.get("class", "").split()]
        self.assertEqual(len(previews), 31)
        self.assertTrue(all(link["href"].endswith((".png", ".svg")) for link in previews))
        self.assertTrue(all(link.get("aria-label", "").startswith("Preview ") for link in previews))
        self.assertIn("a11y-dialog.esm.js", (ROOT / "js/gallery.mjs").read_text())


if __name__ == "__main__":
    unittest.main()
