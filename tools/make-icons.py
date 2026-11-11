"""Иконки WARP Unblock из твоих SVG в корне репозитория.

Знак «!W» — из `warp-w-orange.svg` (берутся фигуры со всеми трансформациями, плитка-rect
отбрасывается), одна W — из `warp-w.svg`. Минимализм: знак без подложки и эффектов, цветом
обозначается состояние (трей, шторка, шапка). Где знак совсем мелкий (строка состояния,
плитка в быстрых настройках), «!» превращается в кляксу — там одна W.
Запуск: python -I tools/make-icons.py

Windows (windows/source/assets/icons/):
  app.ico, app.png — оранжевый «!W» на прозрачном фоне (exe, окно);
  tray-mask.png    — белый «!W», программа красит его в цвет состояния;
  warp-mark.svg    — «!W» для шапки окна, тоже красится по состоянию.
Android (android/app/src/main/res/):
  drawable/ic_foreground.xml — «!W» для адаптивной иконки (фон — @color/icon_bg);
  drawable/ic_mark.xml       — одноцветный «!W» для тематических иконок Android 13+;
  drawable/ic_logo.xml       — «!W» для шапки приложения (цвет — по состоянию);
  drawable/ic_notify.xml     — W для строки состояния и шторки;
  drawable/ic_tile.xml       — W для плитки в быстрых настройках;
  mipmap-*/ic_launcher*.png  — для лаунчеров без адаптивных иконок.
"""
import math
import re
import xml.etree.ElementTree as ET
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
MARK = ROOT / "warp-w-orange.svg"
LETTER = ROOT / "warp-w.svg"
WIN = ROOT / "windows" / "source" / "assets" / "icons"
RES = ROOT / "android" / "app" / "src" / "main" / "res"
ORANGE = "#FD8119"
GRAPHITE = "#171B23"
NS = "{http://www.w3.org/2000/svg}"


def multiply(a, b):
    """Аффинные матрицы (a, b, c, d, e, f) как в SVG: точка → (a·x + c·y + e, b·x + d·y + f)."""
    return (
        a[0] * b[0] + a[2] * b[1], a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3], a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4], a[1] * b[4] + a[3] * b[5] + a[5],
    )


def parse_transform(text):
    m = (1, 0, 0, 1, 0, 0)
    for name, args in re.findall(r"(\w+)\(([^)]*)\)", text or ""):
        v = [float(x) for x in re.split(r"[\s,]+", args.strip()) if x]
        if name == "translate":
            t = (1, 0, 0, 1, v[0], v[1] if len(v) > 1 else 0)
        elif name == "scale":
            t = (v[0], 0, 0, v[1] if len(v) > 1 else v[0], 0, 0)
        elif name == "rotate":
            r = math.radians(v[0])
            cx, cy = (v[1], v[2]) if len(v) == 3 else (0, 0)
            cos, sin = math.cos(r), math.sin(r)
            t = (cos, sin, -sin, cos, cx - cos * cx + sin * cy, cy - sin * cx - cos * cy)
        else:
            raise ValueError(f"неподдержанная трансформация {name}")
        m = multiply(m, t)
    return m


def contour(d, steps=16):
    """Контур из команд M, H, L, Q, Z в виде списка точек."""
    tokens = re.findall(r"[MHLQZ]|-?\d+(?:\.\d+)?", d)
    pts, cmd, i, cur = [], None, 0, (0.0, 0.0)
    while i < len(tokens):
        if tokens[i] in "MHLQZ":
            cmd = tokens[i]
            i += 1
            if cmd == "Z":
                continue
        num = lambda k: float(tokens[i + k])
        if cmd in ("M", "L"):
            cur = (num(0), num(1))
            pts.append(cur)
            i += 2
        elif cmd == "H":
            cur = (num(0), cur[1])
            pts.append(cur)
            i += 1
        elif cmd == "Q":
            (cx, cy), (ex, ey), (sx, sy) = (num(0), num(1)), (num(2), num(3)), cur
            for s in range(1, steps + 1):
                t = s / steps
                u = 1 - t
                pts.append((u * u * sx + 2 * u * t * cx + t * t * ex, u * u * sy + 2 * u * t * cy + t * t * ey))
            cur = (ex, ey)
            i += 4
        else:
            raise ValueError(f"неподдержанная команда {cmd}")
    return pts


