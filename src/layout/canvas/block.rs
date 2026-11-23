//! Blocks a client places: windows launched together and put on the canvas as one unit. A block
//! reserves its tiles at once with placeholders, which the new windows take as a restore's do,
//! by app ID and, among windows of one app, in launch order. A placeholder no window takes is
//! released when its lifetime passes. Parking moves every tiled window aside first, keeping the
//! arrangement, and one undo takes the park back.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use newm_canvas::place::is_bridge;
use newm_canvas::{Canvas, Edge, Id, Rect};
use niri_ipc::NewmBlockAnchor;

use super::{CanvasSpace, LayoutElement, Placeholder, RestoreWindow};
use crate::layout::Layout;

/// The tiles a block keeps clear of every window at the `Nearest` anchor, and parked windows
/// keep clear of the view.
const GAP: i32 = 1;

/// How many blocks a canvas answers status requests for, the most recent ones.
pub const KEPT: usize = 64;

const EPS: f64 = 1e-6;

/// Block ids, unique across canvases.
static NEXT_BLOCK: AtomicU64 = AtomicU64::new(1);

/// One window's place in a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockSlot {
    /// The placeholder that holds the slot's tiles.
    Waiting(Id),
    /// The window that took the placeholder.
    Taken(Id),
    /// The placeholder's lifetime passed with no window.
    Released,
}

/// A block on a canvas: its slots in the request's order and the slot whose window takes the
/// focus once none waits.
#[derive(Debug)]
pub struct Block {
    id: u64,
    slots: Vec<BlockSlot>,
    focus: Option<usize>,
    settled: bool,
}

