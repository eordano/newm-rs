def activation_token(info):
    alice_try("rm -f /tmp/probe/act-*.jsonl")
    child = "probe gtkx plain probe-act-child /tmp/probe/act-child.jsonl"
    lw = probe_window(f"probe gtkx launch probe-act-launcher /tmp/probe/act-launcher.jsonl 'child={child}'",
                      "probe-act-launcher")
    cw = None
    try:
        focus(lw)
        time.sleep(1)
        uin("tap enter")
        cw = wait(lambda: next((w for w in windows() if "probe-act-child" in (w.get("title") or "")), None),
                  "the launched child", 30)
        f = wait(lambda: (get(cw["id"]) or {}).get("is_focused"), "the child focused", 10)
        info["child_focused"] = bool(f)
        info["launcher_log"] = jlines("/tmp/probe/act-launcher.jsonl")[-2:]
        env = alice_try("tr '\\0' '\\n' < /proc/$(pgrep -f '[p]robe-act-child' | head -n1)/environ | grep -E 'XDG_ACTIVATION_TOKEN|DESKTOP_STARTUP_ID'")[1]
        info["child_env"] = env.split()
    finally:
        close(lw, "probe-act-launcher")
        if cw:
            close(cw, "probe-act-child")


def activation_stale(info):
    alice_try("rm -f /tmp/probe/late.jsonl /tmp/probe/busy.json")
    lw = probe_window("probe gtkx plain probe-late /tmp/probe/late.jsonl late=8", "probe-late")
    bw = probe_window("probe-gtk 4.0 probe-busy /tmp/probe/busy.json", "probe-busy")
    try:
        focus(bw)
        for _ in range(6):
            uin("type x; sleep 900")
        time.sleep(2)
        info["late_log"] = jlines("/tmp/probe/late.jsonl")[-3:]
        late = get(lw["id"]) or {}
        info["late_focused"], info["late_urgent"] = late.get("is_focused"), late.get("is_urgent")
        expect(any(d.get("present_again") for d in jlines("/tmp/probe/late.jsonl")), "the probe never re-presented")
        expect(not late.get("is_focused"), "a stale activation stole the focus from the window being typed in")
    finally:
        close(lw, "probe-late")
        close(bw, "probe-busy")


def portal_debug():
    unit = "plasma-xdg-desktop-portal-kde.service"
    alice_try(f"systemctl --user set-environment WAYLAND_DEBUG=1; systemctl --user restart {unit}; "
              "systemctl --user unset-environment WAYLAND_DEBUG")
    time.sleep(2)
    return unit


def filechooser(info, name, cmd, extra=""):
    unit = portal_debug()
    log = f"/tmp/probe/{name}.jsonl"
    alice_try(f"rm -f {log}")
    w = probe_window(f"{cmd} {name} {log} {extra}", name)
    dlg = None
    try:
        focus(w)
        time.sleep(1)
        before = {x["id"] for x in windows()}
        uin("tap enter")
        dlg = wait(lambda: next((x for x in windows() if x["id"] not in before), None), "the file dialog", 40)
        time.sleep(2)
        d = get(dlg["id"]) or dlg
        info["dialog"] = {k: d.get(k) for k in ("app_id", "title", "is_floating", "is_focused")}
        pj = sh(f"journalctl -b -q -o cat --no-pager _SYSTEMD_USER_UNIT={unit} | tail -n 4000")
        info["set_parent_of"] = len(re.findall(r"zxdg_imported_v[12][#@]\d+\.set_parent_of", pj))
        info["import_v1"] = len(re.findall(r"zxdg_importer_v1[#@]\d+\.import", pj))
        info["import_v2"] = len(re.findall(r"zxdg_importer_v2[#@]\d+\.import_toplevel", pj))
        info["imported_destroyed"] = len(re.findall(r"zxdg_imported_v[12][#@]\d+\.destroyed", pj))
        expect(d.get("is_focused"), "the dialog did not get the focus")
        expect(info["set_parent_of"] > 0, "the portal never parented its dialog to the app (xdg-foreign)")
        expect(info["imported_destroyed"] == 0, "the compositor destroyed the imported handle")
    finally:
        uin("tap esc; sleep 500")
        if dlg and get(dlg["id"]):
            close(dlg)
        close(w, name)


