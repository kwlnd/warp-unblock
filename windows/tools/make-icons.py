"""Генерирует иконки WARP Unblock: app.ico (exe), app.png (окно), tray-mask.png (трей).

Запуск: python -I windows/tools/make-icons.py
Результат кладётся в windows/source/assets/icons/.
"""
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "source" / "assets" / "icons"
ORANGE = (246, 130, 31, 255)
WHITE = (255, 255, 255, 255)
S = 1024  # рисуем крупно, потом уменьшаем со сглаживанием


def bezier(p0, p1, p2, p3, n=24):
    pts = []
    for i in range(n + 1):
        t = i / n
        u = 1 - t
        pts.append((
            u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0],
            u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1],
        ))
    return pts


def shield(cx, top, w, h):
    """Контур щита: плоские плечи, дуга к острому низу."""
    r, b = cx + w / 2, top + h
    shoulder = top + h * 0.16
    side = top + h * 0.52
    right = [(cx, top)] + bezier((cx, top), (cx + w * 0.18, top + h * 0.1), (r - w * 0.12, shoulder), (r, shoulder))[1:]
    right += [(r, side)]
    right += bezier((r, side), (r, top + h * 0.8), (cx + w * 0.22, top + h * 0.93), (cx, b))[1:]
    left = [(2 * cx - x, y) for x, y in reversed(right[1:-1])]
    return right + left


def check(draw, cx, cy, size, color, width):
    pts = [(cx - size * 0.42, cy + size * 0.02), (cx - size * 0.1, cy + size * 0.32), (cx + size * 0.45, cy - size * 0.3)]
    draw.line(pts, fill=color, width=width, joint="curve")
    for x, y in (pts[0], pts[-1]):
        draw.ellipse((x - width / 2, y - width / 2, x + width / 2, y + width / 2), fill=color)


def app_icon():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((32, 32, S - 32, S - 32), radius=220, fill=ORANGE)
    d.polygon(shield(S / 2, S * 0.19, S * 0.56, S * 0.64), fill=WHITE)
    check(d, S / 2, S * 0.48, S * 0.3, ORANGE, int(S * 0.065))
    return img


def tray_mask():
    """Белый щит на прозрачном фоне; программа красит его в цвет состояния."""
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.polygon(shield(S / 2, S * 0.04, S * 0.84, S * 0.92), fill=WHITE)
    # Галочка прорезана насквозь, чтобы щит читался и в 16 px.
    check(d, S / 2, S * 0.46, S * 0.42, (0, 0, 0, 0), int(S * 0.11))
    return img


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    app = app_icon()
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    app.save(OUT / "app.ico", sizes=[(s, s) for s in sizes])
    app.resize((256, 256), Image.Resampling.LANCZOS).save(OUT / "app.png")
    tray_mask().resize((32, 32), Image.Resampling.LANCZOS).save(OUT / "tray-mask.png")
    print("written to", OUT)


if __name__ == "__main__":
    main()
