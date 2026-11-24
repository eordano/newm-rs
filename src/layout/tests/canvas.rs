use std::collections::HashMap;

use newm_canvas::{Canvas, Dir, Edge, Rect};
use niri_config::animations::{Curve, EasingParams, Kind};
use niri_config::PresetSize;
use niri_ipc::NewmBlockAnchor;

use super::*;
use crate::layout::canvas::{RestoreCanvas, RestoreWindow};
use crate::window::overlay_position;

#[derive(Debug, Clone)]
enum CanvasOp {
    Op(Box<Op>),
    ResizeEdge(Edge, i32),
    Zoom(i32),
    SendToEdge(Dir),
    SwapNeighbour(Dir),
    MoveFocus(Dir),
    ZoomToFit,
    ZoomBack,
    CycleFloatingFollow,
    GatherIslands,
}

fn arbitrary_edge() -> impl Strategy<Value = Edge> {
    prop_oneof![
        Just(Edge::Left),
        Just(Edge::Right),
        Just(Edge::Top),
        Just(Edge::Bottom),
    ]
}

fn arbitrary_canvas_op() -> impl Strategy<Value = CanvasOp> {
    prop_oneof![
        3 => any::<Op>().prop_map(op),
        1 => (arbitrary_edge(), -2..=2i32).prop_map(|(e, d)| CanvasOp::ResizeEdge(e, d)),
        1 => prop_oneof![Just(-1), Just(1)].prop_map(CanvasOp::Zoom),
        1 => prop_oneof![Just(Dir::Left), Just(Dir::Right)].prop_map(CanvasOp::SendToEdge),
        1 => prop_oneof![Just(Dir::Left), Just(Dir::Right), Just(Dir::Up), Just(Dir::Down)]
            .prop_map(CanvasOp::SwapNeighbour),
        1 => prop_oneof![Just(CanvasOp::ZoomToFit), Just(CanvasOp::ZoomBack)],
        1 => Just(CanvasOp::CycleFloatingFollow),
        1 => Just(CanvasOp::GatherIslands),
    ]
}

fn canvases(layout: &Layout<TestWindow>) -> HashMap<WorkspaceId, Canvas> {
    layout
        .workspaces()
        .map(|(_, _, ws)| (ws.id(), ws.scrolling().canvas().clone()))
        .collect()
}

#[track_caller]
fn check_canvas(layout: &Layout<TestWindow>, before: &HashMap<WorkspaceId, Canvas>, zoomed: bool) {
    layout.verify_invariants();

    for (_, _, ws) in layout.workspaces() {
        let space = ws.scrolling();
        let canvas = space.canvas();

        let rects: Vec<Rect> = canvas.windows().iter().map(|w| w.rect).collect();
        for (a, ra) in rects.iter().enumerate() {
            for rb in &rects[a + 1..] {
                assert!(!ra.overlaps(rb), "{ra:?} and {rb:?} overlap");
            }
        }

        if !zoomed && let Some(old) = before.get(&ws.id()) {
            assert_eq!(
                old.zoom(),
                canvas.zoom(),
                "zoom changed without a zoom action"
            );
        }

        if !space.view_offset().is_gesture()
            && let Some(r) = canvas.focus().and_then(|f| canvas.rect(f))
            && canvas.fits_on_screen(r.w, r.h)
        {
            assert!(canvas.is_visible(r), "focused {r:?} is off screen");
        }

        for tile in space.tiles() {
            let win = tile.window();
            if !win.pending_sizing_mode().is_normal() {
                continue;
            }
            let Some(size) = win.requested_size() else {
                continue;
            };
            let min = win.min_size();
            // A minimum past the canvas's cap is capped by design, and the window is asked for
            // less (canvas_caps_a_huge_minimum_size); niri's generator makes such minimums.
            let (cap_w, cap_h) = space.min_cap();
            let tile_min = tile.min_size_nonfullscreen();
            if tile_min.w <= cap_w {
                assert!(size.w >= min.w, "width {} below minimum {}", size.w, min.w);
            }
            if tile_min.h <= cap_h {
                assert!(size.h >= min.h, "height {} below minimum {}", size.h, min.h);
            }
        }
    }
}

#[track_caller]
fn check_canvas_ops(ops: impl IntoIterator<Item = CanvasOp>) -> Layout<TestWindow> {
    let mut layout = Layout::default();
    check_canvas_ops_on(&mut layout, ops);
    layout
}

/// The last size the layout asked each tiled window for.
fn requested_sizes(layout: &Layout<TestWindow>) -> HashMap<usize, Option<Size<i32, Logical>>> {
    layout
        .workspaces()
        .flat_map(|(_, _, ws)| ws.scrolling().tiles())
        .map(|tile| (*tile.window().id(), tile.window().requested_size()))
        .collect()
}

/// A zoom moves no window on the grid and sends none a new size.
#[track_caller]
fn check_zoom_kept_windows(
    layout: &Layout<TestWindow>,
    before: &HashMap<WorkspaceId, Canvas>,
    sizes: &HashMap<usize, Option<Size<i32, Logical>>>,
) {
    for (_, _, ws) in layout.workspaces() {
        if let Some(old) = before.get(&ws.id()) {
            assert_eq!(
                old.windows(),
                ws.scrolling().canvas().windows(),
                "a zoom moved a window"
            );
        }
    }
    assert_eq!(
        &requested_sizes(layout),
        sizes,
        "a zoom sent a window a new size"
    );
}

#[track_caller]
fn check_canvas_ops_on(layout: &mut Layout<TestWindow>, ops: impl IntoIterator<Item = CanvasOp>) {
    for op in ops {
        let before = canvases(layout);
        let sizes = requested_sizes(layout);
        let spans = grid_spans(layout);
        let resizing = resizes(&op);
        let zoomed = matches!(
            op,
            CanvasOp::Zoom(_) | CanvasOp::ZoomToFit | CanvasOp::ZoomBack
        );
        apply_canvas_op(layout, op);
        if zoomed {
            check_zoom_kept_windows(layout, &before, &sizes);
        }
        if !resizing {
            check_spans_kept(layout, &spans);
        }
        check_canvas(layout, &before, zoomed);
    }
}

fn apply_canvas_op(layout: &mut Layout<TestWindow>, op: CanvasOp) {
    match op {
        CanvasOp::Op(op) => op.apply(layout),
        CanvasOp::ResizeEdge(edge, delta) => layout.canvas_resize_edge(edge, delta),
        CanvasOp::Zoom(delta) => layout.canvas_zoom(delta),
        CanvasOp::SendToEdge(dir) => layout.canvas_send_to_edge(dir),
        CanvasOp::SwapNeighbour(dir) => {
            layout.canvas_swap_neighbour(dir);
        }
        CanvasOp::MoveFocus(dir) => {
            layout.canvas_move_focus(dir);
        }
        CanvasOp::ZoomToFit => layout.canvas_zoom_to_fit(),
        CanvasOp::ZoomBack => {
            layout.canvas_zoom_back();
        }
        CanvasOp::CycleFloatingFollow => layout.cycle_floating_follow(),
        CanvasOp::GatherIslands => {
            layout.canvas_gather_islands();
        }
    }
}

/// A client may ask for any minimum size (`xdg_toplevel.set_min_size` takes an i32), as it may
/// commit any size. The canvas caps what it reads, so its tile math (i32 areas and edges,
/// overflow-checked in release builds) stays in range. The window is then asked for less than its
/// minimum, which [`check_canvas`] allows only past the cap; this test checks the rest: no panic,
/// no overlap, the capped span.
#[test]
fn canvas_caps_a_huge_minimum_size() {
    let mut layout = Layout::default();
    let ops = [
        op(Op::AddOutput(1)),
        add(0),
        add_min(1, i32::MAX, i32::MAX),
        add(2),
        add_min(3, i32::MAX, 1),
        op(Op::FocusColumnLeft),
        op(Op::MoveColumnLeft),
        CanvasOp::MoveFocus(Dir::Down),
        CanvasOp::ResizeEdge(Edge::Right, 2),
        CanvasOp::ResizeEdge(Edge::Bottom, -1),
        CanvasOp::SwapNeighbour(Dir::Right),
        CanvasOp::SendToEdge(Dir::Left),
        CanvasOp::Zoom(-1),
        CanvasOp::ZoomToFit,
        CanvasOp::ZoomBack,
        CanvasOp::GatherIslands,
        op(Op::CloseWindow(0)),
        add(4),
        op(Op::CloseWindow(1)),
        add(5),
    ];
    for op in ops {
        apply_canvas_op(&mut layout, op);
        layout.verify_invariants();
        let canvas = layout.active_workspace().unwrap().scrolling().canvas();
        let rects: Vec<Rect> = canvas.windows().iter().map(|w| w.rect).collect();
        for (a, ra) in rects.iter().enumerate() {
            for rb in &rects[a + 1..] {
                assert!(!ra.overlaps(rb), "{ra:?} and {rb:?} overlap");
            }
        }
        for r in rects {
            assert!(r.w <= 1024 && r.h <= 1024, "{r:?} is not capped");
        }
    }
}

/// niri's window generator gives a window a minimum of (0, n) for any positive n, past the cap
/// too: canvas_random_operations hit one of 506102105 px. check_canvas holds every other window to
/// its minimum and lets the capped axis be asked for less.
#[test]
fn canvas_check_allows_a_minimum_past_the_cap() {
    check_canvas_ops([
        op(Op::AddOutput(1)),
        add(0),
        add_min(1, 0, 506_102_105),
        add_min(2, 300, 200),
        CanvasOp::ResizeEdge(Edge::Bottom, 1),
        op(Op::FocusColumnLeft),
        CanvasOp::Zoom(1),
    ]);
}

/// Each grid window's tiles and the workspace whose canvas holds them, by window id.
fn grid_spans(layout: &Layout<TestWindow>) -> HashMap<usize, (WorkspaceId, Rect)> {
    layout
        .workspaces()
        .flat_map(|(_, _, ws)| {
            ws.scrolling()
                .grid_windows()
                .map(move |(w, r)| (*w.id(), (ws.id(), r)))
        })
        .collect()
}

/// The operations that may change a span: the explicit resizes.
fn resizes(op: &CanvasOp) -> bool {
    match op {
        CanvasOp::ResizeEdge(..) => true,
        CanvasOp::Op(op) => matches!(
            **op,
            Op::SwitchPresetColumnWidth
                | Op::SwitchPresetColumnWidthBack
                | Op::SwitchPresetWindowWidth { .. }
                | Op::SwitchPresetWindowWidthBack { .. }
                | Op::SwitchPresetWindowHeight { .. }
                | Op::SwitchPresetWindowHeightBack { .. }
                | Op::MaximizeColumn
                | Op::SetColumnWidth(_)
                | Op::SetWindowWidth { .. }
                | Op::SetWindowHeight { .. }
                | Op::ResetWindowHeight { .. }
                | Op::ExpandColumnToAvailableWidth
                | Op::InteractiveResizeUpdate { .. }
                | Op::InteractiveResizeEnd { .. }
        ),
        _ => false,
    }
}

/// The span principle (newm-canvas proof C47, C48): outside a resize a window keeps its span, or
/// gets that span as the canvas holding it now opens it, cut to what fits and raised to its
/// minimum. A move never opens it at a new window's span.
#[track_caller]
fn check_spans_kept(layout: &Layout<TestWindow>, before: &HashMap<usize, (WorkspaceId, Rect)>) {
    let canvases = canvases(layout);
    for (id, (ws, r)) in grid_spans(layout) {
        let Some((_, old)) = before.get(&id) else {
            continue;
        };
        if (r.w, r.h) == (old.w, old.h) {
            continue;
        }
        let canvas = &canvases[&ws];
        let w = canvas.windows().iter().find(|w| w.rect == r).unwrap();
        assert_eq!(
            (r.w, r.h),
            canvas.open_span(w.min_w, w.min_h, Some((old.w, old.h))),
            "window {id} went from {old:?} to {r:?} without a resize"
        );
    }
}

fn op(op: Op) -> CanvasOp {
    CanvasOp::Op(Box::new(op))
}

fn add(id: usize) -> CanvasOp {
    op(Op::AddWindow {
        params: TestWindowParams::new(id),
    })
}

fn add_min(id: usize, w: i32, h: i32) -> CanvasOp {
    let mut params = TestWindowParams::new(id);
    params.min_max_size = (Size::from((w, h)), Size::from((0, 0)));
    op(Op::AddWindow { params })
}

fn add_sized(id: usize, width: Option<PresetSize>, height: Option<PresetSize>) -> CanvasOp {
    let mut params = TestWindowParams::new(id);
    params.rules = Some(ResolvedWindowRules {
        default_width: Some(width),
        default_height: Some(height),
        ..ResolvedWindowRules::default()
    });
    op(Op::AddWindow { params })
}

/// The tile size the user's config sets: 240x155.
fn small_tiles() -> Layout<TestWindow> {
    let mut options = Options::default();
    options.layout.canvas.tile_width = 240.;
    options.layout.canvas.tile_height = 155.;
    Layout::with_options(Clock::with_time(Duration::ZERO), options)
}

fn focus_rect(layout: &Layout<TestWindow>) -> Rect {
    let canvas = layout.active_workspace().unwrap().scrolling().canvas();
    canvas.rect(canvas.focus().unwrap()).unwrap()
}

fn rects(layout: &Layout<TestWindow>) -> Vec<Rect> {
    let ws = layout.active_workspace().unwrap();
    let canvas = ws.scrolling().canvas();
    let mut rects: Vec<Rect> = canvas.windows().iter().map(|w| w.rect).collect();
    rects.sort_by_key(|r| (r.j, r.i));
    rects
}

#[test]
fn canvas_windows_open_where_the_view_scrolls_least() {
    let layout = check_canvas_ops([op(Op::AddOutput(1)), add(0), add(1), add(2)]);
    assert_eq!(
        rects(&layout),
        [
            Rect::new(0, 0, 1, 1),
            Rect::new(1, 0, 1, 1),
            Rect::new(1, 1, 1, 1)
        ],
        "beside the focus: right of 1 the view would scroll further than below it"
    );
}

#[test]
fn canvas_minimum_size_takes_more_tiles() {
    let layout = check_canvas_ops([op(Op::AddOutput(1)), add_min(0, 1000, 600)]);
    assert_eq!(rects(&layout), [Rect::new(0, 0, 2, 2)]);
}

#[test]
fn canvas_grow_pushes_and_shrink_closes_lake() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            CanvasOp::Zoom(1),
            add(0),
            add(1),
            op(Op::FocusColumnLeft),
            CanvasOp::ResizeEdge(Edge::Right, 1),
        ],
    );
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 2, 1), Rect::new(2, 0, 1, 1)]
    );

    layout.canvas_resize_edge(Edge::Right, -1);
    check_canvas(&layout, &canvases(&layout), false);
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 1, 1), Rect::new(1, 0, 1, 1)]
    );
}

#[test]
fn canvas_zoom_changes_only_on_zoom() {
    let mut layout = check_canvas_ops([
        op(Op::AddOutput(1)),
        add(0),
        add(1),
        CanvasOp::Zoom(1),
        op(Op::FocusColumnLeft),
        CanvasOp::ResizeEdge(Edge::Bottom, 1),
    ]);
    let zoom = |layout: &Layout<TestWindow>| {
        layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .canvas()
            .zoom()
    };
    assert_eq!(zoom(&layout), 1);
    layout.canvas_zoom(-1);
    assert_eq!(zoom(&layout), 0);
}

#[test]
fn canvas_zoom_reset_returns_to_one_to_one() {
    let mut layout = check_canvas_ops([op(Op::AddOutput(1)), add(0), add(1), CanvasOp::Zoom(2)]);
    let zoom = |layout: &Layout<TestWindow>| {
        layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .canvas()
            .zoom()
    };
    assert_eq!(zoom(&layout), 2);
    layout.canvas_zoom_reset();
    assert_eq!(zoom(&layout), 0);
    layout.canvas_zoom(-1);
    assert_eq!(zoom(&layout), -1);
    layout.canvas_zoom_reset();
    assert_eq!(zoom(&layout), 0);
    // Reset during a held zoom-to-fit: the release does not go back to the remembered zoom.
    layout.canvas_zoom(1);
    layout.canvas_zoom_to_fit();
    layout.canvas_zoom_reset();
    assert!(!layout.canvas_zoom_back());
    assert_eq!(zoom(&layout), 0);
}

#[test]
fn canvas_send_to_edge() {
    let layout = check_canvas_ops([
        op(Op::AddOutput(1)),
        add(0),
        add(1),
        add(2),
        CanvasOp::SendToEdge(Dir::Left),
    ]);
    let ws = layout.active_workspace().unwrap();
    let canvas = ws.scrolling().canvas();
    let focus = canvas.rect(canvas.focus().unwrap()).unwrap();
    assert!(canvas
        .windows()
        .iter()
        .all(|w| w.rect.i >= focus.right() || w.rect == focus));
}

#[test]
fn canvas_placeholder_reserves_a_tile_until_it_expires() {
    let mut layout = check_canvas_ops([op(Op::AddOutput(1)), add(0)]);
    layout.canvas_launch_placeholder(String::from("app"), None, (None, None));
    layout.verify_invariants();
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 1, 1), Rect::new(1, 0, 1, 1)]
    );

    check_canvas_ops_on(&mut layout, [add(1)]);
    assert_eq!(
        rects(&layout),
        [
            Rect::new(0, 0, 1, 1),
            Rect::new(1, 0, 1, 1),
            Rect::new(0, 1, 1, 1)
        ],
        "the placeholder holds (1, 0), so 1 opens below 0"
    );

    check_canvas_ops_on(
        &mut layout,
        [op(Op::AdvanceAnimations { msec_delta: 11000 })],
    );
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 1, 1), Rect::new(0, 1, 1, 1)]
    );
}

/// A small-tile layout whose windows 0.. sit on `tiles`, window 0 focused: a restored session puts
/// each on its saved tile, so a test about drawing does not depend on where new windows open.
fn placed(tiles: &[Rect]) -> Layout<TestWindow> {
    let mut layout = small_tiles();
    check_canvas_ops_on(&mut layout, [op(Op::AddOutput(1))]);
    let canvas = RestoreCanvas {
        windows: (0..tiles.len())
            .map(|k| saved(&format!("app{k}"), "w", tiles[k]))
            .collect(),
        zoom: 0,
        view: (0., 0.),
        focus: Some(0),
    };
    layout.canvas_restore(Some("output1"), 0, canvas, Duration::from_secs(60));
    check_canvas_ops_on(
        &mut layout,
        (0..tiles.len()).map(|k| add_app(k, &format!("app{k}"), "w")),
    );
    layout
}

