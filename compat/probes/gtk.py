"""A GTK 3 or 4 probe window: green, one entry. Enter writes what was typed, the window's scale
and size to OUT as JSON. Usage: gtk.py 3.0|4.0 APP_ID OUT"""

import json
import sys

import gi

VER, APP_ID, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
gi.require_version("Gtk", VER)
gi.require_version("Gdk", VER)
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

GLib.set_prgname(APP_ID)
CSS = "window, .background { background: #30c030; } entry { margin: 40px; }"


def report(win, entry):
    d = {"text": entry.get_text(), "scale_factor": win.get_scale_factor(), "gtk": VER}
    if VER == "4.0":
        surf = win.get_surface()
        d["scale"] = surf.get_scale() if hasattr(surf, "get_scale") else None
        d["size"] = [win.get_width(), win.get_height()]
    else:
        d["size"] = list(win.get_size())
    with open(OUT, "w") as f:
        json.dump(d, f)


provider = Gtk.CssProvider()
if VER == "4.0":
    Gtk.init()
    provider.load_from_string(CSS)
    Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider, 800)
    win = Gtk.Window(title=APP_ID)
    entry = Gtk.Entry()
    win.set_child(entry)
    entry.connect("activate", lambda e: report(win, e))
    win.present()
    GLib.MainLoop().run()
else:
    Gtk.init(sys.argv[:1])
    provider.load_from_data(CSS.encode())
    Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(), provider, 800)
    win = Gtk.Window(title=APP_ID)
    entry = Gtk.Entry()
    win.add(entry)
    entry.connect("activate", lambda e: report(win, e))
    win.connect("destroy", Gtk.main_quit)
    win.show_all()
    Gtk.main()
