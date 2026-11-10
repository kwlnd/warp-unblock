"""Переводит иконки Lucide (ISC) в Android vector drawable: res/drawable/ic_lucide_<name>.xml.

Запуск: python -I android/tools/lucide-to-vector.py <папка с lucide *.svg> <имя> [<имя> ...]
Иконки рисуются линией 2 dp со скруглёнными концами, цвет задаёт tint в Compose.
"""
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "app" / "src" / "main" / "res" / "drawable"


def num(v):
    return float(v or 0)


def element_path(el):
    tag = el.tag.split("}")[-1]
    a = el.attrib
    if tag == "path":
        return a["d"]
    if tag == "circle":
        cx, cy, r = num(a["cx"]), num(a["cy"]), num(a["r"])
        return f"M{cx - r},{cy}a{r},{r} 0 1,0 {2 * r},0a{r},{r} 0 1,0 {-2 * r},0"
    if tag == "line":
        return f"M{a['x1']},{a['y1']}L{a['x2']},{a['y2']}"
    if tag in ("polyline", "polygon"):
        pts = a["points"].replace(",", " ").split()
        pairs = [f"{pts[i]},{pts[i + 1]}" for i in range(0, len(pts), 2)]
        return "M" + "L".join(pairs) + ("Z" if tag == "polygon" else "")
    if tag == "rect":
        x, y, w, h = num(a.get("x")), num(a.get("y")), num(a["width"]), num(a["height"])
        r = num(a.get("rx") or a.get("ry"))
        if not r:
            return f"M{x},{y}h{w}v{h}h{-w}Z"
        return (f"M{x + r},{y}h{w - 2 * r}a{r},{r} 0 0 1 {r},{r}v{h - 2 * r}a{r},{r} 0 0 1 {-r},{r}"
                f"h{-(w - 2 * r)}a{r},{r} 0 0 1 {-r},{-r}v{-(h - 2 * r)}a{r},{r} 0 0 1 {r},{-r}Z")
    raise ValueError(f"unsupported element {tag}")


def convert(src: Path, name: str):
    root = ET.parse(src / f"{name}.svg").getroot()
    paths = [element_path(el) for el in root.iter() if el is not root]
    body = "\n".join(
        f'    <path android:pathData="{d}" android:fillColor="#00000000" android:strokeColor="#FF000000"\n'
        f'        android:strokeWidth="2" android:strokeLineCap="round" android:strokeLineJoin="round" />'
        for d in paths
    )
    xml = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        f"<!-- Lucide «{name}» (ISC), сгенерировано android/tools/lucide-to-vector.py -->\n"
        '<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
        '    android:width="24dp" android:height="24dp" android:viewportWidth="24" android:viewportHeight="24">\n'
        f"{body}\n</vector>\n"
    )
    (OUT / f"ic_lucide_{name.replace('-', '_')}.xml").write_text(xml, encoding="utf-8")


def main():
    src = Path(sys.argv[1])
    OUT.mkdir(parents=True, exist_ok=True)
    for name in sys.argv[2:]:
        convert(src, name)
        print("ok", name)


if __name__ == "__main__":
    main()