def shapes(source):
    """Все фигуры файла в координатах SVG; плитка (rect) не входит."""
    result = []

    def walk(node, m):
        m = multiply(m, parse_transform(node.get("transform")))
        if node.tag == NS + "path":
            result.append([(m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]) for x, y in contour(node.get("d", ""))])
        for child in node:
            walk(child, m)

    walk(ET.parse(source).getroot(), (1, 0, 0, 1, 0, 0))
    return result


def fitted(source, size, share):
    """Фигуры по центру квадрата size×size; большая сторона знака — доля share."""
    polys = shapes(source)
    xs = [x for p in polys for x, _ in p]
    ys = [y for p in polys for _, y in p]
    w, h = max(xs) - min(xs), max(ys) - min(ys)
    k = size * share / max(w, h)
    ox = (size - w * k) / 2 - min(xs) * k
    oy = (size - h * k) / 2 - min(ys) * k
    return [[(ox + x * k, oy + y * k) for x, y in p] for p in polys]


def raster(source, size, fill, share, background=None, radius=0.0, circle=False):
    """Рисуем в 8× и уменьшаем со сглаживанием."""
    big = size * 8
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    if background:
        if circle:
            draw.ellipse((0, 0, big - 1, big - 1), fill=background)
        else:
            draw.rounded_rectangle((0, 0, big - 1, big - 1), radius=int(big * radius), fill=background)
    for poly in fitted(source, big, share):
        draw.polygon(poly, fill=fill)
    return img.resize((size, size), Image.Resampling.LANCZOS)


def path_data(source, size, share):
    return " ".join(
        "M" + " L".join(f"{x:.2f} {y:.2f}" for x, y in poly) + " Z" for poly in fitted(source, size, share)
    )


def vector(source, viewport, share, color, comment):
    size = f"{viewport}dp"
    return f"""<?xml version="1.0" encoding="utf-8"?>
<!-- {comment} Сгенерировано tools/make-icons.py из {source.name} — не править руками. -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="{size}" android:height="{size}"
    android:viewportWidth="{viewport}" android:viewportHeight="{viewport}">
    <path android:fillColor="{color}" android:pathData="{path_data(source, viewport, share)}" />
</vector>
"""


def main():
    WIN.mkdir(parents=True, exist_ok=True)
    app = raster(MARK, 256, ORANGE, 0.96)
    app.save(WIN / "app.ico", sizes=[(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)])
    app.save(WIN / "app.png")
    raster(MARK, 32, "#FFFFFF", 1.0).save(WIN / "tray-mask.png")
    (WIN / "warp-mark.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><path fill="#FFFFFF" d="{path_data(MARK, 64, 1.0)}"/></svg>\n',
        encoding="utf-8",
    )

    # Адаптивная иконка: 108 dp, безопасный круг 66 dp — знак шириной 58 dp (с диагональю влезает).
    drawable = RES / "drawable"
    (drawable / "ic_foreground.xml").write_text(vector(MARK, 108, 58 / 108, ORANGE, "«!W» для адаптивной иконки (фон — @color/icon_bg)."), encoding="utf-8")
    (drawable / "ic_mark.xml").write_text(vector(MARK, 108, 58 / 108, "#FFFFFFFF", "Одноцветный «!W» для тематических иконок Android 13+."), encoding="utf-8")
    (drawable / "ic_logo.xml").write_text(vector(MARK, 24, 1.0, "#FFFFFFFF", "«!W» для шапки приложения; цвет задаёт код."), encoding="utf-8")
    (drawable / "ic_notify.xml").write_text(vector(LETTER, 24, 22 / 24, "#FFFFFFFF", "W для строки состояния и шторки."), encoding="utf-8")
    (drawable / "ic_tile.xml").write_text(vector(LETTER, 24, 20 / 24, "#FFFFFFFF", "W для плитки в быстрых настройках."), encoding="utf-8")
    for name, px in {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}.items():
        folder = RES / f"mipmap-{name}"
        folder.mkdir(parents=True, exist_ok=True)
        raster(MARK, px, ORANGE, 0.66, GRAPHITE, radius=0.22).save(folder / "ic_launcher.png")
        raster(MARK, px, ORANGE, 0.6, GRAPHITE, circle=True).save(folder / "ic_launcher_round.png")
    stale = WIN / "warp-w.svg"
    if stale.exists():
        stale.unlink()
    print("written to", WIN, "and", RES)


if __name__ == "__main__":
    main()
