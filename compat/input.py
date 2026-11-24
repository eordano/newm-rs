# fcitx5 runs as a wrapped binary (comm .fcitx5-wrapped, argv[0] "fcitx5" or a path ending in
# /fcitx5): match its command line, never fcitx5-remote's nor the shell that started it.
FCITX = "'(^|/)fcitx5( |$)'"


def fcitx_down():
    """Stops fcitx5 and waits for it to go: an input method left running composes later typing."""
    alice_try("ps -eo pid,comm,args | grep -i [f]citx > /tmp/probe/fcitx-before-down.txt")
    alice_try(f"pkill -f {FCITX}")
    wait(lambda: alice_try(f"pgrep -f {FCITX}")[0] != 0, "fcitx5 gone", 20)
    alice_try("ps -eo pid,comm,args | grep -i [f]citx > /tmp/probe/fcitx-after-down.txt")


def fcitx_up():
    if alice_try("fcitx5-remote")[0] == 0:
        return
    spawn("fcitx5 -r > /tmp/probe/fcitx5.log 2>&1")
    wait(lambda: alice_try("fcitx5-remote")[0] == 0, "fcitx5 on the session bus", 40)
    time.sleep(2)


CHROME = ("chromium --ozone-platform=wayland --disable-gpu --no-first-run --no-default-browser-check"
          " --password-store=basic --enable-wayland-ime --wayland-text-input-version=3"
          " --user-data-dir=/tmp/chromium-proto file:///etc/newm-compat/page.html"
          " > /tmp/probe/chromium-proto.log 2>&1")


def im_case(info, name, cmd, match, read, err=None, wl=None):
    fcitx_up()
    w = probe_window(cmd, match, timeout=150)
    try:
        focus(w)
        time.sleep(2)
        gb = green_box(shot(f"{name}-win"))
        before = shot(f"{name}-before")
        uin("type nihao; sleep 800")
        pre = shot(f"{name}-preedit")
        box = changed_box(before, pre)
        if box and gb and box[1] < gb[1]:
            inside = changed_box(before.crop((0, gb[1], before.size[0], before.size[1])),
                                 pre.crop((0, gb[1], pre.size[0], pre.size[1])))
            box = inside and (inside[0], inside[1] + gb[1], inside[2], inside[3] + gb[1])
        info["window_box"], info["preedit_box"] = gb, box
        uin("tap space; sleep 600; tap enter; sleep 400")
        got = read(w)
        info["read"] = got
        if err:
            text = cat(err)
            info["qt_focus_warnings"] = len(re.findall(r"Trying to (enable|disable)", text))
        info["inactive_commit_log"] = journal_count(r"inactive text input")
        if wl:
            t = cat(wl)
            for k, pat in (("ti_enter", r"zwp_text_input_v3[#@]\d+\.enter"), ("ti_enable", r"zwp_text_input_v3[#@]\d+\.enable"),
                           ("ti_commit", r"zwp_text_input_v3[#@]\d+\.commit\("), ("ti_preedit", r"zwp_text_input_v3[#@]\d+\.preedit_string"),
                           ("ti_commit_string", r"zwp_text_input_v3[#@]\d+\.commit_string"), ("ti_done", r"zwp_text_input_v3[#@]\d+\.done")):
                info[k] = len(re.findall(pat, t))
        expect(got == "你好", f"read {got!r}, not 你好")
        expect(box is not None, "nothing drawn while composing (no preedit, no candidate popup)")
        if gb:
            near = box[0] >= gb[0] - 80 and box[1] >= gb[1] - 80 and box[1] <= gb[3] + 400
            info["popup_near_window"] = near
            expect(near, f"composing drew at {box}, away from the window {gb}")
    finally:
        close(w, match)


def gtk_json(path):
    return lambda w: read_json(path).get("text")