impl Block {
    /// A new block, with a fresh id, whose slots the placeholders `ids` hold.
    pub fn new(ids: &[Id], focus: Option<usize>) -> Self {
        Self {
            id: NEXT_BLOCK.fetch_add(1, Ordering::Relaxed),
            slots: ids.iter().copied().map(BlockSlot::Waiting).collect(),
            focus,
            settled: false,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// Whether every slot has a window or was released.
    pub fn is_done(&self) -> bool {
        !self
            .slots
            .iter()
            .any(|s| matches!(s, BlockSlot::Waiting(_)))
    }

    /// Whether `placeholder` holds one of the block's slots.
    pub fn holds(&self, placeholder: Id) -> bool {
        self.slots.contains(&BlockSlot::Waiting(placeholder))
    }

    /// Window `window` took `placeholder`.
    pub fn take(&mut self, placeholder: Id, window: Id) {
        for slot in &mut self.slots {
            if *slot == BlockSlot::Waiting(placeholder) {
                *slot = BlockSlot::Taken(window);
            }
        }
    }

    /// The placeholders `expired` are gone with no window: their slots are released.
    pub fn release(&mut self, expired: &[Id]) {
        for slot in &mut self.slots {
            if let BlockSlot::Waiting(p) = *slot
                && expired.contains(&p)
            {
                *slot = BlockSlot::Released;
            }
        }
    }

    /// Once the block is done, the first time: the window that takes the focus, if its slot's
    /// window came.
    pub fn settle(&mut self) -> Option<Option<Id>> {
        if self.settled || !self.is_done() {
            return None;
        }
        self.settled = true;
        let focus = match self.focus.and_then(|k| self.slots.get(k)) {
            Some(BlockSlot::Taken(id)) => Some(*id),
            _ => None,
        };
        Some(focus)
    }

    /// Per slot, the window that took it.
    pub fn windows(&self) -> Vec<Option<Id>> {
        self.slots
            .iter()
            .map(|s| match *s {
                BlockSlot::Taken(id) => Some(id),
                _ => None,
            })
            .collect()
    }
}

/// Keeps at most [`KEPT`] blocks, room for one more: the oldest settled one goes first.
pub fn make_room(blocks: &mut Vec<Block>) {
    if blocks.len() >= KEPT {
        let k = blocks.iter().position(|b| b.settled).unwrap_or(0);
        blocks.remove(k);
    }
}

/// The last park or window placement: the layout before it and after each of its steps, every
/// window's tiles by id. While those layouts are the last steps of the canvas's history, one undo
/// (or redo) takes all of them back (or does them again).
#[derive(Debug)]
pub struct Park {
    layouts: Vec<Vec<(Id, Rect)>>,
    undone: bool,
}

/// The windows' tiles by id.
fn snapshot(c: &Canvas) -> Vec<(Id, Rect)> {
    let mut layout: Vec<(Id, Rect)> = c.windows().iter().map(|w| (w.id, w.rect)).collect();
    layout.sort_by_key(|&(id, _)| id);
    layout
}

/// The smallest rectangle holding `rects`, or none when there are none.
fn bounds(rects: impl IntoIterator<Item = Rect>) -> Option<Rect> {
    rects.into_iter().fold(None, |acc, r| {
        let Some(b) = acc else {
            return Some(r);
        };
        let (i, j) = (b.i.min(r.i), b.j.min(r.j));
        let (right, bottom) = (b.right().max(r.right()), b.bottom().max(r.bottom()));
        Some(Rect::new(i, j, right - i, bottom - j))
    })
}

/// Whether `r`, widened by [`GAP`] on every side, overlaps none of `occupied`.
fn clear(r: Rect, occupied: &[Rect]) -> bool {
    let wide = Rect::new(r.i - GAP, r.j - GAP, r.w + 2 * GAP, r.h + 2 * GAP);
    !occupied.iter().any(|o| o.overlaps(&wide))
}

/// The origin that centres a `w`x`h` block on `centre`, halves rounded to even.
fn centred((w, h): (i32, i32), (cx, cy): (f64, f64)) -> (i32, i32) {
    (
        (cx - f64::from(w) / 2.).round_ties_even() as i32,
        (cy - f64::from(h) / 2.).round_ties_even() as i32,
    )
}

/// The origin of a `w`x`h` block nearest `centre` (by the block's middle) where the block keeps
/// [`GAP`] tiles clear of every rectangle in `occupied`; of equals, the rightmost, then the
/// lowest. The candidates are the centred origin and each column and row just past a window's
/// side; the centred origin when none is clear.
pub fn nearest_free((w, h): (i32, i32), occupied: &[Rect], centre: (f64, f64)) -> (i32, i32) {
    let (ci, cj) = centred((w, h), centre);
    let mut columns = vec![ci];
    let mut rows = vec![cj];
    for o in occupied {
        columns.extend([o.right() + GAP, o.i - w - GAP]);
        rows.extend([o.bottom() + GAP, o.j - h - GAP]);
    }
    columns.sort_unstable();
    columns.dedup();
    rows.sort_unstable();
    rows.dedup();
    let mut best: Option<(f64, i32, i32)> = None;
    for &i in &columns {
        for &j in &rows {
            if !clear(Rect::new(i, j, w, h), occupied) {
                continue;
            }
            let dx = f64::from(i) + f64::from(w) / 2. - centre.0;
            let dy = f64::from(j) + f64::from(h) / 2. - centre.1;
            let d = dx * dx + dy * dy;
            let better =
                best.is_none_or(|(bd, bi, bj)| d < bd - EPS || (d < bd + EPS && (i, j) > (bi, bj)));
            if better {
                best = Some((d, i, j));
            }
        }
    }
    best.map_or((ci, cj), |(_, i, j)| (i, j))
}

/// The view of `c` in tiles: its left column and top row, and how many columns and rows it shows.
fn view_tiles(c: &Canvas) -> (f64, f64, f64, f64) {
    let (tw, th) = c.tile_size();
    let (vx, vy) = c.view();
    let (sw, sh) = c.screen();
    (vx / tw, vy / th, sw / tw, sh / th)
}

/// The origin `anchor` picks on `c` for a block of `slots`, rectangles relative to its origin:
/// the free spot nearest the view's middle at `Nearest`, the one centred on the view at `Camera`,
/// the tile given at `Tiles`, which a window must not hold any of the slots' tiles at.
pub fn block_origin(
    c: &Canvas,
    slots: &[Rect],
    anchor: NewmBlockAnchor,
) -> Result<(i32, i32), String> {
    if slots.is_empty() || slots.iter().any(|r| r.w < 1 || r.h < 1) {
        return Err(String::from("bad block"));
    }
    let size = slots
        .iter()
        .fold((0, 0), |(w, h), r| (w.max(r.right()), h.max(r.bottom())));
    let occupied: Vec<Rect> = c.windows().iter().map(|w| w.rect).collect();
    let (vi, vj, vw, vh) = view_tiles(c);
    let centre = (vi + vw / 2., vj + vh / 2.);
    match anchor {
        NewmBlockAnchor::Nearest => Ok(nearest_free(size, &occupied, centre)),
        NewmBlockAnchor::Camera => Ok(centred(size, centre)),
        NewmBlockAnchor::Tiles(i, j) => {
            let held = slots.iter().any(|r| {
                let at = Rect::new(r.i + i, r.j + j, r.w, r.h);
                occupied.iter().any(|o| o.overlaps(&at))
            });
            if held {
                Err(String::from("the block's tiles are taken"))
            } else {
                Ok((i, j))
            }
        }
    }
}

/// Reserves `slots` at `origin` for the placeholders `ids`: each on its tiles, or the free ones
/// nearest them. The focus and the view stay, unless `fly`: then the view moves the least it
/// must to show the block, and the placeholder of slot `focus` (else the first) takes the focus,
/// which the window that takes it keeps. The focus stays on screen either way.
pub fn reserve_block(
    c: &mut Canvas,
    ids: &[Id],
    slots: &[Rect],
    origin: (i32, i32),
    focus: Option<usize>,
    fly: bool,
) {
    let prev = c.focus();
    let view = c.view();
    for (&id, r) in ids.iter().zip(slots) {
        c.add_at(
            id,
            0.,
            0.,
            Rect::new(r.i + origin.0, r.j + origin.1, r.w, r.h),
        );
    }
    if let Some(prev) = prev {
        c.focus_id(prev);
    }
    c.set_view(view.0, view.1);
    if fly && let Some(shown) = bounds(slots.iter().copied()) {
        if let Some(&f) = focus.and_then(|k| ids.get(k)).or(ids.first()) {
            c.focus_id(f);
        }
        c.ensure_visible(Rect::new(
            origin.0 + shown.i,
            origin.1 + shown.j,
            shown.w,
            shown.h,
        ));
    }
    c.ensure_focus_visible();
}

/// Moves every window of `c` by the same columns, so that they keep their arrangement and the
/// rightmost edge among them ends [`GAP`] tiles left of the view (and of where they were). Each
/// window moves by [`Canvas::move_to`], one that is no bridge first, so no move pushes or gathers
/// another window. The focus goes to the window nearest the view, the rightmost (of equals, the
/// one nearest the view's middle row), and the view moves the least it must to show it: that
/// window ends at its left edge, and the rest of the view is free. None when `c` holds no
/// window.
pub fn park(c: &mut Canvas) -> Option<Park> {
    let all = bounds(c.windows().iter().map(|w| w.rect))?;
    let (vi, ..) = view_tiles(c);
    let right = ((vi + EPS).floor() as i32).min(all.i) - GAP;
    let di = right - all.right();
    let view = c.view();
    let mut left: Vec<Id> = c.windows().iter().map(|w| w.id).collect();
    let mut layouts = vec![snapshot(c)];
    while !left.is_empty() {
        let k = left
            .iter()
            .position(|&id| !is_bridge(c.windows(), id))
            .unwrap_or(0);
        let id = left.remove(k);
        if let Some(r) = c.rect(id) {
            c.move_to(id, r.i + di, r.j);
        }
        layouts.push(snapshot(c));
    }
    c.set_view(view.0, view.1);
    let (_, vj, _, vh) = view_tiles(c);
    let middle = vj + vh / 2.;
    let away = |r: Rect| (f64::from(r.j) + f64::from(r.h) / 2. - middle).abs();
    let nearest = c.windows().iter().min_by(|a, b| {
        (b.rect.right().cmp(&a.rect.right())).then(away(a.rect).total_cmp(&away(b.rect)))
    });
    if let Some(id) = nearest.map(|w| w.id) {
        c.focus_id(id);
    }
    Some(Park {
        layouts,
        undone: false,
    })
}

/// Places window `id` with its top-left corner on `tile`, through one [`Canvas::move_to`] as a
/// drop there by the pointer, then gives it `span` through [`Canvas::resize`] of its right and
/// bottom edges: the move comes first, so no window moves where it leaves. A span below the
/// window's minimum grows to it, and one past the screen at 1:1 shrinks to it. The focus stays.
/// The steps undo as one, as a park's. None when `c` holds no such window.
pub fn place_window(
    c: &mut Canvas,
    id: Id,
    tile: (i32, i32),
    span: Option<(i32, i32)>,
) -> Option<Park> {
    let r = c.rect(id)?;
    let (mut w, mut h) = span.unwrap_or((r.w, r.h));
    let (need_w, need_h) = c.window(id).map(|win| c.need(win.min_w, win.min_h))?;
    w = w.max(need_w).max(1);
    h = h.max(need_h).max(1);
    while w > r.w && !c.fits_at_1to1(w, r.h) {
        w -= 1;
    }
    while h > r.h && !c.fits_at_1to1(w, h) {
        h -= 1;
    }
    let mut layouts = vec![snapshot(c)];
    if (r.i, r.j) != tile && c.move_to(id, tile.0, tile.1) {
        layouts.push(snapshot(c));
    }
    for (edge, target) in [(Edge::Right, w), (Edge::Bottom, h)] {
        let now = c.rect(id).map_or(target, |r| match edge {
            Edge::Right => r.w,
            _ => r.h,
        });
        if target != now && c.resize(id, edge, target - now) {
            layouts.push(snapshot(c));
        }
    }
    Some(Park {
        layouts,
        undone: false,
    })
}

impl Park {
    /// Whether any step changed the canvas.
    pub fn changed(&self) -> bool {
        self.layouts.len() > 1
    }

    /// How many history steps of `c` one undo (or, with `redo`, one redo) takes: all of the
    /// park's while they are the last steps of the history, checked on a copy of `c`; none
    /// otherwise, when it takes one.
    pub fn steps(&self, c: &Canvas, redo: bool) -> Option<usize> {
        let steps = self.layouts.len() - 1;
        if self.undone != redo || steps < 2 {
            return None;
        }
        let mut chain: Vec<&Vec<(Id, Rect)>> = self.layouts.iter().collect();
        if !redo {
            chain.reverse();
        }
        let mut probe = c.clone();
        if snapshot(&probe) != *chain[0] {
            return None;
        }
        for layout in &chain[1..] {
            let stepped = if redo { probe.redo() } else { probe.undo() };
            if !stepped || snapshot(&probe) != **layout {
                return None;
            }
        }
        Some(steps)
    }

    /// Notes that an undo (or, with `redo`, a redo) took the park's steps.
    pub fn stepped(&mut self, redo: bool) {
        self.undone = !redo;
    }
}

impl<W: LayoutElement> CanvasSpace<W> {
    /// Places a block of `windows` (their rectangles relative to its origin) at the origin
    /// [`block_origin`] picks for `anchor`: a placeholder on each window's tiles, reserved by
    /// [`reserve_block`], which the next window with its app ID takes (of several, the first
    /// placed) until `lifetime` passes. The focus waits until no placeholder of the block
    /// waits; then the window of slot `focus` takes it.
    pub fn place_block(
        &mut self,
        windows: Vec<RestoreWindow>,
        anchor: NewmBlockAnchor,
        focus: Option<usize>,
        fly: bool,
        lifetime: Duration,
    ) -> Result<(u64, (i32, i32)), String> {
        let slots: Vec<Rect> = windows.iter().map(|w| w.rect).collect();
        let origin = block_origin(&self.canvas, &slots, anchor)?;
        let now = self.clock.now_unadjusted();
        let deadline = now + lifetime;
        let ids: Vec<Id> = (0..windows.len() as Id).map(|k| self.next_id + k).collect();
        self.next_id += windows.len() as Id;
        self.apply_for("block", |c| {
            reserve_block(c, &ids, &slots, origin, focus, fly);
        });
        let focus_ring = self.options.layout.focus_ring;
        for (&id, w) in ids.iter().zip(windows) {
            self.placeholders.push(Placeholder::new(
                id, w.app_id, None, None, focus_ring, deadline, now,
            ));
        }
        make_room(&mut self.blocks);
        let block = Block::new(&ids, focus);
        let id = block.id();
        self.blocks.push(block);
        self.update_tile_sizes(true, None);
        Ok((id, origin))
    }

    /// Whether `placeholder` holds a block's slot.
    pub(super) fn is_block_slot(&self, placeholder: Id) -> bool {
        self.blocks.iter().any(|b| b.holds(placeholder))
    }

    /// Records that window `id` took `placeholder`, when the placeholder holds a block's slot.
    pub(super) fn block_slot_taken(&mut self, placeholder: Id, id: Id) {
        for b in &mut self.blocks {
            b.take(placeholder, id);
        }
    }

    /// Releases the block slots `expired`, placeholders whose lifetime passed.
    pub(super) fn block_slots_expired(&mut self, expired: &[Id]) {
        for b in &mut self.blocks {
            b.release(expired);
        }
    }

    /// Gives the focus to the window each newly complete block names, which shows it.
    pub(super) fn settle_blocks(&mut self) {
        let settled: Vec<Option<Id>> = self.blocks.iter_mut().filter_map(Block::settle).collect();
        for id in settled.into_iter().flatten() {
            if self.canvas.rect(id).is_some() {
                self.apply_for("block", |c| c.focus_id(id));
            }
        }
    }

    /// Per slot of block `id`, the window that took it and is still here, and whether every
    /// slot has a window or was released; none when this canvas has no block `id`.
    pub fn block_status(&self, id: u64) -> Option<(Vec<Option<&W>>, bool)> {
        let b = self.blocks.iter().find(|b| b.id() == id)?;
        let windows = b
            .windows()
            .into_iter()
            .map(|e| {
                let e = e?;
                self.entries
                    .iter()
                    .find(|x| x.id == e)
                    .map(|x| x.tile.window())
            })
            .collect();
        Some((windows, b.is_done()))
    }

    /// Parks the grid windows (see [`park`]); [`Self::undo`] takes the whole park back as one
    /// step. False when the grid is empty.
    pub fn park(&mut self) -> bool {
        let Some(park) = self.logged("park", None, None, park) else {
            return false;
        };
        self.park = Some(park);
        self.update_tile_sizes(true, None);
        true
    }

    /// Places the grid window `window` on `tile` with `span` (see [`place_window`]) and answers
    /// the tiles it holds then; [`Self::undo`] takes the whole placement back as one step. None
    /// when `window` is not a grid window of this canvas.
    pub fn place_window(
        &mut self,
        window: &W::Id,
        tile: (i32, i32),
        span: Option<(i32, i32)>,
    ) -> Option<Rect> {
        if !self.is_on_grid(window) {
            return None;
        }
        let id = self.id_of(Some(window))?;
        let app = self.app_of(id);
        let placed = self.logged("place", Some(id), app, |c| place_window(c, id, tile, span))?;
        if placed.changed() {
            self.park = Some(placed);
        }
        self.update_tile_sizes(true, None);
        self.canvas.rect(id)
    }

    /// How many history steps one undo (or, with `redo`, one redo) takes (see [`Park::steps`]).
    pub(super) fn park_steps(&self, redo: bool) -> Option<usize> {
        self.park.as_ref()?.steps(&self.canvas, redo)
    }

    /// Notes that an undo (or, with `redo`, a redo) took the park's steps.
    pub(super) fn park_stepped(&mut self, redo: bool) {
        if let Some(park) = &mut self.park {
            park.stepped(redo);
        }
    }
}

impl<W: LayoutElement> Layout<W> {
    /// Places a block on the active workspace's canvas (see [`CanvasSpace::place_block`]): its id
    /// and origin.
    pub fn canvas_place_block(
        &mut self,
        windows: Vec<RestoreWindow>,
        anchor: NewmBlockAnchor,
        focus: Option<usize>,
        fly: bool,
        lifetime: Duration,
    ) -> Result<(u64, (i32, i32)), String> {
        let canvas = self
            .active_workspace_mut()
            .and_then(|ws| ws.canvas_mut(false))
            .ok_or_else(|| String::from("no canvas"))?;
        canvas.place_block(windows, anchor, focus, fly, lifetime)
    }

    /// The windows that took the slots of block `id`, on whichever canvas it is, and whether
    /// none waits (see [`CanvasSpace::block_status`]).
    pub fn canvas_block_status(&self, id: u64) -> Option<(Vec<Option<&W>>, bool)> {
        self.workspaces()
            .find_map(|(_, _, ws)| ws.scrolling().block_status(id))
    }

    /// Places `window` on `tile` of the canvas it is on, with `span` (see
    /// [`CanvasSpace::place_window`]): the tiles it holds then. None when no canvas has it as a
    /// grid window.
    pub fn canvas_place_window(
        &mut self,
        window: &W::Id,
        tile: (i32, i32),
        span: Option<(i32, i32)>,
    ) -> Option<Rect> {
        self.workspaces_mut()
            .find(|ws| ws.has_window(window))
            .and_then(|ws| ws.canvas_mut(false))
            .and_then(|canvas| canvas.place_window(window, tile, span))
    }

    /// Parks the active workspace's grid windows (see [`CanvasSpace::park`]).
    pub fn canvas_park(&mut self) -> bool {
        self.active_workspace_mut()
            .and_then(|ws| ws.canvas_mut(false))
            .is_some_and(|canvas| canvas.park())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(i: i32, j: i32, w: i32, h: i32) -> Rect {
        Rect::new(i, j, w, h)
    }

    #[test]
    fn nearest_free_answers_what_the_client_previews() {
        // The cases and answers of the client's reference search.
        let row = [r(0, 0, 1, 1), r(1, 0, 1, 1), r(2, 0, 1, 1)];
        assert_eq!(nearest_free((3, 2), &row, (2.6665, 2.3225)), (1, 2));
        assert_eq!(nearest_free((2, 2), &[], (2., 2.)), (1, 1));
        assert_eq!(nearest_free((2, 1), &[r(0, 0, 4, 4)], (2., 2.)), (1, 5));
        let corners = [r(0, 0, 2, 2), r(3, 0, 2, 2), r(0, 3, 2, 2), r(3, 3, 2, 2)];
        assert_eq!(
            nearest_free((1, 1), &corners, (2.5, 2.5)),
            (6, 2),
            "of equals, the rightmost"
        );
        assert_eq!(nearest_free((3, 3), &[r(-1, -1, 7, 7)], (2.5, 2.5)), (7, 1));
    }

    #[test]
    fn a_nearest_block_keeps_a_tile_clear_of_every_window() {
        let windows = [r(0, 0, 2, 1), r(2, 0, 1, 3), r(-3, 1, 3, 2), r(1, 3, 2, 2)];
        for size in [(1, 1), (2, 3), (4, 1), (5, 5)] {
            let (i, j) = nearest_free(size, &windows, (1.3, 1.7));
            assert!(
                clear(r(i, j, size.0, size.1), &windows),
                "{size:?} at {i},{j}"
            );
        }
    }
}
