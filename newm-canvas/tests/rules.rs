//! Three proposed rules, not yet confirmed (newm-canvas proof F06, F07 and F08), measured against
//! what the canvas does now:
//!
//! 1. A move reassigns as few tiles as possible: no layout that puts the moved windows where the
//!    move put them, keeps every other span and overlaps nothing reassigns fewer tiles.
//! 2. A rearrangement splits no island: two windows other than the moved ones, connected without
//!    the moved ones before it, stay connected after it, through any window, the moved ones in
//!    their new places included. Windows connect through a shared side. A split the moved window
//!    causes by leaving, as the bridge, is allowed, and so is a moved window left on its own where
//!    the move put it.
//! 3. A window that makes room keeps its row or its column: it moves along one axis only. The moved
//!    windows are exempt.
//!
//! `cargo test --release --test rules -- --ignored --nocapture` prints a report and the smallest
//! counterexample of each.

use std::collections::{HashMap, HashSet};

use newm_canvas::{Canvas, Config, Dir, Edge, Rect};

type Layout = Vec<(u64, Rect)>;

fn layout(c: &Canvas) -> Layout {
    let mut l: Layout = c.windows().iter().map(|w| (w.id, w.rect)).collect();
    l.sort_by_key(|x| x.0);
    l
}

fn owners(l: &[(u64, Rect)]) -> HashMap<(i32, i32), u64> {
    let mut m = HashMap::new();
    for &(id, r) in l {
        for x in r.i..r.right() {
            for y in r.j..r.bottom() {
                m.insert((x, y), id);
            }
        }
    }
    m
}

/// Tiles whose owner, a window or none, differs between two layouts.
fn displaced(a: &[(u64, Rect)], b: &[(u64, Rect)]) -> usize {
    let (oa, ob) = (owners(a), owners(b));
    let keys: HashSet<_> = oa.keys().chain(ob.keys()).collect();
    keys.into_iter().filter(|k| oa.get(k) != ob.get(k)).count()
}

/// Two windows touch when they share a side of positive length; a corner does not count.
fn touch(a: Rect, b: Rect) -> bool {
    ((a.right() == b.i || b.right() == a.i) && a.rows_overlap(&b))
        || ((a.bottom() == b.j || b.bottom() == a.j) && a.cols_overlap(&b))
}

/// The island of each window of `l` but `skip`, as the smallest id in it.
fn islands(l: &[(u64, Rect)], skip: &[u64]) -> HashMap<u64, u64> {
    let ws: Vec<_> = l.iter().filter(|w| !skip.contains(&w.0)).collect();
    let mut label: HashMap<u64, u64> = ws.iter().map(|w| (w.0, w.0)).collect();
    loop {
        let mut changed = false;
        for a in &ws {
            for b in &ws {
                if touch(a.1, b.1) && label[&b.0] < label[&a.0] {
                    label.insert(a.0, label[&b.0]);
                    changed = true;
                }
            }
        }
        if !changed {
            return label;
        }
    }
}

/// Rule 3: a window other than `movers` that changed both its row and its column.
fn bent(before: &[(u64, Rect)], after: &[(u64, Rect)], movers: &[u64]) -> Option<u64> {
    after.iter().find_map(|&(id, r)| {
        let (_, o) = before.iter().find(|w| w.0 == id)?;
        (!movers.contains(&id) && r.i != o.i && r.j != o.j).then_some(id)
    })
}

/// Rule 2: two windows other than `movers`, connected without them before, that are not
/// connected after.
fn split(before: &[(u64, Rect)], after: &[(u64, Rect)], movers: &[u64]) -> Option<(u64, u64)> {
    let (b, a) = (islands(before, movers), islands(after, &[]));
    for (&x, &lx) in &b {
        for (&y, &ly) in &b {
            if x < y && lx == ly && a.contains_key(&x) && a.contains_key(&y) && a[&x] != a[&y] {
                return Some((x, y));
            }
        }
    }
    None
}

struct Search<'a> {
    before: &'a [(u64, Rect)],
    movers: Vec<u64>,
    connected: bool,
    straight: bool,
    best: usize,
    best_layout: Option<Layout>,
    nodes: usize,
    budget: usize,
}

