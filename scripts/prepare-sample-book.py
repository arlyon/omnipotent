#!/usr/bin/env python3
"""Build/check the deterministic, original-text EPUB used for device bring-up."""

import argparse
import io
import json
import zipfile
from html import escape
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SAMPLE = ROOT / "crates/omnipotent-serve/sample"


def book_bytes():
    try:
        catalog = json.loads((SAMPLE / "catalog.json").read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError("cannot read the sample catalog") from error
    publication = catalog["publications"][0]
    metadata = publication["metadata"]
    title = escape(metadata["title"])
    identifier = escape(metadata["identifier"])
    author = escape(metadata["author"][0]["name"])
    paragraphs = "\n".join(
        f"<p>Connection check {number}: this book was downloaded from Omnipotent. "
        "The catalog and EPUB can be served from internal flash through the same "
        "portable HTTP core used on Linux. This is original sample text, not "
        "Wikipedia content or a demonstration of search or model quality.</p>"
        for number in range(1, 51)
    )
    files = {
        "mimetype": "application/epub+zip",
        "META-INF/container.xml": '''<?xml version="1.0" encoding="utf-8"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
<rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>''',
        "EPUB/package.opf": f'''<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:identifier id="book-id">{identifier}</dc:identifier><dc:title>{title}</dc:title>
<dc:language>en</dc:language><dc:creator>{author}</dc:creator>
<meta property="dcterms:modified">2026-11-17T00:00:00Z</meta>
</metadata><manifest>
<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
<item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
</manifest><spine><itemref idref="chapter"/></spine></package>''',
        "EPUB/nav.xhtml": f'''<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en">
<head><title>Contents</title></head><body><nav epub:type="toc" id="toc">
<h1>Contents</h1><ol><li><a href="chapter.xhtml">{title}</a></li></ol>
</nav></body></html>''',
        "EPUB/chapter.xhtml": f'''<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" lang="en"><head><title>{title}</title></head>
<body><h1>{title}</h1><p>If you can read this on your Kindle, catalog browsing,
book downloading, and local opening are working.</p>{paragraphs}</body></html>''',
    }
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        for name, text in files.items():
            entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_STORED
            entry.external_attr = 0o644 << 16
            archive.writestr(entry, text.encode("utf-8"))
    return output.getvalue()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify without rewriting the EPUB")
    args = parser.parse_args()
    path = SAMPLE / "connection-check.epub"
    expected = book_bytes()
    if args.check:
        if not path.exists() or path.read_bytes() != expected:
            raise SystemExit("Sample EPUB is stale; run python scripts/prepare-sample-book.py")
    else:
        path.write_bytes(expected)
    print(f"Sample EPUB {'verified' if args.check else 'prepared'}: {len(expected)} bytes")


if __name__ == "__main__":
    main()
