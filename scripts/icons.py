#!/usr/bin/env python3
"""Rasterise the icon masters into the icon set Tauri bundles.

One master feeds this: icons/icon.svg, the Material Icon Theme's chromatic
icon. It is drawn on a 16-unit grid, so it needs no separate small-size
drawing.

SVG is rendered by headless Chrome (or Edge) because it is the only rasteriser
a stock Windows box is sure to have; everything after that is Pillow.
"""

import io
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

ICONS = Path(__file__).resolve().parent.parent / "crates" / "thread-app" / "icons"

# The master is rendered once at 2048 and downsampled. Rendering each target
# size directly would be sharper in principle, but Chrome's own downscaling is
# worse than Lanczos on the finished bitmap.
MASTER_PX = 2048

# name -> pixel size
TARGETS = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 512,
}

ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]

BROWSERS = [
    os.environ.get("CHROME"),
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    shutil.which("chrome"),
    shutil.which("chromium"),
    shutil.which("google-chrome"),
]


def browser() -> str:
    for candidate in BROWSERS:
        if candidate and Path(candidate).exists():
            return candidate
    sys.exit("no Chrome or Edge found -- set CHROME=/path/to/chrome.exe")


def render(svg: Path, work: Path) -> Image.Image:
    """Screenshot one SVG at MASTER_PX square, on transparency."""
    # Chrome screenshots a page, not a file, and a bare SVG lands inside a
    # document with a default margin. The wrapper pins it to the corner at a
    # known size so the shot is exactly the artwork.
    page = work / (svg.stem + ".html")
    page.write_text(
        "<style>html,body{margin:0;padding:0;background:transparent}</style>"
        f'<img src="{svg.name}" width="{MASTER_PX}" height="{MASTER_PX}">',
        encoding="utf-8",
    )
    shutil.copy(svg, work / svg.name)
    out = work / (svg.stem + ".png")
    subprocess.run(
        [
            browser(),
            "--headless",
            "--disable-gpu",
            "--hide-scrollbars",
            "--force-device-scale-factor=1",
            "--default-background-color=00000000",
            f"--screenshot={out}",
            f"--window-size={MASTER_PX},{MASTER_PX}",
            str(page),
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if not out.exists():
        sys.exit(f"{svg.name}: headless render produced nothing")
    shot = Image.open(out).convert("RGBA")
    # Chrome renders a malformed SVG as a broken-image placeholder rather than
    # failing, which otherwise ships as a set of blank icons. The placeholder
    # is a small glyph in one corner; the artwork spans most of the canvas.
    # (The centre is no use as a probe: the mark is see-through there.)
    box = shot.getchannel("A").getbbox()
    if box is None or box[2] - box[0] < MASTER_PX // 2:
        sys.exit(f"{svg.name}: rendered blank -- is the SVG well formed?")
    return shot


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        master = render(ICONS / "icon.svg", work)

    def at(size: int) -> Image.Image:
        return master.resize((size, size), Image.LANCZOS)

    for name, size in sorted(TARGETS.items(), key=lambda kv: kv[1]):
        at(size).save(ICONS / name)
        print(f"  {name} ({size}px)")

    # Saved from the largest frame: Pillow drops any requested size bigger
    # than the image it is called on, so starting at 16 would write a
    # single-frame .ico and Windows would upscale that everywhere.
    ico = [at(s) for s in ICO_SIZES]
    ico[-1].save(
        ICONS / "icon.ico",
        sizes=[(s, s) for s in ICO_SIZES],
        append_images=ico,
    )
    print(f"  icon.ico ({', '.join(str(s) for s in ICO_SIZES)})")


if __name__ == "__main__":
    main()