def fc_gtk4(info):
    filechooser(info, "probe-fc-gtk4", "GDK_DEBUG=portals probe gtkx fc")


def fc_gtk3(info):
    filechooser(info, "probe-fc-gtk3", "GTK_USE_PORTAL=1 probe gtkx fc", "gtk=3")


def fc_qt(info):
    filechooser(info, "probe-fc-qt", "QT_QPA_PLATFORMTHEME=xdgdesktopportal probe qtx fc")


def portal_screenshot(info):
    r = json.loads(alice("probe dbusx portal-screenshot", timeout=60).strip().splitlines()[-1])
    info.update(r)
    expect(r.get("code") == 0 and (r.get("bytes") or 0) > 1000, f"portal screenshot {r}")


def screencast(info):
    out = alice("probe dbusx mutter-cast Virtual-1 frames=5", timeout=90)
    r = json.loads(out.strip().splitlines()[-1])
    info.update(r)
    expect(r.get("node") is not None, f"no PipeWire node: {r}")
    expect(r.get("gst_rc") == 0, f"pulling 5 frames failed: {r.get('gst_rc')} {r.get('gst_err')}")


def spectacle(info):
    alice_try("rm -f /tmp/probe/spectacle.png")
    rc, outp = alice_try("timeout 60 spectacle -b -n -f -o /tmp/probe/spectacle.png", timeout=90)
    info["rc"] = rc
    machine.copy_from_vm("/tmp/probe/spectacle.png", "shots")
    p = os.path.join(OUT, "shots", "spectacle.png")
    expect(os.path.exists(p), f"no screenshot (rc {rc}): {outp[-300:]}")
    img = Image.open(p)
    info["size"] = img.size
    expect(img.size == (W_PHYS, H_PHYS), f"screenshot {img.size}")


def layer_bar(kb, zone=60, name="probe-bar"):
    alice_try(f"rm -f /tmp/probe/{name}.jsonl")
    spawn(f"WAYLAND_DEBUG=1 probe gtkx layer {name} /tmp/probe/{name}.jsonl kb={kb} zone={zone} color=#3030c0"
          f" > /tmp/probe/{name}.err 2>&1")
    jwait(f"/tmp/probe/{name}.jsonl", lambda d: d.get("mapped"), "the bar mapped", 60)
    time.sleep(1)
    return wait(lambda: color_box(shot(f"{name}-{kb}"), "#3030c0"), "the bar on screen", 10)


def layer_exclusive(info):
    bar = layer_bar("none")
    alice_try("rm -f /tmp/probe/below.json")
    w = probe_window("probe-gtk 4.0 probe-below /tmp/probe/below.json", "probe-below")
    try:
        time.sleep(1.5)
        g = green_box(shot("layer-exclusive"))
        info["bar"], info["window"] = bar, g
        expect(bar[1] <= 4, f"bar not at the top: {bar}")
        expect(g and g[1] >= bar[3] - 2, f"window {g} under the bar's exclusive zone {bar}")
    finally:
        close(w, "probe-below")
        kill("gtkx.py layer probe-bar ")


