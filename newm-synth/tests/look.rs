use newm_synth::{render, Frame, Look, Tables};

fn draw(look: &Look, w: usize, h: usize, time: f64, audio: f32) -> Vec<u32> {
    let tables = Tables::new(look.seed);
    let frame = Frame::new(look, w, h, time, audio);
    let mut px = vec![0u32; w * h];
    render(&tables, &frame, &mut px, w, h, w);
    px
}

/// The largest channel difference and how many pixels differ by more than `levels`.
fn diff(a: &[u32], b: &[u32], levels: u8) -> (u8, usize) {
    let mut max = 0;
    let mut off = 0;
    for (p, q) in a.iter().zip(b) {
        let d = (0..3)
            .map(|i| p.to_le_bytes()[i].abs_diff(q.to_le_bytes()[i]))
            .max()
            .unwrap();
        max = max.max(d);
        if d > levels {
            off += 1;
        }
    }
    (max, off)
}

/// The user's look with every time-dependent term off.
fn frozen() -> Look {
    Look {
        stars: 0.,
        grid_speed: 0.,
        ridge_speed: 0.,
        reflection: 0.,
        cycle_speed: 0.,
        ..Look::default()
    }
}

#[test]
fn the_tuned_look_moves() {
    assert!(Look::default().moves());
}

#[test]
fn with_every_time_term_off_the_picture_never_changes() {
    let look = frozen();
    assert!(!look.moves());
    let first = draw(&look, 240, 160, 0., 0.);
    for t in [0.25, 60., 600., 86_400.5, 3e7] {
        assert_eq!(draw(&look, 240, 160, t, 0.), first, "at {t} s");
    }
}

#[test]
fn each_time_term_alone_moves_the_picture_and_says_so() {
    let base = frozen();
    let terms: [(&str, Look); 6] = [
        (
            "stars",
            Look {
                stars: 0.69,
                ..base
            },
        ),
        // The sun cuts move with the grid speed; without a floor the grid cannot show.
        (
            "sun cuts",
            Look {
                grid_speed: 0.08,
                horizon: -0.6,
                ..base
            },
        ),
        (
            "ridges",
            Look {
                ridge_speed: 6.9,
                ..base
            },
        ),
        (
            "grid",
            Look {
                grid_speed: 0.08,
                ..base
            },
        ),
        (
            "reflection",
            Look {
                reflection: 0.45,
                ..base
            },
        ),
        (
            "cycle",
            Look {
                cycle_speed: 0.15,
                ..base
            },
        ),
    ];
    for (name, look) in terms {
        assert!(look.moves(), "{name}");
        let a = draw(&look, 240, 160, 0., 0.);
        let b = draw(&look, 240, 160, 7.3, 0.);
        assert_ne!(a, b, "{name} changes the picture");
    }
    // The cycle moves nothing when it neither turns the hue nor moves the sun.
    let idle_cycle = Look {
        cycle_speed: 0.15,
        cycle_hue: 0.,
        cycle_sun: 0.,
        ..base
    };
    assert!(!idle_cycle.moves());
}

#[test]
fn wrapping_the_time_terms_changes_nothing_visible() {
    // After 5000 s the ridge offsets have wrapped around the noise table; drawn wrapped and
    // unwrapped (which f32 still holds well at this time), the pictures agree.
    let look = Look::default();
    let tables = Tables::new(look.seed);
    let (w, h) = (320, 200);
    for t in [5000., 20_000.] {
        let mut a = vec![0u32; w * h];
        let mut b = vec![0u32; w * h];
        render(&tables, &Frame::new(&look, w, h, t, 0.), &mut a, w, h, w);
        render(
            &tables,
            &Frame::unwrapped(&look, w, h, t, 0.),
            &mut b,
            w,
            h,
            w,
        );
        let (max, off) = diff(&a, &b, 2);
        assert!(off * 1000 <= w * h, "{t} s: {off} pixels off, max {max}");
    }
}

