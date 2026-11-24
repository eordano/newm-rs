//! Replays seeded random operation sequences and fixed scenarios through the canvas, checking
//! its invariants after every step and each operation's own promise.

use newm_canvas::{Canvas, Config, Dir, Edge, Rect};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }
}

const SCREENS: [(f64, f64); 3] = [(1440.0, 930.0), (4096.0, 1122.0), (1920.0, 1050.0)];
const MINS: [(f64, f64); 5] = [
    (100.0, 100.0),
    (367.0, 132.0),
    (706.0, 513.0),
    (800.0, 300.0),
    (1500.0, 500.0),
];
const CONFIGS: [Config; 4] = [
    Config {
        tile_w: 720.0,
        tile_h: 465.0,
        zoom_step: 0.8,
        zoom_min: -2,
        zoom_max: 4,
        distance_weight: 0.5,
        diagonal_weight: 1.0,
    },
    Config {
        tile_w: 360.0,
        tile_h: 465.0,
        zoom_step: 0.8,
        zoom_min: -2,
        zoom_max: 4,
        distance_weight: 0.5,
        diagonal_weight: 1.0,
    },
    Config {
        tile_w: 600.0,
        tile_h: 400.0,
        zoom_step: 0.9,
        zoom_min: 0,
        zoom_max: 1,
        distance_weight: 0.5,
        diagonal_weight: 1.0,
    },
    Config {
        tile_w: 240.0,
        tile_h: 155.0,
        zoom_step: 0.8,
        zoom_min: -2,
        zoom_max: 7,
        distance_weight: 0.5,
        diagonal_weight: 1.0,
    },
];
const DIRS: [Dir; 4] = [Dir::Left, Dir::Right, Dir::Up, Dir::Down];
const EDGES: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];

#[derive(Debug, Clone, Copy)]
enum Op {
    Add(u64, usize, Option<(i32, i32)>),
    AddAt(u64, usize, Rect),
    Remove(usize),
    Focus(Dir),
    Resize(Edge, i32),
    Swap(Dir),
    MoveBy(usize, Dir),
    SendToEdge(Dir),
    MoveTo(usize, i32, i32),
    Zoom(i32),
    Pan(f64, f64),
    SetMin(usize, usize),
    SetConfig(usize),
    Rename(usize, u64),
    Undo,
    Redo,
    GatherIslands,
}

fn random_op(rng: &mut Rng, next_id: &mut u64) -> Op {
    match rng.below(29) {
        26 => Op::Undo,
        27 => Op::Redo,
        28 => Op::GatherIslands,
        24..=25 => {
            *next_id += 1;
            let at = Rect::new(
                rng.below(9) as i32 - 4,
                rng.below(5) as i32 - 2,
                rng.below(4) as i32 + 1,
                rng.below(3) as i32 + 1,
            );
            Op::AddAt(*next_id, rng.below(MINS.len() as u64) as usize, at)
        }
        0..=4 => {
            *next_id += 1;
            let want = (rng.below(2) == 0).then(|| (rng.below(9) as i32, rng.below(9) as i32));
            Op::Add(*next_id, rng.below(MINS.len() as u64) as usize, want)
        }
        22..=23 => Op::MoveBy(rng.below(64) as usize, rng.pick(&DIRS)),
        5..=6 => Op::Remove(rng.below(64) as usize),
        7..=9 => Op::Focus(rng.pick(&DIRS)),
        10..=13 => Op::Resize(rng.pick(&EDGES), if rng.below(2) == 0 { 1 } else { -1 }),
        14 => Op::Swap(rng.pick(&DIRS)),
        15 => Op::SendToEdge(rng.pick(&[Dir::Left, Dir::Right])),
        16 => Op::MoveTo(
            rng.below(64) as usize,
            rng.below(9) as i32 - 4,
            rng.below(5) as i32 - 2,
        ),
        17 => Op::Zoom(if rng.below(2) == 0 { 1 } else { -1 }),
        18 => Op::Pan(
            rng.below(2000) as f64 - 1000.0,
            rng.below(1000) as f64 - 500.0,
        ),
        19 => Op::SetConfig(rng.below(CONFIGS.len() as u64) as usize),
        20 => {
            *next_id += 1;
            let to = if rng.below(2) == 0 {
                *next_id
            } else {
                rng.below(*next_id)
            };
            Op::Rename(rng.below(64) as usize, to)
        }
        _ => Op::SetMin(
            rng.below(64) as usize,
            rng.below(MINS.len() as u64) as usize,
        ),
    }
}

fn nth(c: &Canvas, k: usize) -> Option<u64> {
    let n = c.windows().len();
    (n > 0).then(|| c.windows()[k % n].id)
}

fn rects(c: &Canvas) -> Vec<(u64, Rect)> {
    let mut v: Vec<_> = c.windows().iter().map(|w| (w.id, w.rect)).collect();
    v.sort_by_key(|x| x.0);
    v
}

/// Pairs ordered along `dir`'s axis before a push keep that order after it.
fn order_kept(before: &[(u64, Rect)], after: &Canvas, dir: Dir) -> Result<(), String> {
    for (a, ra) in before {
        for (b, rb) in before {
            let (Some(na), Some(nb)) = (after.rect(*a), after.rect(*b)) else {
                continue;
            };
            let (was, now) = match dir {
                Dir::Left | Dir::Right => (
                    ra.right() <= rb.i && ra.rows_overlap(rb),
                    na.right() <= nb.i,
                ),
                Dir::Up | Dir::Down => (
                    ra.bottom() <= rb.j && ra.cols_overlap(rb),
                    na.bottom() <= nb.j,
                ),
            };
            if was && !now {
                return Err(format!(
                    "{a} was before {b} along {dir:?} and is not after the push"
                ));
            }
        }
    }
    Ok(())
}

