# KDE and Plasma apps

newm-rs runs KDE apps as Plasma would, as far as a compositor can: their global shortcuts,
KRunner's window search, Spectacle, KDE Connect's remote input, idle inhibition and the KDE
portals. It does not run Plasma's shell. None of this needs the `canvas` feature; the D-Bus
services come with the default `dbus` feature. [SPEC §8](SPEC.md#8-kde-and-desktop-integration)
has the full reference.

## Session environment

Start the session from a session file with `DesktopNames=niri;KDE` (the shipped
`resources/newm-rs.desktop` says so). newm-rs keeps `XDG_CURRENT_DESKTOP=niri:KDE`
and exports it to the systemd user manager and D-Bus activation at start. xdg-desktop-portal reads
the first name and picks `niri-portals.conf`, while Chromium and Qt look for `KDE` anywhere in the
list to choose KWallet and the KDE platform theme.

Set `QT_QPA_PLATFORMTHEME=kde` (Breeze, KDE file dialogs, your KDE settings) in the login
environment: `newm-rs-session` imports the whole login environment into systemd and D-Bus, while the
config's `environment { }` block reaches only the programs newm-rs itself starts. newm-rs does not
set `KDE_FULL_SESSION`; set it there too if an app needs it.

## Portals

The shipped `resources/niri-portals.conf` is niri's (`default=gnome;gtk;`);
xdg-desktop-portal-gnome's screencast and screenshot portals work through niri's GNOME-compatible
D-Bus interfaces. To prefer KDE's portals, put your own `niri-portals.conf` in
`~/.config/xdg-desktop-portal/`. The compat suite tests this routing:

```ini
[preferred]
default=kde;gtk;
org.freedesktop.impl.portal.ScreenCast=gnome;
org.freedesktop.impl.portal.Screenshot=gnome;
org.freedesktop.impl.portal.RemoteDesktop=kde;
```

xdg-desktop-portal-kde creates its RemoteDesktop, ScreenCast, Screenshot, GlobalShortcuts and
other portals only when `XDG_CURRENT_DESKTOP` is exactly `KDE`; `niri:KDE` is not enough. Give its
user service a drop-in, for example
`~/.config/systemd/user/plasma-xdg-desktop-portal-kde.service.d/desktop.conf`:

```ini
[Service]
Environment=XDG_CURRENT_DESKTOP=KDE
```

Then `systemctl --user daemon-reload` and restart the portal. Only that service sees `KDE`.

## What newm-rs serves on D-Bus

Running as a session (`--session`), newm-rs serves KWin's screenshot interface (version 4, for
Spectacle; window captures refused while locked), KWin's window info, KRunner's window search
(a query of two or more letters, or `windows` for all), kglobalaccel for KDE apps' global shortcuts (config binds
always win; changes persist in `$XDG_STATE_HOME/newm-rs/kglobalshortcuts.json`), the freedesktop
ScreenSaver interface, and PowerDevil's inhibition interface for KDE's Inhibit portal. At start
it also reads `X-KDE-Shortcuts` launch keys from the desktop files in each `kglobalaccel` data
directory. [SPEC §8.3](SPEC.md#83-d-bus-services) gives each bus name, path and method.

## Wayland protocols for KDE apps

- `zkde_screencast_unstable_v1` (version 5): Spectacle and other KDE apps record an output, a
  window or a region through PipeWire.
- `org_kde_kwin_fake_input` (version 6): KDE Connect's remote input, through
  xdg-desktop-portal-kde's RemoteDesktop portal. Only that program sees the global; it needs the
  portal routing and the drop-in above.
- `org_kde_kwin_server_decoration_manager`: offered when the config sets `prefer-no-csd`, so KDE
  apps leave out their own title bars.

## What does not work

System Settings' Display page, `kscreen-doctor` and the display OSD (configure outputs in the KDL
or with `wlr-randr` or kanshi); Plasma's shell, panels and task manager (KRunner opens as an
ordinary window); launch feedback, the global menu, screen edges and other KWin-only protocols;
Qt 5 input methods (`text-input-v2`). [SPEC §8.5](SPEC.md#85-what-does-not-work) and
[compat/PROTOCOLS.md](../../compat/PROTOCOLS.md) list each gap.
