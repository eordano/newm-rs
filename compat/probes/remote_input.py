"""Drives the RemoteDesktop portal the way KDE Connect's mousepad plugin does
(plugins/mousepad/waylandremoteinput.cpp, kdeconnect-kde 26.08): CreateSession, SelectDevices
(keyboard + pointer, persist_mode 2), Start, ConnectToEIS (which fails outside KWin), then the
Notify calls KDE Connect falls back to. Prints one line per step; exits non-zero on a failure."""

import asyncio
import random
import sys

from dbus_next import Message, Variant
from dbus_next.aio import MessageBus
from dbus_next.errors import DBusError

PORTAL = "org.freedesktop.portal.Desktop"
PATH = "/org/freedesktop/portal/desktop"
IFACE = "org.freedesktop.portal.RemoteDesktop"
BTN_LEFT = 0x110


async def call(bus, member, signature, body):
    reply = await bus.call(Message(destination=PORTAL, path=PATH, interface=IFACE,
                                   member=member, signature=signature, body=body))
    if reply.message_type.name == "ERROR":
        raise DBusError(reply.error_name, (reply.body or [""])[0])
    return reply.body


async def request(bus, member, signature, body):
    """A portal call answered through a Request object's Response signal."""
    token = f"kdeconnect{random.randrange(1 << 30)}"
    sender = bus.unique_name[1:].replace(".", "_")
    path = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"
    done = asyncio.get_running_loop().create_future()

    def on_message(msg):
        if msg.path == path and msg.member == "Response" and not done.done():
            done.set_result(msg.body)

    bus.add_message_handler(on_message)
    body[-1]["handle_token"] = Variant("s", token)
    await call(bus, member, signature, body)
    code, results = await asyncio.wait_for(done, 30)
    bus.remove_message_handler(on_message)
    print(f"{member}: response {code}", flush=True)
    if code != 0:
        sys.exit(f"{member} answered {code}")
    return results


async def main():
    bus = await MessageBus().connect()
    await bus.call(Message(destination="org.freedesktop.DBus", path="/org/freedesktop/DBus",
                           interface="org.freedesktop.DBus", member="AddMatch", signature="s",
                           body=["type='signal',interface='org.freedesktop.portal.Request'"]))
    token = f"kdeconnect{random.randrange(1 << 30)}"
    res = await request(bus, "CreateSession", "a{sv}",
                        [{"session_handle_token": Variant("s", token)}])
    session = res["session_handle"].value
    await request(bus, "SelectDevices", "oa{sv}",
                  [session, {"types": Variant("u", 3), "persist_mode": Variant("u", 2)}])
    await request(bus, "Start", "osa{sv}", [session, "", {}])
    try:
        await call(bus, "ConnectToEIS", "oa{sv}", [session, {}])
        print("ConnectToEIS: answered (unexpected outside KWin)", flush=True)
    except DBusError as err:
        print(f"ConnectToEIS: failed as outside KWin ({err.text})", flush=True)

    steps = sys.argv[1:] or ["motion 40 30", "click", "keysym 0x68", "keysym 0x49"]
    for step in steps:
        word, *rest = step.split()
        if word == "motion":
            await call(bus, "NotifyPointerMotion", "oa{sv}dd",
                       [session, {}, float(rest[0]), float(rest[1])])
        elif word == "click":
            for state in (1, 0):
                await call(bus, "NotifyPointerButton", "oa{sv}iu", [session, {}, BTN_LEFT, state])
        elif word == "keysym":
            for state in (1, 0):
                await call(bus, "NotifyKeyboardKeysym", "oa{sv}iu",
                           [session, {}, int(rest[0], 0), state])
        elif word == "sleep":
            await asyncio.sleep(float(rest[0]))
        print(f"sent: {step}", flush=True)
        await asyncio.sleep(0.2)
    await asyncio.sleep(1)
    print("done", flush=True)


asyncio.run(main())
