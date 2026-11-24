//! Times the CPU port and the per-frame work: `cargo run --release -p newm-synth --example
//! bench`.

use std::hint::black_box;
use std::time::Instant;

use newm_synth::{render, Frame, Look, Tables};

fn main() {
    let look = Look::default();
    let start = Instant::now();
    let tables = black_box(Tables::new(look.seed));
    println!("Tables::new: {:?}", start.elapsed());

    let n = 1_000_000u32;
    let start = Instant::now();
    for i in 0..n {
        black_box(Frame::new(&look, 2880, 1920, f64::from(i) * 0.25, 0.));
    }
    println!(
        "Frame::new: {:.0} ns",
        start.elapsed().as_secs_f64() * 1e9 / f64::from(n)
    );

    for (w, h) in [(360, 240), (1440, 960), (2560, 720)] {
        let frame = Frame::new(&look, w, h, 60., 0.);
        let mut px = vec![0u32; w * h];
        let start = Instant::now();
        let reps = 3;
        for _ in 0..reps {
            render(&tables, black_box(&frame), &mut px, w, h, w);
        }
        let per = start.elapsed() / reps;
        println!(
            "render (CPU, one core) {w}x{h}: {per:?} ({:.0} ns/px)",
            per.as_secs_f64() * 1e9 / (w * h) as f64
        );
    }
}