fn apply(c: &mut Canvas, op: Op) -> Result<(), String> {
    let zoom = c.zoom();
    let before = rects(c);
    let is_zoom = matches!(op, Op::Zoom(_) | Op::SetConfig(_));
    match op {
        Op::Add(id, m, want) => {
            let (w, h) = MINS[m];
            let r = c.add(id, w, h, want);
            if c.focus() != Some(id) {
                return Err("a new window is not focused".into());
            }
            if (r.w, r.h) != c.open_span(w, h, want) {
                return Err(format!("{r:?} is not the open span for {want:?}"));
            }
            unmoved(&before, c, &[id])?;
        }
        Op::AddAt(id, m, at) => {
            let (w, h) = MINS[m];
            let (sw, sh) = c.open_span(w, h, Some((at.w, at.h)));
            let wanted = Rect::new(at.i, at.j, sw, sh);
            let free = c.windows().iter().all(|o| !o.rect.overlaps(&wanted));
            let r = c.add_at(id, w, h, at);
            if c.focus() != Some(id) {
                return Err("a window put back is not focused".into());
            }
            if (r.w, r.h) != (sw, sh) {
                return Err(format!("{r:?} is not the open span for {at:?}"));
            }
            if free && r != wanted {
                return Err(format!("{at:?} was free, the window went to {r:?}"));
            }
            unmoved(&before, c, &[id])?;
        }
        Op::Remove(k) => {
            if let Some(id) = nth(c, k) {
                let (focused, recent) = (c.focus() == Some(id), c.recent().to_vec());
                c.remove(id);
                let next = recent.iter().copied().find(|&r| r != id);
                if focused && c.focus() != next {
                    return Err(format!(
                        "closing focused {id} focused {:?}, not {next:?}",
                        c.focus()
                    ));
                }
            }
        }
        Op::Focus(d) => {
            c.focus_dir(d);
        }
        Op::Resize(e, d) => {
            let f = c.focus();
            let old = f.and_then(|f| c.rect(f));
            let changed = c.resize_focused(e, d);
            if changed && d > 0 {
                let r = c.rect(f.unwrap()).unwrap();
                if !c.fits_at_1to1(r.w, r.h) {
                    return Err(format!("grew {r:?} past the screen at 1:1"));
                }
            }
            if changed && d < 0 && shrink_was_ocean(&before, f.unwrap(), old.unwrap(), e) {
                for (id, r) in &before {
                    if Some(*id) != f && c.rect(*id) != Some(*r) {
                        return Err(format!("an ocean shrink moved window {id}"));
                    }
                }
            }
        }
        Op::Swap(d) => {
            let pair = c.focus().and_then(|f| Some((f, c.neighbour(f, d)?)));
            if c.swap(d) {
                let (f, n) = pair.unwrap();
                let (fa, nb) = Canvas::swapped(
                    before.iter().find(|w| w.0 == f).unwrap().1,
                    before.iter().find(|w| w.0 == n).unwrap().1,
                    d,
                );
                if (c.rect(f), c.rect(n)) != (Some(fa), Some(nb)) {
                    return Err(format!(
                        "swap {d:?} left {f} at {:?} and {n} at {:?}",
                        c.rect(f),
                        c.rect(n)
                    ));
                }
            }
        }
        Op::MoveBy(k, d) => {
            if let Some(id) = nth(c, k) {
                let old = c.rect(id).unwrap();
                c.move_by(id, d);
                let (di, dj) = d.delta();
                let want = Rect::new(old.i + di, old.j + dj, old.w, old.h);
                if c.rect(id) != Some(want) {
                    return Err(format!("move_by {d:?} took {old:?} to {:?}", c.rect(id)));
                }
            }
        }
        Op::SendToEdge(d) => {
            c.send_to_edge(d);
        }
        Op::Undo => {
            c.undo();
        }
        Op::Redo => {
            c.redo();
        }
        Op::GatherIslands => {
            let (was, n) = (c.clone(), island_count(c));
            let moved = c.gather_islands();
            if n < 2 && *c != was {
                return Err(format!("gather_islands changed a canvas of {n} islands"));
            }
            if moved && island_count(c) >= n {
                return Err(format!(
                    "gather_islands moved, {n} islands left {}",
                    island_count(c)
                ));
            }
            if !moved && rects(c) != rects(&was) {
                return Err("gather_islands moved a window and said it did not".into());
            }
        }
        Op::MoveTo(k, i, j) => {
            if let Some(id) = nth(c, k) {
                c.move_to(id, i, j);
            }
        }
        Op::Zoom(d) => {
            let need = needs(c);
            let was = c.clone();
            let zoomed = c.zoom_by(d);
            unmoved(&before, c, &[])?;
            if need != needs(c) {
                return Err(format!("zoom {d} changed what windows need"));
            }
            focus_shown(c)?;
            if zoomed {
                middle_kept(&was, c)?;
                round_trip(&was, c, d)?;
            }
        }
        Op::Pan(dx, dy) => {
            c.scroll_by(dx, dy);
            c.focus_view_center();
        }
        Op::SetMin(k, m) => {
            if let Some(id) = nth(c, k) {
                let (w, h) = MINS[m];
                let was = c.windows().to_vec();
                let placed = c.set_min(id, w, h);
                if placed && newm_canvas::place::splits(&was, c.windows(), &[id]) {
                    return Err(format!("set_min of {id} split an island by the rules"));
                }
            }
        }
        Op::SetConfig(k) => {
            let config = CONFIGS[k];
            c.set_config(config);
            if c.zoom() != zoom.clamp(config.zoom_min, config.zoom_max) {
                return Err(format!(
                    "set_config moved the zoom from {zoom} to {}",
                    c.zoom()
                ));
            }
        }
        Op::Rename(k, to) => {
            if let Some(from) = nth(c, k) {
                let (taken, rect, focus) = (c.window(to).is_some(), c.rect(from), c.focus());
                if c.rename(from, to) == taken {
                    return Err(format!("rename {from} to {to} answered {}", !taken));
                }
                if !taken
                    && (c.rect(to) != rect || (focus == Some(from)) != (c.focus() == Some(to)))
                {
                    return Err(format!("rename {from} to {to} lost its rectangle or focus"));
                }
            }
        }
    }
    if c.focus() != c.recent().first().copied() {
        return Err(format!(
            "focus {:?} is not the most recent of {:?}",
            c.focus(),
            c.recent()
        ));
    }
    if !is_zoom && c.zoom() != zoom {
        return Err(format!(
            "{op:?} changed the zoom from {zoom} to {}",
            c.zoom()
        ));
    }
    if !matches!(op, Op::Resize(..) | Op::Undo | Op::Redo) {
        spans_kept(&before, c)?;
    }
    c.check()
}

