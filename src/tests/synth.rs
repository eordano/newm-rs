//! The synth background in a running compositor: what exists with the mode off, the element on
//! the output, and the config reload.

use std::time::Duration;

use niri_config::{Config, SynthBackground};
use niri_ipc::SynthMode;
use smithay::backend::input::InputTime;

use super::Fixture;
use crate::niri::{OutputRenderElements, RedrawState};
use crate::render_helpers::shaders::Shaders;
use crate::render_helpers::{RenderCtx, RenderTarget};

fn config(mode: SynthMode) -> Config {
    Config {
        synth_background: SynthBackground {
            mode,
            ..SynthBackground::default()
        },
        ..Config::default()
    }
}

fn set_up(mode: SynthMode) -> Fixture {
    let mut f = Fixture::with_config(config(mode));
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, (640, 400));
    f
}

/// How many synth elements the output's frame holds.
fn synth_elements(f: &mut Fixture) -> usize {
    let output = f.niri_output(1);
    let state = f.niri_state();
    let niri = &state.niri;
    state
        .backend
        .with_primary_renderer(|renderer| {
            let ctx = RenderCtx {
                renderer,
                target: RenderTarget::Output,
                xray: None,
            };
            niri.render_to_vec(ctx, &output, false)
                .iter()
                .filter(|e| matches!(e, OutputRenderElements::Shader(_)))
                .count()
        })
        .unwrap()
}

#[test]
fn egl_off_draws_and_holds_nothing() {
    let mut f = set_up(SynthMode::Off);
    assert!(!f.niri().synth.is_allocated());
    assert_eq!(synth_elements(&mut f), 0);
    let compiled = f
        .niri_state()
        .backend
        .with_primary_renderer(|r| Shaders::get(r).synth.borrow().is_some())
        .unwrap();
    assert!(
        !compiled,
        "the program is compiled only once the background is drawn"
    );
}

#[test]
fn egl_still_draws_one_element_in_front_of_the_backdrop() {
    let mut f = set_up(SynthMode::Still);
    assert_eq!(synth_elements(&mut f), 1);
}

#[test]
fn egl_reloading_the_config_switches_it() {
    let mut f = set_up(SynthMode::Off);
    f.niri_state().reload_config(Ok(config(SynthMode::Still)));
    assert!(f.niri().synth.is_allocated());
    assert_eq!(synth_elements(&mut f), 1);

    f.niri_state().reload_config(Ok(config(SynthMode::Off)));
    assert!(!f.niri().synth.is_allocated());
    assert_eq!(synth_elements(&mut f), 0);
}

fn animated(config: impl FnOnce(&mut SynthBackground)) -> Fixture {
    let mut synth = SynthBackground {
        mode: SynthMode::Animated,
        ..SynthBackground::default()
    };
    config(&mut synth);
    let mut f = Fixture::with_config(Config {
        synth_background: synth,
        // As newm sessions set it: the workspace background lets the synth background show.
        layout: niri_config::Layout {
            background_color: niri_config::Color::new_unpremul(0., 0., 0., 0.),
            ..niri_config::Layout::default()
        },
        ..Config::default()
    });
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, (640, 400));
    f.niri_state().refresh_and_flush_clients();
    f
}

/// One display frame, as a vblank drives it: the output redraws, then says whether the
/// background's picture keeps frames coming.
fn frame(f: &mut Fixture) -> bool {
    let output = f.niri_output(1);
    f.niri().queue_redraw(&output);
    f.niri_state().refresh_and_flush_clients();
    f.niri().synth.is_moving()
}

fn animations_remain(f: &mut Fixture) -> bool {
    let output = f.niri_output(1);
    f.niri().output_state[&output].unfinished_animations_remain
}

/// What a redraw at presentation time `t` does to the picture: advance, then decide.
fn present(f: &mut Fixture, t: Duration) -> bool {
    let output = f.niri_output(1);
    f.niri().synth_advance(&output, t);
    f.niri().synth_after_render(&output, t)
}

