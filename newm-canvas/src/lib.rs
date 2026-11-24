//! newm's strict tile canvas: where newm-rs puts tiled windows.
//!
//! Windows own disjoint rectangles of tiles on an unbounded integer grid. A tile has a fixed
//! logical size (720x465 by default), and a window's size is its span of those tiles at every
//! zoom. The zoom is a camera: it scales how the canvas is drawn, never a window's size or tiles,
//! and changes only through [`Canvas::zoom_by`], or when a config reload ([`Canvas::set_config`])
//! clamps it into new bounds. Positive zoom pulls the camera back, negative zoom magnifies, both
//! about the middle of the screen.
//! The view is an offset into the canvas in on-screen logical pixels at the current zoom, and
//! moves the least it must to show the focused window. A new window opens on the free spot
//! nearest the focused one, the one the view scrolls least to show, and never moves another
//! window; a window a saved session puts back takes its saved tiles, or the free ones nearest
//! them, also moving no other window. When a window moves, swaps or grows, the others go where
//! [`place`]'s rules put them: the least cost in tiles reassigned, distance and diagonal moves,
//! no island split; past the search's budget it pushes whatever is in the way. Shrinking or
//! closing closes the gap when it is a lake (a window lies against it on the far side) and
//! closing it splits no island, never when it is ocean (nothing does: the gap opens onto the
//! empty canvas). A bridge between two islands that leaves by a drop or a close lets them get
//! together, and [`Canvas::gather_islands`] brings every island together. Undo and redo step
//! through these rearrangements while the same windows are on the canvas. Closing the focused
//! window focuses the most recently used one.
//! An app's minimum size makes its window take more tiles, so a window is never smaller than its
//! app allows; at zoom 0 apps are drawn 1:1.
//!
//! Every operation keeps the invariants [`Canvas::check`] tests; `tests/replay.rs` drives random
//! operation sequences through them, and `proof/` models the same rules in Rocq.

use std::collections::{HashSet, VecDeque};

pub mod index;
pub mod place;

use place::{is_bridge, splits};

pub type Id = u64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Rect {
    pub i: i32,
    pub j: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(i: i32, j: i32, w: i32, h: i32) -> Self {
        Self { i, j, w, h }
    }

    pub const fn right(&self) -> i32 {
        self.i + self.w
    }

    pub const fn bottom(&self) -> i32 {
        self.j + self.h
    }

    pub const fn rows_overlap(&self, o: &Rect) -> bool {
        self.j < o.bottom() && o.j < self.bottom()
    }

    pub const fn cols_overlap(&self, o: &Rect) -> bool {
        self.i < o.right() && o.i < self.right()
    }

    pub const fn overlaps(&self, o: &Rect) -> bool {
        self.rows_overlap(o) && self.cols_overlap(o)
    }

    pub const fn contains(&self, i: i32, j: i32) -> bool {
        self.i <= i && i < self.right() && self.j <= j && j < self.bottom()
    }

    pub fn cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (self.j..self.bottom()).flat_map(move |j| (self.i..self.right()).map(move |i| (i, j)))
    }

    const fn shifted(&self, di: i32, dj: i32) -> Rect {
        Rect::new(self.i + di, self.j + dj, self.w, self.h)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
            Dir::Up => (0, -1),
            Dir::Down => (0, 1),
        }
    }

    pub const fn opposite(self) -> Dir {
        match self {
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }
}

