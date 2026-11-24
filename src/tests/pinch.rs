//! Touchpad pinches on the canvas: three or more fingers, or any with the mod key held, zoom the
//! camera; a plain two-finger pinch goes on to the app.

use smithay::backend::input::{InputEvent, InputTime, KeyState, Keycode};

use crate::tests::fixture::Fixture;
use crate::tests::test_input_backend::{
    TestInputBackend, TestKeyboardKeyEvent, TestPinchBegin, TestPinchEnd, TestPinchUpdate,
};

/// Super_L, niri's mod key outside a nested session.
const SUPER: u32 = 133;

fn set_up() -> Fixture {
    let mut f = Fixture::new();
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

/// A pinch with `fingers`, the fingers reaching each of `scales` in turn, then lifting.
fn pinch(f: &mut Fixture, fingers: u32, scales: &[f64]) {
    let state = f.niri_state();
    state.process_input_event(InputEvent::<TestInputBackend>::GesturePinchBegin {
        event: TestPinchBegin { fingers },
    });
    for &scale in scales {
        state.process_input_event(InputEvent::<TestInputBackend>::GesturePinchUpdate {
            event: TestPinchUpdate { scale },
        });
    }
    state.process_input_event(InputEvent::<TestInputBackend>::GesturePinchEnd {
        event: TestPinchEnd,
    });
}

#[test]
fn a_three_finger_pinch_zooms_the_camera_in_whole_steps() {
    let mut f = set_up();
    assert_eq!(zoom(&mut f), 0);
    // Pinching in pulls the camera back: 0.6 is 2.3 steps of the default zoom-step 0.8.
    pinch(&mut f, 3, &[0.9, 0.75, 0.6]);
    assert_eq!(
        zoom(&mut f),
        2,
        "the release keeps the zoom the pinch reached"
    );
    // Spreading brings it closer, from the zoom this pinch began at.
    pinch(&mut f, 4, &[1.3, 1.6]);
    assert_eq!(zoom(&mut f), 0);
}

#[test]
fn a_pinch_follows_the_fingers_both_ways_until_they_lift() {
    let mut f = set_up();
    pinch(&mut f, 3, &[0.6, 1.0]);
    assert_eq!(
        zoom(&mut f),
        0,
        "back where the fingers began, back at the zoom it began at"
    );
}

#[test]
fn a_pinch_stays_within_zoom_min_and_zoom_max() {
    let mut f = set_up();
    pinch(&mut f, 3, &[0.01]);
    assert_eq!(zoom(&mut f), 4, "the default zoom-max");
    pinch(&mut f, 3, &[1000.0]);
    assert_eq!(zoom(&mut f), -2, "the default zoom-min");
}

#[test]
fn the_mod_key_and_two_fingers_zoom_the_camera() {
    let mut f = set_up();
    key(&mut f, KeyState::Pressed);
    pinch(&mut f, 2, &[0.8]);
    key(&mut f, KeyState::Released);
    assert_eq!(zoom(&mut f), 1);
}

#[test]
fn a_plain_two_finger_pinch_goes_on_to_the_app() {
    let mut f = set_up();
    pinch(&mut f, 2, &[0.5, 0.25]);
    assert_eq!(zoom(&mut f), 0);
}
