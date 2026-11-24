"""SDL relative-mouse probe (pointer constraints + relative pointer on Wayland): a window that
turns relative mouse mode on once focused and logs each motion's xrel/yrel and position to OUT.

    sdl.py OUT SECONDS
"""

import ctypes
import json
import os
import sys
import time

os.environ.setdefault("SDL_VIDEODRIVER", "wayland")
import sdl2  # noqa: E402

OUT, SECS = sys.argv[1], float(sys.argv[2])
out = open(OUT, "a", buffering=1)
T0 = time.monotonic()


def log(**d):
    d["at"] = round(time.monotonic() - T0, 3)
    out.write(json.dumps(d) + "\n")


sdl2.SDL_Init(sdl2.SDL_INIT_VIDEO)
log(driver=sdl2.SDL_GetCurrentVideoDriver().decode())
win = sdl2.SDL_CreateWindow(b"probe-sdl", sdl2.SDL_WINDOWPOS_CENTERED, sdl2.SDL_WINDOWPOS_CENTERED,
                            640, 400, sdl2.SDL_WINDOW_SHOWN)
ren = sdl2.SDL_CreateRenderer(win, -1, sdl2.SDL_RENDERER_SOFTWARE)
ev = sdl2.SDL_Event()
rel_on = False
sum_x = sum_y = 0
end = time.time() + SECS
while time.time() < end:
    sdl2.SDL_SetRenderDrawColor(ren, 0x30, 0x30, 0xC0, 255)
    sdl2.SDL_RenderClear(ren)
    sdl2.SDL_RenderPresent(ren)
    while sdl2.SDL_PollEvent(ctypes.byref(ev)):
        if ev.type == sdl2.SDL_WINDOWEVENT and ev.window.event == sdl2.SDL_WINDOWEVENT_FOCUS_GAINED:
            log(focus=True)
            if not rel_on:
                rc = sdl2.SDL_SetRelativeMouseMode(sdl2.SDL_TRUE)
                rel_on = rc == 0
                log(relative=rel_on, rc=rc)
        elif ev.type == sdl2.SDL_MOUSEMOTION:
            sum_x += ev.motion.xrel
            sum_y += ev.motion.yrel
            log(x=ev.motion.x, y=ev.motion.y, xrel=ev.motion.xrel, yrel=ev.motion.yrel,
                sum_x=sum_x, sum_y=sum_y)
        elif ev.type == sdl2.SDL_QUIT:
            end = 0
    time.sleep(0.01)
log(done=True, sum_x=sum_x, sum_y=sum_y)