def layer_kb_exclusive(info):
    alice_try("rm -f /tmp/probe/under.json")
    w = probe_window("probe-gtk 4.0 probe-under /tmp/probe/under.json", "probe-under")
    try:
        focus(w)
        layer_bar("exclusive", name="probe-barx")
        uin("type layerexcl; sleep 200; tap enter; sleep 500")
        got = [d.get("text") for d in jlines("/tmp/probe/probe-barx.jsonl") if "text" in d]
        info["bar_read"] = got
        info["bar_keys"] = len([d for d in jlines("/tmp/probe/probe-barx.jsonl") if "key" in d])
        wl = cat("/tmp/probe/probe-barx.err")
        info["wl_kb_enter"] = len(re.findall(r"wl_keyboard[#@]\d+\.enter\(", wl))
        info["wl_kb_keys"] = len(re.findall(r"wl_keyboard[#@]\d+\.key\(", wl))
        info["kb_mode_set"] = re.findall(r"zwlr_layer_surface_v1[#@]\d+\.set_keyboard_interactivity\((\d+)\)", wl)
        info["window_read"] = cat("/tmp/probe/under.json")[:100]
        expect("layerexcl" in got or (info["wl_kb_enter"] > 0 and info["wl_kb_keys"] >= 9),
               "the exclusive layer surface got no keyboard focus or keys (wl_keyboard)")
    finally:
        kill("gtkx.py layer probe-barx")
        close(w, "probe-under")


def layer_on_demand(info):
    alice_try("rm -f /tmp/probe/od.json")
    w = probe_window("probe-gtk 4.0 probe-od /tmp/probe/od.json", "probe-od")
    try:
        bar = layer_bar("on_demand", name="probe-bard")
        focus(w)
        ex = (bar[0] + bar[2]) // 2 + 200
        ey = (bar[1] + bar[3]) // 2
        uin(f"{abs_to(ex, ey)}; sleep 200; click; sleep 500; type ondemand; sleep 200; tap enter; sleep 500")
        got = [d.get("text") for d in jlines("/tmp/probe/probe-bard.jsonl") if "text" in d]
        info["bar_read"] = got
        expect("ondemand" in got, f"the on-demand bar read {got}")
        g = green_box(shot("layer-ondemand-win"))
        uin(f"{abs_to(*centre(g))}; sleep 200; click; sleep 500; type back; sleep 200; tap enter")
        got2 = read_json("/tmp/probe/od.json").get("text")
        info["window_read"] = got2
        expect(got2 == "back", f"after a click the window read {got2!r}")
    finally:
        kill("gtkx.py layer probe-bard")
        close(w, "probe-od")


def layer_popup(info):
    bar = layer_bar("on_demand", name="probe-barp")
    try:
        shot("layer-popup-before")
        bx, by = bar[0] + int(35 * SCALE), (bar[1] + bar[3]) // 2
        uin(f"{abs_to(bx, by)}; sleep 200; click; sleep 800")
        after = shot("layer-popup-after")
        red = color_box(after, "#c03030", tol=30)
        info["popover"] = red
        info["log"] = [d for d in jlines("/tmp/probe/probe-barp.jsonl") if "popover" in d]
        expect(red is not None, "no popover on screen")
        expect(red[1] >= bar[3] - 4, f"popover {red} not below the bar {bar}")
    finally:
        uin("tap esc")
        kill("gtkx.py layer probe-barp")


def lock_case(info, locker, pkill_pattern):
    alice_try("rm -f /tmp/probe/lockwin.json")
    w = probe_window("probe-gtk 4.0 probe-lockwin /tmp/probe/lockwin.json", "probe-lockwin")
    try:
        focus(w)
        msg("action", "fullscreen-window")
        time.sleep(1.5)
        expect(green_box(screen("lock-0-before")) is not None, "the fullscreen probe is not on screen")
        spawn(f"{locker} > /tmp/probe/locker.log 2>&1")
        time.sleep(3)
        info["locked_hidden"] = green_box(screen("lock-1-locked")) is None
        expect(info["locked_hidden"], "the probe shows through the lock")
        alice_try(f"pkill -9 -f {shlex.quote(pkill_pattern)}")
        time.sleep(2)
        info["after_kill_hidden"] = green_box(screen("lock-2-locker-killed")) is None
        expect(info["after_kill_hidden"], "the probe shows after the locker was killed")
        alice(f"setsid -f sh -c '{locker} > /tmp/probe/locker2.log 2>&1' > /dev/null 2>&1 < /dev/null")
        time.sleep(3)
        uin("type correct-horse-7; sleep 200; tap enter")
        time.sleep(3)
        info["unlocked_shows"] = green_box(screen("lock-3-unlocked")) is not None
        info["refocused"] = (get(w["id"]) or {}).get("is_focused")
        expect(info["unlocked_shows"], "the probe is not back after unlocking")
        expect(info["refocused"], "the focus did not come back to the probe")
    finally:
        alice_try(f"pkill -9 -f {shlex.quote(pkill_pattern)}")
        close(w, "probe-lockwin")