impl Search<'_> {
    fn run(&mut self, cur: &mut Layout, placed: &mut Vec<bool>) {
        self.nodes += 1;
        if self.nodes > self.budget {
            return;
        }
        let cost = displaced(self.before, cur);
        if cost >= self.best {
            return;
        }
        let conflict = (0..cur.len()).find(|&k| {
            !placed[k] && (0..cur.len()).any(|p| placed[p] && cur[k].1.overlaps(&cur[p].1))
        });
        let Some(k) = conflict else {
            if !self.connected || split(self.before, cur, &self.movers).is_none() {
                self.best = cost;
                self.best_layout = Some(cur.clone());
            }
            return;
        };
        let r = cur[k].1;
        let (x0, y0, x1, y1) = cur
            .iter()
            .fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |b, w| {
                (
                    b.0.min(w.1.i),
                    b.1.min(w.1.j),
                    b.2.max(w.1.right()),
                    b.3.max(w.1.bottom()),
                )
            });
        let mut cands: Vec<Rect> = Vec::new();
        for i in x0 - r.w - 1..=x1 + 1 {
            for j in y0 - r.h - 1..=y1 + 1 {
                let c = Rect::new(i, j, r.w, r.h);
                let along = !self.straight || i == r.i || j == r.j;
                if c != r && along && (0..cur.len()).all(|p| !placed[p] || !c.overlaps(&cur[p].1)) {
                    cands.push(c);
                }
            }
        }
        cands.sort_by_key(|c| ((c.i - r.i).abs() + (c.j - r.j).abs(), c.j, c.i));
        placed[k] = true;
        for c in cands {
            cur[k].1 = c;
            self.run(cur, placed);
        }
        cur[k].1 = r;
        placed[k] = false;
    }
}

/// The fewest tiles any layout reassigns that puts each of `targets` on its rectangle, keeps
/// every other window's span and overlaps nothing; with `connected` it also keeps rule 2, with
/// `straight` rule 3. A window moves only because a placed one lands on it (moving any other
/// would only add tiles), to any free spot in the region around the layout. Err past `budget`
/// nodes; Ok(None) when no layout keeps the rules.
fn fewest(
    before: &[(u64, Rect)],
    targets: &[(u64, Rect)],
    connected: bool,
    straight: bool,
    budget: usize,
) -> Result<Option<(usize, Layout)>, ()> {
    let mut cur: Layout = before.to_vec();
    let mut placed = vec![false; cur.len()];
    for &(id, r) in targets {
        let k = cur.iter().position(|w| w.0 == id).ok_or(())?;
        cur[k].1 = r;
        placed[k] = true;
    }
    let mut s = Search {
        before,
        movers: targets.iter().map(|t| t.0).collect(),
        connected,
        straight,
        best: usize::MAX,
        best_layout: None,
        nodes: 0,
        budget,
    };
    s.run(&mut cur, &mut placed);
    if s.nodes > s.budget {
        return Err(());
    }
    Ok(s.best_layout.map(|l| (s.best, l)))
}

#[derive(Debug, Clone, Copy)]
enum Op {
    Add(u64, (f64, f64)),
    Remove(u64),
    Focus(u64),
    Resize(u64, Edge, i32),
    MoveBy(u64, Dir),
    MoveTo(u64, i32, i32),
    Swap(Dir),
    SendToEdge(Dir),
}

const DIRS: [Dir; 4] = [Dir::Left, Dir::Right, Dir::Up, Dir::Down];
const EDGES: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];
const MINS: [(f64, f64); 2] = [(100.0, 100.0), (800.0, 300.0)];

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