/// How many islands the windows make.
fn island_count(c: &Canvas) -> usize {
    let label = newm_canvas::place::islands(c.windows(), &[]);
    label
        .values()
        .collect::<std::collections::HashSet<_>>()
        .len()
}

/// C47: outside a resize every window keeps its span, raised only to what its minimum needs now.
fn spans_kept(before: &[(u64, Rect)], c: &Canvas) -> Result<(), String> {
    for &(id, old) in before {
        let Some(w) = c.window(id) else {
            continue;
        };
        let (nw, nh) = c.need(w.min_w, w.min_h);
        if (w.rect.w, w.rect.h) != (old.w.max(nw), old.h.max(nh)) {
            return Err(format!("{id} went from {old:?} to {:?}", w.rect));
        }
    }
    Ok(())
}

/// The span each window's minimum needs.
fn needs(c: &Canvas) -> Vec<(u64, (i32, i32))> {
    c.windows()
        .iter()
        .map(|w| (w.id, c.need(w.min_w, w.min_h)))
        .collect()
}

/// The focused window is on screen as far as it fits: along each axis it fits on, whole; along
/// an axis it is larger than the screen on, from its start.
fn focus_shown(c: &Canvas) -> Result<(), String> {
    let Some(r) = c.focus().and_then(|f| c.rect(f)) else {
        return Ok(());
    };
    let (x, y, w, h) = c.screen_px(r);
    let (sw, sh) = c.screen();
    let shown = |start: f64, len: f64, screen: f64| {
        if len > screen + 1e-6 {
            start.abs() < 1e-6
        } else {
            start >= -1e-6 && start + len <= screen + 1e-6
        }
    };
    if shown(x, w, sw) && shown(y, h, sh) {
        Ok(())
    } else {
        Err(format!(
            "focused {r:?} is at {:?} on a {sw}x{sh} screen",
            (x, y, w, h)
        ))
    }
}

/// The 1:1 canvas point under the middle of the screen.
fn middle(c: &Canvas) -> (f64, f64) {
    let ((vx, vy), (sw, sh), f) = (c.view(), c.screen(), c.scale());
    ((vx + sw / 2.0) / f, (vy + sh / 2.0) / f)
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-6 * (1.0 + x.abs());
    close(a.0, b.0) && close(a.1, b.1)
}

/// Whether the focus lies whole on screen at `view`, clear of the screen's edges: there the view
/// needs no scroll to show it.
fn whole_at(c: &Canvas, view: (f64, f64)) -> bool {
    let Some(r) = c.focus().and_then(|f| c.rect(f)) else {
        return true;
    };
    let (x, y, w, h) = c.canvas_px(r);
    let (sw, sh) = c.screen();
    let inside =
        |start: f64, len: f64, v: f64, s: f64| start > v + 1e-6 && start + len < v + s - 1e-6;
    inside(x, w, view.0, sw) && inside(y, h, view.1, sh)
}

/// A zoom keeps the canvas point under the middle of the screen when the focus lies whole on
/// screen at the view that keeps it: then showing the focus scrolls nothing.
fn middle_kept(was: &Canvas, c: &Canvas) -> Result<(), String> {
    let (mid, (sw, sh), f) = (middle(was), c.screen(), c.scale());
    let kept = (mid.0 * f - sw / 2.0, mid.1 * f - sh / 2.0);
    if whole_at(c, kept) && !near(middle(c), mid) {
        return Err(format!(
            "zoom {} -> {} moved the middle of the screen from {mid:?} to {:?}, the focus on screen",
            was.zoom(),
            c.zoom(),
            middle(c)
        ));
    }
    Ok(())
}

/// Zooming back: from a view the focus is shown at (one that `ensure_focus_visible` leaves),
/// pulling back and coming closer again returns to it; coming closer and pulling back again
/// returns to it when coming closer kept the middle of the screen (did not scroll).
fn round_trip(was: &Canvas, c: &Canvas, d: i32) -> Result<(), String> {
    let mut shown = was.clone();
    shown.ensure_focus_visible();
    let mut back = c.clone();
    if shown.view() != was.view() || c.zoom() - was.zoom() != d || !back.zoom_by(-d) {
        return Ok(());
    }
    if (d > 0 || near(middle(c), middle(was))) && !near(back.view(), was.view()) {
        return Err(format!(
            "zoom {d} and back left the view at {:?}, not {:?}",
            back.view(),
            was.view()
        ));
    }
    Ok(())
}

/// Every window but `except` keeps its rectangle.
fn unmoved(before: &[(u64, Rect)], after: &Canvas, except: &[u64]) -> Result<(), String> {
    for (id, r) in before {
        if !except.contains(id) && after.rect(*id) != Some(*r) {
            return Err(format!(
                "window {id} moved from {r:?} to {:?}",
                after.rect(*id)
            ));
        }
    }
    Ok(())
}

/// Whether the strip a one-tile shrink of `f` frees is ocean: no other window lies against its
/// far side.
fn shrink_was_ocean(before: &[(u64, Rect)], f: u64, old: Rect, e: Edge) -> bool {
    !before.iter().any(|(id, r)| {
        *id != f
            && match e {
                Edge::Left => r.right() == old.i && r.rows_overlap(&old),
                Edge::Right => r.i == old.right() && r.rows_overlap(&old),
                Edge::Top => r.bottom() == old.j && r.cols_overlap(&old),
                Edge::Bottom => r.j == old.bottom() && r.cols_overlap(&old),
            }
    })
}

#[test]
fn random_sequences_keep_the_invariants() {
    for seed in 1..=400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (w, h) = SCREENS[(seed % 3) as usize];
        let config = CONFIGS[(seed / 3 % CONFIGS.len() as u64) as usize];
        let mut c = Canvas::new(config, w, h);
        let mut next_id = 0;
        let mut trace = Vec::new();
        for _ in 0..200 {
            let op = random_op(&mut rng, &mut next_id);
            trace.push(op);
            if let Err(e) = apply(&mut c, op) {
                panic!(
                    "seed {seed}: {e}\ntrace: {trace:?}\ncanvas: {:?}",
                    rects(&c)
                );
            }
        }
    }
}

