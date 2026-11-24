//! Where windows go when one moves: the placement rules (newm-canvas proof F06-F08).
//!
//! 1. The layout costs the least: tiles reassigned (a tile whose owner, a window or nobody,
//!    changes), plus `distance_weight` per tile of window area moved one tile, plus
//!    `diagonal_weight` per tile of area of each window that changed both its row and its column.
//! 2. It splits no island: two windows connected without the moved ones before stay connected
//!    after, through any window. Windows connect through a shared side.
//! 3. A window moving diagonally is charged, not refused (the diagonal weight in rule 1).
//!
//! The moved windows sit on their targets; every other window keeps its span and nothing
//! overlaps. The search moves a window only because a placed one lands on it, and gives up past
//! [`BUDGET`] steps, when the caller falls back to pushing.
//!
//! The search asks the range tree of [`crate::index`] which windows a placed one lands on and
//! which lie under a candidate, counts each moved window's tiles from shared areas, and checks
//! islands by a flood through the tree: each answer is the one a scan of every window and tile
//! gave (proof: `Index.v`, `Islands.v`, `Cost.v`), so it decides the same layouts in far fewer
//! steps.

use std::collections::HashMap;

use crate::index::{flood, shared_area, Index};
use crate::{Canvas, Dir, Id, Rect, Window};

/// Search steps one placement may take.
pub const BUDGET: usize = 6_000;

/// How far from where it was a window that makes room is looked for, in tiles past its span.
const REACH: i32 = 2;

/// The four directions a part slides along to gather, in the order its moves are tried.
const SLIDES: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

/// A layout's cost against the one before it, in the terms of rule 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cost {
    pub tiles: usize,
    pub distance: f64,
    pub diagonal: f64,
}

impl Cost {
    pub fn total(&self) -> f64 {
        self.tiles as f64 + self.distance + self.diagonal
    }
}

/// Two windows touch when they share a side of positive length; a corner does not count.
pub fn touch(a: &Rect, b: &Rect) -> bool {
    ((a.right() == b.i || b.right() == a.i) && a.rows_overlap(b))
        || ((a.bottom() == b.j || b.bottom() == a.j) && a.cols_overlap(b))
}

/// The island of each window but `skip`, as the smallest id in it: a flood through the windows
/// sharing a side, which the range tree finds ([`Index::parts`]).
pub fn islands(ws: &[Window], skip: &[Id]) -> HashMap<Id, Id> {
    let list: Vec<Window> = ws
        .iter()
        .filter(|w| !skip.contains(&w.id))
        .copied()
        .collect();
    let rects: Vec<Rect> = list.iter().map(|w| w.rect).collect();
    labels(&list, &Index::new(&rects).parts(), |_| true)
}

/// The least id of each island `part` numbers, for the windows `keep` takes, by id.
fn labels(ws: &[Window], part: &[usize], keep: impl Fn(usize) -> bool) -> HashMap<Id, Id> {
    let mut least = vec![Id::MAX; ws.len()];
    for k in (0..ws.len()).filter(|&k| keep(k)) {
        least[part[k]] = least[part[k]].min(ws[k].id);
    }
    (0..ws.len())
        .filter(|&k| keep(k))
        .map(|k| (ws[k].id, least[part[k]]))
        .collect()
}

/// Rule 2 broken: two windows other than `movers`, connected without them before, that are not
/// connected after.
pub fn splits(before: &[Window], after: &[Window], movers: &[Id]) -> bool {
    split_by(before, &islands(before, movers), after)
}

/// [`splits`] from the islands before, `b`: one pass that remembers, for each island before, the
/// island after of its first window, and stops at a window of it found on another.
fn split_by(before: &[Window], b: &HashMap<Id, Id>, after: &[Window]) -> bool {
    split_labels(before, b, &islands(after, &[]))
}

/// [`split_by`] from the islands after, `a`.
fn split_labels(before: &[Window], b: &HashMap<Id, Id>, a: &HashMap<Id, Id>) -> bool {
    let mut first: HashMap<Id, Id> = HashMap::new();
    before.iter().any(|w| match (b.get(&w.id), a.get(&w.id)) {
        (Some(&lb), Some(&la)) => *first.entry(lb).or_insert(la) != la,
        _ => false,
    })
}

/// Whether `id` is the bridge: its neighbours fall on different islands without it.
pub fn is_bridge(ws: &[Window], id: Id) -> bool {
    let Some(w) = ws.iter().find(|w| w.id == id) else {
        return false;
    };
    let near: Vec<Id> = ws
        .iter()
        .filter(|o| o.id != id && touch(&o.rect, &w.rect))
        .map(|o| o.id)
        .collect();
    let label = islands(ws, &[id]);
    near.iter()
        .any(|a| near.iter().any(|b| label[a] != label[b]))
}

/// Keeps `moves` as the gather's best when they cost `c`, less than the best so far.
fn offer(best: &mut Option<(i32, Vec<(Id, Rect)>)>, c: i32, moves: Vec<(Id, Rect)>) {
    if best.as_ref().is_none_or(|b| c < b.0) {
        *best = Some((c, moves));
    }
}

/// No two of `rects` overlap: each overlaps none but itself.
pub fn disjoint(rects: &[Rect]) -> bool {
    disjoint_in(&Index::new(rects), rects)
}

fn disjoint_in(index: &Index, rects: &[Rect]) -> bool {
    let mut near = Vec::new();
    (0..rects.len()).all(|k| {
        index.overlapping_into(&rects[k], &mut near);
        near.iter().all(|&o| o == k)
    })
}

/// The windows `before` put on `rects` overlap nothing and split no island of `before`, whose
/// islands without the movers are `b` ([`disjoint`] and not [`split_by`]): one range tree over
/// `rects` answers both.
fn sound(before: &[Window], b: &HashMap<Id, Id>, rects: &[Rect]) -> bool {
    let index = Index::new(rects);
    if !disjoint_in(&index, rects) {
        return false;
    }
    let after: Vec<Window> = before
        .iter()
        .zip(rects)
        .map(|(w, r)| Window { rect: *r, ..*w })
        .collect();
    !split_labels(before, b, &labels(&after, &index.parts(), |_| true))
}

fn cells(r: &Rect) -> impl Iterator<Item = (i32, i32)> + '_ {
    (r.i..r.right()).flat_map(move |x| (r.j..r.bottom()).map(move |y| (x, y)))
}