def im_gtk4(info):
    alice_try("rm -f /tmp/probe/im-gtk4.json")
    im_case(info, "im-gtk4", "GTK_IM_MODULE=wayland WAYLAND_DEBUG=1 probe-gtk 4.0 im-gtk4 /tmp/probe/im-gtk4.json"
            " 2> /tmp/probe/im-gtk4.wl", "im-gtk4", gtk_json("/tmp/probe/im-gtk4.json"), wl="/tmp/probe/im-gtk4.wl")


def im_gtk3(info):
    alice_try("rm -f /tmp/probe/im-gtk3.json")
    im_case(info, "im-gtk3", "GTK_IM_MODULE=wayland probe-gtk 3.0 im-gtk3 /tmp/probe/im-gtk3.json", "im-gtk3",
            gtk_json("/tmp/probe/im-gtk3.json"))


def im_qt(info):
    alice_try("rm -f /tmp/probe/im-qt.json")
    im_case(info, "im-qt", "probe-qt im-qt /tmp/probe/im-qt.json 2> /tmp/probe/im-qt.err", "im-qt",
            gtk_json("/tmp/probe/im-qt.json"), err="/tmp/probe/im-qt.err")


def im_chromium(info):
    def read(w):
        t = title_when(w, lambda t: "typed=" in t and "你" in t, "the page's title", 20)
        return re.sub(r" - Chromium$", "", t.split("typed=", 1)[1]).strip()
    im_case(info, "im-chromium", CHROME, "dpr=", read)


def wtype_case(info):
    alice_try("rm -f /tmp/probe/wtype.json")
    info["fcitx_procs"] = alice_try("ps -eo pid,comm,args | grep -i [f]citx")[1][-600:]
    w = probe_window("WAYLAND_DEBUG=1 probe-gtk 4.0 probe-wtype /tmp/probe/wtype.json 2> /tmp/probe/wtype.wl", "probe-wtype")
    try:
        focus(w)
        time.sleep(1)
        alice("wtype 'aé✓'")
        alice("wtype -M shift b -m shift")
        alice("wtype -k Return")
        got = read_json("/tmp/probe/wtype.json").get("text")
        info["read"] = got
        wl = cat("/tmp/probe/wtype.wl")
        info["modifier_events"] = re.findall(r"wl_keyboard[#@]\d+\.modifiers\(\d+, (\d+), (\d+), (\d+)", wl)[-6:]
        expect(got in ("aé✓B", "aé✓b"), f"read {got!r}")
        expect(any(m[0] == "1" for m in info["modifier_events"]), "the client never saw wtype's Shift")
        info["note"] = "wtype's keymap has one level per keysym: Shift+b types b; the modifier reached the client"
    finally:
        close(w, "probe-wtype")


def events_window(name, extra="", color="#3030c0", debug=False):
    alice_try(f"rm -f /tmp/probe/{name}.jsonl /tmp/probe/{name}.wl")
    pre = "WAYLAND_DEBUG=1 " if debug else ""
    w = probe_window(f"{pre}probe gtkx events {name} /tmp/probe/{name}.jsonl color={color} {extra}"
                     f" 2> /tmp/probe/{name}.wl", name)
    focus(w)
    time.sleep(1.5)
    box = wait(lambda: color_box(shot(f"{name}-win"), color), f"{name}'s colour on screen", 15)
    return w, box


def virtual_pointer(info):
    w, box = events_window("probe-vptr", debug=True)
    try:
        uin(f"{abs_to(4, 4)}; sleep 200")
        cx, cy = centre(box)
        alice(f"wlrctl pointer move {int(cx / SCALE) - 3} {int(cy / SCALE) - 3}")
        time.sleep(0.5)
        alice("wlrctl pointer click left")
        info["wl_motion"] = wl_events("probe-vptr", r"wl_pointer[#@]\d+\.motion")
        info["wl_enter"] = wl_events("probe-vptr", r"wl_pointer[#@]\d+\.enter")
        info["wl_left_press"] = wl_events("probe-vptr", r"wl_pointer[#@]\d+\.button\(\d+, \d+, 272, 1\)")
        expect(info["wl_left_press"] > 0, "no left press reached the window")
    finally:
        close(w, "probe-vptr")


