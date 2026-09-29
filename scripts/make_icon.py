"""Draw the Lythara Convert icon using curves, polar geometry and Pillow."""
from math import cos, sin, pi
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter
import numpy as np

SIZE = 1024
ROOT = Path(__file__).resolve().parents[1]
assets = ROOT / "assets"
assets.mkdir(exist_ok=True)

y, x = np.mgrid[0:SIZE, 0:SIZE]
radial = np.clip(np.sqrt((x - 340) ** 2 + (y - 240) ** 2) / 1130, 0, 1)
base = np.empty((SIZE, SIZE, 4), dtype=np.uint8)
for c, (near, far) in enumerate(zip((60, 76, 170), (19, 28, 65))):
    base[:, :, c] = (near * (1 - radial) + far * radial).astype(np.uint8)
base[:, :, 3] = 255
mask = Image.new("L", (SIZE, SIZE))
ImageDraw.Draw(mask).rounded_rectangle((8, 8, SIZE - 8, SIZE - 8), radius=230, fill=255)
image = Image.fromarray(base, "RGBA")
image.putalpha(mask)


def bezier(a, b, c, d, samples=110):
    points = []
    for i in range(samples + 1):
        t = i / samples
        u = 1 - t
        points.append((u ** 3 * a[0] + 3 * u * u * t * b[0] + 3 * u * t * t * c[0] + t ** 3 * d[0],
                       u ** 3 * a[1] + 3 * u * u * t * b[1] + 3 * u * t * t * c[1] + t ** 3 * d[1]))
    return points


def arc(radius, first, last):
    return [(512 + radius * cos(a * pi / 180), 512 + radius * sin(a * pi / 180))
            for a in np.linspace(first, last, 290)]


def ribbon(draw, points, width, fill):
    left, right = [], []
    for i, (px, py) in enumerate(points):
        a = points[max(i - 1, 0)]
        b = points[min(i + 1, len(points) - 1)]
        dx, dy = b[0] - a[0], b[1] - a[1]
        length = max(.001, (dx * dx + dy * dy) ** .5)
        nx, ny = -dy / length * width / 2, dx / length * width / 2
        left.append((px + nx, py + ny))
        right.append((px - nx, py - ny))
    draw.polygon(left + list(reversed(right)), fill=fill)
    radius = width / 2
    for px, py in (points[0], points[-1]):
        draw.ellipse((px-radius, py-radius, px+radius, py+radius), fill=fill)


glow = Image.new("RGBA", image.size)
gd = ImageDraw.Draw(glow)
ribbon(gd, arc(315, 28, 302), 96, (132, 169, 255, 120))
image = Image.alpha_composite(image, glow.filter(ImageFilter.GaussianBlur(50)))
line = Image.new("RGBA", image.size)
d = ImageDraw.Draw(line)
ribbon(d, arc(315, 26, 294), 32, (139, 173, 255, 86))
ribbon(d, arc(315, 48, 287), 14, (162, 193, 255, 180))

# Two ribbons cross like a media stream changing direction; the lower stroke
# also forms a simple L in small tray sizes.
upper = bezier((309, 395), (377, 292), (486, 286), (574, 359)) + bezier((574, 359), (646, 420), (652, 488), (713, 505))
lower = bezier((306, 611), (395, 716), (525, 720), (604, 642)) + bezier((604, 642), (662, 584), (665, 533), (714, 504))
ribbon(d, upper, 78, (245, 248, 255, 255))
ribbon(d, lower, 78, (133, 240, 220, 255))
d.ellipse((274, 356, 352, 434), fill=(245, 248, 255, 255))
d.ellipse((266, 572, 344, 650), fill=(133, 240, 220, 255))
d.ellipse((680, 472, 748, 540), fill=(245, 248, 255, 255))
image = Image.alpha_composite(image, line)
image.resize((512, 512), Image.Resampling.LANCZOS).save(assets / "icon.png")
image.save(assets / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print(assets / "icon.png")
