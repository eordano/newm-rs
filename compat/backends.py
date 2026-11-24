V1, V2 = "Virtual-1", "Virtual-2"


def two_outputs(info):
    wait(lambda: len(outputs()) >= 2, "two outputs", 30)
    o = outputs()
    info["outputs"] = {n: logical(x) for n, x in o.items()}
    expect(V1 in o and V2 in o, f"outputs {sorted(o)}, not {V1} and {V2}")
    expect(logical(o[V1]) and logical(o[V2]), "an output is not enabled")
    expect(abs(logical(o[V1])["scale"] - SCALE) < 0.01, f"{V1} scale {logical(o[V1])['scale']}")
    for n in (V1, V2):
        share = share_of(shot(f"wallpaper-{n}", n), WALLPAPER)
        info[f"wallpaper_{n}"] = round(share, 3)
        expect(share > 0.3, f"{n} shows {share:.0%} wallpaper: the output is not drawn")


def first_buffer(info):
    """A window opening on the 1.25 output draws its first buffer at 1.25: the compositor sends
    the scale before the first configure, not after the first frame."""
    pointer_to(0.25, 0.5)
    alice_try("rm -f /tmp/probe/first-*.wl")
    bad = []
    clients = [
        ("foot", "WAYLAND_DEBUG=1 foot --app-id=first-foot sh -c 'sleep 600' 2> /tmp/probe/first-foot.wl",
         "first-foot", "first-foot", 90),
        ("gtk4", "WAYLAND_DEBUG=1 probe-gtk 4.0 first-gtk4 /tmp/probe/first-gtk4.json"
         " 2> /tmp/probe/first-gtk4.wl", "first-gtk4", "first-gtk4", 90),
        ("chromium", "WAYLAND_DEBUG=1 chromium --ozone-platform=wayland --disable-gpu --no-first-run"
         " --no-default-browser-check --password-store=basic --user-data-dir=/tmp/chromium-first"
         " file:///etc/newm-compat/page.html 2> /tmp/probe/first-chromium.wl", "dpr=",
         "user-data-dir=/tmp/chromium-first", 150),
    ]
    for label, cmd, match, kill, timeout in clients:
        w = new_window(cmd, label, match=match, timeout=timeout)
        try:
            time.sleep(3)
            f = first_commit(f"/tmp/probe/first-{label}.wl")
            info[label] = f
            r, before = f["first_ratio"], f["scale_before_first"]
            if r is None or abs(r - SCALE) > 0.03 or before is None:
                bad.append(f"{label}: first buffer ratio {r}, scale before it {before}")
        finally:
            close(w, kill)
    expect(not bad, "; ".join(bad))


def second_output():
    return logical(outputs().get(V2)) is not None


def output_off_on(info):
    """A window on an output that goes away stays reachable (on the other output, when there is
    one); the output comes back drawn and the window takes input again. Without a second output
    (newm-rs cannot drive the software-only second GPU) the only output goes off and on."""
    two = second_output()
    target = V2 if two else V1
    info["target"] = target
    pointer_to(0.8 if two else 0.25, 0.5)
    w = gtk_window("off-gtk4")
    try:
        rc, out = alice_try(f"wlr-randr --output {target} --off")
        if rc != 0 and not two:
            # niri refuses a configuration that leaves no output on: the monitors go off and
            # come back instead (power-off-monitors, then input), as swayidle's timeout does.
            info["off_refused"] = out.strip()[-120:]
            msg("action", "power-off-monitors")
            time.sleep(3)
            sh("uin 'rel 40 40'", timeout=30)
            time.sleep(3)
            ok, d = typed_reaches(w, "off-gtk4", "wakecompat")
            info["typed_after_wake"] = d.get("text")
            expect(ok, f"typing after the monitors woke reached {d.get('text')!r}")
            box = green_box(shot("wake-all"))
            info["green_after_wake"] = box
            expect(box is not None, "the window is not drawn after the monitors woke")
            return
        expect(rc == 0, f"wlr-randr --off failed: {out.strip()[-200:]}")
        wait(lambda: not logical(outputs().get(target)), f"{target} disabled", 15)
        time.sleep(2)
        expect(get(w["id"]) is not None, "the window went away with its output")
        if two:
            ok, d = typed_reaches(w, "off-gtk4", "offoutputcompat")
            info["typed_after_off"] = d.get("text")
            expect(ok, f"typing after {V2} went off reached {d.get('text')!r}")
            box = green_box(shot("off-v1", V1))
            info["green_on_v1"] = box
            expect(box is not None, f"the window is not drawn on {V1} after {V2} went off")
        alice(f"wlr-randr --output {target} --on")
        wait(lambda: logical(outputs().get(target)), f"{target} enabled again", 15)
        time.sleep(2)
        ok, d = typed_reaches(w, "off-gtk4", "onoutputcompat")
        info["typed_after_on"] = d.get("text")
        expect(ok, f"typing after {target} came back reached {d.get('text')!r}")
        box = green_box(shot("on-all"))
        info["green_after_on"] = box
        expect(box is not None, f"the window is not drawn after {target} came back")
    finally:
        alice_try(f"wlr-randr --output {target} --on")
        close(w, "off-gtk4")