/// `n` one-tile windows in a row from (0, 0).
fn row(n: i32) -> Vec<Rect> {
    (0..n).map(|i| Rect::new(i, 0, 1, 1)).collect()
}

fn add_app(id: usize, app_id: &str, title: &str) -> CanvasOp {
    let mut params = TestWindowParams::new(id);
    params.app_id = Some(app_id.to_owned());
    params.title = Some(title.to_owned());
    op(Op::AddWindow { params })
}

fn saved(app_id: &str, title: &str, rect: Rect) -> RestoreWindow {
    RestoreWindow {
        app_id: app_id.to_owned(),
        title: Some(title.to_owned()),
        rect,
    }
}

/// Each grid window's rectangle by window ID.
fn window_rects(layout: &Layout<TestWindow>) -> HashMap<usize, Rect> {
    let ws = layout.active_workspace().unwrap();
    ws.scrolling()
        .grid_windows()
        .map(|(w, r)| (*w.id(), r))
        .collect()
}

#[test]
fn canvas_restore_reserves_saved_tiles_that_windows_take_in_order() {
    let mut layout = check_canvas_ops([op(Op::AddOutput(1))]);
    let canvas = RestoreCanvas {
        windows: vec![
            saved("term", "a", Rect::new(0, 0, 1, 1)),
            saved("term", "b", Rect::new(2, 0, 1, 1)),
            saved("browser", "news", Rect::new(0, 1, 1, 1)),
        ],
        zoom: 1,
        view: (900., 0.),
        focus: Some(1),
    };
    layout.canvas_restore(Some("output1"), 3, canvas, Duration::from_secs(20));
    layout.verify_invariants();
    assert!(layout.canvas_restoring());
    assert_eq!(
        rects(&layout),
        [
            Rect::new(0, 0, 1, 1),
            Rect::new(2, 0, 1, 1),
            Rect::new(0, 1, 1, 1)
        ],
        "a placeholder on each saved rectangle"
    );

    check_canvas_ops_on(
        &mut layout,
        [add_app(1, "term", "b"), add_app(2, "term", "x"), add(3)],
    );
    let by_id = window_rects(&layout);
    assert_eq!(by_id[&1], Rect::new(2, 0, 1, 1), "the title picks the spot");
    assert_eq!(by_id[&2], Rect::new(0, 0, 1, 1), "else the first saved");
    assert_eq!(
        by_id[&3],
        Rect::new(1, 0, 1, 1),
        "no spot saved: opens normally"
    );
    assert!(layout.canvas_restoring(), "the browser has not come back");
    assert_eq!(
        layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .canvas()
            .zoom(),
        0
    );

    Op::AddWindow {
        params: {
            let mut params = TestWindowParams::new(4);
            params.app_id = Some(String::from("browser"));
            params
        },
    }
    .apply(&mut layout);
    layout.verify_invariants();
    assert!(!layout.canvas_restoring());
    assert_eq!(window_rects(&layout)[&4], Rect::new(0, 1, 1, 1));
    let space = layout.active_workspace().unwrap().scrolling();
    let canvas = space.canvas();
    assert_eq!(canvas.zoom(), 1, "the saved zoom once every window is back");
    let (x, y) = canvas.view();
    assert!(
        (x - 720.).abs() < 1e-6 && y.abs() < 1e-6,
        "the saved view at 1:1"
    );
    assert_eq!(space.active_window().map(|w| *w.id()), Some(1));
}

/// A session saved where an app runs under X11 restores where it runs on Wayland: the app's
/// X11 class and its Wayland app ID differ by case, and are one app.
#[test]
fn canvas_restore_matches_an_app_whatever_the_case_of_its_id() {
    let mut layout = check_canvas_ops([op(Op::AddOutput(1))]);
    let canvas = RestoreCanvas {
        windows: vec![
            saved("Chromium-browser", "news", Rect::new(2, 0, 1, 1)),
            saved("com.mitchellh.ghostty", "a", Rect::new(4, 0, 1, 1)),
        ],
        zoom: 0,
        view: (0., 0.),
        focus: Some(0),
    };
    layout.canvas_restore(None, 0, canvas, Duration::from_secs(20));
    check_canvas_ops_on(
        &mut layout,
        [
            add_app(1, "chromium-browser", "news"),
            add_app(2, "COM.MITCHELLH.GHOSTTY", "a"),
        ],
    );
    let by_id = window_rects(&layout);
    assert_eq!(
        by_id[&1],
        Rect::new(2, 0, 1, 1),
        "the Wayland Chromium takes the X11 spot"
    );
    assert_eq!(by_id[&2], Rect::new(4, 0, 1, 1));
    assert!(!layout.canvas_restoring(), "no placeholder waits");
    assert_eq!(rects(&layout).len(), 2);
}

#[test]
fn canvas_restore_drops_unmatched_placeholders_at_its_deadline() {
    let mut layout = check_canvas_ops([op(Op::AddOutput(1)), add(9)]);
    let canvas = RestoreCanvas {
        windows: vec![
            saved("a", "t", Rect::new(3, 0, 1, 1)),
            saved("b", "t", Rect::new(4, 0, 1, 1)),
        ],
        zoom: 0,
        view: (0., 0.),
        focus: Some(0),
    };
    layout.canvas_restore(None, 0, canvas, Duration::from_secs(20));
    check_canvas_ops_on(
        &mut layout,
        [
            add_app(1, "a", "t"),
            op(Op::AdvanceAnimations { msec_delta: 19000 }),
        ],
    );
    assert!(layout.canvas_restoring());
    assert_eq!(rects(&layout).len(), 3);

    check_canvas_ops_on(
        &mut layout,
        [op(Op::AdvanceAnimations { msec_delta: 1000 })],
    );
    assert!(!layout.canvas_restoring());
    let by_id = window_rects(&layout);
    assert_eq!(by_id.len(), 2);
    assert_eq!(by_id[&1], Rect::new(3, 0, 1, 1));
    assert_eq!(
        rects(&layout).len(),
        2,
        "the placeholder no window took is gone"
    );
    let space = layout.active_workspace().unwrap().scrolling();
    assert_eq!(space.active_window().map(|w| *w.id()), Some(1));
}

/// Gives window `id` a new title, as an app does after it maps.
fn retitle(layout: &mut Layout<TestWindow>, id: usize, title: &str) {
    let window = layout
        .windows()
        .map(|(_, w)| w)
        .find(|w| *w.id() == id)
        .unwrap()
        .clone();
    *window.0.title.borrow_mut() = Some(title.to_owned());
    layout.canvas_retitled(&id);
    layout.verify_invariants();
}

#[test]
fn canvas_restore_moves_windows_to_their_saved_spots_as_titles_arrive() {
    let mut layout = check_canvas_ops([op(Op::AddOutput(1))]);
    let canvas = RestoreCanvas {
        windows: vec![
            saved("term", "desk: ✳ a", Rect::new(0, 0, 1, 1)),
            saved("term", "build: ✳ b", Rect::new(2, 0, 1, 1)),
            saved("browser", "* Slack - Chromium", Rect::new(0, 1, 1, 1)),
            saved("browser", "PR 1 - Chromium", Rect::new(2, 1, 1, 1)),
        ],
        zoom: 0,
        view: (0., 0.),
        focus: Some(1),
    };
    layout.canvas_restore(None, 0, canvas, Duration::from_secs(20));

    check_canvas_ops_on(
        &mut layout,
        [add_app(1, "term", "ghostty"), add_app(2, "term", "ghostty")],
    );
    let by_id = window_rects(&layout);
    assert_eq!(
        by_id[&1],
        Rect::new(0, 0, 1, 1),
        "no title yet: the first spot"
    );
    assert_eq!(by_id[&2], Rect::new(2, 0, 1, 1));

    retitle(&mut layout, 1, "build: ✶ b");
    let by_id = window_rects(&layout);
    assert_eq!(
        by_id[&1],
        Rect::new(2, 0, 1, 1),
        "its title takes it to its spot"
    );
    assert_eq!(
        by_id[&2],
        Rect::new(0, 0, 1, 1),
        "the other gets the one it left"
    );
    retitle(&mut layout, 2, "desk: ✳ a");
    assert_eq!(
        window_rects(&layout)[&2],
        Rect::new(0, 0, 1, 1),
        "already home"
    );

    check_canvas_ops_on(&mut layout, [add_app(3, "browser", "New Tab - Chromium")]);
    assert_eq!(window_rects(&layout)[&3], Rect::new(0, 1, 1, 1));
    retitle(&mut layout, 3, "PR 1 - Chromium");
    assert_eq!(
        window_rects(&layout)[&3],
        Rect::new(2, 1, 1, 1),
        "it trades with the placeholder still waiting"
    );
    assert!(layout.canvas_restoring(), "the Slack spot still waits");
    check_canvas_ops_on(
        &mut layout,
        [add_app(4, "browser", "Slack - 1 new item - Chromium")],
    );
    let by_id = window_rects(&layout);
    assert_eq!(by_id[&4], Rect::new(0, 1, 1, 1), "the spot the PR left");
    assert!(!layout.canvas_restoring());
    let space = layout.active_workspace().unwrap().scrolling();
    assert_eq!(
        space.active_window().map(|w| *w.id()),
        Some(1),
        "the saved focus follows the spot"
    );
    assert_eq!(rects(&layout).len(), 4, "no placeholder left over");
}

#[test]
fn canvas_restore_stops_trading_spots_after_a_while() {
    let mut layout = check_canvas_ops([op(Op::AddOutput(1))]);
    let canvas = RestoreCanvas {
        windows: vec![
            saved("term", "a", Rect::new(0, 0, 1, 1)),
            saved("term", "b", Rect::new(2, 0, 1, 1)),
        ],
        zoom: 0,
        view: (0., 0.),
        focus: None,
    };
    layout.canvas_restore(None, 0, canvas, Duration::from_secs(20));
    check_canvas_ops_on(
        &mut layout,
        [
            add_app(1, "term", "x"),
            add_app(2, "term", "y"),
            op(Op::AdvanceAnimations { msec_delta: 91_000 }),
        ],
    );
    retitle(&mut layout, 1, "b");
    assert_eq!(
        window_rects(&layout)[&1],
        Rect::new(0, 0, 1, 1),
        "settled: stays"
    );
}

#[test]
fn canvas_retitles_outside_a_restore_move_nothing() {
    let mut layout = check_canvas_ops([op(Op::AddOutput(1)), add_app(1, "term", "a"), add(2)]);
    let before = window_rects(&layout);
    retitle(&mut layout, 1, "b");
    assert_eq!(window_rects(&layout), before);
}

#[test]
fn canvas_rules_open_windows_at_their_default_span() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add_sized(
                0,
                Some(PresetSize::Fixed(480)),
                Some(PresetSize::Proportion(0.5)),
            ),
            add(1),
            add_sized(
                2,
                Some(PresetSize::Fixed(1680)),
                Some(PresetSize::Fixed(1085)),
            ),
        ],
    );
    assert_eq!(
        rects(&layout),
        [
            Rect::new(0, 0, 2, 2),
            Rect::new(2, 0, 1, 1),
            Rect::new(2, 1, 5, 4)
        ],
        "480 px is 2 tiles, half of 704 px rounds to 2, and 7x7 tiles are cut to the 5x4 that fit"
    );

    let rules = ResolvedWindowRules {
        default_width: Some(Some(PresetSize::Fixed(480))),
        default_height: Some(Some(PresetSize::Proportion(0.5))),
        ..ResolvedWindowRules::default()
    };
    let space = layout.active_workspace().unwrap().scrolling();
    let size = space.new_window_size(None, None, &rules);
    assert_eq!(size, Size::from((480 - 16, 310 - 16)));

    layout.canvas_launch_placeholder(
        String::from("app"),
        None,
        (Some(Some(PresetSize::Fixed(480))), None),
    );
    layout.verify_invariants();
    assert!(
        rects(&layout).iter().any(|r| (r.w, r.h) == (2, 1)),
        "the placeholder holds the rule's 480 px as two tiles"
    );
}

fn add_with_rules(
    id: usize,
    width: Option<Option<PresetSize>>,
    height: Option<Option<PresetSize>>,
) -> CanvasOp {
    let mut params = TestWindowParams::new(id);
    params.rules = Some(ResolvedWindowRules {
        default_width: width,
        default_height: height,
        ..ResolvedWindowRules::default()
    });
    op(Op::AddWindow { params })
}

/// One output whose canvas is `w`x`h` at 1:1 (the output less the default 16 px gaps), with the
/// user's 119x115 tiles and the canvas default size `default`.
fn sized_canvas(
    w: i32,
    h: i32,
    default: (Option<PresetSize>, Option<PresetSize>),
) -> Layout<TestWindow> {
    let mut options = Options::default();
    options.layout.canvas.tile_width = 119.;
    options.layout.canvas.tile_height = 115.;
    (
        options.layout.canvas.default_width,
        options.layout.canvas.default_height,
    ) = default;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    let output = Output::new(
        String::from("output1"),
        PhysicalProperties {
            size: Size::from((w + 16, h + 16)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((w + 16, h + 16)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: String::from("output1"),
        make: None,
        model: None,
        serial: None,
    });
    layout.add_output(output, None);
    let canvas = layout.active_workspace().unwrap().scrolling().canvas();
    assert_eq!(canvas.screen(), (f64::from(w), f64::from(h)));
    layout
}

const USER_DEFAULT: (Option<PresetSize>, Option<PresetSize>) = (
    Some(PresetSize::Fixed(960)),
    Some(PresetSize::Proportion(0.6667)),
);

/// The canvas's `default-width { fixed 960; }` and `default-height { proportion 0.6667; }`
/// open a window no rule sizes at the nearest whole tiles: 960 / 119 = 8.07 gives 8 columns,
/// and 0.6667 of the canvas height gives round(0.6667 * 920 / 115) = 5 rows on a
/// 1428x920 canvas, round(0.6667 * 1112 / 115) = 6 on a 4084x1112 one. A placeholder opens
/// there too.
#[test]
fn canvas_default_size_opens_windows_no_rule_sizes() {
    for (w, h, rows) in [(1428, 920, 5), (4084, 1112, 6)] {
        let mut layout = sized_canvas(w, h, USER_DEFAULT);
        check_canvas_ops_on(&mut layout, [add(0)]);
        assert_eq!(rects(&layout), [Rect::new(0, 0, 8, rows)], "{w}x{h}");
        let space = layout.active_workspace().unwrap().scrolling();
        let size = space.new_window_size(None, None, &ResolvedWindowRules::default());
        assert_eq!(
            size,
            Size::from((0, 0)),
            "{w}x{h}: the first configure leaves both axes to the app now; the default applies \
             only to a window that picks no size (add maps without one)"
        );

        layout.canvas_launch_placeholder(String::from("app"), None, (None, None));
        layout.verify_invariants();
        let spans = rects(&layout)
            .iter()
            .filter(|r| (r.w, r.h) == (8, rows))
            .count();
        assert_eq!(
            spans, 2,
            "{w}x{h}: the placeholder takes the default span too"
        );
    }

    let mut layout = sized_canvas(1428, 920, (None, None));
    check_canvas_ops_on(&mut layout, [add(0)]);
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 1, 1)],
        "unset, a window opens at one tile"
    );
}

/// A window rule's default size wins on its own axis only, and an empty rule asks for one tile
/// there; the other axis takes the canvas's default.
#[test]
fn canvas_window_rule_overrides_the_default_size_per_axis() {
    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    let fixed = |px| Some(Some(PresetSize::Fixed(px)));
    check_canvas_ops_on(&mut layout, [add_with_rules(0, fixed(480), None)]);
    let r = focus_rect(&layout);
    assert_eq!(
        (r.w, r.h),
        (4, 5),
        "480 / 119 = 4.03 columns, the default 5 rows"
    );
    check_canvas_ops_on(&mut layout, [add_with_rules(1, None, fixed(230))]);
    let r = focus_rect(&layout);
    assert_eq!(
        (r.w, r.h),
        (8, 2),
        "the default 8 columns, 230 / 115 = 2 rows"
    );
    check_canvas_ops_on(&mut layout, [add_with_rules(2, Some(None), None)]);
    let r = focus_rect(&layout);
    assert_eq!((r.w, r.h), (1, 5), "an empty rule asks for one tile");

    layout.canvas_launch_placeholder(String::from("app"), None, (fixed(480), None));
    layout.verify_invariants();
    let spans = rects(&layout)
        .iter()
        .filter(|r| (r.w, r.h) == (4, 5))
        .count();
    assert_eq!(spans, 2, "the placeholder takes 0's span");
}

/// A fixed size and a proportion both give the nearest whole number of tiles at 1:1, at least
/// one, cut to what fits on the screen.
#[test]
fn canvas_default_sizes_round_to_the_nearest_tile() {
    use niri_config::PresetSize::{Fixed, Proportion};

    let layout = sized_canvas(1428, 920, (None, None));
    let space = layout.active_workspace().unwrap().scrolling();
    let span = |width: PresetSize, height: PresetSize| {
        let rules = ResolvedWindowRules {
            default_width: Some(Some(width)),
            default_height: Some(Some(height)),
            ..ResolvedWindowRules::default()
        };
        let size = space.new_window_size(None, None, &rules);
        ((size.w + 16) / 119, (size.h + 16) / 115)
    };
    assert_eq!(
        span(Fixed(960), Fixed(287)),
        (8, 2),
        "960 / 119 = 8.07 and 287 / 115 = 2.496 round down"
    );
    assert_eq!(
        span(Fixed(1011), Fixed(288)),
        (8, 3),
        "1011 / 119 = 8.496 rounds down, 288 / 115 = 2.504 up"
    );
    assert_eq!(
        span(Fixed(1012), Proportion(0.7)),
        (9, 6),
        "1012 / 119 = 8.504 and 0.7 * 920 / 115 = 5.6 round up"
    );
    assert_eq!(
        span(Fixed(30), Proportion(0.01)),
        (1, 1),
        "0.25 and 0.08 tiles give one tile"
    );
    assert_eq!(
        span(Proportion(0.33), Proportion(0.6667)),
        (4, 5),
        "0.33 * 1428 / 119 = 3.96 and 0.6667 * 920 / 115 = 5.33"
    );
    assert_eq!(
        span(Fixed(5000), Proportion(2.)),
        (12, 8),
        "cut to the 12x8 tiles that fit at 1:1"
    );
}

