# newm-canvas proof

A Rocq model of the tiled-window decisions in `../src/lib.rs`: rectangles on an integer tile
grid, placement by the engine's layout (a free choice that `place` checks: targets on their
rectangles, spans kept, no overlap, no island split) with the push as its fallback, the lake/ocean
fill on shrink and close unless it splits an island, undo and redo, add's and add_at's free spot
and open span, the recency list that picks the focus after a close, minimum spans, the zoom as a
camera, the view's least-movement rule, and the span principle (a window's span changes only
through a resize or its minimum, so whatever moves it keeps it), for every public operation. The
niri glue that keeps a drag on its canvas and carries a span between canvases,
`../../src/layout/canvas/moves.rs`, is anchored as root `glue`. `proof.json` holds the scope, the
anchors that tie each model definition to its Rust line, the claims in plain words, the
findings and what is not covered. `PLAN.md` is generated from it.

## Run

```
python3 path/to/proofgate.py -C newm-canvas/proof check
```

The gate tool, `proofgate.py`, is not part of this repository. `check` compiles the model under `_build/`, re-checks every claim in the kernel, compares each
claim's meaning and each anchored file with `proof.lock.json`, and runs `replay.sh`: the Rust
replay harness on a copy of the crate (the crate is not a workspace member), including the
tests of the repaired findings F01 and F02. Without `rocq` or `cargo` on PATH it
builds them with nix from the worktree's `flake.lock`.

## Modules

| Module | What it holds |
|---|---|
| `Canvas.v` | the model: one definition per Rust function (`open_span`, `spot`, `spot_near`, `move_by` and `set_focus` with `recent` among them; place.rs's `splits`, `is_bridge`, `accepts` and `gather`'s check; `gather_islands`; `restore` for undo and redo), the float decisions (the tile at each zoom, `need` from the 1:1 tile, EPS) as an `env`, `Config::default` as `shipped` in 1/10000 px, and the fixed scenarios of `tests/replay.rs`, set_config's, rename's, canvas v2's, add_at's and the camera's among them |
| `Push.v` | push's sweep: each round moves a window only forward along the push, so the rounds stop (`settle_stops`), and every window ends clear of the mover and of each window swept before it, so a push leaves the canvas disjoint; fill slides only into free tiles |
| `Claims.v` | one lemma per operation, then the claims: no overlap, minimum spans, only `zoom_by` and `set_config` zoom and `set_config` only clamps, the zoom is a camera, shows the focus as far as it fits, keeps the point under the middle of the screen and comes back from a zoom out and in, grow fits at 1:1, ocean moves nothing, a placed layout keeps rule 2 (C50), undo and redo keep `check()` (C52), focus shown, `least` is least, shared physical edges, add moves nothing and opens at `open_span`, add_at keeps the invariants without the focus clause, moves nothing and lands on its saved tile when it is free, `spot_near`'s search ends on a free spot, `recent` stays a permutation of the ids with the focus first, closing the focus picks the last used window |
| `Spans.v` | the span principle: whatever an operation does, a window it does not resize keeps its span, raised only to its need; a resize changes no other window's span and a rename keeps the renamed window's; undo and redo put back a snapshot's spans (`spans_change_only_by_resize`); a carried span opens unchanged where it fits (`carried_span_opens`) |
| `Order.v` | F01's repair: the sweep's (lead, id) sort puts a window that lies before another first, so a push keeps the order along it of windows whose cross ranges overlap; `move_by` moves one tile and keeps that order |
| `Rules.v` | the three placement rules as checks: tiles reassigned (`displaced`), islands (`split_pair`), moves along a row or a column (`bent`), and what the push fallback still does against them (F06, F07, F08, repaired). `../src/place.rs` enforces rule 2; rules 1 and 3 are measures its cost weighs (tiles, distance, diagonal moves), not constraints; `../tests/rules.rs`'s exact search reports them by hand |
| `Index.v` | `../src/index.rs`: the range tree over top-left corners answers exactly what a scan of every rectangle answers (`overlapping_scan`: a rectangle that overlaps q has its corner in q widened by the widest and tallest held), and so do `touching` and `disjoint`; `shared_area` counts the tiles two rectangles share |
| `Islands.v` | the flood that numbers islands (`flood_parts`), and `splits`, `is_bridge` and the search's leaf check built on it answer `Canvas.splits` and `Canvas.is_bridge`; a close or a shrink splits no island, whether the gap stays open or fills (C51) |
| `Cost.v` | the search's tile term from shared areas counts, per moved window, the tiles of its old or new rectangle but not both that no earlier moved window holds, and sums to rule 1's tiles reassigned (`Rules.displaced`) |
| `Grid.v` | the search step's candidate grid: difference arrays and the 2D prefix sum give each candidate exactly the windows that overlap it, so blocked, clear and pending are the scans' answers; the tile terms sum to rule 1's tiles reassigned in any placement order; a child's room-local tile term is `fresh_term` |
| `Findings.v` | the counterexamples of F01 to F05, F09 (undo left a window below a minimum raised since) and F10 (send_to_edge's fallback measured the edge before the take) (F04: a drag made of remove then add reopened the window at a new window's span; F05: swap traded rectangles, now each keeps its span), each against the policy it found (the old queue push, the zoom that re-sized windows), and the repaired traces |

