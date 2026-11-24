# The camera

The screen shows part of the canvas through a camera. The camera pans and zooms; the windows stay
put. Where windows go is on [canvas.md](canvas.md); every rule and constant is in
[SPEC §4](SPEC.md#4-the-camera).

## Zoom

- Level 0 is 1:1, every app drawn at its own size. Each level scales the drawing by `zoom-step`:
  with the default 0.8, level 1 draws at 0.8 and level -1 at 1.25. Levels are whole numbers from
  `zoom-min` (-2) to `zoom-max` (4).
- A zoom changes how the canvas is drawn, never a window's size or tiles. No app is resized, and
  no app redraws, because of a zoom.
- `zoom-in`, `zoom-out` and `zoom-reset` zoom about the middle of the screen: the canvas point
  under the middle stays in place. niri's default config gives them no keys;
  [input.md](input.md#suggested-binds) suggests some, among them Super+wheel, one level per notch.
- A touchpad pinch with three or more fingers, or any pinch with Mod held, zooms in whole levels
  and keeps the level it reached. A plain two-finger pinch reaches the app under the pointer, so
  browsers and image viewers zoom their own content.
- While zoomed, a click only picks a window: it focuses it, and no pointer event reaches a scaled
  window. The keyboard always reaches the focused app.

## Overview

niri's overview shows every window at once. newm-rs dims everything outside the area the screen
shows, the active workspace's rectangle: windows beyond it, other workspaces and the backdrop.
Bars and other top and overlay layers stay undimmed. `overview-dim` sets how dark, from 0 to 1.

The overview opens and closes by pressing and releasing Mod alone (niri's default config), by a
tap of Super with `canvas { super-tap-overview }` and through niri's top-left hot corner. No
touchpad swipe opens it: four fingers up or down zoom the camera instead, which leaves every
window usable the way it is at any other zoom.

## Zoom to fit

Hold Mod and swipe four fingers on a touchpad: the camera pulls back the fewest levels, up to
`zoom-max`, that fit every window on screen, and the fingers move the focused window one tile at
a time. Release Mod and the camera returns to the level it had, showing the focused window. A
`zoom-reset` in between drops the saved level, so the camera stays at 1:1.

## Panning

Hold Mod and move the pointer, no button pressed: the canvas follows the hand, as if you held it.
The pan starts after 8 logical pixels of motion, so jitter while you press a Mod shortcut never
pans, and motion in the first 100 ms after Mod goes down counts for nothing: the hand settling as
it presses Mod, on a mouse thumb button above all, leaves the camera where it is, and a Mod+drag
begun in that time moves or resizes the window under the pointer. The 8 pixels count from where
the pointer is when the 100 ms end, and a pan under way is never held back. Release Mod and the
window nearest the middle of the screen takes the focus.
`pan-acceleration` sets how much of the pointer's acceleration the pan takes, and `pan-momentum`
how much speed it keeps after the release. On a touchpad, Mod and a three-finger swipe pan the
same way, with no threshold beyond the swipe's own; touchpad pans always glide. A Mod pan does nothing
while the screen is locked, while the overview is open or during a drag.

## Minimap

Whenever the camera, the focus or a window moves, a small plan of the canvas shows for three
quarters of a second in the bottom-left corner of the screen. The canvas has no edges, so neither has the
plan: it is a view of the canvas around the screen, with no frame, fading out toward its sides.
The screen's outline stays in the middle at one size, whatever the zoom, and the canvas slides
under it as the camera pans, its tile corners drawn as dots; what the screen does not show is
dimmed. Each window is drawn on its tiles with as many letters of its name as fit (its app's name; for
a terminal whose window is an ssh session, the remote host's, as the session saver finds it), the
focused one filled with the border's active colour and the others outlined in Nord's frost blue.
A window too far away to draw leaves a dot on the side it lies toward. It tells where the camera
is and which window lies in each direction.

newm-rs is built without the `text` feature, so the plan is painted with a 3 by 5 pixel font of
its own.

## Focus moves the camera as little as possible

When the focus changes, the camera moves the least it must to show the focused window whole. A
window already on screen gets focus without any camera motion. With `canvas { center-on-switch }`,
the window you pick in the Alt+Tab switcher or in KRunner's window search moves to the middle of
the screen. `center-column` centres the view on the focused window on demand.
