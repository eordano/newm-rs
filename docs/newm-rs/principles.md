# Principles

newm-rs is built on twelve principles. Each one below says what it means in practice.

### 1. Tiled windows never overlap other tiled windows.

Every tiled window owns a rectangle of tiles, and no two rectangles share a tile. When a window
grows or moves, its neighbours make room by moving aside. Nothing is ever covered. The Rocq
model of the canvas proves this for every operation.

### 2. A window is never bigger than its display.

A window opens at most as large as fits on its screen at 1:1, and growing it stops at the
screen's size. At 1:1 or zoomed out, you always see a whole window.

### 3. Zoom is a camera.

Zooming changes how the canvas is drawn, not the windows. Every window keeps its size and its
tiles, so no app gets resized, or redraws, because you zoomed. `zoom-reset` brings back 1:1,
every app at its own size.

### 4. Changing focus moves the camera as little as possible.

When the focus moves, the view slides just far enough to show the newly focused window whole. A
window already on screen gets focus without any camera motion. With `center-on-switch`, Alt+Tab
centres the window it lands on.

### 5. Moving the mouse without a click changes nothing.

Sweeping the pointer across windows leaves the focus, the layout and the camera alone. Focus
changes on a click; the camera pans only with Mod held. niri's `focus-follows-mouse` stays off
unless you turn it on. niri's top-left hot corner opens the overview, a view: the focus and the
layout stay as they were.

### 6. Everything the mouse can do, the keyboard can do too.

Dragging a window, swapping it, resizing it, panning and zooming (by wheel or pinch) all have
actions you can bind: `move-column-left` and its siblings, `swap-neighbour`, `resize-edge`, focus
changes and `zoom-in`/`zoom-out`. A drag and the same moves by keys land on the same tiles,
because both go through the same placement rules.

### 7. One hand is enough.

The mouse plus Mod moves windows (Mod + primary drag), resizes them (Mod + secondary drag) and
pans (Mod + move). Bind `zoom-in` and `zoom-out` to Mod and the wheel, as
[input.md](input.md#suggested-binds) suggests, and one hand drives the whole canvas. On a
touchpad, a three-finger pinch zooms with no bind.

### 8. The left hand alone works too.

Every canvas action is a bind, so the left hand can do everything. The suggested set puts focus
on Mod+W/A/S/D, resizing on Mod+Shift+Q/E/S/X, send-to-edge on Mod+Shift+A/D and undo on Mod+Z.

### 9. As fast and as stable as possible.

The layout is a small integer grid, and placement searches it with a range tree. Over 2,612 real
operations from a usage log, the median layout operation takes 46 µs. Disk writes (session saves
and the usage log) run on their own threads and never block the screen. Main-loop work taking
8 ms or more names itself in the debug log.

### 10. As compatible with KDE as possible.

KDE apps, themes and KWallet; KDE global shortcuts, KRunner's window search, Spectacle's
screenshots and recordings, KDE Connect's remote input and the KDE portals. See
[kde.md](kde.md).

### 11. Beautiful and joyful to use.

Windows glide to their new tiles, and the overview dims what lies outside the screen. A drag
shows where the windows will go before you drop, and a resize shows where the window will land.
niri's window rules round the corners (`geometry-corner-radius` with `clip-to-geometry`).

### 12. It respects your muscle memory.

Keys other desktops taught you keep working: Alt+Tab and Print for a screenshot in niri's
defaults, and Alt+Q to close, Mod+Q to quit the app, Mod+Return and Ctrl+Alt+T for a terminal and
Alt+Space for KRunner in the [suggested binds](input.md#suggested-binds).
