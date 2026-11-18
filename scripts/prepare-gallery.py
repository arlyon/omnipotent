#!/usr/bin/env python3
"""Create lightweight gallery previews without changing the source artwork."""
from pathlib import Path

from PIL import Image, ImageOps

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "docs/assets/field-archive"
OUTPUT = ROOT / "assets/gallery"


def main():
    OUTPUT.mkdir(parents=True, exist_ok=True)
    for directory in (SOURCE / "stock/photos", SOURCE / "transparent/cutouts"):
        for path in sorted(directory.glob("*.png")):
            with Image.open(path) as source:
                preview = ImageOps.exif_transpose(source)
                preview.thumbnail((960, 960), Image.Resampling.LANCZOS)
                preview.save(OUTPUT / f"{path.stem}.webp", "WEBP", quality=85, method=6)
                print(f"{path.stem}: {preview.width} × {preview.height}")


if __name__ == "__main__":
    main()
