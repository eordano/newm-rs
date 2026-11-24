"""A GTK 4 parent window and, a second later, a modal dialog on it (Gtk.Window with
transient_for and modal: xdg_toplevel.set_parent plus xdg_dialog_v1.set_modal on Wayland).
Titles: TAG-parent, TAG-dialog. Usage: dialog.py APP_ID TAG"""

import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk  # noqa: E402

APP_ID, TAG = sys.argv[1], sys.argv[2]
GLib.set_prgname(APP_ID)

Gtk.init()
parent = Gtk.Window(title=f"{TAG}-parent")
parent.set_default_size(900, 600)
parent.set_child(Gtk.Label(label="parent"))
parent.present()


def open_dialog():
    dialog = Gtk.Window(title=f"{TAG}-dialog", transient_for=parent, modal=True)
    dialog.set_default_size(320, 160)
    dialog.set_child(Gtk.Label(label="dialog"))
    dialog.present()
    return False


GLib.timeout_add(1000, open_dialog)
GLib.MainLoop().run()