/// A laptop screen: 1440x930 at the default config.
fn laptop() -> Canvas {
    Canvas::new(Config::default(), 1440.0, 930.0)
}

#[test]
fn add_at_puts_a_window_back_on_its_tiles_or_the_nearest_free_ones() {
    let mut c = laptop();
    assert_eq!(
        c.add_at(1, 0.0, 0.0, Rect::new(3, 1, 2, 1)),
        Rect::new(3, 1, 2, 1)
    );
    assert_eq!(
        c.add_at(2, 0.0, 0.0, Rect::new(-2, 0, 1, 2)),
        Rect::new(-2, 0, 1, 2)
    );
    assert_eq!(c.focus(), Some(2));
    assert_eq!(
        c.add_at(3, 0.0, 0.0, Rect::new(3, 1, 2, 1)),
        Rect::new(3, 0, 2, 1),
        "taken: the nearest free spot, the upper one first"
    );
    assert_eq!(
        c.add_at(4, 0.0, 0.0, Rect::new(10, 5, 3, 1)),
        Rect::new(10, 5, 2, 1),
        "cut to the span that fits on the screen at 1:1"
    );
    assert_eq!(
        c.add_at(5, 800.0, 0.0, Rect::new(-2, 0, 1, 1)),
        Rect::new(-2, -1, 2, 1),
        "widened to its minimum, on the nearest free tiles"
    );
    assert_eq!(c.rect(1), Some(Rect::new(3, 1, 2, 1)));
    assert_eq!(c.rect(2), Some(Rect::new(-2, 0, 1, 2)));
    c.check().unwrap();
}

#[test]
fn a_laptop_screen_shows_two_by_two_tiles() {
    let c = laptop();
    assert!(c.fits_at_1to1(2, 2));
    assert!(!c.fits_at_1to1(3, 1));
    assert!(c.fits_on_screen(2, 2));
}

/// Zooms a fresh canvas to `zoom`.
fn zoomed(mut c: Canvas, zoom: i32) -> Canvas {
    assert!(zoom == 0 || c.zoom_by(zoom));
    assert_eq!(c.zoom(), zoom);
    c
}

#[test]
fn grow_left_pushes_the_neighbour_and_keeps_the_zoom() {
    let mut c = laptop();
    c.add(1, 100.0, 100.0, None);
    c.add(2, 100.0, 100.0, None);
    assert_eq!(c.rect(2), Some(Rect::new(1, 0, 1, 1)));
    let before = rects(&c);
    assert!(c.resize_focused(Edge::Left, 1));
    order_kept(&before, &c, Dir::Left).unwrap();
    assert_eq!(c.rect(2), Some(Rect::new(0, 0, 2, 1)));
    assert_eq!(c.rect(1), Some(Rect::new(-1, 0, 1, 1)));
    assert_eq!(c.zoom(), 0);
    c.check().unwrap();
}

#[test]
fn resize_stops_at_the_screen_edge_at_1to1_at_every_zoom() {
    for zoom in [-2, -1, 0, 1, 4] {
        let mut c = zoomed(laptop(), zoom);
        c.add(1, 100.0, 100.0, None);
        assert!(c.resize_focused(Edge::Right, 1));
        assert!(
            !c.resize_focused(Edge::Right, 1),
            "3 tiles of 720 do not fit in 1440 at zoom {zoom}"
        );
        assert!(c.resize_focused(Edge::Bottom, 1));
        assert!(!c.resize_focused(Edge::Bottom, 1));
        assert_eq!(c.rect(1), Some(Rect::new(0, 0, 2, 2)));
        assert_eq!(c.zoom(), zoom);
        c.check().unwrap();
    }
}

#[test]
fn a_large_minimum_takes_the_same_tiles_at_every_zoom() {
    for zoom in [-2, 0, 4] {
        let mut c = zoomed(laptop(), zoom);
        assert_eq!(c.need(367.0, 132.0), (1, 1), "zoom {zoom}");
        let r = c.add(1, 800.0, 500.0, None);
        assert_eq!(
            (r.w, r.h),
            (2, 2),
            "800x500 needs 2x2 tiles of 720x465 at zoom {zoom}"
        );
    }
}

#[test]
fn a_zoom_never_pushes_even_when_a_minimum_is_large() {
    let mut c = laptop();
    c.add(1, 700.0, 450.0, None);
    c.add(2, 100.0, 100.0, None);
    c.focus_id(1);
    let before = rects(&c);
    for d in [1, 1, 1, 1, -1, -1, -1, -1, -1, -1] {
        assert!(c.zoom_by(d));
        unmoved(&before, &c, &[]).unwrap();
        c.check().unwrap();
    }
    assert_eq!(c.zoom(), -2);
}

#[test]
fn magnifying_past_1to1_shows_the_focus_with_the_least_movement() {
    let mut c = laptop();
    c.add(1, 1400.0, 900.0, None);
    c.add_at(2, 100.0, 100.0, Rect::new(2, 0, 1, 1));
    c.focus_id(1);
    assert_eq!(c.rect(1), Some(Rect::new(0, 0, 2, 2)));
    assert!(c.zoom_by(-2), "magnifying is never refused");
    assert_eq!(c.rect(1), Some(Rect::new(0, 0, 2, 2)));
    assert!(!c.fits_on_screen(2, 2));
    assert_eq!(
        c.view(),
        (0.0, 0.0),
        "larger than the screen: shown from its top-left"
    );
    c.focus_id(2);
    let (tw, _) = c.tile_size();
    assert!(
        (c.view().0 - (3.0 * tw - 1440.0)).abs() < 1e-9 && c.view().1 == 0.0,
        "2 ends at the screen's right edge: the least scroll, not {:?}",
        c.view()
    );
    c.check().unwrap();
    c.focus_id(1);
    assert_eq!(c.view(), (0.0, 0.0));
}

#[track_caller]
fn assert_view(c: &Canvas, want: (f64, f64), why: &str) {
    let v = c.view();
    assert!(
        (v.0 - want.0).abs() < 1e-6 && (v.1 - want.1).abs() < 1e-6,
        "{why}: the view is {v:?}, not {want:?}"
    );
}

