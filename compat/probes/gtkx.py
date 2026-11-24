"""GTK probes of the protocol cases. Each appends JSON lines to OUT (one per event it saw).

    gtkx.py ROLE APP_ID OUT [key=value ...]

Roles:
    events     GTK 4 window in `color` logging every input event it gets (type, device source,
               position, button, key and modifiers, pressure, pinch scale, scroll deltas);
               inhibit=1 asks for keyboard-shortcuts-inhibit once focused; cursor=NAME sets a
               named cursor (cursor-shape)
    dnd-src    a drag source offering `text` (gtk=3 or 4; GTK 3 also runs under X11)
    dnd-dst    a drop target logging the text dropped on it (gtk=3 or 4)
    launch     GTK 4 window; Enter launches `child` through Gio.AppInfo with an activation token
    plain      GTK 4 window logging its focus; late=N presents it again after N seconds
    layer      GTK 4 layer-shell bar at the top (zone=exclusive zone, kb=none|exclusive|on_demand)
               with an entry and a popover button at its left end
    fc         Enter opens the portal file chooser (gtk=4: GtkFileDialog, gtk=3: FileChooserNative)
    inhibit    a GtkApplication (org.newm.ProbeInhibit) whose window holds an idle inhibit
               (gtk_application_inhibit: GNOME's session manager or the Inhibit portal)
Every GTK 4 role logs {"focus": bool} on each change of its toplevel's focus.
"""

import json
import os
import sys
import time

APP_ID, ROLE, OUT = sys.argv[2], sys.argv[1], sys.argv[3]
OPT = dict(a.split("=", 1) for a in sys.argv[4:])
VER = "3.0" if OPT.get("gtk") == "3" else "4.0"

import gi  # noqa: E402

gi.require_version("Gtk", VER)
gi.require_version("Gdk", VER)
if ROLE == "layer":
    gi.require_version("Gtk4LayerShell", "1.0")
    from gi.repository import Gtk4LayerShell as LS  # noqa: E402
from gi.repository import Gdk, Gio, GLib, GObject, Gtk  # noqa: E402

GLib.set_prgname(APP_ID)
out = open(OUT, "a", buffering=1)
T0 = time.monotonic()


def log(**d):
    d["at"] = round(time.monotonic() - T0, 3)
    out.write(json.dumps(d) + "\n")


COLOR = OPT.get("color", "#30c030")
CSS = f"window, .background {{ background: {COLOR}; }} entry {{ margin: 8px; }}"
if VER == "4.0":
    Gtk.init()
    _css = Gtk.CssProvider()
    _css.load_from_string(CSS)
    Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), _css, 800)
else:
    Gtk.init(sys.argv[:1])
    _css = Gtk.CssProvider()
    _css.load_from_data(CSS.encode())
    Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(), _css, 800)
    log(backend=type(Gdk.Display.get_default()).__name__)


def watch_focus(win, on_focus=None):
    state = {"focused": None, "inhibited": False}

    def changed(surf, _pspec):
        f = bool(surf.get_state() & Gdk.ToplevelState.FOCUSED)
        if f != state["focused"]:
            state["focused"] = f
            log(focus=f)
            if f and on_focus:
                on_focus(surf)

    def realized(w):
        surf = w.get_surface()
        surf.connect("notify::state", changed)
        if OPT.get("inhibit") == "1":
            surf.connect("notify::shortcuts-inhibited",
                         lambda s, _p: log(shortcuts_inhibited=s.get_property("shortcuts-inhibited")))

    win.connect("realize", realized)


def window(title=None, size=(700, 420)):
    if VER == "4.0":
        w = Gtk.Window(title=title or APP_ID)
        w.set_default_size(*size)
        w.connect("close-request", lambda *_: (log(closed=True), GLib.idle_add(lambda: os._exit(0))) and False)
    else:
        w = Gtk.Window(title=title or APP_ID)
        w.set_default_size(*size)
        w.connect("destroy", Gtk.main_quit)
    return w


def run(win):
    if VER == "4.0":
        win.present()
        GLib.MainLoop().run()
    else:
        win.show_all()
        Gtk.main()