## Reading the model

- Windows are a list; ids are unique (a claim keeps them so). `swap` takes the neighbour as an
  argument and focus moves are `FocusId`: every claim holds for any choice of window. Likewise
  the placement ops carry the engine's answer `p` (and move_to, remove and gather_islands
  gather's answer `g`), and undo and redo are `Restore l` with any snapshot `l`: the model checks
  each as the Rust does and falls back as it does, so the claims hold for every answer, `None`
  (the push) included.
  A minimum's growth (`set_min`, and `enforce_all` in `set_config` and `Restore`) is placed the
  same way (`grow_to`): those ops carry answers `a` for each window and step (`a m Right` to
  widen, `a m Down` to heighten); `pushes` answers `None` everywhere, as the fixed scenarios do.
- `push` sweeps until each window stops moving, as `lib.rs` loops; the model runs |swept| + 1
  rounds and `settle_stops` shows that is always enough. Each window is written back by id.
- `set_config` carries its config as a new `env`, and `run` threads it (`env_after`): a step's
  claims hold under the env in force after it, each new env meeting `good` (clamp asserts
  `zoom_min <= zoom_max`).
- `add` never pushes: it takes the free spot nearest the focus that the view scrolls least to. That needs `check()`'s
  focus clause (with windows on the canvas, the focus is one of them), so C01 assumes it for
  add; C35 is the overlap without it. C17 and C30 prove it on every trace: `recent` stays a
  permutation of the window ids with the focus at its head (`recency_held`).
- `add_at` needs no focus clause: `spot_near` searches rings out to `far`, and the spot `far`
  tiles along `at`'s row lies past every window's right edge, so its `find` always returns
  (C45) and the `unwrap_or` fallback is never taken. A source mutant on that fallback would be
  equivalent, so none is declared.
- The zoom is a camera: `need` takes no zoom, so no window's minimum span depends on it; a grow
  and `open_span` cap at what fits at zoom 0 (`fits e 0`, the code's `fits_at_1to1`), while
  `check()`'s focus clause and C10 use the current zoom (`fits_on_screen`). `zoom_by` never
  fails and changes no window (C16); magnified past 1:1 the focus may not fit, and C36 states
  how it is shown then.
- A zoom rescales the view about the middle of the screen (`centred`, the code's `centred_view`),
  floored to the model's 1/10000 px: C38 keeps the middle to within that unit whenever showing
  the focus then scrolls nothing, and C39 gives back the canvas after a zoom out and in when the
  rescale lands on the unit; C41 meets both premises. Coming closer first comes back only when it
  does not scroll.
- `rename` checks the new id only against the windows, so C10's rename case still states that
  the focus is a window; C17 and C30 now prove it.
- `spot`'s `unwrap_or(right(far))` is never reached: `right(far)` is the last candidate, or
  `right(0)` when `far` is 0, and either is free since no window reaches past it (`spot_ok`
  proves it for `right(far)`). A source mutant on that fallback is equivalent, so none is
  declared.
- One automatic mutant survives by design: `nearest`'s id tie-break `<?` against `<=?` differs
  only for two windows with the same id, which C17 rules out.