def clip_text(info):
    alice("wl-copy compatclip1")
    time.sleep(0.5)
    got = alice("wl-paste -n").strip()
    info["paste"] = got
    log = alice_try("WAYLAND_DEBUG=1 wl-paste -n 2>&1 >/dev/null")[1]
    m = re.search(r"(ext_data_control_manager_v1|zwlr_data_control_manager_v1)[#@]\d+\.get_data_device", log)
    info["manager"] = m.group(1) if m else None
    expect(got == "compatclip1", f"pasted {got!r}")
    alice("wl-copy --primary compatprimary1")
    time.sleep(0.5)
    p = alice("wl-paste --primary -n").strip()
    info["primary"] = p
    expect(p == "compatprimary1", f"primary {p!r}")


def clip_watch(info):
    alice_try("rm -f /tmp/probe/watch.txt")
    alice("setsid -f sh -c \"wl-paste --watch sh -c 'cat >> /tmp/probe/watch.txt; echo >> /tmp/probe/watch.txt'\""
          " > /dev/null 2>&1 < /dev/null")
    time.sleep(1)
    try:
        alice("wl-copy watchone")
        time.sleep(1)
        alice("wl-copy watchtwo")
        got = wait(lambda: "watchtwo" in cat("/tmp/probe/watch.txt")
                   and cat("/tmp/probe/watch.txt"), "both offers watched")
        info["watched"] = got.split()
        expect("watchone" in got, f"watched {got!r}")
    finally:
        kill("wl-paste --watch")


def clip_big(info):
    alice("head -c 10000000 /dev/urandom > /tmp/probe/big.bin")
    alice("wl-copy -t application/x-probe < /tmp/probe/big.bin")
    t0 = time.time()
    alice("wl-paste -t application/x-probe > /tmp/probe/big.out", timeout=60)
    info["secs"] = round(time.time() - t0, 2)
    rc = alice_try("cmp /tmp/probe/big.bin /tmp/probe/big.out")[0]
    expect(rc == 0, "the 10 MB paste differs")
    shot("clip-image-src")
    alice("wl-copy -t image/png < /tmp/shots/clip-image-src.png")
    time.sleep(0.5)
    types = alice("wl-paste -l").split()
    info["types"] = types
    expect("image/png" in types, f"offered types {types}")
    rc = alice_try("wl-paste -t image/png | cmp - /tmp/shots/clip-image-src.png")[0]
    expect(rc == 0, "the PNG paste differs")


def xterm_focused(name):
    w = probe_window(f"xterm -title {name} -e sh -c 'sleep 600'", name)
    focus(w)
    time.sleep(1.5)
    return w


def clip_wl_to_x11(info):
    w = xterm_focused("probe-xclip-a")
    try:
        alice("wl-copy wl2xcompat")
        time.sleep(1)
        alice_try("rm -f /tmp/probe/xpaste.txt")
        spawn("xclip -o -selection clipboard > /tmp/probe/xpaste.txt 2>&1")
        got = wait(lambda: cat("/tmp/probe/xpaste.txt").strip(), "xclip's paste", 10)
        info["xclip"] = got
        expect(got == "wl2xcompat", f"X11 read {got!r}")
    finally:
        close(w, "probe-xclip-a")


def clip_x11_to_wl(info):
    w = xterm_focused("probe-xclip-b")
    try:
        spawn("printf x2wlcompat | xclip -i -selection clipboard")
        got = wait(lambda: alice_try("wl-paste -n")[1].strip() == "x2wlcompat" and "x2wlcompat",
                   "the X11 selection on the Wayland side", 15)
        info["paste"] = got
    finally:
        close(w, "probe-xclip-b")
        kill("xclip -i")


