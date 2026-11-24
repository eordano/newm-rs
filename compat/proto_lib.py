W_PHYS, H_PHYS = 2560, 1080


def jwait(path, pred, what, timeout=15):
    return wait(lambda: next((d for d in jlines(path) if pred(d)), None), what, timeout)


def uin(script, timeout=60):
    sh("uin " + shlex.quote(script), timeout=timeout)


def uinx(script, timeout=60):
    sh("uinx " + shlex.quote(script), timeout=timeout)


def centre(box):
    return (box[0] + box[2]) // 2, (box[1] + box[3]) // 2


def abs_to(x, y):
    return f"abs {int(x)} {int(y)} {W_PHYS} {H_PHYS}"


def drag(a, b, steps=12, hold_ms=400):
    """A left-button drag of the absolute mouse from a to b (physical pixels)."""
    parts = [abs_to(*a), "sleep 150", "button left down", f"sleep {hold_ms}"]
    for i in range(1, steps + 1):
        parts += [abs_to(a[0] + (b[0] - a[0]) * i / steps, a[1] + (b[1] - a[1]) * i / steps), "sleep 60"]
    parts += ["sleep 400", "button left up", "sleep 300"]
    uin("; ".join(parts), timeout=90)


def screen(name):
    """QEMU's own screendump (what the DRM side scans out), as a PIL image."""
    machine.screenshot(name)
    return Image.open(os.path.join(OUT, f"{name}.png")).convert("RGB")


def alive(info=None):
    """The compositor answers IPC and its process lives."""
    rc = alice_try(f"timeout 10 {IPC} msg version")[0]
    up = machine.execute(f"pgrep -x {PROCESS}")[0] == 0
    if info is not None:
        info["alive"] = rc == 0 and up
    return rc == 0 and up


def probe_window(cmd, match, timeout=60):
    """Starts a probe through the compositor and returns its window."""
    return new_window(cmd, match, match=match, timeout=timeout)


def kill(pattern):
    alice_try(f"pkill -f {shlex.quote('[' + pattern[0] + ']' + pattern[1:])}")


def journal_count(pattern):
    return sum(1 for line in journal().splitlines() if re.search(pattern, line))


def rss_kb():
    rc, out = machine.execute(f"ps -C {PROCESS} -o rss= | head -n 1")
    try:
        return int(out.strip())
    except ValueError:
        return None
