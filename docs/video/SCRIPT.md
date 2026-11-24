# newm-rs README video: script and what to explain

Target: one upbeat video for the top of the README, about 2:35, 1920x1080 at 60 fps, H.264 MP4,
plus a short silent loop (GIF or animated WebP) that plays inline.

Look: a stylized replay of the canvas (the camera box `DP-1 · 5120×1440 @ 1.25`, windows drawn by
kind, dark green theme), driven by a real usage log from a newm-rs desktop, sanitized to window
kinds and tiles (no app IDs, no titles). No key hints, no action labels, no counters on
screen: the viewer sees what the canvas does, not which keys did it. Keys in this file are for
the narration's accuracy only.

Voice: a synthetic narrator voice.
Spoken names: "new M R S" (newm-rs), "new M" (newm), "niri". Music: an original upbeat track,
generated in code, ducked under the voice.

Wording: no "... and a X that does Y" phrases, in the narration or on screen. Say what happens
plainly ("the lock screen stays locked"), not as a noun with a relative clause.

## 1. Opening (0:00-0:15)

> Introducing new M R S! A fork of niri, inspired by new M, and by amazing desktop environments
> like KDE. Built on principles.

Picture: the camera flies in from the whole canvas to 1:1 over a real arrangement from the usage log.

## 2. The principles (0:15-1:25), about 5 s each: a title and a small canvas demo

1. **Tiled windows never overlap other tiled windows.** A window grows; its neighbours are
   pushed aside, never covered.
2. **A window is never bigger than its display.** A window grows to the screen edge and stops
   there; resizing never pushes it past the display it is on.
3. **Zoom is a camera.** The camera pulls back and comes closer; every window keeps its size,
   so no app is ever resized by a zoom.
4. **Changing focus moves the camera as little as possible.** The view slides just enough to
   show the newly focused window; Alt+Tab to a window centres it.
5. **Moving the mouse without a click changes nothing.** The pointer sweeps across windows;
   focus and layout stay exactly where they were.
6. **Everything the mouse can do, the keyboard can do too.** The same rearrangement happens
   twice, once dragged and once by keys, and lands on the same tiles.
7. **One hand is enough.** The mouse plus modifier keys moves, resizes, pans and zooms
   everything (Super+drag, Super+right-drag, Super+move, Super+wheel).
8. **The left hand alone works too.** Focus on Super+W/A/S/D, resize on Super+Shift+Q/E/S/X,
   send to an edge on Super+Shift+A/D, undo on Super+Z.
9. **As fast and as stable as possible.** Median layout operation 46 µs (2,612 real operations in the log); disk writes never block the screen.
10. **As compatible with KDE as possible.** KDE apps, themes and KWallet; KDE global shortcuts,
    KRunner, Spectacle, KDE Connect, KDE portals.
11. **Beautiful and joyful to use.** Smooth glides, rounded corners, a quiet dimmed overview.
12. **It respects your muscle memory.** Alt+Tab, Alt+Q, Super+Q, Super+Return, Print,
    Ctrl+Alt+T, Alt+Space for KRunner

## 3. Features (1:25-2:30)

### The canvas

- A 2D grid of fixed tiles; partial tiles at the screen edges; apps drawn 1:1 at their own size.
- Placement rules: a move reassigns the fewest tiles, never splits an island of windows, and a
  window making room keeps its row or column.
- New windows open at the nearest free spot, with the least camera scroll, at the size the app
  asks for (rounded to tiles, remembered per app).
- Live drag preview: windows in the way step aside while you drag; Escape cancels.
- Resize preview: a box shows where the window lands and what it pushes.
- Grow and shrink by edges; shrinking closes the gaps between windows and leaves open space.
- Swap with a neighbour, send to an edge, gather islands together.
- Undo and redo every rearrangement.
- Floating windows follow their parent.

### The camera

- Zoom in and out about the middle of the screen; Super+0 back to 1:1.
- Overview of every window, with everything outside the screen dimmed.
- Zoom to fit while Super is held, back on release.
- Pan with Super and the mouse, or swipe the touchpad and let it glide.
- The view scrolls the least it must to show the focused window fully.
- Alt+Tab with live thumbnails; the window it lands on is centred.

### Your session

- Log out and back in: every window returns to its tiles.
- Terminals reattach their tmux and ssh sessions; browsers reopen their tabs (Chromium-family
  browsers relaunch with their last session, Firefox restores its own).