fn contains(r: &Rect, (x, y): (i32, i32)) -> bool {
    r.i <= x && x < r.right() && r.j <= y && y < r.bottom()
}

/// The cost of moving from `before` to `after` (the same windows, in the same order), the
/// windows in `movers` exempt from the distance and diagonal terms.
pub fn cost(before: &[Window], after: &[Rect], movers: &[Id], dw: f64, gw: f64) -> Cost {
    let moved: Vec<usize> = (0..before.len())
        .filter(|&k| before[k].rect != after[k])
        .collect();
    let owner_before = |c: (i32, i32)| before.iter().position(|w| contains(&w.rect, c));
    let owner_after = |c: (i32, i32)| {
        moved
            .iter()
            .copied()
            .find(|&k| contains(&after[k], c))
            .or_else(|| owner_before(c).filter(|k| !moved.contains(k)))
    };
    let mut seen = std::collections::HashSet::new();
    let mut c = Cost::default();
    for &k in &moved {
        for cell in cells(&before[k].rect).chain(cells(&after[k])) {
            if seen.insert(cell) && owner_before(cell) != owner_after(cell) {
                c.tiles += 1;
            }
        }
        if movers.contains(&before[k].id) {
            continue;
        }
        let (o, n) = (before[k].rect, after[k]);
        let area = f64::from(o.w * o.h);
        let (di, dj) = ((n.i - o.i).abs(), (n.j - o.j).abs());
        c.distance += dw * area * f64::from(di + dj);
        if di != 0 && dj != 0 {
            c.diagonal += gw * area;
        }
    }
    c
}

/// A layout's cost in whole numbers: tiles reassigned, and over the windows that make room
/// (the moved ones exempt) area times tiles moved, area of those moved on both axes, and tiles
/// moved. [`cost`]'s total is `tiles + dw * spread + gw * bent`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Tally {
    tiles: i64,
    spread: i64,
    bent: i64,
    dist: i32,
}

impl Tally {
    fn total(&self, dw: f64, gw: f64) -> f64 {
        self.tiles as f64 + dw * self.spread as f64 + gw * self.bent as f64
    }

    /// The tally after a window adds `tiles` and moves by `(di, dj)`: when it makes room
    /// (`area` is its size), also its distance and diagonal terms; a moved one is exempt (None).
    fn moved(self, tiles: i64, area: Option<i64>, di: i32, dj: i32) -> Tally {
        let Some(area) = area else {
            return Tally {
                tiles: self.tiles + tiles,
                ..self
            };
        };
        let (di, dj) = (di.abs(), dj.abs());
        Tally {
            tiles: self.tiles + tiles,
            spread: self.spread + area * i64::from(di + dj),
            bent: self.bent + if di != 0 && dj != 0 { area } else { 0 },
            dist: self.dist + di + dj,
        }
    }
}

/// The tiles a window adds when it moves from `o` to `n`, the windows in `moved` (as old and new
/// rectangle) moved already: with the old rectangles disjoint and the new ones disjoint, the
/// tiles of `o` or `n` but not both that no moved window's rectangles hold (proof: `Cost.v`).
fn fresh(o: &Rect, n: &Rect, moved: impl Iterator<Item = (Rect, Rect)>) -> i64 {
    let held: i32 = moved
        .map(|(mo, mn)| shared_area(o, &mn) + shared_area(n, &mo))
        .sum();
    i64::from(shared_area(o, o) + shared_area(n, n) - 2 * shared_area(o, n) - held)
}

/// Bits over window positions.
struct Bits(Vec<u64>);

impl Bits {
    fn new(n: usize) -> Self {
        Bits(vec![0; n.div_ceil(64)])
    }

    fn set(&mut self, k: usize, on: bool) {
        let (w, b) = (k / 64, 1u64 << (k % 64));
        if on {
            self.0[w] |= b;
        } else {
            self.0[w] &= !b;
        }
    }

    /// Whether a bit other than `k` is set.
    fn any_but(&self, k: usize) -> bool {
        self.0.iter().enumerate().any(|(w, &x)| {
            let x = if w == k / 64 {
                x & !(1u64 << (k % 64))
            } else {
                x
            };
            x != 0
        })
    }

    fn first(&self) -> Option<usize> {
        self.0
            .iter()
            .position(|&w| w != 0)
            .map(|w| w * 64 + self.0[w].trailing_zeros() as usize)
    }
}

/// Union-find over window positions.
struct Sets(Vec<usize>);

impl Sets {
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

/// The islands of a layout without the window `k` about to be placed, for the complete layouts
/// its candidates make: placing it merges the islands it touches, so a candidate splits an
/// island before exactly when it fails to touch every island the island's other windows lie on
/// now (and, for its own island, the one or more they lie on).
struct Mend {
    /// Each window's island now, without `k` (unused at `k`).
    root: Vec<usize>,
    /// For each island before that `k` must hold together, the islands now its windows lie on.
    must: Vec<Vec<usize>>,
    near: Vec<usize>,
    touched: Vec<usize>,
}

struct Search<'a> {
    before: &'a [Window],
    /// Whether each window, by index, is one of the moved ones, exempt from distance and diagonal.
    mover: Vec<bool>,
    /// The range tree over the rectangles before. A window not placed yet still lies there.
    index: Index,
    /// Each window's island before without the movers, numbered from 0 (none for a mover).
    island: Vec<Option<usize>>,
    /// How many islands `island` numbers.
    count: usize,
    dw: f64,
    gw: f64,
    /// The direction of the move: among equally costly places, those along it come first.
    along: Option<(i32, i32)>,
    best: Option<(f64, i32, Vec<Rect>)>,
    nodes: usize,
    /// The layout being built: a window not placed lies where it was.
    cur: Vec<Rect>,
    placed: Vec<bool>,
    /// The placed windows away from their rectangle before, in the order they moved.
    moved: Vec<usize>,
    /// How many placed windows lie on each window's rectangle before.
    cover: Vec<u32>,
    /// The windows not placed that a placed one lies on: the next to place is the first.
    conflicts: Bits,
    /// The cost of `cur`.
    tally: Tally,
    /// Whether each window is in `moved`.
    away: Vec<bool>,
    /// The windows sharing a side with each window in the layout before (Index::touching).
    sides: Vec<Vec<usize>>,
    /// Candidate lists, one per depth, kept to spare their allocation.
    lists: Vec<Vec<(u128, Rect, bool)>>,
    near: Vec<usize>,
    blockers: Vec<Rect>,
    grid: Vec<[i64; 3]>,
}

