def inventory(info):
    alice(f"wayland-info > /tmp/probe/inventory-{COMPOSITOR}.txt 2>&1")
    text = alice(f"cat /tmp/probe/inventory-{COMPOSITOR}.txt")
    globs = sorted(set(re.findall(r"interface: '([^']+)',\s+version:\s+(\d+)", text)))
    with open(os.path.join(OUT, f"globals-{COMPOSITOR}.txt"), "w") as f:
        f.write("".join(f"{g} {v}\n" for g, v in globs))
    info["globals"] = len(globs)
    expect(any(g == "xdg_wm_base" for g, _ in globs), "no xdg_wm_base")


def inventory_kwin(info):
    alice("setsid -f kwin-ref > /tmp/probe/kwin.log 2>&1 < /dev/null")
    try:
        wait(lambda: machine.execute("test -S /run/user/1000/kwin-ref")[0] == 0, "KWin's socket", 60)
        alice("WAYLAND_DISPLAY=kwin-ref wayland-info > /tmp/probe/inventory-kwin.txt 2>&1")
        text = alice("cat /tmp/probe/inventory-kwin.txt")
        globs = sorted(set(re.findall(r"interface: '([^']+)',\s+version:\s+(\d+)", text)))
        with open(os.path.join(OUT, "globals-kwin.txt"), "w") as f:
            f.write("".join(f"{g} {v}\n" for g, v in globs))
        info["globals"] = len(globs)
        info["kwin"] = alice_try("kwin_wayland --version")[1].strip()
        expect(len(globs) > 20, f"KWin listed only {len(globs)} globals")
    finally:
        alice_try("pkill -f '[k]win_wayland --virtual'")


def drew_check(info, before, name):
    after = shot(name)
    ch, cols, ok = drew(before, after)
    info["changed"], info["colours"] = round(ch, 3), cols
    expect(ok, f"too little drawn: {ch:.3f} of the screen changed, {cols} colours")
    return after


def term_case(info, app, cmd_prefix, match):
    alice_try(f"rm -f /tmp/probe/{app}.txt")
    before = shot(f"{app}-before")
    w = new_window(f"{cmd_prefix} sh -c 'read x; echo \"$x\" > /tmp/probe/{app}.txt; sleep 600'",
                   app, match=match)
    info["app_id"], info["title"] = w.get("app_id"), w.get("title")
    try:
        focus(w)
        time.sleep(1)
        word = f"{app}compat42"
        type_text(word)
        got = wait(lambda: cat(f"/tmp/probe/{app}.txt").strip(), f"{app}'s input")
        info["read"] = got
        expect(got == word, f"{app} read {got!r}, not {word!r}")
        drew_check(info, before, app)
    finally:
        close(w, match)


def foot(info):
    term_case(info, "foot", "foot --app-id=probe-foot", "probe-foot")


def xterm(info):
    info["DISPLAY"] = alice_try(f"{IPC} msg action spawn-sh -- 'echo $DISPLAY > /tmp/probe/display'"
                                " && sleep 1 && cat /tmp/probe/display")[1].strip()
    term_case(info, "xterm", "xterm -title probe-xterm -e", "probe-xterm")


def probe_case(info, name, cmd, word, checks):
    alice_try(f"rm -f /tmp/probe/{name}.json /tmp/probe/{name}.wl")
    before = shot(f"{name}-before")
    w = new_window(f"WAYLAND_DEBUG=1 {cmd} {name} /tmp/probe/{name}.json 2> /tmp/probe/{name}.wl",
                   name, match=name)
    info["app_id"] = w.get("app_id")
    try:
        focus(w)
        time.sleep(1)
        type_text(word)
        d = read_json(f"/tmp/probe/{name}.json")
        info.update(d)
        info.update(wayland_scale(f"/tmp/probe/{name}.wl"))
        expect(d["text"] == word, f"read {d['text']!r}, not {word!r}")
        after = drew_check(info, before, name)
        box = green_box(after)
        info["green_box"] = box
        expect(box is not None, "no probe green in the shot")
        checks(info)
    finally:
        close(w, name)


def fractional(info, reported):
    offered = info.get("preferred_scales") or []
    expect(offered and offered[-1] == SCALE, f"fractional scales offered {offered}, last not {SCALE}")
    expect(reported is not None and abs(reported - SCALE) < 0.01, f"client scale {reported}, not {SCALE}")
    r = info.get("ratio")
    expect(r is not None and abs(r - SCALE) < 0.03,
           f"buffer {info.get('buffer')} / viewport {info.get('destination')} = {r}, not {SCALE}")