fn scene(f: &mut Fixture) -> Duration {
    let name = f.niri_output(1).name();
    f.niri().synth.scene_time(&name).unwrap()
}

fn commit(f: &mut Fixture) -> Option<smithay::backend::renderer::utils::CommitCounter> {
    let name = f.niri_output(1).name();
    f.niri().synth.commit(&name)
}

const HZ60: Duration = Duration::from_nanos(16_666_667);

#[test]
fn egl_off_and_still_ask_for_no_frames() {
    for mode in [SynthMode::Off, SynthMode::Still] {
        let mut f = set_up(mode);
        assert!(!frame(&mut f), "{mode:?}");
        assert!(!animations_remain(&mut f), "{mode:?}");
        assert!(!f.niri().synth.is_waiting(), "{mode:?}");
    }
}

#[test]
fn egl_animated_asks_for_every_frame_while_the_monitors_are_on() {
    let mut f = animated(|_| ());
    assert!(frame(&mut f));
    assert!(animations_remain(&mut f), "the next vblank redraws again");
    assert!(frame(&mut f));

    f.niri().monitors_active = false;
    assert!(!frame(&mut f), "blanked monitors stop it");

    f.niri().monitors_active = true;
    assert!(frame(&mut f), "and waking them starts it again");
    assert!(animations_remain(&mut f));
}

#[test]
fn egl_the_screenshot_ui_stops_it() {
    let mut f = animated(|_| ());
    assert!(frame(&mut f));

    f.niri_state().open_screenshot_ui(false, None);
    assert!(f.niri().screenshot_ui.is_open());
    assert!(!frame(&mut f));

    f.niri().screenshot_ui.close();
    assert!(frame(&mut f));
}

#[test]
fn egl_turning_it_off_stops_and_frees_everything() {
    let mut f = animated(|_| ());
    assert!(frame(&mut f));
    f.niri_state().reload_config(Ok(config(SynthMode::Off)));
    assert!(!frame(&mut f));
    assert!(!f.niri().synth.is_allocated());
}

#[test]
fn egl_a_fullscreen_window_stops_it() {
    let mut f = animated(|_| ());
    assert!(frame(&mut f));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.set_fullscreen(None);
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(640, 400);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    f.niri_complete_animations();

    let output = f.niri_output(1);
    assert!(f.niri().synth_visibility(&output).covered);
    assert!(!frame(&mut f), "an opaque fullscreen window covers it");

    let niri = f.niri();
    let window = niri.layout.windows().next().unwrap().1.window.clone();
    niri.layout.set_fullscreen(&window, false);
    f.double_roundtrip(id);
    let window = f.client(id).window(&surface);
    window.set_size(320, 200);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    f.niri_complete_animations();
    assert!(frame(&mut f), "leaving fullscreen starts it again");
}

/// The tuned look with every time-dependent term switched off.
fn frozen(s: &mut SynthBackground) {
    s.stars = Some(0.);
    s.grid_speed = Some(0.);
    s.ridge_speed = Some(0.);
    s.reflection = Some(0.);
    s.cycle_speed = Some(0.);
}

#[test]
fn egl_a_look_that_cannot_move_asks_for_no_frames() {
    let mut f = animated(frozen);
    assert_eq!(f.niri().synth.mode(), SynthMode::Animated);
    assert!(!frame(&mut f), "nothing would change, so no frames");
    let before = commit(&mut f);
    for _ in 0..10 {
        frame(&mut f);
    }
    assert_eq!(commit(&mut f), before);
    assert!(!animations_remain(&mut f));

    // Switching one term back on starts it.
    let mut config = config(SynthMode::Animated);
    frozen(&mut config.synth_background);
    config.synth_background.stars = Some(0.69);
    f.niri_state().reload_config(Ok(config));
    assert!(frame(&mut f));
}