def lock_swaylock(info):
    lock_case(info, "swaylock -c 101010", "[s]waylock -c")


def lock_newm(info):
    if alice_try("command -v newm-rs-lock")[0] != 0:
        info["skipped"] = "no newm-rs-lock in this package"
        return
    lock_case(info, "newm-rs-lock", "[n]ewm-rs-lock")


def idle_start(tag, secs=3):
    alice_try(f"rm -f /tmp/probe/idle-{tag}")
    spawn(f"swayidle -w timeout {secs} 'touch /tmp/probe/idle-{tag}' > /tmp/probe/swayidle-{tag}.log 2>&1")
    time.sleep(1)


def idle_fired(tag):
    return alice_try(f"test -e /tmp/probe/idle-{tag}")[0] == 0


def held_off(info, tag, start_holder, stop_holder, settle=2):
    idle_start(tag)
    try:
        wait(lambda: idle_fired(tag), "idle without an inhibitor (baseline)", 12)
        alice_try(f"rm -f /tmp/probe/idle-{tag}")
        start_holder()
        time.sleep(settle)
        uin(f"{abs_to(10, 10)}; sleep 100; {abs_to(20, 20)}")
        alice_try(f"rm -f /tmp/probe/idle-{tag}")
        time.sleep(8)
        info["fired_while_held"] = idle_fired(tag)
        stop_holder()
        try:
            wait(lambda: idle_fired(tag), "idle after the inhibitor left", 15)
            info["fired_after"] = True
        except Fail:
            info["fired_after"] = False
        expect(not info["fired_while_held"], "idle fired while inhibited")
        expect(info["fired_after"], "idle never fired after the inhibitor left")
    finally:
        kill(f"swayidle -w timeout 3 'touch /tmp/probe/idle-{tag}'")


def idle_mpv(info):
    held_off(info, "mpv",
             lambda: spawn("mpv --no-config --vo=wlshm --loop=inf --title=probe-mpv"
                           " av://lavfi:testsrc=size=320x240:rate=15 > /tmp/probe/mpv.log 2>&1"),
             lambda: kill("mpv --no-config"), settle=5)


def idle_dbus_screensaver(info):
    held_off(info, "ss",
             lambda: spawn("probe dbusx ss-inhibit 600 > /tmp/probe/ss.json 2>&1"),
             lambda: kill("dbusx.py ss-inhibit"))
    info["inhibit_reply"] = alice_try("head -n1 /tmp/probe/ss.json")[1].strip()


def idle_log():
    """newm-rs's idle-inhibit state changes (target newm_rs::idle), the last few."""
    return [l for l in journal().splitlines() if "idle inhibitors:" in l][-4:]


def idle_gtk4(info):
    # gtk_application_inhibit(window, IDLE) on Wayland takes zwp_idle_inhibit_v1 on the window's
    # surface (gtkapplication-wayland.c); D-Bus only carries flags Wayland cannot hold.
    try:
        held_off(info, "gtk",
                 lambda: spawn("probe gtkx inhibit probe-inhibit /tmp/probe/inhibit.jsonl"),
                 lambda: kill("gtkx.py inhibit probe-inhibit"), settle=4)
    finally:
        info["app_log"] = jlines("/tmp/probe/inhibit.jsonl")[-2:]
        info["idle_log"] = idle_log()


def idle_kde_portal(info):
    # org.freedesktop.portal.Inhibit (idle) -> xdg-desktop-portal-kde -> AddInhibition on
    # org.kde.Solid.PowerManagement (inhibit.cpp), which the compositor answers.
    try:
        held_off(info, "portal",
                 lambda: spawn("probe dbusx portal-inhibit 600 > /tmp/probe/portal-inhibit.json 2>&1"),
                 lambda: kill("dbusx.py portal-inhibit"))
    finally:
        info["portal_reply"] = alice_try("head -n1 /tmp/probe/portal-inhibit.json")[1].strip()
        info["idle_log"] = idle_log()