fn random_op(rng: &mut Rng, ids: &[u64], next: &mut u64) -> Op {
    if ids.is_empty() || rng.below(4) == 0 {
        *next += 1;
        return Op::Add(*next, MINS[rng.below(2) as usize]);
    }
    let any = |rng: &mut Rng| ids[rng.below(ids.len() as u64) as usize];
    let dir = DIRS[rng.below(4) as usize];
    match rng.below(8) {
        0 => Op::Remove(any(rng)),
        1 => Op::Focus(any(rng)),
        2 => Op::Resize(
            any(rng),
            EDGES[rng.below(4) as usize],
            if rng.below(2) == 0 { 1 } else { -1 },
        ),
        3 | 4 => Op::MoveBy(any(rng), dir),
        5 => Op::MoveTo(any(rng), rng.below(6) as i32 - 2, rng.below(4) as i32 - 1),
        6 => Op::Swap(dir),
        _ => Op::SendToEdge(if rng.below(2) == 0 {
            Dir::Left
        } else {
            Dir::Right
        }),
    }
}

/// The op in the Rocq model's terms (newm-canvas/proof/Canvas.v).
fn rocq(op: Op, swap_with: Option<u64>) -> String {
    let d = |d: Dir| format!("{d:?}");
    let e = |e: Edge| match e {
        Edge::Left => "ELeft",
        Edge::Right => "ERight",
        Edge::Top => "ETop",
        Edge::Bottom => "EBottom",
    };
    match op {
        Op::Add(id, (w, h)) => format!("Add {id} ({w}, {h}) None"),
        Op::Remove(id) => format!("Remove {id}"),
        Op::Focus(id) => format!("FocusId {id}"),
        Op::Resize(id, ed, k) => format!("Resize {id} {} ({k})", e(ed)),
        Op::MoveBy(id, dir) => format!("MoveBy {id} {}", d(dir)),
        Op::MoveTo(id, i, j) => format!("MoveTo {id} ({i}) ({j})"),
        Op::Swap(dir) => format!("Swap {} {}", swap_with.unwrap_or(0), d(dir)),
        Op::SendToEdge(dir) => format!("SendToEdge {}", d(dir)),
    }
}

/// Applies `op`; the windows it moves on purpose, with where they ended, and whether it is a
/// move (rule 1 applies) rather than a resize, a close or a focus.
fn apply(c: &mut Canvas, op: Op) -> (Vec<u64>, bool, Option<u64>) {
    match op {
        Op::Add(id, (w, h)) => {
            c.add(id, w, h, None);
            (vec![], false, None)
        }
        Op::Remove(id) => {
            c.remove(id);
            (vec![id], false, None)
        }
        Op::Focus(id) => {
            c.focus_id(id);
            (vec![], false, None)
        }
        Op::Resize(id, e, d) => {
            let grew = c.resize(id, e, d) && d > 0;
            (vec![id], grew, None)
        }
        Op::MoveBy(id, d) => (if c.move_by(id, d) { vec![id] } else { vec![] }, true, None),
        Op::MoveTo(id, i, j) => (
            if c.move_to(id, i, j) {
                vec![id]
            } else {
                vec![]
            },
            true,
            None,
        ),
        Op::Swap(d) => {
            let pair = c.focus().and_then(|f| Some((f, c.neighbour(f, d)?)));
            match pair {
                Some((f, n)) if c.swap(d) => (vec![f, n], true, Some(n)),
                _ => (vec![], true, None),
            }
        }
        Op::SendToEdge(d) => {
            let f = c.focus();
            match f {
                Some(f) if c.send_to_edge(d) => (vec![f], true, None),
                _ => (vec![], true, None),
            }
        }
    }
}

#[derive(Default)]
struct Tally {
    seen: usize,
    worse: usize,
    excess: usize,
    unknown: usize,
    none: usize,
    smallest: Option<(usize, String)>,
}

impl Tally {
    fn keep(&mut self, size: usize, report: impl FnOnce() -> String) {
        if self.smallest.as_ref().is_none_or(|s| size < s.0) {
            self.smallest = Some((size, report()));
        }
    }
}

fn kind(op: Op) -> &'static str {
    match op {
        Op::Add(..) => "add",
        Op::Remove(..) => "remove",
        Op::Focus(..) => "focus",
        Op::Resize(_, _, d) if d > 0 => "grow",
        Op::Resize(..) => "shrink",
        Op::MoveBy(..) => "move_by",
        Op::MoveTo(..) => "move_to",
        Op::Swap(..) => "swap",
        Op::SendToEdge(..) => "send_to_edge",
    }
}