/// The scene time steps by exactly each presentation interval: steady 60 Hz, steady 120 Hz,
/// and the uneven intervals of a VRR output.
#[test]
fn egl_the_scene_clock_is_the_presentation_clock() {
    let mut f = animated(|_| ());
    assert!(frame(&mut f));
    let start = crate::utils::get_monotonic_time() + Duration::from_secs(1);
    let mut t = start;
    assert!(present(&mut f, t));
    let mut last = scene(&mut f);
    let mut commits = 0;
    let steps = [HZ60; 60]
        .into_iter()
        .chain([Duration::from_nanos(8_333_333); 120])
        .chain((0..60).map(|i| Duration::from_micros(7_000 + 997 * (i % 9))));
    for dt in steps {
        let before = commit(&mut f);
        t += dt;
        assert!(present(&mut f, t), "frames keep coming");
        let now = scene(&mut f);
        assert_eq!(
            now - last,
            dt,
            "one presentation interval, no doubled or skipped step"
        );
        assert_ne!(commit(&mut f), before, "every frame draws a new picture");
        commits += 1;
        last = now;
    }
    assert_eq!(commits, 240);
}

/// Stopping and starting again continues from the picture that showed, whatever stopped it.
#[test]
fn egl_a_pause_resumes_without_a_jump() {
    let mut f = animated(|_| ());
    assert!(frame(&mut f));
    let mut t = crate::utils::get_monotonic_time() + Duration::from_secs(1);
    for _ in 0..30 {
        t += HZ60;
        present(&mut f, t);
    }
    let shown = scene(&mut f);

    type Gate = fn(&mut Fixture, bool);
    let gates: [(&str, Gate); 3] = [
        ("monitors", |f, closed| f.niri().monitors_active = !closed),
        ("battery", |f, closed| f.niri().synth.set_on_battery(closed)),
        ("screenshot ui", |f, closed| {
            if closed {
                f.niri_state().open_screenshot_ui(false, None);
            } else {
                f.niri().screenshot_ui.close();
            }
        }),
    ];
    for (name, gate) in gates {
        gate(&mut f, true);
        t += HZ60;
        assert!(!present(&mut f, t), "{name} stops it");
        let shown_before = scene(&mut f);
        // Time passes while it stands.
        t += Duration::from_secs(7);
        present(&mut f, t);
        assert_eq!(scene(&mut f), shown_before, "{name}: it stands");
        gate(&mut f, false);
        f.niri().synth_refresh();
        assert!(f.niri().synth.is_moving(), "{name} lets it go");
        t += Duration::from_secs(3);
        present(&mut f, t);
        assert_eq!(
            scene(&mut f),
            shown_before,
            "{name}: the first frame after the pause shows the picture it stopped on"
        );
        t += HZ60;
        present(&mut f, t);
        assert_eq!(
            scene(&mut f),
            shown_before + HZ60,
            "{name}: then one frame on"
        );
    }
    assert!(scene(&mut f) > shown);
}

#[test]
fn egl_max_fps_caps_the_moves_and_the_timer_takes_the_long_waits() {
    let mut f = animated(|s| s.max_fps = Some(30.));
    assert!(frame(&mut f));
    let mut t = crate::utils::get_monotonic_time() + Duration::from_secs(1);
    let mut moves = 0;
    for _ in 0..120 {
        let before = commit(&mut f);
        t += HZ60;
        assert!(
            present(&mut f, t),
            "30 fps on 60 Hz: the next move is a frame or two away"
        );
        moves += usize::from(commit(&mut f) != before);
    }
    assert!(
        (59..=61).contains(&moves),
        "{moves} moves in 2 s at max-fps 30"
    );

    let mut f = animated(|s| s.max_fps = Some(2.));
    assert!(frame(&mut f));
    let t = crate::utils::get_monotonic_time() + Duration::from_secs(1);
    present(&mut f, t);
    assert!(
        !present(&mut f, t + HZ60),
        "half a second off: no frames meanwhile"
    );
    assert!(
        f.niri().synth.is_waiting(),
        "the timer queues the next move"
    );
}

