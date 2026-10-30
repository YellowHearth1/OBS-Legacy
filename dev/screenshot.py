#!/usr/bin/env python3
"""Знімок екрана TUI у PNG з виводу `tmux capture-pane -e -p`.

Потрібен для скріншотів README (docs/screenshots/) без справжнього вікна:

    tmux new-session -d -s shot -x 120 -y 36 "env COLORTERM=truecolor obs-legacy"
    tmux capture-pane -t shot -e -p > /tmp/screen.ansi
    python3 dev/screenshot.py /tmp/screen.ansi docs/screenshots/home.png

Розуміє SGR-кольори (16, 256, truecolor), жирний шрифт і інверсію. Потрібні
Pillow і моноширинний шрифт (типово JetBrains Mono, інакше DejaVu Sans Mono).
"""

import re
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

# Типові кольори термінала там, де програма не задала свої (Catppuccin Mocha).
DEFAULT_FG = (0xCD, 0xD6, 0xF4)
DEFAULT_BG = (0x1E, 0x1E, 0x2E)
ANSI16 = [
    (0x45, 0x47, 0x5A), (0xF3, 0x8B, 0xA8), (0xA6, 0xE3, 0xA1), (0xF9, 0xE2, 0xAF),
    (0x89, 0xB4, 0xFA), (0xF5, 0xC2, 0xE7), (0x94, 0xE2, 0xD5), (0xBA, 0xC2, 0xDE),
    (0x58, 0x5B, 0x70), (0xF3, 0x8B, 0xA8), (0xA6, 0xE3, 0xA1), (0xF9, 0xE2, 0xAF),
    (0x89, 0xB4, 0xFA), (0xF5, 0xC2, 0xE7), (0x94, 0xE2, 0xD5), (0xA6, 0xAD, 0xC8),
]
FONTS = [
    ("/usr/share/fonts/TTF/JetBrainsMonoNLNerdFontMono-Regular.ttf",
     "/usr/share/fonts/TTF/JetBrainsMonoNLNerdFontMono-Bold.ttf"),
    ("/usr/share/fonts/TTF/DejaVuSansMono.ttf", "/usr/share/fonts/TTF/DejaVuSansMono-Bold.ttf"),
]


def color256(n):
    if n < 16:
        return ANSI16[n]
    if n < 232:
        n -= 16
        steps = [0, 95, 135, 175, 215, 255]
        return (steps[n // 36], steps[n // 6 % 6], steps[n % 6])
    v = 8 + (n - 232) * 10
    return (v, v, v)


def parse(text):
    """Рядки → список клітинок (символ, fg, bg, жирний).

    tmux пише лише зміни атрибутів, тож стан переходить і на наступні рядки.
    """
    rows = []
    fg, bg, bold, rev = None, None, False, False
    for raw in text.split("\n"):
        cells = []
        for part in re.split(r"(\x1b\[[0-9;:]*m)", raw):
            if part.startswith("\x1b["):
                codes = [int(c) if c else 0 for c in re.split(r"[;:]", part[2:-1])] or [0]
                i = 0
                while i < len(codes):
                    c = codes[i]
                    if c == 0:
                        fg, bg, bold, rev = None, None, False, False
                    elif c == 1:
                        bold = True
                    elif c == 22:
                        bold = False
                    elif c == 7:
                        rev = True
                    elif c == 27:
                        rev = False
                    elif 30 <= c <= 37:
                        fg = ANSI16[c - 30]
                    elif 90 <= c <= 97:
                        fg = ANSI16[c - 90 + 8]
                    elif 40 <= c <= 47:
                        bg = ANSI16[c - 40]
                    elif 100 <= c <= 107:
                        bg = ANSI16[c - 100 + 8]
                    elif c == 39:
                        fg = None
                    elif c == 49:
                        bg = None
                    elif c in (38, 48) and i + 1 < len(codes):
                        if codes[i + 1] == 2 and i + 4 < len(codes):
                            rgb = tuple(codes[i + 2:i + 5])
                            i += 4
                        elif codes[i + 1] == 5 and i + 2 < len(codes):
                            rgb = color256(codes[i + 2])
                            i += 2
                        else:
                            rgb = None
                        if c == 38:
                            fg = rgb
                        else:
                            bg = rgb
                    i += 1
            else:
                for ch in part:
                    f, b = fg or DEFAULT_FG, bg or DEFAULT_BG
                    if rev:
                        f, b = b, f
                    cells.append((ch, f, b, bold))
        rows.append(cells)
    while rows and not rows[-1]:
        rows.pop()
    return rows


def render(rows, out, size=15):
    regular, bold = next(((r, b) for r, b in FONTS if Path(r).exists()), (None, None))
    if regular is None:
        sys.exit("Не знайдено моноширинного шрифту (JetBrains Mono або DejaVu Sans Mono).")
    font, font_b = ImageFont.truetype(regular, size), ImageFont.truetype(bold, size)
    cw = round(font.getlength("M"))
    ch = round(size * 1.32)
    cols = max(len(r) for r in rows)
    pad = 12
    img = Image.new("RGB", (cols * cw + pad * 2, len(rows) * ch + pad * 2), DEFAULT_BG)
    d = ImageDraw.Draw(img)
    for y, row in enumerate(rows):
        for x, (c, f, b, is_bold) in enumerate(row):
            px, py = pad + x * cw, pad + y * ch
            d.rectangle([px, py, px + cw - 1, py + ch - 1], fill=b)
            if c == "█":
                d.rectangle([px, py, px + cw - 1, py + ch - 1], fill=f)
            elif c.strip():
                d.text((px, py + (ch - size) // 2 - 1), c, font=font_b if is_bold else font, fill=f)
    img.save(out)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    render(parse(Path(sys.argv[1]).read_text(encoding="utf-8")), sys.argv[2])
