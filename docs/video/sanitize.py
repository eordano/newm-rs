"""Strip the newm-rs usage log down to the window geometry the README video replays.

Reads, per window op: the op kind, seconds since the first op, the acted-on window, every
window's tile rectangle and a generic kind instead of its app id, the camera (view in tiles,
zoom) and, for an open, whether it restored a window from the last session. Drops app ids,
titles, sizes in pixels, rules, times of day and everything else. Of those ops it writes only
what render.py replays: the busiest on-screen run of 12 actions, a run of 7 with two fresh
opens, the clearest drop and resize, and the largest burst of restored windows. The layout
timings (op_us) reach the output only as a summary: their median, count and a log-scale
histogram.

usage: python3 -I sanitize.py USAGE.jsonl OUT.json
"""

import json
import math
import statistics
import sys

KINDS = [  # (kind, substrings of a lowercased app id); first match wins
    ("Screenshot", ("spectacle", "flameshot", "screenshot", "satty", "swappy", "ksnip")),
    ("Terminal", ("alacritty", "kitty", "foot", "ghostty", "konsole", "terminal", "wezterm", "xterm",
                  "terminator", "tilix", "urxvt", "contour", "ptyxis", "blackbox", "rio")),
    ("Browser", ("firefox", "chromium", "chrome", "brave", "vivaldi", "librewolf", "epiphany", "falkon",
                 "qutebrowser", "opera", "edge", "floorp", "zen-browser", "waterfox")),
    ("Text", ("kwrite", "mousepad", "featherpad", "gedit", "text-editor", "notepad")),
    ("Editor", ("code", "codium", "zed", "kate", "emacs", "neovide", "sublime", "jetbrains", "idea",
                "pycharm", "kdevelop", "lapce", "helix", "geany")),
    ("Files", ("dolphin", "nautilus", "nemo", "thunar", "pcmanfm", "caja", "ark", "file-roller",
               "filemanager", "krusader")),
    ("Media", ("vlc", "mpv", "celluloid", "totem", "haruna", "smplayer", "elisa", "rhythmbox", "spotify")),
]
OPS = {"open", "close", "move", "drop", "resize", "send-to-edge", "swap", "gather", "undo",
       "app-grow"}
TILE = (119.0, 113.0)
SCREEN = (4096 / TILE[0], 1152 / TILE[1])  # the logical screen, in tiles
TR = TILE[1] / TILE[0]
CUT, BURST = 1.3, 0.4
ACT = ("s", "end", "w", "before", "after", "view", "zoom")


def kind(app):
    a = (app or "").lower()
    return next((k for k, pats in KINDS if any(p in a for p in pats)), "Window")


def screen_rect(view, zoom):
    s = 0.8 ** zoom
    return [view[0], view[1], SCREEN[0] / s, SCREEN[1] / s]


def coalesce(events):
    out = []
    for e in events:
        p = out[-1] if out else None
        if (p and e["op"] == p["op"] and e["op"] in ("move", "resize") and e["w"] == p["w"]
                and e["s"] - p["end"] < BURST):
            p.update(after=e["after"], end=e["s"], view=[p["view"][0], e["view"][1]],
                     zoom=[p["zoom"][0], e["zoom"][1]])
        else:
            out.append(dict(e, end=e["s"]))
    return out


def far(v0, z0, v1, z1):
    width = SCREEN[0] / 0.8 ** max(z0, z1)
    return math.hypot(v1[0] - v0[0], (v1[1] - v0[1]) * TR) > CUT * width


def rects(layout):
    return {w[0]: tuple(w[2:6]) for w in layout}


def changed(e):
    a, b = rects(e["before"]), rects(e["after"])
    return sum(1 for n in set(a) | set(b) if a.get(n) != b.get(n))


def overlaps(r, scr, m=0.0):
    x, y, sw, sh = scr
    return r[0] < x + sw + m and r[0] + r[2] > x - m and r[1] < y + sh + m and r[1] + r[3] > y - m


def inside(r, scr, m=0.0):
    x, y, sw, sh = scr
    return r[0] >= x - m and r[1] >= y - m and r[0] + r[2] <= x + sw + m and r[1] + r[3] <= y + sh + m


def visible(layout, view, zoom=0):
    scr = screen_rect(view, zoom)
    return [w for w in layout if overlaps(w[2:6], scr)]


