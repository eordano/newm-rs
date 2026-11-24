"""Lay the narration clips (timing.json) on a beat grid for the renderer, the music and the mux.

Each segment becomes a beat: a little air, the clip's speech, a little air, with per-section
minimums. Beats snap to the music's 120 BPM grid, and every section starts on a bar and every
feature group on a half bar, so the cuts land on the music's downbeats. Writes out/timeline.json.

usage: python3 -I timeline.py
"""

import json
import math
import os

HERE = os.path.dirname(os.path.abspath(__file__))
BPM = 120
BEAT = 60 / BPM
BAR = 4 * BEAT
PADS = {  # section: (air before, air after, minimum, snap)
    "open": (0.1, 0.25, 2.0, BEAT),
    "principles": (0.3, 0.45, 4.5, BEAT),
    "features": (0.12, 0.25, 2.5, BEAT),
    "close": (0.4, 2.6, 5.0, BAR),
}
TITLE_PRE = 1.4  # the intro rise before the first word


def group(seg):
    return seg["section"] if seg["section"] != "features" else seg["id"].rstrip("0123456789")


def main():
    spec = json.load(open(os.path.join(HERE, "narration.json")))
    timing = json.load(open(os.path.join(HERE, "timing.json")))
    beats, t = [], 0.0
    for k, seg in enumerate(spec["segments"]):
        tm = timing[seg["id"]]
        lead, tail = tm["speech"]
        pre, post, low, snap = PADS[seg["section"]]
        if k == 0:
            pre = TITLE_PRE
        if beats and group(seg) != beats[-1]["group"]:
            grid = BAR if seg["section"] != beats[-1]["section"] else 2 * BEAT
            end = math.ceil((t - 1e-6) / grid) * grid
            beats[-1]["dur"] = round(beats[-1]["dur"] + end - t, 3)
            t = end
        dur = math.ceil(max(low, pre + (tail - lead) + post) / snap - 1e-6) * snap
        beats.append(dict(seg, group=group(seg), t0=round(t, 3), dur=round(dur, 3),
                          voice_at=round(t + pre - lead, 3), clip=tm["dur"]))
        t += dur
    sections = []
    for b in beats:
        if not sections or sections[-1]["group"] != b["group"]:
            sections.append({"section": b["section"], "group": b["group"], "t0": b["t0"]})
    out = {"bpm": BPM, "fps": 60, "end": round(t, 3), "beats": beats, "sections": sections}
    os.makedirs(os.path.join(HERE, "out"), exist_ok=True)
    json.dump(out, open(os.path.join(HERE, "out", "timeline.json"), "w"), indent=1)
    for b in beats:
        print("%-9s %6.2f %5.2f  %s" % (b["id"], b["t0"], b["dur"], b["section"]))
    print("end %.2f s" % t)


if __name__ == "__main__":
    main()
