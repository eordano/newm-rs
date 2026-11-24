//! A static two-dimensional range tree over rectangles: what the placement search and the island
//! checks ask instead of scanning every window (newm-canvas proof, `Index.v` and `Islands.v`).
//!
//! The tree holds each rectangle's top-left corner. A node covers half of its parent's corners in
//! column order, knows the least and the greatest column under it, and keeps its corners sorted
//! by row. The corners in a box are the union of O(log n) whole nodes, each binary-searched by
//! row, so a query takes O(log² n + k). A rectangle overlaps `q` only when its corner lies in `q`
//! widened left by the widest rectangle held and up by the tallest, so [`Index::overlapping`]
//! asks for that box and keeps the rectangles that overlap `q`: exactly the ones a scan finds,
//! in the same ascending order.

use crate::Rect;

/// A node of the tree: its corners are `ys[lo..hi]` as (row, rectangle), sorted, and it knows the
/// least and the greatest column under it. A leaf (`l` is [`LEAF`]) holds one corner.
#[derive(Clone, Copy)]
struct Node {
    xmin: i32,
    xmax: i32,
    lo: u32,
    hi: u32,
    l: u32,
    r: u32,
}

const LEAF: u32 = u32::MAX;

/// The nodes and the rows of every node, each in one array.
struct Tree {
    nodes: Vec<Node>,
    ys: Vec<(i32, usize)>,
}

impl Tree {
    /// Node::build: one corner is a leaf; more split at half their number, and a bin's rows are
    /// its halves' rows merged (the sorted list of them, the corners being distinct).
    fn build(&mut self, pts: &[(i32, i32, usize)]) -> u32 {
        if let [(x, y, k)] = *pts {
            let lo = self.ys.len() as u32;
            self.ys.push((y, k));
            return self.node(Node {
                xmin: x,
                xmax: x,
                lo,
                hi: lo + 1,
                l: LEAF,
                r: LEAF,
            });
        }
        let (a, b) = pts.split_at(pts.len() / 2);
        let (l, r) = (self.build(a), self.build(b));
        let (ln, rn) = (self.nodes[l as usize], self.nodes[r as usize]);
        let lo = self.ys.len() as u32;
        let (mut i, mut j) = (ln.lo as usize, rn.lo as usize);
        while i < ln.hi as usize || j < rn.hi as usize {
            let v = if j >= rn.hi as usize || (i < ln.hi as usize && self.ys[i] <= self.ys[j]) {
                i += 1;
                self.ys[i - 1]
            } else {
                j += 1;
                self.ys[j - 1]
            };
            self.ys.push(v);
        }
        let hi = self.ys.len() as u32;
        self.node(Node {
            xmin: ln.xmin.min(rn.xmin),
            xmax: ln.xmax.max(rn.xmax),
            lo,
            hi,
            l,
            r,
        })
    }

    fn node(&mut self, n: Node) -> u32 {
        self.nodes.push(n);
        (self.nodes.len() - 1) as u32
    }

    /// Node::corners: pushes the rectangles whose corner lies in columns `x1..=x2` and rows
    /// `y1..=y2`. A node within the columns (a leaf is, when not outside them) is read by
    /// partition_point (the first row >= y1) and take_while (row <= y2).
    fn corners(&self, v: u32, x1: i32, x2: i32, y1: i32, y2: i32, out: &mut Vec<usize>) {
        let n = self.nodes[v as usize];
        if n.xmax < x1 || x2 < n.xmin {
            return;
        }
        if n.l == LEAF || (x1 <= n.xmin && n.xmax <= x2) {
            let ys = &self.ys[n.lo as usize..n.hi as usize];
            let from = ys.partition_point(|p| p.0 < y1);
            out.extend(ys[from..].iter().take_while(|p| p.0 <= y2).map(|p| p.1));
            return;
        }
        self.corners(n.l, x1, x2, y1, y2, out);
        self.corners(n.r, x1, x2, y1, y2, out);
    }
}

/// The range tree over a list of rectangles, which it answers about by their positions in it.
pub struct Index {
    rects: Vec<Rect>,
    tree: Tree,
    root: Option<u32>,
    /// The widest and the tallest rectangle held (0 when none is wider or taller).
    wide: i32,
    tall: i32,
}

impl Index {
    pub fn new(rects: &[Rect]) -> Self {
        let mut pts: Vec<(i32, i32, usize)> = rects
            .iter()
            .enumerate()
            .map(|(k, r)| (r.i, r.j, k))
            .collect();
        pts.sort_unstable_by_key(|p| (p.0, p.2));
        let depth = (usize::BITS - rects.len().leading_zeros()) as usize + 1;
        let mut tree = Tree {
            nodes: Vec::with_capacity(2 * rects.len()),
            ys: Vec::with_capacity(rects.len() * depth),
        };
        let root = (!pts.is_empty()).then(|| tree.build(&pts));
        Self {
            rects: rects.to_vec(),
            tree,
            root,
            wide: rects.iter().map(|r| r.w).fold(0, i32::max),
            tall: rects.iter().map(|r| r.h).fold(0, i32::max),
        }
    }

