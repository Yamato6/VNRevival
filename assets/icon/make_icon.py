"""Draws VNRevival's application icon and banner and writes every format they ship in.

The icon is a red hanko seal bearing 蘇 ("revive"), with the "next" marker
of a visual novel message window in its corner, on a paper-coloured
rounded square. The kanji is drawn with Noto Serif JP (SIL Open Font
License), which must be installed or given with --serif; the banner's
text uses Noto Sans JP the same way (--sans).

    python assets/icon/make_icon.py

Writes, next to this script: vnrevival.png (1024 px), vnrevival-256.png
(the SDL window icon, embedded in the executable), vnrevival.ico (Windows)
and vnrevival.icns (macOS); and ../banner.png (1280 x 640, for the README
and the repository's social preview).
"""

import argparse
import os
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SIZE = 1024
# Drawn larger and scaled down, for smooth edges.
SUPER = 4
OUT = Path(__file__).resolve().parent

INK = (27, 29, 54)
PAPER = (246, 239, 226)
RED = (214, 64, 46)
MUTED = (96, 92, 110)


def find_font(name, given):
    if given:
        return given
    dirs = [
        Path(os.environ.get("WINDIR", "C:/Windows")) / "Fonts",
        Path(os.environ.get("LOCALAPPDATA", "")) / "Microsoft/Windows/Fonts",
        Path.home() / "Library/Fonts",
        Path("/Library/Fonts"),
        Path.home() / ".local/share/fonts",
        Path("/usr/share/fonts"),
    ]
    for d in dirs:
        if d.is_dir():
            for path in d.rglob(f"{name}*"):
                if path.suffix.lower() in (".ttf", ".otf"):
                    return str(path)
    sys.exit(f"{name} not found: install it from Google Fonts or pass its file")


def font(path, px, weight):
    f = ImageFont.truetype(path, px)
    # Variable fonts (NotoSerifJP-VF.ttf) are set to the given weight.
    try:
        f.set_variation_by_axes([weight])
    except OSError:
        pass
    return f


def centred(d, text, cx, cy, f, fill):
    left, top, right, bottom = d.textbbox((0, 0), text, font=f)
    d.text((cx - (left + right) / 2, cy - (top + bottom) / 2), text, font=f, fill=fill)


def draw(size, serif, background=True):
    s = size * SUPER
    k = s / SIZE

    def box(*values):
        return [round(v * k) for v in values]

    icon = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(icon)
    if background:
        d.rounded_rectangle(box(40, 40, 984, 984), radius=round(210 * k), fill=PAPER)
    # The seal.
    d.rounded_rectangle(box(170, 170, 854, 854), radius=round(90 * k), fill=RED)
    # Its inner frame, left out where it would only blur.
    if size >= 48:
        d.rounded_rectangle(box(215, 215, 809, 809), radius=round(60 * k), outline=PAPER, width=round(22 * k))
    centred(d, "蘇", 512 * k, 496 * k, font(serif, round(450 * k), 900), PAPER)
    # The message window's "next" marker.
    d.polygon([tuple(box(694, 708)), tuple(box(762, 708)), tuple(box(728, 758))], fill=PAPER)
    return icon.resize((size, size), Image.LANCZOS)


def banner(serif, sans):
    w, h = 1280, 640
    s = SUPER
    image = Image.new("RGBA", (w * s, h * s), PAPER + (255,))
    seal = draw(400, serif, background=False)
    image.alpha_composite(seal.resize((400 * s, 400 * s), Image.LANCZOS), (90 * s, 120 * s))
    d = ImageDraw.Draw(image)
    d.text((520 * s, 200 * s), "VNRevival", font=font(sans, 120 * s, 800), fill=INK)
    tagline = font(sans, 38 * s, 500)
    d.text((526 * s, 360 * s), "Old visual novels, revived", font=tagline, fill=INK)
    d.text((526 * s, 418 * s), "on Windows, macOS and Linux", font=tagline, fill=MUTED)
    return image.resize((w, h), Image.LANCZOS).convert("RGB")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--serif", help="Noto Serif JP font file")
    parser.add_argument("--sans", help="Noto Sans JP font file")
    args = parser.parse_args()
    serif = find_font("NotoSerifJP", args.serif)
    sans = find_font("NotoSansJP", args.sans)

    big = draw(SIZE, serif)
    big.save(OUT / "vnrevival.png")
    draw(256, serif).save(OUT / "vnrevival-256.png")
    # Small sizes are drawn on their own rather than scaled from 1024.
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    images = [draw(n, serif) for n in ico_sizes]
    images[-1].save(OUT / "vnrevival.ico", sizes=[(n, n) for n in ico_sizes], append_images=images[:-1])
    big.save(OUT / "vnrevival.icns")
    banner(serif, sans).save(OUT.parent / "banner.png", optimize=True)


if __name__ == "__main__":
    main()