impl Search<'_> {
    /// Places window `q` on `r`, which overlaps no placed window, with the tally that results.
    fn put(&mut self, q: usize, r: Rect, tally: Tally) {
        if r != self.before[q].rect {
            self.moved.push(q);
            self.away[q] = true;
        }
        self.cur[q] = r;
        self.placed[q] = true;
        self.conflicts.set(q, false);
        self.tally = tally;
        let mut near = std::mem::take(&mut self.near);
        self.index.overlapping_into(&r, &mut near);
        for &p in near.iter().filter(|&&p| p != q) {
            self.cover[p] += 1;
            if !self.placed[p] {
                self.conflicts.set(p, true);
            }
        }
        self.near = near;
    }

    /// Takes back the last [`Self::put`] of `q`, which came from `tally`.
    fn unput(&mut self, q: usize, tally: Tally) {
        let r = self.cur[q];
        let mut near = std::mem::take(&mut self.near);
        self.index.overlapping_into(&r, &mut near);
        for &p in near.iter().filter(|&&p| p != q) {
            self.cover[p] -= 1;
            if self.cover[p] == 0 {
                self.conflicts.set(p, false);
            }
        }
        self.near = near;
        if self.moved.last() == Some(&q) {
            self.moved.pop();
            self.away[q] = false;
        }
        self.cur[q] = self.before[q].rect;
        self.placed[q] = false;
        self.conflicts.set(q, self.cover[q] > 0);
        self.tally = tally;
    }

    /// The tally of placing window `q`, not placed, on `r`.
    fn tally_with(&self, q: usize, r: &Rect) -> Tally {
        let o = self.before[q].rect;
        let moved = self
            .moved
            .iter()
            .map(|&m| (self.before[m].rect, self.cur[m]));
        let tiles = if *r == o { 0 } else { fresh(&o, r, moved) };
        let area = (!self.mover[q]).then(|| i64::from(o.w * o.h));
        self.tally.moved(tiles, area, r.i - o.i, r.j - o.j)
    }

    /// Whether a layout costing `total` and moving windows `d` tiles is no better than the best.
    fn worse(&self, total: f64, d: i32) -> bool {
        self.best
            .as_ref()
            .is_some_and(|(bc, bd, _)| total > bc + 1e-9 || (total > bc - 1e-9 && d >= *bd))
    }

    /// One step, as entered: counts against the budget, gives up on a layout no better than the
    /// best, keeps a complete one that splits no island, else places the first conflict.
    fn run(&mut self) {
        self.nodes += 1;
        if self.nodes > BUDGET {
            return;
        }
        let (total, d) = (self.tally.total(self.dw, self.gw), self.tally.dist);
        if self.worse(total, d) {
            return;
        }
        self.step(total, d, 0);
    }

    /// The step after the budget and the bound let it through.
    fn step(&mut self, total: f64, d: i32, depth: usize) {
        let Some(k) = self.conflicts.first() else {
            if !self.splits() {
                self.best = Some((total, d, self.cur.clone()));
            }
            return;
        };
        let o = self.cur[k];
        let reach_x = o.w + REACH;
        let reach_y = o.h + REACH;
        let area = f64::from(o.w * o.h);
        let n = self.cur.len();
        // Every candidate lies inside `room`, so only the windows that overlap it can be in one's
        // way or under one; `under` keeps the scan's ascending order.
        let room = Rect::new(
            o.i - reach_x,
            o.j - reach_y,
            o.w + 2 * reach_x,
            o.h + 2 * reach_y,
        );
        // Over the grid of candidates (columns i0..=i1, rows j0..=j1), each window in the room
        // covers the rectangle of candidates that overlap it: c overlaps r exactly when
        // r.i - o.w < c.i < r.right() and r.j - o.h < c.j < r.bottom(). Difference arrays add
        // the placed windows (in the way), the windows not placed but k (one each, and their
        // areas), and the moved windows' rectangles before (tiles they freed, for the cost).
        let (i0, j0) = (o.i - reach_x, o.j - reach_y);
        let (gw, gh) = ((2 * reach_x + 1) as usize, (2 * reach_y + 1) as usize);
        let mut grid = std::mem::take(&mut self.grid);
        grid.clear();
        grid.resize((gw + 1) * (gh + 1), [0; 3]);
        let mut cover = |r: &Rect, slot: usize, v: i64| {
            let (a, b) = (
                (r.i - o.w + 1).max(i0),
                (r.right() - 1).min(i0 + gw as i32 - 1),
            );
            let (c, d) = (
                (r.j - o.h + 1).max(j0),
                (r.bottom() - 1).min(j0 + gh as i32 - 1),
            );
            if a > b || c > d {
                return;
            }
            let (a, b, c, d) = (
                (a - i0) as usize,
                (b - i0) as usize + 1,
                (c - j0) as usize,
                (d - j0) as usize + 1,
            );
            grid[a * (gh + 1) + c][slot] += v;
            grid[b * (gh + 1) + c][slot] -= v;
            grid[a * (gh + 1) + d][slot] -= v;
            grid[b * (gh + 1) + d][slot] += v;
        };
        for p in (0..n).filter(|&p| self.placed[p]) {
            cover(&self.cur[p], 0, 1);
        }
        let mut under = std::mem::take(&mut self.near);
        self.index.overlapping_into(&room, &mut under);
        for &p in under.iter().filter(|&&p| !self.placed[p] && p != k) {
            let r = self.cur[p];
            cover(&r, 1, 1);
            cover(&r, 2, i64::from(r.w * r.h));
        }
        self.near = under;
        for a in 0..=gw {
            for b in 0..=gh {
                let x = a * (gh + 1) + b;
                let left = if a > 0 { grid[x - gh - 1] } else { [0; 3] };
                let up = if b > 0 { grid[x - 1] } else { [0; 3] };
                let corner = if a > 0 && b > 0 {
                    grid[x - gh - 2]
                } else {
                    [0; 3]
                };
                for (((v, l), u), c) in grid[x].iter_mut().zip(left).zip(up).zip(corner) {
                    *v += l + u - c;
                }
            }
        }
        let mut cands = self
            .lists
            .get_mut(depth)
            .map(std::mem::take)
            .unwrap_or_default();
        cands.clear();
        for a in 0..gw {
            for b in 0..gh {
                let [blocked, others_under, pending] = grid[a * (gh + 1) + b];
                let (i, j) = (i0 + a as i32, j0 + b as i32);
                let c = Rect::new(i, j, o.w, o.h);
                if c == o || blocked > 0 {
                    continue;
                }
                let (di, dj) = ((i - o.i).abs(), (j - o.j).abs());
                let kept = f64::from(shared_area(&c, &o));
                let est = 2.0 * (area - kept)
                    + self.dw * area * f64::from(di + dj)
                    + if di != 0 && dj != 0 {
                        self.gw * area
                    } else {
                        0.0
                    }
                    + pending as f64;
                let off = self
                    .along
                    .is_none_or(|(ai, aj)| (i - o.i).signum() == ai && (j - o.j).signum() == aj);
                // The order (est, distance, off the move's direction, row, column) as one key.
                let bits = est.to_bits();
                let est = if bits >> 63 == 1 {
                    !bits
                } else {
                    bits | 1 << 63
                };
                let key = u128::from(est) << 64
                    | u128::from((di + dj) as u32) << 40
                    | u128::from(!off) << 39
                    | (b as u128) << 20
                    | a as u128;
                cands.push((key, c, others_under == 0));
            }
        }
        self.grid = grid;
        cands.sort_unstable_by_key(|c| c.0);
        let before = self.tally;
        // A child on no window not placed, with no other conflict, is a complete layout.
        let others = self.conflicts.any_but(k);
        let mut mend: Option<Mend> = None;
        // The cost of a child: k leaves o for c (Tally::moved by fresh, the moved windows
        // whose rectangles before lie outside the room sharing no tile with c).
        let held: i32 = self
            .moved
            .iter()
            .map(|&m| shared_area(&o, &self.cur[m]))
            .sum();
        let mut freed = std::mem::take(&mut self.blockers);
        freed.clear();
        freed.extend(
            self.moved
                .iter()
                .map(|&m| self.before[m].rect)
                .filter(|r| r.overlaps(&room)),
        );
        let a64 = i64::from(o.w * o.h);
        for &(_, c, clear) in &cands {
            // Entering the child: the budget, then its bound, without placing it.
            self.nodes += 1;
            if self.nodes > BUDGET {
                break;
            }
            let held = held + freed.iter().map(|r| shared_area(&c, r)).sum::<i32>();
            let tiles = shared_area(&o, &o) + shared_area(&c, &c) - 2 * shared_area(&o, &c) - held;
            let tally = before.moved(i64::from(tiles), Some(a64), c.i - o.i, c.j - o.j);
            let (total, d) = (tally.total(self.dw, self.gw), tally.dist);
            if self.worse(total, d) {
                continue;
            }
            if clear && !others {
                let mend = mend.get_or_insert_with(|| self.mend(k));
                if !self.mends(mend, k, &c) {
                    let mut layout = self.cur.clone();
                    layout[k] = c;
                    self.best = Some((total, d, layout));
                }
                continue;
            }
            self.put(k, c, tally);
            self.step(total, d, depth + 1);
            self.unput(k, before);
            if self.nodes > BUDGET {
                break;
            }
        }
        self.blockers = freed;
        if self.lists.len() <= depth {
            self.lists.resize_with(depth + 1, Vec::new);
        }
        self.lists[depth] = cands;
    }

    /// [`splits`] of the layout being built, by position: its islands are a flood whose
    /// neighbours of a window that stayed are its sides before that stayed too, those of a moved
    /// one come from the range tree over the layout before (kept when they stayed), and the
    /// moved windows are scanned for both; one pass then looks for an island before whose
    /// windows lie on two after.
    fn splits(&self) -> bool {
        let (cur, moved, away) = (&self.cur, &self.moved, &self.away);
        let part = flood(cur.len(), |a, out| {
            if away[a] {
                self.index.touching_into(&cur[a], out);
                out.retain(|&b| !away[b]);
            } else {
                out.clear();
                out.extend(self.sides[a].iter().copied().filter(|&b| !away[b]));
            }
            out.extend(moved.iter().copied().filter(|&b| touch(&cur[a], &cur[b])));
        });
        let mut first = vec![usize::MAX; self.count];
        (0..cur.len()).any(|x| match self.island[x] {
            Some(i) if first[i] == usize::MAX => {
                first[i] = part[x];
                false
            }
            Some(i) => first[i] != part[x],
            None => false,
        })
    }

    /// The islands now without `k`, and the islands before that placing `k` must hold together:
    /// those whose windows lie on two or more now, and its own when its other windows lie on any.
    fn mend(&self, k: usize) -> Mend {
        let n = self.cur.len();
        let (away, moved) = (&self.away, &self.moved);
        let mut sets = Sets((0..n).collect());
        for a in (0..n).filter(|&a| a != k && !away[a]) {
            for &b in self.sides[a]
                .iter()
                .filter(|&&b| b > a && b != k && !away[b])
            {
                sets.union(a, b);
            }
        }
        let mut near = Vec::new();
        for (x, &b) in moved.iter().enumerate() {
            self.index.touching_into(&self.cur[b], &mut near);
            for &a in near.iter().filter(|&&a| a != k && !away[a]) {
                sets.union(a, b);
            }
            for &b2 in moved[x + 1..]
                .iter()
                .filter(|&&b2| touch(&self.cur[b], &self.cur[b2]))
            {
                sets.union(b, b2);
            }
        }
        let root: Vec<usize> = (0..n).map(|x| sets.find(x)).collect();
        let mut lie: Vec<Vec<usize>> = vec![Vec::new(); self.count];
        for x in (0..n).filter(|&x| x != k) {
            if let Some(i) = self.island[x]
                && !lie[i].contains(&root[x])
            {
                lie[i].push(root[x]);
            }
        }
        let own = self.island[k];
        let must = lie
            .into_iter()
            .enumerate()
            .filter(|(i, l)| l.len() >= 2 || (Some(*i) == own && !l.is_empty()))
            .map(|(_, l)| l)
            .collect();
        Mend {
            root,
            must,
            near,
            touched: Vec::new(),
        }
    }

    /// Whether `k` placed on `c`, completing the layout, splits an island before ([`splits`]):
    /// whether some island it must hold together lies on an island now that `c` does not touch.
    fn mends(&self, mend: &mut Mend, k: usize, c: &Rect) -> bool {
        let Mend {
            root,
            must,
            near,
            touched,
        } = mend;
        touched.clear();
        self.index.touching_into(c, near);
        touched.extend(
            near.iter()
                .filter(|&&a| a != k && !self.away[a])
                .map(|&a| root[a]),
        );
        touched.extend(
            self.moved
                .iter()
                .filter(|&&b| touch(c, &self.cur[b]))
                .map(|&b| root[b]),
        );
        must.iter().any(|l| l.iter().any(|r| !touched.contains(r)))
    }

    /// The tally of a whole layout, as placing its moved windows one by one would give.
    fn tally_of(&self, cur: &[Rect]) -> Tally {
        let mut seen: Vec<(Rect, Rect)> = Vec::new();
        let mut tally = Tally::default();
        for k in (0..cur.len()).filter(|&k| self.before[k].rect != cur[k]) {
            let (o, n) = (self.before[k].rect, cur[k]);
            let tiles = fresh(&o, &n, seen.iter().copied());
            let area = (!self.mover[k]).then(|| i64::from(o.w * o.h));
            tally = tally.moved(tiles, area, n.i - o.i, n.j - o.j);
            seen.push((o, n));
        }
        tally
    }
}