/// Opens `params` as the compositor opens a tiled window: the first configure asks for what
/// `Workspace::new_window_size` gives (0 on an axis it leaves to the app), the app answers it
/// (keeping its `bbox` size on a 0 axis), the window maps, and then takes its cell.
fn open_asked(layout: &mut Layout<TestWindow>, params: TestWindowParams) -> Size<i32, Logical> {
    let win = TestWindow::new(params);
    let ws = layout.active_workspace().unwrap();
    let minmax = (win.0.min_size, win.0.max_size);
    let size = ws.new_window_size(None, None, false, &win.0.rules, minmax);
    win.clone()
        .request_size(size, SizingMode::Normal, false, None);
    win.communicate();
    layout.add_window(
        win,
        AddWindowTarget::Auto,
        None,
        None,
        false,
        false,
        ActivateWindow::default(),
    );
    layout.verify_invariants();
    settle(layout);
    size
}

fn bbox_params(id: usize, app_id: Option<&str>, w: i32, h: i32) -> TestWindowParams {
    let mut params = TestWindowParams::new(id);
    params.bbox = Rectangle::from_size(Size::from((w, h)));
    params.app_id = app_id.map(str::to_owned);
    params
}

/// The usage log's last open line.
#[cfg(feature = "telemetry")]
fn last_open(layout: &Layout<TestWindow>) -> serde_json::Value {
    usage_lines(layout, 0)
        .into_iter()
        .rev()
        .find(|l| l["op"] == "open")
        .expect("the open is logged")
}

/// A window no rule sizes opens at the size it picked from a 0x0 configure, rounded up to whole
/// tiles with its gaps (300 + 17 px over 119 px tiles, 250 + 17 over 115), not at the canvas's
/// default size: the default is for an app that picks none. It takes one more configure, the
/// cell, which the usage log counts when the window settles.
#[test]
fn canvas_app_picked_size_rounds_up_to_whole_tiles() {
    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    let asked = open_asked(&mut layout, bbox_params(0, None, 300, 250));
    assert_eq!(
        asked,
        Size::from((0, 0)),
        "the first configure asks nothing"
    );
    assert_eq!(rects(&layout), [Rect::new(0, 0, 3, 3)]);
    assert_eq!(
        requested_size(&layout, 0),
        Some(Size::from((3 * 119 - 16, 3 * 115 - 16))),
        "then the cell"
    );
    #[cfg(feature = "telemetry")]
    {
        let open = last_open(&layout);
        assert_eq!(open["size_source"], "app", "{open:#}");
        assert_eq!(open["app_px"], serde_json::json!([300, 250]), "{open:#}");
        assert_eq!(open["span"], serde_json::json!([3, 3]), "{open:#}");
        assert_eq!(open["focused"], true, "{open:#}");
        assert!(open["op_us"].is_u64(), "{open:#}");
        let lines = usage_lines(&layout, 0);
        let settle = lines
            .iter()
            .find(|l| l["op"] == "settle")
            .unwrap_or_else(|| panic!("the window settles: {lines:#?}"));
        assert_eq!(settle["configures"], 2, "0x0, then the cell: {settle:#}");
    }
}

/// An app that picks no size opens at the span of the most recently focused open window of its
/// app on the canvas, before the canvas's default size.
#[test]
fn canvas_same_app_sibling_gives_its_span() {
    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    open_asked(&mut layout, bbox_params(0, Some("term"), 300, 250));
    open_asked(&mut layout, bbox_params(1, Some("other"), 0, 0));
    assert_eq!(window_rects(&layout)[&1].w, 8, "another app: the default");
    open_asked(&mut layout, bbox_params(2, Some("term"), 0, 0));
    let r = window_rects(&layout)[&2];
    assert_eq!((r.w, r.h), (3, 3), "term's open window's span");
    #[cfg(feature = "telemetry")]
    assert_eq!(last_open(&layout)["size_source"], "sibling");
}

/// With none of its app's windows open, an app that picks no size opens at the span its app's
/// last window here closed with.
#[test]
fn canvas_last_closed_window_gives_its_span() {
    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    open_asked(&mut layout, bbox_params(0, Some("term"), 300, 250));
    check_canvas_ops_on(&mut layout, [op(Op::CloseWindow(0))]);
    assert!(rects(&layout).is_empty());
    open_asked(&mut layout, bbox_params(1, Some("term"), 0, 0));
    let r = window_rects(&layout)[&1];
    assert_eq!((r.w, r.h), (3, 3), "the closed window's span");
    #[cfg(feature = "telemetry")]
    assert_eq!(last_open(&layout)["size_source"], "last-closed");
}

/// The span memory knows an app whatever the case of its app ID.
#[test]
fn canvas_span_memory_folds_the_case_of_app_ids() {
    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    open_asked(&mut layout, bbox_params(0, Some("Term"), 300, 250));
    open_asked(&mut layout, bbox_params(1, Some("term"), 0, 0));
    let r = window_rects(&layout)[&1];
    assert_eq!((r.w, r.h), (3, 3), "Term's open window's span");
    check_canvas_ops_on(
        &mut layout,
        [op(Op::CloseWindow(0)), op(Op::CloseWindow(1))],
    );
    open_asked(&mut layout, bbox_params(2, Some("TERM"), 0, 0));
    let r = window_rects(&layout)[&2];
    assert_eq!((r.w, r.h), (3, 3), "the closed window's span");
    #[cfg(feature = "telemetry")]
    assert_eq!(last_open(&layout)["size_source"], "last-closed");
}

/// A window rule's size is the user's choice for the app: it beats the size the app picks on its
/// axis, and the configure asks for it there; the app still picks on the other axis.
#[test]
fn canvas_rule_beats_the_app() {
    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    let mut params = bbox_params(0, None, 300, 250);
    params.rules = Some(ResolvedWindowRules {
        default_width: Some(Some(PresetSize::Fixed(480))),
        ..ResolvedWindowRules::default()
    });
    let asked = open_asked(&mut layout, params);
    assert_eq!(
        asked,
        Size::from((4 * 119 - 16, 0)),
        "480 px: 4 tiles; height left to the app"
    );
    assert_eq!(rects(&layout), [Rect::new(0, 0, 4, 3)]);
    #[cfg(feature = "telemetry")]
    assert_eq!(
        last_open(&layout)["size_source"],
        serde_json::json!({ "w": "rule", "h": "app" })
    );
}

/// An app that picks no size, with no window of its app to go by, opens at the canvas's default
/// size, and at one tile without one.
#[test]
fn canvas_no_useful_size_falls_back_to_the_default() {
    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    open_asked(&mut layout, bbox_params(0, Some("new"), 0, 0));
    assert_eq!(rects(&layout), [Rect::new(0, 0, 8, 5)]);
    #[cfg(feature = "telemetry")]
    assert_eq!(last_open(&layout)["size_source"], "default");

    let mut layout = sized_canvas(1428, 920, (None, None));
    open_asked(&mut layout, bbox_params(0, None, 0, 0));
    assert_eq!(rects(&layout), [Rect::new(0, 0, 1, 1)]);
    #[cfg(feature = "telemetry")]
    assert_eq!(last_open(&layout)["size_source"], "tile");
}

/// An app that keeps a larger size than its cell grows its span through the placement rules,
/// and the usage log says so.
#[cfg(feature = "telemetry")]
#[test]
fn canvas_logs_an_app_growing_its_window() {
    let mut layout = sized_canvas(1428, 920, (None, None));
    check_canvas_ops_on(&mut layout, [add(0), add(1)]);
    settle(&mut layout);
    let skip = usage_lines(&layout, 0).len();
    let h = requested_size(&layout, 0).unwrap().h;
    check_canvas_ops_on(&mut layout, [forced(0, 300, h)]);
    settle(&mut layout);
    assert_eq!(window_rects(&layout)[&0].w, 3, "300 + 17 px take 3 tiles");
    let lines = usage_lines(&layout, skip);
    let grow = lines
        .iter()
        .find(|l| l["op"] == "app-grow")
        .unwrap_or_else(|| panic!("the growth is logged: {lines:#?}"));
    assert_eq!(grow["cause"], "kept-commit", "{grow:#}");
    assert_eq!(grow["pushed"], false, "{grow:#}");
}

/// The canvas's default size sizes tiled windows only: a floating window opens as without it.
#[test]
fn canvas_default_size_leaves_floating_windows_alone() {
    let floating = |default| {
        let mut layout = sized_canvas(1428, 920, default);
        let mut params = TestWindowParams::new(0);
        params.is_floating = true;
        check_canvas_ops_on(&mut layout, [op(Op::AddWindow { params })]);
        assert!(
            rects(&layout).is_empty(),
            "a floating window is not on the grid"
        );
        let ws = layout.active_workspace().unwrap();
        let rules = ResolvedWindowRules::default();
        let sizes = (Size::default(), Size::default());
        let new_size = ws.new_window_size(None, None, true, &rules, sizes);
        let (_, win) = layout.windows().find(|(_, w)| *w.id() == 0).unwrap();
        (new_size, win.requested_size())
    };
    assert_eq!(floating(USER_DEFAULT), floating((None, None)));
}

/// Adds `params` as the compositor adds a window that opens floating: at the size it committed,
/// `bbox`, next to `parent` for a dialog.
fn open_floating(
    layout: &mut Layout<TestWindow>,
    mut params: TestWindowParams,
    parent: Option<usize>,
    activate: ActivateWindow,
) {
    params.is_floating = true;
    let target = match &parent {
        Some(id) => AddWindowTarget::NextTo(id),
        None => AddWindowTarget::Auto,
    };
    let win = TestWindow::new(params);
    layout.add_window(win, target, None, None, false, true, activate);
    layout.verify_invariants();
}

fn sized_params(id: usize, w: i32, h: i32) -> TestWindowParams {
    let mut params = TestWindowParams::new(id);
    params.bbox = Rectangle::from_size(Size::from((w, h)));
    params
}

/// Where the floating window `id` is drawn in the view.
fn floating_rect(layout: &Layout<TestWindow>, id: usize) -> Rectangle<f64, Logical> {
    let ws = layout.active_workspace().unwrap();
    let (tile, pos) = ws
        .floating()
        .tiles_with_render_positions()
        .find(|(tile, _)| *tile.window().id() == id)
        .unwrap();
    Rectangle::new(pos, tile.tile_size())
}

fn requested_size(layout: &Layout<TestWindow>, id: usize) -> Option<Size<i32, Logical>> {
    let (_, win) = layout.windows().find(|(_, w)| *w.id() == id).unwrap();
    win.requested_size()
}

fn focus_id(layout: &Layout<TestWindow>) -> Option<usize> {
    layout.focus().map(|win| *win.id())
}

#[track_caller]
fn assert_close(a: Point<f64, Logical>, b: Point<f64, Logical>, what: &str) {
    assert!(
        (a.x - b.x).abs() <= 1. && (a.y - b.y).abs() <= 1.,
        "{what}: {a:?} != {b:?}"
    );
}

/// A dialog floats over its parent at the size it asked for, centred on the parent as drawn, at
/// 1:1 and on a zoomed-out canvas alike, and takes no tile.
#[test]
fn canvas_dialog_floats_at_its_own_size_centred_on_its_parent() {
    for zoom in [0, 1, 2] {
        let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
        check_canvas_ops_on(&mut layout, [add(0), op(Op::Communicate(0))]);
        if zoom != 0 {
            check_canvas_ops_on(
                &mut layout,
                [
                    CanvasOp::Zoom(zoom),
                    op(Op::AdvanceAnimations { msec_delta: 1000 }),
                ],
            );
        }
        let before = rects(&layout);

        let mut params = sized_params(1, 300, 200);
        params.parent_id = Some(0);
        open_floating(&mut layout, params, Some(0), ActivateWindow::Smart);

        assert_eq!(
            rects(&layout),
            before,
            "zoom {zoom}: the dialog took a tile"
        );
        assert_eq!(requested_size(&layout, 1), Some(Size::from((300, 200))));
        assert_eq!(focus_id(&layout), Some(1), "a dialog of the focus takes it");

        let ws = layout.active_workspace().unwrap();
        let (parent, pos, scale) = ws.tile_view_geo(&0).unwrap();
        let parent = Rectangle::new(pos, parent.tile_size().upscale(scale));
        let dialog = floating_rect(&layout, 1);
        let centre = |r: Rectangle<f64, Logical>| r.loc + r.size.downscale(2.).to_point();
        assert_close(
            centre(dialog),
            centre(parent),
            &format!("zoom {zoom}: dialog centre"),
        );
    }
}

/// Where window `id` is drawn in the view, at the scale it is drawn at.
fn drawn_rect(layout: &Layout<TestWindow>, id: usize) -> Rectangle<f64, Logical> {
    let ws = layout.active_workspace().unwrap();
    let (tile, pos, scale) = ws.tile_view_geo(&id).unwrap();
    Rectangle::new(pos, tile.tile_size().upscale(scale))
}

fn middle(r: Rectangle<f64, Logical>) -> Point<f64, Logical> {
    r.loc + r.size.downscale(2.).to_point()
}

#[track_caller]
fn assert_rect_close(a: Rectangle<f64, Logical>, b: Rectangle<f64, Logical>, what: &str) {
    assert_close(a.loc, b.loc, what);
    assert_close(a.size.to_point(), b.size.to_point(), what);
}

/// A tiled window 0 on a 1428x920 canvas with a dialog 1 of it and an overlay 2 of no parent in
/// the corner, both opened unfocused so that the camera keeps taking the zoom actions, and
/// every window taking pointer input.
fn floating_on_canvas(mode: niri_ipc::FloatingFollow) -> Layout<TestWindow> {
    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    layout.set_floating_follow(mode);
    check_canvas_ops_on(&mut layout, [add(0), op(Op::Communicate(0))]);
    let mut params = sized_params(1, 300, 200);
    params.parent_id = Some(0);
    open_floating(&mut layout, params, Some(0), ActivateWindow::No);
    let mut params = sized_params(2, 200, 150);
    params.rules = Some(ResolvedWindowRules {
        default_floating_position: Some(overlay_position(layout.options.layout.gaps)),
        ..Default::default()
    });
    open_floating(&mut layout, params, None, ActivateWindow::No);
    for (_, win) in layout.windows() {
        win.0.has_input_region.set(true);
    }
    layout
}

/// Pans the camera 30x12 with the mod key held: the content moves 30 right and 12 down.
fn pan(layout: &mut Layout<TestWindow>, end: bool) {
    let output = layout.active_output().unwrap().clone();
    layout.view_offset_gesture_begin(&output, None, false);
    assert!(layout
        .canvas_pan_update(-30., -12., Duration::from_millis(10), false)
        .is_some());
    if end {
        layout.view_offset_gesture_end(Some(false));
    }
}

/// Where the canvas now draws what was drawn at `r` when window 0 was drawn at `tiled`.
fn on_canvas(
    layout: &Layout<TestWindow>,
    tiled: Rectangle<f64, Logical>,
    r: Rectangle<f64, Logical>,
) -> Rectangle<f64, Logical> {
    let now = drawn_rect(layout, 0);
    let k = now.size.w / tiled.size.w;
    Rectangle::new(now.loc + (r.loc - tiled.loc).upscale(k), r.size.upscale(k))
}

/// By default floating windows stay where they are on screen while the canvas pans and zooms.
#[test]
fn canvas_floating_follow_screen_keeps_floating_windows_on_screen() {
    let mut layout = floating_on_canvas(niri_ipc::FloatingFollow::Screen);
    assert_eq!(layout.floating_follow(), niri_ipc::FloatingFollow::Screen);
    let tiled = drawn_rect(&layout, 0);
    let floating = [drawn_rect(&layout, 1), drawn_rect(&layout, 2)];
    pan(&mut layout, true);
    check_canvas_ops_on(&mut layout, [CanvasOp::Zoom(1), op(Op::CompleteAnimations)]);
    assert_ne!(drawn_rect(&layout, 0), tiled, "the camera moved");
    assert_eq!(
        [drawn_rect(&layout, 1), drawn_rect(&layout, 2)],
        floating,
        "the floating windows stayed"
    );
}

/// With `parent` a dialog keeps its offset to its parent as drawn, scaled as the parent is, on
/// every frame of a pan and a zoom; a floating window of no parent stays on screen. The
/// pointer finds the dialog where it is drawn.
#[test]
fn canvas_floating_follow_parent_moves_a_dialog_with_its_parent() {
    let mut layout = floating_on_canvas(niri_ipc::FloatingFollow::Parent);
    let parent = drawn_rect(&layout, 0);
    let dialog = drawn_rect(&layout, 1);
    let other = drawn_rect(&layout, 2);
    let expected = |layout: &Layout<TestWindow>| {
        let now = drawn_rect(layout, 0);
        let k = now.size.w / parent.size.w;
        Rectangle::new(
            middle(now) + (dialog.loc - middle(parent)).upscale(k),
            dialog.size.upscale(k),
        )
    };

    pan(&mut layout, false);
    assert_ne!(drawn_rect(&layout, 0), parent, "the pan moved the parent");
    assert_rect_close(drawn_rect(&layout, 1), expected(&layout), "mid-pan");
    pan(&mut layout, true);
    assert_rect_close(drawn_rect(&layout, 1), expected(&layout), "after the pan");
    assert_eq!(drawn_rect(&layout, 2), other, "no parent: it stays");

    let output = layout.active_output().unwrap().clone();
    let at = middle(drawn_rect(&layout, 1));
    let (win, hit) = layout.window_under(&output, at).unwrap();
    assert_eq!(*win.id(), 1, "the dialog is drawn under {at:?}");
    assert!(matches!(hit, HitType::Input { .. }), "1:1, it takes input");

    check_canvas_ops_on(&mut layout, [CanvasOp::Zoom(1)]);
    let mut between = false;
    for _ in 0..6 {
        check_canvas_ops_on(&mut layout, [op(Op::AdvanceAnimations { msec_delta: 40 })]);
        let scale = drawn_rect(&layout, 0).size.w / parent.size.w;
        between |= scale < 0.999 && scale > 0.801;
        let what = format!("mid-zoom at {scale}");
        assert_rect_close(drawn_rect(&layout, 1), expected(&layout), &what);
    }
    assert!(between, "a frame drew the camera between the two zooms");
    check_canvas_ops_on(&mut layout, [op(Op::CompleteAnimations)]);
    assert_rect_close(drawn_rect(&layout, 1), expected(&layout), "zoomed out");
    assert_eq!(drawn_rect(&layout, 2), other);

    let at = middle(drawn_rect(&layout, 1));
    let (win, hit) = layout.window_under(&output, at).unwrap();
    assert_eq!(*win.id(), 1, "the zoomed-out dialog is drawn under {at:?}");
    assert!(
        matches!(hit, HitType::Activate { .. }),
        "scaled, a hit only activates, as on a zoomed canvas"
    );
}

