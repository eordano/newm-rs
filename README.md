# newm-rs

A Wayland compositor with a strict 2D tile canvas: a fork of niri, inspired by newm and KDE.

## newm-rs in two and a half minutes

<p align="center">
  <a href="docs/video/out/newm-rs.mp4"><img width="960" alt="newm-rs: windows rearranging on a strict 2D tile canvas, replayed from a real session" src="docs/video/out/newm-rs-loop.webp"></a>
</p>

## Principles

1. **Tiled windows never overlap other tiled windows.**
2. **A window is never bigger than its display.**
3. **Zoom is a camera.**
4. **Changing focus moves the camera as little as possible.**
5. **Moving the mouse without a click changes nothing.**
6. **Everything the mouse can do, the keyboard can do too.**
7. **One hand is enough.**
8. **The left hand alone works too.**
9. **As fast and as stable as possible.**
10. **As compatible with KDE as possible.**
11. **Beautiful and joyful to use.**
12. **It respects your muscle memory.**

[docs/newm-rs/principles.md](docs/newm-rs/principles.md) says what each one means in practice.

## Features

### The canvas

- A 2D grid of fixed-size tiles. Every tiled window spans whole tiles, and apps are drawn 1:1 at
  their own size. Tiles at the screen's edges show in part.
- Placement rules: a move reassigns the fewest tiles, never splits an island of touching windows,
  and a window making room keeps its row or column when it can.
- New windows open at the nearest free spot, the one the camera scrolls least to show, at the
  size the app picks, rounded to whole tiles. newm-rs remembers each app's last size.
- Drag with a live preview: windows in the way step aside while you drag. Escape cancels.
- Resize with a preview: a box shows where the window lands and what it pushes.
- Grow and shrink by edges. Shrinking closes a gap between windows (a lake) and leaves open space
  (the ocean) alone.
- Swap with a neighbour, send to an edge, gather islands together.
- Undo and redo every rearrangement.
- Floating windows stay on screen, follow their parent window, or move with the canvas.

### The camera

- Zoom in and out about the middle of the screen; one action returns to 1:1. No window is
  resized by a zoom.
