"""Draws VNRevival's application icon and writes every format it ships in.

A sunrise (the "revival") over a visual novel message window, on a rounded
night-to-dawn square. Run from anywhere with Pillow installed:

    python assets/icon/make_icon.py

Writes, next to this script: vnrevival.png (1024 px), vnrevival-256.png
(the SDL window icon, embedded in the executable), vnrevival.ico (Windows)
and vnrevival.icns (macOS).
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

SIZE = 1024
# Drawn larger and scaled down, for smooth edges.
SUPER = 4
OUT = Path(__file__).resolve().parent


def lerp(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def draw(size):
    s = size * SUPER
    k = s / SIZE

    def box(x0, y0, x1, y1):
        return [round(x0 * k), round(y0 * k), round(x1 * k), round(y1 * k)]

    # Background: night blue at the top to dawn violet at the bottom.
    top, bottom = (24, 28, 68), (112, 52, 120)
    gradient = Image.new("RGB", (1, s))
    for y in range(s):
        gradient.putpixel((0, y), lerp(top, bottom, y / (s - 1)))
    background = gradient.resize((s, s))

    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle(box(40, 40, 984, 984), radius=round(200 * k), fill=255)
    icon = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    icon.paste(background, (0, 0), mask)

    layer = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    # The sun, rising behind the window, with a soft halo.
    for r, alpha in ((330, 40), (290, 70), (250, 255)):
        colour = (255, 196, 92, alpha) if alpha == 255 else (255, 170, 90, alpha)
        d.ellipse(box(512 - r, 560 - r, 512 + r, 560 + r), fill=colour)
    # Rays.
    ray = (255, 214, 130, 255)
    for x0, y0, x1, y1 in (
        (497, 150, 527, 250),
        (232, 300, 300, 360),
        (724, 360, 792, 300),
    ):
        d.line(box(x0, y0, x1, y1)[:2] + box(x0, y0, x1, y1)[2:], fill=ray, width=round(34 * k))
    # Keep the sun inside the rounded square.
    layer.putalpha(ImageChops.multiply(layer.getchannel("A"), mask))
    icon = Image.alpha_composite(icon, layer)

    # The message window: white, slightly see-through, with three lines.
    window = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    w = ImageDraw.Draw(window)
    w.rounded_rectangle(box(150, 560, 874, 860), radius=round(56 * k), fill=(250, 248, 255, 240))
    line = (70, 58, 120, 255)
    for y, x1 in ((640, 760), (710, 700), (780, 560)):
        w.rounded_rectangle(box(230, y - 18, x1, y + 18), radius=round(18 * k), fill=line)
    # The "next" marker at the end of the last line.
    w.polygon([tuple(box(610, 762, 0, 0)[:2]), tuple(box(660, 762, 0, 0)[:2]), tuple(box(635, 800, 0, 0)[:2])], fill=(232, 120, 70, 255))
    icon = Image.alpha_composite(icon, window)

    return icon.resize((size, size), Image.LANCZOS)


def main():
    big = draw(SIZE)
    big.save(OUT / "vnrevival.png")
    draw(256).save(OUT / "vnrevival-256.png")
    # Small sizes are drawn on their own rather than scaled from 1024.
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    images = [draw(n) for n in ico_sizes]
    images[-1].save(OUT / "vnrevival.ico", sizes=[(n, n) for n in ico_sizes], append_images=images[:-1])
    big.save(OUT / "vnrevival.icns")


if __name__ == "__main__":
    main()