/// Three tiles in a row on 2560x1440: pulling back keeps the canvas point under the middle of the
/// screen (1280, 720) there, so the row is drawn about the middle instead of crowded into the
/// top-left, and coming closer again returns to the view. Coming closer first scrolls to show the
/// focus; pulling back then keeps that scroll, scaled.
#[test]
fn a_zoom_keeps_the_middle_of_the_screen() {
    let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
    for id in 1..=3 {
        c.add(id, 100.0, 100.0, None);
    }
    assert_eq!(c.rect(3), Some(Rect::new(2, 0, 1, 1)));
    assert_eq!(c.view(), (0.0, 0.0));
    assert!(c.zoom_by(1));
    assert_view(
        &c,
        (-256.0, -144.0),
        "(1280, 720) at 0.8, less the half screen",
    );
    assert!(c.zoom_by(1));
    assert_view(
        &c,
        (-460.8, -259.2),
        "(1280, 720) at 0.64, less the half screen",
    );
    assert!(c.is_visible(c.rect(3).unwrap()));
    assert!(c.zoom_by(-2));
    assert_view(
        &c,
        (0.0, 0.0),
        "pulled back and closer again: the same view",
    );
    assert!(c.zoom_by(-1));
    assert_view(
        &c,
        (320.0, 0.0),
        "at 1.25 the middle leaves 3's top 180 px off screen: the least scroll shows it",
    );
    assert!(c.zoom_by(1));
    assert_view(
        &c,
        (0.0, -144.0),
        "pulled back again, the 180 px scroll at 1.25 is 144 px at 1:1",
    );
    c.check().unwrap();
}

/// A laptop's 1428x920 canvas with 119x115 tiles: three 4x4 windows fill its top half. Pulled back
/// two steps they are drawn about the middle of the screen, (714, 460) staying where it is.
#[test]
fn a_laptop_canvas_pulls_back_about_the_middle() {
    let config = Config {
        tile_w: 119.0,
        tile_h: 115.0,
        ..Config::default()
    };
    let mut c = Canvas::new(config, 1428.0, 920.0);
    for id in 1..=3 {
        c.add(id, 476.0, 460.0, None);
    }
    assert_eq!(c.rect(3), Some(Rect::new(8, 0, 4, 4)));
    assert_eq!(c.view(), (0.0, 0.0));
    assert!(c.zoom_by(2));
    assert_view(
        &c,
        (-257.04, -165.6),
        "(714, 460) at 0.64, less the half screen",
    );
    let (x, _, w, _) = c.screen_px(c.rect(1).unwrap());
    let (x3, _, w3, _) = c.screen_px(c.rect(3).unwrap());
    assert!(
        (x - (1428.0 - (x3 + w3))).abs() < 1e-6 && w == w3,
        "the row is centred: as much room left of 1 as right of 3"
    );
    assert!(c.zoom_by(-2));
    assert_view(
        &c,
        (0.0, 0.0),
        "pulled back and closer again: the same view",
    );
    assert!(c.zoom_by(-1));
    assert_view(
        &c,
        (357.0, 0.0),
        "at 1.25, the least scroll from (178.5, 115) to show 3",
    );
    assert!(c.zoom_by(1));
    assert_view(
        &c,
        (142.8, -92.0),
        "the scroll of (178.5, -115) at 1.25, at 1:1",
    );
    c.check().unwrap();
}

#[test]
fn shrinking_closes_a_lake() {
    let mut c = Canvas::new(Config::default(), 4096.0, 1122.0);
    c.add(1, 100.0, 100.0, None);
    c.resize(1, Edge::Right, 1);
    c.add(2, 100.0, 100.0, None);
    assert_eq!(c.rect(2), Some(Rect::new(2, 0, 1, 1)));
    c.focus_id(1);
    assert!(c.resize_focused(Edge::Right, -1));
    assert_eq!(
        c.rect(2),
        Some(Rect::new(1, 0, 1, 1)),
        "the gap between 1 and 2 was a lake"
    );
}

#[test]
fn shrinking_leaves_ocean() {
    let mut c = Canvas::new(Config::default(), 4096.0, 1122.0);
    c.add(1, 100.0, 100.0, None);
    c.resize(1, Edge::Bottom, 1);
    c.add(2, 100.0, 100.0, None);
    assert_eq!(c.rect(2), Some(Rect::new(1, 0, 1, 1)));
    c.focus_id(1);
    assert!(!c.resize_focused(Edge::Right, 0));
    c.resize(1, Edge::Right, 1);
    assert_eq!(
        c.rect(2),
        Some(Rect::new(2, 0, 1, 1)),
        "growing pushed 2 right"
    );
    assert!(c.resize(1, Edge::Top, -1));
    assert_eq!(
        c.rect(2),
        Some(Rect::new(2, 0, 1, 1)),
        "the freed top row is open above: ocean"
    );
}

#[test]
fn closing_a_window_closes_its_lake() {
    let mut c = Canvas::new(Config::default(), 4096.0, 1122.0);
    for id in 1..=3 {
        c.add(id, 100.0, 100.0, None);
    }
    c.remove(2);
    assert_eq!(c.rect(3), Some(Rect::new(1, 0, 1, 1)));
    assert_eq!(c.focus(), Some(3));
}

#[test]
fn the_focused_window_is_shown_with_the_least_scroll() {
    let mut c = laptop();
    for id in 1..=4 {
        c.add_at(id, 100.0, 100.0, Rect::new(id as i32 - 1, 0, 1, 1));
    }
    assert_eq!(
        c.view(),
        (720.0 * 2.0, 0.0),
        "four tiles in a row: the last two are on screen"
    );
    c.focus_dir(Dir::Left);
    assert_eq!(
        c.view(),
        (720.0 * 2.0, 0.0),
        "the third is already visible: no scroll"
    );
    c.focus_dir(Dir::Left);
    assert_eq!(c.view(), (720.0, 0.0));
}