#[test]
fn egl_on_battery_follows_on_battery() {
    use niri_config::SynthOnBattery;

    let mut f = animated(|_| ());
    assert!(frame(&mut f));
    f.niri().synth.set_on_battery(true);
    assert!(!frame(&mut f), "the default stands still on battery");
    f.niri().synth.set_on_battery(false);
    assert!(frame(&mut f), "and moves again on mains");

    let mut f = animated(|s| s.on_battery = SynthOnBattery::Animated);
    f.niri().synth.set_on_battery(true);
    assert!(frame(&mut f), "\"animated\" ignores the battery");

    let mut f = animated(|s| {
        s.on_battery = SynthOnBattery::Capped;
        s.on_battery_max_fps = 20.;
    });
    f.niri().synth.set_on_battery(true);
    assert!(frame(&mut f));
    let mut t = crate::utils::get_monotonic_time() + Duration::from_secs(1);
    let mut moves = 0;
    for _ in 0..60 {
        let before = commit(&mut f);
        t += HZ60;
        present(&mut f, t);
        moves += usize::from(commit(&mut f) != before);
    }
    assert!(
        (19..=21).contains(&moves),
        "{moves} moves in 1 s capped at 20"
    );
}

#[test]
fn egl_idle_stands_it_still_until_input_unless_inhibited() {
    let mut f = animated(|s| s.idle_still_after = 60.);
    assert!(frame(&mut f));
    let now = crate::utils::get_monotonic_time();
    f.niri().synth.input(now - Duration::from_secs(61));
    assert!(!frame(&mut f), "a minute without input stops it");
    let output = f.niri_output(1);
    f.niri().synth.idle_inhibited = true;
    assert!(
        f.niri().synth_moves(&output, now),
        "an idle inhibitor (a playing video) keeps it moving"
    );
    f.niri().synth.idle_inhibited = false;
    assert!(!f.niri().synth_moves(&output, now));
    f.niri().notify_activity();
    assert!(frame(&mut f), "input starts it again");

    let mut f = animated(|s| s.idle_still_after = 0.);
    let now = crate::utils::get_monotonic_time();
    f.niri()
        .synth
        .input(now.saturating_sub(Duration::from_secs(86_400)));
    assert!(frame(&mut f), "0 never stops it");
}

#[test]
fn visible_share_subtracts_the_opaque_rectangles() {
    use smithay::utils::{Physical, Rectangle};

    use crate::synth::visible_share;

    let r = |x, y, w, h| Rectangle::<i32, Physical>::new((x, y).into(), (w, h).into());
    let full = r(0, 0, 100, 100);
    assert_eq!(visible_share(full, []), 1.);
    assert_eq!(visible_share(full, [r(0, 0, 100, 50)]), 0.5);
    assert_eq!(
        visible_share(full, [r(0, 0, 100, 50), r(0, 25, 100, 50)]),
        0.25,
        "overlaps count once"
    );
    assert_eq!(visible_share(full, [r(-10, -10, 200, 200)]), 0.);
    assert_eq!(visible_share(full, [r(200, 200, 10, 10)]), 1.);
    // Two columns of windows with 2 px gaps around them leave a few percent.
    let share = visible_share(full, [r(2, 2, 47, 96), r(51, 2, 47, 96)]);
    assert!((share - (1. - 2. * 47. * 96. / 10_000.)).abs() < 1e-9);
    assert_eq!(visible_share(r(0, 0, 0, 10), []), 0.);
}

