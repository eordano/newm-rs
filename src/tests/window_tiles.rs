//! A client that shows windows on given tiles: the window list carries each tiled window's
//! tiles and a request places a window by its id, with the JSON a client sends.

use std::collections::HashMap;

use newm_canvas::Rect;
use niri_ipc::{Reply, Request, Response, WindowLayout};
use serde_json::json;

use crate::tests::client::ClientId;
use crate::tests::fixture::Fixture;

fn ask(f: &mut Fixture, request: serde_json::Value) -> Reply {
    let request: Request = serde_json::from_value(request).unwrap();
    f.niri_state().newm_window_request(request)
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

/// The layout the window list gives each window, by its IPC id.
fn layouts(f: &mut Fixture) -> HashMap<u64, (bool, WindowLayout)> {
    let mut rv = HashMap::new();
    f.niri().layout.with_windows(|mapped, _, _, layout| {
        rv.insert(mapped.id().get(), (mapped.is_floating(), layout));
    });
    rv
}

/// Each grid window's tiles by its IPC id.
fn grid(f: &mut Fixture) -> HashMap<u64, Rect> {
    let ws = f.niri().layout.active_workspace().unwrap();
    ws.scrolling()
        .grid_windows()
        .map(|(w, r)| (w.id().get(), r))
        .collect()
}

/// The client and the ids of its windows, mapped as `a` then `b`.
fn two_windows(f: &mut Fixture) -> (ClientId, u64, u64) {
    let client = f.add_client();
    map(f, client, "a");
    let a = *grid(f).keys().next().unwrap();
    map(f, client, "b");
    let b = *grid(f).keys().find(|id| **id != a).unwrap();
    (client, a, b)
}

/// Makes window `id` float, or tiles it again.
fn toggle_floating(f: &mut Fixture, client: ClientId, id: u64) {
    let window = f.niri().layout.windows().find(|(_, m)| m.id().get() == id);
    let window = window.unwrap().1.window.clone();
    f.niri().layout.toggle_window_floating(Some(&window));
    f.double_roundtrip(client);
}

#[test]
fn the_window_list_carries_the_tiles_of_tiled_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let (client, a, b) = two_windows(&mut f);

    let tiles = grid(&mut f);
    let listed = layouts(&mut f);
    for id in [a, b] {
        let r = tiles[&id];
        let (floating, layout) = &listed[&id];
        assert!(!floating);
        assert_eq!(layout.tiles, Some((r.i, r.j, r.w, r.h)));
        let sent = serde_json::to_value(layout).unwrap();
        assert_eq!(sent["tiles"], json!([r.i, r.j, r.w, r.h]));
    }

    toggle_floating(&mut f, client, a);
    let (floating, layout) = &layouts(&mut f)[&a];
    assert!(floating, "a floating window says so");
    assert_eq!(layout.tiles, None, "and holds no tiles");
    assert_eq!(serde_json::to_value(layout).unwrap()["tiles"], json!(null));

    let old = json!({
        "pos_in_scrolling_layout": [1, 1],
        "tile_size": [100.0, 100.0],
        "window_size": [100, 100],
        "tile_pos_in_workspace_view": null,
        "window_offset_in_tile": [0.0, 0.0]
    });
    let old: WindowLayout = serde_json::from_value(old).unwrap();
    assert_eq!(old.tiles, None, "a layout sent without tiles still parses");
}

#[test]
fn a_tile_change_reaches_the_event_stream_as_a_layout_change() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let (_, a, _) = two_windows(&mut f);
    let before = layouts(&mut f)[&a].1.clone();
    let reply = ask(
        &mut f,
        json!({"NewmPlaceWindow": {"id": a, "tile": [7, 5]}}),
    );
    assert!(reply.is_ok());
    let after = layouts(&mut f)[&a].1.clone();
    assert_ne!(
        before, after,
        "the layouts differ, so WindowLayoutsChanged carries the new one"
    );
    assert_eq!(after.tiles.map(|t| (t.0, t.1)), Some((7, 5)));
}

#[test]
fn place_window_moves_a_window_by_id_and_answers_its_tiles() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let (_, a, b) = two_windows(&mut f);
    let before = grid(&mut f);

    let reply = ask(
        &mut f,
        json!({"NewmPlaceWindow": {"id": a, "tile": [9, 4], "span": [2, 1]}}),
    );
    assert_eq!(
        serde_json::to_value(&reply).unwrap(),
        json!({"Ok": {"NewmWindowTiles": {"id": a, "tiles": [9, 4, 2, 1]}}})
    );
    assert_eq!(grid(&mut f)[&a], Rect::new(9, 4, 2, 1));
    assert_eq!(
        grid(&mut f)[&b],
        before[&b],
        "a move to free tiles moves no other window"
    );

    let rb = before[&b];
    let reply = ask(
        &mut f,
        json!({"NewmPlaceWindow": {"id": a, "tile": [rb.i, rb.j]}}),
    );
    let Ok(Response::NewmWindowTiles { id, tiles }) = reply else {
        panic!("not placed: {reply:?}");
    };
    assert_eq!(
        (id, tiles),
        (a, (rb.i, rb.j, 2, 1)),
        "it takes tiles another window holds"
    );
    let after = grid(&mut f);
    assert_ne!(after[&b], rb, "and that window makes room");
    let (ra, rb) = (after[&a], after[&b]);
    let apart =
        ra.right() <= rb.i || rb.right() <= ra.i || ra.bottom() <= rb.j || rb.bottom() <= ra.j;
    assert!(apart, "no tile holds two windows: {ra:?} {rb:?}");
}

#[test]
fn place_window_refuses_what_it_cannot_place() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let (client, a, b) = two_windows(&mut f);

    let err = |reply: Reply| reply.err();
    assert_eq!(
        err(ask(
            &mut f,
            json!({"NewmPlaceWindow": {"id": a + b + 1, "tile": [0, 0]}})
        )),
        Some(String::from("no such window"))
    );
    assert_eq!(
        err(ask(
            &mut f,
            json!({"NewmPlaceWindow": {"id": a, "tile": [0, 0], "span": [0, 2]}})
        )),
        Some(String::from("bad span"))
    );
    toggle_floating(&mut f, client, a);
    let before = grid(&mut f);
    assert_eq!(
        err(ask(
            &mut f,
            json!({"NewmPlaceWindow": {"id": a, "tile": [0, 0]}})
        )),
        Some(String::from("the window is not on the grid"))
    );
    assert_eq!(
        grid(&mut f)[&b],
        before[&b],
        "a refused request moves nothing"
    );
}