#[test]
fn neighbours_share_physical_edges_at_fractional_scale() {
    let mut c = Canvas::new(Config::default(), 4096.0, 1122.0);
    c.add(1, 100.0, 100.0, None);
    c.add(2, 100.0, 100.0, None);
    c.scroll_by(13.37, 7.7);
    for zoom in [0, 1, 2, 3] {
        while c.zoom() < zoom {
            c.zoom_by(1);
        }
        let a = c.physical(c.rect(1).unwrap(), 1.25);
        let b = c.physical(c.rect(2).unwrap(), 1.25);
        assert_eq!(
            a.0 + a.2,
            b.0,
            "no gap or overlap between neighbours at zoom {zoom}"
        );
    }
}

/// F01 (proof/Findings.v, move_to_reverses_the_windows_it_lands_on): 3 is dropped on 2 and 1,
/// which lie side by side in that order; push moves both to 3's right edge, then 2 pushes 1.
#[test]
fn a_drop_lands_where_it_was_dropped_and_splits_nothing() {
    let mut c = laptop();
    c.add_at(2, 100.0, 100.0, Rect::new(0, 0, 1, 1));
    c.add_at(1, 100.0, 100.0, Rect::new(1, 0, 1, 1));
    c.add_at(3, 100.0, 100.0, Rect::new(0, 1, 1, 1));
    assert!(c.resize(3, Edge::Right, 1));
    let before = c.windows().to_vec();
    assert!(c.move_to(3, 0, 0));
    assert_eq!(
        c.rect(3),
        Some(Rect::new(0, 0, 2, 1)),
        "3 lands where it was dropped"
    );
    assert!(!newm_canvas::place::splits(&before, c.windows(), &[3]));
    for w in &before {
        let r = c.rect(w.id).unwrap();
        assert_eq!((r.w, r.h), (w.rect.w, w.rect.h), "{} kept its span", w.id);
    }
    c.check().unwrap();
}

/// F02 (proof/Findings.v, zoom_out_leaves_the_focus_wider_than_the_screen): zoom_by used to
/// widen the focus through enforce_min. A zoom now changes no window's tiles, so the focus keeps
/// the span that fits at 1:1, and pulling back shows it whole.
#[test]
fn zooming_never_leaves_the_focus_wider_than_the_screen() {
    let mut c = laptop();
    c.add(1, 1400.0, 100.0, None);
    let r = c.rect(1).unwrap();
    assert!(c.fits_at_1to1(r.w, r.h));
    assert!(c.zoom_by(1));
    assert_eq!(c.rect(1), Some(r));
    assert!(c.fits_on_screen(r.w, r.h));
    assert!(c.is_visible(r));
}

#[test]
fn set_config_clamps_the_zoom_and_enforces_minimums() {
    for ((lo, hi), z) in [((1, 4), 1), ((-2, -1), -1), ((-2, 4), 0)] {
        let mut c = laptop();
        c.set_config(Config {
            zoom_min: lo,
            zoom_max: hi,
            ..Config::default()
        });
        assert_eq!(c.zoom(), z);
    }
    let mut c = laptop();
    c.add(1, 700.0, 100.0, None);
    c.add(2, 100.0, 100.0, None);
    c.set_config(Config {
        tile_w: 360.0,
        ..Config::default()
    });
    c.add(3, 720.0, 465.0, None);
    assert_eq!(c.rect(1), Some(Rect::new(0, 0, 2, 1)));
    assert_eq!(c.rect(2), Some(Rect::new(2, 0, 1, 1)));
    assert_eq!(
        c.rect(3),
        Some(Rect::new(2, 1, 2, 1)),
        "below 2: right of it the view would scroll"
    );
    c.check().unwrap();
}

#[test]
fn rename_moves_the_id_and_the_focus() {
    let mut c = laptop();
    c.add(1, 100.0, 100.0, None);
    c.add(2, 100.0, 100.0, None);
    assert!(!c.rename(1, 2));
    assert!(!c.rename(5, 6));
    let mut d = c.clone();
    assert!(c.rename(2, 7));
    assert_eq!(
        (c.rect(7), c.rect(2), c.focus()),
        (Some(Rect::new(1, 0, 1, 1)), None, Some(7))
    );
    assert!(d.rename(1, 7));
    assert_eq!(
        (d.rect(7), d.focus()),
        (Some(Rect::new(0, 0, 1, 1)), Some(2))
    );
    c.check().unwrap();
    d.check().unwrap();
}

fn small(w: f64, h: f64) -> Canvas {
    Canvas::new(CONFIGS[3], w, h)
}

#[test]
fn move_by_lets_the_neighbour_take_the_freed_tile() {
    let mut c = small(2560.0, 1440.0);
    for id in 1..=3 {
        c.add(id, 100.0, 100.0, None);
    }
    c.focus_id(1);
    assert!(c.move_by(1, Dir::Right));
    assert_eq!(
        (c.rect(1), c.rect(2), c.rect(3)),
        (
            Some(Rect::new(1, 0, 1, 1)),
            Some(Rect::new(0, 0, 1, 1)),
            Some(Rect::new(2, 0, 1, 1))
        ),
        "2 takes the tile 1 left (2 tiles reassigned) instead of the row being pushed (3)"
    );
    assert!(c.move_by(1, Dir::Down));
    assert_eq!(c.rect(1), Some(Rect::new(1, 1, 1, 1)));
    assert_eq!(
        c.rect(3),
        Some(Rect::new(2, 0, 1, 1)),
        "nothing was below 1"
    );
    assert!(c.undo() && c.undo());
    assert_eq!(
        c.rect(1),
        Some(Rect::new(0, 0, 1, 1)),
        "two undos put 1 back"
    );
    assert_eq!(c.rect(2), Some(Rect::new(1, 0, 1, 1)));
    assert!(c.redo());
    assert_eq!(
        c.rect(1),
        Some(Rect::new(1, 0, 1, 1)),
        "a redo moves it again"
    );
    assert!(!c.move_by(9, Dir::Left));
    c.check().unwrap();
}

