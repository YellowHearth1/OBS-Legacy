#!/usr/bin/env python3
"""Іконка і банер OBS-Legacy: мінімалістично, з готових складників.

    python3 dev/make-assets.py

Складники (нічого не намальовано з нуля):
  - іконка відеокарти «graphics-card» (bold) з набору Phosphor Icons, MIT —
    assets/third-party/phosphor-graphics-card-bold.svg, ліцензія поруч;
  - напис «580xx» (гілка легасі-драйверів NVIDIA) і назва — шрифтом JetBrains Mono (SIL OFL 1.1),
    перетвореним на криві, тож картинки не залежать від шрифтів у системі.
Логотипів NVIDIA й OBS немає: це торгові марки їхніх власників.

Результат:
  assets/obs-legacy.svg  іконка програми (ярлик у меню, install.sh);
  assets/banner.png      1280×640: шапка README і прев'ю репозиторію на GitHub
                         (Settings → General → Social preview).

Потрібні fontTools (pacman -S python-fonttools), rsvg-convert (librsvg) і шрифт JetBrains Mono.
"""

import re
import subprocess
import sys
import tempfile
from pathlib import Path

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent
GPU_SVG = ROOT / "assets" / "third-party" / "phosphor-graphics-card-bold.svg"
# Межі контуру відеокарти в його viewBox 0 0 256 256.
GPU_BOX = (4, 44, 252, 220)

BG = "#121613"
GREEN = "#76b900"
WHITE = "#f2f5f0"
LIGHT = "#c9d1c7"
MUTED = "#8e9891"


def find_font(style):
    """Файл JetBrains Mono потрібного накреслення (ExtraBold, Regular…)."""
    for family in ("JetBrains Mono", "JetBrainsMono Nerd Font", "JetBrainsMono Nerd Font Mono"):
        out = subprocess.run(
            ["fc-match", "-f", "%{file}\n%{family}", f"{family}:style={style}"],
            capture_output=True, text=True, check=False,
        ).stdout.splitlines()
        if len(out) >= 2 and "JetBrains" in out[1] and Path(out[0]).exists():
            return out[0]
    sys.exit(f"Не знайдено шрифт JetBrains Mono {style}: встанови ttf-jetbrains-mono.")


class Font:
    def __init__(self, path):
        self.tt = TTFont(path)
        self.glyphs = self.tt.getGlyphSet()
        self.cmap = self.tt.getBestCmap()
        self.upm = self.tt["head"].unitsPerEm
        self.hmtx = self.tt["hmtx"]

    def width(self, text, size):
        return sum(self.hmtx[self.cmap.get(ord(c), ".notdef")][0] for c in text) * size / self.upm

    def path(self, text, x, baseline, size):
        """Шлях SVG для тексту: x — лівий край, baseline — базова лінія."""
        scale = size / self.upm
        pen = SVGPathPen(self.glyphs)
        cursor = 0
        for c in text:
            name = self.cmap.get(ord(c), ".notdef")
            self.glyphs[name].draw(TransformPen(pen, (scale, 0, 0, -scale, x + cursor * scale, baseline)))
            cursor += self.hmtx[name][0]
        return pen.getCommands()


def gpu(x, y, width, color):
    """Відеокарта Phosphor: лівий верхній кут контуру в (x, y), ширина width."""
    d = re.search(r'\bd="([^"]+)"', GPU_SVG.read_text(encoding="utf-8")).group(1)
    left, top, right, _ = GPU_BOX
    k = width / (right - left)
    return f'  <g transform="translate({x - left * k:.2f} {y - top * k:.2f}) scale({k:.4f})" fill="{color}"><path d="{d}"/></g>\n'


def make_icon(bold):
    size = 58
    text_w = bold.width("580xx", size)
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256">\n'
        "  <!-- Іконка OBS-Legacy (dev/make-assets.py): відеокарта «graphics-card» з Phosphor Icons\n"
        "       (MIT, Copyright (c) 2023 Phosphor Icons — assets/third-party/LICENSE-phosphor.txt) і напис\n"
        "       «580xx» шрифтом JetBrains Mono (SIL OFL 1.1). Логотипів NVIDIA й OBS немає. -->\n"
        f'  <rect width="256" height="256" rx="48" fill="{BG}"/>\n'
        + gpu(53, 40, 150, GREEN)
        + f'  <path d="{bold.path("580xx", (256 - text_w) / 2, 214, size)}" fill="{GREEN}"/>\n'
        + "</svg>\n"
    )


def make_banner(bold, regular):
    w, h = 1280, 640
    x = 440
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}">\n',
        f'  <rect width="{w}" height="{h}" fill="{BG}"/>\n',
        gpu(120, 232, 248, GREEN),
    ]
    for font, text, size, baseline, color in [
        (bold, "OBS-Legacy", 92, 290, WHITE),
        (regular, "OBS Studio with working NVENC", 36, 350, LIGHT),
        (bold, "NVIDIA 580xx legacy drivers", 36, 400, GREEN),
    ]:
        parts.append(f'  <path d="{font.path(text, x, baseline, size)}" fill="{color}"/>\n')
    note = "Made with Claude Code  ·  inspired by sl5net/build-obs-nvenc-legacy"
    parts.append(f'  <path d="{regular.path(note, 120, 586, 18)}" fill="{MUTED}"/>\n')
    parts.append("</svg>\n")
    return "".join(parts)


def main():
    bold = Font(find_font("ExtraBold"))
    regular = Font(find_font("Regular"))
    icon = ROOT / "assets" / "obs-legacy.svg"
    icon.write_text(make_icon(bold), encoding="utf-8")
    print(icon.relative_to(ROOT))
    with tempfile.TemporaryDirectory() as tmp:
        svg = Path(tmp) / "banner.svg"
        svg.write_text(make_banner(bold, regular), encoding="utf-8")
        png = ROOT / "assets" / "banner.png"
        subprocess.run(["rsvg-convert", "-w", "1280", "-h", "640", "-o", str(png), str(svg)], check=True)
        print(png.relative_to(ROOT))


if __name__ == "__main__":
    main()
