//! newm's live settings over IPC: the list, a set that applies live, a reset, the export and
//! the event every change sends, driven through the requests a settings app sends.

use niri_config::live::LiveOptions;
use niri_config::Config;
use niri_ipc::{Event, FloatingFollow, NewmOption, Request, Response};
use serde_json::Value;

use crate::ipc::server::IpcServer;
use crate::tests::fixture::Fixture;

const FILE: &str =
    "layout {\n    gaps 8\n}\ninput {\n    keyboard {\n        repeat-rate 25\n    }\n}\n";

fn option<'a>(options: &'a [NewmOption], key: &str) -> &'a NewmOption {
    options.iter().find(|o| o.key == key).unwrap()
}

fn options(f: &mut Fixture) -> Vec<NewmOption> {
    match f.niri_state().newm_options_request(Request::NewmOptions) {
        Ok(Response::NewmOptions(options)) => options,
        other => panic!("NewmOptions answered {other:?}"),
    }
}

fn set(f: &mut Fixture, key: &str, value: Value) -> Result<Response, String> {
    f.niri_state().newm_options_request(Request::NewmSetOption {
        key: key.to_owned(),
        value,
    })
}

#[test]
fn live_settings_list_set_reset_export_and_send_events() {
    let mut f = Fixture::with_config(Config::parse_mem(FILE).unwrap());
    f.add_output(1, (1920, 1080));
    let event_loop = f.niri().event_loop.clone();
    f.niri().ipc_server = Some(IpcServer::start(&event_loop, None).unwrap());
    f.niri().live_options = LiveOptions::new(Config::parse_mem(FILE).unwrap(), &[]);
    let events = f.niri().ipc_server.as_ref().unwrap().subscribe();

    let listed = options(&mut f);
    let gaps = option(&listed, "layout.gaps");
    assert_eq!(
        (gaps.kind.as_str(), gaps.category.as_str()),
        ("float", "Appearance")
    );
    let width = option(&listed, "layout.canvas.default-width");
    assert_eq!(width.kind, "size");
    assert_eq!(gaps.value, Value::from(8.));
    assert_eq!(gaps.config_value, Value::from(8.));
    assert!(!gaps.overridden);
    assert_eq!(gaps.max, Some(200.));
    let follow = option(&listed, "layout.canvas.floating-follow");
    assert_eq!(
        follow.choices.as_deref().unwrap(),
        ["screen", "parent", "canvas"]
    );
    assert!(listed
        .iter()
        .any(|o| o.key == "layout.canvas.magnified-input"));

    assert!(matches!(
        set(&mut f, "layout.gaps", Value::from(20)),
        Ok(Response::Handled)
    ));
    assert!(matches!(
        set(&mut f, "input.keyboard.repeat-rate", Value::from(40)),
        Ok(Response::Handled)
    ));
    assert_eq!(f.niri().config.borrow().layout.gaps, 20.);
    assert_eq!(f.niri().config.borrow().input.keyboard.repeat_rate, 40);
    let follow = |f: &mut Fixture| f.niri().layout.floating_follow();
    assert_eq!(follow(&mut f), FloatingFollow::Screen);
    assert!(matches!(
        set(
            &mut f,
            "layout.canvas.floating-follow",
            Value::from("canvas")
        ),
        Ok(Response::Handled)
    ));
    assert_eq!(
        follow(&mut f),
        FloatingFollow::Canvas,
        "the set reached the layout"
    );
    assert_eq!(
        set(&mut f, "layout.gaps", Value::from(500)).unwrap_err(),
        "value 500 is above the maximum 200"
    );
    assert!(set(&mut f, "layout.tile-width", Value::from(500)).is_err());

    let Ok(Event::NewmOptionsChanged(sent)) = events.try_recv() else {
        panic!("a set sends NewmOptionsChanged");
    };
    assert!(option(&sent, "layout.gaps").overridden);
    let Ok(Event::NewmOptionsChanged(sent)) = events.try_recv() else {
        panic!("the second set sends NewmOptionsChanged");
    };
    assert_eq!(
        option(&sent, "input.keyboard.repeat-rate").value,
        Value::from(40)
    );
    assert!(matches!(
        events.try_recv(),
        Ok(Event::NewmOptionsChanged(_))
    ));
    assert!(events.try_recv().is_err(), "a refused set sends nothing");

    let kdl = match f.niri_state().newm_options_request(Request::NewmExport) {
        Ok(Response::NewmExport { kdl }) => kdl,
        other => panic!("NewmExport answered {other:?}"),
    };
    assert!(
        kdl.starts_with("// Live settings exported from newm-rs "),
        "{kdl}"
    );
    let exported = Config::parse_mem(&kdl).unwrap_or_else(|e| panic!("{kdl}\n{e:?}"));
    assert_eq!(exported.layout.gaps, 20.);
    assert_eq!(exported.input.keyboard.repeat_rate, 40);

    // A config file reload keeps the overrides on top of the new file.
    let state = f.niri_state();
    let file = Config::parse_mem("layout {\n    gaps 4\n}\n").unwrap();
    let config = state.niri.live_options.loaded(file);
    state.reload_config(Ok(config));
    assert_eq!(state.niri.config.borrow().layout.gaps, 20.);
    let gaps = option(&options(&mut f), "layout.gaps").clone();
    assert_eq!(gaps.config_value, Value::from(4.));

    let reset = f
        .niri_state()
        .newm_options_request(Request::NewmResetOption {
            key: "layout.gaps".to_owned(),
        });
    assert!(matches!(reset, Ok(Response::Handled)));
    assert_eq!(f.niri().config.borrow().layout.gaps, 4.);
    let Ok(Event::NewmOptionsChanged(sent)) = events.try_recv() else {
        panic!("a reset sends NewmOptionsChanged");
    };
    assert!(!option(&sent, "layout.gaps").overridden);

    let reset = f.niri_state().newm_options_request(Request::NewmResetAll);
    assert!(matches!(reset, Ok(Response::Handled)));
    assert_eq!(f.niri().config.borrow().input.keyboard.repeat_rate, 25);
    assert!(options(&mut f).iter().all(|o| !o.overridden));
    assert!(matches!(
        events.try_recv(),
        Ok(Event::NewmOptionsChanged(_))
    ));
}

