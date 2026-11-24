//! How a grid window moves without changing its span. A span changes only through a resize or
//! the window's minimum (the newm-canvas proof, C47). A drag that stays on its canvas keeps the
//! window on the grid, drawn at the pointer, and its drop is one `Canvas::move_to`: taking the
//! window out and adding it back would open it at a new window's span (F04). A tile that leaves
//! its canvas carries its span, and the canvas it arrives on opens it at that span, cut to what
//! fits there (C48).
//!
//! While the drag stays on its canvas, the others are drawn where the drop would put them: each
//! tile the window snaps to names one drop state, worked out on a copy of the canvas from the
//! layout the drag started on, so dragging back puts them back. Esc cancels the drag.

use std::collections::HashMap;

use newm_canvas::{Id, Rect};
use newm_kde::apps;
use niri_config::PresetSize;
use serde_json::{json, Value};
use smithay::output::Output;
use smithay::utils::{Logical, Point};

use super::motion::{Cause, DRAG};
use super::{CanvasSpace, SPAN_MEMORY};
use crate::layout::monitor::InsertWorkspace;
use crate::layout::tile::Tile;
use crate::layout::{InteractiveMoveState, Layout, LayoutElement};

impl<W: LayoutElement> Layout<W> {
    /// Whether an interactive move of a grid window stays on its canvas: the window is on the
    /// grid (not fullscreen or maximized, which leave through niri's move to floating), and the
    /// pointer is over the workspace that holds it, on that workspace's output.
    pub(in crate::layout) fn canvas_keeps_move(
        &self,
        window: &W::Id,
        output: &Output,
        pos_within_output: Point<f64, Logical>,
    ) -> bool {
        self.monitors().any(|mon| {
            let InsertWorkspace::Existing(id) = mon.insert_position(pos_within_output).0 else {
                return false;
            };
            mon.output() == output
                && mon
                    .workspaces
                    .iter()
                    .any(|ws| ws.id() == id && ws.scrolling().is_on_grid(window))
        })
    }
}

impl<W: LayoutElement> Layout<W> {
    /// Cancels a drag that stayed on its canvas: the preview goes, the window glides home and
    /// nothing drops. False when no such drag is on.
    pub fn canvas_cancel_move(&mut self) -> bool {
        let Some(InteractiveMoveState::Starting { window_id, .. }) = &self.interactive_move else {
            return false;
        };
        let window_id = window_id.clone();
        self.interactive_move = None;
        for mon in self.monitors_mut() {
            mon.dnd_scroll_gesture_end();
        }
        for ws in self.workspaces_mut() {
            ws.scrolling_mut().cancel_moved(&window_id);
            if let Some(tile) = ws.tiles_mut().find(|t| *t.window().id() == window_id) {
                let offset = tile.interactive_move_offset;
                tile.interactive_move_offset = Point::from((0., 0.));
                tile.animate_move_from(offset);
            }
            ws.dnd_scroll_gesture_end();
        }
        true
    }

    /// Shows, for the drag of `window` on its canvas, the drop state of the tile it is over.
    pub(in crate::layout) fn canvas_preview_move(&mut self, window: &W::Id) {
        for ws in self.workspaces_mut() {
            ws.scrolling_mut().preview_moved(window);
        }
    }

    /// Takes down every drag preview, the windows gliding back to their own tiles.
    pub(in crate::layout) fn canvas_clear_previews(&mut self) {
        for ws in self.workspaces_mut() {
            ws.scrolling_mut().clear_preview();
        }
    }
}

impl<W: LayoutElement> CanvasSpace<W> {
    /// The tile a dragged grid window is drawn nearest: its own plus its drag offset in tiles.
    fn drag_target(&self, idx: usize) -> (Id, i32, i32) {
        let e = &self.entries[idx];
        let offset = e.tile.interactive_move_offset;
        let r = self.canvas.rect(e.id).unwrap();
        let (tw, th) = self.canvas.base_tile();
        (
            e.id,
            r.i + (offset.x / tw).round() as i32,
            r.j + (offset.y / th).round() as i32,
        )
    }

    /// Draws the others where dropping `window` on the tile it is drawn nearest would put them.
    pub fn preview_moved(&mut self, window: &W::Id) {
        if !self.is_on_grid(window) {
            return;
        }
        let at = self.drag_target(self.idx_of(window).unwrap());
        if self.drag.is_none_or(|(d, _)| d != at.0) {
            self.drag = self.canvas.rect(at.0).map(|r| (at.0, r));
        }
        if self.preview_at == Some(at) {
            return;
        }
        let (id, i, j) = at;
        let r = self.canvas.rect(id).unwrap();
        let shown = if (i, j) == (r.i, r.j) {
            HashMap::new()
        } else {
            let mut c = self.canvas.clone();
            c.move_to(id, i, j);
            c.windows()
                .iter()
                .filter(|w| w.id != id && Some(w.rect) != self.canvas.rect(w.id))
                .map(|w| (w.id, w.rect))
                .collect()
        };
        self.show_preview(shown);
        self.preview_at = Some(at);
    }

