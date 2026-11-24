//! A resize drag's preview. While Mod + the secondary button drags an edge, a box in the border's
//! active colour shows where the window will be when the button is let go, and the windows the
//! resize pushes are drawn where it pushes them. Each dragged edge moves by the whole tiles the
//! pointer travelled, so the preview changes once per tile, worked out on a copy of the canvas
//! from the layout the drag started on: dragging back puts the others back. The window keeps its
//! size until the release, which applies the previewed resize. Esc cancels the drag.

use std::collections::HashMap;

use newm_canvas::{Edge, Id, Rect};
use niri_config::CornerRadius;
use smithay::utils::{Logical, Point, Rectangle, Size};

use super::{CanvasSpace, MAX_SPAN};
use crate::layout::focus_ring::{FocusRing, FocusRingRenderElement};
use crate::layout::{Layout, LayoutElement};
use crate::render_helpers::renderer::NiriRenderer;
use crate::utils::ResizeEdge;

/// The outline's width, in logical pixels.
const EDGE: f64 = 2.;
/// How opaque the box's fill is, over the border colour's own alpha.
const FILL_ALPHA: f32 = 0.3;

/// The box drawn where the resized window will be.
#[derive(Debug)]
pub struct ResizeBox {
    pub id: Id,
    pub rect: Rect,
    fill: FocusRing,
    edge: FocusRing,
}

impl ResizeBox {
    fn new(id: Id, rect: Rect, border: niri_config::Border) -> Self {
        let ring: niri_config::FocusRing = border.into();
        let fill = FocusRing::new(niri_config::FocusRing {
            off: false,
            width: 0.,
            ..ring
        });
        let edge = FocusRing::new(niri_config::FocusRing {
            off: false,
            width: EDGE,
            ..ring
        });
        Self {
            id,
            rect,
            fill,
            edge,
        }
    }

    pub fn update_render_elements(
        &mut self,
        size: Size<f64, Logical>,
        view_rect: Rectangle<f64, Logical>,
        scale: f64,
    ) {
        let radius = CornerRadius::from(12f32);
        self.fill.update_render_elements(
            size, true, false, false, view_rect, radius, scale, FILL_ALPHA,
        );
        self.edge
            .update_render_elements(size, true, true, false, view_rect, radius, scale, 1.);
    }

    pub fn update_shaders(&mut self) {
        self.fill.update_shaders();
        self.edge.update_shaders();
    }

    pub fn render(
        &self,
        renderer: &mut impl NiriRenderer,
        pos: Point<f64, Logical>,
        push: &mut dyn FnMut(FocusRingRenderElement),
    ) {
        self.edge.render(renderer, pos, push);
        self.fill.render(renderer, pos, push);
    }
}

impl<W: LayoutElement> Layout<W> {
    /// Cancels a resize drag on a canvas: the preview goes and the window keeps its size. False
    /// when no such drag is on.
    pub fn canvas_cancel_resize(&mut self) -> bool {
        let mut cancelled = false;
        for ws in self.workspaces_mut() {
            cancelled |= ws.scrolling_mut().cancel_resize();
        }
        cancelled
    }
}

impl<W: LayoutElement> CanvasSpace<W> {
    /// The whole tiles each dragged edge has moved, outward positive: what the release applies.
    pub(super) fn resize_moves(&self) -> Vec<(Edge, i32)> {
        let Some(resize) = &self.interactive_resize else {
            return Vec::new();
        };
        let (tw, th) = self.canvas.tile_size();
        let tiles = |px: f64, tile: f64| ((px / tile).round() as i32).clamp(-MAX_SPAN, MAX_SPAN);
        let dx = tiles(self.interactive_resize_delta.x, tw);
        let dy = tiles(self.interactive_resize_delta.y, th);
        let edges = resize.data.edges;
        let mut moves = Vec::new();
        if edges.contains(ResizeEdge::LEFT) {
            moves.push((Edge::Left, -dx));
        } else if edges.contains(ResizeEdge::RIGHT) {
            moves.push((Edge::Right, dx));
        }
        if edges.contains(ResizeEdge::TOP) {
            moves.push((Edge::Top, -dy));
        } else if edges.contains(ResizeEdge::BOTTOM) {
            moves.push((Edge::Bottom, dy));
        }
        moves.retain(|&(_, delta)| delta != 0);
        moves
    }

    /// Shows what letting go now would do: the box where the window goes, the others where the
    /// resize pushes them. Recomputed only when the drag crosses into another tile.
    pub(super) fn preview_resize(&mut self) {
        let Some(id) = self
            .interactive_resize
            .as_ref()
            .and_then(|resize| self.id_of(Some(&resize.window)))
        else {
            return;
        };
        let moves = self.resize_moves();
        if self.resize_at.as_ref() == Some(&moves) {
            return;
        }
        let Some(own) = self.canvas.rect(id) else {
            return;
        };
        let mut c = self.canvas.clone();
        for &(edge, delta) in &moves {
            c.resize(id, edge, delta);
        }
        let target = c.rect(id).unwrap_or(own);
        let shown: HashMap<Id, Rect> = if target == own {
            HashMap::new()
        } else {
            c.windows()
                .iter()
                .filter(|w| w.id != id && Some(w.rect) != self.canvas.rect(w.id))
                .map(|w| (w.id, w.rect))
                .collect()
        };
        self.resize_box = if target == own {
            None
        } else {
            match self.resize_box.take() {
                Some(mut b) if b.id == id => {
                    b.rect = target;
                    Some(b)
                }
                _ => Some(ResizeBox::new(id, target, self.options.layout.border)),
            }
        };
        self.show_preview(shown);
        self.resize_at = Some(moves);
    }

    /// Takes the resize preview down, the others gliding back to their own tiles.
    pub(super) fn clear_resize_preview(&mut self) {
        self.resize_box = None;
        if self.resize_at.take().is_some() {
            self.clear_preview();
        }
    }

    /// Cancels the resize drag on this canvas, if one is on: the preview goes, the window keeps
    /// its size, and the cancel is logged.
    pub fn cancel_resize(&mut self) -> bool {
        let Some(resize) = self.interactive_resize.take() else {
            return false;
        };
        self.clear_resize_preview();
        if let Some(idx) = self.idx_of(&resize.window) {
            self.entries[idx]
                .tile
                .window_mut()
                .cancel_interactive_resize();
            let id = self.entries[idx].id;
            let app = self.app_of(id);
            self.logged("cancel", Some(id), app, |_| ());
        }
        true
    }

    /// The box's rectangle in the frame grid tiles render in.
    pub(super) fn resize_box_cell(&self) -> Option<Rectangle<f64, Logical>> {
        let b = self.resize_box.as_ref()?;
        let mut cell = self.cell(b.rect);
        cell.loc -= self.view_point();
        Some(cell)
    }
}

#[cfg(test)]
impl<W: LayoutElement> CanvasSpace<W> {
    /// The box's rectangle and the rectangles the others are previewed on, sorted.
    pub fn resize_preview(&self) -> (Option<Rect>, Vec<Rect>) {
        let mut others: Vec<Rect> = self.preview.values().copied().collect();
        others.sort_by_key(|r| (r.j, r.i));
        (self.resize_box.as_ref().map(|b| b.rect), others)
    }
}
