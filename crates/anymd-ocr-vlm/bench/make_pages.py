#!/usr/bin/env python3
"""Build the spike page set: 50 pages rendered from public-domain text with
exact ground truth, then degraded to imitate scans and phone photos.

usage: make_pages.py OUT_DIR   (needs Pillow + numpy; fonts-noto-cjk, fonts-liberation)
Writes OUT_DIR/pages/*.png|jpg and OUT_DIR/manifest.jsonl (file, cat, lang, text).
"""
import json, os, random, sys, glob
import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = sys.argv[1]
os.makedirs(os.path.join(OUT, "pages"), exist_ok=True)
rng = random.Random(20260929)
nrng = np.random.default_rng(20260929)

def find(patterns):
    for p in patterns:
        hits = sorted(glob.glob(p))
        if hits:
            return hits[0]
    raise SystemExit(f"font not found: {patterns}")

CJK = find(["/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", "/usr/share/fonts/**/NotoSansCJK-Regular.ttc"])
LATIN = find(["/usr/share/fonts/truetype/liberation/LiberationSerif-Regular.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf"])
# Noto CJK .ttc order: JP, KR, SC, TC, HK
CJK_INDEX = {"zh-hans": 2, "zh-hant": 3}

def load(name):
    return [l.rstrip("\n") for l in open(os.path.join(HERE, "corpus", name + ".txt"), encoding="utf-8") if l.strip()]

CORPUS = {
    "en": load("en-alice-ch1") + load("en-alice-ch2") + load("en-alice-ch3") + load("en-alice-ch4"),
    "zh-hant": load("zh-hant-lu-xun-ahq") + load("zh-hant-sunzi"),
    "zh-hans": load("zh-hans-lu-xun-kuangren-riji"),
}

W, H, MARGIN = 1240, 1754, 110  # A4 at 150 dpi

def font_for(lang, size):
    if lang == "en":
        return ImageFont.truetype(LATIN, size)
    return ImageFont.truetype(CJK, size, index=CJK_INDEX[lang])

def wrap(text, font, width, cjk):
    lines, cur = [], ""
    tokens = list(text) if cjk else text.split(" ")
    for tok in tokens:
        cand = cur + tok if cjk else (cur + " " + tok if cur else tok)
        if font.getlength(cand) <= width or not cur:
            cur = cand
        else:
            lines.append(cur)
            cur = tok
    if cur:
        lines.append(cur)
    return lines

def compose(lang, budget, cursor, height=H):
    """Fill one page with consecutive paragraphs; returns (image, truth text, new cursor)."""
    src = CORPUS[lang]
    cjk = lang != "en"
    size = 30 if cjk else 30
    font = font_for(lang, size)
    head = font_for(lang, 44)
    img = Image.new("RGB", (W, height), (255, 255, 255))
    d = ImageDraw.Draw(img)
    y, truth, used = MARGIN, [], 0
    lead = int(size * 1.55)
    idx = cursor
    first = True
    while idx < len(src) + 1000:
        para = src[idx % len(src)]
        idx += 1
        short = len(para) < 30
        f = head if (short and first) else font
        lines = wrap(para, f, W - 2 * MARGIN, cjk)
        need = len(lines) * (int(f.size * 1.5)) + int(size * 0.7)
        if y + need > height - MARGIN or used + len(para) > budget:
            if truth:
                break
        for ln in lines:
            d.text((MARGIN, y), ln, font=f, fill=(20, 20, 20))
            y += int(f.size * 1.5)
        y += int(size * 0.7)
        truth.append(para)
        used += len(para)
        first = False
    return img, "\n".join(truth), idx % len(src)

def scan(img, k):
    g = img.convert("L")
    angle = rng.uniform(-1.4, 1.4)
    g = g.rotate(angle, resample=Image.BICUBIC, expand=False, fillcolor=235)
    g = g.filter(ImageFilter.GaussianBlur(0.9))
    a = np.asarray(g, dtype=np.float32)
    a = a * 0.82 + 30 + nrng.normal(0, 9, a.shape)  # faded ink, paper grey, sensor noise
    yy = np.linspace(0, 1, a.shape[0])[:, None]
    a = a - 18 * yy  # uneven lamp
    a = np.clip(a, 0, 255).astype(np.uint8)
    rgb = np.stack([a, (a * 0.97).astype(np.uint8), (a * 0.90).astype(np.uint8)], axis=2)
    return Image.fromarray(rgb)