/// With `canvas` every floating window moves and scales with the canvas, and the pointer finds
/// it where it is drawn. Back to `screen`, each stays where it was drawn, at 1:1 around its
/// middle.
#[test]
fn canvas_floating_follow_canvas_moves_every_floating_window() {
    let mut layout = floating_on_canvas(niri_ipc::FloatingFollow::Canvas);
    let tiled = drawn_rect(&layout, 0);
    let before = [drawn_rect(&layout, 1), drawn_rect(&layout, 2)];

    pan(&mut layout, true);
    for (i, id) in [1, 2].into_iter().enumerate() {
        let want = on_canvas(&layout, tiled, before[i]);
        assert_rect_close(
            drawn_rect(&layout, id),
            want,
            &format!("{id} after the pan"),
        );
    }
    let drawn = drawn_rect(&layout, 2);
    let ws = layout.active_workspace().unwrap();
    let window_loc = ws.tile_view_geo(&2).unwrap().0.window_loc();
    let output = layout.active_output().unwrap().clone();
    let (win, hit) = layout.window_under(&output, middle(drawn)).unwrap();
    assert_eq!(*win.id(), 2);
    let HitType::Input { win_pos } = hit else {
        panic!("1:1, it takes input: {hit:?}");
    };
    assert_close(
        win_pos,
        drawn.loc + window_loc,
        "the surface is where it is drawn",
    );

    check_canvas_ops_on(&mut layout, [CanvasOp::Zoom(1), op(Op::CompleteAnimations)]);
    for (i, id) in [1, 2].into_iter().enumerate() {
        let want = on_canvas(&layout, tiled, before[i]);
        assert_rect_close(drawn_rect(&layout, id), want, &format!("{id} zoomed out"));
    }

    let drawn = drawn_rect(&layout, 2);
    layout.set_floating_follow(niri_ipc::FloatingFollow::Screen);
    let now = drawn_rect(&layout, 2);
    assert_close(
        middle(now),
        middle(drawn),
        "back on screen where it was drawn",
    );
    assert_eq!(now.size, before[1].size, "at 1:1");
    pan(&mut layout, true);
    assert_eq!(drawn_rect(&layout, 2), now, "and it stays on screen");
}

/// The action cycles screen, parent, canvas; a config reload keeps what it chose until the
/// config changes `floating-follow`.
#[test]
fn canvas_floating_follow_cycles_and_holds_until_the_config_changes() {
    use niri_ipc::FloatingFollow::{Canvas as OnCanvas, Parent, Screen};

    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    let mut seen = Vec::new();
    for _ in 0..3 {
        layout.cycle_floating_follow();
        seen.push(layout.floating_follow());
    }
    assert_eq!(seen, [Parent, OnCanvas, Screen]);

    layout.set_floating_follow(OnCanvas);
    layout.update_config(&Config::default());
    assert_eq!(
        layout.floating_follow(),
        OnCanvas,
        "the config did not change it"
    );
    let parent = Config::parse_mem(
        r#"
        layout {
            canvas {
                floating-follow "parent"
            }
        }
        "#,
    )
    .unwrap();
    layout.update_config(&parent);
    assert_eq!(layout.floating_follow(), Parent, "the config changed it");
    layout.cycle_floating_follow();
    layout.update_config(&parent);
    assert_eq!(layout.floating_follow(), OnCanvas);
    layout.update_config(&Config::default());
    assert_eq!(layout.floating_follow(), Screen);
}

/// A fixed-size window floats at its size in the middle of the working area and leaves the
/// canvas as it was.
#[test]
fn canvas_fixed_size_window_floats_at_its_size() {
    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    check_canvas_ops_on(&mut layout, [add(0), add(1)]);
    let before = rects(&layout);
    let canvas_focus = focus_rect(&layout);

    let mut params = sized_params(2, 400, 300);
    params.min_max_size = (Size::from((400, 300)), Size::from((400, 300)));
    open_floating(&mut layout, params, None, ActivateWindow::Smart);

    assert_eq!(rects(&layout), before);
    assert_eq!(focus_rect(&layout), canvas_focus);
    assert_eq!(requested_size(&layout, 2), Some(Size::from((400, 300))));
    let rect = floating_rect(&layout, 2);
    assert_eq!(rect.size, Size::from((400., 300.)));
    let area = layout.active_workspace().unwrap().working_area();
    assert!(area.contains_rect(rect), "{rect:?} outside {area:?}");
}

/// An overlay such as Chromium's picture-in-picture, opened without asking for activation,
/// floats in the bottom right corner at the size it committed, follows the sizes it commits
/// later, and leaves the focus and the canvas as they were.
#[test]
fn canvas_overlay_opens_unfocused_in_the_corner_at_its_own_size() {
    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    check_canvas_ops_on(&mut layout, [add(0), add(1)]);
    let before = rects(&layout);
    let canvas_focus = focus_rect(&layout);
    assert_eq!(focus_id(&layout), Some(1));

    let gaps = layout.options.layout.gaps;
    let mut params = sized_params(2, 480, 270);
    params.min_max_size = (Size::from((240, 52)), Size::from((1142, 736)));
    let mut rules = ResolvedWindowRules::default();
    let activate = rules.open_activation(false, true, false);
    assert_eq!(activate, ActivateWindow::No);
    rules.default_floating_position = Some(overlay_position(gaps));
    params.rules = Some(rules);
    open_floating(&mut layout, params, None, activate);

    assert_eq!(focus_id(&layout), Some(1), "the overlay took the focus");
    assert!(!layout.active_workspace().unwrap().floating_is_active());
    assert_eq!(rects(&layout), before);
    assert_eq!(focus_rect(&layout), canvas_focus);
    assert_eq!(requested_size(&layout, 2), Some(Size::from((480, 270))));

    let area = layout.active_workspace().unwrap().working_area();
    let rect = floating_rect(&layout, 2);
    assert_eq!(rect.size, Size::from((480., 270.)));
    assert_close(
        rect.loc + rect.size.to_point(),
        area.loc + area.size.to_point() - Point::from((gaps, gaps)),
        "bottom right corner",
    );

    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::SetForcedSize {
                id: 2,
                size: Some(Size::from((640, 360))),
            }),
            op(Op::Communicate(2)),
        ],
    );
    assert_eq!(floating_rect(&layout, 2).size, Size::from((640., 360.)));
    assert_eq!(rects(&layout), before);
    assert_eq!(focus_id(&layout), Some(1));
}

/// A rule that opens a window floating and unfocused, as the user's picture-in-picture rule
/// does, leaves the focus on the tiled window too, token or not.
#[test]
fn canvas_open_focused_false_rule_keeps_the_focus() {
    let rules = ResolvedWindowRules {
        open_floating: Some(true),
        open_focused: Some(false),
        ..ResolvedWindowRules::default()
    };
    let activate = rules.open_activation(true, false, false);
    assert_eq!(activate, ActivateWindow::No);

    let mut layout = sized_canvas(1428, 920, USER_DEFAULT);
    check_canvas_ops_on(&mut layout, [add(0)]);
    let before = rects(&layout);
    let mut params = sized_params(1, 320, 180);
    params.rules = Some(rules);
    open_floating(&mut layout, params, None, activate);
    assert_eq!(focus_id(&layout), Some(0));
    assert_eq!(rects(&layout), before);
    assert_eq!(requested_size(&layout, 1), Some(Size::from((320, 180))));
}

#[test]
fn canvas_move_lets_the_neighbour_take_the_freed_tile_and_swap_neighbour_swaps() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            add(2),
            op(Op::FocusColumnLeft),
            op(Op::FocusColumnLeft),
            op(Op::MoveColumnRight),
        ],
    );
    assert_eq!(focus_rect(&layout), Rect::new(1, 0, 1, 1));
    let by_id = window_rects(&layout);
    assert_eq!(
        (by_id[&0], by_id[&1], by_id[&2]),
        (
            Rect::new(1, 0, 1, 1),
            Rect::new(0, 0, 1, 1),
            Rect::new(2, 0, 1, 1)
        ),
        "1 takes the tile 0 left (2 tiles reassigned) instead of the row being pushed (3)"
    );

    check_canvas_ops_on(&mut layout, [op(Op::MoveWindowDown)]);
    assert_eq!(focus_rect(&layout), Rect::new(1, 1, 1, 1));

    let before = rects(&layout);
    check_canvas_ops_on(&mut layout, [CanvasOp::SwapNeighbour(Dir::Up)]);
    assert_eq!(focus_rect(&layout), Rect::new(0, 0, 1, 1));
    assert_eq!(rects(&layout), before, "equal spans trade their tiles");
}

#[test]
fn canvas_closing_the_focus_returns_to_the_last_used_window() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            add(2),
            op(Op::FocusWindow(0)),
            op(Op::FocusWindow(2)),
            op(Op::CloseWindow(2)),
        ],
    );
    assert_eq!(
        layout.focus().map(|w| *w.id()),
        Some(0),
        "0 was used last; 1 is only nearer"
    );
}

#[test]
fn canvas_zoom_moves_the_camera_not_the_windows() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [op(Op::AddOutput(1)), add(0), add_min(1, 1000, 600), add(2)],
    );
    let sizes = requested_sizes(&layout);
    let before = rects(&layout);
    let zooms = [1, 1, 1, -1, -1, -1, -1, -1];
    check_canvas_ops_on(&mut layout, zooms.map(CanvasOp::Zoom));
    assert_eq!(rects(&layout), before);
    assert_eq!(requested_sizes(&layout), sizes);
    assert_eq!(
        layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .canvas()
            .zoom(),
        -2,
        "magnified past 1:1"
    );
    check_canvas_ops_on(
        &mut layout,
        [op(Op::AdvanceAnimations { msec_delta: 1000 })],
    );
    assert_eq!(requested_sizes(&layout), sizes);
}

/// The view point 10 px (at 1:1) inside the focused window's top-left corner, where the
/// screen shows it. Test windows keep their own size, so the middle of the cell can miss them.
fn focus_corner_on_screen(layout: &Layout<TestWindow>) -> Point<f64, Logical> {
    let canvas = layout.active_workspace().unwrap().scrolling().canvas();
    let (x, y, _, _) = canvas.screen_px(focus_rect(layout));
    let inside = (8. + 10.) * canvas.scale();
    Point::from((8. + x + inside, 8. + y + inside))
}

/// Four tiles in a row on a 1264x704 canvas, the last focused. Pulled back two steps, the camera
/// keeps the middle of the screen: 3 is drawn toward it, over where 2 is at 1:1.
#[test]
fn canvas_pointer_picks_the_window_drawn_under_it_while_zoomed() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [op(Op::AddOutput(1)), add(0), add(1), add(2), add(3)],
    );
    settle(&mut layout);
    check_canvas_ops_on(&mut layout, [CanvasOp::Zoom(2), op(Op::CompleteAnimations)]);
    for (_, win) in layout.windows() {
        win.0.has_input_region.set(true);
    }
    let output = layout.active_output().unwrap().clone();
    let pos = focus_corner_on_screen(&layout);
    let (win, hit) = layout.window_under(&output, pos).unwrap();
    assert_eq!(*win.id(), 3, "3 is drawn under {pos:?}");
    assert!(
        matches!(hit, HitType::Activate { .. }),
        "a zoomed window gets no pointer input: contents_under finds no surface"
    );

    check_canvas_ops_on(
        &mut layout,
        [CanvasOp::Zoom(-2), op(Op::CompleteAnimations)],
    );
    let (win, hit) = layout.window_under(&output, pos).unwrap();
    assert_eq!(*win.id(), 2, "at 1:1, {pos:?} is over 2");
    assert!(matches!(hit, HitType::Input { .. }));
    let pos = focus_corner_on_screen(&layout);
    let (win, hit) = layout.window_under(&output, pos).unwrap();
    assert_eq!(*win.id(), 3);
    assert!(matches!(hit, HitType::Input { .. }));
}

/// Pulling back two steps and coming closer again: every frame of the camera's animation keeps
/// the 1:1 point under the middle of the screen, and the view ends where it began.
#[test]
fn canvas_zoom_animates_about_the_middle_of_the_screen() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [op(Op::AddOutput(1)), add(0), add(1), add(2), add(3)],
    );
    settle(&mut layout);
    let space = |layout: &Layout<TestWindow>| {
        let space = layout.active_workspace().unwrap().scrolling();
        (space.camera().scale, space.drawn_middle())
    };
    let (_, middle) = space(&layout);
    let (vx, vy) = layout
        .active_workspace()
        .unwrap()
        .scrolling()
        .canvas()
        .view();
    for (zoom, steps) in [(2, 6), (-2, 6)] {
        check_canvas_ops_on(&mut layout, [CanvasOp::Zoom(zoom)]);
        let mut scales = Vec::new();
        for _ in 0..steps {
            let (scale, now) = space(&layout);
            scales.push(scale);
            assert!(
                (now.x - middle.x).abs() < 1. && (now.y - middle.y).abs() < 1.,
                "zoom {zoom} at camera scale {scale}: the middle shows {now:?}, not {middle:?}"
            );
            check_canvas_ops_on(&mut layout, [op(Op::AdvanceAnimations { msec_delta: 40 })]);
        }
        assert!(
            scales.windows(2).any(|w| w[0] != w[1]),
            "the camera animated: {scales:?}"
        );
        check_canvas_ops_on(&mut layout, [op(Op::CompleteAnimations)]);
    }
    let (x, y) = layout
        .active_workspace()
        .unwrap()
        .scrolling()
        .canvas()
        .view();
    assert!(
        (x - vx).abs() < 1e-6 && (y - vy).abs() < 1e-6,
        "pulled back and closer again: the view is ({x}, {y}), not ({vx}, {vy})"
    );
}

#[test]
fn canvas_mod_pan_moves_the_camera_one_to_one() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            add(2),
            CanvasOp::Zoom(2),
            op(Op::AdvanceAnimations { msec_delta: 1000 }),
        ],
    );
    let output = layout.active_output().unwrap().clone();
    let view = |layout: &Layout<TestWindow>| {
        layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .canvas()
            .view()
    };
    let (x, y) = view(&layout);
    let before = rects(&layout);
    layout.view_offset_gesture_begin(&output, None, false);
    let panned = layout.canvas_pan_update(-30., -12., Duration::from_millis(10), false);
    assert!(panned.is_some());
    assert_eq!(
        view(&layout),
        (x - 30., y - 12.),
        "the pointer moved 30x12 right and down on screen, and the content with it"
    );
    assert_eq!(rects(&layout), before, "a pan moves no window");
    layout.view_offset_gesture_end(Some(false));
    check_canvas(&layout, &canvases(&layout), false);
    assert_eq!(rects(&layout), before);
}

fn drag(dx: f64, dy: f64, px: f64, py: f64) -> CanvasOp {
    op(Op::InteractiveMoveUpdate {
        window: 0,
        dx,
        dy,
        output_idx: 1,
        px,
        py,
    })
}

#[test]
fn canvas_drag_follows_the_pointer_one_to_one_and_drops_where_it_is_drawn() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            op(Op::InteractiveMoveBegin {
                window: 0,
                output_idx: 1,
                px: 20.,
                py: 20.,
            }),
            drag(5., 0., 25., 20.),
        ],
    );
    let ws = layout.active_workspace().unwrap();
    let tile = ws
        .scrolling()
        .tiles()
        .find(|t| *t.window().id() == 0)
        .unwrap();
    assert_eq!(
        tile.interactive_move_offset,
        Point::from((5., 0.)),
        "no rubber band: the window follows the pointer 1:1"
    );
    check_canvas_ops_on(
        &mut layout,
        [drag(5., 0., 30., 20.), drag(500., 160., 530., 180.)],
    );
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 1, 1), Rect::new(1, 0, 1, 1)],
        "until the drop, 0 keeps its tile and 1 stays where it was"
    );
    check_canvas_ops_on(&mut layout, [op(Op::InteractiveMoveEnd { window: 0 })]);
    assert_eq!(
        rects(&layout),
        [Rect::new(1, 0, 1, 1), Rect::new(2, 1, 1, 1)],
        "0 landed on the tile nearest where it was drawn, (510, 160) on, and 1 stayed: a drop closes no gap"
    );
    assert_eq!(focus_rect(&layout), Rect::new(2, 1, 1, 1));
}

#[test]
fn canvas_drag_keeps_the_span() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            CanvasOp::ResizeEdge(Edge::Right, 1),
            CanvasOp::ResizeEdge(Edge::Bottom, 1),
            op(Op::InteractiveMoveBegin {
                window: 0,
                output_idx: 1,
                px: 200.,
                py: 150.,
            }),
            drag(250., 10., 450., 160.),
            drag(240., 0., 690., 160.),
        ],
    );
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 2, 2)],
        "a drag keeps the window on its tiles until the drop"
    );
    check_canvas_ops_on(&mut layout, [op(Op::InteractiveMoveEnd { window: 0 })]);
    assert_eq!(
        rects(&layout),
        [Rect::new(2, 0, 2, 2)],
        "dropped two tiles right, still two by two"
    );
}

#[test]
fn canvas_drop_on_its_own_tiles_changes_nothing() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            op(Op::InteractiveMoveBegin {
                window: 0,
                output_idx: 1,
                px: 20.,
                py: 20.,
            }),
            drag(100., 60., 120., 80.),
            drag(-90., -50., 30., 30.),
            op(Op::InteractiveMoveEnd { window: 0 }),
        ],
    );
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 1, 1), Rect::new(1, 0, 1, 1)]
    );
}

#[test]
fn canvas_swap_keeps_both_spans() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            CanvasOp::ResizeEdge(Edge::Right, 1),
            CanvasOp::ResizeEdge(Edge::Bottom, 1),
            add(1),
            op(Op::FocusColumnLeft),
            CanvasOp::SwapNeighbour(Dir::Right),
        ],
    );
    assert_eq!(
        (window_rects(&layout)[&0], window_rects(&layout)[&1]),
        (Rect::new(1, 0, 2, 2), Rect::new(0, 0, 1, 1)),
        "0 ends where 1 ended and 1 starts where 0 started, each at its own span"
    );
}