/// A frame whose elements in front of the background cover all but a sliver stops it; one that
/// leaves more starts it again.
#[test]
fn egl_a_mostly_covered_background_stops() {
    use smithay::backend::renderer::element::{Id, Kind};
    use smithay::backend::renderer::utils::CommitCounter;
    use smithay::utils::Rectangle;

    use crate::render_helpers::solid_color::SolidColorRenderElement;

    let mut f = animated(|_| ());
    assert!(frame(&mut f));
    let output = f.niri_output(1);
    let measure = |f: &mut Fixture, h: f64| {
        let state = f.niri_state();
        let niri = &state.niri;
        state
            .backend
            .with_primary_renderer(|renderer| {
                let ctx = RenderCtx {
                    renderer,
                    target: RenderTarget::Output,
                    xray: None,
                };
                let mut elements = niri.render_to_vec(ctx, &output, false);
                let cover = SolidColorRenderElement::new(
                    Id::new(),
                    Rectangle::new((0., 0.).into(), (640., h).into()),
                    CommitCounter::default(),
                    [0., 0., 0., 1.].into(),
                    Kind::Unspecified,
                );
                elements.insert(0, OutputRenderElements::SolidColor(cover));
                niri.synth_measure(&output, &elements);
            })
            .unwrap();
        f.niri().synth_visible_share(&output)
    };
    assert_eq!(measure(&mut f, 0.), 1., "nothing in front of it");
    let share = measure(&mut f, 390.);
    assert!((share - 10. / 400.).abs() < 1e-9, "{share}");
    let t = crate::utils::get_monotonic_time();
    assert!(
        !f.niri().synth_after_render(&output, t),
        "2.5% showing stops it"
    );
    assert!(!f.niri().synth.is_moving());

    let share = measure(&mut f, 300.);
    assert!((share - 0.25).abs() < 1e-9, "{share}");
    assert!(frame(&mut f), "a quarter showing moves");
}

/// Blurred windows see a copy that follows the picture once a second, not every frame.
#[test]
fn egl_the_blurred_copy_follows_once_a_second() {
    let mut f = animated(|_| ());
    assert!(frame(&mut f));
    let mut t = crate::utils::get_monotonic_time() + Duration::from_secs(1);
    let (_, first) = f.niri().synth.xray_picture().unwrap();
    let mut changes = 0;
    let mut last = first;
    for _ in 0..(5 * 60) {
        t += HZ60;
        present(&mut f, t);
        let (time, commit) = f.niri().synth.xray_picture().unwrap();
        assert!(scene(&mut f) - time < crate::synth::XRAY_STEP);
        changes += usize::from(commit != last);
        last = commit;
    }
    assert!((4..=5).contains(&changes), "{changes} copies in 5 s");
}

/// What a frame's pacing costs on the CPU, and the per-dispatch check. Run it with
/// `cargo test --release --features canvas --lib frame_pacing_cost -- --ignored --nocapture`.
#[test]
#[ignore = "a timing, not a check"]
fn frame_pacing_cost() {
    let mut f = animated(|_| ());
    assert!(frame(&mut f));
    let start = crate::utils::get_monotonic_time();
    let n = 200_000u32;
    let t = std::time::Instant::now();
    for i in 1..=n {
        present(&mut f, start + HZ60 * i);
    }
    eprintln!(
        "synth_advance + synth_after_render (the tuned look, one 640x400 output): {:?}",
        t.elapsed() / n
    );
    let t = std::time::Instant::now();
    for _ in 0..n {
        f.niri().synth_refresh();
    }
    eprintln!(
        "synth_refresh (moving, nothing changed): {:?}",
        t.elapsed() / n
    );
    let mut f = set_up(SynthMode::Off);
    let t = std::time::Instant::now();
    for _ in 0..n {
        f.niri().synth_refresh();
    }
    eprintln!("synth_refresh (off): {:?}", t.elapsed() / n);
}

fn act(f: &mut Fixture, action: niri_ipc::Action) {
    // The path `newm-rs msg action` takes: the IPC action becomes a config action.
    let action = niri_config::Action::from(action);
    f.niri_state().do_action(action, false, InputTime::now());
    f.niri_state().refresh_and_flush_clients();
}

