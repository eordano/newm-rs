"""Render the newm-rs README video as raw RGB frames on stdout.

Inputs: out/timeline.json (timeline.py: one beat per narration clip), events.json (sanitize.py:
window kinds, tile rects and camera of the stretches picked from a real newm-rs usage log) and
the session's wallpaper.
The look follows the earlier replay renderer: the stylized canvas, the camera box, windows drawn
by kind with made-up contents, the dark green theme and the accent glow. No key names, action
labels or counters ever reach the screen. Real arrangements drive the opening, the canvas and
camera scenes, the loop and the close; the principle demos and a few feature demos are simulated
on the same canvas.

usage: render.py video TIMELINE EVENTS WALL REG BOLD [FROM_S TO_S]   | ffmpeg -f rawvideo ...
       render.py loop   TIMELINE EVENTS WALL REG BOLD                | ffmpeg ... (30 fps)
       render.py sheet  TIMELINE EVENTS WALL REG BOLD OUT.png T1 T2 ...
       render.py stills TIMELINE EVENTS WALL REG BOLD OUT_DIR T1 T2 ...   (full-size PNGs)
"""

import bisect
import json
import math
import random
import sys
from multiprocessing import Pool

from PIL import Image, ImageDraw, ImageEnhance, ImageFilter, ImageFont

W, H, FPS = 1920, 1080, 60
FULL = (0, 0, W, H)
SCREEN = (4096 / 119, 1152 / 113)  # DP-1: 5120x1440 at scale 1.25, in 119x113 px tiles
TR = 113 / 119
TAG = "DP-1 · 5120×1440 @ 1.25"
REPO = "github.com/eordano/newm-rs"
WORKERS = 48

BG = (11, 12, 22)
DARKER = (6, 6, 12)
LIGHTER = (21, 24, 40)
SELECTION = (31, 37, 58)
MUTED = (45, 52, 80)
FG = (221, 247, 255)
DIM = (106, 110, 149)
FGL = (181, 197, 219)
ACCENT = (130, 251, 156)
CYAN = (124, 248, 247)
BLUE = (130, 157, 212)
TEAL = (80, 247, 212)
MAGENTA = (134, 167, 223)
WARN = (255, 170, 120)

KINDS = {
    "Terminal": (ACCENT, 0xF018D),
    "Browser": (BLUE, 0xF059F),
    "Editor": (CYAN, 0xF0169),
    "Files": (TEAL, 0xF024B),
    "Screenshot": (MAGENTA, 0xF0100),
    "Text": (FGL, 0xF0219),
    "Media": (MAGENTA, 0xF040A),
    "Launcher": (CYAN, 0xF0349),
    "Settings": (FGL, 0xF0493),
    "Wallet": (TEAL, 0xF0584),
    "Window": (DIM, 0xF08C6),
}
LOGO, SCREEN_GLYPH, CURSOR, MOUSE, KEYB = 0xF056E, 0xF10AC, 0xF01BF, 0xF037D, 0xF030C
LOCK, CHECK, HAND, SEARCH, WAVES = 0xF033E, 0xF0E1E, 0xF1821, 0xF0349, 0xF078D
SHIELD, FLASH, CAMERA, WEB = 0xF0565, 0xF0241, 0xF0100, 0xF059F
GROUPS = {"canvas": "THE CANVAS", "camera": "THE CAMERA", "session": "YOUR SESSION",
          "kde": "KDE AND EVERY TOOLKIT", "stab": "STABILITY"}

