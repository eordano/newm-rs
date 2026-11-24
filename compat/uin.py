"""Real input devices for the newm VM tests, through uinput: libinput sees them as it sees
hardware (a USB keyboard, an absolute mouse with a wheel like QEMU's tablet, a five-slot
multitouch clickpad), so the compositor's own device code, libinput's gesture recogniser and the
compositor's bindings all run.

    uin serve                  creates the devices, then runs scripts sent to /run/uin.sock
    uin 'SCRIPT'               sends one script and waits until it ran (exit 1 on an error)

Run as uinx, it serves /run/uin-extra.sock with more devices for the protocol cases instead: a
screen-mapped pen tablet, a touchscreen and a second clickpad for two-finger pinches.

A script is commands separated by ';':
    key NAME down|up           a key by its evdev name without KEY_ (leftmeta, tab, a, enter)
    tap NAME...                each key down then up
    type TEXT                  letters, digits and a few symbols (no spaces)
    sleep MS
    abs X Y W H                the absolute mouse to (X, Y) of a W x H layout
    button left|right|middle down|up
    click                      left down, 60 ms, up
    wheel N                    N notches, positive scrolls down (towards the user)
    rel DX DY [STEP [MS]]      the relative mouse moves DX, DY in STEP-unit events (default 10)
                               MS ms apart (default 8); 60 4 is a flick libinput accelerates
    rbutton left|right|middle down|up   a button of the relative mouse
    swipe F DX DY [MS]         F fingers move DX, DY millimetres over MS (default 300) and lift
    hold F DX DY MS            F fingers move DX, DY millimetres, then stay MS before lifting
uinx commands:
    pen X Y W H [PRESSURE]     the pen touches (X, Y) of a W x H screen (pressure 0..1, default 0.6)
    penmove X Y W H [PRESSURE] the touching pen moves there in 10 steps
    penup                      the pen lifts but stays in proximity
    penaway                    the pen leaves proximity
    touch X Y W H              one finger touches the touchscreen at (X, Y) of W x H
    touchmove X Y W H          the finger moves there in 10 steps
    touchup                    the finger lifts
    pinch MM [MS]              two fingers on the clickpad spread MM millimetres apart over MS
    sleep MS
"""

import os
import socket
import sys
import time

EXTRA = os.path.basename(sys.argv[0]) == "uinx"
SOCK = "/run/uin-extra.sock" if EXTRA else "/run/uin.sock"
TP_W, TP_H, RES = 4000, 2600, 40  # 100 x 65 mm
ABS_MAX = 32767


def devices():
    from evdev import AbsInfo, UInput
    from evdev import ecodes as e

    kb = UInput(
        {e.EV_KEY: list(range(e.KEY_ESC, e.KEY_MICMUTE + 1))},
        name="uin keyboard",
        bustype=e.BUS_USB,
        vendor=0x1D6B,
        product=0x0104,
    )
    ptr = UInput(
        {
            e.EV_KEY: [e.BTN_LEFT, e.BTN_RIGHT, e.BTN_MIDDLE],
            e.EV_REL: [e.REL_WHEEL, e.REL_HWHEEL, e.REL_WHEEL_HI_RES, e.REL_HWHEEL_HI_RES],
            e.EV_ABS: [
                (e.ABS_X, AbsInfo(0, 0, ABS_MAX, 0, 0, 0)),
                (e.ABS_Y, AbsInfo(0, 0, ABS_MAX, 0, 0, 0)),
            ],
        },
        name="uin mouse",
        bustype=e.BUS_USB,
        vendor=0x0627,
        product=0x0001,
    )
    tp = UInput(
        {
            e.EV_KEY: [
                e.BTN_LEFT,
                e.BTN_TOUCH,
                e.BTN_TOOL_FINGER,
                e.BTN_TOOL_DOUBLETAP,
                e.BTN_TOOL_TRIPLETAP,
                e.BTN_TOOL_QUADTAP,
                e.BTN_TOOL_QUINTTAP,
            ],
            e.EV_ABS: [
                (e.ABS_X, AbsInfo(0, 0, TP_W, 0, 0, RES)),
                (e.ABS_Y, AbsInfo(0, 0, TP_H, 0, 0, RES)),
                (e.ABS_MT_SLOT, AbsInfo(0, 0, 4, 0, 0, 0)),
                (e.ABS_MT_TRACKING_ID, AbsInfo(0, 0, 65535, 0, 0, 0)),
                (e.ABS_MT_POSITION_X, AbsInfo(0, 0, TP_W, 0, 0, RES)),
                (e.ABS_MT_POSITION_Y, AbsInfo(0, 0, TP_H, 0, 0, RES)),
            ],
        },
        name="uin touchpad",
        bustype=e.BUS_USB,
        vendor=0x05AC,
        product=0x0265,
        input_props=[e.INPUT_PROP_POINTER, e.INPUT_PROP_BUTTONPAD],
    )
    rel = UInput(
        {
            e.EV_KEY: [e.BTN_LEFT, e.BTN_RIGHT, e.BTN_MIDDLE],
            e.EV_REL: [e.REL_X, e.REL_Y, e.REL_WHEEL, e.REL_WHEEL_HI_RES],
        },
        name="uin relative mouse",
        bustype=e.BUS_USB,
        vendor=0x046D,
        product=0xC077,
    )
    return e, kb, ptr, tp, rel