    /// Takes the preview down, the windows gliding back to their own tiles.
    pub fn clear_preview(&mut self) {
        self.preview_at = None;
        if !self.preview.is_empty() {
            self.show_preview(HashMap::new());
        }
    }

    /// Draws the windows at `shown` (their own tiles when absent), each gliding from where it
    /// is drawn now.
    pub(super) fn show_preview(&mut self, shown: HashMap<Id, Rect>) {
        let before = self.drawn_now(None);
        let mut started = false;
        for idx in 0..self.entries.len() {
            let id = self.entries[idx].id;
            let (Some(old), Some(own)) = (self.shown_rect(id), self.canvas.rect(id)) else {
                continue;
            };
            let new = shown.get(&id).copied().unwrap_or(own);
            if old != new {
                let from = self.cell(old).loc - self.cell(new).loc;
                self.entries[idx].tile.animate_move_from(from);
                started = true;
            }
        }
        self.preview = shown;
        if started {
            self.begin_motion(Cause::carry(DRAG), before);
        }
    }

    /// The tile and the held offset (`Entry::held`) of each window `shown` by a preview,
    /// taken before the op its release applies, for [`Self::keep_previewed`].
    pub(super) fn before_release(
        &self,
        shown: &HashMap<Id, Rect>,
    ) -> HashMap<Id, (Rect, Point<f64, Logical>)> {
        self.entries
            .iter()
            .filter(|e| shown.contains_key(&e.id))
            .filter_map(|e| Some((e.id, (self.canvas.rect(e.id)?, e.held))))
            .collect()
    }

    /// After the op a drag's release applied: a window the preview drew on the tile the op gave
    /// it stays drawn there. `apply` moved it from its old tile by a glide, or, while a shrink
    /// waits, by holding it there (see [`Self::shrinking`]); both are taken back. One the op put
    /// elsewhere moves on as `apply` started it.
    pub(super) fn keep_previewed(
        &mut self,
        shown: &HashMap<Id, Rect>,
        before: &HashMap<Id, (Rect, Point<f64, Logical>)>,
    ) {
        for k in 0..self.entries.len() {
            let w = self.entries[k].id;
            let (Some(&seen), Some(now), Some(&(old, held))) =
                (shown.get(&w), self.canvas.rect(w), before.get(&w))
            else {
                continue;
            };
            if seen != now {
                continue;
            }
            let moved = self.cell(now).loc - self.cell(old).loc;
            let e = &mut self.entries[k];
            let glided = moved + (e.held - held);
            e.held = held;
            if glided != Point::from((0., 0.)) {
                e.tile.animate_move_from(glided);
            }
        }
    }

    /// Ends a cancelled drag of `window` here: the preview goes, and the cancel is logged.
    fn cancel_moved(&mut self, window: &W::Id) {
        self.clear_preview();
        if let Some(id) = self.id_of(Some(window)) {
            self.drag = None;
            let app = self.app_of(id);
            self.logged("cancel", Some(id), app, |_| ());
        }
    }

    /// Whether `window` is a grid window of this canvas: tiled here, neither fullscreen nor
    /// maximized, nor asked to become either.
    pub fn is_on_grid(&self, window: &W::Id) -> bool {
        self.idx_of(window).is_some_and(|idx| {
            let tile = &self.entries[idx].tile;
            tile.sizing_mode().is_normal() && tile.window().pending_sizing_mode().is_normal()
        })
    }

    /// Ends an interactive move that stayed on this canvas: the window goes to the tiles nearest
    /// where it is drawn, through one `Canvas::move_to`, takes the focus, and glides there from
    /// where the pointer left it. A drop on the tiles it holds changes nothing. False when
    /// `window` is not a grid window of this canvas.
    pub fn drop_moved(&mut self, window: &W::Id) -> bool {
        let drag = self.drag.take();
        if !self.is_on_grid(window) {
            return false;
        }
        let idx = self.idx_of(window).unwrap();
        let (id, i, j) = self.drag_target(idx);
        let r = self.canvas.rect(id).unwrap();
        let home_moved = drag.is_some_and(|(d, home)| d == id && home != r);
        let drawn = self.drawn_now(Some(id));
        let shown = std::mem::take(&mut self.preview);
        self.preview_at = None;
        let offset = self.entries[idx].tile.interactive_move_offset;
        self.entries[idx].tile.interactive_move_offset = Point::from((0., 0.));
        if (i, j) != (r.i, r.j) {
            let before = self.before_release(&shown);
            let app = self.app_of(id);
            self.motion_from(drawn);
            self.logged_with(
                "drop",
                Some(id),
                app,
                |c| {
                    c.move_to(id, i, j);
                    c.focus_id(id);
                },
                |_, _| json!({ "home_moved": home_moved }),
            );
            self.keep_previewed(&shown, &before);
            self.update_tile_sizes(true, None);
        } else if !shown.is_empty() {
            self.preview = shown;
            self.clear_preview();
        }
        self.entries[idx].tile.animate_move_from(offset);
        true
    }