def clip_toolkits(info):
    alice_try("rm -f /tmp/probe/clip-gtk.json /tmp/probe/clip-qt.json")
    g = probe_window("probe-gtk 4.0 clip-gtk /tmp/probe/clip-gtk.json", "clip-gtk")
    q = probe_window("probe-qt clip-qt /tmp/probe/clip-qt.json", "clip-qt")
    c = None
    try:
        focus(g)
        time.sleep(1)
        uin("type toolkitclip; sleep 200; key leftctrl down; tap a; tap c; key leftctrl up; sleep 300")
        focus(q)
        time.sleep(1)
        uin("key leftctrl down; tap v; key leftctrl up; sleep 300; tap enter")
        got = read_json("/tmp/probe/clip-qt.json").get("text")
        info["gtk_to_qt"] = got
        expect(got == "toolkitclip", f"Qt pasted {got!r}")
        uin("tap end; type 2; key leftctrl down; tap a; tap c; key leftctrl up; sleep 300")
        c = probe_window(CHROME.replace("chromium-proto", "chromium-clip"), "dpr=", timeout=150)
        focus(c)
        time.sleep(2)
        uin("key leftctrl down; tap v; key leftctrl up; sleep 300; tap end")
        t = title_when(c, lambda t: "typed=toolkitclip2" in t, "Chromium's paste", 20)
        info["qt_to_chromium"] = t
    finally:
        close(g, "clip-gtk")
        close(q, "clip-qt")
        if c:
            close(c, "user-data-dir=/tmp/chromium-clip")


def dnd_case(info, name, src_cmd, dst_cmd, text, src_extra="", dst_extra=""):
    src_log, dst_log = f"/tmp/probe/{name}-src.jsonl", f"/tmp/probe/{name}-dst.jsonl"
    alice_try(f"rm -f {src_log} {dst_log}")
    s = probe_window(f"{src_cmd} {name}-src {src_log} color=#c03030 text={text} {src_extra}", f"{name}-src")
    d = probe_window(f"{dst_cmd} {name}-dst {dst_log} color=#3030c0 {dst_extra}", f"{name}-dst")
    try:
        time.sleep(2)
        img = shot(f"{name}-before")
        sb, db = color_box(img, "#c03030"), color_box(img, "#3030c0")
        info["src_box"], info["dst_box"] = sb, db
        expect(sb and db, "both windows on screen")
        drag(centre(sb), centre(db))
        got = jwait(dst_log, lambda e: "dropped" in e, "the drop", 10)
        info["dropped"] = got["dropped"]
        info["src_log"] = [e for e in jlines(src_log) if "drag" in e][-3:]
        expect(got["dropped"] == text, f"dropped {got['dropped']!r}")
    finally:
        close(s, f"{name}-src")
        close(d, f"{name}-dst")


def dnd_gtk4_qt(info):
    dnd_case(info, "dnd-gq", "probe gtkx dnd-src", "probe qtx dnd-dst", "gtktoqt")


def dnd_qt_gtk4(info):
    dnd_case(info, "dnd-qg", "probe qtx dnd-src", "probe gtkx dnd-dst", "qttogtk")


def dnd_wl_x11(info):
    dnd_case(info, "dnd-wx", "probe gtkx dnd-src", "GDK_BACKEND=x11 probe gtkx dnd-dst", "wltox", dst_extra="gtk=3")


def dnd_x11_wl(info):
    dnd_case(info, "dnd-xw", "GDK_BACKEND=x11 probe gtkx dnd-src", "probe gtkx dnd-dst", "xtowl", src_extra="gtk=3")