def output_randr(info):
    try:
        alice("wlr-randr --output Virtual-1 --scale 1.5")
        def at15():
            outs = js("outputs")
            vals = outs.values() if isinstance(outs, dict) else outs
            return next((o for o in vals if abs((o.get("logical") or {}).get("scale", 0) - 1.5) < 0.01), None)
        o = wait(at15, "scale 1.5", 10)
        info["logical_at_1_5"] = o["logical"]["width"]
        modes = json.loads(alice("wlr-randr --json"))[0]["modes"]
        info["modes"] = len(modes)
        alt = next((m for m in modes if (m["width"], m["height"]) == (1920, 1080)), None)
        if alt:
            alice("wlr-randr --output Virtual-1 --mode 1920x1080")
            time.sleep(2)
            info["after_mode"] = json.loads(alice("wlr-randr --json"))[0]["modes"]
            info["after_mode"] = [(m["width"], m["height"]) for m in info["after_mode"] if m.get("current")]
            expect(info["after_mode"] == [(1920, 1080)], f"mode now {info['after_mode']}")
        expect(abs(info["logical_at_1_5"] - 2560 / 1.5) < 2, f"logical width {info['logical_at_1_5']}")
    finally:
        alice_try("wlr-randr --output Virtual-1 --mode 2560x1080 --scale 1.25 --pos 0,0")
        time.sleep(2)
        info["alive"] = alive()


def gamma(info):
    rc, outp = alice_try("timeout 6 gammastep -m wayland -O 3500 -v 2>&1", timeout=20)
    info["rc"], info["out"] = rc, outp[-300:]
    expect(alive(info), "the compositor stopped answering")
    expect(rc in (0, 124) or re.search(r"(?i)gamma.*(not supported|failed|unavailable)", outp),
           f"gammastep rc {rc}")


def foreign_toplevel(info):
    a = probe_window("probe-gtk 4.0 probe-ft-a /tmp/probe/fta.json", "probe-ft-a")
    b = probe_window("probe-gtk 4.0 probe-ft-b /tmp/probe/ftb.json", "probe-ft-b")
    try:
        lst = json.loads(alice("lswt -j"))
        apps = [t.get("app-id") or t.get("app_id") for t in (lst.get("toplevels") if isinstance(lst, dict) else lst)]
        info["lswt"] = apps
        expect("probe-ft-a" in apps and "probe-ft-b" in apps, f"lswt listed {apps}")
        focus(b)
        # wlrctl 0.2.2 sends activate, then stop; the compositor still sends state events for the
        # handles it holds (the protocol allows them after stop), and libwayland-client 1.26
        # aborts wlrctl on one ("data is non-NULL with zero alloc", exit 134) after the activate is
        # out. A client bug: the case judges the effect, not wlrctl's exit code, and keeps the trace.
        rc, out = alice_try("WAYLAND_DEBUG=1 wlrctl toplevel focus app_id:probe-ft-a 2>&1 | tail -n 60")
        info["wlrctl_aborted"] = "data is non-NULL with zero alloc" in out
        info["wlrctl_tail"] = out[-1500:]
        wait(lambda: (get(a["id"]) or {}).get("is_focused"), "wlrctl's focus", 10)
        alice("wlrctl toplevel close app_id:probe-ft-b")
        wait(lambda: get(b["id"]) is None, "wlrctl's close", 10)
    finally:
        close(a, "probe-ft-a")
        close(b, "probe-ft-b")


