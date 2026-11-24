# newm-rs specification

This document specifies newm-rs completely enough to rebuild it from scratch and to test the
rebuild against the original. It describes the tree as it stands on `main`. Where the author asked
for something the code does differently, or never got, section 15 lists the gap.

Source: https://github.com/eordano/newm-rs

## 0. Conventions

- MUST and MUST NOT state invariants. Every invariant names, where it exists, the test or proof
  claim that checks it.
- "Mod" is niri's mod key: Super in a session on a TTY, Alt when newm-rs runs nested as a window.
- Sizes are logical pixels unless marked physical. A *tile* is a cell of the canvas grid; a *span*
  is a window's width and height in tiles.
- Defaults are the code's. The author's own configuration differs in places; this document gives
  it as "the reference configuration".

## 1. Purpose and principles

newm-rs is a Wayland compositor: a fork of niri with niri's scrolling columns replaced by a strict
2D canvas of fixed tiles. The canvas comes from newm, Jonas Bucher's Python compositor; the
desktop integration follows KDE. Twelve principles decide every behaviour. Their wording is
fixed (the README video speaks them as written here); [principles.md](principles.md) says what
each one means in operation.

| # | Principle | How it is tested |
|---|---|---|
| 1 | Tiled windows never overlap other tiled windows. | `Canvas::check` after every step of the replay harness (400 seeds x 200 random operations); Rocq claims C01, C02, C03, C17, C50, C52. |
| 2 | A window is never bigger than its display. | Claim C05; replay fixed scenario "a grow stops at the screen edge"; `open_span` claims C29, C44. |
| 3 | Zoom is a camera. | Claims C04, C16, C26, C36-C41; replay camera tests. |
| 4 | Changing focus moves the camera as little as possible. | Claims C10, C11, C12; layout tests for `center-on-switch`. |
| 5 | Moving the mouse without a click changes nothing. | Layout tests; niri's `focus-follows-mouse` is off unless the user sets it. |
| 6 | Everything the mouse can do, the keyboard can do too. | Layout tests run drags and key moves through the same `Canvas` operations; replay harness covers `move_to` and `move_by`. |
| 7 | One hand is enough. | Input tests in `src/input/canvas.rs`, `src/tests/mod_pan.rs` and `src/tests/pinch.rs`; the reference bind set. |
| 8 | The left hand alone works too. | The reference bind set parses (`niri-config` tests). |
| 9 | As fast and as stable as possible. | `newm-canvas/tests/bench.rs`; `op_us` in a telemetry build's usage log; slow-work logging (section 10); compat robustness cases. |
| 10 | As compatible with KDE as possible. | `src/dbus/kde_tests.rs`; compat `desktop`, `remote-input` and `smoke` suites. |
| 11 | Beautiful and joyful to use. | Visual review; usage-log motion metrics (no frame jumps). |
| 12 | It respects your muscle memory. | The reference bind set (section 5.3). |

## 2. Foundations

### 2.1 A fork of niri that tracks upstream

- The base is niri v26.04 (workspace version `26.4.0`). newm-rs follows niri's `main` by merging it;
  the last merge base is niri commit `689c1e91` (2026-11-15).
- Changes stay small and grouped so upstream merges stay cheap:
  - new code lives in its own files (`src/layout/canvas.rs`, `src/layout/canvas/`,
    `src/input/canvas.rs`, `src/session.rs`, `src/session/`, the KDE files in `src/dbus/` and
    `src/protocols/`, and the crates below);
  - the canvas replaces the scrolling layout only under the Cargo feature `canvas`;
  - edits to niri's own files are hooks into the new files, mostly behind `#[cfg(feature = "canvas")]`.
- Without `canvas` the build is niri plus newm-rs's non-layout changes (KDE services, protocols,
  stability fixes), with the scrolling layout.
- `docs/wiki/` stays as upstream has it; `niri-config/tests/wiki-parses.rs` parses its KDL.

Crates and directories:

| Path | Contents |
|---|---|
| `src/` | The compositor (niri), with the canvas glue, canvas input, session restore, KDE D-Bus services and extra protocols. |
| `newm-canvas/` | The canvas model: grid, placement engine, push, fill, undo, camera arithmetic. No Wayland, no niri. Has its own tests and the Rocq proof in `newm-canvas/proof/`. |
| `niri-config/` | niri's KDL parser plus `src/canvas.rs` for `layout { canvas { } }`. |
| `niri-ipc/` | niri's IPC types plus `src/canvas.rs` for the canvas actions. |
| `newm-kde/` | Logic of the KDE services (kglobalaccel, KRunner, inhibitions, Qt key codes) with no D-Bus library, so it is unit-testable. |
| `newm-rs-lock/` | The screen locker (section 7). |
| `compat/` | The VM compatibility suite (section 11.3) and `PROTOCOLS.md`. |
| `newm-mac/` | A macOS experiment driving windows with the canvas through the Accessibility API; its own Cargo workspace and lock file. |
| `contrib/usage-summary.py` | Summarizes the usage log with Python's standard library. |
| `docs/video/` | The README video generator (section 12). |

Cargo features:

| Feature | Default | What it enables |
|---|---|---|
| `canvas` | off | newm's canvas in place of the scrolling layout; session restore; the canvas config and actions in `niri-config` and `niri-ipc`. |
| `telemetry` | off | With `canvas`, the usage log (6.7): its writer thread and per-operation timing. |
| `dbus` | on | zbus services, accessibility, `newm-kde`. |
| `systemd` | on | Global environment export, apps in transient scopes. |
| `xdp-gnome-screencast` | on | PipeWire screencasts for xdg-desktop-portal-gnome. |
| `text` | on | pango text: hotkey overlay, exit dialog, config-error notice, screenshot hints, switcher titles. |
| `dinit`, `profile-with-tracy*` | off | As in niri. |

The newm-rs build MUST be `--no-default-features --features canvas,dbus,systemd,xdp-gnome-screencast`
(so `text` and `telemetry` are off), building the packages `newm-rs` and `newm-rs-lock`. The
binary is `newm-rs`, and `resources/` holds the session files under newm-rs's names:

- `newm-rs-session` starts `newm-rs.service`, with
  `trap "systemctl --user stop newm-rs.service" HUP TERM` before it waits on the unit;
- `newm-rs.service` (`ExecStart=newm-rs --session`); packaging points `ExecStart` at the installed
  binary and adds `X-RestartIfChanged=false`, so a system switch never restarts a running session;
- `newm-rs-shutdown.target`, and `newm-rs.desktop` with `DesktopNames=niri;KDE`.

`compat/newm-rs.nix` is the reference packaging.

### 2.2 Language and quality bar

- Rust edition 2024 for every crate but niri's `niri-visual-tests` (2021).
- Zero compiler warnings. `cargo clippy --all-targets --features canvas -- -D warnings` MUST pass.
- `cargo test --features canvas` MUST pass, including all of niri's tests: the layout property
  tests and the client-server tests with their insta snapshots. The canvas, session and usage-log
  tests compile only with `--features canvas`.
- `cargo test -p newm-canvas` and `cargo test -p newm-rs-lock` MUST pass.
- Format with `cargo +nightly fmt --all`, as niri does.
- A change to `newm-canvas` MUST keep the proof gate green (section 3.15).

### 2.3 Configuration

- The config is niri's KDL, read from the same places: `--config`/`-c`, `$NIRI_CONFIG`,
  `$XDG_CONFIG_HOME/niri/config.kdl`, `/etc/niri/config.kdl`. A missing config makes newm-rs write
  niri's default config, unchanged.
- Every niri option keeps its meaning where it makes sense on a grid. newm-rs adds one section,
  `layout { canvas { } }` (section 3.16), and the canvas actions (section 5.2). It adds no window
  rule or debug option.
- An unknown node is an error. A running session keeps its last good config and shows the
  config-error notice.
