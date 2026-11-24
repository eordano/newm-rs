def one_output(info):
    o = outputs()
    info["outputs"] = {n: logical(x) for n, x in o.items()}
    expect(list(o) == ["Virtual-1"], f"outputs {sorted(o)}: the ignored GPU's output is used")
    share = share_of(shot("two-gpu-wallpaper", "Virtual-1"), WALLPAPER)
    info["wallpaper"] = round(share, 3)
    expect(share > 0.3, f"Virtual-1 shows {share:.0%} wallpaper")


def draws_and_types(info):
    w = gtk_window("gpu-gtk4")
    try:
        ok, d = typed_reaches(w, "gpu-gtk4", "twogpucompat")
        info["typed"] = d.get("text")
        expect(ok, f"typing reached {d.get('text')!r}")
        box = green_box(shot("two-gpu-window", "Virtual-1"))
        info["green"] = box
        expect(box is not None, "the probe window is not drawn")
    finally:
        close(w, "gpu-gtk4")


def ignored_device_untouched(info):
    """The compositor holds gpu0's card open and never gpu1's."""
    pid = sh(f"pgrep -o -x {PROCESS}").strip()
    cards = sh(f"ls -l /proc/{pid}/fd | grep -o '/dev/dri/[a-zA-Z0-9]*' | sort -u || true").split()
    g0 = sh("readlink -f /dev/dri/gpu0-card").strip()
    g1 = sh("readlink -f /dev/dri/gpu1-card").strip()
    info["open"], info["gpu0"], info["gpu1"] = cards, g0, g1
    expect(g0 in cards, f"{g0} (gpu0) not open")
    expect(g1 not in cards, f"{g1} (gpu1, ignored) is open")


wait_session()
case("one-output", one_output)
case("draws-and-types", draws_and_types)
case("ignored-device-untouched", ignored_device_untouched)
finish()
