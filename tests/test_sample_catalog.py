import json
import unittest
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SAMPLE = ROOT / "crates/omnipotent-serve/sample"


def parse_fixture_xml(payload: bytes) -> ET.Element:
    # Our generated EPUB uses UTF-8 without DTDs. Reject declarations before
    # parsing so validation cannot resolve or expand entities.
    text = payload.decode("utf-8")
    if "<!DOCTYPE" in text or "<!ENTITY" in text:
        raise ValueError("fixture XML must not contain DTDs or entity declarations")
    parser = ET.XMLPullParser(events=("start",))
    parser.feed(text)
    elements = list(parser.read_events())
    parser.close()
    if not elements:
        raise ValueError("missing XML root")
    first = elements[0]
    if len(first) != 2:
        raise ValueError("invalid XML event")
    root = first[1]
    if not isinstance(root, ET.Element):
        raise ValueError("expected XML root element")
    return root


class SampleCatalog(unittest.TestCase):
    def test_opds2_acquisition_feed_matches_native_epub(self):
        feed = json.loads((SAMPLE / "catalog.json").read_text())
        self.assertTrue(feed["metadata"]["title"])
        self.assertEqual(feed["metadata"]["numberOfItems"], len(feed["publications"]))
        self.assertIn(
            {"rel": "self", "href": "/opds", "type": "application/opds+json"},
            feed["links"],
        )
        publication = feed["publications"][0]
        link = publication["links"][0]
        self.assertEqual(link["rel"], "http://opds-spec.org/acquisition/open-access")
        self.assertEqual(link["type"], "application/epub+zip")
        self.assertEqual(link["href"], "/books/connection-check.epub")
        path = SAMPLE / "connection-check.epub"
        self.assertGreater(path.stat().st_size, 8192)
        with zipfile.ZipFile(path) as archive:
            self.assertIsNone(archive.testzip())
            first = archive.infolist()[0]
            self.assertEqual(first.filename, "mimetype")
            self.assertEqual(first.compress_type, zipfile.ZIP_STORED)
            self.assertEqual(archive.read("mimetype"), b"application/epub+zip")
            ns = {
                "container": "urn:oasis:names:tc:opendocument:xmlns:container",
                "opf": "http://www.idpf.org/2007/opf",
                "dc": "http://purl.org/dc/elements/1.1/",
            }
            container = parse_fixture_xml(archive.read("META-INF/container.xml"))
            rootfile = container.find(".//container:rootfile", ns)
            assert rootfile is not None
            package = parse_fixture_xml(archive.read(rootfile.attrib["full-path"]))
            self.assertEqual(package.attrib["version"], "3.0")
            title = package.find(".//dc:title", ns)
            identifier = package.find(".//dc:identifier", ns)
            assert title is not None and identifier is not None
            self.assertEqual(title.text, publication["metadata"]["title"])
            self.assertEqual(identifier.text, publication["metadata"]["identifier"])
            manifest = {
                item.attrib["id"]: item.attrib
                for item in package.findall("opf:manifest/opf:item", ns)
            }
            self.assertEqual(manifest["nav"]["properties"], "nav")
            for item in manifest.values():
                parse_fixture_xml(archive.read("EPUB/" + item["href"]))
            for item in package.findall("opf:spine/opf:itemref", ns):
                self.assertIn(item.attrib["idref"], manifest)


if __name__ == "__main__":
    unittest.main()