def scale_change(info):
    """A new output scale reaches an open window, which draws at it."""
    pointer_to(0.25, 0.5)
    w = gtk_window("scale-gtk4")
    try:
        time.sleep(2)
        alice(f"wlr-randr --output {V1} --scale 1.5")
        wait(lambda: abs(logical(outputs()[V1])["scale"] - 1.5) < 0.01, f"{V1} at scale 1.5", 15)
        time.sleep(3)
        s = wayland_scale("/tmp/probe/scale-gtk4.wl")
        info["after"] = s
        expect(s["preferred_scales"] and s["preferred_scales"][-1] == 1.5,
               f"scales offered {s['preferred_scales']}, last not 1.5")
        expect(s["ratio"] is not None and abs(s["ratio"] - 1.5) < 0.03,
               f"buffer/destination {s['ratio']} after the change, not 1.5")
    finally:
        alice_try(f"wlr-randr --output {V1} --scale {SCALE}")
        close(w, "scale-gtk4")


def mode_change(info):
    """A new mode applies, and the open window stays on screen."""
    pointer_to(0.25, 0.5)
    w = gtk_window("mode-gtk4")
    try:
        modes = alice(f"wlr-randr --output {V1}")
        info["modes"] = re.findall(r"(\d+x\d+) px", modes)[:8]
        current = logical(outputs()[V1])
        others = [m for m in info["modes"] if m != "2560x1080"
                  and int(m.split("x")[0]) <= 2560 and int(m.split("x")[1]) <= 1600]
        target = others[0] if others else None
        info["target"] = target
        expect(target is not None, f"no other mode to switch to on {V1}: {info['modes']}")
        alice(f"wlr-randr --output {V1} --mode {target}")
        tw = round(int(target.split("x")[0]) / SCALE)
        wait(lambda: abs(logical(outputs()[V1])["width"] - tw) <= 1, f"{V1} at {target}", 15)
        info["logical_before"], info["logical_after"] = current, logical(outputs()[V1])
        focus(w)
        time.sleep(2)
        box = green_box(shot("mode-v1", V1))
        info["green_after"] = box
        expect(box is not None, "the focused window is not on screen after the mode change")
    finally:
        alice_try(f"wlr-randr --output {V1} --mode 2560x1080")
        close(w, "mode-gtk4")


def vt_switch(info):
    """Switching to another VT and back: the session keeps its clients, draws again and takes
    input again."""
    pointer_to(0.25, 0.5)
    pid = sh(f"pgrep -o -x {PROCESS}").strip()
    w = gtk_window("vt-gtk4")
    try:
        time.sleep(2)
        sh("chvt 2")
        time.sleep(3)
        sh("chvt 1")
        time.sleep(4)
        info["pid_same"] = sh(f"pgrep -o -x {PROCESS}").strip() == pid
        expect(info["pid_same"], f"{PROCESS} restarted across the VT switch")
        expect(get(w["id"]) is not None, "the window is gone after the VT switch")
        box = green_box(shot("vt-after", V1))
        info["green_after"] = box
        expect(box is not None, "the window is not drawn after switching back")
        if second_output():
            share = share_of(shot("vt-after-v2", V2), WALLPAPER)
            info["wallpaper_v2_after"] = round(share, 3)
            expect(share > 0.3, f"{V2} shows {share:.0%} wallpaper after switching back")
        ok, d = typed_reaches(w, "vt-gtk4", "vtswitchcompat")
        info["typed"] = d.get("text")
        expect(ok, f"typing after the VT switch reached {d.get('text')!r}")
    finally:
        close(w, "vt-gtk4")


# Each nested compositor runs under its own argv[0] (nested-<tag>), so its cleanup can never
# match the session's own compositor.
NESTED_CMDS = {
    "newm-rs": "NIRI_CONFIG=/etc/newm-compat/nested.kdl exec -a nested-newm-rs newm-rs",
    # wlroots on Mesa's software EGL takes the host's render node from its dmabuf feedback and
    # cannot allocate dumb buffers there in the VM: pixman with shm buffers instead.
    "sway": "WLR_RENDERER=pixman exec -a nested-sway sway --unsupported-gpu -c /etc/newm-compat/nested-sway",
}


def nested(name):
    def run(info):
        pointer_to(0.25, 0.5)
        alice_try("rm -f /tmp/probe/nested.json")
        tag = name.split()[0]
        log = f"/tmp/probe/nested-{tag}.log"
        cmd = NESTED_CMDS[name]
        w = new_window(f"exec > {log} 2>&1; {cmd}", f"nested {name}", timeout=60)
        info["app_id"], info["title"] = w.get("app_id"), w.get("title")
        try:
            focus(w)
            time.sleep(8)
            box = green_box(shot(f"nested-{tag}", V1))
            info["green"] = box
            expect(box is not None, f"no probe window drawn inside nested {name}")
        finally:
            close(w, f"nested-{tag}")
            alice_try("pkill -f '[p]robe-nested'")
    return run


wait_session()
case("two-outputs", two_outputs)
case("first-buffer", first_buffer)
case("output-off-on", output_off_on)
case("scale-change", scale_change)
case("mode-change", mode_change)
case("vt-switch", vt_switch)
for n in [NESTED] + NESTED_OTHERS:
    case("nested-" + n.split()[0], nested(n))
finish()