class Runner:
    def __init__(self):
        self.e, self.kb, self.ptr, self.tp, self.rel = devices()
        self.tid = 100

    def key(self, name, down):
        code = getattr(self.e, "KEY_" + name.upper())
        self.kb.write(self.e.EV_KEY, code, 1 if down else 0)
        self.kb.syn()

    def type(self, text):
        names = {".": "dot", "-": "minus", "/": "slash", ",": "comma", "=": "equal", " ": "space"}
        for ch in text:
            name = names.get(ch, ch.lower())
            shift = ch.isupper()
            if shift:
                self.key("leftshift", True)
            self.key(name, True)
            time.sleep(0.02)
            self.key(name, False)
            if shift:
                self.key("leftshift", False)
            time.sleep(0.02)

    def abs(self, x, y, w, h):
        e = self.e
        self.ptr.write(e.EV_ABS, e.ABS_X, round(float(x) * ABS_MAX / float(w)))
        self.ptr.write(e.EV_ABS, e.ABS_Y, round(float(y) * ABS_MAX / float(h)))
        self.ptr.syn()

    def button(self, which, down, dev=None):
        dev = dev or self.ptr
        code = {"left": self.e.BTN_LEFT, "right": self.e.BTN_RIGHT, "middle": self.e.BTN_MIDDLE}
        dev.write(self.e.EV_KEY, code[which], 1 if down else 0)
        dev.syn()

    def move(self, dx, dy, step=10, ms=8):
        e = self.e
        dx, dy, step = int(dx), int(dy), int(step)
        n = max(1, (max(abs(dx), abs(dy)) + step - 1) // step)
        done_x = done_y = 0
        for i in range(1, n + 1):
            x, y = dx * i // n, dy * i // n
            self.rel.write(e.EV_REL, e.REL_X, x - done_x)
            self.rel.write(e.EV_REL, e.REL_Y, y - done_y)
            self.rel.syn()
            done_x, done_y = x, y
            time.sleep(float(ms) / 1000)

    def wheel(self, n):
        e = self.e
        step = -1 if n > 0 else 1
        for _ in range(abs(n)):
            self.ptr.write(e.EV_REL, e.REL_WHEEL, step)
            self.ptr.write(e.EV_REL, e.REL_WHEEL_HI_RES, 120 * step)
            self.ptr.syn()
            time.sleep(0.05)

    def _touches(self, pts, first):
        e = self.e
        for slot, (x, y) in enumerate(pts):
            self.tp.write(e.EV_ABS, e.ABS_MT_SLOT, slot)
            if first:
                self.tid += 1
                self.tp.write(e.EV_ABS, e.ABS_MT_TRACKING_ID, self.tid)
            self.tp.write(e.EV_ABS, e.ABS_MT_POSITION_X, round(x * RES))
            self.tp.write(e.EV_ABS, e.ABS_MT_POSITION_Y, round(y * RES))
        if first:
            tool = [None, e.BTN_TOOL_FINGER, e.BTN_TOOL_DOUBLETAP, e.BTN_TOOL_TRIPLETAP,
                    e.BTN_TOOL_QUADTAP, e.BTN_TOOL_QUINTTAP][len(pts)]
            self.tp.write(e.EV_KEY, e.BTN_TOUCH, 1)
            self.tp.write(e.EV_KEY, tool, 1)
        self.tp.write(e.EV_ABS, e.ABS_X, round(pts[0][0] * RES))
        self.tp.write(e.EV_ABS, e.ABS_Y, round(pts[0][1] * RES))
        self.tp.syn()

    def _lift(self, n):
        e = self.e
        for slot in range(n):
            self.tp.write(e.EV_ABS, e.ABS_MT_SLOT, slot)
            self.tp.write(e.EV_ABS, e.ABS_MT_TRACKING_ID, -1)
        tool = [None, e.BTN_TOOL_FINGER, e.BTN_TOOL_DOUBLETAP, e.BTN_TOOL_TRIPLETAP,
                e.BTN_TOOL_QUADTAP, e.BTN_TOOL_QUINTTAP][n]
        self.tp.write(e.EV_KEY, e.BTN_TOUCH, 0)
        self.tp.write(e.EV_KEY, tool, 0)
        self.tp.syn()

    def swipe(self, fingers, dx, dy, ms=300, hold_ms=0):
        """Fingers 12 mm apart in a row across the middle; moves in 10 ms frames, so libinput
        sees a steady swipe (no jumps, no thumb in the bottom area)."""
        n = int(fingers)
        dx, dy = float(dx), float(dy)
        x0 = 50.0 - 6.0 * (n - 1) - dx / 2
        y0 = 30.0 - dy / 2
        pts = [(x0 + 12.0 * i, y0) for i in range(n)]
        self._touches(pts, True)
        time.sleep(0.03)
        frames = max(2, int(float(ms) / 10))
        for f in range(1, frames + 1):
            t = f / frames
            self._touches([(x + dx * t, y + dy * t) for x, y in pts], False)
            time.sleep(0.01)
        if hold_ms:
            time.sleep(float(hold_ms) / 1000)
        self._lift(n)
        time.sleep(0.05)

    def run(self, script):
        for cmd in filter(None, (c.strip() for c in script.split(";"))):
            w = cmd.split()
            op, args = w[0], w[1:]
            if op == "key":
                self.key(args[0], args[1] == "down")
            elif op == "tap":
                for k in args:
                    self.key(k, True)
                    time.sleep(0.03)
                    self.key(k, False)
                    time.sleep(0.03)
            elif op == "type":
                self.type(" ".join(args))
            elif op == "sleep":
                time.sleep(float(args[0]) / 1000)
            elif op == "abs":
                self.abs(*args)
            elif op == "button":
                self.button(args[0], args[1] == "down")
            elif op == "click":
                self.button("left", True)
                time.sleep(0.06)
                self.button("left", False)
            elif op == "rel":
                self.move(*args)
            elif op == "rbutton":
                self.button(args[0], args[1] == "down", self.rel)
            elif op == "wheel":
                self.wheel(int(args[0]))
            elif op == "swipe":
                self.swipe(*args)
            elif op == "hold":
                self.swipe(args[0], args[1], args[2], 300, args[3])
            else:
                raise ValueError(f"unknown command {op!r}")


def extra_devices():
    from evdev import AbsInfo, UInput
    from evdev import ecodes as e

    pen = UInput(
        {
            e.EV_KEY: [e.BTN_TOOL_PEN, e.BTN_TOUCH, e.BTN_STYLUS],
            e.EV_ABS: [
                (e.ABS_X, AbsInfo(0, 0, ABS_MAX, 0, 0, 100)),
                (e.ABS_Y, AbsInfo(0, 0, ABS_MAX, 0, 0, 100)),
                (e.ABS_PRESSURE, AbsInfo(0, 0, 4095, 0, 0, 0)),
            ],
        },
        name="uinx pen tablet",
        bustype=e.BUS_USB,
        vendor=0x056A,
        product=0x0358,
        input_props=[e.INPUT_PROP_DIRECT],
    )
    ts = UInput(
        {
            e.EV_KEY: [e.BTN_TOUCH],
            e.EV_ABS: [
                (e.ABS_X, AbsInfo(0, 0, ABS_MAX, 0, 0, 100)),
                (e.ABS_Y, AbsInfo(0, 0, ABS_MAX, 0, 0, 100)),
                (e.ABS_MT_SLOT, AbsInfo(0, 0, 9, 0, 0, 0)),
                (e.ABS_MT_TRACKING_ID, AbsInfo(0, 0, 65535, 0, 0, 0)),
                (e.ABS_MT_POSITION_X, AbsInfo(0, 0, ABS_MAX, 0, 0, 100)),
                (e.ABS_MT_POSITION_Y, AbsInfo(0, 0, ABS_MAX, 0, 0, 100)),
            ],
        },
        name="uinx touchscreen",
        bustype=e.BUS_USB,
        vendor=0x0EEF,
        product=0x0001,
        input_props=[e.INPUT_PROP_DIRECT],
    )
    tp = UInput(
        {
            e.EV_KEY: [e.BTN_LEFT, e.BTN_TOUCH, e.BTN_TOOL_FINGER, e.BTN_TOOL_DOUBLETAP],
            e.EV_ABS: [
                (e.ABS_X, AbsInfo(0, 0, TP_W, 0, 0, RES)),
                (e.ABS_Y, AbsInfo(0, 0, TP_H, 0, 0, RES)),
                (e.ABS_MT_SLOT, AbsInfo(0, 0, 4, 0, 0, 0)),
                (e.ABS_MT_TRACKING_ID, AbsInfo(0, 0, 65535, 0, 0, 0)),
                (e.ABS_MT_POSITION_X, AbsInfo(0, 0, TP_W, 0, 0, RES)),
                (e.ABS_MT_POSITION_Y, AbsInfo(0, 0, TP_H, 0, 0, RES)),
            ],
        },
        name="uinx clickpad",
        bustype=e.BUS_USB,
        vendor=0x05AC,
        product=0x0266,
        input_props=[e.INPUT_PROP_POINTER, e.INPUT_PROP_BUTTONPAD],
    )
    return e, pen, ts, tp


class ExtraRunner:
    def __init__(self):
        self.e, self.pen, self.ts, self.tp = extra_devices()
        self.pen_at = (0, 0)
        self.touch_at = (0, 0)
        self.tid = 1

    def _scr(self, x, y, w, h):
        return int(float(x) / float(w) * ABS_MAX), int(float(y) / float(h) * ABS_MAX)

    def _pen(self, x, y, p, touch):
        e = self.e
        self.pen.write(e.EV_KEY, e.BTN_TOOL_PEN, 1)
        self.pen.write(e.EV_ABS, e.ABS_X, x)
        self.pen.write(e.EV_ABS, e.ABS_Y, y)
        self.pen.write(e.EV_ABS, e.ABS_PRESSURE, int(p * 4095) if touch else 0)
        self.pen.write(e.EV_KEY, e.BTN_TOUCH, 1 if touch else 0)
        self.pen.syn()
        self.pen_at = (x, y)

    def pen_down(self, x, y, w, h, p=0.6):
        sx, sy = self._scr(x, y, w, h)
        self._pen(sx, sy, 0, False)
        time.sleep(0.05)
        self._pen(sx, sy, float(p), True)

    def pen_move(self, x, y, w, h, p=0.6):
        tx, ty = self._scr(x, y, w, h)
        fx, fy = self.pen_at
        for i in range(1, 11):
            self._pen(fx + (tx - fx) * i // 10, fy + (ty - fy) * i // 10, float(p), True)
            time.sleep(0.016)

    def pen_up(self):
        self._pen(*self.pen_at, 0, False)

    def pen_away(self):
        e = self.e
        self.pen.write(e.EV_KEY, e.BTN_TOUCH, 0)
        self.pen.write(e.EV_KEY, e.BTN_TOOL_PEN, 0)
        self.pen.syn()

    def _touch(self, x, y, first):
        e = self.e
        self.ts.write(e.EV_ABS, e.ABS_MT_SLOT, 0)
        if first:
            self.ts.write(e.EV_ABS, e.ABS_MT_TRACKING_ID, self.tid)
            self.tid += 1
        self.ts.write(e.EV_ABS, e.ABS_MT_POSITION_X, x)
        self.ts.write(e.EV_ABS, e.ABS_MT_POSITION_Y, y)
        if first:
            self.ts.write(e.EV_KEY, e.BTN_TOUCH, 1)
        self.ts.write(e.EV_ABS, e.ABS_X, x)
        self.ts.write(e.EV_ABS, e.ABS_Y, y)
        self.ts.syn()
        self.touch_at = (x, y)

    def touch_down(self, x, y, w, h):
        self._touch(*self._scr(x, y, w, h), True)

    def touch_move(self, x, y, w, h):
        tx, ty = self._scr(x, y, w, h)
        fx, fy = self.touch_at
        for i in range(1, 11):
            self._touch(fx + (tx - fx) * i // 10, fy + (ty - fy) * i // 10, False)
            time.sleep(0.016)

    def touch_up(self):
        e = self.e
        self.ts.write(e.EV_ABS, e.ABS_MT_SLOT, 0)
        self.ts.write(e.EV_ABS, e.ABS_MT_TRACKING_ID, -1)
        self.ts.write(e.EV_KEY, e.BTN_TOUCH, 0)
        self.ts.syn()

    def pinch(self, mm, ms=400):
        e = self.e
        mm, ms = float(mm), float(ms)
        cx, cy = TP_W // 2, TP_H // 2
        start = 8 * RES
        steps = max(int(ms / 10), 5)

        def put(gap, first):
            for slot, sign in ((0, -1), (1, 1)):
                self.tp.write(e.EV_ABS, e.ABS_MT_SLOT, slot)
                if first:
                    self.tp.write(e.EV_ABS, e.ABS_MT_TRACKING_ID, 100 + slot + self.tid)
                self.tp.write(e.EV_ABS, e.ABS_MT_POSITION_X, int(cx + sign * gap / 2))
                self.tp.write(e.EV_ABS, e.ABS_MT_POSITION_Y, cy)
            if first:
                self.tp.write(e.EV_KEY, e.BTN_TOUCH, 1)
                self.tp.write(e.EV_KEY, e.BTN_TOOL_DOUBLETAP, 1)
            self.tp.write(e.EV_ABS, e.ABS_X, int(cx - gap / 2))
            self.tp.write(e.EV_ABS, e.ABS_Y, cy)
            self.tp.syn()

        put(start, True)
        for i in range(1, steps + 1):
            time.sleep(ms / steps / 1000)
            put(start + mm * RES * i / steps, False)
        time.sleep(0.05)
        for slot in (0, 1):
            self.tp.write(e.EV_ABS, e.ABS_MT_SLOT, slot)
            self.tp.write(e.EV_ABS, e.ABS_MT_TRACKING_ID, -1)
        self.tp.write(e.EV_KEY, e.BTN_TOUCH, 0)
        self.tp.write(e.EV_KEY, e.BTN_TOOL_DOUBLETAP, 0)
        self.tp.syn()
        self.tid += 2

    def run(self, script):
        for cmd in filter(None, (c.strip() for c in script.split(";"))):
            w = cmd.split()
            op, args = w[0], w[1:]
            if op == "pen":
                self.pen_down(*args)
            elif op == "penmove":
                self.pen_move(*args)
            elif op == "penup":
                self.pen_up()
            elif op == "penaway":
                self.pen_away()
            elif op == "touch":
                self.touch_down(*args)
            elif op == "touchmove":
                self.touch_move(*args)
            elif op == "touchup":
                self.touch_up()
            elif op == "pinch":
                self.pinch(*args)
            elif op == "sleep":
                time.sleep(float(args[0]) / 1000)
            else:
                raise ValueError(f"unknown command {op!r}")


def serve():
    runner = ExtraRunner() if EXTRA else Runner()
    try:
        os.unlink(SOCK)
    except FileNotFoundError:
        pass
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(SOCK)
    os.chmod(SOCK, 0o666)
    srv.listen(4)
    print("uin: devices ready", flush=True)
    while True:
        conn, _ = srv.accept()
        with conn:
            script = conn.makefile().readline()
            try:
                runner.run(script)
                conn.sendall(b"ok\n")
            except Exception as err:  # the client reports it
                conn.sendall(f"error {err!r}\n".encode())


def send(script):
    c = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    c.connect(SOCK)
    c.sendall(script.replace("\n", " ").encode() + b"\n")
    reply = c.makefile().readline().strip()
    if reply != "ok":
        print(f"uin: {reply}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    if sys.argv[1:] == ["serve"]:
        serve()
    else:
        send(" ".join(sys.argv[1:]))
