#!/usr/bin/env python3
"""Generate the PNG assets of the WeChat-oriented fcitx5 theme.

WeChat's embedded candidate window (fcitx5-qt's client-side panel) has two limitations:
1. It does not support SVG icons (missing Qt SVG plugin).
2. Pure color background with BorderWidth creates a 5x5 pixmap in fillBackground().
   When upscaled under fractional scaling (e.g. 1.6x) with QPainter::SmoothPixmapTransform,
   bilinear filtering causes the 2-3px blue border to bleed across the 1px white center,
   producing an unwanted blue gradient wash across the entire panel.

To eliminate the gradient and achieve the crisp, square-corner, solid-white look of the
original system theme:
- panel.png: 64x64 square panel with 3px #3daee9 border and pure white center.
  With 16px 9-patch margin, the stretched center only samples pure white.
- highlight.png: 32x32 solid #3daee9 rectangle.
- SVG icons (prev, next, radio, arrow) are rendered into PNGs using rsvg-convert.
"""
import subprocess
import sys
from pathlib import Path
from PIL import Image

BLUE = (0x3D, 0xAE, 0xE9, 255)
WHITE = (255, 255, 255, 255)


def make_panel(out: Path, border: int = 2):
    """64x64 nine-patch panel with border px blue border and 100% white interior."""
    img = Image.new("RGBA", (64, 64), WHITE)
    for y in range(64):
        for x in range(64):
            if x < border or x >= 64 - border or y < border or y >= 64 - border:
                img.putpixel((x, y), BLUE)
    img.save(out / "panel.png")


def make_highlight(out: Path):
    """32x32 solid blue highlight rectangle."""
    img = Image.new("RGBA", (32, 32), BLUE)
    img.save(out / "highlight.png")


def convert_svgs(out: Path):
    """Convert default theme SVGs to pixel-perfect PNGs."""
    for name in ["prev", "next", "radio", "arrow"]:
        svg_path = Path(f"/usr/share/fcitx5/themes/default/{name}.svg")
        if svg_path.exists():
            subprocess.run(["rsvg-convert", str(svg_path), "-o", str(out / f"{name}.png")], check=True)


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "wechat-default")
    out.mkdir(parents=True, exist_ok=True)
    make_panel(out)
    make_highlight(out)
    convert_svgs(out)
    print(f"Generated clean assets in {out}")


if __name__ == "__main__":
    main()