PRIVILEGED = ["zwlr_data_control_manager_v1", "ext_data_control_manager_v1", "zwlr_screencopy_manager_v1",
              "ext_image_copy_capture_manager_v1", "zwp_virtual_keyboard_manager_v1",
              "zwlr_virtual_pointer_manager_v1", "zwlr_layer_shell_v1", "zwlr_foreign_toplevel_manager_v1",
              "ext_foreign_toplevel_list_v1", "ext_session_lock_manager_v1", "zwp_input_method_manager_v2",
              "zwlr_output_manager_v1", "zwlr_gamma_control_manager_v1", "wp_security_context_manager_v1",
              "zkde_screencast_unstable_v1", "ext_workspace_manager_v1", "wp_drm_lease_device_v1"]


def security_context(info):
    sock = "/run/user/1000/probe-sandbox.sock"
    alice_try("rm -f /tmp/probe/secctx.jsonl")
    spawn(f"probe wl secctx /tmp/probe/secctx.jsonl {sock}")
    try:
        jwait("/tmp/probe/secctx.jsonl", lambda d: "listening" in d or "no_manager" in d, "the listener", 15)
        text = alice(f"WAYLAND_DISPLAY={sock} wayland-info 2>&1")
        globs = sorted(set(re.findall(r"interface: '([^']+)'", text)))
        full = sorted(set(re.findall(r"interface: '([^']+)'", alice("wayland-info 2>&1"))))
        info["sandboxed"], info["full"] = len(globs), len(full)
        leaked = [g for g in PRIVILEGED if g in globs]
        info["leaked"] = leaked
        info["hidden"] = [g for g in PRIVILEGED if g in full and g not in globs]
        expect(globs, "the sandboxed client saw no globals")
        expect(not leaked, f"a sandboxed client can bind {leaked}")
    finally:
        kill("wl.py secctx")


def busctl(args):
    return alice(f"busctl --user --json=short call {args}")


def kglobalaccel_spectacle(info):
    alice_try("rm -f /tmp/probe/kga-monitor.txt")
    spawn("dbus-monitor --session \"interface='org.kde.kglobalaccel.Component'\""
          " \"interface='org.freedesktop.Application'\" > /tmp/probe/kga-monitor.txt 2>&1")
    spawn("spectacle --dbus > /tmp/probe/spectacle-dbus.log 2>&1")
    try:
        comps = wait(lambda: "spectacle" in alice_try("busctl --user call org.kde.kglobalaccel /kglobalaccel "
                                                      "org.kde.KGlobalAccel allComponents")[1]
                     and alice_try("busctl --user call org.kde.kglobalaccel /kglobalaccel org.kde.KGlobalAccel "
                                   "allComponents")[1], "Spectacle's component", 30)
        info["components"] = re.findall(r'"(/component/[^"]+)"', comps)
        sp = next((c for c in info["components"] if "spectacle" in c), None)
        if sp:
            infos = alice_try(f"busctl --user --json=short call org.kde.kglobalaccel {sp} "
                              "org.kde.kglobalaccel.Component allShortcutInfos")[1]
            info["spectacle_keys"] = [(i[1], i[6]) for i in (json.loads(infos)["data"][0] if infos.strip() else [])][:8]
        # The component is saved from the spectacle-screenshot case, so its presence says nothing
        # about this Spectacle: wait for it to be on the bus before pressing.
        wait(lambda: "org.kde.spectacle" in alice_try("busctl --user list --no-legend")[1],
             "Spectacle on the bus", 30)
        time.sleep(1)
        before = {w["id"] for w in windows()}
        uin("key leftshift down; tap sysrq; key leftshift up; sleep 2500")
        mon = cat("/tmp/probe/kga-monitor.txt")
        info["pressed_signals"] = len(re.findall(r"member=globalShortcutPressed", mon))
        # Shift+Print is one of Spectacle's desktop-file actions: kglobalacceld starts those over
        # org.freedesktop.Application (KServiceActionComponent) instead of signalling them.
        info["activate_action"] = len(re.findall(r"member=ActivateAction", mon))
        info["full_screen_asked"] = "FullScreenScreenShot" in mon
        info["new_windows"] = [w.get("app_id") for w in windows() if w["id"] not in before]
        expect(info["pressed_signals"] > 0 or info["full_screen_asked"],
               "Shift+Print neither signalled Spectacle nor asked it for FullScreenScreenShot")
    finally:
        kill("spectacle --dbus")
        kill("dbus-monitor --session")