#[test]
fn a_displays_scale_is_listed_set_and_reset_live() {
    let mut f = Fixture::with_config(Config::default());
    f.add_output(1, (1920, 1080));
    f.niri().live_options = LiveOptions::new(Config::default(), &[]);
    let name = f.niri_output(1).name();
    let key = format!("output.{name}.scale");
    let scale = |f: &Fixture| f.niri_output(1).current_scale().fractional_scale();
    let own = scale(&f);

    let listed = options(&mut f);
    let listed = option(&listed, &key);
    assert_eq!(listed.category, "Displays");
    assert_eq!(listed.value, Value::from(own));
    assert!(!listed.overridden);

    set(&mut f, &key, Value::from(1.5)).unwrap();
    assert_eq!(
        scale(&f),
        1.5,
        "the display is drawn at the new scale at once"
    );
    let listed = options(&mut f);
    let listed = option(&listed, &key);
    assert_eq!(listed.value, Value::from(1.5));
    assert_eq!(
        listed.config_value,
        Value::from(own),
        "what it had by itself"
    );
    assert!(listed.overridden);

    assert!(set(&mut f, &key, Value::from(9)).is_err());
    assert!(set(&mut f, "output.nowhere.scale", Value::from(1)).is_err());
    assert_eq!(scale(&f), 1.5);

    f.niri_state()
        .newm_options_request(Request::NewmResetOption { key: key.clone() })
        .unwrap();
    assert_eq!(
        scale(&f),
        own,
        "a reset gives the display its own scale back"
    );
    let listed = options(&mut f);
    assert!(!option(&listed, &key).overridden);
}
