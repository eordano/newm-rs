# TEST, COMPOSITOR, IPC, UNIT, PROCESS, XFAIL and STRICT come from compat/default.nix.
import json
import os
import re
import shlex
import time

from PIL import Image

OUT = os.environ.get("out", ".")
results = []
SCALE = 1.25


class Fail(Exception):
    pass


def sh(cmd, timeout=60):
    return machine.succeed(cmd, timeout=timeout)


def alice(cmd, timeout=60):
    """A shell command as alice inside her session's environment."""
    return machine.succeed("as-alice " + shlex.quote(cmd), timeout=timeout)


def alice_try(cmd, timeout=60):
    return machine.execute("as-alice " + shlex.quote(cmd), timeout=timeout)


def cat(path):
    return alice_try(f"cat {path}")[1]


def msg(*args):
    return alice(f"{IPC} msg " + " ".join(shlex.quote(str(a)) for a in args))


def js(what):
    return json.loads(alice(f"{IPC} msg --json {what}"))


def windows():
    return js("windows")


def spawn(cmd):
    """Starts cmd through the compositor, so it gets the session's WAYLAND_DISPLAY and DISPLAY."""
    msg("action", "spawn-sh", "--", cmd)


def wait(pred, what, timeout=15):
    end = time.time() + timeout
    last = None
    while time.time() < end:
        try:
            last = pred()
            if last:
                return last
        except Exception as e:
            last = e
        time.sleep(0.5)
    raise Fail(f"timed out waiting for {what} (last: {last!r})")


def expect(cond, what):
    if not cond:
        raise Fail(what)


def journal():
    return sh(f"journalctl -b -q -o cat --no-pager _SYSTEMD_USER_UNIT={UNIT}")


def new_window(cmd, what, timeout=90, match=None):
    """Spawns cmd and returns the first window that was not there before (or that matches)."""
    before = {w["id"] for w in windows()}

    def found():
        for w in windows():
            if w["id"] in before:
                continue
            text = f"{w.get('app_id') or ''} {w.get('title') or ''}".lower()
            if match is None or match.lower() in text:
                return w
        return None

    spawn(cmd)
    return wait(found, f"{what}'s window", timeout)


def get(wid):
    for w in windows():
        if w["id"] == wid:
            return w
    return None


def focus(w):
    msg("action", "focus-window", "--id", w["id"])
    wait(lambda: (get(w["id"]) or {}).get("is_focused"), f"window {w['id']} focused")


def close(w, pattern=None):
    try:
        msg("action", "close-window", "--id", w["id"])
        wait(lambda: get(w["id"]) is None, f"window {w['id']} closed", 10)
    except Exception:
        pass
    if pattern:
        alice_try(f"pkill -f {shlex.quote('[' + pattern[0] + ']' + pattern[1:])}")
        try:
            wait(lambda: get(w["id"]) is None, f"window {w['id']} gone", 10)
        except Fail:
            pass


def title_when(w, pred, what, timeout=15):
    """Waits until pred(title) holds for w's title and returns the title."""
    def title():
        t = (get(w["id"]) or {}).get("title") or ""
        return pred(t) and t
    return wait(title, what, timeout)


def type_text(text):
    sh("uin " + shlex.quote(f"type {text}; sleep 100; tap enter"), timeout=30)


def jlines(path):
    """The JSON lines a probe appended to path (missing file: none)."""
    rc, text = alice_try(f"cat {path} 2>/dev/null")
    outl = []
    for line in text.splitlines():
        try:
            outl.append(json.loads(line))
        except ValueError:
            pass
    return outl


def read_json(path, timeout=20):
    return json.loads(wait(lambda: cat(path).strip() or None, path, timeout))


def shot(name, output=None):
    """A grim screenshot (physical pixels, of one output if given) kept as shots/NAME.png; returns
    it as a PIL image."""
    alice(f"grim {'-o ' + shlex.quote(output) + ' ' if output else ''}/tmp/shots/{name}.png")
    machine.copy_from_vm(f"/tmp/shots/{name}.png", "shots")
    return Image.open(os.path.join(OUT, "shots", f"{name}.png")).convert("RGB")