def relative_pointer(info):
    alice_try("rm -f /tmp/probe/sdl.jsonl /tmp/probe/sdl.wl")
    w = probe_window("WAYLAND_DEBUG=1 probe sdl /tmp/probe/sdl.jsonl 40 2> /tmp/probe/sdl.wl", "probe-sdl")
    try:
        focus(w)
        box = wait(lambda: color_box(shot("sdl-win"), "#3030c0"), "the SDL window", 15)
        cx, cy = centre(box)
        uin(f"{abs_to(cx, cy)}; sleep 300")
        jwait("/tmp/probe/sdl.jsonl", lambda d: d.get("relative") is not None, "relative mode asked", 15)
        uin("rel 200 0 10 8; sleep 200; rel 0 120 10 8; sleep 300")
        time.sleep(1)
        log = cat("/tmp/probe/sdl.wl")
        info["lock_asked"] = bool(re.search(r"zwp_pointer_constraints_v1[#@]\d+\.lock_pointer", log))
        info["locked"] = bool(re.search(r"zwp_locked_pointer_v1[#@]\d+\.locked\(", log))
        info["relative_events"] = len(re.findall(r"zwp_relative_pointer_v1[#@]\d+\.relative_motion\(", log))
        ev = [d for d in jlines("/tmp/probe/sdl.jsonl") if "sum_x" in d]
        info["sum"] = (ev[-1]["sum_x"], ev[-1]["sum_y"]) if ev else None
        expect(info["locked"], "the pointer was never locked")
        expect(info["relative_events"] > 5, f"{info['relative_events']} relative motion events")
        expect(info["sum"] and info["sum"][0] > 60 and info["sum"][1] > 20, f"SDL saw {info['sum']}")
    finally:
        close(w, "sdl.py")


def cursor_shape(info):
    w, box = events_window("probe-cursor", "cursor=crosshair", debug=True)
    try:
        cx, cy = centre(box)
        uin(f"{abs_to(4, 4)}; sleep 200; {abs_to(cx, cy)}; sleep 400; {abs_to(cx + 30, cy)}; sleep 400")
        log = cat("/tmp/probe/probe-cursor.wl")
        shapes = re.findall(r"wp_cursor_shape_device_v1[#@]\d+\.set_shape\((\d+)", log, )
        info["shapes"] = shapes
        info["manager_bound"] = "wp_cursor_shape_manager_v1" in log
        expect(shapes, "GTK set no cursor shape (wp_cursor_shape_device_v1)")
    finally:
        close(w, "probe-cursor")


def tablet(info):
    w, box = events_window("probe-pen", debug=True)
    try:
        cx, cy = centre(box)
        uinx(f"pen {cx} {cy} 2560 1080 0.7; sleep 100; penmove {cx + 80} {cy + 40} 2560 1080 0.7; sleep 100; penup; sleep 100; penaway")
        time.sleep(1)
        ev = [d for d in jlines("/tmp/probe/probe-pen.jsonl") if d.get("src") == "pen" or d.get("tool")]
        info["pen_events"] = len(ev)
        info["wl_tablet_tool_events"] = wl_events("probe-pen", r"zwp_tablet_tool_v2[#@]\d+\.(down|motion|pressure)")
        info["wl_tablet_added"] = wl_events("probe-pen", r"zwp_tablet_seat_v2[#@]\d+\.(tablet_added|tool_added)")
        info["types"] = sorted({d.get("t") for d in ev})
        info["max_pressure"] = max([d.get("pressure", 0) for d in ev] or [0])
        pres = [int(v) for v in re.findall(r"zwp_tablet_tool_v2[#@]\d+\.pressure\((\d+)\)",
                                              cat("/tmp/probe/probe-pen.wl"))]
        info["wl_max_pressure"] = max(pres or [0])
        info["wl_tool_down"] = wl_events("probe-pen", r"zwp_tablet_tool_v2[#@]\d+\.down")
        expect(info["wl_tool_down"] > 0, "no tablet tool down reached the window (tablet-v2)")
        expect(info["wl_max_pressure"] > 20000, f"tablet pressure {info['wl_max_pressure']} of 65535")
    finally:
        close(w, "probe-pen")