def kglobalaccel_refuse(info):
    meta_f12 = 0x10000000 | 0x0100003B
    act = "asa\\(ai\\)"
    busctl("org.kde.kglobalaccel /kglobalaccel org.kde.KGlobalAccel doRegister as 4 probe-kga probe-act ProbeKga ProbeAct")
    r = json.loads(busctl(f"org.kde.kglobalaccel /kglobalaccel org.kde.KGlobalAccel setShortcutKeys "
                          f"{act}u 4 probe-kga probe-act ProbeKga ProbeAct 1 4 {meta_f12} 0 0 0 2"))
    info["reply"] = r.get("data")
    def ints(v):
        return [x for i in v for x in ints(i)] if isinstance(v, list) else [v]
    keys = ints(r.get("data") or [])
    expect(keys and not any(keys), f"Meta+F12, a key the config binds, was granted: {r.get('data')}")


def kglobalaccel_no_context(info):
    comps = json.loads(busctl("org.kde.kglobalaccel /kglobalaccel org.kde.KGlobalAccel allComponents"))["data"][0]
    expect(comps, "no components")
    path = comps[0]
    info["component"] = path
    for m in ("shortcutNames", "allShortcutInfos"):
        rc, outp = alice_try(f"busctl --user call org.kde.kglobalaccel {path} org.kde.kglobalaccel.Component {m}")
        info[m] = rc
        expect(rc == 0, f"{m}() without a context: {outp.strip()[:200]}")


def krunner_windows(info):
    w = probe_window("probe-gtk 4.0 probe-krunner-target /tmp/probe/krt.json", "probe-krunner-target")
    o = probe_window("probe-gtk 4.0 probe-krunner-other /tmp/probe/kro.json", "probe-krunner-other")
    try:
        focus(o)
        r = json.loads(busctl("org.kde.KWin /WindowsRunner org.kde.krunner1 Match s probe-krunner-target"))
        matches = r["data"][0]
        info["matches"] = [(m[0], m[1]) for m in matches][:5]
        expect(matches, "KRunner's window search found nothing")
        busctl(f"org.kde.KWin /WindowsRunner org.kde.krunner1 Run ss {shlex.quote(matches[0][0])} ''")
        wait(lambda: (get(w["id"]) or {}).get("is_focused"), "Run focusing the window", 10)
    finally:
        close(w, "probe-krunner-target")
        close(o, "probe-krunner-other")


wait_session()
case("activation-token", activation_token)
case("activation-stale-token", activation_stale)
case("portal-filechooser-gtk4", fc_gtk4)
case("portal-filechooser-gtk3", fc_gtk3)
case("portal-filechooser-qt", fc_qt)
case("portal-screenshot", portal_screenshot)
case("screencast-pipewire", screencast)
case("spectacle-screenshot", spectacle)
case("layer-exclusive-zone", layer_exclusive)
case("layer-keyboard-exclusive", layer_kb_exclusive)
case("layer-keyboard-on-demand", layer_on_demand)
case("layer-popup", layer_popup)
case("lock-swaylock", lock_swaylock)
case("lock-newm-rs-lock", lock_newm)
case("idle-inhibit-mpv", idle_mpv)
case("idle-dbus-screensaver", idle_dbus_screensaver)
case("idle-inhibit-gtk4", idle_gtk4)
case("idle-kde-inhibit-portal", idle_kde_portal)
case("output-randr", output_randr)
case("gamma-control", gamma)
case("foreign-toplevel", foreign_toplevel)
case("security-context", security_context)
case("kglobalaccel-spectacle", kglobalaccel_spectacle)
case("kglobalaccel-refuses-config-key", kglobalaccel_refuse)
case("kglobalaccel-no-context", kglobalaccel_no_context)
case("krunner-windows", krunner_windows)
case("alive-after-desktop", lambda info: expect(alive(info), "the compositor stopped answering"))
finish()