impl Canvas {
    /// The rectangles, in window order, of the least costly layout that puts each of `targets`
    /// on its rectangle, keeps every other span, overlaps nothing and splits no island; among
    /// equally costly layouts, windows move along `along` first. None when the search finds none
    /// within its budget.
    ///
    /// `seed`, a layout the caller would fall back to, starts as the one to beat when it keeps
    /// the targets on their rectangles, overlaps nothing and splits no island: the search then
    /// looks only for cheaper ones, and returns the seed when it finds none in its budget.
    pub fn arrange(
        &self,
        targets: &[(Id, Rect)],
        along: Option<Dir>,
        seed: Option<Vec<Rect>>,
    ) -> Option<Vec<Rect>> {
        let n = self.windows.len();
        let mut at: Vec<Option<Rect>> = vec![None; n];
        for &(id, r) in targets {
            at[self.index(id)?] = Some(r);
        }
        let movers: Vec<Id> = targets.iter().map(|t| t.0).collect();
        let rects: Vec<Rect> = self.windows.iter().map(|w| w.rect).collect();
        let mover: Vec<bool> = self
            .windows
            .iter()
            .map(|w| movers.contains(&w.id))
            .collect();
        let index = Index::new(&rects);
        let sides: Vec<Vec<usize>> = (0..n).map(|k| index.touching(k)).collect();
        // The islands before without the movers: a flood through the sides between the others.
        let part = flood(n, |a, out| {
            out.clear();
            if !mover[a] {
                out.extend(sides[a].iter().copied().filter(|&b| !mover[b]));
            }
        });
        let before = labels(&self.windows, &part, |k| !mover[k]);
        let mut number: HashMap<Id, usize> = HashMap::new();
        let island = self
            .windows
            .iter()
            .map(|w| {
                let next = number.len();
                before.get(&w.id).map(|l| *number.entry(*l).or_insert(next))
            })
            .collect();
        let mut s = Search {
            before: &self.windows,
            mover,
            sides,
            index,
            island,
            count: number.len(),
            dw: self.config.distance_weight,
            gw: self.config.diagonal_weight,
            along: along.map(Dir::delta),
            best: None,
            nodes: 0,
            cur: rects,
            placed: vec![false; n],
            moved: Vec::new(),
            cover: vec![0; n],
            conflicts: Bits::new(n),
            tally: Tally::default(),
            away: vec![false; n],
            lists: Vec::new(),
            near: Vec::new(),
            blockers: Vec::new(),
            grid: Vec::new(),
        };
        for (k, r) in at.iter().enumerate() {
            if let Some(r) = *r {
                let tally = s.tally_with(k, &r);
                s.put(k, r, tally);
            }
        }
        if let Some(seed) = seed.filter(|seed| {
            seed.len() == n
                && (0..n).all(|k| at[k].is_none_or(|r| seed[k] == r))
                && sound(&self.windows, &before, seed)
        }) {
            let tally = s.tally_of(&seed);
            s.best = Some((tally.total(s.dw, s.gw), tally.dist, seed));
        }
        s.run();
        s.best.map(|b| b.2)
    }