    pub fn len(&self) -> usize {
        self.rects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    pub fn rect(&self, k: usize) -> Rect {
        self.rects[k]
    }

    /// The positions of the rectangles that overlap `q`, ascending.
    pub fn overlapping(&self, q: &Rect) -> Vec<usize> {
        let mut out = Vec::new();
        self.overlapping_into(q, &mut out);
        out
    }

    /// [`Self::overlapping`] into `out`, which it clears first.
    pub fn overlapping_into(&self, q: &Rect, out: &mut Vec<usize>) {
        out.clear();
        if let Some(root) = self.root {
            self.tree.corners(
                root,
                q.i - self.wide + 1,
                q.right() - 1,
                q.j - self.tall + 1,
                q.bottom() - 1,
                out,
            );
        }
        out.retain(|&k| self.rects[k].overlaps(q));
        out.sort_unstable();
    }

    /// The positions of the rectangles that share a side with `a`, ascending, into `out`: with
    /// no span negative, they all overlap the ring of tiles around it.
    pub fn touching_into(&self, a: &Rect, out: &mut Vec<usize>) {
        self.overlapping_into(&ring(a), out);
        out.retain(|&b| crate::place::touch(a, &self.rects[b]));
    }

    /// The positions of the rectangles that share a side with the one at `k`, ascending.
    pub fn touching(&self, k: usize) -> Vec<usize> {
        let mut out = Vec::new();
        self.touching_into(&self.rects[k], &mut out);
        out
    }

    /// The island of each rectangle as the first position in it, through shared sides.
    pub fn parts(&self) -> Vec<usize> {
        flood(self.rects.len(), |a, out| {
            self.touching_into(&self.rects[a], out)
        })
    }
}

/// The tiles around `a` and `a` itself.
pub const fn ring(a: &Rect) -> Rect {
    Rect::new(a.i - 1, a.j - 1, a.w + 2, a.h + 2)
}

/// The island of each of `n` positions as the first position in it: a flood from each position
/// not yet reached, through `neighbours`, which puts a position's neighbours into its buffer.
pub fn flood(n: usize, mut neighbours: impl FnMut(usize, &mut Vec<usize>)) -> Vec<usize> {
    let mut part = vec![usize::MAX; n];
    let mut stack = Vec::new();
    let mut near = Vec::new();
    for s in 0..n {
        if part[s] != usize::MAX {
            continue;
        }
        part[s] = s;
        stack.push(s);
        while let Some(a) = stack.pop() {
            neighbours(a, &mut near);
            for &b in &near {
                if part[b] == usize::MAX {
                    part[b] = s;
                    stack.push(b);
                }
            }
        }
    }
    part
}

/// The tiles `a` and `b` share.
pub const fn shared_area(a: &Rect, b: &Rect) -> i32 {
    let w = min(a.right(), b.right()) - max(a.i, b.i);
    let h = min(a.bottom(), b.bottom()) - max(a.j, b.j);
    if w > 0 && h > 0 {
        w * h
    } else {
        0
    }
}

const fn min(a: i32, b: i32) -> i32 {
    if a < b {
        a
    } else {
        b
    }
}

const fn max(a: i32, b: i32) -> i32 {
    if a > b {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> i32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n) as i32
        }
    }

    #[test]
    fn overlapping_and_touching_answer_what_a_scan_finds() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        for round in 0..400 {
            let n = rng.below(40) as usize;
            let rects: Vec<Rect> = (0..n)
                .map(|_| {
                    let w = rng.below(7) - i32::from(round % 5 == 0);
                    let h = rng.below(7) - i32::from(round % 7 == 0);
                    Rect::new(rng.below(30) - 15, rng.below(30) - 15, w, h)
                })
                .collect();
            let index = Index::new(&rects);
            for _ in 0..30 {
                let q = Rect::new(
                    rng.below(40) - 20,
                    rng.below(40) - 20,
                    rng.below(12) - 1,
                    rng.below(12) - 1,
                );
                let scan: Vec<usize> = (0..n).filter(|&k| rects[k].overlaps(&q)).collect();
                assert_eq!(index.overlapping(&q), scan, "{rects:?} {q:?}");
            }
            for k in 0..n {
                if rects.iter().any(|r| r.w < 0 || r.h < 0) {
                    break;
                }
                let scan: Vec<usize> = (0..n)
                    .filter(|&b| crate::place::touch(&rects[k], &rects[b]))
                    .collect();
                assert_eq!(index.touching(k), scan, "{rects:?} {k}");
            }
        }
    }

    #[test]
    fn shared_area_counts_the_shared_tiles() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..2000 {
            let a = Rect::new(
                rng.below(12) - 6,
                rng.below(12) - 6,
                rng.below(6),
                rng.below(6),
            );
            let b = Rect::new(
                rng.below(12) - 6,
                rng.below(12) - 6,
                rng.below(6),
                rng.below(6),
            );
            let count = a.cells().filter(|&(x, y)| b.contains(x, y)).count();
            assert_eq!(shared_area(&a, &b) as usize, count, "{a:?} {b:?}");
        }
    }
}