def touch(info):
    w, box = events_window("probe-touch", debug=True)
    try:
        cx, cy = centre(box)
        uinx(f"touch {cx} {cy} 2560 1080; sleep 100; touchmove {cx + 60} {cy + 30} 2560 1080; sleep 100; touchup")
        time.sleep(1)
        types = [d.get("t") for d in jlines("/tmp/probe/probe-touch.jsonl") if d.get("t")]
        info["wl_touch_down"] = wl_events("probe-touch", r"wl_touch[#@]\d+\.down")
        info["wl_seat_caps"] = re.findall(r"wl_seat[#@]\d+\.capabilities\((\d+)\)", cat("/tmp/probe/probe-touch.wl"))[-2:]
        info["types"] = sorted(set(types))
        info["wl_touch_up"] = wl_events("probe-touch", r"wl_touch[#@]\d+\.up")
        expect(info["wl_touch_down"] > 0 and info["wl_touch_up"] > 0, "no touch down/up reached the window")
    finally:
        close(w, "probe-touch")


def pinch(info):
    w, box = events_window("probe-pinch", debug=True)
    try:
        cx, cy = centre(box)
        uin(f"{abs_to(cx, cy)}; sleep 300")
        uinx("pinch 25 500")
        time.sleep(1)
        ev = [d for d in jlines("/tmp/probe/probe-pinch.jsonl") if d.get("t") == "touchpad_pinch"]
        info["wl_pinch_begin"] = wl_events("probe-pinch", r"zwp_pointer_gesture_pinch_v1[#@]\d+\.begin")
        info["pinch_events"] = len(ev)
        info["max_scale"] = max([d.get("scale", 0) for d in ev] or [0])
        scales = [float(v) for v in re.findall(r"zwp_pointer_gesture_pinch_v1[#@]\d+\.update\(\d+, [-0-9.]+, [-0-9.]+, ([0-9.]+),",
                                               cat("/tmp/probe/probe-pinch.wl"))]
        info["wl_max_scale"] = max(scales or [0])
        expect(info["wl_pinch_begin"] > 0, "no pinch reached the window (pointer-gestures)")
        expect(info["wl_max_scale"] > 1.2, f"pinch scale {info['wl_max_scale']}")
    finally:
        close(w, "probe-pinch")


def trio():
    """Three probe windows, each in its own colour; the last one opened has the focus."""
    wins = []
    for name, colour in (("probe-tp-a", "#c03030"), ("probe-tp-b", "#3030c0"), ("probe-tp-c", "#30a0a0")):
        wins.append((name, colour, events_window(name, color=colour)[0]))
    return wins


def close_trio(wins):
    for name, _colour, w in wins:
        close(w, name)


def focused_id():
    return next((w["id"] for w in windows() if w["is_focused"]), None)


def key_focus(w, action):
    """The window one press of a focus key reaches from w."""
    focus(w)
    msg("action", action)
    time.sleep(0.6)
    return focused_id()


def swipe3_focus_once(info):
    """Three fingers move the focus one window per swipe, however long the swipe is."""
    wins = trio()
    try:
        last = wins[-1][2]
        for action, swipe in (("focus-column-left", "swipe 3 -60 0"), ("focus-column-right", "swipe 3 60 0"),
                              ("focus-window-up", "swipe 3 0 -40"), ("focus-window-down", "swipe 3 0 40")):
            one = key_focus(last, action)
            if one != last["id"]:
                break
        info["key"], info["start"], info["one_key_press"] = action, last["id"], one
        expect(one != last["id"], "no focus key moves the focus from the last window")
        focus(last)
        uin(f"{swipe}; sleep 600", timeout=90)
        info["after_swipe"] = focused_id()
        expect(info["after_swipe"] == one,
               f"a long three-finger swipe focused {info['after_swipe']}, one key press focuses {one}")
    finally:
        close_trio(wins)