fn mode(f: &mut Fixture) -> SynthMode {
    f.niri().synth.mode()
}

#[test]
fn egl_toggling_from_off_animates_and_toggling_again_frees_it() {
    let mut f = set_up(SynthMode::Off);
    act(&mut f, niri_ipc::Action::ToggleSynthBackground {});
    assert_eq!(mode(&mut f), SynthMode::Animated);
    assert!(frame(&mut f));
    assert_eq!(synth_elements(&mut f), 1);

    act(&mut f, niri_ipc::Action::ToggleSynthBackground {});
    assert_eq!(mode(&mut f), SynthMode::Off);
    assert!(!frame(&mut f));
    assert!(!f.niri().synth.is_allocated());
}

#[test]
fn egl_toggling_a_drawn_background_switches_its_motion_only() {
    for (configured, rest) in [
        (SynthMode::Still, SynthMode::Still),
        (SynthMode::Animated, SynthMode::Still),
    ] {
        let mut f = set_up(configured);
        f.niri_state().refresh_and_flush_clients();
        if configured == SynthMode::Still {
            act(&mut f, niri_ipc::Action::ToggleSynthBackground {});
            assert_eq!(mode(&mut f), SynthMode::Animated);
        }
        act(&mut f, niri_ipc::Action::ToggleSynthBackground {});
        assert_eq!(mode(&mut f), rest, "{configured:?}");
        assert!(!frame(&mut f));
        assert_eq!(synth_elements(&mut f), 1, "the still frame stays drawn");
        act(&mut f, niri_ipc::Action::ToggleSynthBackground {});
        assert_eq!(mode(&mut f), SynthMode::Animated);
        assert!(frame(&mut f));
    }
}
#[test]
fn egl_a_set_mode_holds_until_the_config_changes_the_mode() {
    let mut f = set_up(SynthMode::Off);
    act(
        &mut f,
        niri_ipc::Action::SetSynthBackground {
            mode: SynthMode::Still,
        },
    );
    assert_eq!(mode(&mut f), SynthMode::Still);

    let mut other = config(SynthMode::Off);
    other.synth_background.brightness = Some(0.3);
    f.niri_state().reload_config(Ok(other));
    assert_eq!(
        mode(&mut f),
        SynthMode::Still,
        "another knob keeps the set mode"
    );

    f.niri_state()
        .reload_config(Ok(config(SynthMode::Animated)));
    assert_eq!(
        mode(&mut f),
        SynthMode::Animated,
        "a new mode in the config wins"
    );
}

#[test]
fn newm_rs_msg_action_parses_both_actions() {
    use clap::Parser as _;

    use crate::cli::{Cli, Msg, Sub};

    for (args, expected) in [
        (
            &["toggle-synth-background"][..],
            niri_ipc::Action::ToggleSynthBackground {},
        ),
        (
            &["set-synth-background", "animated"][..],
            niri_ipc::Action::SetSynthBackground {
                mode: SynthMode::Animated,
            },
        ),
    ] {
        let cli =
            Cli::try_parse_from(["newm-rs", "msg", "action"].iter().chain(args).copied()).unwrap();
        let Some(Sub::Msg {
            msg: Msg::Action { action },
            ..
        }) = cli.subcommand
        else {
            panic!("{args:?} did not parse as an action");
        };
        assert_eq!(format!("{action:?}"), format!("{expected:?}"));
    }
}

fn listening(f: &mut Fixture) -> bool {
    f.niri_state().refresh_and_flush_clients();
    f.niri().synth.is_listening()
}

#[test]
fn egl_the_tuned_audio_amounts_alone_never_listen() {
    // The tuned look moves the grid and ridges with sound (audio-grid 0.06, audio-ridge 0.02),
    // but without react-to-sound nothing opens a capture.
    let mut f = animated(|_| ());
    assert!(frame(&mut f));
    assert!(!listening(&mut f));
}