def events():
    win = window()
    area = Gtk.DrawingArea(hexpand=True, vexpand=True, focusable=True, can_focus=True)
    win.set_child(area)
    if "cursor" in OPT:
        area.set_cursor(Gdk.Cursor.new_from_name(OPT["cursor"], None))
    names = {getattr(Gdk.EventType, n): n.lower() for n in dir(Gdk.EventType) if n.isupper()}

    def on_event(_ctrl, ev):
        t = ev.get_event_type()
        d = {"t": names.get(t, str(t))}
        try:
            dev = ev.get_device()
            if dev is not None:
                d["src"] = dev.get_source().value_nick
            tool = ev.get_device_tool()
            if tool is not None:
                d["tool"] = tool.get_tool_type().value_nick
        except Exception:  # noqa: BLE001
            pass
        try:
            ok, x, y = ev.get_position()
            if ok:
                d["x"], d["y"] = round(x, 1), round(y, 1)
        except Exception:  # noqa: BLE001
            pass
        try:
            if t in (Gdk.EventType.BUTTON_PRESS, Gdk.EventType.BUTTON_RELEASE):
                d["button"] = ev.get_button()
            elif t in (Gdk.EventType.KEY_PRESS, Gdk.EventType.KEY_RELEASE):
                d["key"] = Gdk.keyval_name(ev.get_keyval())
                d["mods"] = int(ev.get_modifier_state())
            elif t == Gdk.EventType.SCROLL:
                d["dx"], d["dy"] = ev.get_deltas()
            elif t == Gdk.EventType.TOUCHPAD_PINCH:
                d["phase"] = ev.get_gesture_phase().value_nick
                d["scale"] = round(ev.get_pinch_scale(), 3)
                d["fingers"] = ev.get_touchpad_gesture_n_fingers()
            elif t in (Gdk.EventType.TOUCHPAD_SWIPE, Gdk.EventType.TOUCHPAD_HOLD):
                d["phase"] = ev.get_gesture_phase().value_nick
                d["fingers"] = ev.get_touchpad_gesture_n_fingers()
            ok, p = ev.get_axis(Gdk.AxisUse.PRESSURE)
            if ok:
                d["pressure"] = round(p, 3)
        except Exception as e:  # noqa: BLE001
            d["err"] = str(e)
        log(**d)
        return False

    legacy = Gtk.EventControllerLegacy()
    legacy.connect("event", on_event)
    area.add_controller(legacy)

    def focused(surf):
        area.grab_focus()
        if OPT.get("inhibit") == "1":
            surf.inhibit_system_shortcuts(None)
            log(inhibit_asked=True)

    watch_focus(win, focused)
    run(win)


def dnd_src():
    text = OPT.get("text", f"dnd-{APP_ID}")
    win = window(size=(500, 400))
    if VER == "4.0":
        area = Gtk.DrawingArea(hexpand=True, vexpand=True)
        src = Gtk.DragSource(actions=Gdk.DragAction.COPY)
        src.connect("prepare", lambda *_: (log(drag="prepare"), Gdk.ContentProvider.new_for_value(text))[1])
        src.connect("drag-end", lambda _s, _d, deleted: log(drag="end"))
        src.connect("drag-cancel", lambda _s, _d, reason: (log(drag="cancel", reason=int(reason)), False)[1])
        area.add_controller(src)
        win.set_child(area)
        watch_focus(win)
    else:
        box = Gtk.EventBox()
        box.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], Gdk.DragAction.COPY)
        box.drag_source_add_text_targets()
        box.connect("drag-begin", lambda *_: log(drag="begin"))
        box.connect("drag-data-get", lambda _w, _c, data, _i, _t: (log(drag="get"), data.set_text(text, -1)))
        box.connect("drag-end", lambda *_: log(drag="end"))
        win.add(box)
    run(win)


def dnd_dst():
    win = window(size=(500, 400))
    if VER == "4.0":
        area = Gtk.DrawingArea(hexpand=True, vexpand=True)
        dst = Gtk.DropTarget.new(GObject.TYPE_STRING, Gdk.DragAction.COPY)
        dst.connect("enter", lambda *_: (log(drop="enter"), Gdk.DragAction.COPY)[1])
        dst.connect("drop", lambda _t, value, _x, _y: (log(dropped=value), True)[1])
        area.add_controller(dst)
        win.set_child(area)
        watch_focus(win)
    else:
        box = Gtk.EventBox()
        box.drag_dest_set(Gtk.DestDefaults.ALL, [], Gdk.DragAction.COPY)
        box.drag_dest_add_text_targets()
        box.connect("drag-motion", lambda *_: (log(drop="motion"), False)[1])
        box.connect("drag-data-received", lambda _w, _c, _x, _y, data, _i, _t: log(dropped=data.get_text()))
        win.add(box)
    run(win)