def swipe4_zoom(info):
    """Four fingers up pull the camera back (windows are drawn smaller), down bring it closer; no
    overview opens."""
    wins = trio()
    try:
        colour = wins[-1][1]

        def width(name):
            box = wait(lambda: color_box(shot(name), colour), name, 15)
            return box[2] - box[0]

        info["before"] = width("tp-zoom-before")
        uin("swipe 4 0 -30; sleep 800", timeout=90)
        info["pulled_back"] = width("tp-zoom-out")
        try:
            info["overview"] = msg("overview-state").strip()[:60]
        except Exception as e:
            info["overview"] = f"unknown: {e}"[:60]
        expect(info["pulled_back"] < info["before"] * 0.95,
               f"four fingers up left the window {info['pulled_back']} px wide, from {info['before']}")
        uin("swipe 4 0 30; sleep 800", timeout=90)
        info["closer"] = width("tp-zoom-in")
        expect(info["closer"] > info["pulled_back"] * 1.05,
               f"four fingers down left the window {info['closer']} px wide, from {info['pulled_back']}")
    finally:
        msg("action", "zoom-reset")
        close_trio(wins)


def super_swipe3_pan(info):
    """Super and three fingers pan the camera after the fingers; the windows keep their tiles."""
    wins = trio()
    try:
        colour = wins[-1][1]

        def tiles():
            return sorted((w["id"], str(w["layout"].get("pos_in_scrolling_layout"))) for w in windows())

        before_tiles = tiles()
        before = wait(lambda: color_box(shot("tp-pan-before"), colour), "the window before the pan", 15)
        uin("key leftmeta down; sleep 250; swipe 3 -25 12; sleep 250", timeout=90)
        after = color_box(shot("tp-pan-held"), colour)
        uin("key leftmeta up; sleep 600")
        info["before"], info["after"] = before, after
        expect(after is not None, "the window left the screen")
        info["moved"] = (after[0] - before[0], after[1] - before[1])
        expect(info["moved"][0] < -40, f"fingers going left moved the window by {info['moved']}")
        expect(tiles() == before_tiles, "a pan moved a window to other tiles")
    finally:
        close_trio(wins)


MINIMAP_BOX = (0, H_PHYS - 230, 330, H_PHYS)


def minimap(info):
    """A focus move shows the minimap in the bottom-left corner, with the screen's near-white
    outline, and it goes again."""
    wins = trio()
    try:
        first, last = wins[0][2], wins[-1][2]
        focus(last)
        time.sleep(2.5)
        # The plan stays 750 ms: the shot is taken by the command that moves the focus, a moment
        # after it, since the action's reply can come before the frame that draws the plan.
        alice(f"{IPC} msg action focus-window --id {first['id']} && sleep 0.2 && grim /tmp/shots/tp-minimap-on.png")
        machine.copy_from_vm("/tmp/shots/tp-minimap-on.png", "shots")
        shown = Image.open(os.path.join(OUT, "shots", "tp-minimap-on.png")).convert("RGB").crop(MINIMAP_BOX)
        time.sleep(2.5)
        gone = shot("tp-minimap-off").crop(MINIMAP_BOX)
        big = shown.resize((shown.width * 3, shown.height * 3), Image.NEAREST)
        big.save(os.path.join(OUT, "shots", "tp-minimap-crop.png"))
        info["changed"] = changed_box(gone, shown)
        info["outline"] = color_box(shown, "#ddf7ff", tol=40, step=1, min_n=20)
        info["outline_after"] = color_box(gone, "#ddf7ff", tol=40, step=1, min_n=20)
        expect(info["changed"] is not None, "nothing was drawn in the corner after a focus move")
        expect(info["outline"] is not None, "no outline of the screen in the corner")
        expect(info["outline_after"] is None, "the minimap stayed")
        # The plan is 240x160 logical pixels, 12 from the left and bottom edges, and the screen's
        # outline sits in its middle: a plan drawn at the wrong scale puts it elsewhere.
        cx, cy = centre(info["outline"])
        want = ((12 + 120) * SCALE, MINIMAP_BOX[3] - MINIMAP_BOX[1] - (12 + 80) * SCALE)
        info["outline_centre"], info["plan_centre"] = (cx, cy), want
        expect(abs(cx - want[0]) <= 8 and abs(cy - want[1]) <= 8,
               f"the screen's outline is centred at {(cx, cy)}, the plan's middle is {want}")
    finally:
        close_trio(wins)