class Draw(ImageDraw.ImageDraw):
    """ImageDraw whose text honours the fill's alpha on an RGB image (Pillow draws text
    opaque there): a translucent run is drawn on a small layer and pasted through its alpha."""

    def __init__(self, im, mode=None):
        super().__init__(im, mode)
        self.img = im

    def text(self, xy, text, fill=None, font=None, anchor=None, stroke_width=0, stroke_fill=None, **kw):
        a = fill[3] if isinstance(fill, tuple) and len(fill) == 4 else 255
        if a >= 255 or not text:
            return super().text(xy, text, fill=fill, font=font, anchor=anchor, stroke_width=stroke_width,
                                stroke_fill=stroke_fill, **kw)
        if a <= 0:
            return None
        bx = self.textbbox(xy, text, font=font, anchor=anchor, stroke_width=stroke_width)
        x0, y0 = math.floor(bx[0]) - 2, math.floor(bx[1]) - 2
        x1, y1 = math.ceil(bx[2]) + 2, math.ceil(bx[3]) + 2
        if x1 <= x0 or y1 <= y0:
            return None
        layer = Image.new("RGBA", (x1 - x0, y1 - y0), (0, 0, 0, 0))
        sf = tuple(stroke_fill[:3]) + (255,) if stroke_fill else None
        ImageDraw.Draw(layer).text((xy[0] - x0, xy[1] - y0), text, fill=tuple(fill[:3]) + (255,), font=font,
                                   anchor=anchor, stroke_width=stroke_width, stroke_fill=sf, **kw)
        layer.putalpha(layer.getchannel("A").point(lambda v: v * a // 255))
        self.img.paste(layer, (x0, y0), layer)
        return None


G = {}


# ---------------------------------------------------------------- easing and small helpers

def clamp(x, a=0.0, b=1.0):
    return max(a, min(b, x))


def ph(u, a, b):
    return clamp((u - a) / (b - a)) if b > a else float(u >= a)


def ease_out(x):
    return 1 - (1 - clamp(x)) ** 3


def ease_io(x):
    return 0.5 - 0.5 * math.cos(math.pi * clamp(x))


def ease_back(x):
    x = clamp(x)
    c = 1.70158
    return 1 + (c + 1) * (x - 1) ** 3 + c * (x - 1) ** 2


def lerp(a, b, x):
    return a + (b - a) * x


def rgba(c, a):
    return tuple(c[:3]) + (max(0, min(255, int(a))),)


def win(n, kind, i, j, w, h, a=1.0, s=1.0, **o):
    return [n, kind, i, j, w, h, a, s, o]


def static(layout, **o):
    return [[w[0], w[1], w[2], w[3], w[4], w[5], 1.0, 1.0, dict(o)] for w in layout]


# ---------------------------------------------------------------- setup

def init(paths):
    tl, ev, wall, reg, bold = paths

    def font(size, b=False):
        return ImageFont.truetype(bold if b else reg, size)

    G["f"] = {"brand": font(150, True), "num": font(104, True), "h1": font(56, True),
              "h2": font(40, True), "cap": font(33), "t": font(26), "tb": font(26, True),
              "s": font(20), "sb": font(20, True), "xs": font(15), "g": font(26), "gm": font(36),
              "gl": font(56), "gxl": font(96), "ptr": font(48), "stat": font(170, True), "ws": font(17),
              "wxs": font(14)}
    G["tl"] = json.load(open(tl))
    G.update(json.load(open(ev)))
    img = Image.open(wall).convert("RGB")
    s = H / img.height
    img = img.resize((int(img.width * s), H), Image.LANCZOS)
    x0 = (img.width - W) // 2
    G["wall"] = img.crop((x0, 0, x0 + W, H))
    G["back"] = ImageEnhance.Brightness(G["wall"]).enhance(0.6)
    G["card"] = ImageEnhance.Brightness(G["wall"]).enhance(0.42)
    G["patterns"] = {}
    beats = G["tl"]["beats"]
    G["t0s"] = [b["t0"] for b in beats]
    sec = {}
    for b in beats:
        sec.setdefault(b["group"], []).append(b)
    for b in beats:
        grp = sec[b["group"]]
        b["st"] = b["t0"] - grp[0]["t0"]
        b["SD"] = sum(x["dur"] for x in grp)
        b["gdurs"] = [x["dur"] for x in grp]
    G["title"] = title_layer()
    prepare()


# ---------------------------------------------------------------- the real log

ACTION, CUT = 14, 1.3


def screen_rect(view, zoom):
    s = 0.8 ** zoom
    return [view[0], view[1], SCREEN[0] / s, SCREEN[1] / s]


def centered_rect(c, zoom):
    s = 0.8 ** zoom
    return [c[0] - SCREEN[0] / s / 2, c[1] - SCREEN[1] / s / 2, SCREEN[0] / s, SCREEN[1] / s]


def cam_for(scr):
    x, y, sw, sh = scr
    return [x + sw / 2, y + sh / 2, max(sw * 1.18, sh * 1.4 * W * TR / H, 36.0)]


def cam_lerp(a, b, x):
    return [lerp(a[0], b[0], x), lerp(a[1], b[1], x), math.exp(lerp(math.log(a[2]), math.log(b[2]), x))]


def gap_frames(g):
    return 0 if g < 0.2 else min(16, 4 + int(2.5 * math.log2(1 + g)))


def far(v0, z0, v1, z1):
    width = SCREEN[0] / 0.8 ** max(z0, z1)
    return math.hypot(v1[0] - v0[0], (v1[1] - v0[1]) * TR) > CUT * width


def tween(a_list, b_list, x):
    a_map, b_map = {w[0]: w for w in a_list}, {w[0]: w for w in b_list}
    out = []
    for n in sorted(set(a_map) | set(b_map)):
        a, b = a_map.get(n), b_map.get(n)
        src, dst = a or b, b or a
        if a is None:
            alpha, scale = x, 0.86 + 0.14 * x
        elif b is None:
            alpha, scale = 1 - x, 1 - 0.14 * x
        else:
            alpha, scale = 1.0, 1.0
        out.append([n, dst[1]] + [lerp(src[k], dst[k], x) for k in range(2, 6)] + [alpha, scale, {}])
    return out


def replay(actions, action_frames=ACTION):
    """Per-frame (windows, focus, view, zoom, dip) for a run of coalesced actions."""
    frames, prev = [], None
    for e in actions:
        if prev is not None:
            n = gap_frames(e["s"] - prev["end"])
            v0, z0, v1, z1 = prev["view"][1], prev["zoom"][1], e["view"][0], e["zoom"][0]
            cut = far(v0, z0, v1, z1)
            for f in range(n):
                x = ease_io((f + 1) / n)
                if cut:
                    (v, z), dip = ((v0, z0) if x < 0.5 else (v1, z1)), 1 - abs(x - 0.5) * 2
                else:
                    v, z, dip = [lerp(v0[k], v1[k], x) for k in range(2)], lerp(z0, z1, x), 0.0
                frames.append((tween(prev["after"], e["before"], x), prev["w"], v, z, dip))
        v0, z0, v1, z1 = e["view"][0], e["zoom"][0], e["view"][1], e["zoom"][1]
        for f in range(action_frames):
            x = ease_out((f + 1) / action_frames)
            v, z = [lerp(v0[k], v1[k], x) for k in range(2)], lerp(z0, z1, x)
            frames.append((tween(e["before"], e["after"], x), e["w"], v, z, 0.0))
        prev = e
    return frames


def rects(layout):
    return {w[0]: tuple(w[2:6]) for w in layout}


def overlaps(r, scr, m=0.0):
    x, y, sw, sh = scr
    return r[0] < x + sw + m and r[0] + r[2] > x - m and r[1] < y + sh + m and r[1] + r[3] > y - m


def cluster(layout, c, reach=70):
    return [w for w in layout if abs(w[2] + w[4] / 2 - c[0]) < reach and abs(w[3] + w[5] / 2 - c[1]) < reach / 2]


def bbox_cam(layout, pad=1.12):
    x0 = min(w[2] for w in layout)
    y0 = min(w[3] for w in layout)
    x1 = max(w[2] + w[4] for w in layout)
    y1 = max(w[3] + w[5] for w in layout)
    span = max((x1 - x0) * pad, (y1 - y0) * pad * W * TR / H, 40)
    return [(x0 + x1) / 2, (y0 + y1) / 2, span], (x0, y0, x1 - x0, y1 - y0)


def prepare():
    S = G["stretch"]
    view = S[0]["view"][0]
    scr = screen_rect(view, 0)
    c = (scr[0] + scr[2] / 2, scr[1] + scr[3] / 2)
    lay = cluster(S[0]["before"], c)
    inn = [w for w in lay if overlaps(w[2:6], scr)]
    focus = max(inn, key=lambda w: -abs(w[2] + w[4] / 2 - c[0]))[0] if inn else None
    whole, box = bbox_cam(lay)
    G["A"] = {"wins": lay, "view": view, "scr": scr, "c": c, "focus": focus, "whole": whole, "box": box}
    G.setdefault("restore", {"wins": lay, "view": view, "zoom": 0, "order": [w[0] for w in lay]})
    G["cache"] = {}


def frames_of(key):
    if key not in G["cache"]:
        G["cache"][key] = replay(G[key])
    return G["cache"][key]


# ---------------------------------------------------------------- drawing

def pattern(n, kind):
    if (n, kind) in G["patterns"]:
        return G["patterns"][(n, kind)]
    r = random.Random(n * 7919 + len(kind))
    out = []
    if kind == "Terminal":
        y = 0.04
        while y < 0.86:
            if r.random() < 0.2:
                out.append((0.03, y, 0.025, 0.035, ACCENT, 230))
                out.append((0.07, y, r.uniform(0.15, 0.45), 0.035, FG, 150))
            else:
                out.append((0.03, y, r.uniform(0.2, 0.85), 0.03, ACCENT, 70))
            y += 0.065
        out.append((0.03, y, 0.025, 0.045, ACCENT, 255))
    elif kind == "Browser":
        out.append((0.02, 0.03, 0.96, 0.07, SELECTION, 255))
        out.append((0.05, 0.048, 0.4, 0.035, DIM, 170))
        out.append((0.06, 0.16, 0.88, 0.28, LIGHTER, 255))
        out.append((0.06, 0.16, 0.3, 0.28, BLUE, 40))
        y = 0.5
        while y < 0.92:
            out.append((0.06, y, r.uniform(0.4, 0.88), 0.03, FGL, 80))
            y += 0.06
    elif kind == "Editor":
        out.append((0.0, 0.0, 0.07, 1.0, LIGHTER, 255))
        y, ind = 0.04, 0
        while y < 0.94:
            ind = max(0, min(4, ind + r.choice((-1, 0, 0, 1))))
            out.append((0.02, y, 0.03, 0.025, DIM, 120))
            out.append((0.1 + 0.05 * ind, y, r.uniform(0.15, 0.55), 0.028,
                        r.choice((CYAN, BLUE, FG, FGL, ACCENT)), 130))
            y += 0.055
    elif kind == "Files":
        for row in range(3):
            for col in range(4):
                out.append((0.06 + col * 0.23, 0.08 + row * 0.3, 0.14, 0.16, TEAL, 110))
                out.append((0.06 + col * 0.23, 0.27 + row * 0.3, 0.14, 0.03, FGL, 80))
    elif kind == "Launcher":
        out.append((0.05, 0.08, 0.9, 0.14, SELECTION, 255))
        out.append((0.09, 0.13, 0.3, 0.04, FG, 160))
        for k in range(4):
            out.append((0.05, 0.3 + k * 0.16, 0.08, 0.1, CYAN, 120))
            out.append((0.17, 0.33 + k * 0.16, r.uniform(0.3, 0.7), 0.04, FGL, 90))
    elif kind == "Settings":
        out.append((0.0, 0.0, 0.26, 1.0, LIGHTER, 255))
        for k in range(6):
            out.append((0.04, 0.06 + k * 0.12, 0.17, 0.04, FGL, 90))
            out.append((0.32, 0.08 + k * 0.14, r.uniform(0.25, 0.45), 0.04, FGL, 90))
            out.append((0.82, 0.07 + k * 0.14, 0.1, 0.06, ACCENT if k % 2 else MUTED, 160))
    elif kind == "Wallet":
        for k in range(5):
            out.append((0.05, 0.08 + k * 0.17, 0.9, 0.12, LIGHTER, 255))
            out.append((0.09, 0.12 + k * 0.17, 0.06, 0.05, TEAL, 170))
            out.append((0.2, 0.12 + k * 0.17, r.uniform(0.25, 0.5), 0.04, FGL, 100))
    elif kind == "Media":
        out.append((0.04, 0.05, 0.92, 0.72, DARKER, 255))
        out.append((0.44, 0.33, 0.12, 0.16, MAGENTA, 120))
        out.append((0.04, 0.86, 0.92, 0.03, MUTED, 255))
        out.append((0.04, 0.86, 0.38, 0.03, MAGENTA, 200))
    elif kind == "Screenshot":
        out.append((0.08, 0.08, 0.84, 0.6, LIGHTER, 255))
        out.append((0.2, 0.2, 0.4, 0.3, MAGENTA, 60))
        for k in range(3):
            out.append((0.08 + k * 0.3, 0.78, 0.24, 0.1, SELECTION, 255))
    else:
        y = 0.06
        while y < 0.9:
            out.append((0.05, y, r.uniform(0.3, 0.9), 0.03, FGL, 80))
            y += 0.07
    G["patterns"][(n, kind)] = out
    return out


def glow(img, box, alpha, color=ACCENT, spread=26, blur=11, strength=150):
    m = spread
    x0, y0, x1, y1 = (int(v) for v in box)
    if x1 - x0 < 4 or y1 - y0 < 4 or alpha <= 0.01:
        return
    mask = Image.new("L", (x1 - x0 + 2 * m, y1 - y0 + 2 * m), 0)
    ImageDraw.Draw(mask).rounded_rectangle([m - 3, m - 3, m + x1 - x0 + 3, m + y1 - y0 + 3],
                                           radius=14, fill=int(strength * alpha))
    img.paste(color, (x0 - m, y0 - m), mask.filter(ImageFilter.GaussianBlur(blur)))


def window(d, box, n, kind, alpha, focused, px, o):
    color, icon = KINDS.get(kind, KINDS["Window"])
    style = o.get("style")
    label = o.get("label", kind.lower())
    x0, y0, x1, y1 = box
    rad = max(3, min(12, px * 0.14))
    if style == "x11":
        rad = 2
    elif style == "adw":
        rad = max(4, min(18, px * 0.22))
    d.rounded_rectangle(box, radius=rad, fill=rgba(BG, 236 * alpha))
    head = min(max(10, px * 0.34), (y1 - y0) * 0.22)
    if style in ("gtk", "adw"):
        head *= 1.3
    elif style == "x11":
        head *= 0.8
    hfill = SELECTION if style == "adw" else LIGHTER
    d.rounded_rectangle([x0, y0, x1, y0 + head], radius=rad, fill=rgba(hfill, 245 * alpha),
                        corners=(True, True, False, False))
    if head >= 16 and x1 - x0 > 110:
        f = G["f"]["wxs"] if head < 26 else G["f"]["ws"]
        if style in ("gtk", "adw"):
            tw = d.textlength(label, font=f) + 24
            cx = (x0 + x1) / 2 - tw / 2
            d.text((cx, y0 + head / 2), chr(icon), font=f, anchor="lm", fill=rgba(color, 255 * alpha))
            d.text((cx + 24, y0 + head / 2), label, font=f, anchor="lm", fill=rgba(FGL, 255 * alpha))
            d.ellipse([x1 - 26, y0 + head / 2 - 8, x1 - 10, y0 + head / 2 + 8], fill=rgba(MUTED, 255 * alpha))
        elif style in ("chromium", "electron") or o.get("tabs") is not None:
            ntab = o.get("tabs", 3) if o.get("tabs") is not None else 3
            tx = x0 + 10
            for k in range(int(ntab)):
                tw = min(150, (x1 - x0 - 120) / 4)
                d.rounded_rectangle([tx, y0 + 4, tx + tw, y0 + head], radius=6,
                                    fill=rgba(BG if k == 0 else SELECTION, 255 * alpha))
                if k == 0:
                    d.text((tx + 10, y0 + head / 2 + 2), chr(icon), font=f, anchor="lm", fill=rgba(color, 255 * alpha))
                    d.text((tx + 32, y0 + head / 2 + 2), label, font=f, anchor="lm", fill=rgba(FGL, 255 * alpha))
                tx += tw + 6
        else:
            d.text((x0 + 10, y0 + head / 2), chr(icon), font=f, anchor="lm", fill=rgba(color, 255 * alpha))
            d.text((x0 + 32, y0 + head / 2), label, font=f, anchor="lm", fill=rgba(DIM if style is None else FGL, 255 * alpha))
        if style not in ("gtk", "adw"):
            for k in range(3):
                cx = x1 - 14 - k * 16
                if style == "x11":
                    d.rectangle([cx - 4, y0 + head / 2 - 4, cx + 4, y0 + head / 2 + 4], fill=rgba(MUTED, 255 * alpha))
                else:
                    d.ellipse([cx - 4, y0 + head / 2 - 4, cx + 4, y0 + head / 2 + 4], fill=rgba(MUTED, 255 * alpha))
    top = y0 + head
    if style in ("qt", "kde") and y1 - y0 > 80:
        for k in range(4):
            d.rounded_rectangle([x0 + 10 + k * 52, top + 6, x0 + 50 + k * 52, top + 14], radius=3,
                                fill=rgba(FGL, 70 * alpha))
        top += 18
    bx0, by0 = x0 + 7, top + 7
    bw, bh = x1 - x0 - 14, y1 - top - 14
    if bw > 40 and bh > 30:
        items = pattern(n, kind)
        k = int(len(items) * clamp(o.get("fill", 1.0)) + 0.5)
        for fx, fy, fw, fh, c, a in items[:k]:
            rx0, ry0 = bx0 + fx * bw, by0 + fy * bh
            d.rounded_rectangle([rx0, ry0, rx0 + max(2, fw * bw), ry0 + max(2, fh * bh)],
                                radius=2, fill=rgba(c, a * alpha))
    edge = rgba(ACCENT, 160 * alpha) if focused else rgba(MUTED, 150 * alpha)
    d.rounded_rectangle(box, radius=rad, outline=edge, width=1)
    fl = o.get("flash", 0)
    if fl > 0.01:
        d.rounded_rectangle(box, radius=rad, outline=rgba(ACCENT, 230 * fl * alpha), width=2)


def dashed(d, box, color, a, dash=12, width=2):
    x0, y0, x1, y1 = box
    for (ax, ay, bx, by) in ((x0, y0, x1, y0), (x1, y0, x1, y1), (x1, y1, x0, y1), (x0, y1, x0, y0)):
        length = math.hypot(bx - ax, by - ay)
        k = 0.0
        while k < length:
            e = min(length, k + dash)
            d.line([(lerp(ax, bx, k / length), lerp(ay, by, k / length)),
                    (lerp(ax, bx, e / length), lerp(ay, by, e / length))], fill=rgba(color, a), width=width)
            k += dash * 1.8


def pointer(d, x, y, a=1.0, size="ptr"):
    d.text((x, y), chr(CURSOR), font=G["f"][size], anchor="lt", fill=rgba(FG, 255 * a),
           stroke_width=3, stroke_fill=rgba(DARKER, 220 * a))


def paint(img, vp, cam, wins, focus=None, scr=None, inner=None, grid=0.0, edge_cells=0.0,
          ghosts=(), preview=None, ptr=None, dip=0.0, dim=1.0, tag=True, tagpulse=0.0, extra=None,
          border=True):
    """Draw the canvas through camera cam=(cx, cy, tiles across) into viewport vp of img."""
    x0, y0, vw, vh = vp
    full = (vw, vh) == (W, H)
    sub = G["back"].copy() if full else G["back"].crop((x0, y0, x0 + vw, y0 + vh))
    d = Draw(sub, "RGBA")
    cx, cy, span = cam
    px = vw / span
    py = px * TR
    ox, oy = cx - span / 2, cy - (vh / py) / 2

    def to(i, j):
        return (i - ox) * px, (j - oy) * py

    cols, rows = int(span) + 2, int(vh / py) + 2
    if grid > 0.01:
        for i in range(math.floor(ox), math.floor(ox) + cols):
            x = (i - ox) * px
            d.line([(x, 0), (x, vh)], fill=rgba(FGL, 34 * grid))
        for j in range(math.floor(oy), math.floor(oy) + rows):
            y = (j - oy) * py
            d.line([(0, y), (vw, y)], fill=rgba(FGL, 34 * grid))
    elif px >= 14 and cols * rows < 6000:
        for i in range(math.floor(ox), math.floor(ox) + cols):
            for j in range(math.floor(oy), math.floor(oy) + rows):
                x, y = to(i, j)
                d.ellipse([x - 1, y - 1, x + 1, y + 1], fill=rgba(FGL, 45))
    if edge_cells > 0.01 and scr:
        sx, sy, sw, sh = scr
        cells = set()
        for i in range(math.floor(sx), math.ceil(sx + sw)):
            for j in (math.floor(sy), math.ceil(sy + sh) - 1):
                cells.add((i, j))
        for j in range(math.floor(sy), math.ceil(sy + sh)):
            for i in (math.floor(sx), math.ceil(sx + sw) - 1):
                cells.add((i, j))
        for i, j in cells:
            (ax, ay), (bx, by) = to(i, j), to(i + 1, j + 1)
            d.rectangle([ax + 1, ay + 1, bx - 1, by - 1], fill=rgba(ACCENT, 30 * edge_cells))
    for gi, gj, gw, gh, ga in ghosts:
        (ax, ay), (bx, by) = to(gi, gj), to(gi + gw, gj + gh)
        d.rounded_rectangle([ax + 3, ay + 3, bx - 3, by - 3], radius=10, fill=rgba(ACCENT, 16 * ga))
        dashed(d, [ax + 3, ay + 3, bx - 3, by - 3], ACCENT, 170 * ga)
    order = sorted(wins, key=lambda w: (bool(w[8].get("lift")), w[0] == focus))
    for wv in order:
        n, kind, i, j, w, h, a, s, o = wv
        if a <= 0.01:
            continue
        (ax, ay), (bx, by) = to(i, j), to(i + w, j + h)
        if bx < -40 or by < -40 or ax > vw + 40 or ay > vh + 40:
            continue
        g = min(5.0, px * 0.05)
        mx, my = (ax + bx) / 2, (ay + by) / 2
        hw, hh = (bx - ax) / 2 * s - g, (by - ay) / 2 * s - g
        box = [mx - hw, my - hh, mx + hw, my + hh]
        if n == focus or o.get("lift"):
            glow(sub, box, a * (1.0 if n == focus else 0.6))
        window(d, box, n, kind, a, n == focus, px, o)
    if preview:
        pi_, pj, pw, ph_, pa = preview
        (ax, ay), (bx, by) = to(pi_, pj), to(pi_ + pw, pj + ph_)
        d.rounded_rectangle([ax + 2, ay + 2, bx - 2, by - 2], radius=10, fill=rgba(ACCENT, 34 * pa),
                            outline=rgba(ACCENT, 220 * pa), width=3)
    if scr:
        sx, sy, sw, sh = scr
        (sx0, sy0), (sx1, sy1) = to(sx, sy), to(sx + sw, sy + sh)
        mask = Image.new("L", (vw, vh), int(120 * dim))
        md = ImageDraw.Draw(mask)
        md.rounded_rectangle([sx0, sy0, sx1, sy1], radius=12, fill=0)
        if inner:
            ix, iy, iw, ih, ia = inner
            (ix0, iy0), (ix1, iy1) = to(ix, iy), to(ix + iw, iy + ih)
            md.rectangle([sx0, sy0, sx1, sy1], fill=int(110 * ia))
            md.rounded_rectangle([ix0, iy0, ix1, iy1], radius=10, fill=0)
        sub.paste(DARKER, (0, 0), mask)
        if inner and inner[4] > 0.05:
            d.rounded_rectangle([ix0, iy0, ix1, iy1], radius=10, outline=rgba(FGL, 70 * inner[4]), width=1)
        d.rounded_rectangle([sx0, sy0, sx1, sy1], radius=12, outline=rgba(ACCENT, 60 + 120 * tagpulse), width=1 + int(2 * tagpulse))
        if tag:
            f = G["f"]["xs"]
            iw, gap, tw = run_metrics(d, TAG, SCREEN_GLYPH, f)
            d.rounded_rectangle([sx0 + 10, sy0 - 30, sx0 + 10 + 20 + iw + gap + tw, sy0 - 6], radius=7,
                                fill=(11, 12, 22, 200), outline=rgba(ACCENT, 50 + 150 * tagpulse))
            label_run(d, sx0 + 10, sy0 - 18, TAG, SCREEN_GLYPH, f, 10, 1.0, color=FGL)
    if extra:
        extra(sub, d, to, px)
    if ptr:
        x, y = to(ptr[0], ptr[1])
        pointer(d, x, y, ptr[2] if len(ptr) > 2 else 1.0)
    if dip > 0:
        sub.paste(DARKER, (0, 0, vw, vh), Image.new("L", (vw, vh), int(235 * dip)))
    if full:
        img.paste(sub)
    else:
        m = Image.new("L", (vw, vh), 0)
        ImageDraw.Draw(m).rounded_rectangle([0, 0, vw - 1, vh - 1], radius=18, fill=255)
        img.paste(sub, (x0, y0), m)
        if border:
            Draw(img, "RGBA").rounded_rectangle([x0, y0, x0 + vw - 1, y0 + vh - 1], radius=18,
                                                          outline=rgba(ACCENT, 70), width=1)
    return lambda i, j: (to(i, j)[0] + x0, to(i, j)[1] + y0)


def glass(d, box, a=1.0, r=14, edge=60):
    d.rounded_rectangle(box, radius=r, fill=(11, 12, 22, int(215 * a)), outline=rgba(ACCENT, edge * a), width=1)


def run_metrics(d, text, icon, f):
    """The drawn run of a label, measured in the font it is drawn in: (icon width, gap, text
    width). Each width is the larger of the advance and the ink, since Nerd Font icons draw
    wider than their advance."""
    def width(s):
        return max(d.textlength(s, font=f), d.textbbox((0, 0), s, font=f, anchor="ls")[2]) if s else 0.0
    iw = width(chr(icon)) if icon else 0.0
    gap = round(f.size * 0.5) if icon and text else 0
    return iw, gap, width(text)


def label_run(d, x, cy, text, icon, f, pad, a, icolor=ACCENT, color=FG):
    """Draw icon, gap and text from x, centred on cy; return the box width with equal pads."""
    iw, gap, tw = run_metrics(d, text, icon, f)
    tx = x + pad
    if icon:
        d.text((tx, cy), chr(icon), font=f, anchor="lm", fill=rgba(icolor, 255 * a))
        tx += iw + gap
    if text:
        d.text((tx, cy), text, font=f, anchor="lm", fill=rgba(color, 255 * a))
    return pad + iw + gap + tw + pad


def pill(d, x, y, text, icon=None, a=1.0, font="s", center=False, color=FG, hl=0.0):
    f = G["f"][font]
    pad = max(14, round(f.size * 0.75))
    iw, gap, tw = run_metrics(d, text, icon, f)
    w = pad + iw + gap + tw + pad
    hgt = f.size + 22
    if center:
        x -= w / 2
    d.rounded_rectangle([x, y, x + w, y + hgt], radius=hgt / 2,
                        fill=rgba(lerp_c(BG, (24, 60, 40), hl), 215 * a), outline=rgba(ACCENT, (60 + 160 * hl) * a), width=1)
    label_run(d, x, y + hgt / 2, text, icon, f, pad, a, color=color)
    return w


def lerp_c(a, b, x):
    return tuple(int(lerp(a[k], b[k], x)) for k in range(3))


def wrap(d, text, font, maxw):
    lines, cur = [], ""
    for word in text.split():
        t = (cur + " " + word).strip()
        if d.textlength(t, font=font) <= maxw or not cur:
            cur = t
        else:
            lines.append(cur)
            cur = word
    return lines + [cur]


def title_layer():
    img = Image.new("RGBA", (1200, 330), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    glow_m = Image.new("L", img.size, 0)
    ImageDraw.Draw(glow_m).text((600, 140), "newm-rs", font=G["f"]["brand"], anchor="mm", fill=200)
    glow_m = glow_m.filter(ImageFilter.GaussianBlur(18))
    img.paste(ACCENT + (255,), (0, 0), glow_m)
    d.text((600, 140), "newm-rs", font=G["f"]["brand"], anchor="mm", fill=ACCENT + (255,))
    d.text((600, 262), "a Wayland compositor · a strict 2D tile canvas", font=G["f"]["t"],
           anchor="mm", fill=FG + (255,))
    return img


def put_title(img, cx, cy, scale, a):
    if a <= 0.01:
        return
    t = G["title"]
    w, h = int(t.width * scale), int(t.height * scale)
    t = t.resize((w, h), Image.BICUBIC)
    if a < 1:
        t.putalpha(t.getchannel("A").point(lambda v: int(v * a)))
    img.paste(t, (int(cx - w / 2), int(cy - h / 2)), t)


def darken(img, a):
    if a > 0.005:
        img.paste(DARKER, (0, 0, W, H), Image.new("L", (W, H), int(255 * clamp(a))))


def replay_frame(img, frame, vp=FULL, **kw):
    wins, focus, v, z, dip = frame
    scr = screen_rect(v, z)
    return paint(img, vp, cam_for(scr), wins, focus=focus, scr=scr, dip=dip, **kw)


def sample(frames, lt, D, speed=1.0):
    k = int(lt * FPS * speed)
    return frames[min(len(frames) - 1, max(0, k))]


def fit_speed(frames, D, lead=0.3, tail=0.5):
    return max(1.0, len(frames) / ((D - lead - tail) * FPS))


# ---------------------------------------------------------------- the opening and the close

def opening(lt, D, b):
    st = b["st"] + lt
    d1, d2 = b["gdurs"][0], b["gdurs"][1]
    A = G["A"]
    cam1 = cam_for(A["scr"])
    cam0 = list(A["whole"])
    img = Image.new("RGB", (W, H))
    if st < d1 + d2:
        x = ease_io(ph(st, d1 - 0.1, d1 + d2 - 0.3))
        drift = 1.0 + 0.04 * (1 - ph(st, 0, d1))
        cam = cam_lerp([cam0[0], cam0[1], cam0[2] * drift], cam1, x)
        paint(img, FULL, cam, static(A["wins"]), focus=A["focus"] if x > 0.6 else None,
              scr=A["scr"], tag=x > 0.5, dim=0.4 + 0.6 * x)
        darken(img, 0.6 * (1 - ease_io(ph(st, d1 - 0.2, d1 + 0.8))) + 0.6 * (1 - ph(st, 0, 0.9)) * 0.6)
        ta = ph(st, 0.25, 1.0) * (1 - ph(st, d1 + 0.1, d1 + 0.7))
        sc = lerp(0.92, 1.0, ease_out(ph(st, 0.25, 1.1))) + 0.06 * ph(st, d1, d1 + 0.7)
        put_title(img, W / 2, H / 2 - 20 - 80 * ease_io(ph(st, d1, d1 + 0.7)), sc, ta)
    else:
        frames = frames_of("stretch")
        replay_frame(img, sample(frames, st - d1 - d2, D))
    return img


def close(lt, D, b):
    img = Image.new("RGB", (W, H))
    x = ease_io(ph(lt, 0.0, 0.55 * D))
    frames = frames_of("stretch")
    wins, focus, v, z, dip = frames[-1]
    scr = screen_rect(v, z)
    lay = [w for w in wins if w[6] > 0.5]
    whole, _ = bbox_cam(cluster(lay, (scr[0] + scr[2] / 2, scr[1] + scr[3] / 2)) or lay)
    paint(img, FULL, cam_lerp(cam_for(scr), whole, x), wins, focus=focus, scr=scr, dim=1 - 0.5 * x)
    darken(img, 0.62 * ease_io(ph(lt, 0.3, 1.6)) + 0.4 * ph(lt, D - 1.2, D))
    put_title(img, W / 2, 330, lerp(0.9, 1.0, ease_out(ph(lt, 0.2, 1.0))), ph(lt, 0.2, 0.8))
    d = Draw(img, "RGBA")
    L = b["clip"]
    pre = b["voice_at"] - b["t0"]
    at = pre + L * 0.45
    a = ease_out(ph(lt, at, at + 0.4))
    d.text((W / 2, 590 + 16 * (1 - a)), "check it out", font=G["f"]["h2"], anchor="mm", fill=rgba(FG, 255 * a))
    la = ease_out(ph(lt, at + 0.2, at + 0.7))
    pill(d, W / 2, 680, REPO, icon=WEB, a=la, font="t", center=True)
    darken(img, ph(lt, D - 0.9, D) * 0.95)
    return img


# ---------------------------------------------------------------- principles

PV = (120, 330, 1680, 680)


def card():
    return G["card"].copy()


def pcam(cx=15.0, cy=6.0, span=32.0):
    return [cx, cy, span]


def corner_icons(d, icons, x=None, y=None):
    """Icons at the panel's top-right: [(glyph, brightness)]."""
    x = PV[0] + PV[2] - 40 if x is None else x
    y = PV[1] + 34 if y is None else y
    for g, a in reversed(icons):
        if isinstance(g, str):
            d.text((x, y + 30), g, font=G["f"]["h2"], anchor="rm", fill=rgba(FG, 255 * a))
            x -= 60
            continue
        box = [x - 84, y, x, y + 70]
        d.rounded_rectangle(box, radius=14, fill=rgba(BG, 200), outline=rgba(ACCENT, 50 + 170 * a), width=1 + int(a > 0.6))
        d.text(((box[0] + box[2]) / 2, y + 35), chr(g), font=G["f"]["gl"], anchor="mm",
               fill=rgba(lerp_c(DIM, ACCENT, a), 255))
        x -= 100


def keycap(d, x, y, w, h, lit, a=1.0):
    d.rounded_rectangle([x, y, x + w, y + h], radius=8, fill=rgba(lerp_c(LIGHTER, (40, 110, 70), lit), 235 * a),
                        outline=rgba(lerp_c(MUTED, ACCENT, lit), 255 * a), width=1 + int(lit > 0.5))
    d.rounded_rectangle([x + 5, y + 4, x + w - 5, y + h - 9], radius=6, fill=rgba(lerp_c(SELECTION, (60, 150, 95), lit), 200 * a))


KEYS = {"tab": (0, 0, 1.5), "q": (1.5, 0, 1), "w": (2.5, 0, 1), "e": (3.5, 0, 1), "r": (4.5, 0, 1), "t": (5.5, 0, 1),
        "caps": (0, 1, 1.75), "a": (1.75, 1, 1), "s": (2.75, 1, 1), "d": (3.75, 1, 1), "f": (4.75, 1, 1), "g": (5.75, 1, 1),
        "shift": (0, 2, 2.25), "z": (2.25, 2, 1), "x": (3.25, 2, 1), "c": (4.25, 2, 1), "v": (5.25, 2, 1), "b": (6.25, 2, 1),
        "ctrl": (0, 3, 1.25), "super": (1.25, 3, 1.25), "alt": (2.5, 3, 1.25), "space": (3.75, 3, 3.5)}


def keyboard(d, x, y, lit, unit=50, a=1.0):
    """The left half of a keyboard, keys unlabeled; lit maps key -> 0..1."""
    glass(d, [x - 18, y - 18, x + 7.25 * unit + 18, y + 4 * unit + 10], a)
    for k, (kx, ky, kw) in KEYS.items():
        keycap(d, x + kx * unit, y + ky * unit, kw * unit - 6, unit - 6, lit.get(k, 0.0), a)


def pr_display(lt, D, b):
    img = card()
    u = lt / D
    scr = [0.0, 0.0, SCREEN[0], SCREEN[1]]
    gx = ease_io(ph(u, 0.1, 0.42))
    gy = ease_io(ph(u, 0.46, 0.64))
    push = ease_out(ph(u, 0.68, 0.76)) * (1 - ease_io(ph(u, 0.76, 0.86)))
    wmax, hmax = math.floor(SCREEN[0]) - 0.4, math.floor(SCREEN[1])
    aw = lerp(9, wmax, gx) + 0.6 * push
    ah = lerp(5, hmax, gy) + 0.4 * push
    aw, ah = min(aw, wmax + 0.25), min(ah, hmax + 0.15)
    bi = 10.5 + (aw - 9)
    wins = [win(1, "Browser", 0.2, 0.1, aw, ah, flash=push),
            win(2, "Terminal", bi + 0.5, 0.1, 8, 5), win(3, "Editor", bi + 0.5, 5.1, 8, 5)]
    paint(img, PV, [SCREEN[0] / 2 + 4, SCREEN[1] / 2, 50], wins, focus=1, scr=scr, dim=0.9,
          tagpulse=push)
    return img


def pr_focus(lt, D, b):
    img = card()
    u = lt / D
    lay = [win(1, "Files", -30, 1, 9, 7), win(2, "Media", -20, 2, 8, 6), win(3, "Terminal", 0, 1, 8, 8),
           win(4, "Browser", 9, 1, 10, 8), win(5, "Editor", 20, 1, 8, 8), win(6, "Text", 29, 1, 9, 7)]
    x0 = -1.0
    x1 = 38.3 - SCREEN[0]
    x2 = -25.5 - SCREEN[0] / 2
    s1 = ease_io(ph(u, 0.14, 0.34))
    s2 = ease_io(ph(u, 0.7, 0.9))
    sx = lerp(lerp(x0, x1, s1), x2, s2)
    scr = [sx, -0.1, SCREEN[0], SCREEN[1]]
    focus = 5 if u < 0.1 else (6 if u < 0.7 else 1)
    cam = [lerp(lerp(17, 21, s1), sx + SCREEN[0] / 2, s2), 5.0, 50]
    paint(img, PV, cam, lay, focus=focus, scr=scr, dim=0.9)
    a = ph(u, 0.44, 0.5) * (1 - ph(u, 0.68, 0.72))
    if a > 0.01:
        d = Draw(img, "RGBA")
        thumbs = [lay[5], lay[4], lay[3], lay[2], lay[0]]
        sel = 0 if u < 0.54 else (2 if u < 0.6 else 4)
        cx, cy = PV[0] + PV[2] / 2, PV[1] + PV[3] / 2 + 40
        tw, th, gap = 200, 130, 22
        total = len(thumbs) * tw + (len(thumbs) - 1) * gap
        glass(d, [cx - total / 2 - 26, cy - th / 2 - 34, cx + total / 2 + 26, cy + th / 2 + 34], a)
        for k, w in enumerate(thumbs):
            bx = cx - total / 2 + k * (tw + gap)
            box = [bx, cy - th / 2, bx + tw, cy + th / 2]
            if k == sel:
                d.rounded_rectangle([box[0] - 8, box[1] - 8, box[2] + 8, box[3] + 8], radius=14,
                                    outline=rgba(ACCENT, 240 * a), width=3)
            window(d, box, w[0], w[1], a, k == sel, 30, {})
    return img


def pr_overlap(lt, D, b):
    img = card()
    u = lt / D
    g1, g2 = ease_io(ph(u, 0.14, 0.4)), ease_io(ph(u, 0.5, 0.7))
    f1, f2 = math.sin(math.pi * ph(u, 0.14, 0.4)), math.sin(math.pi * ph(u, 0.5, 0.7))
    wins = [win(1, "Terminal", 2, 1, 7, 6), win(2, "Browser", 9, 1, 7 + 4 * g1, 6 + g2),
            win(3, "Editor", 16 + 4 * g1, 1, 7, 6, flash=f1), win(4, "Files", 9, 7 + g2, 7, 3, flash=f2),
            win(5, "Text", 2, 7, 7, 4)]
    paint(img, PV, pcam(14.5, 6.0, 32), wins, focus=2)
    return img


def pr_zoom(lt, D, b):
    img = card()
    A = G["A"]
    u = lt / D
    z = 2.2 * ease_io(ph(u, 0.1, 0.42)) - 2.2 * ease_io(ph(u, 0.6, 0.9))
    scr = centered_rect(A["c"], z)
    paint(img, PV, [A["c"][0], A["c"][1], 66], static(A["wins"]), focus=A["focus"], scr=scr, dim=0.85)
    d = Draw(img, "RGBA")
    x, y = PV[0] + PV[2] - 330, PV[1] + 40
    glass(d, [x, y, x + 290, y + 64])
    d.text((x + 34, y + 32), chr(SEARCH), font=G["f"]["gm"], anchor="mm", fill=ACCENT)
    d.line([(x + 70, y + 32), (x + 266, y + 32)], fill=rgba(MUTED, 255), width=4)
    kx = x + 70 + 196 * (z / 2.2)
    d.ellipse([kx - 10, y + 22, kx + 10, y + 42], fill=ACCENT)
    return img


def catmull(pts, x):
    n = len(pts) - 1
    k = min(n - 1, int(x * n))
    t = x * n - k
    p0, p1_, p2_, p3 = pts[max(0, k - 1)], pts[k], pts[k + 1], pts[min(n, k + 2)]
    return tuple(0.5 * (2 * p1_[c] + (-p0[c] + p2_[c]) * t + (2 * p0[c] - 5 * p1_[c] + 4 * p2_[c] - p3[c]) * t * t
                        + (-p0[c] + 3 * p1_[c] - 3 * p2_[c] + p3[c]) * t ** 3) for c in range(2))


def pr_pointer(lt, D, b):
    img = card()
    A = G["A"]
    u = lt / D
    scr = A["scr"]
    inn = sorted([w for w in A["wins"] if overlaps(w[2:6], scr)], key=lambda w: w[2])[:6]
    pts = [(scr[0] - 2, scr[1] + scr[3] * 0.8)]
    for k, w in enumerate(inn):
        pts.append((w[2] + w[4] * 0.5, w[3] + w[5] * (0.35 if k % 2 else 0.7)))
    pts.append((scr[0] + scr[2] + 2, scr[1] + scr[3] * 0.3))
    x = ease_io(ph(u, 0.06, 0.94))
    pos = catmull(pts, x)

    def trail(sub, d, to, px):
        for k in range(1, 14):
            q = catmull(pts, clamp(x - k * 0.008))
            tx, ty = to(*q)
            d.ellipse([tx - 3, ty - 3, tx + 3, ty + 3], fill=rgba(ACCENT, 120 - 8 * k))

    paint(img, PV, [A["c"][0], A["c"][1], 42], static(A["wins"]), focus=A["focus"], scr=scr,
          ptr=(pos[0], pos[1], ph(u, 0.03, 0.08)), extra=trail)
    return img


def pr_twice(lt, D, b):
    img = card()
    u = lt / D
    pa = ease_io(ph(u, 0.08, 0.38))
    q = ease_io(ph(pa, 0.25, 0.75))
    r = ease_io(ph(u, 0.42, 0.5))
    h1, h2 = ease_io(ph(u, 0.56, 0.64)), ease_io(ph(u, 0.7, 0.78))
    if u < 0.42:
        ci, cj, ai, bi = lerp(19, 4, pa), 2 - 0.7 * math.sin(math.pi * pa), lerp(4, 11, q), lerp(11, 18, q)
        lift = 0 < pa < 1
    elif u < 0.52:
        ci, cj, ai, bi = lerp(4, 19, r), 2, lerp(11, 4, r), lerp(18, 11, r)
        lift = False
    else:
        ci, cj = lerp(lerp(19, 11, h1), 4, h2), 2
        bi, ai = lerp(11, 18, h1), lerp(4, 11, h2)
        lift = False
    wins = [win(1, "Terminal", ai, 2, 7, 6), win(2, "Browser", bi, 2, 7, 6),
            win(3, "Editor", ci, cj, 7, 6, lift=lift)]
    ga = 0.9 * (ph(u, 0.05, 0.12) * (1 - ph(u, 0.86, 0.95)))
    ptr = (ci + 3.5, cj + 0.5, ph(u, 0.03, 0.08) * (1 - ph(u, 0.4, 0.44))) if u < 0.44 else None
    paint(img, PV, pcam(14.5, 5.0, 32), wins, focus=3, ghosts=[(4, 2, 7, 6, ga)], ptr=ptr)
    d = Draw(img, "RGBA")
    mouse_a = 1.0 if u < 0.44 or u > 0.88 else 0.25
    keyb_a = 1.0 if u > 0.5 else 0.25
    press = max(math.sin(math.pi * ph(u, 0.55, 0.6)), math.sin(math.pi * ph(u, 0.69, 0.74)))
    icons = [(MOUSE, mouse_a)]
    if u > 0.88:
        icons.append(("=", ph(u, 0.88, 0.92)))
    icons.append((KEYB, clamp(keyb_a + press * 0.5)))
    corner_icons(d, icons)
    if u > 0.9:
        a = ph(u, 0.9, 0.95)
        pill(d, PV[0] + PV[2] / 2, PV[1] + PV[3] - 90, "same tiles", icon=CHECK, a=a, font="t", center=True)
    return img


def pr_onehand(lt, D, b):
    img = card()
    u = lt / D
    m = ease_io(ph(u, 0.08, 0.28))
    rz = ease_io(ph(u, 0.34, 0.52))
    pan = ease_io(ph(u, 0.58, 0.72)) - 0.6 * ease_io(ph(u, 0.74, 0.8))
    zm = ease_io(ph(u, 0.82, 0.92)) - 0.5 * ease_io(ph(u, 0.93, 0.99))
    ci, cj = lerp(21, 20, m), lerp(2, 7, m)
    wins = [win(1, "Terminal", 3, 2, 8 + 3 * rz, 6), win(2, "Browser", 11 + 3 * rz, 2, 8, 5, flash=math.sin(math.pi * rz)),
            win(3, "Editor", ci, cj, 6, 4), win(4, "Files", 3, 8, 8, 3)]
    if u < 0.31:
        ptr = (ci + 3, cj + 0.4)
    elif u < 0.55:
        ptr = (11 + 3 * rz - 0.2, 7.8)
    else:
        ptr = (18 - 4 * pan, 6)
    cam = pcam(15 + 5 * pan, 5.5, 32 * (1 + 0.45 * zm))
    paint(img, PV, cam, wins, focus=3 if u < 0.31 else 1, ptr=(ptr[0], ptr[1], ph(u, 0.03, 0.07)))
    d = Draw(img, "RGBA")
    held = 0.6 + 0.4 * math.sin(lt * 6)
    x, y = PV[0] + PV[2] - 40, PV[1] + 34
    corner_icons(d, [(HAND, 1.0), (MOUSE, 1.0)], x=x - 70)
    keycap(d, x - 62, y + 8, 56, 54, held)
    return img


def pr_lefthand(lt, D, b):
    img = card()
    u = lt / D
    steps = [(0.06, "d"), (0.17, "s"), (0.28, "a"), (0.42, "e"), (0.58, "dd"), (0.76, "z")]
    rz = ease_io(ph(u, 0.42, 0.5))
    se = ease_io(ph(u, 0.58, 0.66)) - ease_io(ph(u, 0.76, 0.84))
    focus = 1
    for t_, k in steps[:3]:
        if u >= t_:
            focus = {"d": 2, "s": 5, "a": 4}[k]
    ei = lerp(14, 21, se) if True else 0
    fi = lerp(23, 14, se)
    wins = [win(1, "Terminal", 3, 1, 8, 5), win(2, "Browser", 12, 1, 8, 5), win(3, "Editor", 21, 1, 7, 5),
            win(4, "Files", 3, 7, 8 + 2 * rz, 4, flash=math.sin(math.pi * rz)),
            win(5, "Text", lerp(12, 14, rz) if u < 0.58 else ei, 7, 8, 4),
            win(6, "Window", lerp(21, 23, rz) if u < 0.58 else fi, 7, 7, 4)]
    if u >= 0.58:
        focus = 5
    paint(img, PV, pcam(20.5, 5.6, 42), wins, focus=focus)
    d = Draw(img, "RGBA")
    lit = {}
    combos = {"d": ["super", "d"], "s": ["super", "s"], "a": ["super", "a"], "e": ["super", "shift", "e"],
              "dd": ["super", "shift", "d"], "z": ["super", "z"]}
    for t_, k in steps:
        p = 1 - ph(u, t_ + 0.07, t_ + 0.11) if u >= t_ else 0.0
        for key in combos[k]:
            lit[key] = max(lit.get(key, 0), p)
    keyboard(d, PV[0] + PV[2] - 410, PV[1] + PV[3] / 2 - 100, lit, unit=50)
    return img


def pr_fast(lt, D, b):
    img = card()
    d = Draw(img, "RGBA")
    us = G["op_us"]
    u = lt / D
    glass(d, [PV[0], PV[1], PV[0] + PV[2], PV[1] + PV[3]], 0.9, r=18, edge=70)
    v = us["median"] * ease_out(ph(lt, 0.2, 1.2))
    x0, y0 = PV[0] + 70, PV[1] + 60
    d.text((x0, y0 + 90), "%d µs" % round(v), font=G["f"]["stat"], anchor="lm", fill=ACCENT)
    d.text((x0 + 6, y0 + 215), "median layout operation", font=G["f"]["h2"], anchor="lm", fill=FG)
    d.text((x0 + 6, y0 + 268), "%s real operations, one desktop's log" % format(us["n"], ","), font=G["f"]["t"],
           anchor="lm", fill=FGL)
    hist = us["log2_hist"]
    top = max(hist)
    hx, hy, hw, hh = PV[0] + 980, PV[1] + 60, 620, 300
    grow = ease_out(ph(lt, 0.3, 1.4))
    bw = hw / len(hist)
    for k, c in enumerate(hist):
        bh = hh * c / top * grow
        d.rounded_rectangle([hx + k * bw + 3, hy + hh - bh, hx + (k + 1) * bw - 3, hy + hh], radius=3,
                            fill=rgba(ACCENT if k == 5 else TEAL, 200 if k == 5 else 120))
    mx = hx + math.log2(us["median"]) * bw
    d.line([(mx, hy - 10), (mx, hy + hh)], fill=rgba(FG, 200 * grow), width=2)
    for k, lab in ((0, "1 µs"), (5, "32 µs"), (10, "1 ms"), (15, "32 ms")):
        d.text((hx + (k + 0.5) * bw, hy + hh + 22), lab, font=G["f"]["xs"], anchor="mm", fill=DIM)
    a = ease_out(ph(u, 0.55, 0.68))
    pill(d, PV[0] + 70, PV[1] + PV[3] - 130 + 12 * (1 - a), "disk writes never block the screen", icon=FLASH,
         a=a, font="t", hl=a)
    return img


def pr_kde(lt, D, b):
    img = card()
    u = lt / D
    spec = [("Files", 1, 1, 8, 6), ("Terminal", 10, 1, 7, 4), ("Launcher", 18, 1, 8, 4),
            ("Screenshot", 10, 6, 7, 4), ("Settings", 18, 6, 8, 5), ("Wallet", 1, 8, 8, 3)]
    wins = []
    for k, (kind, i, j, w, h) in enumerate(spec):
        a = ease_back(ph(u, 0.04 + 0.06 * k, 0.14 + 0.06 * k))
        wins.append(win(k + 1, kind, i, j, w, h, a=clamp(a * 1.2), s=0.8 + 0.2 * a))
    paint(img, PV, pcam(13.5, 5.6, 30), wins, focus=5 if u > 0.4 else None)
    d = Draw(img, "RGBA")
    chips = ["KDE apps", "themes", "KWallet", "global shortcuts", "KRunner", "Spectacle", "KDE Connect", "portals"]
    x, y = PV[0] + 10, PV[1] + PV[3] + 14
    for k, c in enumerate(chips):
        a = ease_out(ph(u, 0.3 + 0.06 * k, 0.38 + 0.06 * k))
        x += pill(d, x, y + 10 * (1 - a), c, a=a, font="s", hl=0.3 * a) + 12
    return img


def pr_beauty(lt, D, b):
    img = card()
    u = lt / D
    L1 = [(1, "Terminal", 2, 1, 8, 6), (2, "Browser", 11, 1, 9, 6), (3, "Editor", 21, 1, 7, 10),
          (4, "Files", 2, 8, 8, 4), (5, "Media", 11, 8, 9, 4)]
    L2 = [(1, "Terminal", 21, 1, 7, 10), (2, "Browser", 2, 1, 9, 6), (3, "Editor", 12, 1, 8, 6),
          (4, "Files", 12, 8, 8, 4), (5, "Media", 2, 8, 9, 4)]
    g = ease_back(ph(u, 0.06, 0.36)) - ease_back(ph(u, 0.74, 0.98))
    wins = [win(a[0], a[1], lerp(a[2], c[2], g), lerp(a[3], c[3], g), lerp(a[4], c[4], g), lerp(a[5], c[5], g))
            for a, c in zip(L1, L2)]
    ov = ease_io(ph(u, 0.4, 0.5)) * (1 - ease_io(ph(u, 0.64, 0.74)))
    cam = pcam(15, 6.3, 31 * (1 + 0.35 * ov))
    scr = [cam[0] - 15.5 * (1 + 0.35 * ov) / 1.0, cam[1] - 6.8, 31 * (1 + 0.35 * ov), 13.6] if ov > 0.01 else None
    inner = (1, 0, 28, 12.4, ov) if ov > 0.01 else None
    paint(img, PV, cam, wins, focus=1 if u < 0.5 else 2, scr=scr, inner=inner, tag=False, dim=ov)
    return img


def pr_muscle(lt, D, b):
    img = card()
    u = lt / D
    base = [(1, "Terminal", 2, 1, 8, 6), (2, "Browser", 11, 1, 9, 6), (3, "Editor", 21, 1, 7, 6),
            (4, "Files", 11, 8, 9, 4)]
    sel = 0 if u < 0.08 else (1 if u < 0.15 else 2)
    focus = [1, 2, 3][sel] if u < 0.24 else 3
    t_open = ease_back(ph(u, 0.3, 0.42))
    close_a = 1 - ease_io(ph(u, 0.86, 0.96))
    wins = [win(*w) for w in base]
    wins[3] = win(*base[3], a=close_a, s=0.86 + 0.14 * close_a)
    wins.append(win(5, "Terminal", 21, 8, 7, 4, a=clamp(t_open * 1.2), s=0.85 + 0.15 * t_open))
    if 0.3 < u < 0.86:
        focus = 5
    paint(img, PV, pcam(15, 6.2, 31), wins, focus=focus)
    d = Draw(img, "RGBA")
    sw_a = ph(u, 0.0, 0.03) * (1 - ph(u, 0.22, 0.26))
    if sw_a > 0.01:
        cx, cy = PV[0] + PV[2] / 2, PV[1] + PV[3] / 2
        glass(d, [cx - 420, cy - 110, cx + 420, cy + 110], sw_a)
        for k, w in enumerate(base[:3]):
            x0 = cx - 390 + k * 270
            box = [x0, cy - 80, x0 + 240, cy + 80]
            if k == sel:
                d.rounded_rectangle([box[0] - 8, box[1] - 8, box[2] + 8, box[3] + 8], radius=14,
                                    outline=rgba(ACCENT, 230 * sw_a), width=3)
            window(d, box, w[0], w[1], sw_a, k == sel, 34, {})
    fl = math.sin(math.pi * ph(u, 0.5, 0.58))
    if fl > 0.01:
        img.paste((255, 255, 255), (PV[0], PV[1], PV[0] + PV[2], PV[1] + PV[3]), Image.new("L", (PV[2], PV[3]), int(140 * fl)))
        d = Draw(img, "RGBA")
        d.text((PV[0] + PV[2] / 2, PV[1] + PV[3] / 2), chr(CAMERA), font=G["f"]["gxl"], anchor="mm", fill=rgba(BG, 255 * fl))
    la = ph(u, 0.62, 0.66) * (1 - ph(u, 0.8, 0.84))
    if la > 0.01:
        cx, cy = PV[0] + PV[2] / 2, PV[1] + 150
        glass(d, [cx - 380, cy - 40, cx + 380, cy + 40], la, r=40)
        d.text((cx - 340, cy), chr(SEARCH), font=G["f"]["gm"], anchor="lm", fill=rgba(ACCENT, 255 * la))
        n = int(clamp((u - 0.64) / 0.12) * 7)
        d.text((cx - 290, cy), "terminal"[:n], font=G["f"]["t"], anchor="lm", fill=rgba(FG, 255 * la))
    return img


# ---------------------------------------------------------------- features

def f_grid(lt, D, b):
    A = G["A"]
    img = Image.new("RGB", (W, H))
    cam = cam_for(A["scr"])
    cam[2] *= lerp(1.06, 1.0, ease_out(ph(lt, 0, D)))
    paint(img, FULL, cam, static(A["wins"]), focus=A["focus"], scr=A["scr"],
          grid=ease_io(ph(lt, 0.2, 0.9)), edge_cells=ease_io(ph(lt, 0.9, 1.6)))
    return img


def f_open(lt, D, b):
    img = Image.new("RGB", (W, H))
    frames = frames_of("opens")
    replay_frame(img, sample(frames, lt, D, fit_speed(frames, D, 0.1, 0.3)))
    return img


def f_drag(lt, D, b):
    e = G["drag"]
    img = Image.new("RGB", (W, H))
    u = lt / D
    p = ease_io(ph(u, 0.14, 0.78))
    q = ease_io(ph(p, 0.3, 0.75))
    a, bb = rects(e["before"]), rects(e["after"])
    wid = e["w"]
    wins = []
    for w in tween(e["before"], e["after"], q):
        if w[0] == wid:
            r0, r1 = a[wid], bb[wid]
            lifted = 0 < p < 1
            w = [wid, w[1], lerp(r0[0], r1[0], p), lerp(r0[1], r1[1], p) - 0.5 * math.sin(math.pi * p),
                 lerp(r0[2], r1[2], p), lerp(r0[3], r1[3], p), 0.92 if lifted else 1.0,
                 1.03 if lifted else 1.0, {"lift": lifted}]
            cur = w
        wins.append(w)
    scr = screen_rect(e["view"][0], 0)
    ga = ph(p, 0.45, 0.6) * (1 - ph(u, 0.8, 0.86))
    r1 = bb[wid]
    ptr = (cur[2] + cur[4] / 2, cur[3] + 0.45, ph(u, 0.04, 0.1) * (1 - ph(u, 0.86, 0.92)))
    if u < 0.14:
        r0 = a[wid]
        k = ease_io(ph(u, 0.02, 0.13))
        ptr = (lerp(r0[0] + r0[2] + 3, r0[0] + r0[2] / 2, k), lerp(r0[1] + r0[3] + 2, r0[1] + 0.45, k), ph(u, 0.02, 0.06))
    paint(img, FULL, cam_for(scr), wins, focus=wid, scr=scr, ghosts=[(r1[0], r1[1], r1[2], r1[3], ga)], ptr=ptr)
    return img


def f_resize(lt, D, b):
    e = G["resize"]
    img = Image.new("RGB", (W, H))
    u = lt / D
    a, bb = rects(e["before"]), rects(e["after"])
    wid = e["w"]
    pv = ease_io(ph(u, 0.1, 0.45))
    ap = ease_io(ph(u, 0.55, 0.75))
    r0, r1 = a[wid], bb[wid]
    prev = (lerp(r0[0], r1[0], pv), lerp(r0[1], r1[1], pv), lerp(r0[2], r1[2], pv), lerp(r0[3], r1[3], pv),
            ph(u, 0.06, 0.12) * (1 - ph(u, 0.72, 0.8)))
    moved = [n for n in a if n != wid and n in bb and a[n] != bb[n]]
    ga = ph(u, 0.3, 0.42) * (1 - ph(u, 0.66, 0.74))
    ghosts = [(bb[n][0], bb[n][1], bb[n][2], bb[n][3], ga) for n in moved]
    scr = screen_rect(e["view"][0], 0)
    corner = (lerp(r0[0] + r0[2], r1[0] + r1[2], pv) - 0.3, lerp(r0[1] + r0[3], r1[1] + r1[3], pv) - 0.3,
              ph(u, 0.04, 0.1) * (1 - ph(u, 0.8, 0.86)))
    paint(img, FULL, cam_for(scr), tween(e["before"], e["after"], ap), focus=wid, scr=scr, ghosts=ghosts,
          preview=prev, ptr=corner)
    return img


def f_undo(lt, D, b):
    img = Image.new("RGB", (W, H))
    frames = frames_of("opens")
    n = len(frames) - 1
    u = lt / D
    if u < 0.5:
        k = n * (1 - ease_io(ph(u, 0.05, 0.45)))
    else:
        k = n * ease_io(ph(u, 0.55, 0.95))
    replay_frame(img, frames[int(k)])
    d = Draw(img, "RGBA")
    x0, x1, y = W / 2 - 300, W / 2 + 300, H - 175
    glass(d, [x0 - 30, y - 24, x1 + 30, y + 24], 1.0, r=24)
    d.line([(x0, y), (x1, y)], fill=rgba(MUTED, 255), width=4)
    kx = lerp(x0, x1, k / max(1, n))
    d.line([(x0, y), (kx, y)], fill=rgba(ACCENT, 230), width=4)
    d.ellipse([kx - 9, y - 9, kx + 9, y + 9], fill=ACCENT)
    return img


def f_zoom(lt, D, b):
    A = G["A"]
    img = Image.new("RGB", (W, H))
    u = lt / D
    z = 3.0 * ease_io(ph(u, 0.05, 0.32)) - 1.5 * ease_io(ph(u, 0.4, 0.55)) - 1.5 * ease_out(ph(u, 0.66, 0.74))
    scr = centered_rect(A["c"], z)
    pulse = math.sin(math.pi * ph(u, 0.72, 0.9))
    paint(img, FULL, cam_for(scr), static(A["wins"]), focus=A["focus"], scr=scr, tagpulse=pulse)
    if pulse > 0.01:
        d = Draw(img, "RGBA")
        pill(d, W / 2, 200, "1:1", a=pulse, font="tb", center=True, hl=pulse)
    return img


def f_overview(lt, D, b):
    A = G["A"]
    img = Image.new("RGB", (W, H))
    u = lt / D
    o = ease_io(ph(u, 0.06, 0.36)) * (1 - ease_io(ph(u, 0.84, 1.0)))
    bx, by, bw, bh = A["box"]
    base = A["scr"]
    big = [bx - 1, by - 1, max(bw + 2, (bh + 2) * SCREEN[0] / SCREEN[1]), max(bh + 2, (bw + 2) * SCREEN[1] / SCREEN[0])]
    big[0] = bx + bw / 2 - big[2] / 2
    big[1] = by + bh / 2 - big[3] / 2
    scr = [lerp(base[k], big[k], o) for k in range(4)]
    paint(img, FULL, cam_for(scr), static(A["wins"]), focus=A["focus"], scr=scr,
          inner=(base[0], base[1], base[2], base[3], o))
    return img


def f_switcher(lt, D, b):
    A = G["A"]
    img = Image.new("RGB", (W, H))
    u = lt / D
    inn = sorted([w for w in A["wins"] if overlaps(w[2:6], A["scr"])], key=lambda w: w[2])
    if A["focus"] in [w[0] for w in inn]:
        k0 = [w[0] for w in inn].index(A["focus"])
        inn = inn[k0:] + inn[:k0]
    inn = inn[:5]
    steps = [0.16, 0.3, 0.44]
    sel = sum(1 for s in steps if u >= s) % max(1, len(inn))
    rel = ease_io(ph(u, 0.6, 0.8))
    chosen = inn[sel] if inn else None
    scr = list(A["scr"])
    if chosen and rel > 0:
        cx = chosen[2] + chosen[4] / 2
        target = clamp(cx - scr[2] / 2, chosen[2] + chosen[4] - scr[2] + 1, chosen[2] - 1)
        scr[0] = lerp(scr[0], target, rel)
    focus = chosen[0] if (chosen and u >= 0.6) else A["focus"]
    to = paint(img, FULL, cam_for(scr), static(A["wins"]), focus=focus, scr=scr)
    a = ph(u, 0.04, 0.1) * (1 - ph(u, 0.58, 0.64))
    if a > 0.01 and inn:
        d = Draw(img, "RGBA")
        (sx0, sy0), (sx1, sy1) = to(scr[0], scr[1]), to(scr[0] + scr[2], scr[1] + scr[3])
        cx, cy = (sx0 + sx1) / 2, (sy0 + sy1) / 2
        tw, th, gap = 230, 150, 26
        total = len(inn) * tw + (len(inn) - 1) * gap
        glass(d, [cx - total / 2 - 30, cy - th / 2 - 40, cx + total / 2 + 30, cy + th / 2 + 40], a)
        for k, w in enumerate(inn):
            x0 = cx - total / 2 + k * (tw + gap)
            box = [x0, cy - th / 2, x0 + tw, cy + th / 2]
            if k == sel:
                d.rounded_rectangle([box[0] - 9, box[1] - 9, box[2] + 9, box[3] + 9], radius=14,
                                    outline=rgba(ACCENT, 240 * a), width=3)
            window(d, box, w[0], w[1], a, k == sel, 32, {})
    return img


def f_restore(lt, D, b):
    R = G["restore"]
    img = Image.new("RGB", (W, H))
    u = lt / D
    order = [n for n in R["order"] if n in {w[0] for w in R["wins"]}]
    rest = [w[0] for w in R["wins"] if w[0] not in order]
    seq = order + rest
    scr = screen_rect(R["view"], R["zoom"])
    vis = [w[0] for w in R["wins"] if overlaps(w[2:6], scr, 4)]
    seq = [n for n in seq if n in vis] + [n for n in seq if n not in vis]
    wins, ghosts = [], []
    for w in R["wins"]:
        k = seq.index(w[0])
        t0 = 0.12 + 0.5 * k / max(1, len(vis))
        a = ease_out(ph(u, t0, t0 + 0.14))
        wins.append([w[0], w[1], w[2], w[3], w[4], w[5], a, 0.9 + 0.1 * a, {}])
        ghosts.append((w[2], w[3], w[4], w[5], ph(u, 0.04, 0.1) * (1 - a)))
    inbox = [n for n in seq if any(w[0] == n and overlaps(w[2:6], scr, -2) for w in R["wins"])]
    paint(img, FULL, cam_for(scr), wins, focus=(inbox or seq)[0] if u > 0.3 else None, scr=scr, ghosts=ghosts,
          dip=1 - ease_io(ph(u, 0.0, 0.08)))
    return img


def f_reattach(lt, D, b):
    R = G["restore"]
    img = Image.new("RGB", (W, H))
    u = lt / D
    scr = screen_rect(R["view"], R["zoom"])
    wins = []
    for k, w in enumerate(R["wins"]):
        o = {}
        if w[1] == "Terminal":
            o["fill"] = ease_io(ph(u, 0.08 + 0.05 * (k % 4), 0.5 + 0.05 * (k % 4)))
        elif w[1] == "Browser":
            o["tabs"] = int(1 + 4 * ph(u, 0.35 + 0.03 * (k % 3), 0.8))
            o["fill"] = ph(u, 0.35, 0.6)
        wins.append([w[0], w[1], w[2], w[3], w[4], w[5], 1.0, 1.0, o])
    term = next((w[0] for w in R["wins"] if w[1] == "Terminal" and overlaps(w[2:6], scr)), None)
    paint(img, FULL, cam_for(scr), wins, focus=term, scr=scr)
    return img


def f_lock(lt, D, b):
    A = G["A"]
    img = Image.new("RGB", (W, H))
    u = lt / D
    paint(img, FULL, cam_for(A["scr"]), static(A["wins"]), focus=A["focus"], scr=A["scr"])
    lk = ease_io(ph(u, 0.03, 0.15)) * (1 - ease_io(ph(u, 0.92, 1.0)))
    if lk > 0.01:
        small = img.resize((W // 6, H // 6), Image.BILINEAR).filter(ImageFilter.GaussianBlur(3))
        blur = small.resize((W, H), Image.BILINEAR)
        blur = ImageEnhance.Brightness(blur).enhance(0.55)
        img = Image.blend(img, blur, lk)
    crash = ph(u, 0.45, 0.47) * (1 - ph(u, 0.6, 0.63))
    d = Draw(img, "RGBA")
    cx, cy = W / 2, H / 2 - 30
    ca = lk * (1 - crash)
    if ca > 0.01:
        glass(d, [cx - 230, cy - 170, cx + 230, cy + 170], ca, r=24, edge=120)
        ring = (lt * 90) % 360
        d.arc([cx - 90, cy - 130, cx + 90, cy + 50], ring, ring + 250, fill=rgba(ACCENT, 200 * ca), width=4)
        d.text((cx, cy - 40), chr(LOCK), font=G["f"]["gxl"], anchor="mm", fill=rgba(ACCENT, 255 * ca))
        d.text((cx, cy + 110), "locked", font=G["f"]["h2"], anchor="mm", fill=rgba(FG, 255 * ca))
    if crash > 0.01:
        d.text((cx, cy - 40), chr(LOCK), font=G["f"]["gxl"], anchor="mm", fill=rgba(DIM, 160 * crash))
    na = ph(u, 0.47, 0.5) * (1 - ph(u, 0.84, 0.9))
    if na > 0.01:
        pill(d, cx, cy + 210, "locker restarted · still locked", icon=SHIELD, a=na, font="s", center=True)
    return img


TOOLKITS = [("gtk 3", "gtk", "Files"), ("gtk 4", "gtk", "Text"), ("libadwaita", "adw", "Settings"),
            ("qt", "qt", "Editor"), ("kde", "kde", "Files"), ("browsers", "chromium", "Browser"),
            ("electron", "electron", "Window"), ("x11", "x11", "Terminal")]


def f_toolkits(lt, D, b):
    img = Image.new("RGB", (W, H))
    u = lt / D
    scr = [0, 0, SCREEN[0], SCREEN[1]]
    wins = []
    for k, (label, style, kind) in enumerate(TOOLKITS):
        a = ease_back(ph(u, 0.04 + 0.07 * k, 0.14 + 0.07 * k))
        i, j = 0.8 + (k % 4) * 8.3, 0.3 + (k // 4) * 5.0
        wins.append(win(k + 1, kind, i, j, 7.8, 4.6, a=clamp(a * 1.2), s=0.85 + 0.15 * a, label=label, style=style))
    pulse = math.sin(math.pi * ph(u, 0.7, 0.95))
    paint(img, FULL, cam_for(scr), wins, focus=6 if u > 0.6 else None, scr=scr, tagpulse=pulse)
    return img


def f_kde(lt, D, b):
    A = G["A"]
    img = Image.new("RGB", (W, H))
    u = lt / D
    inn = [w for w in A["wins"] if overlaps(w[2:6], A["scr"])]
    kinds = []
    for w in sorted(inn, key=lambda w: w[2]):
        if w[1] != "Window" and w[1] not in [k[1] for k in kinds]:
            kinds.append(w)
    kinds = kinds[:3] or inn[:3]
    sel = 0 if u < 0.32 else min(len(kinds) - 1, 1)
    chosen = kinds[sel] if kinds else None
    focus = chosen[0] if (chosen and u > 0.45) else A["focus"]
    paint(img, FULL, cam_for(A["scr"]), static(A["wins"]), focus=focus, scr=A["scr"])
    d = Draw(img, "RGBA")
    la = ph(u, 0.04, 0.1) * (1 - ph(u, 0.44, 0.5))
    if la > 0.01 and kinds:
        cx, y0 = W / 2, 250
        glass(d, [cx - 420, y0, cx + 420, y0 + 74 + 64 * len(kinds)], la, r=20, edge=110)
        d.text((cx - 390, y0 + 38), chr(SEARCH), font=G["f"]["gm"], anchor="lm", fill=rgba(ACCENT, 255 * la))
        word = chosen[1].lower() if chosen else ""
        n = int(clamp((u - 0.1) / 0.2) * len(word))
        d.text((cx - 340, y0 + 38), word[:n], font=G["f"]["t"], anchor="lm", fill=rgba(FG, 255 * la))
        for k, w in enumerate(kinds):
            yy = y0 + 80 + 64 * k
            if k == sel:
                d.rounded_rectangle([cx - 400, yy, cx + 400, yy + 56], radius=12, fill=rgba(SELECTION, 230 * la),
                                    outline=rgba(ACCENT, 150 * la))
            color, icon = KINDS.get(w[1], KINDS["Window"])
            d.text((cx - 370, yy + 28), chr(icon), font=G["f"]["g"], anchor="lm", fill=rgba(color, 255 * la))
            d.text((cx - 320, yy + 28), w[1].lower() + " window", font=G["f"]["t"], anchor="lm", fill=rgba(FGL, 255 * la))
    chips = [(SEARCH, "KRunner window search"), (KEYB, "global shortcuts"), (WAVES, "KDE Connect input"),
             (SHIELD, "Inhibit and desktop portals")]
    x = W / 2 - 860
    for k, (g, c) in enumerate(chips):
        a = ease_out(ph(u, 0.45 + 0.08 * k, 0.53 + 0.08 * k))
        x += pill(d, x, 830 + 10 * (1 - a), c, icon=g, a=a, font="s", hl=0.3 * a) + 18
    return img


def f_vm(lt, D, b):
    img = card()
    d = Draw(img, "RGBA")
    u = lt / D
    suites = ["smoke", "gaps", "remote input", "backends", "two GPUs"]
    cols = ["newm-rs"]
    x0, y0 = 590, 220
    glass(d, [x0 - 60, y0 - 60, x0 + 800, y0 + 120 + 92 * len(suites)], 0.95, r=20, edge=80)
    for c, name in enumerate(cols):
        d.text((x0 + 650, y0 + 10), name, font=G["f"]["tb"], anchor="mm", fill=FG)
    for r, s in enumerate(suites):
        y = y0 + 90 + r * 92
        d.text((x0, y), s, font=G["f"]["t"], anchor="lm", fill=FGL)
        for c in range(len(cols)):
            k = r * len(cols) + c
            a = ease_back(ph(u, 0.08 + 0.06 * k, 0.16 + 0.06 * k))
            cx = x0 + 650
            d.rounded_rectangle([cx - 46, y - 30, cx + 46, y + 30], radius=12, fill=rgba(LIGHTER, 230),
                                outline=rgba(ACCENT, 60 + 160 * clamp(a)))
            if a > 0.01:
                f = ImageFont.truetype(G["f"]["gm"].path, max(8, int(36 * clamp(a, 0, 1.3))))
                d.text((cx, y), chr(CHECK), font=f, anchor="mm", fill=rgba(ACCENT, 255 * clamp(a)))
    return img


SCENES = {"title": opening, "flyin": opening, "flyin2": opening, "close": close,
          "p1": pr_overlap, "p2": pr_display, "p3": pr_zoom, "p4": pr_focus, "p5": pr_pointer,
          "p6": pr_twice, "p7": pr_onehand, "p8": pr_lefthand, "p9": pr_fast, "p10": pr_kde,
          "p11": pr_beauty, "p12": pr_muscle,
          "f_grid": f_grid, "f_open": f_open, "f_drag": f_drag, "f_resize": f_resize, "f_undo": f_undo,
          "f_zoom": f_zoom, "f_overview": f_overview, "f_switcher": f_switcher, "f_restore": f_restore,
          "f_reattach": f_reattach, "f_lock": f_lock, "f_toolkits": f_toolkits, "f_kde": f_kde,
          "f_vm": f_vm}


# ---------------------------------------------------------------- overlays and transitions

def caption(d, text, lt, D):
    if not text:
        return
    a = ease_out(ph(lt, 0.08, 0.32)) * (1 - ph(lt, D - 0.18, D))
    if a <= 0.01:
        return
    f = G["f"]["cap"]
    tw = d.textlength(text, font=f)
    y = H - 118 + 14 * (1 - a)
    d.rounded_rectangle([W / 2 - tw / 2 - 30, y, W / 2 + tw / 2 + 30, y + 64], radius=32,
                        fill=(11, 12, 22, int(220 * a)), outline=rgba(ACCENT, 90 * a), width=1)
    d.text((W / 2, y + 32), text, font=f, anchor="mm", fill=rgba(FG, 255 * a))


def header(img, b, lt):
    d = Draw(img, "RGBA")
    a = ease_out(ph(lt, 0.0, 0.35))
    dx = -50 * (1 - a)
    num = "%02d" % int(b["id"][1:])
    d.text((120 + dx, 200), num, font=G["f"]["num"], anchor="lm", fill=rgba(ACCENT, 255 * a))
    lines = wrap(d, b["caption"], G["f"]["h1"], 1440)
    y = 200 - (len(lines) - 1) * 35
    for k, line in enumerate(lines):
        d.text((330 + dx * 1.6, y + k * 70), line, font=G["f"]["h1"], anchor="lm", fill=rgba(FG, 255 * a))
    d.line([(300 + dx, 140), (300 + dx, 260)], fill=rgba(ACCENT, 120 * a), width=2)


def section_tag(d, b, lt):
    a = ease_out(ph(b["st"] + lt, 0.0, 0.4))
    pill(d, 56, 44, GROUPS.get(b["group"], ""), icon=LOGO, a=a, font="tb")


def compose(k, lt):
    b = G["tl"]["beats"][k]
    lt = clamp(lt, 0.0, b["dur"])
    img = SCENES[b["scene"]](lt, b["dur"], b)
    if b["section"] == "principles":
        header(img, b, lt)
    elif b["section"] in ("features", "open"):
        d = Draw(img, "RGBA")
        if b["section"] == "features":
            section_tag(d, b, lt)
        caption(d, b["caption"], lt, b["dur"])
    return img


def frame_at(t):
    beats = G["tl"]["beats"]
    k = max(0, bisect.bisect_right(G["t0s"], t) - 1)
    b = beats[k]
    lt = t - b["t0"]
    img = compose(k, lt)
    if k == 0:
        return img
    prev = beats[k - 1]
    if prev["group"] == b["group"] and b["section"] == "open":
        return img
    wipe = prev["section"] != b["section"] or prev["group"] != b["group"]
    td = 0.42 if wipe else 0.28
    if lt >= td:
        return img
    old = compose(k - 1, prev["dur"])
    x = ease_io(lt / td)
    if not wipe:
        return Image.blend(old, img, x)
    edge = lerp(-260, W + 40, x)
    mask = Image.new("L", (W, H), 0)
    ImageDraw.Draw(mask).polygon([(0, 0), (edge + 220, 0), (edge, H), (0, H)], fill=255)
    out = Image.composite(img, old, mask)
    d = Draw(out, "RGBA")
    d.line([(edge + 220, 0), (edge, H)], fill=rgba(ACCENT, 230), width=4)
    d.line([(edge + 228, 0), (edge + 8, H)], fill=rgba(ACCENT, 70), width=10)
    return out


def render_frame(f):
    return frame_at(f / FPS).tobytes()


# ---------------------------------------------------------------- the loop

LOOP_S, LOOP_FPS, BACK_S = 12.0, 30, 0.9


def loop_frames():
    frames = frames_of("stretch")
    n_main = int((LOOP_S - BACK_S) * LOOP_FPS)
    n_back = int(BACK_S * LOOP_FPS)
    out = []
    for k in range(n_main):
        out.append(("main", int(k * (len(frames) - 1) / (n_main - 1))))
    for k in range(n_back):
        out.append(("back", (k + 1) / (n_back + 1)))
    return out


def render_loop(item):
    kind, v = item
    frames = frames_of("stretch")
    img = Image.new("RGB", (W, H))
    if kind == "main":
        replay_frame(img, frames[v])
    else:
        x = ease_io(v)
        a, z = frames[-1], frames[0]
        wins = tween([w[:6] for w in a[0] if w[6] > 0.5], [w[:6] for w in z[0] if w[6] > 0.5], x)
        view = [lerp(a[2][k], z[2][k], x) for k in range(2)]
        replay_frame(img, (wins, z[1] if x > 0.5 else a[1], view, lerp(a[3], z[3], x), 0.0))
    return img.resize((960, 540), Image.LANCZOS).tobytes()


# ---------------------------------------------------------------- main

SW, SH, SCOLS = 640, 360, 4


def sheet_item(t):
    img = frame_at(t).resize((SW, SH), Image.LANCZOS)
    d = Draw(img, "RGBA")
    d.rectangle([0, 0, 92, 22], fill=(0, 0, 0, 180))
    d.text((6, 11), "%.1fs" % t, font=G["f"]["xs"], anchor="lm", fill=FG)
    return img.tobytes()


def main(argv):
    mode, paths = argv[1], argv[2:7]
    tl = json.load(open(paths[0]))
    if mode == "sheet":
        out, times = argv[7], [float(t) for t in argv[8:]]
        with Pool(min(WORKERS, len(times)), initializer=init, initargs=(paths,)) as pool:
            tiles = pool.map(sheet_item, times)
        rows = (len(tiles) + SCOLS - 1) // SCOLS
        sheet = Image.new("RGB", (SCOLS * SW, rows * SH))
        for k, raw in enumerate(tiles):
            sheet.paste(Image.frombytes("RGB", (SW, SH), raw), ((k % SCOLS) * SW, (k // SCOLS) * SH))
        sheet.save(out)
        return
    if mode == "stills":
        out, times = argv[7], [float(t) for t in argv[8:]]
        init(paths)
        for t in times:
            frame_at(t).save("%s/still-%06.2f.png" % (out, t))
        return
    out = sys.stdout.buffer
    if mode == "loop":
        init(paths)
        items = loop_frames()
        with Pool(WORKERS, initializer=init, initargs=(paths,)) as pool:
            for raw in pool.imap(render_loop, items, chunksize=4):
                out.write(raw)
        return
    a = float(argv[7]) if len(argv) > 7 else 0.0
    z = float(argv[8]) if len(argv) > 8 else tl["end"]
    with Pool(WORKERS, initializer=init, initargs=(paths,)) as pool:
        for raw in pool.imap(render_frame, range(int(a * FPS), int(z * FPS)), chunksize=4):
            out.write(raw)


if __name__ == "__main__":
    main(sys.argv)