- Pinch the touchpad with three fingers, or with Mod held, to zoom in whole steps. A plain
  two-finger pinch stays with the app. Super+wheel zooms one step per notch with the
  [suggested binds](docs/newm-rs/input.md#suggested-binds).
- An overview of every window, with everything outside the screen dimmed.
- Zoom to fit while Mod is held during a four-finger swipe, back on release.
- Pan with Mod and the mouse, with acceleration and momentum you can tune, or with Mod and three
  fingers on the touchpad.
- A minimap in the bottom-left corner shows where the camera and the windows are whenever
  something moves.
- The view scrolls the least it must to show the focused window whole.
- Alt+Tab with live thumbnails, optionally centring the window it lands on.

### Your session

- Log out and back in: every window returns to its tiles.
- Terminal windows (Ghostty, Alacritty, kitty, foot, Konsole, WezTerm and others) reattach their
  tmux and ssh sessions; Chromium-family browsers relaunch with their last session, and Firefox
  restores its own.
- The lock screen stays locked even if the locker crashes. `newm-rs-lock`, the bundled locker,
  never exits while the session is locked.
- Screenshots and screen recording with Spectacle and the desktop portals.
- An opt-in usage log of what the canvas does, in builds with the `telemetry` feature. It never
  leaves your machine.

### KDE and every toolkit

- GTK 3, GTK 4, libadwaita, Qt and KDE apps, Firefox, Chromium and other browsers, Electron and
  X11 apps, crisp at fractional scale.
- KDE global shortcuts, KRunner's window search, KDE's Inhibit portal, KDE Connect's remote input.
- Input methods, clipboard managers, keyboard shortcuts for VM viewers and remote desktops, modal
  dialogs, the system bell, monitor power, explicit sync, and sandboxed apps kept away from
  privileged protocols.

### Stability

- A VM suite runs real apps against newm-rs.
- A Rocq model proves the canvas's rules: no overlap, minimum sizes kept, the zoom as a camera,
  the least camera motion.
- Any main-loop work of 8 ms or more names itself in the log.
- Disk writes (session saves, the usage log) run on their own threads and never block the screen.

## Status

newm-rs is daily-driven. It tracks upstream niri's `main` branch. The canvas and session restore
sit behind the `canvas` Cargo feature, and the usage log behind `telemetry`; built without
`canvas`, newm-rs keeps niri's scrolling layout.

## Getting started

### Build with Nix

```sh
nix build            # or: nix build .#newm-rs
nix run . -- --version
```

The flake's default package, `newm-rs`, builds with the `canvas` feature. It installs
`bin/newm-rs`, `bin/newm-rs-lock`, `bin/newm-rs-session`, the systemd user units
`newm-rs.service` and `newm-rs-shutdown.target`, the portals config and a `newm-rs` session file
in `share/wayland-sessions`. `newm-rs-debug` is the same build unoptimized. The flake's overlay
adds `newm-rs` to nixpkgs.

### Build with cargo

```sh
cargo build --release --features canvas
cargo build --release -p newm-rs-lock
```

This builds `target/release/newm-rs` with the canvas. It needs the libraries niri needs (see
[docs/newm-rs/development.md](docs/newm-rs/development.md)); `nix develop` provides them. Install
it as niri is installed: [niri's Getting Started](docs/wiki/Getting-Started.md) and
[packaging notes](docs/wiki/Packaging-niri.md) apply.

### The session

Start the session from a TTY with `newm-rs-session` (`resources/newm-rs-session` for a cargo build). It
imports your login environment into systemd, starts the compositor's user service and waits for
it. The Nix package installs a display-manager session file; for a cargo build, add one such as
`/usr/share/wayland-sessions/newm-rs.desktop`:

```ini
[Desktop Entry]
Name=newm-rs
Exec=newm-rs-session
Type=Application
DesktopNames=niri;KDE
```

`DesktopNames=niri;KDE` gives the session `XDG_CURRENT_DESKTOP=niri:KDE`, which KDE apps want;
see [docs/newm-rs/kde.md](docs/newm-rs/kde.md).

### The config

newm-rs reads niri's KDL config from the same place: `~/.config/niri/config.kdl` (or
`$NIRI_CONFIG`, or `--config`). On first start it writes niri's default config there. Every niri
option keeps working; newm-rs adds `layout { canvas { } }` and a few bind actions:

```kdl
layout {
    canvas {
        tile-width 720
        tile-height 465
        default-width { proportion 0.5; }
        floating-follow "parent"
    }
}

binds {
    Mod+0 { zoom-reset; }
    Mod+Z { undo; }
    Mod+Shift+Z { redo; }
}
```

`newm-rs validate` checks a config; see
[docs/newm-rs/configuration.md](docs/newm-rs/configuration.md).

## niri and newm

- **niri** gives newm-rs everything but the layout: the backends, rendering, input, protocols,
  IPC, config, animations and the overview. newm-rs swaps niri's scrolling columns for the tile
  canvas and adds session restore and the KDE services.
- **newm**, Jonas Bucher's Python compositor for laptops and touchpads, gave the idea: windows
  tiled on a 2D canvas you pan and zoom over.

## Documentation

- [Principles](docs/newm-rs/principles.md)
- [Specification](docs/newm-rs/SPEC.md): every rule, default and test
- [The canvas](docs/newm-rs/canvas.md)
- [The camera](docs/newm-rs/camera.md)
- [Input](docs/newm-rs/input.md)
- [Configuration](docs/newm-rs/configuration.md)
- [Session restore and the usage log](docs/newm-rs/session.md)
- [KDE and Plasma apps](docs/newm-rs/kde.md)
- [Wayland protocols](docs/newm-rs/protocols.md)
- [IPC](docs/newm-rs/ipc.md)
- [Development](docs/newm-rs/development.md)
- [The README video](docs/newm-rs/video.md)

niri's own documentation is in [docs/wiki](docs/wiki/README.md).

## Credits

- [niri](https://github.com/niri-wm/niri), by Ivan Molodetskikh and its contributors: newm-rs is
  niri with a different layout.
- [newm](https://github.com/jbuchermn/newm), by Jonas Bucher (jbuchermn): the idea of the 2D
  canvas comes from it.
- [Smithay](https://github.com/Smithay/smithay), the Wayland compositor library niri and newm-rs
  are built on.

## License

GPL-3.0-or-later, as niri. See [LICENSE](LICENSE).