def shortcuts_inhibit(info):
    alice_try("rm -f /tmp/probe/bind-fired")
    w, _ = events_window("probe-noinhibit")
    try:
        uin("key leftmeta down; tap f12; key leftmeta up; sleep 500")
        info["bind_without_inhibit"] = alice_try("test -e /tmp/probe/bind-fired")[0] == 0
    finally:
        close(w, "probe-noinhibit")
    alice_try("rm -f /tmp/probe/bind-fired")
    w, _ = events_window("probe-inhibit", "inhibit=1", debug=True)
    try:
        got = jwait("/tmp/probe/probe-inhibit.jsonl", lambda d: "shortcuts_inhibited" in d,
                    "the inhibitor's answer", 10)
        info["inhibited"] = got["shortcuts_inhibited"]
        uin("key leftmeta down; tap f12; key leftmeta up; sleep 500")
        info["bind_with_inhibit"] = alice_try("test -e /tmp/probe/bind-fired")[0] == 0
        keys = wl_events("probe-inhibit", r"wl_keyboard[#@]\d+\.key\(\d+, \d+, 88, 1\)")
        info["client_got_f12"] = bool(keys)
        expect(info["bind_without_inhibit"], "Super+F12 did not fire without an inhibitor (test bind)")
        expect(info["inhibited"], "the compositor did not grant the inhibitor")
        expect(not info["bind_with_inhibit"] and keys, "Super+F12 still went to the compositor")
    finally:
        close(w, "probe-inhibit")


def devices(info):
    text = sh("libinput list-devices 2>&1")
    devs = {}
    for block in text.split("\n\n"):
        m = re.search(r"Device:\s+(.*)", block)
        c = re.search(r"Capabilities:\s+(.*)", block)
        if m and ("uin" in m.group(1)):
            devs[m.group(1).strip()] = c.group(1).strip() if c else None
    info["devices"] = devs
    expect("tablet" in (devs.get("uinx pen tablet") or ""), f"libinput devices {devs}")
    expect("touch" in (devs.get("uinx touchscreen") or ""), f"libinput devices {devs}")


def wl_events(name, pattern):
    return len(re.findall(pattern, cat(f"/tmp/probe/{name}.wl")))


wait_session()
case("uinput-devices", devices)
case("im-fcitx5-gtk4", im_gtk4)
case("im-fcitx5-gtk3", im_gtk3)
case("im-fcitx5-qt", im_qt)
case("im-fcitx5-chromium", im_chromium)
fcitx_down()
case("wtype-unicode", wtype_case)
case("virtual-pointer", virtual_pointer)
case("clipboard-text-primary", clip_text)
case("clipboard-watch", clip_watch)
case("clipboard-10mb-image", clip_big)
case("clipboard-wayland-to-x11", clip_wl_to_x11)
case("clipboard-x11-to-wayland", clip_x11_to_wl)
case("clipboard-gtk-qt-chromium", clip_toolkits)
case("dnd-gtk4-to-qt", dnd_gtk4_qt)
case("dnd-qt-to-gtk4", dnd_qt_gtk4)
case("dnd-wayland-to-x11", dnd_wl_x11)
case("dnd-x11-to-wayland", dnd_x11_wl)
case("relative-pointer-lock", relative_pointer)
case("cursor-shape", cursor_shape)
case("tablet-pen", tablet)
case("touchscreen", touch)
case("touchpad-pinch", pinch)
case("touchpad-swipe3-focus-once", swipe3_focus_once)
case("touchpad-swipe4-zoom", swipe4_zoom)
case("touchpad-super-swipe3-pan", super_swipe3_pan)
case("minimap", minimap)
case("keyboard-shortcuts-inhibit", shortcuts_inhibit)
case("alive-after-input", lambda info: expect(alive(info), "the compositor stopped answering"))
finish()
