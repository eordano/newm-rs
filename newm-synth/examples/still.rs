//! Writes one still of the look as a PNG, drawn on the CPU: for the lock screen, or to look.
//!
//! `cargo run --release -p newm-synth --example still -- OUT.png [WIDTH HEIGHT [SECONDS
//! [AUDIO]]]`, by default 1440x960 at 0 s with no sound, in the user's tuned look.

use newm_synth::{render, Frame, Look, Tables};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().map_or("synth-still.png", String::as_str);
    let num = |i: usize, d: f64| args.get(i).map_or(d, |a| a.parse().expect("a number"));
    let (w, h) = (num(1, 1440.) as usize, num(2, 960.) as usize);
    let (time, audio) = (num(3, 0.), num(4, 0.) as f32);

    let look = Look::default();
    let tables = Tables::new(look.seed);
    let frame = Frame::new(&look, w, h, time, audio);
    let mut px = vec![0u32; w * h];
    let start = std::time::Instant::now();
    render(&tables, &frame, &mut px, w, h, w);
    eprintln!("drew {w}x{h} in {:?}", start.elapsed());
    newm_synth::png::write(out.as_ref(), &px, w, h).expect("writing the PNG");
}
