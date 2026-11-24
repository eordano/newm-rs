//! How long the movement decisions take as the canvas fills, and a digest of every layout they
//! leave, so that a faster implementation can show it decides the same moves.
//!
//! `cargo test --release --test bench -- --ignored --nocapture`

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use newm_canvas::{Canvas, Config, Dir, Edge};

const DIRS: [Dir; 4] = [Dir::Left, Dir::Right, Dir::Up, Dir::Down];
const EDGES: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];
/// Minimum sizes in logical px: at 119x113 tiles a 960 px window spans 9x9.
const MINS: [(f64, f64); 5] = [
    (960.0, 960.0),
    (600.0, 400.0),
    (1200.0, 900.0),
    (300.0, 300.0),
    (450.0, 1000.0),
];

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

#[derive(Default)]
struct Times(Vec<Duration>);

impl Times {
    fn line(&mut self, name: &str) -> String {
        if self.0.is_empty() {
            return format!("{name:>8}: -");
        }
        self.0.sort();
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        let sum: Duration = self.0.iter().sum();
        format!(
            "{name:>8}: n={:>3} median {:>8.3} ms  p90 {:>8.3} ms  max {:>8.3} ms  sum {:>9.2} ms",
            self.0.len(),
            ms(self.0[self.0.len() / 2]),
            ms(self.0[self.0.len() * 9 / 10]),
            ms(*self.0.last().unwrap()),
            ms(sum)
        )
    }
}

fn digest(c: &Canvas, h: &mut DefaultHasher) {
    for w in c.windows() {
        (w.id, w.rect.i, w.rect.j, w.rect.w, w.rect.h).hash(h);
    }
}

fn run(tile: (f64, f64), n: usize, ops: usize, seed: u64) -> (u64, Vec<String>) {
    let config = Config {
        tile_w: tile.0,
        tile_h: tile.1,
        ..Config::default()
    };
    let mut c = Canvas::new(config, 2560.0, 1440.0);
    let mut rng = Rng(seed);
    let mut h = DefaultHasher::new();
    for id in 1..=n as u64 {
        let (w, hh) = MINS[rng.below(MINS.len() as u64) as usize];
        c.add(id, w, hh, None);
        digest(&c, &mut h);
    }
    let names = ["move_by", "move_to", "swap", "grow", "shrink", "remove"];
    let mut times: Vec<Times> = names.iter().map(|_| Times::default()).collect();
    let mut next = n as u64;
    for _ in 0..ops {
        let ids: Vec<u64> = c.windows().iter().map(|w| w.id).collect();
        let id = ids[rng.below(ids.len() as u64) as usize];
        let dir = DIRS[rng.below(4) as usize];
        let kind = rng.below(10) as usize;
        let t = Instant::now();
        let slot = match kind {
            0..=3 => {
                c.move_by(id, dir);
                0
            }
            4 | 5 => {
                let r = c.rect(id).unwrap();
                let (di, dj) = (rng.below(13) as i32 - 6, rng.below(9) as i32 - 4);
                c.move_to(id, r.i + di, r.j + dj);
                1
            }
            6 => {
                c.focus_id(id);
                let t = Instant::now();
                c.swap(dir);
                times[2].0.push(t.elapsed());
                digest(&c, &mut h);
                continue;
            }
            7 => {
                c.resize(id, EDGES[rng.below(4) as usize], 1);
                3
            }
            8 => {
                c.resize(id, EDGES[rng.below(4) as usize], -1);
                4
            }
            _ => {
                c.remove(id);
                let e = t.elapsed();
                next += 1;
                let (w, hh) = MINS[rng.below(MINS.len() as u64) as usize];
                c.add(next, w, hh, None);
                times[5].0.push(e);
                digest(&c, &mut h);
                continue;
            }
        };
        times[slot].0.push(t.elapsed());
        c.check().unwrap();
        digest(&c, &mut h);
    }
    let lines = names
        .iter()
        .zip(times.iter_mut())
        .map(|(n, t)| t.line(n))
        .collect();
    (h.finish(), lines)
}

#[test]
#[ignore]
fn movement_bench() {
    let sizes: Vec<usize> = std::env::var("BENCH_N")
        .map(|s| s.split(',').map(|x| x.parse().unwrap()).collect())
        .unwrap_or_else(|_| vec![8, 16, 32, 64]);
    let ops: usize = std::env::var("BENCH_OPS")
        .map(|s| s.parse().unwrap())
        .unwrap_or(120);
    for tile in [(119.0, 113.0), (720.0, 465.0)] {
        for &n in &sizes {
            let t = Instant::now();
            let (d, lines) = run(tile, n, ops, 0x9e37_79b9_7f4a_7c15 ^ n as u64);
            println!(
                "tile {}x{} windows {n}: digest {d:016x} total {:.1} ms",
                tile.0,
                tile.1,
                t.elapsed().as_secs_f64() * 1e3
            );
            for l in lines {
                println!("  {l}");
            }
        }
    }
}