def best_stretch(acts, n, opens=0, skip=()):
    best = None
    for i in range(len(acts) - n):
        if i in skip:
            continue
        seq = acts[i:i + n]
        if any(e["zoom"] != [0, 0] for e in seq):
            continue
        if any(far(e["view"][0], 0, e["view"][1], 0) for e in seq):
            continue
        if any(far(seq[k - 1]["view"][1], 0, seq[k]["view"][0], 0) for k in range(1, n)):
            continue
        if opens and sum(1 for e in seq if e["op"] == "open" and not e.get("restore")) < opens:
            continue
        vis = [len(visible(e["after"], e["view"][1])) for e in seq]
        if min(vis) < 3:
            continue
        acted = [e for e in seq if e["w"] is not None and any(
            w[0] == e["w"] and overlaps(w[2:6], screen_rect(e["view"][1], 0), 2) for w in e["after"] + e["before"])]
        score = (sum(min(5, changed(e)) for e in seq) + 2 * len({e["w"] for e in seq})
                 + 3 * sum(min(v, 7) for v in vis) / n + 2 * len(acted))
        if best is None or score > best[0]:
            best = (score, i)
    return best[1]


def best_event(acts, op):
    best = None
    for e in acts:
        if e["op"] != op or e["zoom"] != [0, 0] or far(e["view"][0], 0, e["view"][1], 0):
            continue
        scr = screen_rect(e["view"][0], 0)
        a, b = rects(e["before"]), rects(e["after"])
        if e["w"] not in a or e["w"] not in b:
            continue
        moved = [n for n in a if n != e["w"] and n in b and a[n] != b[n]]
        if not moved:
            continue
        rs = [a[n] for n in moved] + [b[n] for n in moved] + [a[e["w"]], b[e["w"]]]
        if not all(inside(r, scr, 1.0) for r in rs):
            continue
        aw, bw = a[e["w"]], b[e["w"]]
        disp = sum(abs(aw[k] - bw[k]) for k in range(4))
        score = 3 * min(len(moved), 4) + min(disp, 12) + 0.5 * min(len(visible(e["before"], e["view"][0])), 8)
        if best is None or score > best[0]:
            best = (score, e)
    return best[1]


def only(e, keys):
    return {k: e[k] for k in keys}


def pick(evs):
    acts = coalesce(evs)
    i = best_stretch(acts, 12)
    j = best_stretch(acts, 7, opens=2, skip=set(range(i - 6, i + 12)))
    out = {"stretch": [only(e, ACT) for e in acts[i:i + 12]], "opens": [only(e, ACT) for e in acts[j:j + 7]],
           "drag": only(best_event(acts, "drop"), ACT[2:6]), "resize": only(best_event(acts, "resize"), ACT[2:6])}
    rs = [k for k, e in enumerate(evs) if e.get("restore")]
    bursts, cur = [], []
    for k in rs:
        if cur and evs[k]["s"] - evs[cur[-1]]["s"] > 20:
            bursts.append(cur)
            cur = []
        cur.append(k)
    if cur:
        bursts.append(cur)
    good = []
    for bu in bursts:
        last = evs[bu[-1]]
        v, z = last["view"][1], last["zoom"][1]
        if len(visible(last["after"], v, z)) >= 3:
            good.append(bu)
    if good:
        bu = max(good, key=len)
        last = evs[bu[-1]]
        out["restore"] = {"wins": last["after"], "view": last["view"][1], "zoom": last["zoom"][1],
                          "order": [evs[k]["w"] for k in bu]}
    return out


def main(src, dst):
    events, ids, t0, us = [], {}, None, []
    for line in open(src):
        try:
            e = json.loads(line)
        except ValueError:
            continue
        if e.get("op") not in OPS or "t" not in e:
            continue
        t0 = e["t"] if t0 is None else t0
        if isinstance(e.get("op_us"), (int, float)):
            us.append(e["op_us"])

        def wins(key):
            out = []
            for w in e.get(key, []):
                n = ids.setdefault(w["id"], len(ids) + 1)
                out.append([n, kind(w.get("app"))] + list(w["tiles"]))
            return out

        cam = e.get("camera", {})

        def view(key):
            x, y = cam.get(key, [0.0, 0.0])
            return [round(x / TILE[0], 3), round(y / TILE[1], 3)]

        ev = {
            "s": round((e["t"] - t0) / 1000, 2),
            "op": e["op"],
            "w": ids.setdefault(e["window"], len(ids) + 1) if "window" in e else None,
            "before": wins("before"),
            "after": wins("after"),
            "view": [view("before"), view("after")],
            "zoom": cam.get("zoom", [0, 0]),
        }
        if e["op"] == "open" and e.get("restore"):
            ev["restore"] = True
        events.append(ev)
    hist = [0] * 16
    for v in us:
        hist[max(0, min(15, int(math.log2(max(1, v)))))] += 1
    summary = {"median": statistics.median(us) if us else 0, "n": len(us), "log2_hist": hist}
    json.dump(dict(pick(events), op_us=summary), open(dst, "w"), separators=(",", ":"))
    print(len(events), "events,", len(ids), "windows,", events[-1]["s"] if events else 0, "s,",
          "op_us median", summary["median"], "over", summary["n"])


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
