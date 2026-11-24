# IPC

newm-rs answers niri's IPC unchanged: the socket in `$NIRI_SOCKET`, `newm-rs msg` on the command
line (niri's own `niri msg` works too), and the JSON protocol described in
[niri's IPC page](../wiki/IPC.md). A build with the `canvas` feature
adds the [live settings](#live-settings) requests, the [block](#blocks) requests and the
[canvas actions](SPEC.md#52-keyboard-parity-canvas-actions):

```sh
newm-rs msg action zoom-out
newm-rs msg action resize-edge right 2
newm-rs msg action swap-neighbour up
newm-rs msg action set-floating-follow parent
newm-rs msg action launch-placeholder --app-id org.example.App
```

Every build also takes `toggle-synth-background` and `set-synth-background off|still|animated`
([configuration.md](configuration.md#synth-background-)); without the `synth` feature they only
log a warning.

`launch-placeholder --app-id ID [--icon PATH]` exists only over IPC and suits launchers: called
right before starting an app, it draws a placeholder tile where the app's next window opens, at
the size window rules matching that app ID alone give it (else the canvas default), until a
window with that app ID maps there or 10 seconds pass.

Next to the socket, `$NIRI_SOCKET.floating-follow` holds the current floating-follow mode on one
line (`screen`, `parent` or `canvas`). It changes by atomic rename whenever the mode does, so a
status bar can watch it.

## Live settings

A settings app can read and change newm's settings while the session runs. A build
with the `canvas` feature answers five requests and sends one event for them.

- An override lives in memory until the compositor exits. Nothing is written to disk.
- An override applies through the path a config file reload takes: the compositor takes the
  config file as last loaded, applies every override on top of it, and reloads that config.
- Every config file load applies the overrides again, so a reload (a deploy that rewrites the
  file) keeps them. `config_value` always shows what the file says.

| Request | JSON | Reply |
| --- | --- | --- |
| `NewmOptions` | `"NewmOptions"` | `{"Ok":{"NewmOptions":[option, ...]}}` |
| `NewmSetOption` | `{"NewmSetOption":{"key":"layout.gaps","value":12}}` | `{"Ok":"Handled"}` or `{"Err":"..."}` |
| `NewmResetOption` | `{"NewmResetOption":{"key":"layout.gaps"}}` | `{"Ok":"Handled"}` or `{"Err":"unknown option ..."}` |
| `NewmResetAll` | `"NewmResetAll"` | `{"Ok":"Handled"}` |
| `NewmExport` | `"NewmExport"` | `{"Ok":{"NewmExport":{"kdl":"..."}}}` |

Every set and reset, and every config file load, sends the event
`{"NewmOptionsChanged":[option, ...]}` to each event stream. A new event stream gets the list in
its first burst.

One option:

```json
{
  "key": "layout.canvas.pan-momentum",
  "kind": "float",
  "value": 0.25,
  "config_value": 1.0,
  "overridden": true,
  "min": 0.0,
  "max": 1.0,
  "step": 0.05,
  "choices": null,
  "unit": null,
  "category": "Camera",
  "label": "Pan glide after releasing Mod",
  "help": "At 0 the camera stops where the hand stops, and at 1 it glides on like a touchpad swipe."
}
```

- `kind` is `bool`, `int`, `float`, `choice`, `color` or `size`, and says what `value` holds:
  a JSON boolean, a whole number, a number, one of `choices`, a color string, or a new window's
  length. A set takes any CSS color (`"#rrggbb"`, `"#rrggbbaa"`, `"red"`); the answer is
  `"#rrggbbaa"`. A `size` (`default-width`, `default-height`) is one number: 0 is one tile, a
  value up to 1 is a share of the canvas, and a larger one is pixels.
- `min`, `max` and `step` are set for `int` and `float`; `choices` for `choice`; `unit` (`px`,
  `ms`, `/s`, `steps`) where the number has one.
- `category` is one of `Canvas`, `Camera`, `Pointer`, `Touchpad`, `Keyboard`, `Cursor`,
  `Appearance`, `Animations`, `Displays`. `label` is a short fragment the control completes;
  `help` is one sentence.
- `Displays` holds one option per connected display, after the fixed ones: its scale, a `float`
  from 0.5 to 4 in steps of 0.25 with the key `output.NAME.scale` (`NAME` is the connector, such
  as `eDP-1`, as an `output "NAME"` node names it). Its `config_value` is the scale the config
  file gives the display or, when the file gives none, the one the compositor chose for it.
  Plugging a display in or out sends `NewmOptionsChanged`; an override stays for a display that
  is unplugged and applies when it returns. A display's scale decides how many tiles its screen
  holds, and a window never grows past its screen, so a display at too large a scale leaves the
  canvas little room.

A set checks the value against the kind and the bounds and answers why it refuses one:

```json
{"NewmSetOption":{"key":"layout.canvas.pan-momentum","value":3}}
{"Err":"value 3 is above the maximum 1"}
```

The export writes a KDL node for every override, nested as in the config file, after a comment
line that names the build; it parses with the config parser. A switch that is off but has no
`false` form in the config (such as `tap`) and an unset choice (`accel-profile "default"`) come
out as a comment that says to delete the node:

```kdl
// Live settings exported from newm-rs 26.04.
layout {
    gaps 12
    canvas {
        pan-momentum 0.25
        magnified-input
    }
}
input {
    touchpad {
        // tap: off, so the config file has no "tap" node
    }
}
```

On the command line, `newm-rs msg newm-options`, `newm-set-option KEY VALUE` (the value as
JSON; text that is not JSON is a string), `newm-reset-option KEY`, `newm-reset-all` and
`newm-export` send them.

### Keys

The table comes from the registry in `niri-config/src/live.rs`, the one place that defines
every key.

| Key | Kind | Range | Category | Label |
| --- | --- | --- | --- | --- |
| `layout.canvas.center-on-switch` | bool | true, false | Canvas | Centre the window picked in the switcher |
| `layout.canvas.super-tap-overview` | bool | true, false | Canvas | Super tap opens the overview |
| `layout.canvas.floating-follow` | choice | screen, parent, canvas | Canvas | Floating windows follow |
| `layout.canvas.overview-dim` | float | 0 to 1, step 0.05 | Canvas | Overview dims the area off screen by |
| `layout.canvas.default-width` | size | 0 to 7680 px, step 1; 0 is one tile, up to 1 a share | Canvas | New windows open this wide |
| `layout.canvas.default-height` | size | 0 to 4320 px, step 1; 0 is one tile, up to 1 a share | Canvas | New windows open this tall |
| `layout.canvas.distance-weight` | float | 0 to 100, step 0.05 | Canvas | Cost per tile a window moves to make room |
| `layout.canvas.diagonal-weight` | float | 0 to 100, step 0.05 | Canvas | Cost of making room along both axes |
| `layout.canvas.zoom-step` | float | 0.05 to 1, step 0.05 | Camera | Scale change per zoom step |
| `layout.canvas.zoom-min` | int | -16 to 16 steps, step 1 | Camera | Closest zoom level |
| `layout.canvas.zoom-max` | int | -16 to 16 steps, step 1 | Camera | Farthest zoom level |
| `layout.canvas.magnified-input` | bool | true, false | Camera | Clicks reach apps while zoomed in |
| `layout.canvas.pan-acceleration` | float | 0 to 1, step 0.05 | Camera | Share of pointer acceleration a Mod pan takes |
| `layout.canvas.pan-momentum` | float | 0 to 1, step 0.05 | Camera | Pan glide after releasing Mod |
| `input.mouse.natural-scroll` | bool | true, false | Pointer | Natural scrolling |
| `input.mouse.accel-speed` | float | -1 to 1, step 0.05 | Pointer | Pointer speed |
| `input.mouse.accel-profile` | choice | default, adaptive, flat | Pointer | Acceleration profile |
| `input.mouse.scroll-factor` | float | 0 to 10, step 0.1 | Pointer | Scroll distance multiplier |
| `input.touchpad.tap` | bool | true, false | Touchpad | Tap to click |
| `input.touchpad.natural-scroll` | bool | true, false | Touchpad | Natural scrolling |
| `input.touchpad.accel-speed` | float | -1 to 1, step 0.05 | Touchpad | Pointer speed |
| `input.touchpad.accel-profile` | choice | default, adaptive, flat | Touchpad | Acceleration profile |
| `input.touchpad.scroll-factor` | float | 0 to 10, step 0.1 | Touchpad | Scroll distance multiplier |
| `input.keyboard.repeat-delay` | int | 50 to 2000 ms, step 10 | Keyboard | Key repeat starts after |
| `input.keyboard.repeat-rate` | int | 0 to 100 /s, step 1 | Keyboard | Key repeats per second |
| `cursor.xcursor-size` | int | 8 to 128 px, step 1 | Cursor | Cursor size |
| `cursor.hide-when-typing` | bool | true, false | Cursor | Hide the cursor while typing |
| `layout.gaps` | size | 0 to 200 px, step 1 | Appearance | Gap between windows |
| `layout.focus-ring.width` | size | 0 to 64 px, step 1 | Appearance | Focus ring width |
| `layout.focus-ring.active-color` | color | a CSS color, answered as `#rrggbbaa` | Appearance | Focus ring color |
| `layout.border.width` | size | 0 to 64 px, step 1 | Appearance | Border width |
| `layout.border.active-color` | color | a CSS color, answered as `#rrggbbaa` | Appearance | Focused window's border color |
| `layout.border.inactive-color` | color | a CSS color, answered as `#rrggbbaa` | Appearance | Other windows' border color |
| `animations.off` | bool | true, false | Animations | Turn animations off |
| `animations.slowdown` | float | 0.1 to 10, step 0.1 | Animations | Animation slowdown |

Not live settings: an output's mode, position and other properties than its scale (the `Output`
request changes them), binds, window and layer
rules, workspaces, the environment and startup commands (they are not plain values, or they act
only at start), the tile size (a change re-lays out every window), `one-workspace` (turning it on
gathers windows) and `restore-session` (it acts only at start).

## Blocks

A client that brings windows over from another host places them on the active canvas as one
block: the compositor reserves the block's tiles at once, launches each window's command, and
gives each new window its slot. A build with the `canvas` feature answers three requests.
[SPEC 6.9](SPEC.md#69-blocks-over-ipc) has the rules.

| Request | JSON | Reply |
| --- | --- | --- |
| `NewmPlaceBlock` | `{"NewmPlaceBlock":{"windows":[window, ...],"anchor":anchor,"focus":1,"fly":true}}` | `{"Ok":{"NewmBlock":{"id":1,"origin":[12,0]}}}` or `{"Err":"..."}` |
| `NewmBlockStatus` | `{"NewmBlockStatus":{"id":1}}` | `{"Ok":{"NewmBlockStatus":{"id":1,"windows":[42,null],"done":false}}}` or `{"Err":"no such block"}` |
| `NewmParkWindows` | `"NewmParkWindows"` | `{"Ok":"Handled"}` |

One window of a block:

```json
{
  "command": ["foot", "--working-directory", "/home/me/src"],
  "cwd": "/home/me/src",
  "env": {"SOME_VAR": "1"},
  "app_id": "foot",
  "rect": [0, 0, 8, 6]
}
```

- `rect` is the window's tiles relative to the block's origin: column, row, width and height.
- `cwd` and `env` are optional. A `cwd` that does not exist is skipped; a command whose path does
  not exist runs by its program's name.
- `anchor` is `"Nearest"` (the free spot nearest the view's middle, one tile clear of every
  window), `"Camera"` (centred on the view) or `{"Tiles":[column,row]}` (that origin exactly).
- `focus` is the index of the window that takes the focus once the block is complete, or `null`.
  `fly` moves the camera to show the block as it is placed.
- `origin` in the reply is the tile the block's origin is on.
- `windows` in a status holds, per slot, the id of the window that took it (as `Windows` lists
  it), or `null`. `done` is true once every slot has a window or was released: a slot no window
  takes within 20 seconds is released.
- Errors: `bad block` (no windows, a window without a command, or a rect without area) and
  `the block's tiles are taken` (a `Tiles` anchor on a tile a window holds).

`NewmParkWindows` moves the active canvas's tiled windows together, keeping their arrangement,
to just left of the view. The camera then shows the parked window nearest the view at its left
edge, which takes the focus; the rest of the view is free for a block. One undo takes the park
back.