def drew(before, after, min_changed=0.015, min_colours=6):
    """Did a window draw content: enough pixels changed, in more than a handful of colours."""
    w, h = after.size
    n = diff = 0
    colours = set()
    for y in range(0, h, 6):
        for x in range(0, w, 6):
            n += 1
            pa, pb = before.getpixel((x, y)), after.getpixel((x, y))
            if max(abs(pa[i] - pb[i]) for i in range(3)) > 24:
                diff += 1
                colours.add(tuple(v // 16 for v in pb))
    return diff / max(n, 1), len(colours), diff / max(n, 1) >= min_changed and len(colours) >= min_colours


def scan_box(img, keep, step, min_n=1):
    """The bounding box of the pixels (x, y) on a step grid where keep(x, y), or None under min_n."""
    w, h = img.size
    pts = [(x, y) for y in range(0, h, step) for x in range(0, w, step) if keep(x, y)]
    if len(pts) < min_n:
        return None
    xs, ys = zip(*pts)
    return min(xs), min(ys), max(xs), max(ys)


def hexrgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def color_box(img, colour, tol=14, step=4, min_n=30):
    """Bounding box (x0, y0, x1, y1) of `colour` in a shot, or None."""
    want = hexrgb(colour) if isinstance(colour, str) else colour
    return scan_box(img, lambda x, y: all(abs(p - q) < tol for p, q in zip(img.getpixel((x, y)), want)),
                    step, min_n)


def green_box(img):
    """The bounding box of the probes' #30c030 in a shot, or None."""
    return color_box(img, (0x30, 0xC0, 0x30), tol=12, min_n=50)


def changed_box(a, b, thr=24, step=3):
    return scan_box(b, lambda x, y: max(abs(p - q) for p, q in zip(a.getpixel((x, y)), b.getpixel((x, y)))) > thr,
                    step)


REQ = re.compile(r"(->\s+)?([a-z_0-9]+)[@#](\d+)\.([a-z_0-9]+)\((.*)\)\s*$")
NEWID = re.compile(r"new id (?:[a-z_0-9]+)[@#](\d+)")
OBJ = re.compile(r"[a-z_0-9]+[@#](\d+)")


def wl_commits(log):
    """A WAYLAND_DEBUG log (libwayland's interface#id or older interface@id) as its committed
    buffers, (surface, buffer size, viewport destination, buffer scale, the first scale offered
    to the surface before the commit), the fractional scales offered per surface, and the
    toplevel: the surface with the largest committed area."""
    buffers, params, attached, dest, scale, prefs, seen = {}, {}, {}, {}, {}, {}, {}
    viewport, frac = {}, {}
    commits = []
    for line in cat(log).splitlines():
        m = REQ.search(line)
        if not m:
            continue
        _, iface, oid, op, args = m.groups()
        nums = [int(v) for v in re.findall(r"(?<![#@\w])-?\d+", args)]
        newid = NEWID.search(args)
        objs = OBJ.findall(args)
        if iface == "wl_shm_pool" and op == "create_buffer" and newid and len(nums) >= 3:
            buffers[newid.group(1)] = (nums[1], nums[2])
        elif iface == "zwp_linux_buffer_params_v1" and op == "create_immed" and newid:
            buffers[newid.group(1)] = (nums[0], nums[1])
        elif iface == "zwp_linux_buffer_params_v1" and op == "create":
            params[oid] = (nums[0], nums[1])
        elif iface == "zwp_linux_buffer_params_v1" and op == "created" and newid and oid in params:
            buffers[newid.group(1)] = params[oid]
        elif iface == "wp_viewporter" and op == "get_viewport" and newid and len(objs) >= 2:
            viewport[newid.group(1)] = objs[-1]
        elif iface == "wp_fractional_scale_manager_v1" and op == "get_fractional_scale" and newid:
            frac[newid.group(1)] = objs[-1]
        elif iface == "wp_viewport" and op == "set_destination" and oid in viewport:
            dest[viewport[oid]] = (nums[0], nums[1]) if nums[0] > 0 else None
        elif iface == "wp_fractional_scale_v1" and op == "preferred_scale" and oid in frac:
            prefs.setdefault(frac[oid], []).append(nums[0] / 120)
            seen.setdefault(frac[oid], nums[0] / 120)
        elif iface == "wl_surface" and op == "preferred_buffer_scale":
            seen.setdefault(oid, float(nums[0]))
        elif iface == "wl_surface" and op == "attach":
            attached[oid] = objs[0] if objs else None
        elif iface == "wl_surface" and op == "set_buffer_scale":
            scale[oid] = nums[0]
        elif iface == "wl_surface" and op == "commit" and attached.get(oid) in buffers:
            commits.append((oid, buffers[attached[oid]], dest.get(oid), scale.get(oid, 1), seen.get(oid)))

    def area(c):
        d = c[2] or (c[1][0] // c[3], c[1][1] // c[3])
        return d[0] * d[1]

    return commits, prefs, max(commits, key=area)[0] if commits else None


def wayland_scale(log):
    """What a WAYLAND_DEBUG log says about the toplevel's scaling: its last committed buffer, the
    viewport destination, their ratio, the buffer scale set and the fractional scales offered."""
    commits, prefs, top = wl_commits(log)
    info = {"buffer": None, "destination": None, "ratio": None, "buffer_scale": None,
            "preferred_scales": []}
    if not commits:
        return info
    last = [c for c in commits if c[0] == top][-1]
    info["buffer"], info["destination"], info["buffer_scale"] = last[1], last[2], last[3]
    if last[2]:
        info["ratio"] = round(last[1][0] / last[2][0], 3)
    info["preferred_scales"] = prefs.get(top, [])
    return info


def debug():
    try:
        ws = [(w.get("id"), w.get("app_id"), w.get("title"), w.get("is_focused")) for w in windows()]
        print(f"compat debug: windows {ws}")
        print("compat debug: " + "\n".join(journal().splitlines()[-25:]))
    except Exception as e:
        print(f"compat debug: {e}")


def case(name, fn):
    """Runs one case; fn(info) fills info with facts kept with the result, even on a failure."""
    t0 = time.time()
    err = ""
    info = {}
    try:
        with subtest(name):
            fn(info)
        status = "ok"
    except Exception as e:
        status = "fail"
        err = str(e).replace("\n", " ")[:600]
        debug()
    why = XFAIL.get(name)
    if why:
        status = "xpass" if status == "ok" else "xfail"
    results.append({"compositor": COMPOSITOR, "test": TEST, "case": name, "status": status,
                    "xfail": why or "", "secs": round(time.time() - t0, 1), "error": err,
                    "info": info})
    print(f"compat: {status.upper()} {COMPOSITOR}/{name}{': ' + err if err else ''} {json.dumps(info)}",
          flush=True)


def wait_session(timeout=180):
    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("uin.service")
    wait(lambda: alice_try(f"{IPC} msg version")[0] == 0, f"{COMPOSITOR}'s IPC", timeout)
    wait(lambda: len(js("outputs")) >= 1, "an output", 30)


def finish():
    with open(os.path.join(OUT, f"journal-{COMPOSITOR}{'' if TEST == 'smoke' else '-' + TEST}.txt"), "w") as f:
        f.write(journal())
    with open(os.path.join(OUT, "results.json"), "w") as f:
        json.dump(results, f, indent=1)
    lines = [f"{r['status']:5} {r['compositor']}/{r['case']}"
             + (f" [xfail: {r['xfail']}]" if r["xfail"] else "")
             + (f": {r['error']}" if r["error"] else "")
             + (f" {json.dumps(r['info'])}" if r["info"] else "") for r in results]
    with open(os.path.join(OUT, "results.txt"), "w") as f:
        f.write("\n".join(lines) + "\n")
    print("\n".join(["compat summary:"] + lines), flush=True)
    for d in ("/tmp/shots", "/tmp/probe"):
        try:
            machine.copy_from_vm(d, "")
        except Exception as e:
            print(f"compat: could not copy {d}: {e}")
    failed = [r["case"] for r in results if r["status"] == "fail"]
    if failed and STRICT:
        raise Exception(f"failed: {', '.join(failed)}")