    /// Places `targets` by [`Self::arrange`]; false, with nothing changed, when it finds no
    /// layout, so that the caller can fall back to pushing.
    pub(crate) fn place(
        &mut self,
        targets: &[(Id, Rect)],
        along: Option<Dir>,
        seed: Option<Vec<Rect>>,
    ) -> bool {
        let Some(rects) = self.arrange(targets, along, seed) else {
            return false;
        };
        if !self.accepts(targets, &rects) {
            return false;
        }
        for (w, r) in self.windows.iter_mut().zip(rects) {
            w.rect = r;
        }
        true
    }

    /// Whether `rects`, in window order, is a layout [`Self::place`] may take: one rectangle per
    /// window, each target on its rectangle, every other window at its span, no two overlapping,
    /// and no island split. The search is trusted for the cost, never for these.
    pub fn accepts(&self, targets: &[(Id, Rect)], rects: &[Rect]) -> bool {
        let movers: Vec<Id> = targets.iter().map(|t| t.0).collect();
        let after: Vec<Window> = self
            .windows
            .iter()
            .zip(rects)
            .map(|(w, r)| Window { rect: *r, ..*w })
            .collect();
        rects.len() == self.windows.len()
            && self.windows.iter().zip(rects).all(|(w, r)| {
                match targets.iter().find(|t| t.0 == w.id) {
                    Some(&(_, t)) => *r == t,
                    None => (r.w, r.h) == (w.rect.w, w.rect.h),
                }
            })
            && disjoint(rects)
            && !splits(&self.windows, &after, &movers)
    }

    /// The bridge `gone` left the windows of its island apart: each part of `group` but the
    /// largest slides, smallest move first, the least distance that makes it touch the largest
    /// part again, stopping at the first window in its way. The result keeps every span and
    /// overlaps nothing, or nothing moves.
    pub(crate) fn gather(&mut self, group: &[Id]) {
        self.gather_kept(group, 6, 64, false);
    }