#[test]
fn canvas_window_keeps_its_span_on_another_workspace() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            CanvasOp::ResizeEdge(Edge::Right, 1),
            CanvasOp::ResizeEdge(Edge::Bottom, 1),
            op(Op::MoveWindowToWorkspaceDown(true)),
        ],
    );
    assert_eq!(window_rects(&layout)[&0].w, 2);
    assert_eq!(window_rects(&layout)[&0].h, 2);
}

fn forced(id: usize, w: i32, h: i32) -> CanvasOp {
    op(Op::SetForcedSize {
        id,
        size: Some(Size::from((w, h))),
    })
}

/// Lets every window answer the size asked of it, enough times for a refusal to grow its span
/// and the window to answer again.
fn settle(layout: &mut Layout<TestWindow>) {
    for _ in 0..3 {
        for id in 0..8 {
            check_canvas_ops_on(layout, [op(Op::Communicate(id))]);
        }
    }
}

/// The focused window, at the size it committed, is drawn whole within the working area.
#[track_caller]
fn assert_focus_drawn_whole(layout: &Layout<TestWindow>, path: &str) {
    let space = layout.active_workspace().unwrap().scrolling();
    let (rect, area) = space
        .focus_drawn()
        .unwrap_or_else(|| panic!("{path}: the focused window answered and fits on screen"));
    let far = rect.loc + rect.size.to_point();
    let area_far = area.loc + area.size.to_point();
    let eps = 1e-6;
    assert!(
        rect.loc.x >= area.loc.x - eps
            && rect.loc.y >= area.loc.y - eps
            && far.x <= area_far.x + eps
            && far.y <= area_far.y + eps,
        "{path}: focused window drawn at {rect:?}, outside {area:?}"
    );
}

#[test]
fn canvas_window_keeping_a_larger_size_takes_the_tiles_that_hold_it() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [op(Op::AddOutput(1)), add(0), add(1), forced(1, 600, 200)],
    );
    settle(&mut layout);
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 1, 1), Rect::new(1, 0, 3, 2)],
        "asked for 224x139, 1 kept 600x200: 600 + 17 px takes 3 tiles, 200 + 17 takes 2"
    );
    let tile = |layout: &Layout<TestWindow>| {
        let ws = layout.active_workspace().unwrap();
        let tile = ws
            .scrolling()
            .tiles()
            .find(|t| *t.window().id() == 1)
            .unwrap();
        (
            tile.window().size(),
            tile.window().requested_size().unwrap(),
        )
    };
    let (size, asked) = tile(&layout);
    assert_eq!(size, Size::from((600, 200)));
    assert_eq!(asked, Size::from((704, 294)), "its tile now holds it");
    assert_focus_drawn_whole(&layout, "refusal");

    check_canvas_ops_on(&mut layout, [CanvasOp::ResizeEdge(Edge::Right, -2)]);
    assert_eq!(
        focus_rect(&layout),
        Rect::new(1, 0, 3, 2),
        "below what it kept"
    );

    check_canvas_ops_on(&mut layout, [forced(1, 100, 100)]);
    settle(&mut layout);
    assert_eq!(
        focus_rect(&layout),
        Rect::new(1, 0, 3, 2),
        "it took less than it kept: no span changes by itself"
    );
    check_canvas_ops_on(&mut layout, [CanvasOp::ResizeEdge(Edge::Right, -2)]);
    assert_eq!(
        focus_rect(&layout),
        Rect::new(1, 0, 1, 2),
        "the minimum fell back to the declared one"
    );
}

#[test]
fn canvas_every_focus_path_shows_the_focused_window_whole() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            add(2),
            add(3),
            add(4),
            forced(2, 600, 400),
            forced(4, 500, 300),
        ],
    );
    settle(&mut layout);
    assert_focus_drawn_whole(&layout, "a new window maps with the focus");
    let output = layout.active_output().unwrap().clone();

    let paths: [(&str, Vec<CanvasOp>); 17] = [
        ("focus left", vec![op(Op::FocusColumnLeft)]),
        ("focus right", vec![op(Op::FocusColumnRight)]),
        ("focus first", vec![op(Op::FocusColumnFirst)]),
        ("focus last", vec![op(Op::FocusColumnLast)]),
        ("focus down", vec![op(Op::FocusWindowDown)]),
        ("focus up", vec![op(Op::FocusWindowUp)]),
        (
            "click, IPC, activation, Alt+Tab",
            vec![op(Op::FocusWindow(2))],
        ),
        ("click on 0", vec![op(Op::FocusWindow(0))]),
        (
            "interactive move drop",
            vec![
                op(Op::InteractiveMoveBegin {
                    window: 2,
                    output_idx: 1,
                    px: 20.,
                    py: 20.,
                }),
                op(Op::InteractiveMoveUpdate {
                    window: 2,
                    dx: 1000.,
                    dy: 400.,
                    output_idx: 1,
                    px: 1020.,
                    py: 420.,
                }),
                op(Op::InteractiveMoveEnd { window: 2 }),
            ],
        ),
        (
            "interactive resize",
            vec![
                op(Op::FocusWindow(4)),
                op(Op::InteractiveResizeBegin {
                    window: 4,
                    edges: ResizeEdge::RIGHT,
                }),
                op(Op::InteractiveResizeUpdate {
                    window: 4,
                    dx: 250.,
                    dy: 0.,
                }),
                op(Op::InteractiveResizeEnd { window: 4 }),
            ],
        ),
        ("keyboard grow", vec![CanvasOp::ResizeEdge(Edge::Right, 1)]),
        (
            "keyboard shrink",
            vec![CanvasOp::ResizeEdge(Edge::Right, -1)],
        ),
        ("zoom out", vec![CanvasOp::Zoom(1)]),
        ("click while zoomed", vec![op(Op::FocusWindow(1))]),
        ("zoom in", vec![CanvasOp::Zoom(-1)]),
        ("close returns by recency", vec![op(Op::CloseWindow(1))]),
        ("swap", vec![CanvasOp::SwapNeighbour(Dir::Right)]),
    ];
    for (path, ops) in paths {
        check_canvas_ops_on(&mut layout, ops);
        settle(&mut layout);
        assert_focus_drawn_whole(&layout, path);
    }

    layout.activate_window_without_raising(&0);
    settle(&mut layout);
    assert_focus_drawn_whole(&layout, "activate without raising");

    for is_touchpad in [true, false] {
        layout.view_offset_gesture_begin(&output, None, is_touchpad);
        layout.canvas_pan_update(700., 300., Duration::from_millis(10), is_touchpad);
        layout.view_offset_gesture_end(Some(is_touchpad));
        settle(&mut layout);
        assert_focus_drawn_whole(&layout, "the end of a pan");
    }

    check_canvas_ops_on(&mut layout, [op(Op::FocusWindow(4))]);
    let mut options = Options::default();
    options.layout.canvas.tile_width = 300.;
    options.layout.canvas.tile_height = 200.;
    layout.update_options(options);
    settle(&mut layout);
    assert_focus_drawn_whole(&layout, "a config reload");

    output.change_current_state(
        Some(Mode {
            size: Size::from((1600, 900)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    layout.update_output_size(&output);
    settle(&mut layout);
    assert_focus_drawn_whole(&layout, "an output resize");
}

/// Each tile's drawn rectangle in the view: zoom 0 draws tiles 1:1.
fn drawn_tiles(layout: &Layout<TestWindow>) -> HashMap<usize, Rectangle<f64, Logical>> {
    let ws = layout.active_workspace().unwrap();
    ws.tiles_with_render_positions()
        .map(|(tile, pos, _)| {
            (
                *tile.window().id(),
                Rectangle::new(pos, tile.animated_tile_size()),
            )
        })
        .collect()
}

#[track_caller]
fn assert_no_drawn_overlap(layout: &Layout<TestWindow>, when: &str) {
    let tiles: Vec<_> = drawn_tiles(layout).into_iter().collect();
    for (a, (ia, ra)) in tiles.iter().enumerate() {
        for (ib, rb) in &tiles[a + 1..] {
            let overlap = ra
                .intersection(*rb)
                .filter(|r| r.size.w > 1e-6 && r.size.h > 1e-6);
            assert!(
                overlap.is_none(),
                "{when}: {ia} at {ra:?} and {ib} at {rb:?} are drawn over each other"
            );
        }
    }
}

/// 0 is two tiles wide and settled, 1 sits right of it; 0 is focused.
fn wide_then_neighbour(options: Options) -> Layout<TestWindow> {
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            op(Op::FocusWindow(0)),
            CanvasOp::ResizeEdge(Edge::Right, 1),
            op(Op::Communicate(0)),
            op(Op::Communicate(1)),
            op(Op::CompleteAnimations),
        ],
    );
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 2, 1), Rect::new(2, 0, 1, 1)]
    );
    layout
}

fn small_tile_options() -> Options {
    let mut options = Options::default();
    options.layout.canvas.tile_width = 240.;
    options.layout.canvas.tile_height = 155.;
    options
}

#[test]
fn canvas_shrink_moves_the_neighbours_only_after_the_window_shrank() {
    let mut layout = wide_then_neighbour(small_tile_options());
    let before = drawn_tiles(&layout);

    check_canvas_ops_on(&mut layout, [CanvasOp::ResizeEdge(Edge::Right, -1)]);
    assert_eq!(
        rects(&layout),
        [Rect::new(0, 0, 1, 1), Rect::new(1, 0, 1, 1)],
        "the model closed the lake at once"
    );
    check_canvas_ops_on(&mut layout, [op(Op::AdvanceAnimations { msec_delta: 100 })]);
    assert_eq!(
        drawn_tiles(&layout),
        before,
        "until 0 commits its smaller size, nothing moves on screen"
    );
    assert_no_drawn_overlap(&layout, "before the commit");

    check_canvas_ops_on(&mut layout, [op(Op::Communicate(0))]);
    for step in 0..20 {
        assert_no_drawn_overlap(&layout, &format!("{step} frames after the commit"));
        check_canvas_ops_on(&mut layout, [op(Op::AdvanceAnimations { msec_delta: 16 })]);
    }
    check_canvas_ops_on(&mut layout, [op(Op::CompleteAnimations)]);
    let after = drawn_tiles(&layout);
    assert!(
        after[&1].loc.x < before[&1].loc.x,
        "1 moved into the freed tile: {:?} -> {:?}",
        before[&1],
        after[&1]
    );
    assert_no_drawn_overlap(&layout, "settled");
}

#[test]
fn canvas_shrink_stops_waiting_for_a_frozen_app() {
    let mut layout = wide_then_neighbour(small_tile_options());
    let before = drawn_tiles(&layout);
    check_canvas_ops_on(
        &mut layout,
        [
            CanvasOp::ResizeEdge(Edge::Right, -1),
            op(Op::AdvanceAnimations { msec_delta: 250 }),
        ],
    );
    assert_eq!(drawn_tiles(&layout), before, "still waiting");
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AdvanceAnimations { msec_delta: 100 }),
            op(Op::CompleteAnimations),
        ],
    );
    assert!(
        drawn_tiles(&layout)[&1].loc.x < before[&1].loc.x,
        "past the time limit, 1 moves although 0 never answered"
    );
}

#[test]
fn canvas_grow_never_draws_over_the_windows_it_pushes() {
    const FAST: Kind = Kind::Easing(EasingParams {
        duration_ms: 50,
        curve: Curve::Linear,
    });
    const SLOW: Kind = Kind::Easing(EasingParams {
        duration_ms: 1000,
        curve: Curve::Linear,
    });
    let mut options = small_tile_options();
    options.animations.window_resize.anim.kind = FAST;
    options.animations.window_movement.0.kind = SLOW;
    let mut layout = wide_then_neighbour(options);

    check_canvas_ops_on(
        &mut layout,
        [
            CanvasOp::ResizeEdge(Edge::Right, 1),
            op(Op::AdvanceAnimations { msec_delta: 16 }),
            op(Op::Communicate(0)),
        ],
    );
    for step in 0..10 {
        assert_no_drawn_overlap(&layout, &format!("{step} frames into the grow"));
        check_canvas_ops_on(&mut layout, [op(Op::AdvanceAnimations { msec_delta: 10 })]);
    }
}

#[test]
fn canvas_overview_display_area_is_the_screen_at_overview_scale() {
    let mut layout = placed(&row(7));
    check_canvas_ops_on(
        &mut layout,
        [op(Op::FocusWindow(0)), op(Op::CompleteAnimations)],
    );
    let output = layout.active_output().unwrap().clone();
    assert_eq!(
        layout
            .monitor_for_output(&output)
            .unwrap()
            .overview_display_area(),
        None,
        "no overview, no dim"
    );
    check_canvas_ops_on(
        &mut layout,
        [op(Op::ToggleOverview), op(Op::CompleteAnimations)],
    );
    let mon = layout.monitor_for_output(&output).unwrap();
    let zoom = mon.overview_zoom();
    let area = mon.overview_display_area().unwrap();
    let view = Size::from((1280., 720.));
    assert_eq!(area.size, view.upscale(zoom));
    assert_eq!(
        area.loc,
        (view.to_point() - area.size.to_point()).downscale(2.),
        "the active workspace's view, centred at overview scale"
    );

    let screen = Rectangle::from_size(view);
    let mut off_screen = 0;
    for (tile, pos, _) in mon.active_workspace_ref().tiles_with_render_positions() {
        let shown = Rectangle::new(pos, tile.tile_size());
        let in_overview =
            Rectangle::new(area.loc + pos.upscale(zoom), tile.tile_size().upscale(zoom));
        if screen.contains_rect(shown) {
            assert!(area.contains_rect(in_overview), "{shown:?} is on screen");
        } else if shown.intersection(screen).is_none() {
            off_screen += 1;
            assert!(
                in_overview.intersection(area).is_none(),
                "{shown:?} is off screen, so outside the display area"
            );
        }
    }
    assert!(off_screen > 0, "some window lies off screen");

    let bands = crate::layout::monitor::dim_bands(view, area);
    let dimmed: f64 = bands.iter().map(|b| b.size.w * b.size.h).sum();
    assert_eq!(dimmed + area.size.w * area.size.h, view.w * view.h);
    for band in bands {
        assert!(
            band.intersection(area).is_none(),
            "{band:?} dims the display area"
        );
    }

    let off = Rectangle::new(Point::from((-100., 600.)), Size::from((400., 400.)));
    let bands = crate::layout::monitor::dim_bands(view, off);
    assert_eq!(
        bands[0],
        Rectangle::new(Point::from((0., 0.)), Size::from((1280., 600.)))
    );
    assert_eq!(bands[1].size.h, 0., "the hole reaches the bottom edge");
    assert_eq!(bands[2].size.w, 0., "and the left edge");
    assert_eq!(
        bands[3],
        Rectangle::new(Point::from((300., 600.)), Size::from((980., 120.)))
    );
}

/// Where each window of the active canvas shows on the output, with whether it shows within
/// `inner`, from where its tile renders through `to_output`.
fn on_output(
    layout: &Layout<TestWindow>,
    output: &Output,
    to_output: impl Fn(Rectangle<f64, Logical>) -> Rectangle<f64, Logical>,
) -> Vec<(usize, Rectangle<f64, Logical>)> {
    let mon = layout.monitor_for_output(output).unwrap();
    mon.active_workspace_ref()
        .tiles_with_render_positions()
        .map(|(tile, pos, _)| {
            let id = *tile.window().id();
            (id, to_output(Rectangle::new(pos, tile.tile_size())))
        })
        .collect()
}

/// Checks that the windows drawn are those on the output, and returns those on it but outside
/// `inner`, where the 1:1 view would not draw them, with their rectangles on the output.
#[track_caller]
fn assert_drawn_on_output(
    layout: &Layout<TestWindow>,
    output: &Output,
    rects: &[(usize, Rectangle<f64, Logical>)],
    inner: Rectangle<f64, Logical>,
) -> Vec<(usize, Rectangle<f64, Logical>)> {
    let screen = Rectangle::from_size(Size::from((1280., 720.)));
    let drawn = layout
        .monitor_for_output(output)
        .unwrap()
        .drawn_canvas_windows();
    let mut beyond = Vec::new();
    let mut off = 0;
    for &(id, rect) in rects {
        if rect.overlaps(screen) {
            assert!(drawn.contains(&id), "{id} at {rect:?} is on the output");
            if !rect.overlaps(inner) {
                beyond.push((id, rect));
            }
        } else {
            off += 1;
            assert!(!drawn.contains(&id), "{id} at {rect:?} is off the output");
        }
    }
    assert!(off > 0, "some window lies off the output: {rects:?}");
    beyond
}

/// Twelve windows in a row and one moved five tiles down, the first focused: at 1:1 the screen
/// shows only the first five. The overview draws every window that falls on the output at
/// overview scale, also those outside the display area (dimmed there), and none off the output.
/// The pointer picks one beside and one below the display area, and picking one focuses it and
/// shows it whole once the overview closes.
#[test]
fn canvas_overview_draws_every_window_on_the_output() {
    let mut layout = placed(&row(12));
    let mut ops = vec![op(Op::FocusWindow(2))];
    ops.extend((0..5).map(|_| op(Op::MoveWindowDown)));
    ops.extend([op(Op::FocusWindow(0)), op(Op::CompleteAnimations)]);
    check_canvas_ops_on(&mut layout, ops);
    settle(&mut layout);
    let output = layout.active_output().unwrap().clone();
    let screen = Rectangle::from_size(Size::from((1280., 720.)));

    let rects = on_output(&layout, &output, |r| r);
    assert_drawn_on_output(&layout, &output, &rects, screen);

    check_canvas_ops_on(
        &mut layout,
        [op(Op::ToggleOverview), op(Op::CompleteAnimations)],
    );
    let mon = layout.monitor_for_output(&output).unwrap();
    let zoom = mon.overview_zoom();
    let area = mon.overview_display_area().unwrap();
    let rects = on_output(&layout, &output, |r| {
        Rectangle::new(area.loc + r.loc.upscale(zoom), r.size.upscale(zoom))
    });
    let beyond = assert_drawn_on_output(&layout, &output, &rects, area);

    let middle = |r: Rectangle<f64, Logical>| r.loc + r.size.downscale(2.).to_point();
    let rows = area.loc.y..area.loc.y + area.size.h;
    let beside = beyond
        .iter()
        .find(|(_, r)| screen.contains_rect(*r) && rows.contains(&middle(*r).y))
        .expect("a window beside the display area, whole on the output");
    let below = beyond
        .iter()
        .find(|(_, r)| screen.contains_rect(*r) && middle(*r).y > rows.end)
        .expect("a window below the display area, whole on the output");
    assert_eq!(below.0, 2);
    for &(id, rect) in [beside, below] {
        let (win, hit) = layout.window_under(&output, middle(rect)).unwrap();
        assert_eq!(*win.id(), id, "the pointer picks {id} at {rect:?}");
        assert!(matches!(hit, HitType::Activate { .. }));
    }

    layout.toggle_overview_to_workspace(0);
    layout.activate_window(&2);
    check_canvas_ops_on(&mut layout, [op(Op::CompleteAnimations)]);
    settle(&mut layout);
    assert!(!layout.is_overview_open());
    assert_eq!(layout.focus().map(|w| *w.id()), Some(2));
    assert_focus_drawn_whole(&layout, "picked below the display area in the overview");
}

