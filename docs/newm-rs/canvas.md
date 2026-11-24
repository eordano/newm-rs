# The canvas

newm-rs replaces niri's scrolling columns with a strict 2D grid of tiles. This page covers where
tiled windows go and how they move; [camera.md](camera.md) covers what part of the canvas you see.
The exact rules, with their invariants and proofs, are in [SPEC §3](SPEC.md#3-the-canvas-model).

## Tiles

- The canvas is an unbounded grid. Every tile has the same logical size, 720×465 by default
  (`tile-width`, `tile-height`).
- Each tiled window owns a rectangle of whole tiles. Two tiled windows never share a tile.
- A window's span of tiles changes only through a resize or the app's minimum size; moves, swaps,
  drags, zooms and the windows around it keep it. An app's minimum size makes its window take more
  tiles, so a window is never smaller than the app allows.
- At zoom 0 (1:1) every app is drawn at its own size, pixel for pixel.
- The tile size does not depend on the screen, so the tiles at the screen's edges show in part.
- `layout { gaps }` keeps half a gap between a window and the lines of its tiles.

## Placement rules

When a window moves, swaps or grows, the windows in its way make room. newm-rs picks the
arrangement by three rules ([SPEC §3.8](SPEC.md#38-the-placement-engine)):

1. **Fewest tiles reassigned.** The cheapest arrangement wins; `distance-weight` and
   `diagonal-weight` add a cost for each tile a window making room moves and for a diagonal move.
2. **Islands stay whole.** Windows sharing a side form an island; windows connected before a move
   stay connected after it. A corner does not count.
3. **Rows and columns are kept.** A window making room may move diagonally, but rule 1 charges it
   for that, so it keeps its row or column when it can.

Past the search's step budget, the best arrangement found so far wins; with none found, the moved
window pushes whatever lies in its way.

## New windows

A new window opens on the free spot nearest the focused window, the one the camera scrolls least
to show, and opening it never moves another window. Its size comes, per axis, from a window rule,
else the size the app picks, else the last size of the same app's window, else
`canvas { default-width }` / `default-height`, else one tile, rounded to whole tiles and cut to
what fits on the screen at 1:1 ([SPEC §3.10](SPEC.md#310-open-size)). Floating windows keep niri's
sizing.

## Moving and resizing

- niri's `move-column-left`/`-right` and `move-window-up`/`-down` move the focused window one tile.
- `swap-neighbour DIRECTION` trades places with the neighbour; each window keeps its size.
- `send-to-edge left` or `right` moves the focused window past every other window on that side.
- `gather-islands` slides every island but the largest the least it must to touch it.
- Drag a window with Mod and the primary button, or by its title bar: the windows in the way step
  aside live, showing where they go if you drop there.
- Drag an edge with Mod and the secondary button: a box shows where the window lands and the
  windows it pushes are drawn where they go; the resize applies on release.
- Escape cancels either drag: everything returns where it was, and the key never reaches the app.
- `resize-edge EDGE TILES` moves one edge by whole tiles, positive outward. niri's width and height
  actions work in whole tiles too ([SPEC §3.11](SPEC.md#311-moving-windows)). A window never grows
  past what fits on its display at 1:1.

## Lakes and ocean

Shrinking or closing a window leaves a gap. A **lake** has a window lying against it on the far
side: the windows close the gap, unless that would split an island. The **ocean** opens onto the
empty canvas: nothing moves. When a window bridging two islands is dropped elsewhere or closed,
the islands come together.

## Undo, focus, floating windows

- `undo` takes back the last rearrangement (a move, drop, swap, resize, send-to-edge or gather),
  and `redo` does it again, while the same windows stay on the canvas.
- niri's focus actions move the focus to the neighbouring window on the grid; closing the focused
  window focuses the most recently used one. niri actions with no meaning on a grid (consume,
  expel, the tabbed display) do nothing.
- Floating windows live above the canvas, as in niri. `canvas { floating-follow }` decides whether
  they stay on screen (`"screen"`, the default), follow their tiled parent (`"parent"`) or move
  with the canvas (`"canvas"`) when the camera pans or zooms
  ([SPEC §3.13](SPEC.md#313-floating-windows)).
- `canvas { one-workspace }` keeps exactly one workspace per output.