    /// [`Self::gather`] in up to `rounds` moves of up to `reach` tiles each; with `bend`, a part
    /// that no straight slide brings to the largest may go along one axis, then the other.
    pub(crate) fn gather_kept(&mut self, group: &[Id], rounds: usize, reach: i32, bend: bool) {
        let before = self.windows.clone();
        self.gather_moves(group, rounds, reach, bend);
        let rects: Vec<Rect> = self.windows.iter().map(|w| w.rect).collect();
        let kept = before
            .iter()
            .zip(&rects)
            .all(|(w, r)| (r.w, r.h) == (w.rect.w, w.rect.h));
        if rects.len() != before.len() || !kept || !disjoint(&rects) {
            self.windows = before;
        }
    }

    fn gather_moves(&mut self, group: &[Id], rounds: usize, reach: i32, bend: bool) {
        let area = |c: &Canvas, ids: &[Id]| -> i32 {
            ids.iter()
                .filter_map(|&id| c.rect(id))
                .map(|r| r.w * r.h)
                .sum()
        };
        for _ in 0..rounds {
            let label = islands(&self.windows, &[]);
            let mut parts: Vec<Vec<Id>> = Vec::new();
            for &id in group {
                let Some(&l) = label.get(&id) else { continue };
                match parts.iter_mut().find(|p| label[&p[0]] == l) {
                    Some(p) => p.push(id),
                    None => parts.push(vec![id]),
                }
            }
            if parts.len() <= 1 {
                return;
            }
            parts.sort_by_key(|p| std::cmp::Reverse(area(self, p)));
            let anchor = parts[0].clone();
            let mut best: Option<(i32, Vec<(Id, Rect)>)> = None;
            for part in &parts[1..] {
                let a = area(self, part);
                for dir in SLIDES {
                    if let Some(d) = self.slide(part, &anchor, (0, 0), dir, reach) {
                        offer(&mut best, d * a, self.shifted(part, dir.0 * d, dir.1 * d));
                    }
                }
            }
            if bend && best.is_none() {
                for part in &parts[1..] {
                    let a = area(self, part);
                    for dir in SLIDES {
                        if let Some((c, moves)) = self.bent(part, &anchor, dir, reach) {
                            offer(&mut best, c * a, moves);
                        }
                    }
                }
            }
            let Some((_, moves)) = best else { return };
            for (id, r) in moves {
                let k = self.index(id).unwrap();
                self.windows[k].rect = r;
            }
        }
    }

    /// `part`'s rectangles shifted by `(di, dj)`.
    fn shifted(&self, part: &[Id], di: i32, dj: i32) -> Vec<(Id, Rect)> {
        part.iter()
            .map(|&id| (id, self.rect(id).unwrap().shifted(di, dj)))
            .collect()
    }

    /// Whether `part` shifted by `(di, dj)` lands on a window outside it.
    fn blocked(&self, part: &[Id], di: i32, dj: i32) -> bool {
        let rest = self.windows.iter().filter(|w| !part.contains(&w.id));
        self.shifted(part, di, dj)
            .iter()
            .any(|(_, s)| rest.clone().any(|w| s.overlaps(&w.rect)))
    }

    /// How far, 1 to `reach` tiles, `part` shifted by `from` slides along `dir` before it touches
    /// a window of `anchor`; None when a window outside it is in its way first, or it never does.
    fn slide(
        &self,
        part: &[Id],
        anchor: &[Id],
        from: (i32, i32),
        dir: (i32, i32),
        reach: i32,
    ) -> Option<i32> {
        for d in 1..=reach {
            let (di, dj) = (from.0 + dir.0 * d, from.1 + dir.1 * d);
            if self.blocked(part, di, dj) {
                return None;
            }
            let near = self.shifted(part, di, dj).iter().any(|(_, s)| {
                self.windows
                    .iter()
                    .any(|w| anchor.contains(&w.id) && touch(s, &w.rect))
            });
            if near {
                return Some(d);
            }
        }
        None
    }

    /// The least bent move of `part` that starts along `dir`: the first distance along `dir`,
    /// stopping at the first window in its way, from which a slide across it touches `anchor`,
    /// with the tiles both legs take and where the part lands.
    fn bent(
        &self,
        part: &[Id],
        anchor: &[Id],
        dir: (i32, i32),
        reach: i32,
    ) -> Option<(i32, Vec<(Id, Rect)>)> {
        let across = [(dir.1, dir.0), (-dir.1, -dir.0)];
        for d in 1..=reach {
            let from = (dir.0 * d, dir.1 * d);
            if self.blocked(part, from.0, from.1) {
                return None;
            }
            let lined = self.shifted(part, from.0, from.1).iter().any(|(_, s)| {
                self.windows.iter().any(|w| {
                    let a = &w.rect;
                    anchor.contains(&w.id)
                        && if dir.0 != 0 {
                            s.cols_overlap(a) || s.right() == a.i || a.right() == s.i
                        } else {
                            s.rows_overlap(a) || s.bottom() == a.j || a.bottom() == s.j
                        }
                })
            });
            if !lined {
                continue;
            }
            let leg = across
                .iter()
                .filter_map(|&x| Some((self.slide(part, anchor, from, x, reach)?, x)))
                .min_by_key(|&(e, _)| e);
            if let Some((e, x)) = leg {
                let moves = self.shifted(part, from.0 + x.0 * e, from.1 + x.1 * e);
                return Some((d + e, moves));
            }
        }
        None
    }

    /// The windows of `id`'s island but `id`.
    pub(crate) fn island_of(&self, id: Id) -> Vec<Id> {
        let label = islands(&self.windows, &[]);
        let Some(&home) = label.get(&id) else {
            return Vec::new();
        };
        self.windows
            .iter()
            .map(|w| w.id)
            .filter(|&w| w != id && label[&w] == home)
            .collect()
    }
}

/// The search before the range tree, kept to show the tree changes no decision.
#[cfg(test)]
pub(crate) mod reference {
    use std::collections::HashMap;

    use super::{cells, contains, touch, BUDGET, REACH};
    use crate::{Dir, Id, Rect, Window};

    /// The island of each window but `skip`, as the smallest id in it.
    pub(super) fn islands(ws: &[Window], skip: &[Id]) -> HashMap<Id, Id> {
        let list: Vec<&Window> = ws.iter().filter(|w| !skip.contains(&w.id)).collect();
        let mut label: HashMap<Id, Id> = list.iter().map(|w| (w.id, w.id)).collect();
        loop {
            let mut changed = false;
            for a in &list {
                for b in &list {
                    if touch(&a.rect, &b.rect) && label[&b.id] < label[&a.id] {
                        label.insert(a.id, label[&b.id]);
                        changed = true;
                    }
                }
            }
            if !changed {
                return label;
            }
        }
    }

