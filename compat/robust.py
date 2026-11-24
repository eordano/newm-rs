# A failure is a compositor that stops answering, dies, freezes input, or sends a well-behaved
# client a protocol error.

WESTON = ["clickdot", "cliptest", "constraints", "dnd", "eventdemo", "flower", "fullscreen",
          "multi-resource", "presentation-shm", "resizor", "scaler", "smoke", "stacking",
          "subsurfaces", "tablet", "terminal", "transformed"]
PROTO_ERR = re.compile(r"(?i)(protocol error|error in client communication|wl_display@1: error|[a-z_0-9]+[#@]\d+: error \d+:)")
# Clients that break the protocol themselves: the error they get is the compositor doing its job.
CLIENT_BUGS = {"presentation-shm": "commits a buffer before its first configure (weston client bug)"}


def weston(info, name):
    log = f"/tmp/probe/weston-{name}.log"
    before = {w["id"] for w in windows()}
    spawn(f"weston-{name} > {log} 2>&1")
    try:
        w = wait(lambda: next((w for w in windows() if w["id"] not in before), None), f"weston-{name}'s window", 25)
        info["app_id"], info["title"] = w.get("app_id"), w.get("title")
    except Fail:
        info["no_window"] = True
    time.sleep(3)
    text = cat(log)
    info["log_tail"] = text[-300:]
    err = PROTO_ERR.search(text)
    expect(alive(info), "the compositor stopped answering")
    kill(f"weston-{name}")
    time.sleep(0.5)
    if err and name in CLIENT_BUGS:
        info["client_bug"] = CLIENT_BUGS[name]
        info["error_sent"] = err.group(0)
        err = None
    expect(not err, f"protocol error: {text[-300:]}")
    expect(not info.get("no_window") or name == "multi-resource", "no window")


def weston_cases():
    for n in WESTON:
        case(f"weston-{n}", lambda info, n=n: weston(info, n))


def wl(mode, *args, timeout=120):
    log = f"/tmp/probe/wl-{mode}.jsonl"
    alice_try(f"rm -f {log}")
    rc, outp = alice_try(f"timeout {timeout} probe wl {mode} {log} {' '.join(args)} 2>&1", timeout=timeout + 30)
    return rc, outp, jlines(log)


def responsive(info):
    """Input still reaches a fresh window: the compositor is not stuck in a grab."""
    alice_try("rm -f /tmp/probe/after.json")
    w = probe_window("WAYLAND_DEBUG=1 probe-gtk 4.0 probe-after /tmp/probe/after.json 2> /tmp/probe/after.wl",
                     "probe-after")
    try:
        focus(w)
        time.sleep(1)
        g = green_box(shot("responsive"))
        if g:
            uin(f"{abs_to(*centre(g))}; sleep 200; click; sleep 300")
        type_text("stillhere")
        try:
            got = read_json("/tmp/probe/after.json").get("text")
        except Exception:
            wl = cat("/tmp/probe/after.wl")
            info["after_wl_keys"] = len(re.findall(r"wl_keyboard[#@]\d+\.key\(", wl))
            info["after_wl_enter"] = len(re.findall(r"wl_keyboard[#@]\d+\.enter\(", wl))
            info["after_wl_leave"] = len(re.findall(r"wl_keyboard[#@]\d+\.leave\(", wl))
            info["after_wl_tail"] = wl[-1500:]
            info["focused"] = alice_try(f"{IPC} msg -j focused-window")[1][-400:]
            raise
        info["after_text"] = got
        expect(got == "stillhere", f"a new window read {got!r}")
    finally:
        close(w, "probe-after")


def noack(info):
    rc, outp, log = wl("noack", timeout=30)
    info["client"] = log[-3:]
    info["protocol_error"] = next((d["protocol_error"] for d in log if "protocol_error" in d), None)
    expect(alive(info), "the compositor stopped answering")
    responsive(info)


def huge(info):
    r0 = rss_kb()
    rc, outp, log = wl("huge", "16384", timeout=60)
    info["client"] = log[-3:]
    info["rss_kb_before"], info["rss_kb_after"] = r0, rss_kb()
    expect(alive(info), "the compositor stopped answering")
    responsive(info)


def flap(info):
    r0 = rss_kb()
    t0 = time.time()
    rc, outp, log = wl("flap", "1000", timeout=600)
    info["secs"] = round(time.time() - t0, 1)
    info["client"] = log[-2:]
    info["rss_kb_before"], info["rss_kb_after"] = r0, rss_kb()
    done = next((d["flapped"] for d in log if "flapped" in d), 0)
    info["flapped"] = done
    expect(alive(info), "the compositor stopped answering")
    expect(done == 1000, f"only {done} of 1000 map/unmap cycles ({log[-1:] if log else outp[-200:]})")
    expect(not windows() or not any("probe-flap" in (w.get("title") or "") for w in windows()),
           "a flapped toplevel is still listed")


def crash_mid(info, kind):
    log = f"/tmp/probe/wl-grab-{kind}.jsonl"
    alice_try(f"rm -f {log}")
    spawn(f"probe wl grab {log} {kind} > /tmp/probe/wl-grab-{kind}.out 2>&1")
    jwait(log, lambda d: d.get("ready"), "the grab probe mapped", 20)
    time.sleep(1)
    g = wait(lambda: green_box(shot(f"grab-{kind}")), "the grab probe on screen", 10)
    cx, cy = centre(g)
    uin(f"{abs_to(cx, cy)}; sleep 200; button left down; sleep 900; "
        f"{abs_to(cx + 120, cy + 60)}; sleep 300; {abs_to(cx + 200, cy + 90)}; sleep 300; button left up")
    info["client"] = jlines(log)[-3:]
    expect(any(d.get("started") == kind for d in jlines(log)), f"the client never started its {kind}")
    expect(alive(info), "the compositor stopped answering")
    responsive(info)


wait_session()
weston_cases()
case("misbehave-never-acks-configure", noack)
case("misbehave-16384px-buffer", huge)
case("misbehave-map-unmap-1000", flap)
case("crash-mid-interactive-move", lambda info: crash_mid(info, "move"))
case("crash-mid-interactive-resize", lambda info: crash_mid(info, "resize"))
case("crash-mid-popup-grab", lambda info: crash_mid(info, "popup"))
case("alive-after-robust", lambda info: expect(alive(info), "the compositor stopped answering"))
finish()