#[test]
fn swap_keeps_both_spans_and_pushes_what_is_in_the_way() {
    let mut c = small(2560.0, 1440.0);
    c.add(1, 100.0, 100.0, None);
    assert!(c.resize(1, Edge::Right, 1) && c.resize(1, Edge::Bottom, 1));
    c.add(2, 100.0, 100.0, None);
    c.add(3, 100.0, 100.0, None);
    assert!(c.move_to(3, 2, 1));
    c.focus_id(1);
    assert!(c.swap(Dir::Right));
    assert_eq!(
        (c.rect(1), c.rect(2), c.rect(3)),
        (
            Some(Rect::new(1, 0, 2, 2)),
            Some(Rect::new(0, 0, 1, 1)),
            Some(Rect::new(0, 1, 1, 1))
        ),
        "1 ends where 2 ended and 2 starts where 1 started, both at their spans; 3 moves into the tile 1 left, which costs no tile more"
    );
    assert!(c.swap(Dir::Left));
    assert_eq!(
        (c.rect(1), c.rect(2)),
        (Some(Rect::new(0, 0, 2, 2)), Some(Rect::new(2, 0, 1, 1))),
        "swapping back puts both where they were"
    );
    c.check().unwrap();
}

#[test]
fn new_windows_take_the_nearest_free_spot_the_view_scrolls_least_to() {
    let mut c = small(2560.0, 1440.0);
    let mut spots = Vec::new();
    for id in 1..=7 {
        c.focus_id(1);
        let before = rects(&c);
        spots.push(c.add(id, 100.0, 100.0, None));
        unmoved(&before, &c, &[id]).unwrap();
        c.check().unwrap();
    }
    let at = |i, j| Rect::new(i, j, 1, 1);
    assert_eq!(
        spots,
        [
            at(0, 0),
            at(1, 0),
            at(0, 1),
            at(0, -1),
            at(-1, 0),
            at(1, -1),
            at(1, 1)
        ],
        "beside 1 first, where the view scrolls least (left and above scroll it), then the corners"
    );
}

#[test]
fn a_crowded_row_gets_the_nearest_free_spot() {
    let mut c = small(2560.0, 1440.0);
    for id in 1..=5 {
        c.focus_id(1);
        c.add(id, 100.0, 100.0, None);
    }
    c.move_to(2, 2, 0);
    c.focus_id(1);
    let before = rects(&c);
    let r = c.add(6, 100.0, 100.0, Some((2, 1)));
    unmoved(&before, &c, &[6]).unwrap();
    assert_eq!(
        r,
        Rect::new(1, -1, 2, 1),
        "the nearest free spot that fits two tiles: on 1's top side"
    );
    c.check().unwrap();
}

#[test]
fn a_new_window_opens_at_its_wanted_span() {
    let mut c = small(2560.0, 1440.0);
    assert_eq!(c.add(1, 380.0, 145.0, Some((7, 7))), Rect::new(0, 0, 7, 7));
    assert_eq!(
        c.add(2, 380.0, 145.0, None),
        Rect::new(7, 0, 2, 1),
        "no wanted span: the minimum's"
    );
    assert_eq!(
        c.open_span(0.0, 0.0, Some((50, 50))),
        (10, 9),
        "cut to what fits on 2560x1440"
    );
    assert_eq!(
        c.open_span(1000.0, 100.0, Some((1, 0))),
        (5, 1),
        "never below the minimum"
    );
    for zoom in [-2, 3, 7] {
        let c = zoomed(small(2560.0, 1440.0), zoom);
        assert_eq!(
            c.open_span(0.0, 0.0, Some((50, 50))),
            (10, 9),
            "cut to what fits at 1:1 at zoom {zoom}"
        );
    }
}

#[test]
fn closing_the_focus_returns_to_the_last_used_window() {
    let mut c = small(2560.0, 1440.0);
    for id in 1..=3 {
        c.add(id, 100.0, 100.0, None);
    }
    c.focus_id(1);
    c.focus_id(3);
    assert_eq!(c.recent(), [3, 1, 2]);
    c.remove(3);
    assert_eq!(c.focus(), Some(1), "1 was used last; 2 is only nearer");
    assert!(c.rename(2, 9));
    assert_eq!(c.recent(), [1, 9]);
    c.remove(1);
    assert_eq!(c.focus(), Some(9));
    c.remove(9);
    assert_eq!((c.focus(), c.recent()), (None, &[][..]));
    c.check().unwrap();
}

#[test]
fn ghostty_takes_two_small_tiles() {
    let mut c = small(2560.0, 1440.0);
    assert_eq!(c.need(367.0, 132.0), (2, 1));
    assert_eq!(c.need(380.0, 145.0), (2, 1), "padded by the gaps");
    assert!(c.zoom_by(3));
    assert_eq!(
        c.need(367.0, 132.0),
        (2, 1),
        "the zoom never changes what a window needs"
    );
    c.add(1, 367.0, 132.0, None);
    assert!(c.zoom_by(-5));
    assert_eq!(
        c.rect(1),
        Some(Rect::new(0, 0, 2, 1)),
        "spans never change on zoom"
    );
    c.check().unwrap();
}

#[test]
fn undo_raises_a_window_to_a_minimum_set_since() {
    let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
    c.add(1, 100.0, 100.0, None);
    c.add(2, 100.0, 100.0, None);
    assert!(c.move_by(2, Dir::Right));
    c.set_min(2, 1400.0, 100.0);
    assert!(c.undo());
    assert_eq!(
        c.rect(2).map(|r| (r.w, r.h)),
        Some((2, 1)),
        "the layout from before the move, with 2 at the two tiles its minimum needs now"
    );
    c.check().unwrap();
}

#[test]
fn place_accepts_only_layouts_that_keep_the_rules() {
    let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
    for (id, i) in [(1, 0), (2, 1), (3, 2)] {
        c.add_at(id, 100.0, 100.0, Rect::new(i, 0, 1, 1));
    }
    let r = |i, j| Rect::new(i, j, 1, 1);
    let t = [(1, r(1, 0))];
    assert!(
        c.accepts(&t, &[r(1, 0), r(0, 0), r(2, 0)]),
        "2 takes the tile 1 left"
    );
    assert!(
        !c.accepts(&t, &[r(1, 0), r(0, 0)]),
        "one rectangle per window"
    );
    assert!(
        !c.accepts(&t, &[r(0, 0), r(1, 0), r(2, 0)]),
        "1 off its target"
    );
    assert!(
        !c.accepts(&t, &[r(1, 0), Rect::new(-1, 0, 2, 1), r(2, 0)]),
        "2's span changed"
    );
    assert!(!c.accepts(&t, &[r(1, 0), r(1, 0), r(2, 0)]), "2 on 1");
    assert!(
        !c.accepts(&t, &[r(1, 0), r(0, 5), r(2, 0)]),
        "2 cut off from 3"
    );
}