/// Pulled back two steps outside the overview, the camera draws every window on screen at its
/// scale, also those outside the 1:1 view, and the pointer picks them; none off screen is drawn.
#[test]
fn canvas_zoomed_out_camera_draws_every_window_on_screen() {
    let mut layout = placed(&row(12));
    let mut ops = Vec::new();
    ops.extend([
        op(Op::FocusWindow(0)),
        op(Op::CompleteAnimations),
        CanvasOp::Zoom(2),
        op(Op::CompleteAnimations),
    ]);
    check_canvas_ops_on(&mut layout, ops);
    settle(&mut layout);
    let output = layout.active_output().unwrap().clone();
    let screen = Rectangle::from_size(Size::from((1280., 720.)));

    let space_rects: Vec<_> = {
        let ws = layout.active_workspace().unwrap();
        ws.tiles_with_render_positions()
            .map(|(tile, pos, _)| (*tile.window().id(), Rectangle::new(pos, tile.tile_size())))
            .collect()
    };
    let space = layout.active_workspace().unwrap().scrolling();
    let at_one: Vec<_> = space_rects
        .iter()
        .filter(|(_, r)| r.overlaps(screen))
        .map(|(id, _)| *id)
        .collect();
    let rects: Vec<_> = space_rects
        .iter()
        .map(|&(id, r)| (id, space.tile_camera(&id).rect_to_view(r)))
        .collect();
    assert_drawn_on_output(&layout, &output, &rects, screen);
    let (id, rect) = rects
        .iter()
        .find(|(id, r)| !at_one.contains(id) && screen.contains_rect(*r))
        .copied()
        .expect("a window outside the 1:1 view drawn whole on screen");

    let middle = rect.loc + rect.size.downscale(2.).to_point();
    let (win, hit) = layout.window_under(&output, middle).unwrap();
    assert_eq!(*win.id(), id, "the pointer picks {id} at {rect:?}");
    assert!(matches!(hit, HitType::Activate { .. }));
}

/// Window 0's hit a third of the way into where it is drawn, `zoom` steps from 1:1, with
/// `magnified-input` on or off: the hit, the view point, and that point mapped back through the
/// camera less the window buffer's place on the canvas, the point in the window's own pixels.
fn magnified_hit(
    zoom: i32,
    magnified_input: bool,
) -> (HitType, Point<f64, Logical>, Point<f64, Logical>) {
    let mut layout = placed(&row(3));
    let mut options = small_tile_options();
    options.layout.canvas.magnified_input = magnified_input;
    layout.update_options(options);
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::FocusWindow(0)),
            op(Op::CompleteAnimations),
            CanvasOp::Zoom(zoom),
            op(Op::CompleteAnimations),
        ],
    );
    settle(&mut layout);
    for (_, win) in layout.windows() {
        win.0.has_input_region.set(true);
    }
    let output = layout.active_output().unwrap().clone();
    let ws = layout.active_workspace().unwrap();
    let (tile, tile_pos) = ws
        .tiles_with_render_positions()
        .find(|(tile, _, _)| *tile.window().id() == 0)
        .map(|(tile, pos, _)| (tile, pos))
        .unwrap();
    let camera = ws.scrolling().tile_camera(&0);
    let drawn = camera.rect_to_view(Rectangle::new(tile_pos, tile.tile_size()));
    let view = drawn.loc + drawn.size.downscale(3.).to_point();
    let in_window = camera.from_view(view) - (tile_pos + tile.buf_loc());
    let (win, hit) = layout.window_under(&output, view).unwrap();
    assert_eq!(*win.id(), 0, "the pointer is over window 0 at {view:?}");
    (hit, view, in_window)
}

/// Zoomed in with `magnified-input`, the pointer reaches the window under it: the hit is input,
/// and the point the pointer code sends the window (the view point less `win_pos`) is the view
/// point mapped back through the camera, so a click lands where the window is drawn. Without
/// the option, or zoomed out, a hit only picks the window.
#[test]
fn canvas_magnified_input_reaches_the_window_where_it_is_drawn() {
    let (hit, view, in_window) = magnified_hit(-1, true);
    let HitType::Input { win_pos } = hit else {
        panic!("a magnified hit with magnified-input is input, not {hit:?}");
    };
    let sent = view - win_pos;
    assert!(
        (sent.x - in_window.x).abs() < 1e-6 && (sent.y - in_window.y).abs() < 1e-6,
        "the window gets {sent:?}, drawn there is its {in_window:?}"
    );
    assert!(
        (view - in_window).x.abs() > 1.,
        "the camera magnifies, so the view point is not the window point"
    );

    let (hit, _, _) = magnified_hit(-1, false);
    assert!(matches!(hit, HitType::Activate { .. }), "off: {hit:?}");
    let (hit, _, _) = magnified_hit(1, true);
    assert!(
        matches!(hit, HitType::Activate { .. }),
        "zoomed out: {hit:?}"
    );

    let (hit, view, in_window) = magnified_hit(0, true);
    let HitType::Input { win_pos } = hit else {
        panic!("a 1:1 hit is input, not {hit:?}");
    };
    let sent = view - win_pos;
    assert!((sent.x - in_window.x).abs() < 1e-6 && (sent.y - in_window.y).abs() < 1e-6);
}

fn active_canvas(layout: &Layout<TestWindow>) -> &Canvas {
    layout.active_workspace().unwrap().scrolling().canvas()
}

/// The span of tiles every window lies in.
fn bounds(canvas: &Canvas) -> Rect {
    let rects = canvas.windows().iter().map(|w| w.rect);
    let i = rects.clone().map(|r| r.i).min().unwrap();
    let j = rects.clone().map(|r| r.j).min().unwrap();
    let right = rects.clone().map(|r| r.right()).max().unwrap();
    let bottom = rects.map(|r| r.bottom()).max().unwrap();
    Rect::new(i, j, right - i, bottom - j)
}

#[track_caller]
fn assert_every_window_on_screen(layout: &Layout<TestWindow>) {
    let canvas = active_canvas(layout);
    for w in canvas.windows() {
        assert!(canvas.is_visible(w.rect), "{:?} is off screen", w.rect);
    }
}

/// The mod key and four fingers: the camera pulls back to show every window, keeps them all on
/// screen while the focused window moves tile by tile, and returns to its zoom when the mod key
/// is released.
#[test]
fn canvas_zoom_to_fit_follows_the_moves_and_zooms_back() {
    let mut layout = placed(&row(9));
    let mut ops = Vec::new();
    ops.extend([
        op(Op::FocusWindow(0)),
        CanvasOp::Zoom(-1),
        op(Op::CompleteAnimations),
    ]);
    check_canvas_ops_on(&mut layout, ops);
    let canvas = active_canvas(&layout);
    assert_eq!(bounds(canvas), Rect::new(0, 0, 9, 1));
    assert!(
        !canvas.is_visible(bounds(canvas)),
        "magnified, the row does not fit"
    );

    check_canvas_ops_on(&mut layout, [CanvasOp::ZoomToFit]);
    assert_eq!(
        active_canvas(&layout).zoom(),
        3,
        "nine 240 px tiles fit in 1264 px from 0.8^3 on"
    );
    assert_every_window_on_screen(&layout);

    let mut at = focus_rect(&layout);
    for (dir, zoom) in [
        (Dir::Left, 3),
        (Dir::Left, 4),
        (Dir::Down, 4),
        (Dir::Left, 4),
    ] {
        check_canvas_ops_on(&mut layout, [CanvasOp::MoveFocus(dir), CanvasOp::ZoomToFit]);
        let (di, dj) = dir.delta();
        at = Rect::new(at.i + di, at.j + dj, at.w, at.h);
        assert_eq!(focus_rect(&layout), at, "one tile {dir:?}");
        assert_eq!(
            active_canvas(&layout).zoom(),
            zoom,
            "after {dir:?} to {at:?}"
        );
        assert_every_window_on_screen(&layout);
    }

    check_canvas_ops_on(
        &mut layout,
        [CanvasOp::MoveFocus(Dir::Left), CanvasOp::ZoomToFit],
    );
    let canvas = active_canvas(&layout);
    assert_eq!(bounds(canvas).w, 13);
    assert_eq!(canvas.zoom(), 4, "capped at zoom-max");
    assert!(canvas.is_visible(focus_rect(&layout)));

    check_canvas_ops_on(&mut layout, [CanvasOp::ZoomBack]);
    let canvas = active_canvas(&layout);
    assert_eq!(canvas.zoom(), -1, "back to the zoom before the swipe");
    assert!(canvas.is_visible(focus_rect(&layout)));
    assert!(!layout.canvas_zoom_back(), "it zooms back once");
}

#[test]
fn canvas_zoom_to_fit_leaves_a_canvas_that_fits() {
    let mut layout = small_tiles();
    check_canvas_ops_on(&mut layout, [op(Op::AddOutput(1)), add(0), add(1), add(2)]);
    let before = active_canvas(&layout).clone();
    check_canvas_ops_on(&mut layout, [CanvasOp::ZoomToFit]);
    let canvas = active_canvas(&layout);
    assert_eq!(
        (canvas.zoom(), canvas.view()),
        (before.zoom(), before.view())
    );
    assert!(layout.canvas_zoom_back());
    assert_eq!(active_canvas(&layout).zoom(), 0);
}

#[test]
fn canvas_switcher_confirm_can_centre_the_window() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            add(2),
            add(3),
            add(4),
            add(5),
            add(6),
            op(Op::FocusWindow(0)),
        ],
    );
    layout.canvas_center_window(&4);
    check_canvas_ops_on(&mut layout, [op(Op::FocusWindow(4))]);
    assert_eq!(layout.focus().map(|w| *w.id()), Some(4));
    let canvas = layout.active_workspace().unwrap().scrolling().canvas();
    let (x, y, w, h) = canvas.screen_px(focus_rect(&layout));
    let (sw, sh) = canvas.screen();
    assert!(
        (x + w / 2. - sw / 2.).abs() < 1e-6 && (y + h / 2. - sh / 2.).abs() < 1e-6,
        "4 is centred on screen, and the focus that follows keeps it there"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: if std::env::var_os("RUN_SLOW_TESTS").is_none() { 64 } else { 2048 },
        ..ProptestConfig::default()
    })]

    #[test]
    fn canvas_random_operations(
        ops in proptest::collection::vec(arbitrary_canvas_op(), 1..80)
    ) {
        let mut ops = ops;
        ops.insert(0, op(Op::AddOutput(1)));
        check_canvas_ops(ops);
    }

    /// With `one-workspace`, any run of operations keeps one workspace on every output that has
    /// the flag (verify_invariants checks it); a reload in the run may turn it off or on.
    #[test]
    fn canvas_random_operations_one_workspace(
        ops in proptest::collection::vec(arbitrary_canvas_op(), 1..80)
    ) {
        let mut ops = ops;
        ops.insert(0, op(Op::AddOutput(1)));
        let mut layout = one_workspace_layout();
        check_canvas_ops_on(&mut layout, ops);
    }
}

/// How many islands the active canvas's windows make.
fn island_count(layout: &Layout<TestWindow>) -> usize {
    let label = newm_canvas::place::islands(active_canvas(layout).windows(), &[]);
    label
        .values()
        .collect::<std::collections::HashSet<_>>()
        .len()
}

/// The gather-islands action (Super+I): two islands become one, every window keeping its span,
/// the op is logged as a gather, and undo puts the windows back.
#[test]
fn canvas_gather_islands_makes_one_island() {
    let mut layout = placed(&[
        Rect::new(0, 0, 1, 1),
        Rect::new(1, 0, 1, 1),
        Rect::new(5, 0, 1, 1),
        Rect::new(6, 0, 1, 1),
        Rect::new(7, 0, 1, 1),
    ]);
    assert_eq!(island_count(&layout), 2);
    let before = rects(&layout);
    #[cfg(feature = "telemetry")]
    let skip = usage_lines(&layout, 0).len();
    check_canvas_ops_on(&mut layout, [CanvasOp::GatherIslands]);
    assert_eq!(island_count(&layout), 1, "{:?}", rects(&layout));
    let gathered: Vec<Rect> = (3..8).map(|i| Rect::new(i, 0, 1, 1)).collect();
    assert_eq!(rects(&layout), gathered, "0 and 1 slide right to 2");
    #[cfg(feature = "telemetry")]
    assert!(
        usage_lines(&layout, skip)
            .iter()
            .any(|l| l["op"] == "gather"),
        "the gather is logged"
    );
    assert!(layout.canvas_undo());
    assert_eq!(rects(&layout), before, "undo puts the islands back apart");
}

/// The usage lines the active canvas wrote, from the `skip`th on.
#[cfg(feature = "telemetry")]
fn usage_lines(layout: &Layout<TestWindow>, skip: usize) -> Vec<serde_json::Value> {
    let lines = layout.active_workspace().unwrap().scrolling().usage_lines();
    lines[skip.min(lines.len())..].to_vec()
}

#[cfg(feature = "telemetry")]
fn frames(layout: &mut Layout<TestWindow>, n: usize) {
    for _ in 0..n {
        check_canvas_ops_on(layout, [op(Op::AdvanceAnimations { msec_delta: 16 })]);
    }
}

#[cfg(feature = "telemetry")]
fn motion_of<'a>(lines: &'a [serde_json::Value], kind: &str) -> &'a serde_json::Value {
    lines
        .iter()
        .find(|l| l["op"] == "motion" && l["kind"] == kind)
        .unwrap_or_else(|| panic!("no {kind} motion in {lines:#?}"))
}

#[cfg(feature = "telemetry")]
fn num(v: &serde_json::Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("{v} is not a number"))
}

#[cfg(feature = "telemetry")]
#[test]
fn canvas_logs_the_motion_an_op_draws_once_it_settles() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            add(2),
            op(Op::FocusColumnLeft),
            op(Op::FocusColumnLeft),
            op(Op::CompleteAnimations),
        ],
    );
    frames(&mut layout, 2);
    let skip = usage_lines(&layout, 0).len();

    check_canvas_ops_on(&mut layout, [op(Op::MoveColumnRight)]);
    let lines = usage_lines(&layout, skip);
    let mv = lines
        .iter()
        .find(|l| l["op"] == "move")
        .expect("the move is logged");
    let seq = mv["seq"].as_u64().unwrap();
    let moved = mv["moved"].as_array().unwrap();
    assert_eq!(moved.len(), 2, "0 moves right and 1 takes its tile: {mv:#}");
    for w in moved {
        assert_eq!(num(&w["d_tiles"][2]), 1., "{w}");
        assert!(num(&w["d_px"][2]) > 200., "a tile is 240 px wide: {w}");
    }
    assert_eq!(mv["camera"]["jump"].as_array().unwrap().len(), 3, "{mv:#}");
    assert!(
        !lines.iter().any(|l| l["op"] == "motion" && l["seq"] == seq),
        "the motion is logged once it settles, not when it begins"
    );

    frames(&mut layout, 100);
    let lines = usage_lines(&layout, skip);
    let motion = lines
        .iter()
        .find(|l| l["op"] == "motion" && l["seq"] == seq)
        .unwrap_or_else(|| panic!("no motion for the move in {lines:#?}"));
    assert_eq!(motion["kind"], "move");
    assert_eq!(motion["cut"], false, "{motion:#}");
    assert!(num(&motion["frames"]) > 5., "{motion:#}");
    assert!(num(&motion["ms"]) > 80., "{motion:#}");
    let windows = &motion["windows"];
    assert_eq!(windows["moved"], 2, "{motion:#}");
    let path = num(&windows["max_path"]["v"]);
    assert!(
        path > 200. && path < 300.,
        "a glide one tile over: {motion:#}"
    );
    assert!(num(&windows["max_speed"]["v"]) > 0., "{motion:#}");
    assert!(num(&windows["max_accel"]["v"]) > 0., "{motion:#}");
    assert!(
        num(&windows["max_jump"]["v"]) < path / 2.,
        "a glide, not a jump: {motion:#}"
    );

    let after = usage_lines(&layout, 0).len();
    frames(&mut layout, 10);
    assert_eq!(
        usage_lines(&layout, 0).len(),
        after,
        "a settled canvas logs nothing per frame"
    );
}

#[cfg(feature = "telemetry")]
#[test]
fn canvas_logs_the_motion_of_a_pan_and_a_zoom() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            add(2),
            op(Op::CompleteAnimations),
        ],
    );
    frames(&mut layout, 2);
    let skip = usage_lines(&layout, 0).len();
    let output = layout.active_output().unwrap().clone();

    layout.view_offset_gesture_begin(&output, None, false);
    for k in 1..=5 {
        layout.canvas_pan_update(-30., -12., Duration::from_millis(16 * k), false);
        frames(&mut layout, 1);
    }
    assert!(
        !usage_lines(&layout, skip)
            .iter()
            .any(|l| l["op"] == "motion"),
        "a pan's episode lasts while the fingers are on"
    );
    layout.view_offset_gesture_end(Some(false));
    // The glide lasts until it is slower than 1.5 px/s: about 2.4 s from 2,000 px/s.
    frames(&mut layout, 200);
    let lines = usage_lines(&layout, skip);
    let pan = motion_of(&lines, "pan-pointer");
    let step = 30f64.hypot(12.);
    assert!(num(&pan["camera"]["path"]) >= 5. * step - 1., "{pan:#}");
    assert!(num(&pan["camera"]["max_jump"]) >= step - 1., "{pan:#}");
    assert!(
        num(&pan["camera"]["path"]) > 10. * step,
        "letting go carries the view on: {pan:#}"
    );
    assert!(
        num(&pan["camera"]["max_jump"]) < 1.5 * step,
        "the fling glides from where the fingers left, not in one frame: {pan:#}"
    );
    assert!(
        num(&pan["camera"]["max_speed"]) < 1.5 * step / 0.016,
        "and slows from the fingers' speed, never faster: {pan:#}"
    );
    assert_eq!(pan["windows"]["moved"], 0, "a pan moves no window: {pan:#}");

    let skip = usage_lines(&layout, 0).len();
    check_canvas_ops_on(&mut layout, [CanvasOp::Zoom(2)]);
    frames(&mut layout, 100);
    let lines = usage_lines(&layout, skip);
    let zoom = motion_of(&lines, "zoom");
    let scale = zoom["scale"].as_array().unwrap();
    assert!(
        num(&scale[0]) < num(&scale[1]),
        "the camera's scale moved: {zoom:#}"
    );
    assert!(num(&zoom["camera"]["path"]) > 0., "{zoom:#}");
    assert!(num(&zoom["camera"]["max_speed"]) > 0., "{zoom:#}");
    assert!(num(&zoom["camera_zoomed"]["path"]) > 0., "{zoom:#}");
}