def gtk4(info):
    probe_case(info, "probe-gtk4", "probe-gtk 4.0", "gtkfourcompat",
               lambda i: fractional(i, i.get("scale")))


def gtk3(info):
    def checks(i):
        expect(i.get("scale_factor") == 2, f"GTK3 scale factor {i.get('scale_factor')}, not 2 (ceil 1.25)")
    probe_case(info, "probe-gtk3", "probe-gtk 3.0", "gtkthreecompat", checks)


def qt(info):
    def checks(i):
        expect(i.get("platform") == "wayland", f"Qt platform {i.get('platform')}")
        expect((i.get("style") or "").lower() == "breeze", f"Qt style {i.get('style')}, not Breeze (KDE theme)")
        fractional(i, i.get("dpr"))
    probe_case(info, "probe-qt", "probe-qt", "qtkdecompat", checks)


def app_case(info, name, cmd, match=None, kill=None, typed=None):
    before = shot(f"{name}-before")
    w = new_window(f"{cmd} > /tmp/probe/{name}.log 2>&1", name, match=match, timeout=120)
    info["app_id"], info["title"] = w.get("app_id"), w.get("title")
    try:
        time.sleep(3)
        if typed:
            focus(w)
            t0 = get(w["id"])["title"]
            type_text(typed)
            t1 = title_when(w, lambda t: t != t0, f"{name}'s title to change after typing")
            info["title_after"] = t1
        drew_check(info, before, name)
    finally:
        close(w, kill)


def gtk4_widget_factory(info):
    app_case(info, "gtk4-widget-factory", "gtk4-widget-factory", kill="gtk4-widget-factory")


def gtk3_widget_factory(info):
    app_case(info, "gtk3-widget-factory", "gtk3-widget-factory", kill="gtk3-widget-factory")


def kwrite(info):
    app_case(info, "kwrite", "kwrite /tmp/probe/kwrite.txt", kill="kwrite", typed="kwritecompat")


def libadwaita(info):
    alice_try("rm -f /tmp/probe/zenity.txt")
    try:
        refs = sh("nix-store -q --references $(readlink -f /run/current-system/sw/bin/zenity"
                  " | sed 's|/bin/.*||') | grep -c libadwaita || true").strip()
        info["links_libadwaita"] = refs != "0"
    except Exception:
        pass
    before = shot("zenity-before")
    w = new_window("zenity --entry --title=probe-zenity --text=compat > /tmp/probe/zenity.txt",
                   "zenity", match="probe-zenity")
    info["app_id"] = w.get("app_id")
    try:
        focus(w)
        time.sleep(1)
        drew_check(info, before, "zenity")
        type_text("adwaitacompat")
        got = wait(lambda: cat("/tmp/probe/zenity.txt").strip(), "zenity's answer")
        info["read"] = got
        expect(got == "adwaitacompat", f"zenity printed {got!r}")
    finally:
        close(w, "probe-zenity")


def chromium(info):
    before = shot("chromium-before")
    w = new_window("chromium --ozone-platform=wayland --disable-gpu --no-first-run"
                   " --no-default-browser-check --password-store=basic"
                   " --user-data-dir=/tmp/chromium-probe file:///etc/newm-compat/page.html"
                   " > /tmp/probe/chromium.log 2>&1", "Chromium", match="dpr=", timeout=150)
    info["app_id"], info["title"] = w.get("app_id"), w.get("title")
    try:
        m = re.search(r"dpr=([0-9.]+)", w["title"])
        info["dpr_first_title"] = float(m.group(1)) if m else None
        focus(w)
        time.sleep(2)
        type_text("chromecompat")
        t = title_when(w, lambda t: "typed=chromecompat" in t, "the page to read the typed text", 30)
        info["title_after"] = t
        m = re.search(r"dpr=([0-9.]+)", t)
        info["dpr"] = float(m.group(1)) if m else None
        drew_check(info, before, "chromium")
        expect(info["dpr"] is not None and abs(info["dpr"] - SCALE) < 0.01,
               f"devicePixelRatio {info['dpr']}, not {SCALE}")
    finally:
        close(w, "user-data-dir=/tmp/chromium-probe")


wait_session()
case("inventory", inventory)
case("foot", foot)
case("gtk4-probe", gtk4)
case("gtk4-widget-factory", gtk4_widget_factory)
case("gtk3-probe", gtk3)
case("gtk3-widget-factory", gtk3_widget_factory)
case("libadwaita-zenity", libadwaita)
case("qt-kde-probe", qt)
case("kwrite", kwrite)
case("xterm-xwayland", xterm)
case("chromium", chromium)
case("kwin-reference", inventory_kwin)
finish()
