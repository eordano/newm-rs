# Configuration

newm-rs reads niri's KDL config. Everything in niri's configuration wiki
([docs/wiki](../wiki/Configuration:-Introduction.md)) still applies: outputs, input, binds,
window rules, layer rules, animations, `environment`, `spawn-at-startup` and the rest. newm-rs
adds `layout { canvas { } }` and a set of bind actions, both only in a build with the `canvas`
Cargo feature, and `synth-background { }` with two actions, which every build parses and a build
with the `synth` feature (on by default) draws. It adds no window-rule or debug options.

## Where the config lives

The same places niri looks, in this order:

1. the path given with `--config` (or `-c`);
2. `$NIRI_CONFIG`, when set and not empty;
3. `$XDG_CONFIG_HOME/niri/config.kdl`, or `~/.config/niri/config.kdl`;
4. `/etc/niri/config.kdl`.

When none exists, newm-rs writes niri's default config to the user path. The config reloads live
on save. `newm-rs validate` parses a file and prints any
error; add `-c FILE` to check a file other than the live one.

## How niri's options behave

- A node the parser does not know is an error: `newm-rs validate` points at it ("unexpected node"),
  and a running session keeps its last good config and shows niri's config error notice.
- A build without the `canvas` feature does not know `canvas { }` or the canvas actions, so a
  config using them does not parse there.
- niri's scrolling-layout options still parse, and the canvas ignores the ones it has no use
  for: it never centres a column (`center-focused-column`), and a new window takes its size from
  window rules and `canvas { default-width }`, not from the layout's `default-column-width`.
- `layout { gaps }` still applies: a window sits half a gap inside its tiles.

## `layout { canvas { } }`

```kdl
layout {
    gaps 8
    canvas {
        tile-width 720
        tile-height 465
        zoom-step 0.8
        zoom-min -2
        zoom-max 4
        overview-dim 0.4
        default-width { proportion 0.5; }
        default-height { fixed 700; }
        super-tap-overview
        super-tap-zoom 0.33
        center-on-switch
        floating-follow "parent"
        magnified-input
        restore-exclude app-id="^mpv$"
    }
}
```

Flags (`super-tap-overview`, `center-on-switch`, `restore-session`, `one-workspace`,
`magnified-input`) take no argument to turn on, or `true` / `false`. `magnified-input` lets a
click reach the app under the pointer while the camera magnifies (zoomed in past 1:1), where it
lands on the point drawn under the pointer; without it, and always while zoomed out, a click only
picks the window.

