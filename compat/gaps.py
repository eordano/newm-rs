# Run on both desktops: a case that passes on one and fails on the other is a parity gap.


def output_power(info):
    """wlr-output-power-management-v1 through wlopm, as swayidle configs call it."""
    def modes():
        rc, out = alice_try("wlopm")
        info["wlopm"] = out.strip()
        return rc, out
    rc, out = modes()
    expect(rc == 0 and " on" in out, f"wlopm lists no output on: rc {rc}, {out!r}")
    alice("wlopm --off '*'")
    wait(lambda: " off" in modes()[1], "wlopm to report the output off")
    alice("wlopm --on '*'")
    wait(lambda: " on" in modes()[1] and " off" not in modes()[1], "wlopm to report the output on")
    before = shot("power-before")
    w = new_window("foot", "foot", match="foot")
    try:
        after = shot("power-after")
        ch, cols, ok = drew(before, after)
        info["changed"] = round(ch, 3)
        expect(ok, f"nothing drawn after --on: {ch:.3f} of the screen changed")
    finally:
        close(w)


def shortcuts_inhibit(info):
    """keyboard-shortcuts-inhibit-v1 through GTK 4's inhibit_system_shortcuts, as a VM viewer."""
    log = "/tmp/probe/inhibit.jsonl"
    alice(f"rm -f {log} /tmp/probe/fired-t /tmp/probe/fired-y")
    w = new_window(f"probe-inhibit inhibitcompat {log}", "the inhibit probe", match="inhibitcompat")
    try:
        focus(w)
        wait(lambda: {"inhibited": True} in jlines(log), "the compositor to activate the inhibitor")

        def presses(name):
            return sum(1 for d in jlines(log) if d.get("key") == name and d.get("super"))

        def combo(key):
            sh("uin " + shlex.quote(f"key leftmeta down; tap {key}; key leftmeta up; sleep 300"))

        combo("t")
        wait(lambda: presses("t") == 1, "Super+T to reach the probe")
        fired = machine.execute("test -e /tmp/probe/fired-t")[0] == 0
        expect(not fired, "Super+T fired its bind while the probe inhibited shortcuts")

        combo("y")
        wait(lambda: machine.execute("test -e /tmp/probe/fired-y")[0] == 0,
             "the allow-inhibiting=false bind Super+Y to fire")
        expect(presses("y") == 0, "the probe got Super+Y, an allow-inhibiting=false bind")

        combo("esc")
        wait(lambda: {"inhibited": False} in jlines(log), "Super+Escape to turn the inhibitor off")
        combo("t")
        wait(lambda: machine.execute("test -e /tmp/probe/fired-t")[0] == 0,
             "Super+T to fire once the inhibitor is off")
        expect(presses("t") == 1, "the probe got Super+T after the toggle")
        info["log"] = jlines(log)[-6:]
    finally:
        close(w, "inhibitcompat")


def system_bell(info):
    """xdg-system-bell-v1 through GTK 4's gdk_surface_beep: an unfocused window's bell makes it
    urgent (IPC is_urgent), as niri marks a window whose activation it did not honour."""
    log = "/tmp/probe/bell.log"
    alice(f"rm -f {log}")
    w = new_window(f"probe-bell bellcompat {log} 6", "the bell probe", match="bellcompat")
    f = new_window("foot", "foot", match="foot")
    try:
        focus(f)
        expect(not (get(w["id"]) or {}).get("is_urgent"), "the probe was urgent before ringing")
        wait(lambda: alice_try(f"grep -c ring {log}")[1].strip() not in ("", "0"), "the probe to ring", 20)
        wait(lambda: (get(w["id"]) or {}).get("is_urgent"), "the ringing window to turn urgent", 10)
        info["rings"] = alice_try(f"grep -c ring {log}")[1].strip()
    finally:
        close(f)
        close(w, "bellcompat")


def modal_dialog(info):
    """xdg-dialog-v1 through a GTK 4 modal dialog: it floats centred on its parent, focusing the
    parent (IPC, a click on it) lands on the dialog, and closing the dialog focuses the parent."""
    p = new_window("probe-dialog dialogcompat dialogcompat", "the dialog probe's parent",
                   match="dialogcompat-parent")
    f = None
    try:
        d = wait(lambda: next((w for w in windows() if w.get("title") == "dialogcompat-dialog"), None),
                 "the modal dialog", 20)
        wait(lambda: (get(d["id"]) or {}).get("is_focused"), "the dialog to take the focus")
        p, d = get(p["id"]), get(d["id"])
        expect(d.get("is_floating"), "the dialog is not floating")
        lp, ld = p.get("layout") or {}, d.get("layout") or {}
        pos_p, pos_d = lp.get("tile_pos_in_workspace_view"), ld.get("tile_pos_in_workspace_view")
        size_p, size_d = lp.get("tile_size"), ld.get("tile_size")
        info["parent"], info["dialog"] = [pos_p, size_p], [pos_d, size_d]
        if pos_p and pos_d and size_p and size_d:
            cp = (pos_p[0] + size_p[0] / 2, pos_p[1] + size_p[1] / 2)
            cd = (pos_d[0] + size_d[0] / 2, pos_d[1] + size_d[1] / 2)
            info["centre_offset"] = [round(cd[0] - cp[0]), round(cd[1] - cp[1])]
            expect(abs(cd[0] - cp[0]) <= 24 and abs(cd[1] - cp[1]) <= 24,
                   f"the dialog's centre {cd} is not its parent's {cp}")
        # Another window takes the focus; focusing the parent lands on the dialog.
        f = new_window("foot", "foot", match="foot")
        focus(f)
        msg("action", "focus-window", "--id", p["id"])
        wait(lambda: (get(d["id"]) or {}).get("is_focused"), "IPC focus of the parent to land on the dialog")
        expect(not (get(p["id"]) or {}).get("is_focused"), "the parent took the focus from its dialog")
        # A click on the parent, beside the dialog, lands on the dialog too.
        if pos_p and size_p:
            focus(f)
            outs = js("outputs") or {}
            o = next(iter(outs.values())) if isinstance(outs, dict) else outs[0]
            lw, lh = o["logical"]["width"], o["logical"]["height"]
            x, y = int(pos_p[0] + 12), int(pos_p[1] + size_p[1] / 2)
            sh("uin " + shlex.quote(f"abs {x} {y} {lw} {lh}; sleep 150; click; sleep 250"))
            wait(lambda: (get(d["id"]) or {}).get("is_focused"), "a click on the parent to land on the dialog")
            info["clicked"] = [x, y]
        # Closing the dialog gives the focus back to the parent.
        msg("action", "close-window", "--id", d["id"])
        wait(lambda: get(d["id"]) is None, "the dialog to close")
        wait(lambda: (get(p["id"]) or {}).get("is_focused"), "the parent to take the focus back")
    finally:
        if f:
            close(f)
        close(p, "dialogcompat")


wait_session()
case("output-power-wlopm", output_power)
case("shortcuts-inhibit-gtk4", shortcuts_inhibit)
case("system-bell-gtk4", system_bell)
case("modal-dialog-gtk4", modal_dialog)
finish()
