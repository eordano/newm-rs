def outputs():
    """niri's `outputs` answer as {name: output}, whether it comes as a map or a list."""
    o = js("outputs")
    return o if isinstance(o, dict) else {x["name"]: x for x in o}


def logical(o):
    return (o or {}).get("logical") or None


def pointer_to(fx, fy):
    """Moves the absolute mouse to a fraction of the layout's bounding box and clicks there: the
    click on the empty canvas makes that output the active one."""
    sh(f"uin 'abs {int(fx * 1000)} {int(fy * 1000)} 1000 1000; sleep 150; click'", timeout=30)
    time.sleep(0.5)


def share_of(img, rgb, tol=14):
    w, h = img.size
    n = hit = 0
    for y in range(0, h, 8):
        for x in range(0, w, 8):
            n += 1
            p = img.getpixel((x, y))
            if all(abs(p[i] - rgb[i]) <= tol for i in range(3)):
                hit += 1
    return hit / max(n, 1)


WALLPAPER = (0x20, 0x30, 0x40)


def first_commit(log):
    """What a WAYLAND_DEBUG log says about the toplevel surface's FIRST committed buffer: its
    size, the viewport destination or buffer scale it came with, their ratio, and whether a
    scale (wp_fractional_scale_v1.preferred_scale or wl_surface.preferred_buffer_scale) reached
    the client before it. The toplevel surface is the one with the largest committed area."""
    commits, _, top = wl_commits(log)
    info = {"first_buffer": None, "first_destination": None, "first_ratio": None,
            "scale_before_first": None, "commits": len(commits)}
    if not commits:
        return info
    first = next(c for c in commits if c[0] == top)
    info["first_buffer"], info["first_destination"] = first[1], first[2]
    info["first_ratio"] = (round(first[1][0] / first[2][0], 3) if first[2]
                           else float(first[3]))
    info["scale_before_first"] = first[4]
    return info


def gtk_window(name, word=None):
    """A GTK 4 probe window under WAYLAND_DEBUG, logged to /tmp/probe/NAME.wl."""
    alice_try(f"rm -f /tmp/probe/{name}.json /tmp/probe/{name}.wl")
    return new_window(f"WAYLAND_DEBUG=1 probe-gtk 4.0 {name} /tmp/probe/{name}.json "
                      f"2> /tmp/probe/{name}.wl", name, match=name)


def typed_reaches(w, name, word):
    focus(w)
    time.sleep(1)
    alice_try(f"rm -f /tmp/probe/{name}.json")
    type_text(word)
    d = read_json(f"/tmp/probe/{name}.json")
    return d.get("text") == word, d