- The lock screen stays locked even if the locker crashes.
- Screenshots and screen recording with Spectacle and the desktop portals.

### KDE and every toolkit

- GTK 3, GTK 4, libadwaita, Qt and KDE, Firefox, Chromium and other browsers, Electron and X11 apps, crisp at fractional
  scale 1.25.
- KDE global shortcuts, KRunner window search, KDE's Inhibit portal, KDE Connect remote input.
- Input methods (fcitx5), clipboard managers, keyboard shortcuts for VM viewers and remote
  desktops, modal dialogs, the system bell, monitor power, explicit sync, sandboxed apps kept
  away from privileged protocols.

### Stability

- Every build runs real apps in a VM suite.
- Any main-loop work over 8 ms names itself in the log.

## 4. Close (2:30-2:35)

> new M R S. Check it out!

Picture: pull back to the whole canvas, the name, the repository link
(github.com/eordano/newm-rs).

## Narration as spoken

Generated by `narrate.py` from `narration.json`, one clip per beat in the narrator voice, each
checked by a speech-to-text transcript. The text below is the TTS input; the spoken names are spelled
for the voice ("new M R S", and "new-M" where "new M" was heard as "GNU M"). The feature
lines are the narration's short form of section 3.

- `open1` (1.8 s): Introducing new M R S!
- `open2` (6.5 s): A fork of niri, inspired by new-M, and by amazing desktop environments like KDE.
- `open3` (1.4 s): Built on principles.
- `p1` (4.8 s): One. Tiled windows never overlap other tiled windows.
- `p2` (3.6 s): Two. A window is never bigger than its display.
- `p3` (5.6 s): Three. Zoom is a camera: no app is ever resized by a zoom.
- `p4` (4.5 s): Four. Changing focus moves the camera as little as possible.
- `p5` (3.4 s): Five. Moving the mouse without a click changes nothing.
- `p6` (4.1 s): Six. Everything the mouse can do, the keyboard can do too.
- `p7` (2.7 s): Seven. One hand is enough.
- `p8` (3.0 s): Eight. The left hand alone works too.
- `p9` (9.0 s): Nine. As fast and as stable as possible: forty-six microseconds per layout, and disk writes never block the screen.
- `p10` (3.2 s): Ten. As compatible with KDE as possible.
- `p11` (3.9 s): Eleven. Beautiful, and joyful to use.
- `p12` (2.8 s): Twelve. It respects your muscle memory.
- `canvas1` (4.0 s): A grid of fixed tiles, every app drawn at its own size.
- `canvas2` (2.6 s): New windows open at the nearest free spot.
- `canvas3` (3.5 s): Drag a window, and the others step aside, live.
- `canvas4` (2.8 s): Resize, and a box shows where it lands.
- `canvas5` (1.8 s): Undo and redo everything.
- `camera1` (3.8 s): Zoom about the middle, and one key brings you back to one to one.
- `camera2` (3.9 s): The overview shows every window, with the rest dimmed.
- `camera3` (4.3 s): Alt Tab shows live thumbnails, and centres the window.
- `session1` (4.0 s): Log out and back in: every window returns to its tiles.
- `session2` (4.3 s): Terminals reattach their sessions, and browsers reopen their tabs.
- `session3` (3.6 s): The lock screen stays locked, even if the locker crashes.
- `kde1` (8.9 s): GTK, Qt and KDE, Firefox, Chromium and other browsers, Electron and X11 apps: all crisp at fractional scale.
- `kde2` (5.6 s): KDE shortcuts, KRunner, KDE Connect and the portals all work.
- `stab1` (2.9 s): Every build runs real apps in a VM suite.
- `close` (2.5 s): new M R S. Check it out!

For pacing and privacy (the timeline fits about 2:35):

- Principle 9 speaks the stat as "forty-six microseconds per layout"; the screen keeps
  "46 µs, median layout operation" and the count recomputed from the log (2,612 ops).
- Apps are named only as families: "Firefox, Chromium and other browsers", "terminals",
  "browsers reopen their tabs"; window title bars show only kinds.
- Alt+Tab line: "Alt Tab shows live thumbnails, and centres the window."
- Opening: "Built on principles."; principle 1 starts on the bar at 0:14, the music's rise
  filling the beat after the line.