#[test]
#[ignore = "a report on two proposed rules, run by hand"]
fn proposed_rules_survey() {
    let mut rule1: HashMap<&str, Tally> = HashMap::new();
    let mut rule12: HashMap<&str, Tally> = HashMap::new();
    let mut rule2: HashMap<&str, Tally> = HashMap::new();
    let mut rule3: HashMap<&str, Tally> = HashMap::new();
    let mut rule123: HashMap<&str, Tally> = HashMap::new();
    let mut price3: HashMap<&str, Tally> = HashMap::new();
    for seed in 1..=3000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut c = Canvas::new(Config::default(), 2560.0, 1440.0);
        let (mut next, mut trace) = (0, Vec::new());
        for _ in 0..14 {
            let ids: Vec<u64> = c.windows().iter().map(|w| w.id).collect();
            let op = random_op(&mut rng, &ids, &mut next);
            let before = layout(&c);
            let (movers, is_move, swap_with) = apply(&mut c, op);
            trace.push(rocq(op, swap_with));
            let after = layout(&c);
            let k = kind(op);
            let size = before.len() * 100 + trace.len();
            let show = |extra: String| {
                format!(
                    "trace: [{}]\nbefore: {before:?}\nafter:  {after:?}\n{extra}",
                    trace.join("; ")
                )
            };
            if !movers.is_empty() {
                let t = rule3.entry(k).or_default();
                t.seen += 1;
                if let Some(x) = bent(&before, &after, &movers) {
                    t.worse += 1;
                    t.keep(size, || {
                        show(format!("{x} moved along both axes, movers {movers:?}"))
                    });
                }
            }
            if !movers.is_empty() {
                let t = rule2.entry(k).or_default();
                t.seen += 1;
                if let Some((x, y)) = split(&before, &after, &movers) {
                    t.worse += 1;
                    t.keep(size, || {
                        show(format!("{x} and {y} split, movers {movers:?}"))
                    });
                }
            }
            if !is_move || movers.is_empty() || before.len() > 7 {
                continue;
            }
            let targets: Vec<(u64, Rect)> = after
                .iter()
                .copied()
                .filter(|w| movers.contains(&w.0))
                .collect();
            let did = displaced(&before, &after);
            let mut found = [None, None, None];
            for (n, (table, connected, straight)) in [
                (&mut rule1, false, false),
                (&mut rule12, true, false),
                (&mut rule123, true, true),
            ]
            .into_iter()
            .enumerate()
            {
                let t = table.entry(k).or_default();
                t.seen += 1;
                match fewest(&before, &targets, connected, straight, 400_000) {
                    Err(()) => t.unknown += 1,
                    Ok(None) => t.none += 1,
                    Ok(Some((best, alt))) => {
                        found[n] = Some(best);
                        if best < did {
                            t.worse += 1;
                            t.excess += did - best;
                            t.keep(size, || {
                                show(format!("reassigned {did}, fewest {best}: {alt:?}"))
                            });
                        }
                    }
                }
            }
            if let (Some(a), Some(b)) = (found[1], found[2]) {
                let t = price3.entry(k).or_default();
                t.seen += 1;
                if b > a {
                    t.worse += 1;
                    t.excess += b - a;
                }
            }
        }
    }
    for (name, table) in [
        ("rule 1, fewest tiles", &rule1),
        ("rule 1 within rule 2", &rule12),
        ("rule 1 within rules 2 and 3", &rule123),
        ("price of rule 3 over rules 1 and 2", &price3),
        ("rule 2, no split", &rule2),
        ("rule 3, one axis", &rule3),
    ] {
        println!("== {name}");
        let mut kinds: Vec<_> = table.iter().collect();
        kinds.sort_by_key(|k| k.0);
        for (k, t) in kinds {
            println!(
                "{k:>13}: {} of {} worse, {} tiles over, {} past the budget, {} with no layout",
                t.worse, t.seen, t.excess, t.unknown, t.none
            );
            if let Some((_, s)) = &t.smallest {
                println!("{s}\n");
            }
        }
    }
}
