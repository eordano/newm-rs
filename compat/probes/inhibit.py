"""A GTK 4 window that inhibits the compositor's shortcuts, as a VM viewer does
(Gdk.Toplevel.inhibit_system_shortcuts: keyboard-shortcuts-inhibit-unstable-v1 on Wayland).
It appends one JSON line per event to OUT: {"inhibited": bool} when the compositor's answer
changes, {"key": NAME, "super": bool} per key press. Usage: inhibit.py APP_ID OUT"""

import json
import sys

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

APP_ID, OUT = sys.argv[1], sys.argv[2]
GLib.set_prgname(APP_ID)


def log(d):
    with open(OUT, "a") as f:
        f.write(json.dumps(d) + "\n")


def on_key(_ctl, keyval, _code, state):
    log({"key": Gdk.keyval_name(keyval), "super": bool(state & Gdk.ModifierType.SUPER_MASK)})
    return True


def on_map(win):
    surf = win.get_surface()
    surf.connect("notify::shortcuts-inhibited",
                 lambda s, _p: log({"inhibited": s.get_property("shortcuts-inhibited")}))
    surf.inhibit_system_shortcuts(None)


Gtk.init()
win = Gtk.Window(title=APP_ID)
win.set_child(Gtk.Label(label="inhibit"))
ctl = Gtk.EventControllerKey()
ctl.connect("key-pressed", on_key)
win.add_controller(ctl)
win.connect("map", on_map)
win.present()
GLib.MainLoop().run()