- Live settings: over IPC a settings app overrides the options listed in `niri-config/src/live.rs`
  ([IPC](ipc.md#live-settings)). An override stays in memory for the session, applies through
  the config reload path, and applies again on top of every config file load; nothing is written
  to disk.
- `layout { gaps }` applies: a window sits half a gap inside its cell.
- Scrolling-layout options still parse; the canvas ignores those it has no use for
  (`center-focused-column`, the layout's `default-column-width`).

### 2.4 No text rendering

The author chose "no text at all": newm-rs builds without pango. With `text` off:

- the hotkey overlay, exit-confirmation dialog and config-error notice draw as textless panels (the
  exit dialog is a 320x112 dark panel with a red frame; the config notice is a coloured bar, green
  when a default config was written, red on an error);
- the screenshot UI shows no hints;
- the Alt+Tab switcher shows live thumbnails without titles.

`text` stays in the default feature set to match upstream, so a plain
`cargo build --features canvas` renders text (open item 1).

### 2.5 Xwayland

X11 apps run through xwayland-satellite, which niri spawns from the config's
`xwayland-satellite { path ... }` block. Frame callbacks MUST reach X11 windows even when they are
off screen (niri's visibility check is skipped for xwayland-satellite's clients, keeping the
once-per-refresh-cycle dedup): Xwayland completes a present only on a frame callback, and
Chromium's GPU thread presents to every window, so one undrawn X11 window would drop every
Chromium window to one frame per second. An unconditional send MUST NOT be used: on a TTY it
redraws after every commit, a busy loop.

Fixes for X11 popup parents, opaque regions and drag and drop between X11 and Wayland apps are
xwayland-satellite patches outside this repository (open item 11).

### 2.6 Screen sharing and recording

- Screencasts go through niri's in-compositor PipeWire path and xdg-desktop-portal-gnome, as in
  niri (Mutter-compatible D-Bus interfaces).
- Spectacle and other KDE apps record through `zkde_screencast_unstable_v1` version 5 (section 8.4).

## 3. The canvas model

### 3.1 Terms

| Term | Meaning |
|---|---|
| Canvas | One unbounded integer grid per workspace (one workspace per output with `one-workspace`). |
| Tile | A grid cell of fixed logical size `tile-width` x `tile-height` at 1:1. |
| Rect | `(i, j, w, h)`: column, row, width and height in tiles; `w, h >= 1`. |
| Span | `(w, h)` of a rect. |
| Need | The span an app's minimum size requires (3.4). |
| Island | A maximal set of windows connected through shared sides. Two rects *touch* when they share a side of positive length; a corner does not count. |
| Bridge | A window whose neighbours fall on different islands without it. |
| Lake | A gap left by a shrink or close with a window lying against it on the far side. |
| Ocean | Such a gap with nothing beyond it: it opens onto the empty canvas. |
| Zoom | An integer camera level; scale = `zoom-step ^ zoom`. |
| View | The camera's offset into the canvas, in on-screen logical pixels at the current zoom. |
| Recent | The window ids, most recently focused first. |

### 3.2 Grid geometry

- Tile size is fixed in logical pixels and does not depend on the screen. Default 720x465.
  The reference configuration uses small tiles so a window can reach the screen's full size
  with fine granularity: about 119x113, which gives 12x8 tiles on a 1440x960 laptop panel and
  34x9 on a 5120x1440 ultrawide at scale 1.25 (4096x1152 logical).
- The canvas area (the "screen" of the model) is the output's working area (the output less
  layer-shell exclusive zones such as a bar, less niri's struts) less half a gap on each side.
  A screen rarely holds a whole number of tiles; the tiles at its edges show in part.
- A window's cell is its rect in canvas pixels; the window sits half a gap inside the cell.
- At zoom 0 every app is drawn 1:1, pixel for pixel. Grid tiles render at 1:1 in their own frame
  and are drawn through the camera, which scales them about the canvas's top-left.
- Cell edges snap to physical pixels from their grid line:
  `edge_x(k) = round((k * tile_w_on_screen - view_x) * output_scale)`, likewise `edge_y`, and a
  rect's physical size is the difference of its two edges. Neighbours MUST share their edge pixel
  exactly at any output scale and view (claim C13).

### 3.3 Invariants

After every public canvas operation (`Canvas::check`; claims C01, C09, C10, C17, C30):

- MUST: window ids are unique.
- MUST: no two tiled rects overlap.
- MUST: every window's span is at least its need.
- MUST: `recent` holds every window id exactly once, the focus first.
- MUST: with windows on the canvas, the focus is one of them.
- MUST: a focused window whose span fits on screen at the current zoom is fully on screen
  (except right after `scroll_by`/`set_view`, which a pan's end follows with a refocus).
- MUST NOT: any operation but `zoom_by` and `set_config` changes the zoom; `set_config` only clamps
  it into the new bounds (C04, C26).
- MUST: a window's span changes only through a resize or its need (the span principle, C47).
  Moves, swaps, drags, zooms, gathers, undo of others' moves and neighbours' growth keep it.

### 3.4 Minimum sizes

- `need(min_w, min_h) = (max(1, ceil(min_w / tile_w - 1e-6)), max(1, ceil(min_h / tile_h - 1e-6)))`
  with the 1:1 tile, whatever the zoom. `min` includes the window's border and gaps plus one
  pixel for rounding.
- An app's minimum makes its window take more tiles; the window is never drawn below the size the
  app allows, and never scaled down to fit.
- An app committing a buffer larger than it was asked for (it kept a size when asked for less) has
  that size counted as its effective minimum on that axis, so its window takes the tiles holding
  it and is drawn 1:1 without overlapping neighbours. The kept size falls back to none once the app
  commits a smaller size in answer to a request.
- Client-supplied minimums are clamped to 65,535 px and at most 1,024 tiles, so tile arithmetic
  stays in `i32`.
- Growth from a raised minimum (`set_min`, `set_config`, restore) widens then heightens the window
  by the placement rules; it records no undo step (undo must not take an app below the size it
  insists on), and the usage log records it as `app-grow` with its cause (`min-size`,
  `kept-commit`, `config`) and `pushed` when the rules found no layout.

### 3.5 Never bigger than the display

- `fits_at_1to1(w, h)`: `w * tile_w <= screen_w + 1e-6` and `h * tile_h <= screen_h + 1e-6`.
- MUST: a grow MUST be refused when the new span does not fit at 1:1 (C05).
- MUST: `open_span` cuts any wanted span to the largest that fits at 1:1, and never below need.
- A need larger than the screen wins over the display bound (the app cannot be drawn smaller).
- Zoomed in past 1:1, a window may be larger than the screen; it is then shown from its top-left.

### 3.6 Operations

| Operation | Effect | Records undo | Others move? |
|---|---|---|---|
| `add(id, min, want)` | Opens at `open_span` on the spot 3.9 picks, focuses it. | No; forgets history | Never (C28) |
| `add_at(id, min, at)` | Restore: opens at `open_span(at's span)` on `at` if free, else the nearest free spot (rings outward, ties up then left). | No; forgets history | Never (C43) |
| `remove(id)` | Closes; fills its lake from the right, then from below, unless that splits an island; a bridge's close lets its parts gather. Focus goes to the most recent window, else the nearest. | No; forgets history | Lake only |
| `resize(id, edge, delta)` | Moves one edge by whole tiles: outward grows (placement rules, push fallback), inward shrinks and fills the lake. Refused at `delta = 0`, below need, or a grow not fitting at 1:1. | Yes | Yes |
| `move_by(id, dir)` | One tile along `dir`, placement rules seeded by the push; the gap behind stays open. | Yes | Yes |
| `move_to(id, i, j)` | Drop at `(i, j)` keeping the span; placement rules; a bridge's island gathers. A move onto its own tiles changes nothing. | Yes | Yes |
| `swap(dir)` | Focus and its neighbour in `dir` trade places, each keeping its span (3.11). | Yes | Yes |
| `send_to_edge(dir)` | Left or right, in its row, past every other window. | Yes | Yes |
| `gather_islands()` | Every island but the largest slides to touch it (3.8.6). Refused with fewer than two islands or when none can move. | Yes | Yes |
| `set_min(id, min)` | Raises the span to the new need (3.4). | No | By rules |
| `focus_id`, `focus_dir` | Focus changes; view moves the least. | No | No |
| `zoom_by(delta)` | Camera only (section 4). | No | No |
| `set_screen`, `set_config` | New canvas area or config; config clamps zoom, enforces needs. | No | By rules |
| `rename(from, to)` | Keeps rect, minimum, focus and recency under a new id. | Forgets history | No |
| `undo`, `redo` | Restores a snapshot (3.12). | Moves between stacks | Yes |

### 3.7 Growing and shrinking: lakes and ocean

- A grow places the window on its new rect by the placement rules (3.8), seeded with the push
  along the grown edge's outward direction; when no layout passes, it pushes.
- A shrink frees a strip on the shrunk side. The *chain* is the windows lying against the strip on
  that side, and the windows lying against those, transitively. The chain slides into the gap
  together, by as many tiles as it can (at most the gap's depth) without overlapping anything.
- MUST: an ocean strip (empty chain) moves no other window (C07).
- MUST: when a fill would split an island, it is reverted and the gap stays open (C51).
- The windows a shrink moves wait for the shrinking window to commit its new size, at most 300 ms
  (niri's transaction limit), so a slow app cannot stall the layout and nothing overlaps on screen
  while the app redraws: the window shrinks first, then the neighbours close the gap in step with
  the resize animation.
- Example: windows A (0,0,2,1) and B (2,0,1,1). Shrinking A's right edge by 1 frees (1,0,1,1); B lies
  against it, so B slides to (1,0,1,1). With B at (4,0,1,1) the strip is ocean and nothing moves.

### 3.8 The placement engine

`newm-canvas/src/place.rs` decides where the windows in the way go when windows move, swap, grow
or drop. Inputs: the layout before, the *targets* (moved windows on their new rects), a preferred
direction `along`, and optionally a *seed* layout.

#### 3.8.1 Rules

1. **Least cost.** The chosen layout minimizes
   `cost = tiles + distance_weight * sum(area_k * (|di_k| + |dj_k|)) + diagonal_weight * sum(area_k for windows with di_k != 0 and dj_k != 0)`,
   where `tiles` counts every tile whose owner (a window or nobody) changes, and the sums run over
   the windows making room (the targets are exempt from both weighted terms). Defaults
   `distance-weight 0.5`, `diagonal-weight 1`, each 0 to 100.
2. **No island split (hard).** Two windows other than the movers, connected without them before,
   MUST stay connected after, through any windows.
3. **Rows and columns kept (soft).** A window making room may move diagonally, but rule 1 charges it
   `diagonal_weight * area`, so it keeps its row or column when it can.

Every accepted layout MUST put each target on its rect, keep every other window's span, overlap
nothing and split no island (`place::accepts`, claim C50). The search is trusted for the cost,
never for these checks; a layout failing them is rejected and the caller falls back.

#### 3.8.2 Search

- A window moves only because a placed window lands on it. The next window to place is the first
  window not yet placed that a placed one overlaps.
- Candidates for a window making room lie within `REACH = 2` tiles past its span around where it
  was.
- Branch and bound: a partial layout no better than the best so far is cut. Among equally costly
  layouts, moves along `along` come first.
- The search takes at most `BUDGET = 6000` steps. Past it, the best layout found is used, else the
  caller pushes.
- A seed (the push the caller would fall back to) that passes `accepts` is the layout to beat;
  without a cheaper layout within the budget the seed is returned.
- Data structures (`newm-canvas/src/index.rs`): a static 2D range tree over rects' top-left
  corners (a merge-sort tree: each node covers half its parent's corners in column order, knows
  its column range, keeps corners sorted by row). A query box widened left by the widest rect and
  up by the tallest returns exactly the rects overlapping `q`, in ascending order, in
  O(log^2 n + k). Touching is a query on the ring around a rect. Islands are a flood over the
  touching lists. The tile term of each moved window comes from shared areas:
  `|o| + |n| - 2|o ∩ n|` less what earlier moved windows' old and new rects share with it. The
  search step's candidates come from 2D difference arrays and a prefix sum. Costs are integer
  tallies (tiles, area x distance, diagonal area), combined as floats only at the end.
- The pre-range-tree search is kept as `place::reference` under `cfg(test)`; differential tests
  MUST show both decide the same layout on every run.
- Measured (64 windows on 119x113 tiles): the full benchmark takes 23 ms (4.8 s with
  `place::reference`); the worst single drop takes about 3 ms.

#### 3.8.3 The push (fallback)

The windows other than the mover are swept in order along `dir` (nearest the mover's side first,
ties by id). Each moves along `dir` just far enough to clear the mover, if it overlaps it, and to
stay past every window swept before it whose cross-axis range overlaps its own. The sweep repeats
until no window moves (at most one round more than the windows swept, C24). Properties: nothing
overlaps afterwards (C02); a window moves only when something moved into it; windows keep their
order along `dir` (C14, finding F01).

#### 3.8.4 Which operations use it

Grow, `move_by`, `move_to` (drop), `swap`, `send_to_edge`, and growth from a minimum. Opening a
window and restoring one never use it: they take a free spot and move nothing.

#### 3.8.5 Bridges

When the moved window was a bridge, the island it held together may fall apart. After a drop or a
close, each part of that island but the largest slides, the smallest move first, the least
distance needed to touch the largest part again, stopping at the first window in its way. The
result keeps every span and overlaps nothing, or nothing moves. Getting together outranks rule 1.

#### 3.8.6 Gather islands

`gather-islands` brings every island into one: each island but the largest slides, smallest move
first, the least distance needed to touch the largest, stopping at the first window in its
way; when no straight slide works for any island, a bent move (along one axis, then the other) is
tried; this repeats until one island is left or none can move. Every span is kept and nothing
overlaps, or nothing moves. Undo takes it back.

### 3.9 New-window placement

- A new tiled window opens beside the focused window `f` on the free spot of its span chosen by, in
  order:
  1. the fewest tiles between the spot and `f` (a spot touching `f` only at a corner counts as one
     away);
  2. the least camera scroll needed to show it at the current zoom;
  3. the side: right, below, left, above, then the corners, right ones first;
  4. the spot nearest `f`'s corner; then column, then row.
- The spot in `f`'s rows past every window's right edge is always free, so the search terminates.
- MUST: opening a window moves no other window (C28) and opens it at exactly `open_span` (C29).
- With no focus (an empty canvas) the window opens at the origin.

### 3.10 Open size

A new tiled window's span is decided per axis, first match wins:

1. a window rule: `default-column-width` (width), `default-window-height` (height);
2. the size the app picks for itself: the first configure asks for 0 on every axis no rule sizes,
   and the size the app commits is rounded up to whole tiles at map;
3. the span of this app's most recently focused window on the same canvas, or of its last window
   to close or leave the canvas (each canvas remembers the last spans of 64 apps);
4. `canvas { default-width }` / `canvas { default-height }`;
5. one tile.

A fixed size (logical pixels) or a proportion (of the canvas area at 1:1) becomes the nearest whole
number of tiles, at least one; an empty node (`default-width {}`) keeps one tile. The result is cut to what fits at 1:1 and raised to need. Floating windows
keep niri's sizing. The usage log's `open` line records the source (`size_source`), the committed
size (`app_px`) and the span; a window not at its cell size within 5 s is logged `unsettled`.

The reference configuration opens windows at 960 px wide and 2/3 of the screen height
(`default-width { fixed 960; }`, `default-height { proportion 0.6667; }`).

Example: on 119x113 tiles, a browser committing 1280x760 opens at 11x7 tiles; a second window of
the same browser that picks no size opens at the first one's span.

### 3.11 Moving windows

#### By keyboard

- niri's `move-column-left/right`, `move-window-up/down` move the focused window one tile
  (`move_by`); the windows in the way make room by the rules; the gap behind stays open.
- `swap-neighbour DIR`: the neighbour is the nearest window in `DIR` (nearest gap first, then the
  smallest offset across). Along `DIR` the pair keeps the extent it covered: the focus ends where
  the neighbour ended, the neighbour starts where the focus started; across `DIR` they trade their
  first row or column. Equal spans exchange rects. Example, swap right: focus `a=(0,0,2,1)`,
  neighbour `b=(2,0,1,1)` give `a'=(1,0,2,1)`, `b'=(0,0,1,1)`. MUST NOT resize either window
  (finding F05). Push fallback: the neighbour steps out, the focus lands and pushes along `DIR`,
  the neighbour comes back last and pushes the other way.
- `send-to-edge left|right`: the focused window moves in its row to just past the leftmost
  (rightmost) edge of every other window. Fallback: take it out (filling its lake), measure the
  edge after the take, push (finding F10).
- niri's `move-column-to-first/last` map to `send-to-edge`.

#### By drag

- Mod + primary drag, or a title-bar drag, moves a tiled window. The drag starts after 8 logical
  pixels of motion; until then the window follows the pointer 1:1, so jitter in a Mod+click
  never moves it.
- While the drag stays on its canvas, the window stays on the grid, drawn at the pointer, and the
  others are drawn where the drop would put them: each tile the window snaps to is one drop state,
  worked out on a copy of the canvas from the layout the drag started on, so dragging back puts
  them back (live preview).
- Release drops it on the tiles nearest where it is drawn: one `move_to`, then focus. A drop MUST
  NOT reopen the window at a new window's span (finding F04).
- Escape during the drag cancels it: the others glide back, the window returns home, nothing
  drops. The Escape key and its release reach no app.
- A window leaving its canvas (another output, another workspace in the overview) leaves the
  canvas at that moment (its lake closes then) and carries its span; the canvas it arrives on opens
  it at that span, cut to what fits there and raised to need (claims C48, C49). A span is carried
  through a floating spell too. A carried window MUST NOT take a same-app placeholder.
- Fullscreen and maximized tiles, pending ones included, leave through niri's move to floating.

#### Resize by drag

- Mod + secondary drag moves the nearest edges by the whole tiles the pointer travelled. A box in
  the border's active colour (fill alpha 0.3, 2 px edge) shows where the window will be on release,
  and the windows the resize pushes are drawn where it pushes them, worked out on a copy of the
  canvas from the drag's start layout. The window keeps its size until release, which applies the
  previewed resize with no second glide. Escape cancels; the window keeps its size.
- A layout change mid-drag MUST re-preview the live drag.

#### Resizing by keyboard

- `resize-edge EDGE TILES` moves one edge: positive outward, negative inward; at most 1,024 tiles
  per action.
- niri's `set-column-width` / `set-window-height` set the width or height in whole tiles, moving
  the right or bottom edge; `switch-preset-column-width(-back)` grows (shrinks) the width by one
  tile; `switch-preset-window-height` grows the height by one tile; `reset-window-height` shrinks
  the height to need; `maximize-column` toggles between the widest span fitting at 1:1 and the
  width need allows.

### 3.12 Undo and redo

- Every rearrangement (move, drop, swap, resize, send-to-edge, gather) records the layout before
  it; at most 64 steps. A new rearrangement clears redo.
- A park (6.9) records one step per window it moves. While those are the last steps of the
  history, one undo takes all of them back and one redo does them all again.
- Opening, closing or renaming a window forgets the history.
- Restoring a snapshot MUST be refused, with nothing changed, unless it holds exactly the
  windows on the canvas at positive spans with no two overlapping; a refused undo forgets the
  history.
- After a restore every window is raised to its current need (finding F09), and the focus is shown.
- Undo and redo keep every invariant (C52).

### 3.13 Floating windows

- Floating windows live above the canvas, as in niri, and are the only windows drawn over tiled
  ones. `toggle-window-floating` moves a window between canvas and floating layer.
- With no `open-floating` rule, a window opens:
  - floating when it has a parent (a dialog) or a fixed height (min height = max height > 0);
  - as an *overlay* when it has no parent and caps its size on both axes within the logical size
    of the output it opens on, without a fixed height (picture-in-picture windows, which cap at
    80% of the display): it floats at its own size in the working area's bottom-right corner, one
    gap from the edges, and takes focus only when it asks for activation. A cap larger than the
    output on either axis (Firefox caps at 8102 or 13017 px) limits nothing: that window tiles;
  - tiled otherwise.
- `canvas { floating-follow }` decides what floating windows do when the camera pans or zooms:

| Mode | Behaviour |
|---|---|
| `"screen"` (default) | Stay where they are on screen. |
| `"parent"` | A dialog moves and scales with its tiled parent; other floating windows stay on screen. |
| `"canvas"` | Every floating window moves and scales with the canvas. |

- `cycle-floating-follow` steps screen, parent, canvas, screen; `set-floating-follow MODE` picks
  one. Either holds until the config changes the setting. The current mode is written, by atomic
  rename, to the one-line file `$NIRI_SOCKET.floating-follow`, so a bar can show and switch it.

### 3.14 Focus

- niri's `focus-column-left/right`, `focus-window-up/down` focus the neighbour in that direction
  (3.11's neighbour rule). `focus-column-first/last` focus the leftmost/rightmost window;
  `focus-column N` focuses the window starting the N-th tile column holding one.
- Closing the focused window focuses the most recently used window left (C31), else the nearest.
- `center-column` centres the view on the focus (top-left aligned when it is larger than the
  screen).
- niri actions with no meaning on a grid do nothing: consume/expel, `consume-or-expel-window-*`,
  the tabbed column display.

### 3.15 Proof obligations

`newm-canvas/proof/` holds a Rocq model of every public `newm-canvas` operation, checked by a gate.

- **Model.** `Canvas.v` has one definition per Rust function (including `open_span`, `spot`,
  `spot_near`, `move_by`, `set_focus`, place.rs's `splits`, `is_bridge`, `accepts`, gather's check,
  `gather_islands`, `restore`). Floats are integers in 1/10000 px; `Config::default` is `shipped`.
  The engine's answer, gather's answer and undo's snapshot are free choices: each claim holds for
  every answer, the push fallback (`None`) included.
- **Modules.** `Push.v` (the sweep terminates and leaves the canvas disjoint), `Claims.v` (the
  claims), `Spans.v` (span principle, carried spans), `Order.v` (push keeps order), `Rules.v`
  (the three rules as checks), `Index.v` (range tree answers what a scan answers), `Islands.v`
  (flood, splits, bridge; close and shrink split nothing), `Cost.v` and `Grid.v` (the search's tile
  terms sum to rule 1's tiles reassigned in any placement order; candidate grid exactness),
  `Findings.v` (counterexamples F01-F05, F09, F10 against the old policies, and the repaired
  traces).
- **Claims (68, C01-C71).** Grouped: no overlap and minimum spans after every operation (C01-C03,
  C06, C09, C17); only zoom and config change the zoom (C04, C26); grow fits at 1:1 (C05); ocean
  moves nothing (C07, C08); focus shown and `least` is least (C10-C12, C36); shared physical edges
  (C13); push order and termination (C14, C24, C25, C33); zoom is a camera, keeps the middle,
  round-trips (C16, C38-C41); add and add_at move nothing, open at `open_span`, land on free spots
  (C28, C29, C42-C46); recency is a permutation with the focus first, a close picks the last used
  (C30, C31); `move_by` moves exactly one tile (C32); span principle and carried spans (C47-C49);
  placed layouts keep rule 2 (C50); close and shrink split no island (C51); undo and redo keep
  `check()` (C52); range tree, touching, disjoint, shared area, flood, splits, bridge, search
  checks exact (C53-C70); side order of a new window (C71). C18, C20, C22, C27, C34, C37, C40,
  C46 replay fixed scenarios of `tests/replay.rs` in the model.
- **Findings.** Counterexamples to earlier policies, each repaired and replayed as a regression
  test: F01 a push reorders the windows it lands on; F02 a zoom leaves the focus wider than the
  screen; F03 `set_config` changes the zoom too; F04 a drop reopens a window at a new window's
  span; F05 a swap trades rects, changing spans; F06 a move reassigns more tiles than needed; F07
  a rearrangement splits an island; F08 a drop onto its own tiles moves a neighbour diagonally;
  F09 undo leaves a window below a raised minimum; F10 send-to-edge's fallback lands the focus on
  a window.
- **Out of the model (stated in `proof.json` as uncovered):** floating point beyond the 1/10000 px
  grid, `i32` overflow, the search's control flow and budget (only its checks are proved), undo
  stack bookkeeping, floating windows, rendering, animation, input, and the niri glue in
  `src/layout/canvas/moves.rs` (bound by anchor hash only and tested by layout tests).
- **Anchors and lock.** `proof.json` ties each model definition to its Rust line by anchor;
  `proof.lock.json` records each claim's meaning and each anchored file's hash. A drift in
  either fails the gate until re-recorded with the author's approval.
- **Gate.** Compiles the model, re-checks every claim in the kernel, compares claims and anchors
  with the lock, and runs `replay.sh`: the Rust replay harness on a copy of the crate outside the
  workspace, including the regression tests of the repaired findings.
- **Mutants.** 60 declared source mutants; each MUST be killed by a claim or a test. Survivors are
  either proved equivalent (documented) or become tests.
- Compiling proofs is not enough: a review checks each claim's statement against the code's
  meaning, and the anchors against current code.

### 3.16 Canvas options

`layout { canvas { ... } }`; flags take no argument or `true`/`false`. A value outside an option's
range is an error ("invalid value") unless the table says it clamps.

| Option | Type | Default | Range | Meaning |
|---|---|---|---|---|
| `tile-width` | number | 720 | 1-65535 | Tile width at 1:1, in logical pixels. |
| `tile-height` | number | 465 | 1-65535 | Tile height at 1:1. |
| `zoom-step` | number | 0.8 | 0-1, clamped to 0.05-1 | Scale of one zoom level (4.1). |
| `zoom-min` | integer | -2 | clamped to -16..16 | Closest level; negative levels magnify. |
| `zoom-max` | integer | 4 | clamped to -16..16 | Farthest level; zoom to fit stops here. Swapped with `zoom-min` when smaller. |
| `distance-weight` | number | 0.5 | 0-100 | Rule 1's distance term: higher keeps the windows making room nearer where they were. |
| `diagonal-weight` | number | 1 | 0-100 | Rule 1's diagonal term: higher keeps them in their row or column. |
| `overview-dim` | number | 0.4 | 0-1 | Alpha of the black layer over what lies outside the screen's area in the overview. |
| `pan-acceleration` | number | 1 | 0-1 | Share of pointer acceleration a Mod pan takes: 0 follows the hand, 1 the accelerated cursor. |
| `pan-momentum` | number | 1 | 0-1 | Share of its speed a Mod pan keeps on release: 0 stops, 1 glides on. |
| `default-width` | preset size | unset (one tile) | | Open width when no rule sizes it (3.10). |
| `default-height` | preset size | unset (one tile) | | Open height when no rule sizes it. |
| `super-tap-overview` | flag | off | | A Super tap toggles the overview (4.3). |
| `center-on-switch` | flag | off | | Alt+Tab and KRunner picks centre the window (4.7). |
| `floating-follow` | string | `"screen"` | screen, parent, canvas | 3.13. |
| `restore-session` | flag | on | | Section 6. |
| `restore-exclude` | `app-id="REGEX"`, repeatable | none | | Apps never saved nor relaunched. The regex matches anywhere in the app ID, case-sensitive unless it starts with `(?i)`. |
| `one-workspace` | flag | off | | 4.8. |
| `magnified-input` | flag | off | | Pointer input reaches apps while the camera magnifies (4.1). |

Reference configuration: small fixed tiles (3.2), `zoom-max 7`, `super-tap-overview`,
`center-on-switch`, `one-workspace`, `pan-acceleration 0`, `pan-momentum 0`, 960 px x 2/3 open
size.

## 4. The camera

### 4.1 Zoom is a camera

- The camera draws the canvas at `scale = zoom-step ^ zoom`. Level 0 is 1:1; with step 0.8, level 1
  draws at 0.8, level -1 at 1.25. Positive levels pull back (more tiles on screen); negative levels
  magnify past 1:1 (blurry, allowed by the author).
- MUST: a zoom changes no window's tiles, span, minimum or id, nor the focus or recency (C16), and
  sends no configure to any app.
- `zoom_by(delta)` clamps the level to `[zoom-min, zoom-max]` and is refused only when the
  clamped level does not change.
- Levels are whole numbers. `zoom-in`, `zoom-out`, Mod+wheel and a touchpad pinch (4.6) all move
  the camera by whole levels through `zoom_by`.
- The zoom is about the middle of the screen: the canvas point under the middle stays there.
  `view' = (view + screen / 2) / from_scale * to_scale - screen / 2` per axis (C38). Then the view
  moves the least it must to show the focus, or as much of it as fits (C36).
- During the zoom animation the view follows the animated scale so every frame keeps the middle.
- Pulling back `d` levels and coming closer `d` levels returns the same canvas and view when
  neither end is clamped and the focus was shown without scrolling (C39).
- While zoomed (level != 0), the pointer only picks windows: a hit activates (focuses) the window
  and no pointer event reaches a scaled surface. Keyboard input always reaches the focused app.
- With `magnified-input`, a magnifying camera (level < 0, scale > 1) passes pointer input on: a
  hit on a window is input, and the window-local point it sends is the view point mapped back
  through the camera (`from_view`), so motion, buttons, axis and touch land where the window is
  drawn. A floating window drawn at a magnifying scale (`floating-follow`) does the same.
  Zoomed out (scale < 1) the pointer only picks, as in the overview. During a drag the motion
  keeps the press point's mapping (smithay's click grab holds the focus it started with), so
  the window sees the pointer move by screen pixels until the button is released.
- Rendering and hit testing skip tiles outside the area the view shows through the camera.
- Fullscreen and maximized windows are not on the grid and are drawn without the camera.

### 4.2 Zoom reset

`zoom-reset` returns the camera to 1:1 as a zoom by the levels it is away, keeping the middle. It
drops any level saved by zoom-to-fit, so a later Mod release stays at 1:1. Reference bind: Super+0.

### 4.3 Overview

- niri's overview shows every window of the output: windows outside the active canvas's view MUST
  be drawn too.
- Everything outside the area the screen shows (the active workspace's rect: windows beyond it,
  other workspaces, the backdrop) is dimmed by a black layer of alpha `overview-dim` (0.4). niri's
  translucent workspace box is not drawn. Top and overlay layer surfaces stay undimmed.
- The overview opens and closes by: a Super tap with `super-tap-overview` (Super pressed and
  released within 300 ms with no other key or button since, and no Ctrl, Alt or Shift held when it
  went down); niri's hot
  corner; niri's `toggle-overview` action (niri's default config binds a Mod release).
- In the overview, canvas swipes and Mod pans are off; niri's own gestures apply.

### 4.4 Zoom to fit while Mod is held

Mod plus a four-finger touchpad swipe pulls the camera back the fewest levels, up to `zoom-max`,
that fit every window on screen (the first call saves the current level; later calls keep the
canvas fitting as it grows). The fingers move the focused window one tile per step, pushing what
is in the way. Releasing Mod returns the camera to the saved level, showing the focused window.

### 4.5 Panning

- **Mod and the mouse.** Pointer motion with Mod held, no button and no grab pans the camera 1:1,
  as if the content were held. The pan starts once the motion since Mod settled passes 8 logical
  pixels, with all of that motion, and cancels the Super tap. A motion that pans focuses no window.
- **Settle.** Motion in the first 100 ms after Mod goes down counts toward no pan, and the 8 px
  count starts from where the pointer is when the 100 ms end. The hand settling as it presses Mod
  (a Mod key on a mouse thumb button above all) leaves the camera where it is, so a Mod+secondary
  press in that time starts a resize and a Mod+primary press a move on the window under the
  pointer. Mod is the configured mod key (Super, Alt when nested, or `input { mod-key }`), told
  by its keysym. The settle applies only to a pan's start: a pan under way goes on 1:1.
- **Acceleration.** Per event, `pan = unaccelerated + pan_acceleration * (accelerated - unaccelerated)`.
- **Momentum.** On Mod release the view keeps `pan_momentum` of its velocity (touchpad pans keep
  all of it) and decelerates at the touchpad swipe tracker's rate (0.997 per ms) from the release
  point; then the window nearest the middle of the screen takes the focus and is shown.
- Mod pans do nothing while locked, in the overview, or during a drag.
- A touchpad pan (niri's view gesture where no canvas swipe claims the fingers) moves the view by
  the working area's width per 1,200 units of finger motion.

### 4.6 Touchpad gestures

| Gesture | Action |
|---|---|
| Three fingers, any direction | Focus moves one window, as one press of Super+H/J/K/L: once per swipe. |
| Four fingers, sideways | Focus moves one window left or right, once per swipe. |
| Four fingers, up or down | Zoom: one level per 100 units, up pulling the camera back (`zoom-out`), down bringing it closer. |
| Mod + three fingers | Pans the camera after the fingers on both axes, with the touchpad's momentum on release. |
| Mod + four fingers | Zoom to fit (4.4) and move the focused window one tile per step, pushing. |
| Three or more fingers, pinching | Zoom: the camera follows the fingers in whole levels. |
| Mod + a pinch, any finger count | The same zoom. |
| Two fingers, pinching, no Mod | Reaches the app under the pointer (browsers and image viewers zoom with it). |

- A swipe takes the axis it went furthest along once its motion passes 16 units (GNOME's
  threshold), with all of that motion.
- One step is the motion a pan takes to scroll one tile, never less than 100 units (50 for Mod +
  four fingers, on the zoomed-out canvas); the remainder carries over.
- Directions follow the fingers, whatever the natural-scroll setting.
- A focus swipe is done after its first step: more motion, or motion back, moves nothing.
- Nothing steps while locked; a swipe begun with Mod does nothing more once Mod is up.
- A pinch that zooms takes the level `start + round(ln(scale) / ln(zoom-step))`, clamped to
  `[zoom-min, zoom-max]`: `start` is the level when the pinch began and `scale` the fingers'
  spread relative to theirs then (libinput's pinch scale). Pinching in (`scale` below 1) pulls
  the camera back; spreading brings it closer. Each change of level is one `zoom_by`, about the
  middle of the screen, with the camera animation.
- The finger count and Mod are read when the pinch begins. No app receives a pinch that zooms,
  and releasing it keeps the level it reached.
- Pinches are off in the overview and zoom nothing while locked.

### 4.7 Focus moves the camera the least

- `least(view, start, len, screen)` per axis: if `len > screen` or `start < view` then `start`;
  else if `start + len > view + screen` then `start + len - screen`; else `view`.
- MUST: a focus change, open, move or resize moves the view by `least` only (C11, C12): a window
  already on screen gets focus with no camera motion; a larger-than-screen window aligns to its
  top-left.
- With `center-on-switch`, confirming a window in the Alt+Tab switcher (niri's recent-windows
  switcher: windows, not apps, with live thumbnails) focuses it and centres it. KRunner window
  picks do the same.

### 4.8 One workspace per output

With `one-workspace`, every output keeps exactly one workspace and one canvas. Actions that only
switch to, move to, reorder or name a workspace do nothing; the window halves of
`...OrWorkspace...` actions still act; named workspaces and `open-on-workspace` rules are ignored;
turning it on gathers each output's other workspaces onto its active one.

## 5. Input model

### 5.1 Pointer

| Input | Effect |
|---|---|
| Motion, no button, no Mod | Nothing: focus, layout and camera stay (principle 5). |
| Click | Focuses the window under the pointer (only picks while zoomed). |
| Mod + primary drag, title-bar drag | Move with live preview (3.11). |
| Mod + secondary drag | Resize with preview (3.11). |
| Escape during either drag | Cancel. |
| Mod + motion, no button | Pan, past 8 px after Mod's first 100 ms (4.5). |
| Middle-button drag | Scrolls (libinput `scroll-method "on-button-down"`, `scroll-button 274`, set in the input config). |

`input { focus-follows-mouse }` is niri's opt-in and stays off by default.

With the reference binds (5.3), Mod+wheel zooms one level per notch: wheel up comes closer, wheel
down pulls back. Wheel binds take only a mouse wheel: two-finger touchpad scrolling with Mod held
reaches the app under the pointer, unless a `TouchpadScroll*` bind takes it.

### 5.2 Keyboard parity: canvas actions

Every action but `launch-placeholder` works as a bind and over IPC (`newm-rs msg action ...`).

| Action | Arguments | Effect | Mouse or touchpad |
|---|---|---|---|
| `resize-edge` | `left`/`right`/`top`/`bottom`, signed tiles | Moves one edge of the focused tiled window: positive outward, negative inward (3.11). | Mod + secondary drag |
| `zoom-in`, `zoom-out` | | One level closer, one level back (4.1). | Mod+wheel with the reference binds; a three-finger pinch; four fingers down or up |
| `zoom-reset` | | Back to 1:1 (4.2). | (none) |
| `undo`, `redo` | | The last rearrangement taken back or done again (3.12). | Escape cancels a drag in progress |
| `gather-islands` | | Every island joins the largest (3.8.6). | (none) |
| `send-to-edge` | `left`/`right` | The focused tiled window moves in its row past every other window (3.11). | A long drag |
| `swap-neighbour` | `left`/`right`/`up`/`down` | The focus and its neighbour trade places, each keeping its span (3.11). | (none) |
| `quit-app` | | Closes every window with the focused window's PID and app ID. | (none) |
| `cycle-floating-follow` | | Next floating-follow mode: screen, parent, canvas, screen (3.13). | (none) |
| `set-floating-follow` | `screen`/`parent`/`canvas` | That mode, until the config changes the setting (3.13). | (none) |
| `toggle-synth-background` | | Starts or stops the synth background's motion; needs the `synth` feature, not `canvas` ([configuration.md](configuration.md#synth-background-)). | (none) |
| `set-synth-background` | `off`/`still`/`animated` | That mode, until the config changes the setting; needs `synth`. | (none) |
| `launch-placeholder` | `--app-id ID [--icon PATH]` | IPC only: a placeholder where the app's next window opens (6.6). | (none) |
| niri `move-column-*`, `move-window-*` | | One tile, pushing (3.11). | Mod + primary drag |
| niri `focus-*` | | 3.14. | Click |

### 5.3 Reference bind set

The shipped default config is niri's, unchanged; canvas actions have no default keys, so nothing
changes for a niri user who enables the feature. The author's session uses this set (spawn
targets generic):

| Keys | Action |
|---|---|
| Super+H/J/K/L, Super+arrows, Super+W/A/S/D | Focus left/down/up/right |
| Super+Shift+H/J/K/L, Super+Shift+arrows | Move the window one tile, pushing |
| Ctrl+Super+H/J/K/L | Swap with the neighbour |
| Super+Shift+Q | `resize-edge "left" 1` (grow to the left) |
| Super+Shift+E | `resize-edge "right" -1` (shrink from the right) |
| Super+Shift+S | `resize-edge "top" -1` (shrink from the top) |
| Super+Shift+X | `resize-edge "bottom" 1` (grow down) |
| Super+Shift+A / Super+Shift+D | Send to the far left / far right |
| Super+U, Super+Minus, Super+KP_Subtract, Super+wheel down | Zoom out |
| Super+N, Super+Plus, Super+Equal, Super+KP_Add, Super+wheel up | Zoom in |
| Super+0 | Zoom reset |
| Super+Z / Super+Shift+Z | Undo / redo |
| Super+I | Gather islands |
| Super tap | Overview |
| Alt+Tab | Window switcher, centring the pick |
| Alt+Q | Close the window |
| Super+Q | Quit the app (all its windows) |
| Super+F | Fullscreen |
| Super+T | Float / tile |
| Super+Return, Ctrl+Alt+T | Terminal |
| Super+Space | Launcher |
| Alt+Space | KRunner |
| Super+E | File manager |
| Super+Period | Emoji picker |
| Super+V | Clipboard history |
| Print, Super+Shift+Print | Screenshot; region screenshot |
| Super+R, Super+Shift+R | Region recording |
| Super+Escape, Ctrl+Alt+L | Lock (`loginctl lock-session`) |
| Ctrl+Super+Escape | Kill a window picked with the pointer (a script over niri's `pick-window` request) |
| Super+Shift+P | Log out (`quit` without confirmation) |

### 5.4 niri's default binds on the canvas

| Default bind | niri action | Canvas meaning |
|---|---|---|
| Mod+arrows, Mod+H/J/K/L | `focus-column-left` and the like | Focus the neighbour. |
| Mod+Ctrl+arrows, Mod+Ctrl+H/J/K/L | `move-column-left` and the like | Move one tile, pushing. |
| Mod+Home/End | `focus-column-first`/`-last` | Focus leftmost/rightmost. |
| Mod+Ctrl+Home/End | `move-column-to-first`/`-last` | Send to edge. |
| Mod+R, Mod+Shift+R | `switch-preset-column-width`, `-back` | Width +1 / -1 tile. |
| Mod+Ctrl+Shift+R | `switch-preset-window-height` | Height +1 tile. |
| Mod+Ctrl+R | `reset-window-height` | Height to need. |
| Mod+Minus/Equal, Mod+Shift+Minus/Equal | `set-column-width`, `set-window-height` | Width / height in whole tiles. |
| Mod+F | `maximize-column` | Widest span fitting / width need. |
| Mod+Shift+F | `fullscreen-window` | Fullscreen. |
| Mod+C | `center-column` | Centre the view on the focus. |
| Mod+V | `toggle-window-floating` | Floating toggle. |
| Mod+Q, Alt+F4 | `close-window` | Close the window. |
| Mod (press and release) | `toggle-overview` | Overview. |
| Alt+Tab, Mod+Tab | the recent-windows switcher | Switcher with live thumbnails (4.7). |
| Print | `screenshot` | niri's screenshot UI. |
| Mod+BracketLeft/Right, Mod+Comma/Period, Mod+W | consume, expel, tabbed display | Nothing (no columns). |

Workspace and monitor binds work as in niri, except with `one-workspace` (4.8).

## 6. Session

All of section 6 exists only with the `canvas` feature; the usage log (6.7) also needs `telemetry`.

### 6.1 Saving

- File: `$XDG_STATE_HOME/newm-rs/session.json` (default `~/.local/state/newm-rs/session.json`).
  MUST be mode 0600 in a directory created 0700 (titles and command lines are private).
- Written atomically: temporary file, fsync, rename.
- Checked every 2 s; a change is saved once it has held for one check, so a burst makes one write.
- A dedicated writer thread does the I/O; a busy writer defers the write to the next check. The
  main loop MUST NOT write or fsync the file.
- The `quit` action and the exit dialog save one last time, waiting up to 3 s for the writer.
- A stop by SIGINT, SIGTERM or SIGHUP MUST NOT save: a teardown closing windows must not overwrite
  the last settled session.
- Nothing is saved until the previous session has been put back.
- Only the session on a TTY saves and restores; a nested newm-rs does neither.
- `restore-session false` turns saving and restoring off.

### 6.2 Contents

Per canvas: output connector name, workspace index, zoom, view, focused window. Per tiled window:
app ID, title, tile rect, launch index. Per floating window: app ID, title, position, size. Per
launch: command line and working directory, read from `/proc` at save time. Processes of
xwayland-satellite and Xwayland are skipped (their command lines bring back nothing).

### 6.3 Never saved

- dialogs (windows with a parent) and windows without an app ID;
- app IDs containing, ignoring case: `polkit`, `pinentry`, `askpass`, `xdg-desktop-portal`,
  `org.freedesktop.impl.portal`, `kwallet`, `ksecretd`, `org.kde.kded`, `swaylock`, `gtklock`,
  `hyprlock`, `newm-rs-lock`;
- apps matching a `restore-exclude` regex (also never relaunched).

### 6.4 Restoring

At every start, on a TTY:

1. `spawn-at-startup` commands run first; launches skip programs they already started.
2. Wait for an output: up to 20 tries of 500 ms.
3. Each canvas goes to the output with the same connector name, else the active one, and the same
   workspace index, else the active one.
4. A placeholder is drawn on every saved tile.
5. Each saved process launches once per process and command, its command found in this order: the
   desktop file of its app (6.10) under `$XDG_DATA_HOME/applications` and each
   `$XDG_DATA_DIRS/applications`; the saved command line and working directory.
6. A mapping window takes a placeholder of its app (6.10) through `add_at`. Among several, the title
   score picks: `score(saved, now) = 1` when equal, else
   `0.99 * |words(saved) ∩ words(now)| / |words(saved) ∪ words(now)|` over lowercase alphanumeric
   words. At or above 0.5 the title decides, below it the app alone.
7. For 90 s after the restore starts, a window whose title changes may trade spots with a
   placeholder or a restored window of the same app when the trade improves the summed title
   scores by more than 0.25 (titles arrive late: a terminal once tmux or ssh attaches, a browser
   once its tab loads). A trade moves no other window.
8. When no placeholder is left, or after 20 s, each canvas takes its saved focus, zoom and view
   (moved the least to show the focus). Unclaimed placeholders vanish (logged
   `restore-expired`); later windows open normally.

Apps needed only by floating windows are not relaunched; saved floating positions are not applied.

### 6.5 Terminals

- For the windows of a known terminal (by app ID or X11 class, in the table below), newm-rs finds
  each window's running tmux client or ssh command on a worker thread from the process tree under
  the terminal's PID. A window whose process is not its terminal's (xterm under XWayland comes
  from xwayland-satellite) has its terminal's processes found by name. The result is keyed by
  window, not by title.
- A window whose process is its terminal's and serves no other window takes the tmux client or
  ssh under that process, whatever its title. Titles tie the rest: windows that share one
  process (Ghostty, gnome-terminal), windows whose processes are found by name, and the remote
  tmux session behind an ssh window. Konsole's ` — Konsole` title suffix is dropped first.
- By title, each window takes one client with its title: a restored ssh window takes a client on
  the session its command attaches; the other windows, oldest first, pair with the other clients
  in `list-clients` order (oldest first). Two windows share a session only when two clients are
  on it; a window left without a client keeps the plain relaunch.
- tmux clients come from
  `tmux list-clients -F '#{client_pid}\t#{session_name}\t#{T:set-titles-string}'`; matching by
  title needs tmux's `set-titles` on. The title-directory stat runs off the main loop (it may hit
  a network mount).
- A remote tmux session is asked through an ssh ControlMaster connection already open:
  `-o BatchMode=yes -o ProxyCommand=false -T`, only when `ssh -G` says ControlMaster is on and the
  control socket exists. newm-rs MUST NOT open a new ssh connection, prompt, or trigger a security
  key while querying. Answers are cached 30 s and re-asked at most every 60 s; queries time out
  after 3 s. Without an open master, a window reruns the ssh under its own process; windows of
  one process match their ssh by the host the title names (alias or hostname, then elimination
  of lone pairs).
- Relaunch: the window's terminal with its own launch form, from `$HOME`, running
  `sh -c '<cmd>; exec "${SHELL:-sh}"'` (`CMD` below); with no `<cmd>` found, a plain window in the
  directory the title names, else `$HOME` (`DIR`):

  | Terminal | App ID or X11 class | Launch form |
  |---|---|---|
  | Ghostty | `com.mitchellh.ghostty` | `ghostty +new-window --working-directory=DIR [-e CMD]` |
  | Alacritty | `Alacritty` | `alacritty --working-directory DIR [-e CMD]` |
  | kitty | `kitty` | `kitty [--single-instance [--instance-group=G]] --directory=DIR [CMD]`, the instance flags kept when the saved kitty ran with them |
  | foot, footclient | `foot`, `footclient` | `foot --working-directory=DIR [CMD]`, `footclient` likewise |
  | Konsole | `org.kde.konsole` | `konsole --workdir DIR [-e CMD]` |
  | WezTerm | `org.wezfurlong.wezterm` | `wezterm start --cwd DIR [-- CMD]` |
  | GNOME Terminal | `org.gnome.Terminal` | `gnome-terminal --working-directory=DIR [-- CMD]` |
  | Xfce Terminal | `xfce4-terminal` | `xfce4-terminal --working-directory=DIR [-x CMD]` |
  | xterm | `XTerm` | `cd DIR && xterm [-e CMD]` (no directory flag) |

  Any other app ID relaunches from its desktop entry or command line, without reattaching.


  `<cmd>` is:
  - local tmux: `tmux new-session -A -s <session>`;
  - ssh with a known remote tmux session whose name holds only letters, digits, `_` and `-`:
    `ssh -t <args> tmux new-session -A -s <session>`, else the original ssh command line;
  - before ssh, `exec 9>"$XDG_RUNTIME_DIR/newm-rs-ssh-<hash>.lock"; flock 9; ssh -O check <conn> ||
    ssh -fN <conn>; flock -u 9`: windows to the same host serialize on one lock, the first opens
    the master, the rest reuse it, so a restore costs one authentication (one key touch) per host.

### 6.6 Browsers and placeholders

- A bare launch of `chromium`, `chromium-browser`, `google-chrome` (stable, beta, unstable),
  `brave`, `brave-browser`, `microsoft-edge`, `microsoft-edge-stable`, `vivaldi` or
  `vivaldi-stable` gets `--restore-last-session` at restore time. A launch with a URL, `--app`, or
  the flag already present is left alone. Firefox restores its own session.
- `launch-placeholder --app-id ID [--icon PATH]` (IPC) reserves the spot and span the next window
  of that app would take (window rules matching only the app ID, else the canvas default) and
  draws a placeholder tile there, with the icon when given, until a window of that app maps there
  or 10 s pass. A launcher calls it right before starting the app.

### 6.7 The usage log

- Only a build with the `telemetry` feature writes it; the flake's package and a plain cargo build
  start no writer thread and time no operation. No config option turns it off.
- File `$XDG_STATE_HOME/newm-rs/usage.jsonl`, one JSON object per line, local only: newm-rs has no
  network code. Records apps (by their one name, 6.10) and tiles, never titles.
- Lines: each canvas operation (`open`, `close`, `move`, `resize`, `swap`, `drop`, `gather`,
  `send-to-edge`, `undo`, `redo`, `cancel`, `app-grow`) with time, sequence number `seq`, app ID,
  every window's tiles before and after, the rule cost, camera view before and after (1:1 and
  zoomed), camera jump, per-window travel, `op_us`, `during_drag`; `open` adds `size_source`,
  `app_px`, span, focus taken, time since last open, restore match; plus `settle`/`unsettled`,
  `float` (how a new window was classified), `restore-expired`.
- Motion episodes: from an operation's animations, a pan, or a drag preview, each frame samples the
  camera's view point and scale and every animating window; the episode ends when all settle (or
  `cut` by the next operation) and writes one `{"op":"motion"}` line with path, speed,
  acceleration and the largest per-frame jump, keyed by `seq`. Nothing is written per frame;
  movement under 0.5 px does not count.
- One writer thread fed by a bounded channel of 4,096 lines. A full channel drops lines and the
  next line carries the `dropped` count; the compositor MUST NOT wait on the file.
- At 64 MiB the file rotates to `usage.jsonl.1` (replacing it): at most two files.

### 6.8 Logout and the session unit

- Logging out goes through IPC: `newm-rs msg action quit --skip-confirmation` (a bar's Log out
  button runs it). This saves the session (6.1) and exits.
- `newm-rs-session` traps HUP and TERM and stops `newm-rs.service`, so a display manager ending
  the session never leaves the compositor running outside it.
- `X-RestartIfChanged=false` on the unit (the reference packaging's; the flake package's unit lacks
  it): a system switch never restarts the running session; the new binary starts at the next login.

### 6.9 Blocks over IPC

A client that brings a set of windows over from another host places them as one block
([IPC](ipc.md#blocks)).
- `NewmPlaceBlock` takes each window's command, directory, variables, app ID and tiles relative
  to the block, an anchor, a focus slot and `fly`. It reserves all the block's tiles at once on the
  active workspace's canvas, with placeholders as a restore's (6.4), so no other window takes them,
  then launches every command: in its directory when that exists, by its program's name when its
  path is gone, with the variables on top of the compositor's environment.
- Anchors: `Nearest` is the origin nearest the view's middle (by the block's middle) where the
  block stays one tile clear of every window; the candidates are the centred origin and each
  column and row one tile past a window's side, and of equals the rightmost, then the lowest wins.
  `Camera` centres the block on the view (halves round to even). `Tiles` puts the origin on the
  tile given, or places nothing and answers an error when a window holds one of the block's tiles.
  At `Nearest` and `Camera` a slot whose tiles are held goes to the free spot nearest them.
- Each new window takes the placeholder of its app (6.10); of several of one app, the first placed.
  A placeholder no window takes within the restore lifetime (20 s) is released, and its tiles are
  free again.
- Without `fly` the camera and the focus stay. With `fly` the camera moves the least it must to
  show the block, and the focus slot's placeholder takes the focus, which the window that takes it
  keeps. No other window of the block takes the focus as it maps. Once every slot has a window or
  was released, the focus slot's window takes the focus.
- `NewmBlockStatus` answers, per slot, the id of the window that took it (none while it waits,
  once released, or after that window closed) and whether every slot has a window or was released.
  A canvas answers for its 64 most recent blocks.
- `NewmParkWindows` moves every window on the active canvas's grid by the same columns, keeping
  their arrangement, until their rightmost edge is one tile left of the view and of where they
  were. Each window moves by a move that pushes and gathers nothing (no bridge first, 3.8.5). The
  focus goes to the parked window nearest the view, the rightmost (of equals, the one nearest the
  view's middle row), and the camera moves the least it must to show it: at the view's left edge,
  with the rest of the view free. One undo takes the whole park back and one redo does it again
  (3.12).

### 6.10 App identity

An app's windows give different app IDs by toolkit and host: an X11 class differs in case from
the Wayland app ID (`Chromium-browser`, `chromium-browser`), and a toolkit may give a short name
where the desktop file has a reverse-DNS one (`dolphin`, `org.kde.dolphin`). Session restore,
blocks, the span a new window takes from its app's windows and the usage log compare apps by one
name, made from the app ID:

1. lowercased, with a `.desktop` suffix dropped;
2. replaced by the ID of the desktop file that names it (lowercased), searched in
   `$XDG_DATA_HOME/applications` and each `$XDG_DATA_DIRS/applications`. A desktop file names an
   app ID, in this order: by its own ID; by its `StartupWMClass`, in a file shown in menus; by
   the program its `Exec` runs or the last part of its reverse-DNS ID, in a file shown in menus;
   by its `StartupWMClass`, in a hidden file (`NoDisplay` or `Hidden`). An earlier way wins; a
   name two files claim the same way names neither, nor any file a later way.

newm-rs reads the desktop files at start, and again on an app ID they do not name once they are
10 s old. Window rules and `restore-exclude` do not use this name: their regular expressions match
the app ID as the window gives it.

## 7. Lock

- Locking uses `ext-session-lock-v1`: the compositor shows only the locker's surfaces and sends it
  all input. Locking requests come from `loginctl lock-session`, ScreenSaver `Lock`/`SetActive`
  (section 8), or binds.
- The locker is `newm-rs-lock`, a separate Rust program: one lock surface per output showing the
  wallpaper (scaled to cover, centre-cropped) and a centred password box (300x44, scaled by the
  output scale, shrunk on narrow outputs) where each typed character is a dot (10 px dots, 8 px
  apart, capped at the box width) with a cursor. Nothing else is shown: no clock, no notifications,
  no window contents.
- Options: `-i|--wallpaper PATH`, `--bg`, `--fg`, `--accent`, `--error` (`#RRGGBB`; defaults
  `#0b0f14`, `#d8dee9`, `#7ef0a0`, `#e06c75`), `-f|--daemonize` (keys are ignored until the fork).
- Keys: printable text adds characters; Backspace removes one; Escape clears; Enter on an empty
  field does nothing; input is ignored while a check runs. The frame stays quiet over the
  picture: a 1 px line, `fg` three quarters toward `bg` while empty, `accent` 60% toward `bg` while
  typing and 80% while checking. A wrong password draws it 2 px in `error`. The cursor keeps
  `accent` while idle.
- Passwords are checked off the Wayland thread through PAM service `newm-lock`
  (`pam_authenticate` then `pam_acct_mgmt` for the current user). Password only: no fingerprint
  or security-key waits.
- The password buffer never reallocates and is wiped when shortened or dropped.
- The locker sets logind's `LockedHint` on its session while locked.
- MUST: the locker never exits while locked except after a successful unlock; if it crashes the
  session stays locked (ext-session-lock semantics, and niri keeps the lock).
- MUST: while locked, ScreenShot2 window captures are refused (screen captures show only the lock
  surfaces), kglobalaccel app
  shortcuts never fire, canvas swipes step nothing, and Mod pans do nothing.

## 8. KDE and desktop integration

### 8.1 Environment

| Variable | Set by | Value |
|---|---|---|
| `XDG_CURRENT_DESKTOP` | newm-rs at start | Kept when its first entry is `niri` (e.g. `niri:KDE` from `DesktopNames=niri;KDE`), else `niri`; exported to the systemd user manager and D-Bus activation. |
| `XDG_SESSION_TYPE` | newm-rs at start | `wayland` |
| `QT_QPA_PLATFORMTHEME` | login environment; newm-rs does not set it | `kde` (Breeze, KDE file dialogs, KDE settings) |
| `KDE_FULL_SESSION` | login environment, if an app needs it | `true`; newm-rs does not set it |
| Chromium/Electron Wayland hint | login environment; newm-rs does not set it | Ask Chromium-based apps for Wayland. |

Why `niri:KDE`: xdg-desktop-portal reads the first entry and picks `niri-portals.conf`, while
Chromium and Qt look for `KDE` anywhere in the list to pick KWallet and the KDE platform theme.
The config's `environment { }` block reaches only programs newm-rs starts; `newm-rs-session` imports
the login environment into systemd and D-Bus.

### 8.2 Portals

- Shipped `resources/niri-portals.conf`: `default=gnome;gtk;`, Access and Notification `gtk`, Secret
  `oo7-portal;gnome-keyring;` (niri's).
- KDE routing (tested by the compat suite): `default=kde;gtk;`, ScreenCast and Screenshot `gnome`,
  RemoteDesktop `kde`; the reference session also routes Notification to `gtk` and Secret to
  KWallet.
- xdg-desktop-portal-kde creates its RemoteDesktop, ScreenCast, Screenshot, GlobalShortcuts,
  InputCapture, Clipboard and Wallpaper portals only when `XDG_CURRENT_DESKTOP` is exactly `KDE`.
  Its user service MUST get a drop-in setting `Environment=XDG_CURRENT_DESKTOP=KDE`
  (`plasma-xdg-desktop-portal-kde.service.d/`); only that service sees `KDE`.

### 8.3 D-Bus services

Taken when running as a session (`--session`); a name owned by another program is taken back when
that program leaves.

| Bus name, path | Behaviour |
|---|---|
| `org.kde.KWin.ScreenShot2`, `/org/kde/KWin/ScreenShot2` | Version 4: `CaptureScreen`, `CaptureActiveScreen`, `CaptureActiveWindow`, `CaptureWindow`, `CaptureArea`, `CaptureWorkspace`, `CaptureInteractive` (no picking step: the window or screen under the pointer). `include-cursor` works. Window captures are refused while locked. Image sizes and areas are overflow-checked. |
| `org.kde.KWin`, `/KWin` | `queryWindowInfo` (click a window; Escape cancels; a click on empty canvas answers with the output) and `getWindowInfo(uuid)`: caption, app ID as `resourceClass`/`resourceName`/`desktopFile`, geometry. |
| `org.kde.KWin`, `/WindowsRunner` (`org.kde.krunner1`) | KRunner lists open windows matching a query of two or more letters by title or app ID; `window` or `windows` lists all. Running a match focuses the window (centred with `center-on-switch`). |
| `org.kde.kglobalaccel`, `/kglobalaccel` | KDE apps register global shortcuts. Config binds always win; a registration on a config-bound key is refused; app shortcuts never fire while locked. Changes made through the interface (KDE's Shortcuts page) persist in `$XDG_STATE_HOME/newm-rs/kglobalshortcuts.json` (0600). |
| `org.freedesktop.ScreenSaver`, `/org/freedesktop/ScreenSaver` and `/ScreenSaver` | `Inhibit`/`UnInhibit` hold off idle (a hold ends when its owner leaves the bus); `Lock` and `SetActive(true)` ask logind to lock; `GetActive` reports the lock; `GetActiveTime`, `GetSessionIdleTime`, `SimulateUserActivity` work; `Throttle` is a no-op. |
| `org.kde.Solid.PowerManagement` and `.PolicyAgent`, `/org/kde/Solid/PowerManagement/PolicyAgent` | PowerDevil's inhibition API for KDE's Inhibit portal: `AddInhibition`, `ReleaseInhibition`, `ListInhibitions`, `HasInhibition`; session or screen inhibitions hold off idle. |

Launch keys: at start, newm-rs reads desktop files from `$XDG_DATA_HOME/kglobalaccel` and each
`$XDG_DATA_DIRS/kglobalaccel` (first file of a name wins). `X-KDE-Shortcuts` in `[Desktop Entry]`
launches the app; in `[Desktop Action NAME]` it runs that action. A key changed in the Shortcuts
page replaces the file's key, which stays the default. `DBusActivatable=true` apps are activated
over `org.freedesktop.Application`; others run their `Exec`.

### 8.4 Wayland protocols for KDE apps

- `zkde_screencast_unstable_v1` v5: output, window and region streams on niri's PipeWire casts;
  pointer hidden, embedded or as metadata; no virtual outputs. Buffers carry real presentation
  timestamps: without them KPipeWire drops every frame after the first.
- `org_kde_kwin_fake_input` v6: offered only to xdg-desktop-portal-kde, checked by the client's
  executable name read through its PID (a leading `.` and a `-wrapped` suffix stripped); no other
  client sees the global. KDE Connect's remote input reaches the compositor through the
  RemoteDesktop portal.
- `org_kde_kwin_server_decoration_manager` when the config sets `prefer-no-csd`.

### 8.5 What does not work

- System Settings' Display page, `kscreen-doctor`, the display OSD, external-monitor brightness: no
  `kde_output_device_registry_v2`/`kde_output_management_v2`. Outputs are configured in KDL or with
  wlr-output-management tools such as `wlr-randr` and kanshi.
- Plasma's shell: no `org_kde_plasma_shell`, window management or virtual desktop protocols;
  plasmashell does not run; KRunner opens as an ordinary window.
- Launch feedback, global menu, screen edges, lock-screen overlays, KWin shadows and decoration
  palettes, `text-input-v2` (Qt 5 input methods; Qt 6 uses v3).
- KWin D-Bus extras beyond 8.3 (virtual desktops, OSD).

## 9. Protocols

### 9.1 Served

Measured with `wayland-info` by the compat suite's `inventory` case, plus the globals added after
it:

- Core and shell: `wl_compositor` 6, `wl_subcompositor`, `wl_shm` 2, `wl_output` 4, `wl_seat` 9,
  `wl_data_device_manager` 3, `xdg_wm_base` 7, `zxdg_decoration_manager_v1`, `xdg_activation_v1`,
  `zxdg_exporter_v2`/`zxdg_importer_v2`, `zxdg_output_manager_v1` 3, `xdg_toplevel_tag_manager_v1`,
  `wp_viewporter`, `wp_fractional_scale_manager_v1`, `wp_presentation` 2,
  `wp_cursor_shape_manager_v1` 2, `zwp_linux_dmabuf_v1` 6, `wp_drm_lease_device_v1` (per DRM
  device), `wp_security_context_manager_v1`.
- Input: `zwp_pointer_constraints_v1`, `zwp_pointer_gestures_v1` 3, `zwp_relative_pointer_manager_v1`,
  `zwp_tablet_manager_v2`, `zwp_text_input_manager_v3`, `zwp_input_method_manager_v2`,
  `zwp_virtual_keyboard_manager_v1`, `zwlr_virtual_pointer_manager_v1` 2,
  `zwp_keyboard_shortcuts_inhibit_manager_v1`.
- Shells and tools: `zwlr_layer_shell_v1` 5, `ext_session_lock_manager_v1`, `ext_idle_notifier_v1` 2,
  `zwp_idle_inhibit_manager_v1`, `zwlr_foreign_toplevel_manager_v1` 3, `ext_foreign_toplevel_list_v1`,
  `ext_workspace_manager_v1`, `zwlr_output_manager_v1` 4, `zwlr_gamma_control_manager_v1`,
  `zwlr_data_control_manager_v1` 2, `ext_data_control_manager_v1`,
  `zwp_primary_selection_device_manager_v1`, `ext_background_effect_manager_v1`, `mutter_x11_interop`.
- Capture: `zwlr_screencopy_manager_v1` 3, `ext_image_copy_capture_manager_v1` with output and
  foreign-toplevel capture sources.
- KDE: `zkde_screencast_unstable_v1` 5, `org_kde_kwin_server_decoration_manager`,
  `org_kde_kwin_fake_input` 6 (restricted).

### 9.2 Added or verified by newm-rs

| Protocol | Status |
|---|---|
| `zwlr_output_power_manager_v1` | Added; `wlopm --off '*'` turns outputs off (compat `output-power-wlopm`). |
| `xdg_system_bell_v1` | Added; GTK 4 and terminal bells reach the compositor (`system-bell-gtk4`). |
| `xdg_wm_dialog_v1` | Added; modal dialogs are marked (`modal-dialog-gtk4`). |
| `wp_linux_drm_syncobj_manager_v1` | Added (explicit sync) when the render GPU supports it, for every client but xwayland-satellite: X11 windows use implicit sync, since Xwayland on explicit sync shows GPU-rendered X11 windows half-drawn. |
| `org_kde_kwin_fake_input` v6 | Added, for xdg-desktop-portal-kde only. |
| `zwp_keyboard_shortcuts_inhibit_manager_v1` | From niri; VM viewers and remote desktops receive Super and Alt+Tab (`shortcuts-inhibit-gtk4`, `keyboard-shortcuts-inhibit`). |

### 9.3 Security context

Clients inside a `wp_security_context_v1` (Flatpak and other sandboxes) MUST NOT see the
privileged globals: session lock, layer shell, input method, virtual keyboard and pointer, data
control, foreign toplevel, workspaces, output management, output power, gamma control, screencopy
and image capture, screencasting, `mutter_x11_interop` and fake input. Unsandboxed clients see
them, as in niri.

### 9.4 Not served

`xdg_toplevel_drag_manager_v1` (tab detach), `xdg_toplevel_icon_manager_v1` (per-window icons),
`wp_color_manager_v1` and `wp_color_representation_manager_v1`, `wp_fifo_manager_v1`,
`wp_commit_timing_manager_v1`, `wp_pointer_warp_v1`, `wp_tearing_control_manager_v1`,
`wp_content_type_manager_v1`, `wp_alpha_modifier_v1`, `wp_single_pixel_buffer_manager_v1` (niri
builds it only under `cfg(test)`), the KDE output and Plasma shell protocols (8.5),
`zwp_text_input_manager_v2`, `xdg_session_manager_v1`. `compat/PROTOCOLS.md` ranks each gap by what
users notice.

## 10. Stability requirements

- MUST NOT do synchronous disk I/O, fsync, or any stat able to reach a network or automounted
  file system on the main loop: session writes, usage-log writes, terminal discovery and ssh queries run
  on worker threads.
- MUST bound every queue between the main loop and a worker: the usage log drops (and counts)
  rather than blocks; the session writer skips a check while busy.
- MUST clamp client-controlled numbers before arithmetic: minimum sizes (65,535 px, 1,024 tiles),
  committed sizes, ScreenShot2 areas and image sizes (checked multiplication), zkde region sizes,
  and values read from a damaged `session.json`. A bad file is ignored, never a crash.
- MUST log any main-loop work taking 8 ms or more at debug level as
  `slow main-loop work: <what> took <n> ms` (guards around refresh, animation advance, redraw,
  client flush, each input event and the session tick).
- Placement MUST stay within its search budget (6,000 steps) and fall back to the push.
- A shrinking window's neighbours wait at most 300 ms for its commit.
- MUST render with a software EGL device (llvmpipe) on the primary GPU when it has nothing else,
  with a warning (niri skips software EGL devices, which left a session with no output). Software
  EGL on secondary GPUs stays skipped.
- Send a new window its fractional scale before its first commit, so its first buffer is drawn at
  the right scale.
- Main-loop stall targets: no operation the user triggers takes 8 ms or more on the main loop;
  placement for 64 windows takes about 3 ms at worst; the median layout operation takes tens of
  microseconds (46 us measured).
- Robustness: a client never acking configures, a 16384 px buffer, 1,000 map/unmap cycles, or a
  client dying mid-move, mid-resize or mid-popup-grab MUST NOT stop the compositor answering,
  freeze input, or send a well-behaved client a protocol error.

## 11. Testing

### 11.1 Unit and replay tests

- `newm-canvas/tests/replay.rs`: 400 seeds x 200 random operations over three canvas areas
  (1440x930, 4096x1122, 1920x1050), four configs and five minimum sizes; after every step
  `Canvas::check` plus each operation's own promise (order kept by pushes, spans kept outside a
  resize, focus shown, middle kept by zooms, zoom round trips, ocean moves nothing, swaps land on
  `Canvas::swapped`'s rects), and fixed scenarios mirrored in the proof.
- `newm-canvas/tests/rules.rs`: an exact branch-and-bound search reports rules 1-3 (ignored by
  default; run by hand).
- `newm-canvas/tests/bench.rs`: times movement decisions as the canvas fills (ignored by default;
  `--release -- --ignored --nocapture`); its decision digests MUST equal the reference search's.
- Differential tests between the range-tree search and `place::reference`.
- `src/layout/tests/canvas.rs` (about 75 tests): drags, carried spans, drop preview, resize
  preview, shrink holds, open size, placeholders, overview, zoom to fit, floating follow.
- `src/input/canvas.rs`: pan thresholds and settle, acceleration, swipe axes and steps, Super tap, which
  pinches zoom and the level a pinch reaches.
- `src/tests/mod_pan.rs`: the Mod pan's settle through the input path: in Mod's first 100 ms, a
  Mod+secondary press resizes and a Mod+primary press moves with the camera put; afterwards,
  motion past 8 px pans; a pan under way goes on; the same with Alt as the mod key.
- `src/tests/pinch.rs`: pinches through the input path: three fingers and Mod with two zoom in
  whole levels within the bounds, a plain two-finger pinch leaves the camera alone.
- `src/session*`: save format, never-saved list, launch planning, title matching, terminal command
  lines, Chromium flags.
- `src/dbus/kde_tests.rs`, `newm-kde`: kglobalaccel, KRunner, inhibitions, Qt key codes.
- `newm-rs-lock`: argument parsing, field state, rendering geometry and colours, secret wiping.
- `niri-config`: canvas option parsing, defaults, clamps, actions.
- All of niri's tests, including the insta snapshot tests.

### 11.2 The Rocq gate

Section 3.15. It MUST pass for any change to `newm-canvas` before merging.

### 11.3 The compat VM suite

`compat/` runs NixOS VM tests (`pkgs.testers.runNixOSTest`) against newm-rs as packaged (canvas
features, `newm-rs-session`, `newm-rs.service`). Each VM
logs in on tty1 the way a display manager would, starts the session, opens real apps, types
through uinput devices, and checks what the app saw.

- Guest: 2560x1080 virtio GPU without 3D (Mesa llvmpipe), 4 GB, 4 cores, uinput keyboard, mice and
  clickpad, KDE session environment, one output at scale 1.25, the reference canvas settings and
  `prefer-no-csd`.
- `smoke`: `inventory` (globals), `foot`, `gtk4-probe` (fractional scale 1.25 end to end),
  `gtk4-widget-factory`, `gtk3-probe` (buffer scale 2, compositor downscales), `gtk3-widget-factory`,
  `libadwaita-zenity`, `qt-kde-probe` (Breeze loads, DPR 1.25), `kwrite`, `xterm-xwayland`,
  `chromium` (`devicePixelRatio` 1.25), `kwin-reference` (KWin's globals for comparison).
- `input`: uinput devices, fcitx5 in GTK 4, GTK 3, Qt and Chromium, `wtype` Unicode, virtual
  pointer, clipboard (text, primary, 10 MB image, Wayland to X11 and back, across toolkits,
  watchers), drag and drop (GTK 4 and Qt both ways, Wayland and X11 both ways), relative pointer,
  cursor shape, tablet pen, touchscreen, a plain two-finger pinch reaching a GTK app,
  keyboard-shortcuts inhibit.
- `desktop`: activation tokens (fresh and stale), portal file choosers (GTK 4, GTK 3, Qt), portal
  screenshot, PipeWire screencast, Spectacle screenshot, layer-shell exclusive zone, keyboard
  exclusive and on-demand, layer popups, swaylock and newm-rs-lock, idle inhibition (mpv, D-Bus
  ScreenSaver, GTK 4, KDE Inhibit portal), output randr, gamma control, foreign toplevel, security
  context, kglobalaccel (Spectacle, refusal of config keys, no-context), KRunner windows.
- `gaps`: `output-power-wlopm`, `shortcuts-inhibit-gtk4`, `system-bell-gtk4`, `modal-dialog-gtk4`.
- `robust`: weston demo clients, the misbehaving and crashing clients of section 10.
- `remote-input`: KDE Connect's remote mouse and keyboard through the RemoteDesktop portal into a
  focused client; explicit sync.
- `backends`: two virtio GPUs (scale 1.25 and 1), both outputs drawn, first buffer already at 1.25,
  an output off and on with a window on it, a scale change reaching an open window, a mode change
  keeping the focus on screen, a VT switch and back, nested sessions (newm-rs's winit backend and
  a stand-in for the other compositor). `two-gpu`: the second GPU ignored and never opened.
- Every suite ends with an `alive-after-*` case: the compositor still answers.
- Results: `checks.<system>.compat-newm-rs` (the `smoke` suite) and `compat-newm-rs-<suite>` (the
  others) fail on a failing case; the matching `compat-report.*` packages never fail and write `results.txt`, `results.json`, each
  compositor's globals, the journal, screenshots and probe output.
- A case known to fail is listed in the compositor's `xfail` map with its reason; it reports
  `xfail`, and `xpass` once it passes, which prompts removing the entry. newm-rs's current
  entries: `explicit-sync` and `two-outputs` (software EGL limits in the VM), `portal-screenshot`,
  `screencast-pipewire`, `idle-inhibit-gtk4` and `idle-kde-inhibit-portal` (portal frontend in the
  VM).

## 12. Documentation and the README video

- niri's wiki stays as upstream has it under `docs/wiki/`.
- The README opens with a 2:35 video (1920x1080, 60 fps, H.264) and a silent looping WebP: a real
  usage log, sanitized to window kinds and tiles, replayed on a stylized canvas with no key hints
  or counters, under a synthetic narrator voice and a generated track. Script:
  `docs/video/SCRIPT.md`; pipeline: [video.md](video.md). Outputs stay out of git.
- Wording rule for all docs and narration: plain statements; no "... and a X that does Y" phrases;
  apps named generically ("Firefox, Chromium and other browsers").

## 13. Ports

newm-mac (macOS) is a separate port outside this specification; it consumes the `newm-canvas`
crate unchanged.

## 14. Acceptance checklist

A rebuild matches newm-rs when:

1. `cargo clippy --all-targets --features canvas -- -D warnings` and `cargo test --features canvas`
   pass with zero warnings, niri's tests included.
2. The replay harness passes 400 x 200 random steps with every invariant of 3.3.
3. The proof gate passes with all 68 claims and every declared mutant killed.
4. A grow stops at the screen edge; a shrink closes a lake and leaves ocean; a drop and the same
   key moves give the same layout; a swap changes no span; undo restores the exact layout.
5. A zoom sends no configure to any client and keeps the canvas point under the middle of the
   screen.
6. Moving the pointer across windows without a click changes nothing.
7. A logout and login brings every saved window back to its tiles; terminals reattach tmux and
   ssh with one authentication per host.
8. Killing the locker leaves the session locked; ScreenShot2 refuses window captures while locked.
9. The compat suite passes except its listed xfails.
10. No main-loop work logs 8 ms or more during the replayed operations.
11. A three-finger pinch, or Mod and a pinch, zooms in whole levels and reaches no app; a plain
    two-finger pinch reaches the app; Mod+wheel zooms one level per notch with the reference
    binds.

## 15. Open items

Gaps between the author's intent and `main`:

1. **`text` in default features:** a plain `cargo build` enables it and renders text with pango
   (2.4).
2. **Terminal reattach untested end to end:** no VM case reattaches a terminal; each terminal's
   launch form comes from its `--help` and has unit tests (6.5).
3. **Proof gate tool:** the gate runner the proof README invokes is not in the repository.
4. **Disruption guards:** guards against disruptive interactions (an interrupted drag, camera
   moves on unfocused opens, open bursts, X11 overlay decisions, slow configures) only log.
5. **Missing protocols:** `xdg_toplevel_icon_manager_v1`, `xdg_toplevel_drag_manager_v1`, colour
   management, FIFO, commit timing, pointer warp, tearing control, content type, alpha modifier
   and single-pixel buffer (9.4).
6. **KDE output and Plasma shell protocols:** not built; explicit sync and remote input came first
   (8.5).
7. **Explicit sync unproven in CI:** the VM's software EGL imports no dmabuf, so the syncobj case
    is xfail; only hardware EGL verifies it.
8. **compat xfails:** portal screenshot, PipeWire screencast and two idle-inhibit cases fail in
    the VM only; a software-only secondary GPU cannot drive an output.
9. **Main-loop stall targets have no test:** the slow-work log names stalls, but no test fails on
    them.
10. **Config across upgrades:** a running older binary rejects config nodes it predates, so after
    an upgrade the live reload fails until the next login (a switch never restarts the unit).
11. **Xwayland fixes outside the tree:** drag and drop between X11 and Wayland apps, X11 popup
    parents and opaque regions depend on xwayland-satellite patches outside this repository
    (2.5).
12. **Tile size default:** the code default stays 720x465 (newm's tile at its default zoom); the
    author's small fixed tiles (12x8 on a laptop panel) live in the configuration.
13. **Reference binds not shipped:** canvas actions have no default keys; the bind set of 5.3
    is not in `resources/default-config.kdl`.