#[cfg(feature = "telemetry")]
#[test]
fn canvas_drag_preview_is_one_motion_episode_until_the_drop() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            add(2),
            op(Op::FocusColumnLeft),
            op(Op::FocusColumnLeft),
            op(Op::CompleteAnimations),
        ],
    );
    frames(&mut layout, 2);
    let skip = usage_lines(&layout, 0).len();
    check_canvas_ops_on(
        &mut layout,
        [op(Op::InteractiveMoveBegin {
            window: 0,
            output_idx: 1,
            px: 100.,
            py: 50.,
        })],
    );
    for k in 1..=4 {
        check_canvas_ops_on(&mut layout, [drag(50., 0., 100. + 50. * f64::from(k), 50.)]);
        frames(&mut layout, 10);
    }
    assert!(
        !usage_lines(&layout, skip)
            .iter()
            .any(|l| l["op"] == "motion"),
        "the preview's episode lasts while the drag is on"
    );
    check_canvas_ops_on(&mut layout, [op(Op::InteractiveMoveEnd { window: 0 })]);
    frames(&mut layout, 100);
    let lines = usage_lines(&layout, skip);
    let preview = motion_of(&lines, "drag");
    assert!(
        num(&preview["windows"]["moved"]) >= 1.,
        "the preview moved 1 out of the way: {preview:#}"
    );
    let drop = lines
        .iter()
        .position(|l| l["op"] == "drop")
        .expect("the drop is logged");
    let seq = &lines[drop]["seq"];
    assert!(
        lines[..drop].iter().any(|l| l["kind"] == "drag"),
        "the drop ends the preview's episode"
    );
    let motion = lines[drop..]
        .iter()
        .find(|l| l["op"] == "motion" && &l["seq"] == seq)
        .unwrap_or_else(|| panic!("no motion for the drop in {lines:#?}"));
    assert_eq!(motion["kind"], "drop");
}

fn one_workspace_options() -> Options {
    let mut options = Options::default();
    options.layout.canvas.one_workspace = true;
    options
}

fn one_workspace_layout() -> Layout<TestWindow> {
    Layout::with_options(Clock::with_time(Duration::ZERO), one_workspace_options())
}

/// A layout part that turns `one-workspace` on or off.
fn one_workspace_part(on: bool) -> CanvasOp {
    op(Op::UpdateConfig {
        layout_config: Box::new(niri_config::LayoutPart {
            canvas: Some(niri_config::canvas::CanvasPart {
                one_workspace: Some(Flag(on)),
                ..Default::default()
            }),
            ..Default::default()
        }),
    })
}

/// canvas_random_operations found it: a workspace-switch gesture begun on the one workspace there
/// is, then a reload turning `one-workspace` on, left the gesture running (merge_workspaces
/// returned before ending it). check_canvas_ops verifies the invariants after each op.
#[test]
fn turning_one_workspace_on_ends_a_workspace_switch_gesture() {
    let layout = check_canvas_ops([
        op(Op::AddOutput(1)),
        op(Op::WorkspaceSwitchGestureBegin {
            output_idx: 1,
            is_touchpad: false,
        }),
        one_workspace_part(true),
    ]);
    assert!(layout.monitors().all(|mon| mon.workspace_switch.is_none()));
}

type WorkspaceMap = Vec<(
    String,
    usize,
    Vec<(WorkspaceId, Vec<usize>, Option<String>)>,
)>;

/// Every output's active workspace index and workspaces: id, window ids and name.
fn workspace_map(layout: &Layout<TestWindow>) -> WorkspaceMap {
    layout
        .monitors()
        .map(|mon| {
            let workspaces = mon
                .workspaces
                .iter()
                .map(|ws| {
                    let mut ids: Vec<usize> = ws.windows().map(|w| *w.id()).collect();
                    ids.sort_unstable();
                    (ws.id(), ids, ws.name().cloned())
                })
                .collect();
            (
                mon.output_name().clone(),
                mon.active_workspace_idx,
                workspaces,
            )
        })
        .collect()
}

/// The windows on the one workspace of `output`.
#[track_caller]
fn one_workspace_windows(layout: &Layout<TestWindow>, output: usize) -> Vec<usize> {
    let name = format!("output{output}");
    let (_, _, workspaces) = workspace_map(layout)
        .into_iter()
        .find(|(n, _, _)| *n == name)
        .unwrap();
    assert_eq!(workspaces.len(), 1, "{name} keeps one workspace");
    let (_, ids, ws_name) = workspaces.into_iter().next().unwrap();
    assert_eq!(ws_name, None, "the one workspace of {name} is unnamed");
    ids
}

/// With `one-workspace`, every workspace action and gesture leaves the layout as it was: no
/// workspace to switch to, move to, reorder, rename or carry to another output.
#[test]
fn one_workspace_actions_and_gestures_change_nothing() {
    let mut layout = one_workspace_layout();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            op(Op::AddOutput(2)),
            add(0),
            add(1),
            add(2),
        ],
    );
    assert_eq!(one_workspace_windows(&layout, 1), [0, 1, 2]);
    assert_eq!(one_workspace_windows(&layout, 2), Vec::<usize>::new());

    let still = [
        Op::FocusWorkspaceDown,
        Op::FocusWorkspaceUp,
        Op::FocusWorkspace(2),
        Op::FocusWorkspaceAutoBackAndForth(0),
        Op::FocusWorkspaceAutoBackAndForth(1),
        Op::FocusWorkspacePrevious,
        Op::MoveWindowToWorkspaceDown(true),
        Op::MoveWindowToWorkspaceUp(false),
        Op::MoveWindowToWorkspace {
            window_id: Some(1),
            workspace_idx: 2,
        },
        Op::MoveColumnToWorkspaceDown(true),
        Op::MoveColumnToWorkspaceUp(true),
        Op::MoveColumnToWorkspace(3, false),
        Op::MoveWorkspaceDown,
        Op::MoveWorkspaceUp,
        Op::MoveWorkspaceToIndex {
            ws_name: None,
            target_idx: 2,
        },
        Op::MoveWorkspaceToOutput(2),
        Op::MoveWorkspaceToMonitor {
            ws_name: None,
            output_id: 2,
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
        Op::UnsetWorkspaceName { ws_name: None },
        Op::AddNamedWorkspace {
            ws_name: 2,
            output_names: vec![1],
            layout_config: None,
        },
        Op::WorkspaceSwitchGestureBegin {
            output_idx: 1,
            is_touchpad: true,
        },
        Op::WorkspaceSwitchGestureUpdate {
            delta: 300.,
            timestamp: Duration::from_millis(10),
            is_touchpad: true,
        },
        Op::WorkspaceSwitchGestureEnd {
            is_touchpad: Some(true),
        },
        Op::WorkspaceSwitchGestureBegin {
            output_idx: 1,
            is_touchpad: false,
        },
        Op::WorkspaceSwitchGestureUpdate {
            delta: -300.,
            timestamp: Duration::from_millis(20),
            is_touchpad: false,
        },
        Op::WorkspaceSwitchGestureEnd { is_touchpad: None },
        Op::ToggleOverview,
        Op::CompleteAnimations,
        Op::WorkspaceSwitchGestureBegin {
            output_idx: 1,
            is_touchpad: true,
        },
        Op::WorkspaceSwitchGestureUpdate {
            delta: 500.,
            timestamp: Duration::from_millis(30),
            is_touchpad: true,
        },
        Op::WorkspaceSwitchGestureEnd {
            is_touchpad: Some(true),
        },
        Op::FocusWorkspaceDown,
        Op::DndUpdate {
            output_idx: 1,
            px: 640.,
            py: 1.,
        },
        Op::AdvanceAnimations { msec_delta: 500 },
        Op::DndUpdate {
            output_idx: 1,
            px: 640.,
            py: 719.,
        },
        Op::AdvanceAnimations { msec_delta: 500 },
        Op::DndEnd,
        Op::ToggleOverview,
        Op::CompleteAnimations,
    ];
    for o in still {
        let before = (
            workspace_map(&layout),
            grid_spans(&layout),
            layout.focus().map(|w| *w.id()),
        );
        check_canvas_ops_on(&mut layout, [op(o.clone())]);
        let after = (
            workspace_map(&layout),
            grid_spans(&layout),
            layout.focus().map(|w| *w.id()),
        );
        assert_eq!(after, before, "{o:?} changed the layout");
        assert!(layout.find_workspace_by_name("ws1").is_none());
        assert!(layout.find_workspace_by_name("ws2").is_none());
    }

    // Moving a window to another output still works: it lands on that output's one workspace.
    check_canvas_ops_on(
        &mut layout,
        [op(Op::MoveWindowToOutput {
            window_id: Some(2),
            output_id: 2,
            target_ws_idx: None,
        })],
    );
    assert_eq!(one_workspace_windows(&layout, 1), [0, 1]);
    assert_eq!(one_workspace_windows(&layout, 2), [2]);
}

/// Dragging a window in the overview to above the first workspace makes a new workspace there;
/// with `one-workspace` the window stays on the one workspace.
#[test]
fn one_workspace_overview_drop_makes_no_workspace() {
    let drag = [
        op(Op::AddOutput(1)),
        add(0),
        add(1),
        op(Op::ToggleOverview),
        op(Op::CompleteAnimations),
        op(Op::InteractiveMoveBegin {
            window: 0,
            output_idx: 1,
            px: 640.,
            py: 360.,
        }),
        op(Op::InteractiveMoveUpdate {
            window: 0,
            dx: 0.,
            dy: -1000.,
            output_idx: 1,
            px: 640.,
            py: 5.,
        }),
        op(Op::InteractiveMoveEnd { window: 0 }),
    ];

    let mut layout = Layout::default();
    check_canvas_ops_on(&mut layout, drag.clone());
    let (_, _, workspaces) = &workspace_map(&layout)[0];
    assert_eq!(
        workspaces.len(),
        3,
        "without the flag the drop makes a workspace: {workspaces:?}"
    );

    let mut layout = one_workspace_layout();
    check_canvas_ops_on(&mut layout, drag);
    assert_eq!(one_workspace_windows(&layout, 1), [0, 1]);
}

/// Turning `one-workspace` on by a reload gathers every window of an output onto its active
/// workspace, as new windows keeping their spans at free spots, and removes the other workspaces
/// and their names; turning it off brings back niri's empty workspace at the end.
#[test]
fn one_workspace_reload_gathers_the_windows_on_the_active_workspace() {
    let mut layout = Layout::default();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add_sized(0, Some(PresetSize::Fixed(1440)), None),
            add(1),
            op(Op::FocusWorkspaceDown),
            add(2),
            add(3),
            op(Op::FocusWorkspaceDown),
            add_sized(4, None, Some(PresetSize::Fixed(930))),
            op(Op::AddNamedWorkspace {
                ws_name: 1,
                output_names: vec![1],
                layout_config: None,
            }),
            op(Op::FocusWorkspace(2)),
        ],
    );
    let (_, active, workspaces) = workspace_map(&layout).remove(0);
    let (active_id, active_windows, _) = workspaces[active].clone();
    assert_eq!(active_windows, [2, 3]);
    assert!(workspaces.len() >= 4, "{workspaces:?}");
    let spans = grid_spans(&layout);

    // check_canvas_ops_on also checks that no window changed its span and none overlap.
    check_canvas_ops_on(&mut layout, [one_workspace_part(true)]);
    assert_eq!(one_workspace_windows(&layout, 1), [0, 1, 2, 3, 4]);
    let (_, _, workspaces) = workspace_map(&layout).remove(0);
    assert_eq!(workspaces[0].0, active_id, "the active workspace stays");
    let merged = grid_spans(&layout);
    for id in [2, 3] {
        assert_eq!(merged[&id].1, spans[&id].1, "window {id} stayed put");
    }
    for id in [0, 4] {
        let (old, new) = (spans[&id].1, merged[&id].1);
        assert_eq!((new.w, new.h), (old.w, old.h), "window {id} kept its span");
    }
    assert!(layout.find_workspace_by_name("ws1").is_none());

    check_canvas_ops_on(&mut layout, [one_workspace_part(false)]);
    let (_, _, workspaces) = workspace_map(&layout).remove(0);
    assert_eq!(workspaces.len(), 2, "niri's empty workspace is back");
    assert_eq!(workspaces[0].1, [0, 1, 2, 3, 4]);
}

/// With `one-workspace`, a disconnected output's windows join another output's one workspace,
/// an output that connects gets an empty one, and the first output after none gathers them all.
#[test]
fn one_workspace_outputs_coming_and_going_merge_the_windows() {
    let mut layout = one_workspace_layout();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            op(Op::AddOutput(2)),
            op(Op::FocusOutput(2)),
            add(2),
            add_sized(3, Some(PresetSize::Fixed(1440)), None),
            op(Op::RemoveOutput(2)),
        ],
    );
    assert_eq!(one_workspace_windows(&layout, 1), [0, 1, 2, 3]);

    check_canvas_ops_on(&mut layout, [op(Op::AddOutput(2))]);
    assert_eq!(one_workspace_windows(&layout, 1), [0, 1, 2, 3]);
    assert_eq!(one_workspace_windows(&layout, 2), Vec::<usize>::new());

    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::FocusOutput(2)),
            add(4),
            op(Op::RemoveOutput(1)),
            op(Op::RemoveOutput(2)),
            add(5),
            op(Op::AddOutput(3)),
        ],
    );
    assert_eq!(one_workspace_windows(&layout, 3), [0, 1, 2, 3, 4, 5]);
}

/// With `one-workspace` in the config, its named workspaces are never made.
#[test]
fn one_workspace_ignores_named_workspaces_in_the_config() {
    let config = niri_config::Config::parse_mem(
        r#"
        layout {
            canvas {
                one-workspace
            }
        }
        workspace "ws1"
        workspace "ws2" {
            open-on-output "output1"
        }
        "#,
    )
    .unwrap();
    let mut layout = Layout::new(Clock::with_time(Duration::ZERO), &config);
    check_canvas_ops_on(&mut layout, [op(Op::AddOutput(1)), add(0)]);
    for ws_config in &config.workspaces {
        layout.ensure_named_workspace(ws_config);
    }
    layout.verify_invariants();
    assert_eq!(one_workspace_windows(&layout, 1), [0]);
    assert!(layout.find_workspace_by_name("ws1").is_none());
    assert!(layout.find_workspace_by_name("ws2").is_none());
}

fn resize_drag(window: usize, dx: f64, dy: f64) -> CanvasOp {
    op(Op::InteractiveResizeUpdate { window, dx, dy })
}

#[test]
fn canvas_resize_drag_previews_what_the_release_does() {
    let mut layout = placed(&row(3));
    let before = window_rects(&layout);
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::InteractiveResizeBegin {
                window: 0,
                edges: ResizeEdge::RIGHT,
            }),
            resize_drag(0, 100., 0.),
        ],
    );
    let ws = layout.active_workspace().unwrap();
    assert_eq!(
        ws.scrolling().resize_preview(),
        (None, vec![]),
        "under half a tile, nothing to preview"
    );
    check_canvas_ops_on(&mut layout, [resize_drag(0, 336., 0.)]);
    let (shown, others) = layout
        .active_workspace()
        .unwrap()
        .scrolling()
        .resize_preview();
    assert_eq!(
        window_rects(&layout),
        before,
        "the drag changes nothing until the release"
    );
    assert_eq!(shown, Some(Rect::new(0, 0, 2, 1)), "one tile wider");
    assert!(
        !others.is_empty(),
        "the neighbour is drawn where the resize pushes it"
    );
    check_canvas_ops_on(&mut layout, [op(Op::InteractiveResizeEnd { window: 0 })]);
    let after = window_rects(&layout);
    assert_eq!(
        after[&0],
        Rect::new(0, 0, 2, 1),
        "the release applies the previewed size"
    );
    let mut moved: Vec<Rect> = after
        .iter()
        .filter(|(w, r)| **w != 0 && before[*w] != **r)
        .map(|(_, r)| *r)
        .collect();
    moved.sort_by_key(|r| (r.j, r.i));
    assert_eq!(moved, others, "the others land where the preview drew them");
    assert_eq!(
        layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .resize_preview(),
        (None, vec![]),
        "the release takes the preview down"
    );
}

#[test]
fn canvas_resize_drag_back_and_cancel_keep_the_layout() {
    let mut layout = placed(&row(3));
    let before = window_rects(&layout);
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::InteractiveResizeBegin {
                window: 1,
                edges: ResizeEdge::LEFT | ResizeEdge::BOTTOM,
            }),
            resize_drag(1, -300., 200.),
        ],
    );
    let (shown, _) = layout
        .active_workspace()
        .unwrap()
        .scrolling()
        .resize_preview();
    assert!(
        shown.is_some(),
        "a tile left and a tile down previews a bigger window"
    );
    check_canvas_ops_on(&mut layout, [resize_drag(1, -20., 10.)]);
    assert_eq!(
        layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .resize_preview(),
        (None, vec![]),
        "dragging back takes the preview down"
    );
    check_canvas_ops_on(&mut layout, [resize_drag(1, 0., 330.)]);
    assert!(layout.canvas_cancel_resize(), "Esc cancels the resize drag");
    assert!(!layout.canvas_cancel_resize(), "and only once");
    check_canvas_ops_on(&mut layout, [op(Op::InteractiveResizeEnd { window: 1 })]);
    assert_eq!(
        window_rects(&layout),
        before,
        "a cancelled drag resizes nothing"
    );
    assert_eq!(
        layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .resize_preview(),
        (None, vec![])
    );
}