    /// Rule 2 broken: two windows other than `movers`, connected without them before, that are not
    /// connected after.
    pub(super) fn splits(before: &[Window], after: &[Window], movers: &[Id]) -> bool {
        let (b, a) = (islands(before, movers), islands(after, &[]));
        b.iter().any(|(x, lx)| {
            b.iter().any(|(y, ly)| {
                x < y && lx == ly && a.contains_key(x) && a.contains_key(y) && a[x] != a[y]
            })
        })
    }

    /// No two of `rects` overlap.
    pub(super) fn disjoint(rects: &[Rect]) -> bool {
        (0..rects.len()).all(|a| (a + 1..rects.len()).all(|b| !rects[a].overlaps(&rects[b])))
    }

    struct Search<'a> {
        before: &'a [Window],
        /// The window on each tile before, by index.
        owner: HashMap<(i32, i32), usize>,
        movers: Vec<Id>,
        dw: f64,
        gw: f64,
        /// The direction of the move: among equally costly places, those along it come first.
        along: Option<(i32, i32)>,
        best: Option<(f64, i32, Vec<Rect>)>,
        nodes: usize,
    }

    impl Search<'_> {
        /// [`cost`]'s total, from the tiles of the windows that moved only.
        fn cost_of(&self, cur: &[Rect]) -> f64 {
            let moved: Vec<usize> = (0..cur.len())
                .filter(|&k| self.before[k].rect != cur[k])
                .collect();
            let after = |c: (i32, i32)| {
                moved
                    .iter()
                    .copied()
                    .find(|&k| contains(&cur[k], c))
                    .or_else(|| self.owner.get(&c).copied().filter(|k| !moved.contains(k)))
            };
            let mut seen = std::collections::HashSet::new();
            let mut total = 0.0;
            for &k in &moved {
                for c in cells(&self.before[k].rect).chain(cells(&cur[k])) {
                    if seen.insert(c) && self.owner.get(&c).copied() != after(c) {
                        total += 1.0;
                    }
                }
                if self.movers.contains(&self.before[k].id) {
                    continue;
                }
                let (o, n) = (self.before[k].rect, cur[k]);
                let area = f64::from(o.w * o.h);
                let (di, dj) = ((n.i - o.i).abs(), (n.j - o.j).abs());
                total += self.dw * area * f64::from(di + dj);
                if di != 0 && dj != 0 {
                    total += self.gw * area;
                }
            }
            total
        }

        fn moved_by(&self, cur: &[Rect]) -> i32 {
            (0..cur.len())
                .filter(|&k| !self.movers.contains(&self.before[k].id))
                .map(|k| {
                    (cur[k].i - self.before[k].rect.i).abs()
                        + (cur[k].j - self.before[k].rect.j).abs()
                })
                .sum()
        }

        fn search(&mut self, cur: &mut Vec<Rect>, placed: &mut Vec<bool>) {
            self.nodes += 1;
            if self.nodes > BUDGET {
                return;
            }
            let total = self.cost_of(cur);
            let d = self.moved_by(cur);
            if let Some((bc, bd, _)) = &self.best
                && (total > bc + 1e-9 || (total > bc - 1e-9 && d >= *bd))
            {
                return;
            }
            let n = cur.len();
            let conflict = (0..n)
                .find(|&k| !placed[k] && (0..n).any(|p| placed[p] && cur[k].overlaps(&cur[p])));
            let Some(k) = conflict else {
                let after: Vec<Window> = (0..n)
                    .map(|k| Window {
                        rect: cur[k],
                        ..self.before[k]
                    })
                    .collect();
                if !splits(self.before, &after, &self.movers) {
                    self.best = Some((total, d, cur.clone()));
                }
                return;
            };
            let o = cur[k];
            let reach_x = o.w + REACH;
            let reach_y = o.h + REACH;
            let area = f64::from(o.w * o.h);
            let mut cands: Vec<(f64, i32, i32, Rect)> = Vec::new();
            for i in o.i - reach_x..=o.i + reach_x {
                for j in o.j - reach_y..=o.j + reach_y {
                    let c = Rect::new(i, j, o.w, o.h);
                    if c == o || (0..n).any(|p| placed[p] && c.overlaps(&cur[p])) {
                        continue;
                    }
                    let (di, dj) = ((i - o.i).abs(), (j - o.j).abs());
                    let kept = cells(&c).filter(|&x| contains(&o, x)).count() as f64;
                    let pending: f64 = (0..n)
                        .filter(|&p| !placed[p] && p != k && c.overlaps(&cur[p]))
                        .map(|p| f64::from(cur[p].w * cur[p].h))
                        .sum();
                    let est = 2.0 * (area - kept)
                        + self.dw * area * f64::from(di + dj)
                        + if di != 0 && dj != 0 {
                            self.gw * area
                        } else {
                            0.0
                        }
                        + pending;
                    let off = self.along.is_none_or(|(ai, aj)| {
                        (i - o.i).signum() == ai && (j - o.j).signum() == aj
                    });
                    cands.push((est, di + dj, i32::from(!off), c));
                }
            }
            cands.sort_by(|a, b| {
                a.0.total_cmp(&b.0)
                    .then(a.1.cmp(&b.1))
                    .then(a.2.cmp(&b.2))
                    .then(a.3.j.cmp(&b.3.j))
                    .then(a.3.i.cmp(&b.3.i))
            });
            placed[k] = true;
            for (_, _, _, c) in cands {
                cur[k] = c;
                self.search(cur, placed);
                if self.nodes > BUDGET {
                    break;
                }
            }
            cur[k] = o;
            placed[k] = false;
        }
    }

    pub(super) fn arrange(
        windows: &[Window],
        dw: f64,
        gw: f64,
        targets: &[(Id, Rect)],
        along: Option<Dir>,
        seed: Option<Vec<Rect>>,
    ) -> Option<Vec<Rect>> {
        let mut cur: Vec<Rect> = windows.iter().map(|w| w.rect).collect();
        let mut placed = vec![false; cur.len()];
        for &(id, r) in targets {
            let k = windows.iter().position(|w| w.id == id)?;
            cur[k] = r;
            placed[k] = true;
        }
        let owner = windows
            .iter()
            .enumerate()
            .flat_map(|(k, w)| cells(&w.rect).map(move |c| (c, k)))
            .collect();
        let mut s = Search {
            before: windows,
            owner,
            movers: targets.iter().map(|t| t.0).collect(),
            dw,
            gw,
            along: along.map(Dir::delta),
            best: None,
            nodes: 0,
        };
        if let Some(seed) = seed.filter(|seed| {
            seed.len() == cur.len()
                && (0..cur.len()).all(|k| !placed[k] || seed[k] == cur[k])
                && (0..seed.len()).all(|a| (a + 1..seed.len()).all(|b| !seed[a].overlaps(&seed[b])))
                && !splits(
                    windows,
                    &windows
                        .iter()
                        .zip(seed)
                        .map(|(w, r)| Window { rect: *r, ..*w })
                        .collect::<Vec<_>>(),
                    &s.movers,
                )
        }) {
            s.best = Some((s.cost_of(&seed), s.moved_by(&seed), seed));
        }
        s.search(&mut cur, &mut placed);
        s.best.map(|b| b.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> i32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n) as i32
        }
    }

    const MINS: [(f64, f64); 5] = [
        (960.0, 960.0),
        (600.0, 400.0),
        (1200.0, 900.0),
        (300.0, 300.0),
        (100.0, 100.0),
    ];
    const DIRS: [Dir; 4] = [Dir::Left, Dir::Right, Dir::Up, Dir::Down];

    fn canvas(rng: &mut Rng, tile: (f64, f64), weights: (f64, f64), n: usize) -> Canvas {
        let config = Config {
            tile_w: tile.0,
            tile_h: tile.1,
            distance_weight: weights.0,
            diagonal_weight: weights.1,
            ..Config::default()
        };
        let mut c = Canvas::new(config, 2560.0, 1440.0);
        for id in 1..=n as u64 {
            let (w, h) = MINS[rng.below(MINS.len() as u64) as usize];
            c.add(id, w, h, None);
            if rng.below(3) == 0 {
                let ids: Vec<Id> = c.windows().iter().map(|w| w.id).collect();
                c.focus_id(ids[rng.below(ids.len() as u64) as usize]);
            }
        }
        c
    }

    /// The range tree and the cost from shared areas change no decision: on canvases at newm-rs's
    /// tiles and the default's, with the shipped weights and with weights whose sums round,
    /// every placement the search makes, with and without a seed, is the one the scan made.
    #[test]
    fn arrange_decides_what_the_scan_decided() {
        let mut rng = Rng(0x0dd_ba11_cafe_f00d);
        let mut placed = 0;
        for round in 0..72 {
            let tile = if round % 2 == 0 {
                (119.0, 113.0)
            } else {
                (720.0, 465.0)
            };
            let weights = if round % 3 == 2 {
                (0.3, 0.7)
            } else {
                (0.5, 1.0)
            };
            let c = canvas(&mut rng, tile, weights, 2 + round % 14);
            let (dw, gw) = (c.config.distance_weight, c.config.diagonal_weight);
            for _ in 0..6 {
                let ws = c.windows();
                let w = ws[rng.below(ws.len() as u64) as usize];
                let dir = DIRS[rng.below(4) as usize];
                let (di, dj) = (rng.below(9) - 4, rng.below(7) - 3);
                let target = Rect::new(w.rect.i + di, w.rect.j + dj, w.rect.w, w.rect.h);
                let mut targets = vec![(w.id, target)];
                if rng.below(4) == 0
                    && let Some(n) = c.neighbour(w.id, dir)
                {
                    let (a, b) = Canvas::swapped(w.rect, c.rect(n).unwrap(), dir);
                    targets = vec![(w.id, a), (n, b)];
                }
                let seed = (targets.len() == 1).then(|| c.pushed(w.id, target, dir));
                for seed in [seed.clone(), None] {
                    let fast = c.arrange(&targets, Some(dir), seed.clone());
                    let slow = reference::arrange(ws, dw, gw, &targets, Some(dir), seed);
                    assert_eq!(fast, slow, "{ws:?} {targets:?} {dir:?}");
                    placed += usize::from(fast.is_some());
                }
            }
        }
        assert!(placed > 100, "only {placed} placements found a layout");
    }

    /// Islands, splits and disjoint answer what their scans answered, on layouts that overlap
    /// too.
    #[test]
    fn island_checks_answer_what_the_scan_answered() {
        let mut rng = Rng(0x5eed_1234_abcd_ef01);
        for round in 0..300 {
            let n = 1 + rng.below(24) as usize;
            let spread = 3 + rng.below(12);
            let ws: Vec<Window> = (0..n)
                .map(|k| Window {
                    id: (rng.below(1000) * 64 + k as i32) as Id,
                    rect: Rect::new(
                        rng.below(spread as u64) - spread / 2,
                        rng.below(spread as u64) - spread / 2,
                        1 + rng.below(3),
                        1 + rng.below(3),
                    ),
                    min_w: 0.0,
                    min_h: 0.0,
                })
                .collect();
            let rects: Vec<Rect> = ws.iter().map(|w| w.rect).collect();
            assert_eq!(disjoint(&rects), reference::disjoint(&rects), "{rects:?}");
            let skip: Vec<Id> = ws
                .iter()
                .map(|w| w.id)
                .filter(|_| rng.below(5) == 0)
                .collect();
            assert_eq!(
                islands(&ws, &skip),
                reference::islands(&ws, &skip),
                "{ws:?}"
            );
            let mut after: Vec<Window> = Vec::new();
            for w in &ws {
                let (di, dj) = (rng.below(3) - 1, rng.below(3) - 1);
                if round % 3 != 0 || rng.below(6) != 0 {
                    after.push(Window {
                        rect: Rect::new(w.rect.i + di, w.rect.j + dj, w.rect.w, w.rect.h),
                        ..*w
                    });
                }
            }
            assert_eq!(
                splits(&ws, &after, &skip),
                reference::splits(&ws, &after, &skip),
                "{ws:?} {after:?} {skip:?}"
            );
            if let Some(w) = ws.first() {
                assert_eq!(is_bridge(&ws, w.id), {
                    let near: Vec<Id> = ws
                        .iter()
                        .filter(|o| o.id != w.id && touch(&o.rect, &w.rect))
                        .map(|o| o.id)
                        .collect();
                    let label = reference::islands(&ws, &[w.id]);
                    near.iter()
                        .any(|a| near.iter().any(|b| label[a] != label[b]))
                });
            }
        }
    }
}