def launch():
    win = window()
    entry = Gtk.Entry()
    win.set_child(entry)

    def go():
        ctx = Gdk.Display.get_default().get_app_launch_context()
        info = Gio.AppInfo.create_from_commandline(
            OPT["child"], "probe-child", Gio.AppInfoCreateFlags.SUPPORTS_STARTUP_NOTIFICATION)
        ok = info.launch([], ctx)
        log(launched=ok)

    entry.connect("activate", lambda *_: go())
    watch_focus(win)
    run(win)


def plain():
    win = window()
    entry = Gtk.Entry()
    win.set_child(entry)
    entry.connect("activate", lambda e: log(text=e.get_text()))
    watch_focus(win)
    if "late" in OPT:
        def again():
            log(present_again=True)
            win.present()
            return False
        GLib.timeout_add(int(float(OPT["late"]) * 1000), again)
    run(win)


def layer():
    win = Gtk.Window(title=APP_ID)
    LS.init_for_window(win)
    LS.set_namespace(win, APP_ID)
    LS.set_layer(win, LS.Layer.TOP)
    for edge in (LS.Edge.TOP, LS.Edge.LEFT, LS.Edge.RIGHT):
        LS.set_anchor(win, edge, True)
    LS.set_exclusive_zone(win, int(OPT.get("zone", "60")))
    LS.set_keyboard_mode(win, {"none": LS.KeyboardMode.NONE, "exclusive": LS.KeyboardMode.EXCLUSIVE,
                               "on_demand": LS.KeyboardMode.ON_DEMAND}[OPT.get("kb", "none")])
    win.set_default_size(-1, 60)
    box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
    pop_label = Gtk.Label(label="popover")
    pop_label.set_size_request(300, 200)
    pop = Gtk.Popover(child=pop_label)
    _pc = Gtk.CssProvider()
    _pc.load_from_string("popover contents { background: #c03030; }")
    Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), _pc, 900)
    pop.connect("show", lambda *_: log(popover="shown"))
    pop.connect("closed", lambda *_: log(popover="closed"))
    btn = Gtk.MenuButton(popover=pop, label="menu", can_focus=False)
    btn.set_size_request(60, 50)
    entry = Gtk.Entry(hexpand=True)
    entry.connect("activate", lambda e: log(text=e.get_text()))
    focus_ctl = Gtk.EventControllerFocus()
    focus_ctl.connect("enter", lambda *_: log(entry_focus=True))
    focus_ctl.connect("leave", lambda *_: log(entry_focus=False))
    entry.add_controller(focus_ctl)
    box.append(btn)
    box.append(entry)
    win.set_child(box)
    win.set_focus(entry)
    keys = Gtk.EventControllerKey()
    keys.connect("key-pressed", lambda _c, kv, _code, _st: (log(key=Gdk.keyval_name(kv)), False)[1])
    win.add_controller(keys)
    win.connect("map", lambda *_: log(mapped=True))
    win.present()
    GLib.MainLoop().run()


def fc():
    win = window()
    entry = Gtk.Entry()
    if VER == "4.0":
        win.set_child(entry)

        def go():
            dlg = Gtk.FileDialog(title="probe-file-dialog")
            log(asked=True)

            def done(d, res):
                try:
                    f = d.open_finish(res)
                    log(chosen=f.get_path() if f else None)
                except GLib.Error as e:
                    log(dialog_error=e.message)

            dlg.open(win, None, done)
        watch_focus(win)
    else:
        win.add(entry)

        def go():
            dlg = Gtk.FileChooserNative.new("probe-file-dialog", win, Gtk.FileChooserAction.OPEN, None, None)
            dlg.connect("response", lambda d, r: log(response=int(r), chosen=d.get_filename()))
            log(asked=True)
            dlg.show()
            win._keep = dlg
    entry.connect("activate", lambda *_: go())
    run(win)


def inhibit():
    app = Gtk.Application(application_id="org.newm.ProbeInhibit")

    def activate(a):
        win = Gtk.ApplicationWindow(application=a, title=APP_ID)
        win.set_default_size(500, 300)
        watch_focus(win)
        win.present()

        def hold():
            cookie = a.inhibit(win, Gtk.ApplicationInhibitFlags.IDLE, "compat test")
            log(cookie=cookie)
            return False
        GLib.timeout_add(1000, hold)

    app.connect("activate", activate)
    app.run([])


{"inhibit": inhibit, "events": events, "dnd-src": dnd_src, "dnd-dst": dnd_dst, "launch": launch, "plain": plain,
 "layer": layer, "fc": fc}[ROLE]()
