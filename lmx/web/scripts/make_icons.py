"""Generate lmx PWA icons (any + maskable) with Pillow. Run from lmx/web: python scripts/make_icons.py"""
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "public" / "icons"
OUT.mkdir(parents=True, exist_ok=True)
BG_TOP, BG_BOTTOM = (40, 30, 72), (16, 18, 32)
ACCENT, ACCENT2 = (167, 139, 250), (244, 114, 182)


def gradient(size: int) -> Image.Image:
    img = Image.new("RGB", (size, size))
    d = ImageDraw.Draw(img)
    for y in range(size):
        t = y / (size - 1)
        d.line([(0, y), (size, y)], fill=tuple(int(a + (b - a) * t) for a, b in zip(BG_TOP, BG_BOTTOM)))
    return img


def glyph(img: Image.Image, scale: float) -> None:
    """Three stacked cards (a model library) centred in `scale` of the canvas."""
    s = img.size[0]
    d = ImageDraw.Draw(img)
    box = s * scale
    x0, y0 = (s - box) / 2, (s - box) / 2
    w, h = box * 0.62, box * 0.78
    r = box * 0.08
    for i, col in enumerate([(90, 80, 140), ACCENT2, ACCENT]):
        off = box * 0.12 * i
        d.rounded_rectangle([x0 + off, y0 + box * 0.22 - off * 0.6, x0 + off + w, y0 + box * 0.22 - off * 0.6 + h],
                            radius=r, fill=col)
    # an "L" on the front card
    fx, fy = x0 + box * 0.24, y0 + box * 0.22 - box * 0.144
    lw = box * 0.07
    d.rectangle([fx + w * 0.22, fy + h * 0.2, fx + w * 0.22 + lw, fy + h * 0.78], fill=(20, 18, 36))
    d.rectangle([fx + w * 0.22, fy + h * 0.78 - lw, fx + w * 0.72, fy + h * 0.78], fill=(20, 18, 36))


def rounded(img: Image.Image, radius_ratio: float) -> Image.Image:
    s = img.size[0]
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, s - 1, s - 1], radius=int(s * radius_ratio), fill=255)
    out = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    out.paste(img, (0, 0), mask)
    return out


for size in (192, 512):
    any_icon = gradient(size)
    glyph(any_icon, 0.78)
    rounded(any_icon, 0.22).save(OUT / f"icon-{size}.png")
    maskable = gradient(size)  # full bleed; glyph inside the 80% safe zone
    glyph(maskable, 0.56)
    maskable.save(OUT / f"maskable-{size}.png")

apple = gradient(180)  # iOS: opaque, square corners (iOS applies its own mask)
glyph(apple, 0.7)
apple.save(OUT / "apple-touch-icon-180.png")
print("icons written to", OUT)
