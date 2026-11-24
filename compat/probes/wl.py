"""Raw Wayland probes (pywayland): misbehaving clients and a security-context listener.

    wl.py MODE OUT [args]

Modes (each logs JSON lines to OUT, including any protocol error it was sent):
    noack          an xdg toplevel that never acks a configure and attaches a buffer anyway
    huge N         a toplevel that commits an N x N shm buffer (sparse memfd)
    flap N         maps and unmaps a toplevel N times (role objects made and destroyed each time)
    grab KIND      maps a toplevel; on the first pointer button press starts KIND (move, resize
                   or popup with a grab) with that serial, then exits abruptly 0.3 s later
    secctx SOCK    opens a security-context listener at SOCK (engine org.newm.compat, app probe)
                   and stays up until killed
"""

import json
import mmap
import os
import socket
import sys
import time

from pywayland.client import Display
from pywayland.protocol.wayland import WlCompositor, WlSeat, WlShm
from pywayland.protocol.xdg_shell import XdgToplevel, XdgWmBase

MODE, OUT = sys.argv[1], sys.argv[2]
ARGS = sys.argv[3:]
out = open(OUT, "a", buffering=1)
T0 = time.monotonic()


def log(**d):
    d["at"] = round(time.monotonic() - T0, 3)
    out.write(json.dumps(d) + "\n")


display = Display()
try:
    display.connect()
except Exception as e:  # noqa: BLE001
    log(connect_error=str(e))
    sys.exit(2)
G = {}


def on_global(registry, name, iface, version):
    G[iface] = (name, version)


registry = display.get_registry()
registry.dispatcher["global"] = on_global
display.roundtrip()
log(globals=len(G))


def bind(iface_cls, version=1):
    name, v = G[iface_cls.name]
    return registry.bind(name, iface_cls, min(version, v))


def rt():
    try:
        display.roundtrip()
        return True
    except Exception as e:  # noqa: BLE001
        log(protocol_error=str(e))
        return False


compositor = bind(WlCompositor, 4)
shm = bind(WlShm, 1)
wm = bind(XdgWmBase, 2)
wm.dispatcher["ping"] = lambda w, serial: w.pong(serial)
seat = bind(WlSeat, 5)


def buffer(w, h, color=0xFF30C030):
    stride = w * 4
    size = stride * h
    fd = os.memfd_create("probe", 0)
    os.ftruncate(fd, size)
    if size <= 64 * 1024 * 1024:
        m = mmap.mmap(fd, size)
        m.write(color.to_bytes(4, "little") * (w * h))
        m.close()
    pool = shm.create_pool(fd, size)
    buf = pool.create_buffer(0, w, h, stride, WlShm.format.argb8888.value)
    pool.destroy()
    os.close(fd)
    return buf


class Top:
    def __init__(self, title, ack=True):
        self.surface = compositor.create_surface()
        self.xs = wm.get_xdg_surface(self.surface)
        self.top = self.xs.get_toplevel()
        self.top.set_title(title)
        self.top.set_app_id(title)
        self.configured = None
        self.size = (0, 0)
        self.ack = ack

        def xs_configure(xs, serial):
            self.configured = serial
            if self.ack:
                xs.ack_configure(serial)

        def top_configure(t, w, h, states):
            self.size = (w, h)

        self.xs.dispatcher["configure"] = xs_configure
        self.top.dispatcher["configure"] = top_configure
        self.top.dispatcher["close"] = lambda t: log(close_asked=True)
        self.surface.commit()

    def wait_configure(self, timeout=5):
        end = time.time() + timeout
        while self.configured is None and time.time() < end:
            if not rt():
                return False
        return self.configured is not None

    def map(self, w=None, h=None, color=0xFF30C030):
        w = w or (self.size[0] or 400)
        h = h or (self.size[1] or 300)
        self.surface.attach(buffer(w, h, color), 0, 0)
        self.surface.damage_buffer(0, 0, w, h)
        self.surface.commit()

    def destroy(self):
        self.top.destroy()
        self.xs.destroy()
        self.surface.destroy()


def noack():
    t = Top("probe-noack", ack=False)
    log(configured=t.wait_configure())
    t.map()
    ok = rt() and rt()
    log(survived_client=ok)


def huge():
    n = int(ARGS[0]) if ARGS else 16384
    t = Top("probe-huge")
    t.wait_configure()
    t.map(n, n)
    ok = rt()
    log(committed=n, ok=ok)
    end = time.time() + 8
    while time.time() < end and ok:
        ok = rt()
        time.sleep(0.5)
    log(alive=ok)


def flap():
    n = int(ARGS[0]) if ARGS else 1000
    done = 0
    for i in range(n):
        t = Top(f"probe-flap")
        if not t.wait_configure():
            log(stuck_at=i)
            break
        t.map(200, 150)
        if not rt():
            break
        t.destroy()
        if not rt():
            break
        done += 1
    log(flapped=done)


def grab():
    kind = ARGS[0]
    t = Top(f"probe-grab-{kind}")
    t.wait_configure()
    t.map(600, 400)
    rt()
    ptr = seat.get_pointer()
    state = {"serial": None}
    ptr.dispatcher["button"] = lambda p, serial, tm, button, st: state.update(serial=serial) if st == 1 else None
    log(ready=True)
    end = time.time() + 30
    while state["serial"] is None and time.time() < end:
        if not rt():
            return
        time.sleep(0.02)
    if state["serial"] is None:
        log(no_press=True)
        return
    s = state["serial"]
    if kind == "move":
        t.top.move(seat, s)
    elif kind == "resize":
        t.top.resize(seat, s, XdgToplevel.resize_edge.bottom_right.value)
    else:
        pos = wm.create_positioner()
        pos.set_size(300, 200)
        pos.set_anchor_rect(10, 10, 1, 1)
        psurf = compositor.create_surface()
        pxs = wm.get_xdg_surface(psurf)
        popup = pxs.get_popup(t.xs, pos)
        pxs.dispatcher["configure"] = lambda xs, serial: xs.ack_configure(serial)
        popup.grab(seat, s)
        psurf.commit()
        rt()
        psurf.attach(buffer(300, 200, 0xFFC03030), 0, 0)
        psurf.commit()
    rt()
    log(started=kind)
    time.sleep(0.3)
    log(exiting=True)
    out.flush()
    os._exit(0)


def secctx():
    from pywayland.protocol.security_context_v1 import WpSecurityContextManagerV1

    path = ARGS[0]
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass
    lsock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    lsock.bind(path)
    lsock.listen(8)
    r, w = os.pipe()
    if WpSecurityContextManagerV1.name not in G:
        log(no_manager=True)
        return
    mgr = bind(WpSecurityContextManagerV1, 1)
    ctx = mgr.create_listener(lsock.fileno(), r)
    ctx.set_sandbox_engine("org.newm.compat")
    ctx.set_app_id("probe")
    ctx.set_instance_id("1")
    ctx.commit()
    log(listening=rt(), path=path)
    while True:
        if not rt():
            break
        time.sleep(1)


{"noack": noack, "huge": huge, "flap": flap, "grab": grab, "secctx": secctx}[MODE]()