/// Lets go of `window`'s resize drag and checks it does what the preview showed just before.
#[track_caller]
fn assert_release_matches_preview(layout: &mut Layout<TestWindow>, window: usize, when: &str) {
    let before = window_rects(layout);
    let (shown, others) = layout
        .active_workspace()
        .unwrap()
        .scrolling()
        .resize_preview();
    check_canvas_ops_on(layout, [op(Op::InteractiveResizeEnd { window })]);
    let after = window_rects(layout);
    let applied = Some(after[&window]).filter(|r| *r != before[&window]);
    assert_eq!(applied, shown, "{when}: the release applies the box");
    let mut moved: Vec<Rect> = after
        .iter()
        .filter(|(w, r)| **w != window && before.get(*w).is_some_and(|b| b != *r))
        .map(|(_, r)| *r)
        .collect();
    moved.sort_by_key(|r| (r.j, r.i));
    assert_eq!(
        moved, others,
        "{when}: the others land where the preview drew them"
    );
}

#[test]
fn canvas_resize_drag_shrink_release_keeps_the_previewed_neighbour() {
    let mut layout = wide_then_neighbour(small_tile_options());
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::InteractiveResizeBegin {
                window: 0,
                edges: ResizeEdge::RIGHT,
            }),
            resize_drag(0, -240., 0.),
            op(Op::CompleteAnimations),
        ],
    );
    let (shown, others) = layout
        .active_workspace()
        .unwrap()
        .scrolling()
        .resize_preview();
    assert_eq!(shown, Some(Rect::new(0, 0, 1, 1)), "one tile narrower");
    assert_eq!(
        others,
        vec![Rect::new(1, 0, 1, 1)],
        "1 fills the freed tile"
    );
    let previewed = drawn_tiles(&layout)[&1].loc;
    check_canvas_ops_on(&mut layout, [op(Op::InteractiveResizeEnd { window: 0 })]);
    for step in 0..10 {
        check_canvas_ops_on(&mut layout, [op(Op::AdvanceAnimations { msec_delta: 50 })]);
        let at = drawn_tiles(&layout)[&1].loc;
        assert!(
            (at.x - previewed.x).abs() < 0.5 && (at.y - previewed.y).abs() < 0.5,
            "{step}: 1 was previewed at {previewed:?} and is drawn at {at:?} after the release"
        );
    }
    check_canvas_ops_on(
        &mut layout,
        [op(Op::Communicate(0)), op(Op::CompleteAnimations)],
    );
    assert_eq!(
        drawn_tiles(&layout)[&1].loc,
        previewed,
        "settled where the preview drew it"
    );
}

#[test]
fn canvas_resize_preview_follows_a_change_during_the_drag() {
    let mut layout = placed(&row(3));
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::InteractiveResizeBegin {
                window: 0,
                edges: ResizeEdge::RIGHT,
            }),
            resize_drag(0, 336., 0.),
        ],
    );
    let (_, others) = layout
        .active_workspace()
        .unwrap()
        .scrolling()
        .resize_preview();
    assert!(!others.is_empty(), "the neighbour is previewed pushed");
    check_canvas_ops_on(&mut layout, [op(Op::FocusWindow(2))]);
    assert_release_matches_preview(&mut layout, 0, "after a focus change");
}

#[test]
fn canvas_resize_preview_follows_a_zoom_during_the_drag() {
    let mut layout = placed(&row(3));
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::InteractiveResizeBegin {
                window: 0,
                edges: ResizeEdge::RIGHT,
            }),
            resize_drag(0, 336., 0.),
            CanvasOp::Zoom(-1),
        ],
    );
    assert_release_matches_preview(&mut layout, 0, "after a zoom");
}

#[test]
fn canvas_carried_span_is_not_taken_by_a_placeholder() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add_app(0, "app", "w"),
            CanvasOp::ResizeEdge(Edge::Right, 1),
            CanvasOp::ResizeEdge(Edge::Bottom, 1),
            op(Op::FocusWorkspaceDown),
        ],
    );
    layout.canvas_launch_placeholder(String::from("app"), None, (None, None));
    assert_eq!(
        layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .canvas()
            .windows()
            .len(),
        1,
        "the placeholder waits on the second workspace"
    );
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::FocusWorkspaceUp),
            op(Op::MoveWindowToWorkspaceDown(true)),
        ],
    );
    let r = window_rects(&layout)[&0];
    assert_eq!((r.w, r.h), (2, 2), "0 keeps the span it carried");
}

#[test]
fn canvas_drop_off_the_grid_takes_the_preview_down() {
    let mut layout = small_tiles();
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            op(Op::FocusWindow(0)),
            op(Op::InteractiveMoveBegin {
                window: 0,
                output_idx: 1,
                px: 20.,
                py: 20.,
            }),
            drag(240., 0., 260., 20.),
        ],
    );
    let (_, shown) = layout
        .active_workspace()
        .unwrap()
        .scrolling()
        .resize_preview();
    assert!(!shown.is_empty(), "dropping 0 on 1's tile would move 1");
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::FullscreenWindow(0)),
            op(Op::InteractiveMoveEnd { window: 0 }),
        ],
    );
    assert_eq!(
        layout
            .active_workspace()
            .unwrap()
            .scrolling()
            .resize_preview()
            .1,
        vec![],
        "a drop that leaves the grid takes the preview down"
    );
}

#[cfg(feature = "telemetry")]
#[test]
fn canvas_pan_momentum_zero_stops_where_the_hand_stopped() {
    let mut options = Options::default();
    options.layout.canvas.tile_width = 240.;
    options.layout.canvas.tile_height = 155.;
    options.layout.canvas.pan_momentum = 0.;
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    check_canvas_ops_on(
        &mut layout,
        [
            op(Op::AddOutput(1)),
            add(0),
            add(1),
            add(2),
            op(Op::CompleteAnimations),
        ],
    );
    frames(&mut layout, 2);
    let skip = usage_lines(&layout, 0).len();
    let output = layout.active_output().unwrap().clone();
    layout.view_offset_gesture_begin(&output, None, false);
    for k in 1..=5 {
        layout.canvas_pan_update(-30., -12., Duration::from_millis(16 * k), false);
        frames(&mut layout, 1);
    }
    layout.view_offset_gesture_end(Some(false));
    frames(&mut layout, 200);
    let lines = usage_lines(&layout, skip);
    let pan = motion_of(&lines, "pan-pointer");
    let step = 30f64.hypot(12.);
    assert!(num(&pan["camera"]["path"]) >= 5. * step - 1., "{pan:#}");
    assert!(
        num(&pan["camera"]["path"]) < 6. * step,
        "letting go does not carry the view on: {pan:#}"
    );
}

fn slot(app_id: &str, rect: Rect) -> RestoreWindow {
    RestoreWindow {
        app_id: app_id.to_owned(),
        title: None,
        rect,
    }
}

/// A terminal two tiles wide and a browser two tiles tall right of it.
fn two_slots() -> Vec<RestoreWindow> {
    vec![
        slot("term", Rect::new(0, 0, 2, 1)),
        slot("browser", Rect::new(2, 0, 1, 2)),
    ]
}

fn place_block(
    layout: &mut Layout<TestWindow>,
    slots: Vec<RestoreWindow>,
    anchor: NewmBlockAnchor,
    focus: Option<usize>,
    fly: bool,
) -> Result<(u64, (i32, i32)), String> {
    let rv = layout.canvas_place_block(slots, anchor, focus, fly, Duration::from_secs(20));
    layout.verify_invariants();
    rv
}

fn block_status(layout: &Layout<TestWindow>, id: u64) -> (Vec<Option<usize>>, bool) {
    let (windows, done) = layout.canvas_block_status(id).unwrap();
    (
        windows.into_iter().map(|w| w.map(|w| *w.id())).collect(),
        done,
    )
}

/// Each slot's rectangle moved to `origin`.
fn at(origin: (i32, i32), slots: &[RestoreWindow]) -> Vec<Rect> {
    slots
        .iter()
        .map(|s| Rect::new(s.rect.i + origin.0, s.rect.j + origin.1, s.rect.w, s.rect.h))
        .collect()
}

#[test]
fn canvas_block_at_nearest_lands_whole_a_tile_clear_of_the_windows() {
    let mut layout = placed(&row(3));
    let before = window_rects(&layout);
    let (_, origin) = place_block(
        &mut layout,
        two_slots(),
        NewmBlockAnchor::Nearest,
        None,
        false,
    )
    .unwrap();
    let all = rects(&layout);
    let block = at(origin, &two_slots());
    assert_eq!(all.len(), 5);
    for r in &block {
        assert!(all.contains(r), "slot {r:?} reserved");
        for w in before.values() {
            let wide = Rect::new(r.i - 1, r.j - 1, r.w + 2, r.h + 2);
            assert!(!w.overlaps(&wide), "slot {r:?} a tile clear of {w:?}");
        }
    }
    assert_eq!(window_rects(&layout), before, "no window moves");
    assert_eq!(focus_id(&layout), Some(0), "the focus stays");
    assert_eq!(active_canvas(&layout).view(), (0., 0.), "the camera stays");
}

#[test]
fn canvas_block_at_camera_centres_on_the_view() {
    let mut layout = small_tiles();
    check_canvas_ops_on(&mut layout, [op(Op::AddOutput(1))]);
    let c = active_canvas(&layout);
    let ((tw, th), (sw, sh), (vx, vy)) = (c.tile_size(), c.screen(), c.view());
    let centre = ((vx + sw / 2.) / tw, (vy + sh / 2.) / th);
    let want = (
        (centre.0 - 1.5).round_ties_even() as i32,
        (centre.1 - 1.).round_ties_even() as i32,
    );
    let (_, origin) = place_block(
        &mut layout,
        two_slots(),
        NewmBlockAnchor::Camera,
        None,
        false,
    )
    .unwrap();
    assert_eq!(origin, want);
    assert_eq!(rects(&layout), at(origin, &two_slots()));
}

#[test]
fn canvas_block_at_tiles_lands_there_or_not_at_all() {
    let mut layout = placed(&row(3));
    let (_, origin) = place_block(
        &mut layout,
        two_slots(),
        NewmBlockAnchor::Tiles(5, 0),
        None,
        false,
    )
    .unwrap();
    assert_eq!(origin, (5, 0));
    let mut all = rects(&layout);
    all.retain(|r| r.i >= 5);
    assert_eq!(all, at((5, 0), &two_slots()));

    let held = rects(&layout);
    assert_eq!(
        place_block(
            &mut layout,
            two_slots(),
            NewmBlockAnchor::Tiles(1, 0),
            None,
            false
        ),
        Err(String::from("the block's tiles are taken"))
    );
    assert_eq!(rects(&layout), held, "nothing reserved");
    assert_eq!(
        place_block(
            &mut layout,
            Vec::new(),
            NewmBlockAnchor::Nearest,
            None,
            false
        ),
        Err(String::from("bad block"))
    );
}

#[test]
fn canvas_block_windows_take_their_slots_by_app_in_launch_order() {
    let mut layout = placed(&row(1));
    let slots = vec![
        slot("term", Rect::new(0, 0, 1, 1)),
        slot("term", Rect::new(1, 0, 1, 1)),
        slot("browser", Rect::new(0, 1, 2, 1)),
    ];
    let (id, origin) = place_block(
        &mut layout,
        slots,
        NewmBlockAnchor::Tiles(4, 0),
        None,
        false,
    )
    .unwrap();
    assert_eq!(block_status(&layout, id), (vec![None, None, None], false));

    check_canvas_ops_on(
        &mut layout,
        [add_app(10, "term", "a"), add_app(11, "browser", "b")],
    );
    assert_eq!(
        block_status(&layout, id),
        (vec![Some(10), None, Some(11)], false)
    );
    check_canvas_ops_on(&mut layout, [add_app(12, "term", "c")]);
    assert_eq!(
        block_status(&layout, id),
        (vec![Some(10), Some(12), Some(11)], true)
    );
    let by_id = window_rects(&layout);
    assert_eq!(by_id[&10], Rect::new(origin.0, 0, 1, 1));
    assert_eq!(by_id[&12], Rect::new(origin.0 + 1, 0, 1, 1));
    assert_eq!(by_id[&11], Rect::new(origin.0, 1, 2, 1));
    assert_eq!(rects(&layout).len(), 4, "no placeholder left");
    assert!(layout.canvas_block_status(id + 1000).is_none());
}

/// A block saved on a host where an app runs under X11 fills on a host where it runs on Wayland.
#[test]
fn canvas_block_slots_match_an_app_whatever_the_case_of_its_id() {
    let mut layout = placed(&row(1));
    let slots = vec![
        slot("Chromium-browser", Rect::new(0, 0, 2, 1)),
        slot("term", Rect::new(2, 0, 1, 1)),
    ];
    let (id, _) = place_block(
        &mut layout,
        slots,
        NewmBlockAnchor::Tiles(4, 0),
        None,
        false,
    )
    .unwrap();
    check_canvas_ops_on(
        &mut layout,
        [
            add_app(10, "Term", "a"),
            add_app(11, "chromium-browser", "b"),
        ],
    );
    assert_eq!(block_status(&layout, id), (vec![Some(11), Some(10)], true));
    assert_eq!(rects(&layout).len(), 3, "no placeholder left");
}

#[test]
fn canvas_block_focuses_its_window_and_flies_the_camera_when_asked() {
    let mut layout = placed(&row(1));
    let far = Rect::new(20, 0, 3, 2);
    let (fly, _) = place_block(
        &mut layout,
        two_slots(),
        NewmBlockAnchor::Tiles(20, 0),
        Some(1),
        true,
    )
    .unwrap();
    assert!(
        active_canvas(&layout).is_visible(far),
        "the camera shows the block"
    );
    assert_eq!(
        focus_id(&layout),
        None,
        "slot 1's placeholder has the focus"
    );
    check_canvas_ops_on(&mut layout, [add_app(1, "browser", "b")]);
    assert_eq!(focus_id(&layout), Some(1), "its window keeps it");
    check_canvas_ops_on(&mut layout, [add_app(2, "term", "t")]);
    assert!(block_status(&layout, fly).1);
    assert_eq!(
        focus_id(&layout),
        Some(1),
        "another of the block's windows takes none"
    );
    assert!(active_canvas(&layout).is_visible(far));

    let view = active_canvas(&layout).view();
    let (stay, _) = place_block(
        &mut layout,
        two_slots(),
        NewmBlockAnchor::Tiles(40, 0),
        Some(0),
        false,
    )
    .unwrap();
    assert_eq!(active_canvas(&layout).view(), view, "the camera stays");
    assert_eq!(focus_id(&layout), Some(1), "and so does the focus");
    check_canvas_ops_on(&mut layout, [add_app(3, "term", "t")]);
    assert_eq!(focus_id(&layout), Some(1), "until the block is complete");
    check_canvas_ops_on(&mut layout, [add_app(4, "browser", "b")]);
    assert_eq!(block_status(&layout, stay), (vec![Some(3), Some(4)], true));
    assert_eq!(focus_id(&layout), Some(3), "then slot 0's window takes it");
    assert!(active_canvas(&layout).is_visible(Rect::new(40, 0, 2, 1)));
}

#[test]
fn canvas_block_releases_a_slot_no_window_takes() {
    let mut layout = placed(&row(1));
    let (id, _) = place_block(
        &mut layout,
        two_slots(),
        NewmBlockAnchor::Tiles(3, 0),
        Some(1),
        false,
    )
    .unwrap();
    check_canvas_ops_on(
        &mut layout,
        [
            add_app(1, "term", "t"),
            op(Op::AdvanceAnimations { msec_delta: 19000 }),
        ],
    );
    assert_eq!(block_status(&layout, id), (vec![Some(1), None], false));
    assert_eq!(rects(&layout).len(), 3);

    check_canvas_ops_on(
        &mut layout,
        [op(Op::AdvanceAnimations { msec_delta: 1000 })],
    );
    assert_eq!(block_status(&layout, id), (vec![Some(1), None], true));
    assert_eq!(rects(&layout).len(), 2, "the released tiles are free");
    check_canvas_ops_on(&mut layout, [add_app(2, "browser", "late")]);
    assert_eq!(block_status(&layout, id), (vec![Some(1), None], true));
}

#[test]
fn canvas_park_moves_the_windows_aside_as_one_and_undoes_in_one_step() {
    // Window 1 is a bridge: moving it first would gather the others.
    let tiles = [
        Rect::new(0, 0, 1, 1),
        Rect::new(1, 0, 1, 1),
        Rect::new(2, 0, 1, 1),
        Rect::new(1, 1, 2, 1),
    ];
    let mut layout = placed(&tiles);
    let before = window_rects(&layout);
    let view = active_canvas(&layout).view();
    assert!(layout.canvas_park());
    layout.verify_invariants();
    let after = window_rects(&layout);
    let di = after[&0].i - before[&0].i;
    for (id, r) in &before {
        assert_eq!(
            after[id],
            Rect::new(r.i + di, r.j, r.w, r.h),
            "window {id} keeps its place"
        );
    }
    let c = active_canvas(&layout);
    let left = (view.0 / c.tile_size().0).floor() as i32;
    assert_eq!(after.values().map(|r| r.right()).max(), Some(left - 1));
    let focus = focus_id(&layout).unwrap();
    assert_eq!(
        after[&focus].right(),
        left - 1,
        "the focus goes to a window nearest the view"
    );
    assert!(c.is_visible(after[&focus]), "which the camera shows");

    assert!(layout.canvas_undo());
    layout.verify_invariants();
    assert_eq!(
        window_rects(&layout),
        before,
        "one undo takes the park back"
    );
    assert!(layout.canvas_redo());
    assert_eq!(window_rects(&layout), after, "one redo parks again");
    assert!(layout.canvas_undo());
    assert_eq!(window_rects(&layout), before);
    assert!(!layout.canvas_undo(), "the park was the only step");
}

#[test]
fn canvas_block_at_nearest_lands_in_the_view_a_park_cleared() {
    let mut layout = placed(&row(3));
    assert!(layout.canvas_park());
    let (_, origin) = place_block(
        &mut layout,
        two_slots(),
        NewmBlockAnchor::Nearest,
        None,
        false,
    )
    .unwrap();
    let c = active_canvas(&layout);
    assert!(
        c.is_visible(Rect::new(origin.0, origin.1, 3, 2)),
        "the view the park cleared holds the block"
    );
}