#[test]
fn egl_react_to_sound_listens_only_while_animated_and_shown() {
    let mut f = animated(|s| s.react_to_sound = true);
    assert!(listening(&mut f));

    f.niri().monitors_active = false;
    assert!(!listening(&mut f), "a blanked screen stops listening");
    f.niri().monitors_active = true;
    assert!(listening(&mut f));

    act(
        &mut f,
        niri_ipc::Action::SetSynthBackground {
            mode: SynthMode::Still,
        },
    );
    assert!(!listening(&mut f), "still does not listen");
    act(
        &mut f,
        niri_ipc::Action::SetSynthBackground {
            mode: SynthMode::Animated,
        },
    );
    assert!(listening(&mut f));

    let mut deaf = config(SynthMode::Animated);
    deaf.synth_background.react_to_sound = true;
    deaf.synth_background.audio_grid = Some(0.);
    deaf.synth_background.audio_ridge = Some(0.);
    deaf.synth_background.audio_sun = Some(0.);
    f.niri_state().reload_config(Ok(deaf));
    assert!(
        !listening(&mut f),
        "with every audio amount at zero, nothing to hear"
    );

    let mut off = config(SynthMode::Off);
    off.synth_background.react_to_sound = true;
    f.niri_state().reload_config(Ok(off));
    assert!(!listening(&mut f));
    assert!(!f.niri().synth.is_allocated());
}

/// Presents one 60 Hz frame `i` frames after `start`, with `level` heard just before, and says
/// whether the picture changed.
fn present_with(f: &mut Fixture, start: Duration, i: u32, level: Option<f32>) -> bool {
    if let Some(level) = level {
        f.niri().synth.hear(level);
    }
    let before = commit(f);
    present(f, start + HZ60 * i);
    commit(f) != before
}

#[test]
fn egl_a_still_look_moves_only_when_the_level_moves_a_step() {
    let mut f = animated(|s| {
        frozen(s);
        s.react_to_sound = true;
    });
    assert!(listening(&mut f));
    assert!(
        !frame(&mut f),
        "silence and a look that cannot move: no frames"
    );

    // Sound comes in: frames start, the first move draws the level.
    f.niri().synth.hear(0.8);
    assert!(frame(&mut f));
    let start = crate::utils::get_monotonic_time() + Duration::from_secs(1);
    present(&mut f, start);
    assert!(present_with(&mut f, start, 1, None));
    let loud = f.niri().synth.drawn_step().unwrap();
    assert!(loud > 0);

    // The same loudness again and again: the level settles and nothing more is drawn.
    let redraws = (2..40)
        .filter(|&i| present_with(&mut f, start, i, Some(0.8)))
        .count();
    assert!(redraws <= 3, "{redraws} redraws for a steady sound");

    // Silence: the level falls back step by step, then the frames stop.
    let mut i = 40;
    while f.niri().synth.is_moving() && i < 400 {
        present_with(&mut f, start, i, None);
        i += 1;
    }
    assert_eq!(f.niri().synth.drawn_step(), Some(0));
    assert!(!frame(&mut f), "silent and settled: no frames");
}

#[test]
fn egl_sound_never_adds_frames() {
    let mut f = animated(|s| s.react_to_sound = true);
    assert!(listening(&mut f));
    assert!(frame(&mut f));
    let start = crate::utils::get_monotonic_time() + Duration::from_secs(1);
    let output = f.niri_output(1);
    for i in 1..=8 {
        // Twenty blocks between two frames queue nothing by themselves...
        for b in 0..20 {
            f.niri().synth.hear(if b % 2 == 0 { 0.9 } else { 0.1 });
            f.niri().synth_refresh();
            assert!(
                matches!(
                    f.niri().output_state[&output].redraw_state,
                    RedrawState::Idle
                ),
                "a sound block alone redraws nothing"
            );
        }
        // ...and the frame draws once.
        assert!(present_with(&mut f, start, i, None));
    }
}
