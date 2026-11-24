# Input

newm-rs ships niri's default config unchanged, so a fresh install has niri's key bindings. The
canvas gives most of them a grid meaning ([SPEC §5.4](SPEC.md#54-niris-default-binds-on-the-canvas))
and adds pointer and touchpad behaviour, no bind needed. The canvas actions have no keys by
default; the [suggested binds](#suggested-binds) below give them some.

Mod is niri's mod key: Super in a normal session, Alt when newm-rs runs nested in another
window. niri's `input { mod-key }` changes it.

## Mouse

- Moving the pointer changes nothing: focus, windows and the camera stay where they are. Two niri
  features are exceptions only by choice: `input { focus-follows-mouse }` (off unless set), and
  the top-left hot corner, which opens the overview, a view that leaves focus and layout alone.
- A click focuses the window under the pointer.
- Mod + primary drag, or a title-bar drag, moves the window; the windows in its way step aside
  live, and the release drops it.
- Mod + secondary drag resizes by whole tiles, with a preview box.
- Escape cancels either drag.
- Mod + moving the pointer, no button, pans the camera; releasing Mod focuses the window nearest
  the middle of the screen. Motion in Mod's first 100 ms pans nothing, so a Mod + secondary press
  then resizes, and a Mod + primary press moves, the window under the pointer.

[SPEC §5.1](SPEC.md#51-pointer) has the full table, [canvas.md](canvas.md#moving-and-resizing)
the drags and [camera.md](camera.md#panning) the pans.

## Touchpad

| Gesture | What it does |
|---|---|
| Three fingers | Moves the focus one window the way the fingers go: one window per swipe. |
| Four fingers sideways | Moves the focus one window left or right. |
| Four fingers up or down | Zooms the camera a level per step: up pulls it back, down brings it closer. |
| Mod + three fingers | Pans the camera after the fingers. |
| Mod + four fingers | Zooms out to fit the canvas and moves the focused window one tile per step; releasing Mod zooms back. |
| Three or more fingers, pinching | Zooms the camera in whole levels: pinching in pulls it back, spreading brings it closer. |
| Mod + a pinch, any number of fingers | Zooms the camera the same way. |
| Two fingers, pinching | Reaches the app under the pointer, as browsers and image viewers expect. |

Thresholds, steps and the pinch's level: [SPEC §4.6](SPEC.md#46-touchpad-gestures).

## Suggested binds

These are suggestions, not defaults: copy what you like into `binds { }`. niri refuses a config
binding one key twice, so first delete the default config's binds on the same keys: Mod+D,
Mod+W, Mod+Q, Mod+Minus, Mod+Equal, Mod+Shift+E (quit; Ctrl+Alt+Delete still quits),
Mod+WheelScrollUp and Mod+WheelScrollDown. Mod+D is the default launcher bind; move the launcher
to another key.

```kdl
binds {
    // One hand on the mouse: Mod+wheel zooms one level per notch.
    // Wheel binds take only a mouse wheel; touchpad scrolling reaches the app.
    Mod+WheelScrollUp { zoom-in; }
    Mod+WheelScrollDown { zoom-out; }

    // The left hand alone: focus, resize, send to an edge, undo.
    Mod+W { focus-window-up; }
    Mod+A { focus-column-left; }
    Mod+S { focus-window-down; }
    Mod+D { focus-column-right; }
    Mod+Shift+Q { resize-edge "left" 1; }
    Mod+Shift+E { resize-edge "right" -1; }
    Mod+Shift+S { resize-edge "top" -1; }
    Mod+Shift+X { resize-edge "bottom" 1; }
    Mod+Shift+A { send-to-edge "left"; }
    Mod+Shift+D { send-to-edge "right"; }
    Mod+Z { undo; }
    Mod+Shift+Z { redo; }

    // Camera and canvas.
    Mod+0 { zoom-reset; }
    Mod+Equal { zoom-in; }
    Mod+Minus { zoom-out; }
    Mod+G { gather-islands; }
    Mod+Alt+Left { swap-neighbour "left"; }
    Mod+Alt+Right { swap-neighbour "right"; }
    Mod+Alt+Up { swap-neighbour "up"; }
    Mod+Alt+Down { swap-neighbour "down"; }

    // Muscle memory: use your own terminal.
    Alt+Q { close-window; }
    Mod+Q { quit-app; }
    Mod+Return { spawn "alacritty"; }
    Ctrl+Alt+T { spawn "alacritty"; }
    Alt+Space { spawn "krunner"; }
}
```

niri's defaults already cover Alt+Tab (the window switcher) and Print (screenshot). The reference
bind set the author uses is in [SPEC §5.3](SPEC.md#53-reference-bind-set).
