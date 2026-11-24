use std::time::Duration;

use newm_synth::{level_of_rms, quantise, rms, Level, Look, ATTACK, RELEASE};

const TICK: Duration = Duration::from_millis(250);

#[test]
fn the_level_maps_loudness_on_a_decibel_scale() {
    assert_eq!(level_of_rms(0.), 0.);
    assert_eq!(level_of_rms(f32::NAN), 0.);
    // −50 dBFS and quieter is 0, −10 dBFS and louder is 1, −30 dBFS half way.
    assert_eq!(level_of_rms(10f32.powf(-50. / 20.)), 0.);
    assert!((level_of_rms(10f32.powf(-30. / 20.)) - 0.5).abs() < 1e-4);
    assert_eq!(level_of_rms(0.5), 1.);
    // A full-scale sine has an RMS of 1/√2.
    let sine: Vec<f32> = (0..480).map(|i| (i as f32 * 0.1309).sin()).collect();
    assert!((rms(&sine) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
}

#[test]
fn the_level_rises_fast_and_falls_slowly() {
    let mut l = Level::new();
    l.feed(1.);
    let up = l.advance(Duration::from_secs_f32(ATTACK));
    assert!(
        (up - (1. - (-1f32).exp())).abs() < 1e-4,
        "one attack time: 63%: {up}"
    );
    let mut l = Level::new();
    l.feed(1.);
    assert!(
        l.advance(TICK) > 0.99,
        "a quarter second is five attack times"
    );
    let down = l.advance(Duration::from_secs_f32(RELEASE));
    assert!(
        (down - (-1f32).exp()).abs() < 0.01,
        "one release time: 37% left: {down}"
    );
}

#[test]
fn a_beat_between_two_frames_still_counts() {
    let mut l = Level::new();
    for block in [0.1, 0.9, 0.2] {
        l.feed(block);
    }
    assert!(l.pending());
    assert!(l.advance(TICK) > 0.89, "the loudest block of the frame");
    assert!(!l.pending());
}

#[test]
fn silence_settles_to_exactly_zero() {
    let mut l = Level::new();
    l.feed(1.);
    l.advance(TICK);
    for _ in 0..40 {
        l.advance(TICK);
    }
    assert_eq!(
        l.level(),
        0.,
        "ten seconds of release reach zero, so nothing keeps drawing"
    );
}

#[test]
fn the_tuned_look_draws_the_level_in_visible_steps() {
    let look = Look::default();
    // On a 1920-row display: the nearest ridge rises 0.02 × 0.15 × 1920 = 5.8 px at full level,
    // so a quarter pixel is 1/23 of the level; the grid's 6% brightens its green by 21 levels,
    // one level per 1/21. The finer of the two decides.
    let q = look.level_quantum(1920).unwrap();
    assert!((q - 0.25 / 5.76).abs() < 1e-4, "{q}");
    // On a 960-row picture the ridge moves half as many pixels, and the grid decides.
    let q960 = look.level_quantum(960).unwrap();
    assert!(q960 > q && q960 < 0.05, "{q960}");
    // No amounts, no steps: sound moves nothing.
    let deaf = Look {
        audio_grid: 0.,
        audio_ridge: 0.,
        audio_sun: 0.,
        ..look
    };
    assert!(!deaf.hears());
    assert_eq!(deaf.level_quantum(1920), None);
}

#[test]
fn a_change_within_a_step_draws_the_same() {
    let q = Look::default().level_quantum(1920).unwrap();
    let (a, _) = quantise(0.30, q);
    let (b, _) = quantise(0.30 + q * 0.4, q);
    let (c, drawn) = quantise(0.30 + q * 1.2, q);
    assert_eq!(a, b, "under half a step either side draws the same");
    assert_ne!(a, c);
    assert!((drawn - c as f32 * q).abs() < 1e-6 && drawn <= 1.);
    assert_eq!(quantise(0., q).0, 0);
}
