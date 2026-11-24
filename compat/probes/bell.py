"""A GTK 4 window that rings the bell for its surface after DELAY seconds
(gdk_surface_beep: xdg-system-bell-v1 on Wayland), then once more every 2 s, writing a line to OUT
per ring. Usage: bell.py APP_ID OUT DELAY"""

import sys

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import GLib, Gtk  # noqa: E402

APP_ID, OUT, DELAY = sys.argv[1], sys.argv[2], float(sys.argv[3])
GLib.set_prgname(APP_ID)


def ring(win):
    win.get_surface().beep()
    with open(OUT, "a") as f:
        f.write("ring\n")
    return True


Gtk.init()
win = Gtk.Window(title=APP_ID)
win.set_child(Gtk.Label(label="bell"))
win.present()


def first():
    ring(win)
    GLib.timeout_add(2000, ring, win)
    return False


GLib.timeout_add(int(DELAY * 1000), first)
GLib.MainLoop().run()