#[test]
fn the_ridges_drift_without_a_seam() {
    // Across the moment the nearest layer's base offset wraps, consecutive frames 1/4 s apart
    // differ no more than frames elsewhere do.
    let look = Look {
        stars: 0.,
        grid_speed: 0.,
        reflection: 0.,
        cycle_speed: 0.,
        ..Look::default()
    };
    // The nearest layer drifts 6.9 × 0.02 = 0.138 cells/s; its offset (2 × 37.1 + drift)
    // reaches 1024 at (1024 − 74.2) / 0.138 s.
    let wrap = (1024. - 74.2) / 0.138;
    let step = |t: f64| {
        let a = draw(&look, 320, 200, t, 0.);
        let b = draw(&look, 320, 200, t + 0.25, 0.);
        diff(&a, &b, 8).1
    };
    let across = step(wrap - 0.125);
    let elsewhere = step(wrap - 100.);
    assert!(
        across <= elsewhere * 2 + 10,
        "across the wrap {across}, elsewhere {elsewhere}"
    );
}

#[test]
fn tables_follow_the_seed() {
    assert_eq!(Tables::new(50), Tables::new(50));
    assert_ne!(Tables::new(50).rgba(), Tables::new(51).rgba());
    let t = Tables::new(50);
    // Values sit in 0..1 and repeat with the table.
    assert_eq!(t.ridge(3), t.ridge(3 + 1024));
    assert_eq!(t.star(-1, 5), t.star(255, 261));
    assert!((0..1024).all(|n| (0. ..1.).contains(&t.ridge(n))));
}

#[test]
fn star_density_matches_the_stars_knob() {
    // `stars 0.69` lights cells whose first hash is at least 1 − 0.69 × 0.12: 8.3% of them.
    let t = Tables::new(50);
    let lit = (0..256)
        .flat_map(|y| (0..256).map(move |x| (x, y)))
        .filter(|&(x, y)| t.star(x, y)[0] >= 1. - 0.69 * 0.12)
        .count();
    let share = lit as f64 / 65536.;
    assert!((0.075..0.091).contains(&share), "{share}");
}

/// The playground's hash, `fract(sin(n·127.1 + seed·17.3)·43758.5453)`, with a correctly
/// rounded `sin`, or with the argument first reduced by a float 2π as GPU sine units do (their
/// sine is exact only on one period; GLSL leaves `sin`'s precision to the driver).
fn lab_hash(n: f32, reduced: bool) -> f32 {
    let arg = n * 127.1 + 50. * 17.3;
    let s = if reduced {
        let tau = std::f32::consts::TAU;
        (arg - (arg / tau).round() * tau).sin()
    } else {
        f64::from(arg).sin() as f32
    };
    // The playground writes 43758.5453; as an f32 it is 43758.547.
    let v = s * 43_758.547;
    v - v.floor()
}

#[test]
fn the_playgrounds_hash_depends_on_the_driver() {
    // The cells the ridges' octaves use reach into the thousands, where reducing the argument
    // by a float 2π is off by about 1e-4. Times 43758 that moves the hash by whole units: the
    // value is decided by the driver's sine, not by the seed.
    let differ = (0..4000)
        .map(|n| n as f32)
        .filter(|&n| (lab_hash(n, false) - lab_hash(n, true)).abs() > 0.01)
        .count();
    eprintln!("{differ} of 4000 ridge cells hash differently");
    assert!(differ > 2000, "{differ} of 4000 cells");
}

#[test]
fn painting_rows_draws_exactly_what_shade_gives() {
    let looks = [
        Look::default(),
        Look {
            bits: 3.,
            scanlines: 0.6,
            curve: -0.2,
            layers: 6.,
            ..Look::default()
        },
    ];
    for look in looks {
        let tables = Tables::new(look.seed);
        let (w, h) = (173, 97);
        let frame = Frame::new(&look, w, h, 123.4, 0.5);
        let whole = draw(&look, w, h, 123.4, 0.5);
        for y in 0..h {
            for x in 0..w {
                assert_eq!(
                    whole[y * w + x],
                    newm_synth::shade(&tables, &frame, x, y),
                    "{x},{y}"
                );
            }
        }
        // A band of rows drawn alone matches the same rows of the whole picture.
        let mut band = vec![0u32; w * 20];
        newm_synth::Painter::new().paint_rows(&tables, &frame, &mut band, w, 40..60, w);
        assert_eq!(band, whole[40 * w..60 * w]);
    }
}