/// A window edge, named by the side of the window it is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    /// The direction the edge moves when the window grows.
    pub const fn outward(self) -> Dir {
        match self {
            Edge::Left => Dir::Left,
            Edge::Right => Dir::Right,
            Edge::Top => Dir::Up,
            Edge::Bottom => Dir::Down,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Config {
    /// Logical width of a tile: windows are sized from it at every zoom.
    pub tile_w: f64,
    /// Logical height of a tile.
    pub tile_h: f64,
    /// Each zoom step multiplies the scale the camera draws the canvas at by this (below 1: more
    /// tiles on screen).
    pub zoom_step: f64,
    pub zoom_min: i32,
    pub zoom_max: i32,
    /// What a window that makes room pays per tile of its area for each tile it moves.
    pub distance_weight: f64,
    /// What a window that makes room pays per tile of its area when it changes both its row and
    /// its column.
    pub diagonal_weight: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tile_w: 720.0,
            tile_h: 465.0,
            zoom_step: 0.8,
            zoom_min: -2,
            zoom_max: 4,
            distance_weight: 0.5,
            diagonal_weight: 1.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Window {
    pub id: Id,
    pub rect: Rect,
    /// Smallest logical size the app accepts, padding included.
    pub min_w: f64,
    pub min_h: f64,
}

const EPS: f64 = 1e-6;

#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    config: Config,
    windows: Vec<Window>,
    focus: Option<Id>,
    /// Window ids, the most recently focused first.
    recent: Vec<Id>,
    zoom: i32,
    view: (f64, f64),
    screen: (f64, f64),
    /// Layouts before each rearrangement, the last one last, while the same windows are here.
    undo: Vec<Vec<(Id, Rect)>>,
    /// Layouts undone, the last one undone last.
    redo: Vec<Vec<(Id, Rect)>>,
}

/// How many rearrangements undo reaches back.
const HISTORY: usize = 64;

impl Canvas {
    pub fn new(config: Config, screen_w: f64, screen_h: f64) -> Self {
        Self {
            config,
            windows: Vec::new(),
            focus: None,
            recent: Vec::new(),
            zoom: 0,
            view: (0.0, 0.0),
            screen: (screen_w, screen_h),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// The rectangles, in window order, after putting `id` on `target` and pushing along `dir`:
    /// the layout a rearrangement falls back to.
    fn pushed(&self, id: Id, target: Rect, dir: Dir) -> Vec<Rect> {
        let mut c = self.clone();
        if let Some(k) = c.index(id) {
            c.windows[k].rect = target;
            c.push(id, dir);
        }
        c.windows.iter().map(|w| w.rect).collect()
    }

    fn snapshot(&self) -> Vec<(Id, Rect)> {
        self.windows.iter().map(|w| (w.id, w.rect)).collect()
    }

    /// Remembers the layout before a rearrangement, for undo.
    fn record(&mut self) {
        self.undo.push(self.snapshot());
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn forget_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Puts every window back on its tiles in `layout`, then raises each to the span its minimum
    /// needs now (a minimum may have grown since); false, with nothing changed, unless `layout`
    /// holds exactly the windows on the canvas at positive spans, no two overlapping.
    fn restore(&mut self, layout: &[(Id, Rect)]) -> bool {
        if layout.len() != self.windows.len() {
            return false;
        }
        let mut next = self.windows.clone();
        for w in &mut next {
            let Some(&(_, r)) = layout.iter().find(|(id, _)| *id == w.id) else {
                return false;
            };
            w.rect = r;
        }
        let rects: Vec<Rect> = next.iter().map(|w| w.rect).collect();
        if rects.iter().any(|r| r.w < 1 || r.h < 1) || !place::disjoint(&rects) {
            return false;
        }
        self.windows = next;
        self.enforce_all();
        self.ensure_focus_visible();
        true
    }

    /// Takes back the last rearrangement. False when there is none, or when windows opened or
    /// closed since, which also forgets the history.
    pub fn undo(&mut self) -> bool {
        let Some(layout) = self.undo.pop() else {
            return false;
        };
        let now = self.snapshot();
        if !self.restore(&layout) {
            self.forget_history();
            return false;
        }
        self.redo.push(now);
        true
    }

    /// Does again the last rearrangement undone.
    pub fn redo(&mut self) -> bool {
        let Some(layout) = self.redo.pop() else {
            return false;
        };
        let now = self.snapshot();
        if !self.restore(&layout) {
            self.forget_history();
            return false;
        }
        self.undo.push(now);
        true
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    pub fn window(&self, id: Id) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn rect(&self, id: Id) -> Option<Rect> {
        self.window(id).map(|w| w.rect)
    }

    pub fn focus(&self) -> Option<Id> {
        self.focus
    }

    /// Window ids, the most recently focused first.
    pub fn recent(&self) -> &[Id] {
        &self.recent
    }

    /// Focuses `id` and makes it the most recently used window.
    fn set_focus(&mut self, id: Id) {
        self.focus = Some(id);
        self.recent.retain(|&r| r != id);
        self.recent.insert(0, id);
    }

    pub fn zoom(&self) -> i32 {
        self.zoom
    }

    pub fn view(&self) -> (f64, f64) {
        self.view
    }

    pub fn screen(&self) -> (f64, f64) {
        self.screen
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// The scale the camera draws the canvas at: `zoom_step` to the power of the zoom, 1 at
    /// zoom 0.
    pub fn scale(&self) -> f64 {
        self.config.zoom_step.powi(self.zoom)
    }

    /// Logical size of one tile at 1:1, which windows are sized from at every zoom.
    pub fn base_tile(&self) -> (f64, f64) {
        (self.config.tile_w, self.config.tile_h)
    }

    /// On-screen logical size of one tile at the current zoom: the base tile through the camera.
    pub fn tile_size(&self) -> (f64, f64) {
        let f = self.scale();
        (self.config.tile_w * f, self.config.tile_h * f)
    }

    /// Tiles a window with this minimum size needs, whatever the zoom.
    pub fn need(&self, min_w: f64, min_h: f64) -> (i32, i32) {
        let (tw, th) = self.base_tile();
        let n = |min: f64, t: f64| ((min / t) - EPS).ceil().max(1.0) as i32;
        (n(min_w, tw), n(min_h, th))
    }

    /// Whether a span of tiles fits on the screen at 1:1: the most a window grows or opens at,
    /// so its size never depends on the camera.
    pub fn fits_at_1to1(&self, w: i32, h: i32) -> bool {
        let (tw, th) = self.base_tile();
        w as f64 * tw <= self.screen.0 + EPS && h as f64 * th <= self.screen.1 + EPS
    }

    /// Whether a span of tiles fits on the screen at the current zoom: whether it can be shown
    /// whole.
    pub fn fits_on_screen(&self, w: i32, h: i32) -> bool {
        let (tw, th) = self.tile_size();
        w as f64 * tw <= self.screen.0 + EPS && h as f64 * th <= self.screen.1 + EPS
    }

    /// The rectangle in on-screen logical canvas pixels at the current zoom.
    pub fn canvas_px(&self, r: Rect) -> (f64, f64, f64, f64) {
        let (tw, th) = self.tile_size();
        (
            r.i as f64 * tw,
            r.j as f64 * th,
            r.w as f64 * tw,
            r.h as f64 * th,
        )
    }

    /// The rectangle in on-screen logical pixels relative to the screen's top-left.
    pub fn screen_px(&self, r: Rect) -> (f64, f64, f64, f64) {
        let (x, y, w, h) = self.canvas_px(r);
        (x - self.view.0, y - self.view.1, w, h)
    }

    /// Physical pixel of the vertical grid line `k` at this output scale.
    pub fn edge_x(&self, k: i32, scale: f64) -> i64 {
        ((k as f64 * self.tile_size().0 - self.view.0) * scale).round() as i64
    }

    /// Physical pixel of the horizontal grid line `k` at this output scale.
    pub fn edge_y(&self, k: i32, scale: f64) -> i64 {
        ((k as f64 * self.tile_size().1 - self.view.1) * scale).round() as i64
    }

    /// The rectangle snapped to physical pixels: `(x, y, w, h)` relative to the screen. Both
    /// sides come from the grid line they sit on, so neighbours share their edge exactly.
    pub fn physical(&self, r: Rect, scale: f64) -> (i64, i64, i64, i64) {
        let x0 = self.edge_x(r.i, scale);
        let y0 = self.edge_y(r.j, scale);
        (
            x0,
            y0,
            self.edge_x(r.right(), scale) - x0,
            self.edge_y(r.bottom(), scale) - y0,
        )
    }

    /// Replaces the configuration (a config reload): the zoom is clamped to the new range and
    /// windows take more tiles where their minimum needs it at the new tile size.
    pub fn set_config(&mut self, config: Config) {
        self.config = config;
        self.zoom = self.zoom.clamp(config.zoom_min, config.zoom_max);
        self.enforce_all();
        self.ensure_focus_visible();
    }

    /// Raises every window, in window order, to the span its minimum needs.
    fn enforce_all(&mut self) {
        let ids: Vec<Id> = self.windows.iter().map(|w| w.id).collect();
        for id in ids {
            let _ = self.enforce_min(id);
        }
    }

    /// Gives window `from` the id `to`, keeping its rectangle, minimum, focus and recency.
    pub fn rename(&mut self, from: Id, to: Id) -> bool {
        if self.index(to).is_some() {
            return false;
        }
        let Some(k) = self.index(from) else {
            return false;
        };
        self.windows[k].id = to;
        self.forget_history();
        if self.focus == Some(from) {
            self.focus = Some(to);
        }
        if let Some(r) = self.recent.iter_mut().find(|r| **r == from) {
            *r = to;
        }
        true
    }

    pub fn set_screen(&mut self, w: f64, h: f64) {
        self.screen = (w, h);
        self.ensure_focus_visible();
    }

    pub fn set_view(&mut self, x: f64, y: f64) {
        self.view = (x, y);
    }

    pub fn scroll_by(&mut self, dx: f64, dy: f64) {
        self.view = (self.view.0 + dx, self.view.1 + dy);
    }

    fn index(&self, id: Id) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    fn free(&self, r: &Rect, except: &[Id]) -> bool {
        self.windows
            .iter()
            .all(|w| except.contains(&w.id) || !w.rect.overlaps(r))
    }

    /// Moves the view the least it must so `r` is fully on screen at the current zoom; a
    /// rectangle larger than the screen is aligned to its top-left.
    pub fn ensure_visible(&mut self, r: Rect) {
        let (x, y, w, h) = self.canvas_px(r);
        self.view.0 = Self::least(self.view.0, x, w, self.screen.0);
        self.view.1 = Self::least(self.view.1, y, h, self.screen.1);
    }

    fn least(view: f64, start: f64, len: f64, screen: f64) -> f64 {
        if len > screen + EPS || start < view {
            start
        } else if start + len > view + screen {
            start + len - screen
        } else {
            view
        }
    }

    pub fn ensure_focus_visible(&mut self) {
        if let Some(r) = self.focus.and_then(|id| self.rect(id)) {
            self.ensure_visible(r);
        }
    }

    /// Whether `r` is fully on screen at the current zoom.
    pub fn is_visible(&self, r: Rect) -> bool {
        let (x, y, w, h) = self.screen_px(r);
        x >= -EPS && y >= -EPS && x + w <= self.screen.0 + EPS && y + h <= self.screen.1 + EPS
    }

    /// Pushes what is in the way of `mover` along `dir`. The other windows are swept in order
    /// along `dir` (nearest the mover's side first); each moves just far enough to clear the
    /// mover, if it overlaps it, and to stay past every window swept before it that shares its
    /// cross-axis range. So nothing overlaps afterwards, a window only moves when something
    /// moved into it, and windows keep their order along `dir`.
    fn push(&mut self, mover: Id, dir: Dir) {
        let Some(m) = self.rect(mover) else { return };
        let mut order: Vec<usize> = (0..self.windows.len())
            .filter(|&k| self.windows[k].id != mover)
            .collect();
        order.sort_by_key(|&k| {
            let r = self.windows[k].rect;
            let lead = match dir {
                Dir::Right => r.i,
                Dir::Left => -r.right(),
                Dir::Down => r.j,
                Dir::Up => -r.bottom(),
            };
            (lead, self.windows[k].id)
        });
        let mut swept: Vec<Rect> = Vec::with_capacity(order.len());
        for k in order {
            let mut r = self.windows[k].rect;
            loop {
                let before = r;
                if r.overlaps(&m) {
                    r = Self::past(r, &m, dir);
                }
                for p in &swept {
                    let shares = match dir {
                        Dir::Left | Dir::Right => p.rows_overlap(&r),
                        Dir::Up | Dir::Down => p.cols_overlap(&r),
                    };
                    let behind = match dir {
                        Dir::Right => r.i < p.right(),
                        Dir::Left => r.right() > p.i,
                        Dir::Down => r.j < p.bottom(),
                        Dir::Up => r.bottom() > p.j,
                    };
                    if shares && behind {
                        r = Self::past(r, p, dir);
                    }
                }
                if r == before {
                    break;
                }
            }
            self.windows[k].rect = r;
            swept.push(r);
        }
    }

    /// `r` moved along `dir` until it lies just past `p`.
    const fn past(r: Rect, p: &Rect, dir: Dir) -> Rect {
        match dir {
            Dir::Left => Rect::new(p.i - r.w, r.j, r.w, r.h),
            Dir::Right => Rect::new(p.right(), r.j, r.w, r.h),
            Dir::Up => Rect::new(r.i, p.j - r.h, r.w, r.h),
            Dir::Down => Rect::new(r.i, p.bottom(), r.w, r.h),
        }
    }

    fn touches(side: Dir, x: &Rect, p: &Rect) -> bool {
        match side {
            Dir::Right => x.i == p.right() && x.rows_overlap(p),
            Dir::Left => x.right() == p.i && x.rows_overlap(p),
            Dir::Down => x.j == p.bottom() && x.cols_overlap(p),
            Dir::Up => x.bottom() == p.j && x.cols_overlap(p),
        }
    }

    /// The windows lying against `freed` on `side`, and the windows lying against those on the
    /// same side: what slides into the gap.
    pub fn chain(&self, freed: Rect, side: Dir) -> Vec<Id> {
        let mut chain: Vec<Id> = Vec::new();
        let mut queue: VecDeque<Rect> = VecDeque::from([freed]);
        while let Some(p) = queue.pop_front() {
            for w in &self.windows {
                if !chain.contains(&w.id) && Self::touches(side, &w.rect, &p) {
                    chain.push(w.id);
                    queue.push_back(w.rect);
                }
            }
        }
        chain
    }

    /// Closes the gap `freed` leaves when it is a lake (a window lies against it on `side`): the
    /// chain slides into it together by as many tiles as it can (at most the gap's depth)
    /// without overlapping anything else. An ocean gap (nothing on that side) stays.
    fn fill(&mut self, freed: Rect, side: Dir) -> bool {
        let chain = self.chain(freed, side);
        if chain.is_empty() {
            return false;
        }
        let (di, dj) = side.opposite().delta();
        let depth = if di != 0 { freed.w } else { freed.h };
        for k in (1..=depth).rev() {
            let ok = chain.iter().all(|id| {
                let r = self.rect(*id).unwrap().shifted(di * k, dj * k);
                self.free(&r, &chain)
            });
            if ok {
                for w in self.windows.iter_mut().filter(|w| chain.contains(&w.id)) {
                    w.rect = w.rect.shifted(di * k, dj * k);
                }
                return true;
            }
        }
        false
    }

    /// Grows `id` to the span its minimum size needs: wider, then taller, each step placed by
    /// [`Self::grow`]; false when a step fell back to the push. Records no undo step: undo must
    /// not take an app below the size it insists on, and a restore enforces every minimum again.
    fn enforce_min(&mut self, id: Id) -> bool {
        let Some(k) = self.index(id) else { return true };
        let w = self.windows[k];
        let (nw, nh) = self.need(w.min_w, w.min_h);
        let mut placed = true;
        if w.rect.w < nw {
            placed &= self.grow(id, Rect::new(w.rect.i, w.rect.j, nw, w.rect.h), Dir::Right);
        }
        let r = self.windows[k].rect;
        if r.h < nh {
            placed &= self.grow(id, Rect::new(r.i, r.j, r.w, nh), Dir::Down);
        }
        placed
    }

    /// Puts `id` on `target`, its rectangle grown along `dir`, by the placement rules (the
    /// engine's layout, checked by [`Self::accepts`]), the push along `dir` its seed; when no
    /// layout passes, it pushes along `dir` as a resize does, and returns false.
    fn grow(&mut self, id: Id, target: Rect, dir: Dir) -> bool {
        let seed = self.pushed(id, target, dir);
        if self.place(&[(id, target)], Some(dir), Some(seed)) {
            return true;
        }
        let k = self.index(id).expect("grow takes a window on the canvas");
        self.windows[k].rect = target;
        self.push(id, dir);
        false
    }

    /// The span a new window opens at: `want` cut to the largest span that fits on the screen at
    /// 1:1, and never below the span its minimum needs. The zoom plays no part.
    pub fn open_span(&self, min_w: f64, min_h: f64, want: Option<(i32, i32)>) -> (i32, i32) {
        let (nw, nh) = self.need(min_w, min_h);
        let Some((ww, wh)) = want else {
            return (nw, nh);
        };
        let (tw, th) = self.base_tile();
        let most = |screen: f64, t: f64| ((screen + EPS) / t).floor() as i32;
        (
            ww.min(most(self.screen.0, tw)).max(nw),
            wh.min(most(self.screen.1, th)).max(nh),
        )
    }

    /// Where a `w`x`h` window opens beside `f`; the spot in `f`'s rows past every window's
    /// right edge is always free, so the search ends there.
    /// Where a new window of span `w`x`h` opens beside the focused rectangle `f`: the free spot
    /// nearest `f` (the fewest tiles between them; a spot touching it only at a corner is one
    /// away), then the one the view scrolls least to show, then right, below, left, above, then
    /// the nearest to `f`'s corner.
    fn spot(&self, f: Rect, w: i32, h: i32) -> Rect {
        let gap = |c: &Rect| {
            let dx = (f.i - c.right()).max(c.i - f.right()).max(0);
            let dy = (f.j - c.bottom()).max(c.j - f.bottom()).max(0);
            dx + dy + i32::from(dx == 0 && dy == 0 && !place::touch(c, &f))
        };
        let scroll = |c: &Rect| {
            let (x, y, cw, ch) = self.canvas_px(*c);
            (Self::least(self.view.0, x, cw, self.screen.0) - self.view.0).abs()
                + (Self::least(self.view.1, y, ch, self.screen.1) - self.view.1).abs()
        };
        let side = |c: &Rect| {
            if c.i >= f.right() {
                0
            } else if c.j >= f.bottom() {
                1
            } else if c.right() <= f.i {
                2
            } else {
                3
            }
        };
        for d in 0.. {
            let mut best: Option<(f64, i32, i32, Rect)> = None;
            for i in f.i - w - d..=f.right() + d {
                for j in f.j - h - d..=f.bottom() + d {
                    let c = Rect::new(i, j, w, h);
                    if gap(&c) != d || !self.free(&c, &[]) {
                        continue;
                    }
                    let key = (scroll(&c), side(&c), (c.j - f.j).abs() + (c.i - f.i).abs());
                    if best.is_none_or(|b| {
                        key.0 < b.0 - EPS || (key.0 < b.0 + EPS && (key.1, key.2) < (b.1, b.2))
                    }) {
                        best = Some((key.0, key.1, key.2, c));
                    }
                }
            }
            if let Some(b) = best {
                return b.3;
            }
        }
        unreachable!("an unbounded canvas always has a free spot")
    }

    /// Opens a window at its [`open_span`](Self::open_span) beside the focused one and focuses
    /// it, on the spot [`Self::spot`] picks. No other window moves.
    pub fn add(&mut self, id: Id, min_w: f64, min_h: f64, want: Option<(i32, i32)>) -> Rect {
        assert!(
            self.index(id).is_none(),
            "window {id} is already on the canvas"
        );
        let (w, h) = self.open_span(min_w, min_h, want);
        let rect = match self.focus.and_then(|f| self.rect(f)) {
            None => Rect::new(0, 0, w, h),
            Some(f) => self.spot(f, w, h),
        };
        self.windows.push(Window {
            id,
            rect,
            min_w,
            min_h,
        });
        self.forget_history();
        self.set_focus(id);
        self.ensure_focus_visible();
        rect
    }

    /// The free spot nearest `at` for a span: the nearest ring of tiles around it first, then the
    /// nearest spot in that ring, ties to the upper, then the left one. The spot in `at`'s rows
    /// past every window's right edge is always free, so the search ends there.
    fn spot_near(&self, min_w: f64, min_h: f64, at: Rect) -> Rect {
        let (w, h) = self.open_span(min_w, min_h, Some((at.w, at.h)));
        let shifted = |(di, dj): (i32, i32)| Rect::new(at.i + di, at.j + dj, w, h);
        let far = self
            .windows
            .iter()
            .map(|o| o.rect.right() - at.i)
            .max()
            .unwrap_or(0)
            .max(0);
        (0..=far)
            .flat_map(|d| {
                let mut ring: Vec<(i32, i32)> = if d == 0 {
                    vec![(0, 0)]
                } else {
                    (-d..=d)
                        .flat_map(|di| [(di, -d), (di, d)])
                        .chain((1 - d..d).flat_map(|dj| [(-d, dj), (d, dj)]))
                        .collect()
                };
                ring.sort_by_key(|&(di, dj)| (di * di + dj * dj, dj, di));
                ring
            })
            .map(shifted)
            .find(|r| self.free(r, &[]))
            .unwrap_or(shifted((far, 0)))
    }

    /// Puts a window back at `at` (a saved session) and focuses it: at the span `at` opens at
    /// ([`open_span`](Self::open_span) of `at`'s span), on `at`'s tile when that spot is free, else
    /// on the free spot nearest it. No other window moves.
    pub fn add_at(&mut self, id: Id, min_w: f64, min_h: f64, at: Rect) -> Rect {
        assert!(
            self.index(id).is_none(),
            "window {id} is already on the canvas"
        );
        let rect = self.spot_near(min_w, min_h, at);
        self.windows.push(Window {
            id,
            rect,
            min_w,
            min_h,
        });
        self.forget_history();
        self.set_focus(id);
        self.ensure_focus_visible();
        rect
    }

    /// Closes a window: its lake is filled from the right, then from below; when it had the
    /// focus, the focus goes to the most recently used window left, else the nearest one.
    pub fn remove(&mut self, id: Id) -> bool {
        let Some(k) = self.index(id) else {
            return false;
        };
        let before = self.windows.clone();
        let group = if is_bridge(&before, id) {
            self.island_of(id)
        } else {
            Vec::new()
        };
        let freed = self.windows.remove(k).rect;
        self.recent.retain(|&r| r != id);
        let open = self.windows.clone();
        self.fill(freed, Dir::Right);
        self.fill(freed, Dir::Down);
        if splits(&before, &self.windows, &[id]) {
            self.windows = open;
        }
        self.gather(&group);
        self.forget_history();
        if self.focus == Some(id) {
            self.focus = None;
            if let Some(next) = self.recent.first().copied().or_else(|| self.nearest(freed)) {
                self.set_focus(next);
            }
        }
        self.ensure_focus_visible();
        true
    }

    fn nearest(&self, r: Rect) -> Option<Id> {
        let c = Self::center(&r);
        self.windows
            .iter()
            .min_by(|a, b| {
                let da = Self::dist2(c, Self::center(&a.rect));
                let db = Self::dist2(c, Self::center(&b.rect));
                da.total_cmp(&db).then(a.id.cmp(&b.id))
            })
            .map(|w| w.id)
    }

    fn center(r: &Rect) -> (f64, f64) {
        (r.i as f64 + r.w as f64 / 2.0, r.j as f64 + r.h as f64 / 2.0)
    }

    fn dist2(a: (f64, f64), b: (f64, f64)) -> f64 {
        (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
    }

    /// Updates a window's minimum size; it takes more tiles if it needs them, placed by the
    /// placement rules. False when the rules found no layout and the growth pushed instead.
    pub fn set_min(&mut self, id: Id, min_w: f64, min_h: f64) -> bool {
        let Some(k) = self.index(id) else { return true };
        self.windows[k].min_w = min_w;
        self.windows[k].min_h = min_h;
        let placed = self.enforce_min(id);
        self.ensure_focus_visible();
        placed
    }

    pub fn focus_id(&mut self, id: Id) -> bool {
        if self.index(id).is_none() {
            return false;
        }
        self.set_focus(id);
        self.ensure_focus_visible();
        true
    }

    /// The window `dir` of `from`: nearest gap first, then the smallest offset across.
    pub fn neighbour(&self, from: Id, dir: Dir) -> Option<Id> {
        let f = self.rect(from)?;
        let fc = Self::center(&f);
        self.windows
            .iter()
            .filter(|w| w.id != from)
            .filter_map(|w| {
                let r = w.rect;
                let c = Self::center(&r);
                let (gap, across) = match dir {
                    Dir::Right if r.i >= f.right() => (r.i - f.right(), (c.1 - fc.1).abs()),
                    Dir::Left if r.right() <= f.i => (f.i - r.right(), (c.1 - fc.1).abs()),
                    Dir::Down if r.j >= f.bottom() => (r.j - f.bottom(), (c.0 - fc.0).abs()),
                    Dir::Up if r.bottom() <= f.j => (f.j - r.bottom(), (c.0 - fc.0).abs()),
                    _ => return None,
                };
                Some((gap, across, w.id))
            })
            .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)))
            .map(|t| t.2)
    }

    pub fn focus_dir(&mut self, dir: Dir) -> bool {
        match self.focus.and_then(|f| self.neighbour(f, dir)) {
            Some(n) => self.focus_id(n),
            None => false,
        }
    }

    /// Moves one edge of a window by `delta` tiles: outward grows it (pushing what is in the
    /// way; refused past the screen at 1:1, whatever the zoom), inward shrinks it (refused below
    /// its minimum span) and closes the lake it leaves. The zoom never changes.
    pub fn resize(&mut self, id: Id, edge: Edge, delta: i32) -> bool {
        let Some(k) = self.index(id) else {
            return false;
        };
        let w = self.windows[k];
        let r = w.rect;
        let new = match edge {
            Edge::Left => Rect::new(r.i - delta, r.j, r.w + delta, r.h),
            Edge::Right => Rect::new(r.i, r.j, r.w + delta, r.h),
            Edge::Top => Rect::new(r.i, r.j - delta, r.w, r.h + delta),
            Edge::Bottom => Rect::new(r.i, r.j, r.w, r.h + delta),
        };
        let (nw, nh) = self.need(w.min_w, w.min_h);
        if delta == 0 || new.w < nw || new.h < nh || (delta > 0 && !self.fits_at_1to1(new.w, new.h))
        {
            return false;
        }
        self.record();
        if delta > 0 {
            let seed = self.pushed(id, new, edge.outward());
            if !self.place(&[(id, new)], Some(edge.outward()), Some(seed)) {
                self.windows[k].rect = new;
                self.push(id, edge.outward());
            }
        } else {
            let before = self.windows.clone();
            self.windows[k].rect = new;
            let d = -delta;
            let (freed, side) = match edge {
                Edge::Left => (Rect::new(r.i, r.j, d, r.h), Dir::Left),
                Edge::Right => (Rect::new(new.right(), r.j, d, r.h), Dir::Right),
                Edge::Top => (Rect::new(r.i, r.j, r.w, d), Dir::Up),
                Edge::Bottom => (Rect::new(r.i, new.bottom(), r.w, d), Dir::Down),
            };
            let open = self.windows.clone();
            self.fill(freed, side);
            if splits(&before, &self.windows, &[id]) {
                self.windows = open;
            }
        }
        self.ensure_focus_visible();
        true
    }

    /// Resizes the focused window.
    pub fn resize_focused(&mut self, edge: Edge, delta: i32) -> bool {
        match self.focus {
            Some(f) => self.resize(f, edge, delta),
            None => false,
        }
    }

    /// Swaps the focused window with its neighbour in `dir`, both keeping their spans (see
    /// [`Self::swapped`]). The neighbour steps out while the focus lands and pushes what it lands
    /// on along `dir`; then the neighbour comes back last and pushes what it lands on the other
    /// way.
    pub fn swap(&mut self, dir: Dir) -> bool {
        let Some(f) = self.focus else { return false };
        let Some(n) = self.neighbour(f, dir) else {
            return false;
        };
        let (fa, nb) = Self::swapped(self.rect(f).unwrap(), self.rect(n).unwrap(), dir);
        self.record();
        if self.place(&[(f, fa), (n, nb)], Some(dir), None) {
            self.ensure_focus_visible();
            return true;
        }
        let kn = self.index(n).unwrap();
        let mut w = self.windows.remove(kn);
        let k = self.index(f).unwrap();
        self.windows[k].rect = fa;
        self.push(f, dir);
        w.rect = nb;
        self.windows.push(w);
        self.push(n, dir.opposite());
        self.ensure_focus_visible();
        true
    }

    /// Where a swap in `dir` puts the focus `a` and its neighbour `b`, at their own spans. Along
    /// `dir` the pair keeps the extent it covers: the focus ends where the neighbour ended, the
    /// neighbour starts where the focus started. Across it they trade their first row or column.
    /// With equal spans that is an exchange of the two rectangles.
    pub const fn swapped(a: Rect, b: Rect, dir: Dir) -> (Rect, Rect) {
        match dir {
            Dir::Right => (
                Rect::new(b.right() - a.w, b.j, a.w, a.h),
                Rect::new(a.i, a.j, b.w, b.h),
            ),
            Dir::Left => (
                Rect::new(b.i, b.j, a.w, a.h),
                Rect::new(a.right() - b.w, a.j, b.w, b.h),
            ),
            Dir::Down => (
                Rect::new(b.i, b.bottom() - a.h, a.w, a.h),
                Rect::new(a.i, a.j, b.w, b.h),
            ),
            Dir::Up => (
                Rect::new(b.i, b.j, a.w, a.h),
                Rect::new(a.i, a.bottom() - b.h, b.w, b.h),
            ),
        }
    }

    /// Moves a window one tile in `dir`, pushing whatever is in the way along `dir`. The gap it
    /// leaves behind stays open.
    pub fn move_by(&mut self, id: Id, dir: Dir) -> bool {
        let Some(k) = self.index(id) else {
            return false;
        };
        let (di, dj) = dir.delta();
        let target = self.windows[k].rect.shifted(di, dj);
        self.record();
        let seed = self.pushed(id, target, dir);
        if !self.place(&[(id, target)], Some(dir), Some(seed)) {
            self.windows[k].rect = target;
            self.push(id, dir);
        }
        self.ensure_focus_visible();
        true
    }

    /// Takes a window out of the canvas, closing the lake it leaves.
    fn take(&mut self, id: Id) -> Option<Window> {
        let k = self.index(id)?;
        let w = self.windows.remove(k);
        self.fill(w.rect, Dir::Right);
        self.fill(w.rect, Dir::Down);
        Some(w)
    }

    /// Moves the focused window past every other window on the left or right, in its row.
    /// Past the search's budget it takes the window out and measures the edge after the take,
    /// whose fill may have slid a window into the tiles the focus left.
    pub fn send_to_edge(&mut self, dir: Dir) -> bool {
        let Some(f) = self.focus else { return false };
        if self.windows.len() < 2 || !matches!(dir, Dir::Left | Dir::Right) {
            return false;
        }
        let r = self.rect(f).unwrap();
        let others = self.windows.iter().filter(|o| o.id != f);
        let i = match dir {
            Dir::Left => others.map(|o| o.rect.i).min().unwrap() - r.w,
            _ => others.map(|o| o.rect.right()).max().unwrap(),
        };
        let target = Rect::new(i, r.j, r.w, r.h);
        self.record();
        let seed = self.pushed(f, target, dir);
        if !self.place(&[(f, target)], Some(dir), Some(seed)) {
            let mut w = self.take(f).unwrap();
            w.rect.i = match dir {
                Dir::Left => self.windows.iter().map(|o| o.rect.i).min().unwrap() - w.rect.w,
                _ => self.windows.iter().map(|o| o.rect.right()).max().unwrap(),
            };
            self.windows.push(w);
        }
        self.ensure_focus_visible();
        true
    }

    /// Moves a window to tile `(i, j)`, the others placed by [`place`]'s rules; when it was the
    /// bridge between islands, they get together. A move onto its own tiles changes nothing.
    pub fn move_to(&mut self, id: Id, i: i32, j: i32) -> bool {
        let Some(r) = self.rect(id) else {
            return false;
        };
        let target = Rect::new(i, j, r.w, r.h);
        if target == r {
            return true;
        }
        let group = if is_bridge(&self.windows, id) {
            self.island_of(id)
        } else {
            Vec::new()
        };
        self.record();
        let (di, dj) = (i - r.i, j - r.j);
        let along = if di.abs() >= dj.abs() {
            if di > 0 {
                Dir::Right
            } else {
                Dir::Left
            }
        } else if dj > 0 {
            Dir::Down
        } else {
            Dir::Up
        };
        let seed = self.pushed(id, target, along);
        if !self.place(&[(id, target)], Some(along), Some(seed)) {
            let mut w = self.take(id).unwrap();
            w.rect = target;
            self.windows.push(w);
            self.push(id, Dir::Right);
        }
        self.gather(&group);
        self.ensure_focus_visible();
        true
    }

    /// Gathers every window into one island: each island but the largest slides, smallest move
    /// first, the least distance that makes it touch the largest, stopping at the first window
    /// in its way, or, when no island can, the least along one axis and then the other; until
    /// one island is left or none can move. Every span is kept and nothing overlaps, or nothing
    /// moves. False, with nothing changed, when fewer than two islands are on the canvas or none
    /// moved; else undo takes it back.
    pub fn gather_islands(&mut self) -> bool {
        let label = place::islands(&self.windows, &[]);
        if label.values().collect::<HashSet<_>>().len() < 2 {
            return false;
        }
        let before = self.windows.clone();
        let all: Vec<Id> = before.iter().map(|w| w.id).collect();
        let (lo_i, lo_j, hi_i, hi_j) = before.iter().fold(
            (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
            |(a, b, c, d), w| {
                let r = w.rect;
                (a.min(r.i), b.min(r.j), c.max(r.right()), d.max(r.bottom()))
            },
        );
        let reach = (hi_i - lo_i).max(hi_j - lo_j);
        self.gather_kept(&all, all.len(), reach, true);
        let moved = self.windows != before;
        if moved {
            let after = std::mem::replace(&mut self.windows, before);
            self.record();
            self.windows = after;
        }
        self.ensure_focus_visible();
        moved
    }

    /// Moves the camera by `delta` zoom steps (positive: pulls back, more tiles on screen;
    /// negative: magnifies, past 1:1 too). No window changes its tiles. The canvas point under the
    /// middle of the screen stays there, as when a camera pulls back or comes closer; then the view
    /// moves the least it must to show the focus, or as much of it as fits. Refused only when the
    /// clamped zoom does not change.
    pub fn zoom_by(&mut self, delta: i32) -> bool {
        let z = (self.zoom + delta).clamp(self.config.zoom_min, self.config.zoom_max);
        if z == self.zoom {
            return false;
        }
        let from = self.scale();
        self.zoom = z;
        self.view = self.centred_view(self.view, from, self.scale());
        self.ensure_focus_visible();
        true
    }

    /// The view that keeps the canvas point under the middle of the screen in place when the
    /// camera's scale goes from `from` to `to`. Views are in on-screen pixels at their scale.
    pub fn centred_view(&self, view: (f64, f64), from: f64, to: f64) -> (f64, f64) {
        let about = |v: f64, screen: f64| (v + screen / 2.0) / from * to - screen / 2.0;
        (about(view.0, self.screen.0), about(view.1, self.screen.1))
    }

    /// Focuses the window nearest the middle of the screen and shows it (the end of a pan).
    pub fn focus_view_center(&mut self) -> bool {
        let (tw, th) = self.tile_size();
        let c = (
            (self.view.0 + self.screen.0 / 2.0) / tw,
            (self.view.1 + self.screen.1 / 2.0) / th,
        );
        let best = self
            .windows
            .iter()
            .min_by(|a, b| {
                Self::dist2(c, Self::center(&a.rect))
                    .total_cmp(&Self::dist2(c, Self::center(&b.rect)))
                    .then(a.id.cmp(&b.id))
            })
            .map(|w| w.id);
        match best {
            Some(id) => self.focus_id(id),
            None => false,
        }
    }

    /// The window under a point in on-screen logical pixels.
    pub fn window_at(&self, x: f64, y: f64) -> Option<Id> {
        let (tw, th) = self.tile_size();
        let i = ((x + self.view.0) / tw).floor() as i32;
        let j = ((y + self.view.1) / th).floor() as i32;
        self.windows
            .iter()
            .find(|w| w.rect.contains(i, j))
            .map(|w| w.id)
    }

    /// The tile under a point in on-screen logical pixels.
    pub fn tile_at(&self, x: f64, y: f64) -> (i32, i32) {
        let (tw, th) = self.tile_size();
        (
            ((x + self.view.0) / tw).floor() as i32,
            ((y + self.view.1) / th).floor() as i32,
        )
    }

    /// The canvas's invariants; every public operation keeps them.
    pub fn check(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        for w in &self.windows {
            if !ids.insert(w.id) {
                return Err(format!("window {} appears twice", w.id));
            }
            let (nw, nh) = self.need(w.min_w, w.min_h);
            if w.rect.w < nw || w.rect.h < nh {
                return Err(format!(
                    "window {} has {:?}, below its minimum span {nw}x{nh}",
                    w.id, w.rect
                ));
            }
        }
        for (a, wa) in self.windows.iter().enumerate() {
            for wb in &self.windows[a + 1..] {
                if wa.rect.overlaps(&wb.rect) {
                    return Err(format!(
                        "windows {} {:?} and {} {:?} overlap",
                        wa.id, wa.rect, wb.id, wb.rect
                    ));
                }
            }
        }
        let recent: HashSet<Id> = self.recent.iter().copied().collect();
        if recent.len() != self.recent.len() || recent != ids {
            return Err(format!(
                "recency {:?} is not the window ids once each",
                self.recent
            ));
        }
        match (self.focus, self.windows.is_empty()) {
            (None, false) => return Err("windows but no focus".into()),
            (Some(f), _) if self.index(f).is_none() => {
                return Err(format!("focus {f} is not a window"));
            }
            _ => {}
        }
        if let Some(r) = self.focus.and_then(|f| self.rect(f))
            && self.fits_on_screen(r.w, r.h)
            && !self.is_visible(r)
        {
            return Err(format!(
                "focused {r:?} is not fully on screen (view {:?})",
                self.view
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_refuses_a_partial_overlapping_or_empty_snapshot() {
        let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
        c.add(1, 100.0, 100.0, None);
        c.add(2, 100.0, 100.0, None);
        let before = c.snapshot();
        let r = |i| Rect::new(i, 0, 1, 1);
        assert!(!c.restore(&[(1, r(0))]), "a window missing");
        assert!(
            !c.restore(&[(1, r(0)), (3, r(5))]),
            "a window missing, one unknown named"
        );
        assert!(!c.restore(&[(1, r(0)), (2, r(0))]), "2 on 1");
        assert!(
            !c.restore(&[(1, r(0)), (2, Rect::new(3, 0, 0, 1))]),
            "an empty span"
        );
        assert_eq!(c.snapshot(), before);
        assert!(c.restore(&[(1, r(0)), (2, r(3))]));
        assert_eq!(c.rect(2), Some(r(3)));
    }

    /// F01 (Findings.repaired_push_keeps_their_order, C25): 2 lies left of 1 but after it in the
    /// window list, and 3 is put on both; the sweep in (lead, id) order keeps 2 before 1. The old
    /// queue moved them in list order and 1 then pushed 2 past it.
    #[test]
    fn push_keeps_the_order_of_the_windows_it_lands_on() {
        let mut c = Canvas::new(Config::default(), 1440.0, 930.0);
        c.add_at(1, 100.0, 100.0, Rect::new(1, 0, 1, 1));
        c.add_at(2, 100.0, 100.0, Rect::new(0, 0, 1, 1));
        c.add_at(3, 100.0, 100.0, Rect::new(0, 1, 2, 1));
        let k = c.index(3).unwrap();
        c.windows[k].rect = Rect::new(0, 0, 2, 1);
        c.push(3, Dir::Right);
        assert_eq!(c.rect(2), Some(Rect::new(2, 0, 1, 1)), "2 stays first");
        assert_eq!(c.rect(1), Some(Rect::new(3, 0, 1, 1)));
        assert_eq!(c.rect(3), Some(Rect::new(0, 0, 2, 1)), "the mover stays");
    }
}
