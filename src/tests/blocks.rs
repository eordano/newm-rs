//! Blocks over IPC: a client places windows as one block, polls which windows took its slots,
//! and parks the windows already open, with the JSON a client sends.

use std::collections::HashMap;

use newm_canvas::Rect;
use niri_ipc::{NewmBlockAnchor, Reply, Request, Response};
use serde_json::json;

use crate::tests::client::ClientId;
use crate::tests::fixture::Fixture;

const PLACE: &str = r#"{"NewmPlaceBlock":{"windows":[
    {"command":["true"],"cwd":null,"env":{"SOME_VAR":"1"},"app_id":"a","rect":[0,0,1,1]},
    {"command":["true"],"cwd":"/","env":{},"app_id":"b","rect":[1,0,1,1]}
  ],"anchor":{"Tiles":[3,0]},"focus":1,"fly":true}}"#;

fn ask(f: &mut Fixture, request: Request) -> Reply {
    f.niri_state().newm_block_request(request)
}

fn map(f: &mut Fixture, id: ClientId, app_id: &str) {
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.set_app_id(app_id);
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
}

/// Each grid window's tiles by its IPC id.
fn grid(f: &mut Fixture) -> HashMap<u64, Rect> {
    let ws = f.niri().layout.active_workspace().unwrap();
    ws.scrolling()
        .grid_windows()
        .map(|(w, r)| (w.id().get(), r))
        .collect()
}

#[test]
fn block_requests_parse_and_answer_as_a_client_expects() {
    let place: Request = serde_json::from_str(PLACE).unwrap();
    let Request::NewmPlaceBlock(block) = place else {
        panic!("not a block: {place:?}");
    };
    assert_eq!(block.anchor, NewmBlockAnchor::Tiles(3, 0));
    assert_eq!((block.focus, block.fly), (Some(1), true));
    assert_eq!(block.windows[0].env["SOME_VAR"], "1");
    assert_eq!(block.windows[1].rect, (1, 0, 1, 1));
    let nearest: Request = serde_json::from_value(json!({"NewmPlaceBlock": {
        "windows": [{"command": ["x"], "app_id": "x", "rect": [0, 0, 2, 1]}],
        "anchor": "Nearest", "focus": null, "fly": false}}))
    .unwrap();
    assert!(matches!(nearest, Request::NewmPlaceBlock(b) if b.anchor == NewmBlockAnchor::Nearest));
    assert!(matches!(
        serde_json::from_str(r#""NewmParkWindows""#).unwrap(),
        Request::NewmParkWindows
    ));
    assert!(matches!(
        serde_json::from_str(r#"{"NewmBlockStatus":{"id":7}}"#).unwrap(),
        Request::NewmBlockStatus { id: 7 }
    ));

    let placed: Reply = Ok(Response::NewmBlock {
        id: 1,
        origin: (3, 0),
    });
    assert_eq!(
        serde_json::to_value(placed).unwrap(),
        json!({"Ok": {"NewmBlock": {"id": 1, "origin": [3, 0]}}})
    );
    let status: Reply = Ok(Response::NewmBlockStatus {
        id: 1,
        windows: vec![Some(5), None],
        done: false,
    });
    assert_eq!(
        serde_json::to_value(status).unwrap(),
        json!({"Ok": {"NewmBlockStatus": {"id": 1, "windows": [5, null], "done": false}}})
    );
}

#[test]
fn a_block_of_two_windows_lands_where_asked_and_reports_them() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    let Ok(Response::NewmBlock { id, origin }) = ask(&mut f, serde_json::from_str(PLACE).unwrap())
    else {
        panic!("the block was not placed");
    };
    assert_eq!(origin, (3, 0));
    let status = ask(&mut f, Request::NewmBlockStatus { id });
    assert!(matches!(
        status,
        Ok(Response::NewmBlockStatus { ref windows, done: false, .. }) if windows == &[None, None]
    ));

    map(&mut f, client, "b");
    map(&mut f, client, "a");
    let mut ids: Vec<u64> = grid(&mut f).keys().copied().collect();
    ids.sort_unstable();
    let (b, a) = (ids[0], ids[1]);
    let Ok(Response::NewmBlockStatus { windows, done, .. }) =
        ask(&mut f, Request::NewmBlockStatus { id })
    else {
        panic!("no status");
    };
    assert_eq!(windows, [Some(a), Some(b)]);
    assert!(done);
    let tiles = grid(&mut f);
    assert_eq!(tiles[&a], Rect::new(3, 0, 1, 1));
    assert_eq!(tiles[&b], Rect::new(4, 0, 1, 1));
    let focused = f.niri().layout.focus().map(|w| w.id().get());
    assert_eq!(focused, Some(b), "the window of slot 1 takes the focus");

    assert_eq!(
        ask(&mut f, serde_json::from_str(PLACE).unwrap()).err(),
        Some(String::from("the block's tiles are taken"))
    );
    assert_eq!(
        ask(&mut f, Request::NewmBlockStatus { id: id + 1000 }).err(),
        Some(String::from("no such block"))
    );

    let ws = f.niri().layout.active_workspace().unwrap();
    let c = ws.scrolling().canvas();
    let left = (c.view().0 / c.tile_size().0).floor() as i32;
    assert!(matches!(
        ask(&mut f, Request::NewmParkWindows),
        Ok(Response::Handled)
    ));
    let parked = grid(&mut f);
    assert_eq!(parked[&b].i - parked[&a].i, 1, "the arrangement stays");
    assert_eq!(parked[&b].right(), left - 1, "just left of the view");
}

#[test]
fn a_block_without_a_command_is_refused() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let bad = json!({"NewmPlaceBlock": {
        "windows": [{"command": [], "app_id": "x", "rect": [0, 0, 1, 1]}],
        "anchor": "Camera", "focus": null, "fly": true}});
    assert_eq!(
        ask(&mut f, serde_json::from_value(bad).unwrap()).err(),
        Some(String::from("bad block"))
    );
}