#[test]
fn closing_never_splits_an_island() {
    let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
    c.add_at(1, 800.0, 300.0, Rect::new(0, 0, 2, 1));
    c.add_at(2, 800.0, 300.0, Rect::new(2, 0, 2, 1));
    c.add_at(3, 100.0, 100.0, Rect::new(2, -1, 1, 1));
    let before = c.windows().to_vec();
    assert!(c.remove(1));
    assert_eq!(
        c.rect(2),
        Some(Rect::new(2, 0, 2, 1)),
        "sliding 2 into the lake would part it from 3"
    );
    assert!(!newm_canvas::place::splits(&before, c.windows(), &[1]));
    c.check().unwrap();
}

#[test]
fn a_grown_minimum_is_placed_without_splitting_an_island() {
    let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
    let (_, th) = c.base_tile();
    c.add_at(1, 100.0, 100.0, Rect::new(0, 0, 1, 1));
    c.add_at(2, 100.0, 100.0, Rect::new(0, 1, 2, 1));
    c.add_at(3, 100.0, 100.0, Rect::new(2, 1, 1, 1));
    let before = c.windows().to_vec();
    assert!(c.set_min(1, 100.0, th * 1.5), "the rules place the growth");
    assert_eq!(
        c.rect(1),
        Some(Rect::new(0, 0, 1, 2)),
        "1 grows down to its need"
    );
    assert!(
        !newm_canvas::place::splits(&before, c.windows(), &[1]),
        "pushing 2 down alone would part it from 3: {:?}",
        c.windows()
    );
    assert_eq!(
        c.rect(2).map(|r| (r.w, r.h)),
        Some((2, 1)),
        "2 keeps its span"
    );
    assert_eq!(
        c.rect(3).map(|r| (r.w, r.h)),
        Some((1, 1)),
        "3 keeps its span"
    );
    c.check().unwrap();
}

#[test]
fn gathering_makes_one_island_and_undo_takes_it_back() {
    let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
    for (id, at) in [
        (1, Rect::new(0, 0, 2, 1)),
        (2, Rect::new(2, 0, 1, 2)),
        (3, Rect::new(6, 0, 1, 1)),
        (4, Rect::new(0, 4, 1, 1)),
        (5, Rect::new(1, 4, 1, 1)),
    ] {
        c.add_at(id, 100.0, 100.0, at);
    }
    assert_eq!(island_count(&c), 3);
    let before = rects(&c);
    assert!(c.gather_islands());
    assert_eq!(island_count(&c), 1, "{:?}", rects(&c));
    spans_kept(&before, &c).unwrap();
    c.check().unwrap();
    assert_eq!(
        c.rect(1),
        Some(Rect::new(0, 0, 2, 1)),
        "the largest island stays"
    );
    assert_eq!(
        c.rect(2),
        Some(Rect::new(2, 0, 1, 2)),
        "the largest island stays"
    );
    assert_eq!(c.rect(3), Some(Rect::new(3, 0, 1, 1)), "3 slides left to 2");
    assert_eq!(
        c.rect(4),
        Some(Rect::new(0, 1, 1, 1)),
        "4 and 5 slide up to 1"
    );
    assert_eq!(
        c.rect(5),
        Some(Rect::new(1, 1, 1, 1)),
        "4 and 5 slide up to 1"
    );
    let gathered = rects(&c);
    assert!(c.undo());
    assert_eq!(rects(&c), before, "undo puts every window back");
    c.check().unwrap();
    assert!(c.redo());
    assert_eq!(rects(&c), gathered, "redo gathers again");
}

#[test]
fn one_island_is_left_as_it_is() {
    let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
    c.add_at(1, 100.0, 100.0, Rect::new(0, 0, 1, 1));
    c.add_at(2, 100.0, 100.0, Rect::new(1, 0, 1, 1));
    let was = c.clone();
    assert!(!c.gather_islands());
    assert_eq!(c, was);
    let mut empty = Canvas::new(Config::default(), 2560.0, 1440.0);
    assert!(!empty.gather_islands());
}

#[test]
fn islands_far_apart_or_across_a_diagonal_gather() {
    let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
    c.add_at(1, 100.0, 100.0, Rect::new(0, 0, 1, 1));
    c.add_at(2, 100.0, 100.0, Rect::new(100, 0, 1, 1));
    assert!(c.gather_islands());
    assert_eq!(
        c.rect(2),
        Some(Rect::new(1, 0, 1, 1)),
        "2 slides 99 tiles left"
    );
    c.check().unwrap();

    let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
    c.add_at(1, 100.0, 100.0, Rect::new(0, 0, 1, 1));
    c.add_at(2, 100.0, 100.0, Rect::new(5, 5, 1, 1));
    assert!(c.gather_islands());
    assert_eq!(
        c.rect(2),
        Some(Rect::new(1, 0, 1, 1)),
        "no straight slide reaches 1: 2 goes left, then up beside it"
    );
    c.check().unwrap();
}

/// Canvas.opens_right_below_left_above (C71): beside a focus the view shows whole, no side spot
/// needs a scroll, so the side decides: right, below, left, above, then the corners, right first.
#[test]
fn a_new_window_opens_right_below_left_above() {
    let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
    c.add_at(1, 100.0, 100.0, Rect::new(1, 1, 1, 1));
    for id in 2..=7 {
        c.focus_id(1);
        c.add(id, 100.0, 100.0, None);
    }
    let got: Vec<Rect> = (1..=7).map(|id| c.rect(id).unwrap()).collect();
    let r = |i, j| Rect::new(i, j, 1, 1);
    assert_eq!(
        got,
        [
            r(1, 1),
            r(2, 1),
            r(1, 2),
            r(0, 1),
            r(1, 0),
            r(2, 0),
            r(2, 2)
        ]
    );
    assert_eq!(c.view(), (0.0, 0.0), "none of them scrolled");
}
