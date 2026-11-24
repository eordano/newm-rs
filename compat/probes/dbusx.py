"""D-Bus probes of the protocol cases (Gio): screenshots and screencasts through the compositor's
own APIs and through xdg-desktop-portal. Prints one JSON object.

    dbusx.py portal-screenshot          org.freedesktop.portal.Screenshot, interactive=false
    dbusx.py mutter-cast CONNECTOR       org.gnome.Mutter.ScreenCast RecordMonitor -> node id
    dbusx.py portal-cast                 org.freedesktop.portal.ScreenCast (monitor, no dialog
                                         expected: portal-wlr's chooser is off) -> node id + fd
    dbusx.py ss-inhibit SECONDS          org.freedesktop.ScreenSaver.Inhibit, the connection kept
                                         open for SECONDS, then the process exits (no UnInhibit)
    dbusx.py portal-inhibit SECONDS      org.freedesktop.portal.Inhibit (idle) held for SECONDS
For a cast, `frames=N` (default 5) pulls N buffers from the node with gst-launch's pipewiresrc.
"""

import json
import os
import subprocess
import sys
import time

from gi.repository import Gio, GLib

bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
CMD = sys.argv[1]
OPT = dict(a.split("=", 1) for a in sys.argv[2:] if "=" in a)
POS = [a for a in sys.argv[2:] if "=" not in a]
res = {"cmd": CMD}
loop = GLib.MainLoop()


def call(name, path, iface, method, args, sig=None, fds=None, timeout=20000):
    return bus.call_with_unix_fd_list_sync(name, path, iface, method, args,
                                           GLib.VariantType(sig) if sig else None,
                                           Gio.DBusCallFlags.NONE, timeout, fds, None)


def portal_request(method_iface, method, args, token):
    """Calls a portal method and waits for its Request's Response."""
    sender = bus.get_unique_name()[1:].replace(".", "_")
    path = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"
    got = {}

    def on_resp(_c, _s, _p, _i, _sig, params):
        got["code"], got["results"] = params.unpack()
        loop.quit()

    sub = bus.signal_subscribe("org.freedesktop.portal.Desktop", "org.freedesktop.portal.Request",
                               "Response", path, None, Gio.DBusSignalFlags.NO_MATCH_RULE, on_resp)
    call("org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", method_iface, method, args)
    tid = GLib.timeout_add_seconds(25, loop.quit)
    loop.run()
    GLib.source_remove(tid) if "code" in got else None
    bus.signal_unsubscribe(sub)
    return got.get("code"), got.get("results")


def pull(node, fd=None):
    frames = int(OPT.get("frames", "5"))
    gst = ["gst-launch-1.0", "-q", "pipewiresrc"]
    if fd is not None:
        gst += [f"fd={fd}"]
    gst += [f"path={node}", f"num-buffers={frames}", "!", "fakesink"]
    try:
        p = subprocess.run(gst, capture_output=True, text=True, timeout=30,
                           pass_fds=[fd] if fd is not None else [])
        res["gst_rc"] = p.returncode
        res["gst_err"] = p.stderr[-400:]
    except subprocess.TimeoutExpired:
        res["gst_rc"] = "timeout"


try:
    if CMD == "portal-screenshot":
        code, r = portal_request("org.freedesktop.portal.Screenshot", "Screenshot",
                                 GLib.Variant("(sa{sv})", ("", {"handle_token": GLib.Variant("s", "shot1"),
                                                                "interactive": GLib.Variant("b", False)})),
                                 "shot1")
        res["code"], res["uri"] = code, (r or {}).get("uri")
        if res["uri"]:
            p = res["uri"].replace("file://", "")
            res["bytes"] = os.path.getsize(p) if os.path.exists(p) else None
    elif CMD == "mutter-cast":
        conn = POS[0]
        nodes = {}
        r = call("org.gnome.Mutter.ScreenCast", "/org/gnome/Mutter/ScreenCast",
                 "org.gnome.Mutter.ScreenCast", "CreateSession", GLib.Variant("(a{sv})", ({},)), "(o)")
        spath = r[0].unpack()[0]
        r = call("org.gnome.Mutter.ScreenCast", spath, "org.gnome.Mutter.ScreenCast.Session",
                 "RecordMonitor", GLib.Variant("(sa{sv})", (conn, {})), "(o)")
        stream = r[0].unpack()[0]

        def added(_c, _s, _p, _i, _sig, params):
            nodes["id"] = params.unpack()[0]
            loop.quit()

        bus.signal_subscribe(None, "org.gnome.Mutter.ScreenCast.Stream", "PipeWireStreamAdded", stream,
                             None, Gio.DBusSignalFlags.NONE, added)
        call("org.gnome.Mutter.ScreenCast", spath, "org.gnome.Mutter.ScreenCast.Session", "Start",
             GLib.Variant("()", ()))
        GLib.timeout_add_seconds(15, loop.quit)
        loop.run()
        res["node"] = nodes.get("id")
        if res["node"] is not None:
            pull(res["node"])
        call("org.gnome.Mutter.ScreenCast", spath, "org.gnome.Mutter.ScreenCast.Session", "Stop",
             GLib.Variant("()", ()))
    elif CMD == "portal-cast":
        P = "org.freedesktop.portal.ScreenCast"
        code, r = portal_request(P, "CreateSession", GLib.Variant("(a{sv})", ({
            "handle_token": GLib.Variant("s", "c1"), "session_handle_token": GLib.Variant("s", "s1")},)), "c1")
        sess = (r or {}).get("session_handle")
        res["create"] = code
        code, r = portal_request(P, "SelectSources", GLib.Variant("(oa{sv})", (sess, {
            "handle_token": GLib.Variant("s", "c2"), "types": GLib.Variant("u", 1)})), "c2")
        res["select"] = code
        code, r = portal_request(P, "Start", GLib.Variant("(osa{sv})", (sess, "", {
            "handle_token": GLib.Variant("s", "c3")})), "c3")
        res["start"] = code
        streams = (r or {}).get("streams") or []
        res["node"] = streams[0][0] if streams else None
        if res["node"] is not None:
            reply, fdl = call("org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", P,
                              "OpenPipeWireRemote", GLib.Variant("(oa{sv})", (sess, {})), "(h)")
            fd = fdl.get(reply.unpack()[0])
            pull(res["node"], fd)
    elif CMD == "ss-inhibit":
        r = call("org.freedesktop.ScreenSaver", "/org/freedesktop/ScreenSaver", "org.freedesktop.ScreenSaver",
                 "Inhibit", GLib.Variant("(ss)", ("probe-ss", "compat test")), "(u)")
        res["cookie"] = r[0].unpack()[0]
        print(json.dumps(res), flush=True)
        time.sleep(float(POS[0]))
        res["held"] = True
    elif CMD == "portal-inhibit":
        sender = bus.get_unique_name()[1:].replace(".", "_")
        r = call("org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
                 "org.freedesktop.portal.Inhibit", "Inhibit",
                 GLib.Variant("(sua{sv})", ("", 8, {"handle_token": GLib.Variant("s", "inh1"),
                                                    "reason": GLib.Variant("s", "compat test")})), "(o)")
        res["request"] = r[0].unpack()[0]
        print(json.dumps(res), flush=True)
        time.sleep(float(POS[0]))
        res["held"] = True
except Exception as e:  # noqa: BLE001
    res["error"] = str(e)
print(json.dumps(res))
