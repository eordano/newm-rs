use super::*;

fn pointer_location(f: &mut Fixture) -> (f64, f64) {
    let loc = f.niri().seat.get_pointer().unwrap().current_location();
    (loc.x, loc.y)
}

#[test]
fn absolute_motion_lands_in_global_coordinates_after_authenticate() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));
    let id = f.add_client();
    let fake = f
        .client(id)
        .state
        .fake_input
        .clone()
        .expect("fake input global");

    // Before authenticate, KWin ignores requests, and so does newm-rs.
    let before = pointer_location(&mut f);
    fake.pointer_motion_absolute(300., 200.);
    f.roundtrip(id);
    assert_eq!(pointer_location(&mut f), before);

    fake.authenticate("test".into(), "remote input".into());
    fake.pointer_motion_absolute(300., 200.);
    f.roundtrip(id);
    assert_eq!(pointer_location(&mut f), (300., 200.));

    // The second output sits right of the first: global coordinates reach it.
    fake.pointer_motion_absolute(2000., 100.);
    f.roundtrip(id);
    assert_eq!(pointer_location(&mut f), (2000., 100.));

    fake.pointer_motion(-50., 30.);
    f.roundtrip(id);
    let (x, y) = pointer_location(&mut f);
    assert!(
        (x - 1950.).abs() < 1e-6 && (y - 130.).abs() < 1e-6,
        "{x},{y}"
    );
}

#[test]
fn keys_and_buttons_from_fake_input_do_not_panic() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();
    let fake = f.client(id).state.fake_input.clone().unwrap();
    fake.authenticate("test".into(), "remote input".into());
    fake.button(0x110, 1);
    fake.button(0x110, 0);
    fake.axis(0, 15.);
    fake.keyboard_key(30, 1);
    fake.keyboard_key(30, 0);
    // 'A' takes Shift; a keysym no key types is dropped.
    fake.keyboard_keysym(0x41, 1);
    fake.keyboard_keysym(0x41, 0);
    fake.keyboard_keysym(0x6c1, 1);
    fake.touch_down(1, 100., 100.);
    fake.touch_motion(1, 120., 110.);
    fake.touch_up(1);
    fake.touch_frame();
    fake.destroy();
    f.roundtrip(id);
}