def photo(img, k):
    bg = Image.new("RGB", (1700, 2200), (48, 44, 40))
    page = img.convert("RGB").resize((1240, 1754))
    bg.paste(page, (230, 230))
    j = lambda: rng.uniform(-90, 90)
    src = [(230, 230), (1470, 230), (1470, 1984), (230, 1984)]
    dst = [(230 + j(), 230 + j()), (1470 + j(), 230 + j()), (1470 + j(), 1984 + j()), (230 + j(), 1984 + j())]
    coef = np.linalg.solve(
        np.array([[x, y, 1, 0, 0, 0, -u * x, -u * y] for (x, y), (u, v) in zip(dst, src)]
                 + [[0, 0, 0, x, y, 1, -v * x, -v * y] for (x, y), (u, v) in zip(dst, src)], dtype=np.float64),
        np.array([u for _, (u, v) in zip(dst, src)] + [v for _, (u, v) in zip(dst, src)], dtype=np.float64))
    bg = bg.transform(bg.size, Image.PERSPECTIVE, coef.tolist(), Image.BICUBIC, fillcolor=(48, 44, 40))
    bg = bg.filter(ImageFilter.GaussianBlur(1.3))
    a = np.asarray(bg, dtype=np.float32)
    xx = np.linspace(0.75, 1.1, a.shape[1])[None, :, None]
    yy = np.linspace(1.05, 0.8, a.shape[0])[:, None, None]
    a = a * xx * yy + nrng.normal(0, 6, a.shape)
    out = Image.fromarray(np.clip(a, 0, 255).astype(np.uint8))
    return out.resize((1275, 1650))

plan = ([("en", "clean")] * 12 + [("zh-hant", "clean")] * 10 + [("zh-hans", "clean")] * 8
        + [("en", "scan")] * 3 + [("zh-hant", "scan")] * 3 + [("zh-hans", "scan")] * 3
        + [("en", "photo")] * 3 + [("zh-hant", "photo")] * 3 + [("zh-hans", "photo")] * 2)
# 12+10+8+9+8 = 47; three mixed EN + zh-Hant pages complete the 50
mixed = 3
cursors = {"en": 0, "zh-hant": 0, "zh-hans": 0}
rows = []
for n, (lang, cat) in enumerate(plan):
    budget = 900 if lang == "en" else 500
    img, truth, cursors[lang] = compose(lang, budget, cursors[lang])
    if cat == "scan":
        out, ext, q = scan(img, n), "jpg", 60
    elif cat == "photo":
        out, ext, q = photo(img, n), "jpg", 70
    else:
        out, ext, q = img, "png", None
    name = f"{n:02d}-{cat}-{lang}.{ext}"
    path = os.path.join(OUT, "pages", name)
    out.save(path, quality=q) if q else out.save(path)
    rows.append({"file": name, "cat": cat, "lang": lang, "text": truth})
for m in range(mixed):
    # top half English, bottom half Traditional Chinese
    img, t1, cursors["en"] = compose("en", 450, cursors["en"], H // 2)
    img2, t2, cursors["zh-hant"] = compose("zh-hant", 250, cursors["zh-hant"], H // 2)
    page = Image.new("RGB", (W, H), (255, 255, 255))
    page.paste(img, (0, 0))
    page.paste(img2, (0, H // 2))
    name = f"{len(rows):02d}-mixed-en+zh-hant.png"
    page.save(os.path.join(OUT, "pages", name))
    rows.append({"file": name, "cat": "mixed", "lang": "en+zh-hant", "text": t1 + "\n" + t2})
with open(os.path.join(OUT, "manifest.jsonl"), "w", encoding="utf-8") as f:
    for r in rows:
        f.write(json.dumps(r, ensure_ascii=False) + "\n")
print(len(rows), "pages")
