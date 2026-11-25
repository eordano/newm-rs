//! A tap of the mod key alone with `canvas { super-tap-zoom }`: from 1:1 it zooms the camera out to
//! the step nearest the scale; from any other zoom it returns the camera to 1:1.

use niri_config::Config;
use smithay::backend::input::{InputEvent, InputTime, KeyState, Keycode};

use crate::tests::fixture::Fixture;
use crate::tests::test_input_backend::{TestInputBackend, TestKeyboardKeyEvent};

/// Super_L, niri's mod key outside a nested session.
const SUPER: u32 = 133;

fn set_up(canvas: &str) -> Fixture {
    let config = format!("layout {{\n canvas {{\n zoom-step 0.8\n zoom-max 7\n {canvas}\n }}\n}}");
    let mut f = Fixture::with_config(Config::parse_mem(&config).unwrap());
    f.add_output(1, (1920, 1080));
    f
}

fn zoom(f: &mut Fixture) -> i32 {
    f.niri().layout.canvas_zoom_state().unwrap().zoom
}

fn key(f: &mut Fixture, state: KeyState) {
    f.niri_state()
        .process_input_event(InputEvent::<TestInputBackend>::Keyboard {
            event: TestKeyboardKeyEvent {
                time: InputTime::from_micros(0),
                code: Keycode::new(SUPER),
                state,
                count: 1,
            },
        });
}

fn tap(f: &mut Fixture) {
    key(f, KeyState::Pressed);
    key(f, KeyState::Released);
}

#[test]
fn a_super_tap_zooms_out_and_the_next_returns_to_one_to_one() {
    let mut f = set_up("super-tap-zoom 0.33\nsuper-tap-overview");
    tap(&mut f);
    assert_eq!(zoom(&mut f), 5);
    assert!(!f.niri().layout.is_overview_open());
    tap(&mut f);
    assert_eq!(zoom(&mut f), 0);
}

#[test]
fn a_super_tap_at_any_other_zoom_returns_to_one_to_one() {
    let mut f = set_up("super-tap-zoom 0.33");
    f.niri().layout.canvas_zoom(-2);
    tap(&mut f);
    assert_eq!(zoom(&mut f), 0);
}

#[test]
fn without_super_tap_zoom_a_tap_keeps_the_zoom() {
    let mut f = set_up("");
    tap(&mut f);
    assert_eq!(zoom(&mut f), 0);
}