    /// Gives a tile leaving this canvas the span it holds here.
    pub(super) fn carry_span(&self, id: Id, tile: &mut Tile<W>) {
        tile.canvas_span = self.canvas.rect(id).map(|r| (r.w, r.h));
    }

    /// The span a tile arriving on this canvas asks to open at, and where it came from: the span
    /// it carries from the canvas it left; else, per axis, the span its window rule sets, the size
    /// the app picked where its first configure left that axis to it (rounded up to whole tiles
    /// with its border and gaps, as a minimum is), the span of the most recently focused open grid
    /// window of its app here, the span its app's last window that closed or moved away from here
    /// had, the canvas's default size (unless an empty rule opts out of it), and one tile.
    pub(super) fn arrival_span(&self, tile: &mut Tile<W>) -> OpenSpan {
        if let Some(span) = tile.canvas_span.take() {
            return OpenSpan {
                span,
                source: json!("carried"),
                app_px: None,
                carried: true,
            };
        }
        let win = tile.window();
        let rules = win.rules();
        let rule = |size: Option<Option<PresetSize>>, horizontal| {
            size.flatten().map(|s| self.preset_span(s, horizontal))
        };
        let got = win.size();
        let asked = win.requested_size();
        let picked = |asked: Option<i32>, got: i32| asked == Some(0) && got > 0;
        let pad = self.inset() * 2. + 1. / f64::min(self.scale, 1.);
        let (need_w, need_h) = self.canvas.need(
            tile.tile_width_for_window_width(f64::from(got.w)) + pad,
            tile.tile_height_for_window_height(f64::from(got.h)) + pad,
        );
        let memory = win.app_id().and_then(|app| {
            let app = apps::canonical(&app);
            self.sibling_span(&app)
                .map(|s| (s, "sibling"))
                .or_else(|| self.remembered_span(&app).map(|s| (s, "last-closed")))
        });
        let canvas = &self.options.layout.canvas;
        let default =
            |rule: Option<Option<PresetSize>>, size: Option<PresetSize>, horizontal| match rule {
                Some(None) => None,
                _ => size.map(|s| self.preset_span(s, horizontal)),
            };
        let axis = |rule: Option<i32>,
                    app: Option<i32>,
                    memory: Option<(i32, &'static str)>,
                    default: Option<i32>| {
            rule.map(|n| (n, "rule"))
                .or(app.map(|n| (n, "app")))
                .or(memory)
                .or(default.map(|n| (n, "default")))
                .unwrap_or((1, "tile"))
        };
        let (w, sw) = axis(
            rule(rules.default_width, true),
            picked(asked.map(|a| a.w), got.w).then_some(need_w),
            memory.map(|((w, _), s)| (w, s)),
            default(rules.default_width, canvas.default_width, true),
        );
        let (h, sh) = axis(
            rule(rules.default_height, false),
            picked(asked.map(|a| a.h), got.h).then_some(need_h),
            memory.map(|((_, h), s)| (h, s)),
            default(rules.default_height, canvas.default_height, false),
        );
        OpenSpan {
            span: (w, h),
            source: if sw == sh {
                json!(sw)
            } else {
                json!({ "w": sw, "h": sh })
            },
            app_px: Some([got.w, got.h]),
            carried: false,
        }
    }

    /// The span of the most recently focused open grid window of `app` (as [`apps::canonical`])
    /// here.
    fn sibling_span(&self, app: &str) -> Option<(i32, i32)> {
        self.canvas.recent().iter().find_map(|&id| {
            let e = self.entries.iter().find(|e| e.id == id)?;
            let on_grid = e.tile.sizing_mode().is_normal();
            let same = |id: String| apps::canonical(&id) == app;
            if !on_grid || !e.tile.window().app_id().is_some_and(same) {
                return None;
            }
            self.canvas.rect(id).map(|r| (r.w, r.h))
        })
    }

    /// The span the last window of `app` (as [`apps::canonical`]) that closed or moved away from
    /// here had.
    fn remembered_span(&self, app: &str) -> Option<(i32, i32)> {
        self.span_memory
            .iter()
            .rev()
            .find(|(a, _)| a == app)
            .map(|&(_, span)| span)
    }

    /// Remembers `span` as the last one of `app`'s windows here; the least recent app goes past
    /// [`SPAN_MEMORY`] apps.
    pub(super) fn remember_span(&mut self, app: String, span: (i32, i32)) {
        let app = apps::canonical(&app);
        self.span_memory.retain(|(a, _)| *a != app);
        self.span_memory.push((app, span));
        if self.span_memory.len() > SPAN_MEMORY {
            self.span_memory.remove(0);
        }
    }
}

/// The span a new tile asks to open at (see [`CanvasSpace::arrival_span`]), where it came from
/// for the usage log, the size the app had committed then, and whether the tile carried it.
pub(super) struct OpenSpan {
    pub span: (i32, i32),
    pub source: Value,
    pub app_px: Option<[i32; 2]>,
    pub carried: bool,
}
