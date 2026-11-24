#!/usr/bin/env python3
"""Sums up newm-rs's usage log (usage.jsonl): where new windows' spans came from, how fast they
settle, apps growing their windows, what happened during drags, float decisions, session
restore, camera motion and quick corrections.

Usage: usage-summary.py [usage.jsonl]   (default: $XDG_STATE_HOME/newm-rs/usage.jsonl)

Python standard library only.
"""

import json
import os
import sys
from collections import Counter, defaultdict

CORRECTION_MS = 5000
CORRECTIONS = {"resize", "undo", "cancel"}
NOT_OPS = {"motion", "settle", "unsettled", "float", "restore-expired"}


def default_path():
    state = os.environ.get("XDG_STATE_HOME") or os.path.join(
        os.environ.get("HOME", ""), ".local", "state"
    )
    return os.path.join(state, "newm-rs", "usage.jsonl")


def read(path):
    lines, bad = [], 0
    with open(path, encoding="utf-8") as f:
        for raw in f:
            try:
                lines.append(json.loads(raw))
            except ValueError:
                bad += 1
    return lines, bad


def percentiles(values):
    if not values:
        return "none"
    v = sorted(values)

    def at(p):
        return v[min(len(v) - 1, int(p / 100 * len(v)))]

    return f"n={len(v)} p50={at(50)} p90={at(90)} p99={at(99)} max={v[-1]}"


def section(title):
    print(f"\n== {title}")


def show(counter, indent="  "):
    if not counter:
        print(f"{indent}none")
    for key, n in counter.most_common():
        print(f"{indent}{n:6}  {key}")


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else default_path()
    lines, bad = read(path)
    print(f"{path}: {len(lines)} lines" + (f", {bad} unreadable" if bad else ""))
    dropped = sum(l.get("dropped", 0) for l in lines)
    if dropped:
        print(f"lines dropped by a full writer queue: {dropped}")

    by_op = defaultdict(list)
    for l in lines:
        by_op[l.get("op")].append(l)

    section("size sources of new windows (per axis when they differ)")
    sources = Counter()
    for l in by_op["open"]:
        s = l.get("size_source")
        if isinstance(s, dict):
            sources.update(f"{axis}:{src}" for axis, src in sorted(s.items()))
        elif s is not None:
            sources[s] += 1
    show(sources)

    section("configures until a new window shows its cell")
    print("  configures " + percentiles([l["configures"] for l in by_op["settle"]]))
    print("  ms         " + percentiles([l["ms"] for l in by_op["settle"]]))
    unsettled = Counter(l.get("app") for l in by_op["unsettled"])
    print(f"  unsettled after 5 s: {sum(unsettled.values())}")
    show(unsettled, "    ")

    section("app-grow by app (cause; pushed = the rules found no layout)")
    grows = Counter()
    for l in by_op["app-grow"]:
        grows[f"{l.get('app')} ({l.get('cause')}{', pushed' if l.get('pushed') else ''})"] += 1
    show(grows)

    section("ops logged during a drag")
    show(Counter(l.get("op") for l in lines if l.get("during_drag")))
    drops = by_op["drop"]
    moved = sum(1 for l in drops if l.get("home_moved"))
    print(f"  drops whose window's own tiles moved during the drag: {moved} of {len(drops)}")

    section("float decisions")
    show(Counter(l.get("decision") for l in by_op["float"]))
    huge = [l for l in by_op["float"] if l.get("max_huge")]
    print(f"  max_huge: {len(huge)} ({sum(1 for l in huge if l.get('x11'))} X11)")
    show(Counter(f"{l.get('app')} -> {l.get('decision')}" for l in huge), "    ")

    section("session restore")
    restored = [l["restore"] for l in by_op["open"] if isinstance(l.get("restore"), dict)]
    show(Counter(r.get("matched_by") for r in restored))
    print("  waited ms  " + percentiles([r.get("waited_ms", 0) for r in restored]))
    expired = Counter(l.get("app") for l in by_op["restore-expired"])
    print(f"  placeholders expired: {sum(expired.values())}")
    show(expired, "    ")

    section("camera motion per kind (max jump px, max speed px/s, max acceleration px/s^2)")
    kinds = defaultdict(lambda: [0.0, 0.0, 0.0])
    for l in by_op["motion"]:
        cam = l.get("camera") or {}
        k = kinds[l.get("kind")]
        for i, field in enumerate(("max_jump", "max_speed", "max_accel")):
            k[i] = max(k[i], cam.get(field) or 0.0)
    if not kinds:
        print("  none")
    for kind, (jump, speed, accel) in sorted(kinds.items(), key=lambda kv: str(kv[0])):
        print(f"  {kind!s:16} jump {jump:10.1f}  speed {speed:10.1f}  accel {accel:12.1f}")

    section(f"corrections: a resize, undo or cancel within {CORRECTION_MS // 1000} s of the op before")
    ops = sorted(
        (l for l in lines if l.get("op") not in NOT_OPS and "t" in l),
        key=lambda l: (l["t"], l.get("seq", 0)),
    )
    corrections = Counter()
    for prev, cur in zip(ops, ops[1:]):
        if cur.get("op") in CORRECTIONS and cur["t"] - prev["t"] <= CORRECTION_MS:
            corrections[f"{cur['op']} after {prev.get('op')}"] += 1
    show(corrections)

    section("op time (op_us)")
    print("  " + percentiles([l["op_us"] for l in lines if "op_us" in l]))


if __name__ == "__main__":
    main()
