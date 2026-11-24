//! Floating windows that follow the canvas camera (`floating-follow`).
//!
//! A floating window keeps its place in the floating space. One that follows something has an
//! anchor: where the view drew that reference when the window started following it. The view
//! then draws the window through the map that takes the reference from there to where it is
//! drawn now, so the window keeps its offset to it, scaled as it is. A window that stops
//! following is put where it was drawn, at 1:1.

use niri_ipc::{FloatingFollow, PositionChange};
use smithay::utils::{Logical, Point};

use super::{Follow, Workspace};
use crate::layout::tile::Tile;
use crate::layout::LayoutElement;

/// What a floating window follows.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Reference<Id> {
    /// The canvas, through its camera.
    Canvas,
    /// The tiled window a dialog belongs to, as drawn.
    Parent(Id),
}

/// A point of a reference as the view draws it, and the scale it is drawn at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Drawn {
    pos: Point<f64, Logical>,
    scale: f64,
}

impl Drawn {
    /// The map from where the view drew the reference to where it draws it `now`.
    fn follow_to(self, now: Drawn) -> Follow {
        Follow {
            from: self.pos,
            to: now.pos,
            scale: now.scale / self.scale,
        }
    }
}

#[derive(Debug)]
pub(super) struct Anchor<Id> {
    pub window: Id,
    reference: Reference<Id>,
    /// The reference as drawn when the window started following it.
    at: Drawn,
    /// How the view drew the window at the last sync, for when the reference is gone.
    last: Follow,
}

impl<W: LayoutElement> Workspace<W> {
    /// Where the view draws `reference` now: the canvas's grid origin through its camera, or the
    /// middle of the parent's tile.
    fn drawn(&self, reference: &Reference<W::Id>) -> Option<Drawn> {
        match reference {
            Reference::Canvas => {
                let (pos, scale) = self.scrolling.drawn_origin();
                Some(Drawn { pos, scale })
            }
            Reference::Parent(id) => {
                let (tile, pos, _) = self
                    .scrolling
                    .tiles_with_render_positions()
                    .find(|(tile, _, _)| tile.window().id() == id)?;
                let camera = self.scrolling.tile_camera(id);
                let middle = pos + tile.tile_size().downscale(2.).to_point();
                Some(Drawn {
                    pos: camera.to_view(middle),
                    scale: camera.scale,
                })
            }
        }
    }

    /// The tiled window floating window `id` is a dialog of, directly or through other dialogs.
    fn tiled_ancestor(&self, id: &W::Id) -> Option<W::Id> {
        let mut child = self
            .floating
            .tiles()
            .map(Tile::window)
            .find(|w| w.id() == id)?;
        for _ in 0..16 {
            let mut tiled = self.scrolling.tiles().map(Tile::window);
            if let Some(parent) = tiled.find(|p| child.is_child_of(p)) {
                return Some(parent.id().clone());
            }
            child = self
                .floating
                .tiles()
                .map(Tile::window)
                .find(|p| child.is_child_of(p))?;
        }
        None
    }

    /// What floating window `id` follows in the current mode.
    fn wanted_reference(&self, id: &W::Id) -> Option<Reference<W::Id>> {
        match self.options.floating_follow {
            FloatingFollow::Screen => None,
            FloatingFollow::Canvas => Some(Reference::Canvas),
            FloatingFollow::Parent => self.tiled_ancestor(id).map(Reference::Parent),
        }
    }

    /// How the view draws floating window `id` now, when it follows the canvas.
    pub(super) fn follow_of(&self, id: &W::Id) -> Option<Follow> {
        let anchor = self.follow.iter().find(|a| a.window == *id)?;
        Some(
            self.drawn(&anchor.reference)
                .map_or(anchor.last, |now| anchor.at.follow_to(now)),
        )
    }

    /// Gives each floating window the anchor the mode asks for. A window starts following where
    /// it is drawn, so nothing moves; one that stops following, or whose reference changed or
    /// went, stays where it was drawn last, at 1:1 around its middle.
    pub(super) fn sync_follow(&mut self) {
        let ids: Vec<W::Id> = self
            .floating
            .tiles()
            .map(|tile| tile.window().id().clone())
            .collect();
        let mut anchors = std::mem::take(&mut self.follow);
        anchors.retain(|a| ids.contains(&a.window));

        for id in ids {
            let wanted = self.wanted_reference(&id);
            if let Some(idx) = anchors.iter().position(|a| a.window == id) {
                let anchor = &mut anchors[idx];
                let now = self.drawn(&anchor.reference);
                if let Some(now) = now.filter(|_| Some(&anchor.reference) == wanted.as_ref()) {
                    anchor.last = anchor.at.follow_to(now);
                    continue;
                }
                let anchor = anchors.remove(idx);
                let follow = now.map_or(anchor.last, |now| anchor.at.follow_to(now));
                self.put_where_drawn(&id, follow);
            }
            let Some(reference) = wanted else { continue };
            let Some(at) = self.drawn(&reference) else {
                continue;
            };
            anchors.push(Anchor {
                window: id,
                reference,
                at,
                last: at.follow_to(at),
            });
        }

        self.follow = anchors;
    }

    /// Moves floating window `id` so that its middle is where `follow` draws it.
    fn put_where_drawn(&mut self, id: &W::Id, follow: Follow) {
        let Some((tile, pos)) = self
            .floating
            .tiles_with_offsets()
            .find(|(tile, _)| tile.window().id() == id)
        else {
            return;
        };
        let middle = pos + tile.tile_size().downscale(2.).to_point();
        let delta = follow.to_view(middle) - middle;
        if delta.x.abs() < 1e-9 && delta.y.abs() < 1e-9 {
            return;
        }
        self.floating.move_window(
            Some(id),
            PositionChange::AdjustFixed(delta.x),
            PositionChange::AdjustFixed(delta.y),
            false,
        );
    }
}