A settings app overrides most of these options and many of niri's while the session runs, over
the [live settings IPC](ipc.md#live-settings). An override stays in memory until the session
ends and stays on top of the file when it reloads; nothing is written to the file. The app's
export gives the overrides as nodes to paste here. [SPEC §3.16](SPEC.md#316-canvas-options) lists every
option with its type, default and range.

## `synth-background { }`

An animated background the compositor draws behind everything: a synthwave night with stars, a
cut sun, drifting ridges and a perspective grid, in the look the user tuned in a WebGL
playground. It is off unless the block says otherwise, and with only a mode it draws that look
exactly:

```kdl
synth-background {
    mode "animated"        // "off" (the default), "still" (one frame) or "animated"
    // max-fps 30          // a cap on moves a second (1 to 1000); unset, the display's own rate
    on-battery "still"     // on battery: "still" (the default), "animated" or "capped"
    on-battery-max-fps 30  // the cap "capped" applies (1 to 1000)
    idle-still-after 300   // seconds without input before it stands still (0 never)
}
```

Every other knob defaults to the tuned value shown here; set any of them to change it:

| Knob | Tuned | Range | What it does |
|---|---|---|---|
| `seed` | 50 | integer | Which stars and ridges |
| `horizon` | -0.22 | -1..1 | Horizon height: -0.5 bottom, 0.5 top |
| `sky-fall` | 9.3 | 0..100 | How fast the sky darkens above the horizon glow |
| `stars` | 0.69 | 0..8 | Share of star cells lit (1 = 12%) |
| `sun-size` | 0.07 | 0..1 | Sun radius, in screen heights |
| `sun-y` | 0.145 | -1..1 | Sun centre above the horizon |
| `sun-x` | -0.52 | -4..4 | Sun centre across from the middle |
| `stripes` | 7 | 0..64 | Cuts across the sun's lower part (under 0.5 none) |
| `stripe-width` | 0.5 | 0..1 | How wide the cuts grow downward |
| `halo` | 0.07 | 0..1 | Reach of the glow around the sun (0 none) |
| `reflection` | 0.45 | 0..4 | The sun's shimmering reflection on the floor |
| `layers` | 3 | 0..6 | Ridge layers |
| `ridge-height` | 0.15 | -1..1 | Nearest ridge height, in screen heights |
| `roughness` | 0.5 | 0..1 | Weight of each finer ridge octave |
| `ridge-frequency` | 9 | 0..100 | Ridge noise cells per screen height |
| `ridge-speed` | 6.9 | -100..100 | Sideways drift of the ridges |
| `haze` | 0.45 | 0..2 | How far ridges fade toward the glow colour |
| `rim` | 0.4 | 0..8 | Grid-coloured line along the ridge tops |
| `perspective` | 0.915 | 0..10 | Camera height over the floor grid |
| `grid-x` | 2 | 0..64 | Grid lines across, per floor unit |
| `grid-z` | 2.5 | 0..64 | Grid lines in depth, per floor unit |
| `line-width` | 0.7 | 0..16 | Grid line width, in pixels at their distance |
| `grid-glow` | 0.1 | 0..8 | Soft glow around the grid lines |
| `grid-speed` | 0.08 | -10..10 | How fast the grid (and the sun's cuts) move |
| `curve` | 0.03 | -1..1 | How much the floor bends down at the sides |
| `cycle-speed` | 0.15 | -100..100 | Slow cycle; one cycle lasts 60 / cycle-speed seconds |
| `cycle-sun` | 0 | -1..1 | How far the cycle moves the sun up and down |
| `cycle-hue` | 22 | -360..360 | How far, in degrees, the cycle turns the hue |
| `hue` | 0 | -360..360 | A fixed hue turn, in degrees |
| `saturation` | 0.67 | 0..4 | 1 as is, 0 grey |
| `brightness` | 0.7 | 0..4 | Factor on every colour |
| `scanlines` | 0 | 0..1 | Scanline darkening |
| `bits` | 8 | 1..8 | Bits per channel; under 7.5 dithered down |
| `audio-sun` | 0 | 0..4 | Sun growth with sound |
| `audio-grid` | 0.06 | 0..4 | Grid brightening with sound |
| `audio-ridge` | 0.02 | 0..4 | Ridge rise with sound |
| `react-to-sound` | off | flag | Listen to the sound playing at all |
| `sky-color` | `"#0b0c16"` | colour | Sky high up |
| `glow-color` | `"#2f8f5b"` | colour | Horizon glow, haze, near floor |
| `grid-color` | `"#82fb9c"` | colour | Grid lines and ridge rims |
| `sun-top-color` | `"#ddf7ff"` | colour | Top of the sun |
| `sun-bottom-color` | `"#82fb9c"` | colour | Bottom of the sun |
| `ridge-color` | `"#0d0f1c"` | colour | Nearest ridges |

`react-to-sound` (off unless set) makes the look move with the sound playing, by the `audio-*`
amounts; the amounts alone listen to nothing. It listens only while the mode is animated, the
background shows and some amount is not zero, through one passive PipeWire capture of the
default output's monitor, which never keeps an idle sound card awake. Sound never redraws
frames: it changes what the frames that come anyway draw, and a level that changes less than a
visible step redraws nothing. A reload applies every knob.

Animated, the picture moves on every frame the output presents, on the output's own frame clock
(120 times a second on a 120 Hz output, following VRR), and its time is the frames' presentation
time, so every step is the same length on a steady display. `max-fps` caps the moves for those
who want fewer. It stops moving, and asks for no frames, while the screen is blanked or locked,
the screenshot UI is open, an opaque fullscreen window covers it, opaque windows leave less than
5% of the output showing, the machine runs on battery with `on-battery "still"`, nothing was
typed or moved for `idle-still-after` seconds (unless an idle inhibitor, such as a playing video,
holds), or the knobs switch off every moving part (`stars 0`, `grid-speed 0`, `ridge-speed 0`,
`reflection 0`, and a cycle that turns neither the hue nor the sun). When it starts again it
continues from the picture it stopped on. Windows that blur what is behind them see a copy that
follows the picture once a second, so the blur is not redone on every frame.

The battery is read from the kernel's power supplies (an adapter that is not online), once and
then on each udev `power_supply` event; nothing polls. `on-battery "still"` is the default because
a laptop panel can only drop to self refresh while nothing redraws, and the shader runs over every
showing pixel each frame.

`toggle-synth-background` starts or stops the motion: from animated it goes back to the
configured mode when that is off or still (to still when the config says animated), from anything
else to animated. `set-synth-background "off"|"still"|"animated"` picks a mode. Either holds
until the config's `mode` changes. Both bind and work over IPC:
`newm-rs msg action toggle-synth-background`.

## Bind actions

The canvas actions (`resize-edge`, `zoom-in`, `zoom-out`, `zoom-reset`, `undo`, `redo`,
`gather-islands`, `send-to-edge`, `swap-neighbour`, `quit-app`, `cycle-floating-follow`,
`set-floating-follow`) go in `binds { }` like any niri action and also work over
[IPC](ipc.md); `launch-placeholder` works only over IPC.
[SPEC §5.2](SPEC.md#52-keyboard-parity-canvas-actions) says what each one does, and
[input.md](input.md#suggested-binds) suggests keys for them. niri's own actions keep working and
act on the canvas ([SPEC §5.4](SPEC.md#54-niris-default-binds-on-the-canvas)).
