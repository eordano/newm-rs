//! newm's tile canvas in place of the scrolling layout: windows own disjoint rectangles of a tile
//! grid (`newm_canvas::Canvas`), and the view scrolls in both axes. `CanvasSpace` offers the API
//! `Workspace` uses from `ScrollingSpace`.
//!
//! The zoom is a camera. Grid tiles render at 1:1 in their own frame, sized by the base tile
//! whatever the zoom, and are drawn through a [`Camera`] that scales them about the canvas's
//! top-left, the way the overview draws workspaces. A zoom keeps the canvas point under the middle
//! of the screen (`Canvas::zoom_by`), and while the camera's scale moves the view follows it, so
//! every frame of the animation scales about the middle too. Fullscreen and maximized tiles are
//! not on the grid and are drawn without it. While the camera is zoomed out, the pointer only
//! picks windows: a hit is [`HitType::Activate`], so no pointer event reaches a scaled surface.
//! Zoomed in, it does the same unless `magnified-input` is set: the hit then maps the pointer
//! back through the camera ([`HitType::through_magnifier`]), and a click lands where the window
//! is drawn.
//!
//! Rendering and hit testing skip the tiles outside the rectangle of the view the output shows,
//! seen through the camera: the view itself, or in the overview all of the output at overview
//! scale, where the active workspace's windows reach past its view (`Monitor::shown_area`).

mod block;
mod minimap;
mod motion;
mod moves;
mod placeholder;
mod rematch;
mod resize;
pub(crate) mod usage;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use minimap::Minimap;
use motion::{now_ms, Cause, Motion, TELEMETRY};
use newm_canvas::{Canvas, Config as CanvasConfig, Dir, Edge, Id, Rect};
use newm_kde::apps;
use niri_config::utils::MergeWith as _;
use niri_config::PresetSize;
use niri_ipc::{ColumnDisplay, FloatingFollow, SizeChange, WindowLayout};
use placeholder::Placeholder;
use rematch::{title_score, Slot, REMATCH_FOR, TITLE_MATCH};
use resize::ResizeBox;
use serde_json::{json, Value};
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::utils::RescaleRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::output::Output;
use smithay::utils::{Logical, Point, Rectangle, Scale, Serial, Size};

use super::closing_window::{ClosingWindow, ClosingWindowRenderElement};
use super::focus_ring::FocusRingRenderElement;
use super::monitor::InsertPosition;
use super::scrolling::{ColumnWidth, ScrollDirection};
use super::tile::{Tile, TileRenderElement};
use super::workspace::{InteractiveResize, WorkspaceId};
use super::{
    ConfigureIntent, HitType, InteractiveResizeData, Layout, LayoutElement, MonitorSet, Options,
    RemovedTile, RenderLayer,
};
use crate::animation::{Animation, Clock};
use crate::input::swipe_tracker::{SwipeTracker, DECELERATION_TOUCHPAD};
use crate::niri_render_elements;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::xray::XrayPos;
use crate::render_helpers::RenderCtx;
use crate::utils::transaction::{Transaction, TransactionBlocker};
use crate::utils::ResizeEdge;
use crate::window::ResolvedWindowRules;

/// Most tiles one action moves an edge by, so spans stay far from integer overflow.
const MAX_SPAN: i32 = 1024;

/// Touchpad movement that pans the view by the width of the working area.
const VIEW_GESTURE_WORKING_AREA_MOVEMENT: f64 = 1200.;

/// The largest minimum size, in logical pixels, a window's tiles grow to (an app's minimum, or a
/// size it committed), and at most [`MAX_SPAN`] tiles: a client may ask for any i32, and the
/// canvas's tile math (areas, edges) must stay in i32.
const MAX_MIN_PX: f64 = 65535.;

/// Longest the windows a shrink moves wait for the shrinking window to commit its new size: niri's
/// transaction time limit, so a slow or frozen app cannot stall the layout.
const SHRINK_WAIT: Duration = Duration::from_millis(300);

#[derive(Debug)]
pub struct CanvasSpace<W: LayoutElement> {
    entries: Vec<Entry<W>>,
    canvas: Canvas,
    next_id: Id,
    view_offset: ViewOffset,
    interactive_resize: Option<InteractiveResize<W>>,
    interactive_resize_delta: Point<f64, Logical>,
    /// The box a resize drag draws where its window will be (see [`resize`]).
    resize_box: Option<ResizeBox>,
    minimap: Minimap,
    /// The edge moves the resize preview is for.
    resize_at: Option<Vec<(Edge, i32)>>,
    /// The camera's scale on its way to the canvas's after a zoom.
    camera_zoom: Option<CameraZoom>,
    /// The zoom the camera had before [`Self::zoom_to_fit`] pulled it back, which
    /// [`Self::zoom_back`] returns it to.
    zoom_back: Option<i32>,
    /// The tile under the pointer during an interactive move: where a dropped window goes.
    drop_target: Cell<Option<(i32, i32)>>,
    /// Where the windows other than a dragged one are drawn while the drag previews its drop:
    /// the drop state's rectangles, uncommitted.
    preview: HashMap<Id, Rect>,
    /// The dragged window and the tile its preview is for.
    preview_at: Option<(Id, i32, i32)>,
    /// Closing windows, and whether each is drawn through the camera.
    closing_windows: Vec<(ClosingWindow, bool)>,
    /// Windows whose tiles shrank while they are still drawn larger than them. Until each commits
    /// its new size, or `shrink_deadline` passes, the windows any change moves keep their drawn
    /// positions ([`Entry::held`]), so none is drawn over a shrinking window.
    shrinking: Vec<Id>,
    shrink_deadline: Duration,
    placeholders: Vec<Placeholder>,
    /// A saved canvas being put back (see [`Self::restore`]).
    restore: Option<Restore>,
    /// Until when restored windows may trade spots as their titles change (see [`rematch`]).
    rematch_until: Option<Duration>,
    /// Blocks a client placed, the most recent last (see [`Self::place_block`]).
    blocks: Vec<block::Block>,
    /// The last park, for undo to take back in one step (see [`Self::park`]).
    park: Option<block::Park>,
    /// The motion episode under way, for the usage log.
    motion: Motion,
    /// The span the last window of each app closed or moved away from here had, the most recent
    /// last, for at most [`SPAN_MEMORY`] apps: a new window of the app that leaves its size to
    /// nobody opens at it.
    span_memory: Vec<(String, (i32, i32))>,
    /// The window an interactive move drags on this canvas and its rectangle when the drag
    /// began, for the usage log.
    drag: Option<(Id, Rect)>,
    view_size: Size<f64, Logical>,
    working_area: Rectangle<f64, Logical>,
    parent_area: Rectangle<f64, Logical>,
    scale: f64,
    clock: Clock,
    options: Rc<Options>,
}

niri_render_elements! {
    CanvasRenderElement<R> => {
        Tile = TileRenderElement<R>,
        ClosingWindow = ClosingWindowRenderElement,
        Placeholder = FocusRingRenderElement,
        PlaceholderIcon = MemoryRenderBufferRenderElement<R>,
    }
}

niri_render_elements! {
    CanvasSpaceRenderElement<R> => {
        Canvas = CanvasRenderElement<R>,
        Camera = RescaleRenderElement<CanvasRenderElement<R>>,
    }
}

/// A window a saved session puts back on a canvas: the app ID and title it had, and its tiles.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoreWindow {
    pub app_id: String,
    pub title: Option<String>,
    pub rect: Rect,
}

/// A canvas camera's zoom and the bounds and step it zooms by (`zoom-min`, `zoom-max`,
/// `zoom-step`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasZoom {
    pub zoom: i32,
    pub step: f64,
    pub min: i32,
    pub max: i32,
}

/// A canvas from a saved session: its grid windows, the camera's zoom and view (in logical pixels
/// at 1:1) and the index of the focused window.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoreCanvas {
    pub windows: Vec<RestoreWindow>,
    pub zoom: i32,
    pub view: (f64, f64),
    pub focus: Option<usize>,
}

/// A saved canvas being put back: the placeholders still waiting for their windows, and the
/// focus and camera it takes once none waits.
#[derive(Debug)]
struct Restore {
    waiting: Vec<Id>,
    focus: Option<Id>,
    zoom: i32,
    view: (f64, f64),
    deadline: Duration,
}

/// The default width and height window rules give a new window: per axis, `None` when no rule
/// sets it and `Some(None)` when one sets it empty.
pub type RuleSize = (Option<Option<PresetSize>>, Option<Option<PresetSize>>);

/// How the canvas draws its grid tiles: a point `p` of the frame they render in shows at
/// `origin + (p - origin) * scale` in the workspace view. At scale 1 the two frames are one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub origin: Point<f64, Logical>,
    pub scale: f64,
}

impl Camera {
    fn unscaled(origin: Point<f64, Logical>) -> Self {
        Self { origin, scale: 1. }
    }

    pub fn is_zoomed(self) -> bool {
        self.scale != 1.
    }

    pub fn to_view(self, p: Point<f64, Logical>) -> Point<f64, Logical> {
        if self.is_zoomed() {
            self.origin + (p - self.origin).upscale(self.scale)
        } else {
            p
        }
    }

    pub fn from_view(self, p: Point<f64, Logical>) -> Point<f64, Logical> {
        if self.is_zoomed() {
            self.origin + (p - self.origin).downscale(self.scale)
        } else {
            p
        }
    }

    pub fn rect_to_view(self, r: Rectangle<f64, Logical>) -> Rectangle<f64, Logical> {
        Rectangle::new(self.to_view(r.loc), r.size.upscale(self.scale))
    }

    pub fn rect_from_view(self, r: Rectangle<f64, Logical>) -> Rectangle<f64, Logical> {
        Rectangle::new(self.from_view(r.loc), r.size.downscale(self.scale))
    }

    /// The backdrop position for elements drawn through the camera, from the view's.
    fn xray(self, xray: XrayPos) -> XrayPos {
        if !self.is_zoomed() {
            return xray;
        }
        XrayPos {
            pos_in_backdrop: (xray.pos_in_backdrop + self.origin).downscale(self.scale)
                - self.origin,
            zoom: xray.zoom * self.scale,
        }
    }
}

/// The camera's scale moving from `from` to the canvas's: `from` while `anim` is at 1, the
/// canvas's at 0, geometric in between.
#[derive(Debug)]
struct CameraZoom {
    from: f64,
    anim: Animation,
}

/// A tiled window on the canvas.
#[derive(Debug)]
pub struct Entry<W: LayoutElement> {
    tile: Tile<W>,
    id: Id,
    width: ColumnWidth,
    is_full_width: bool,
    is_pending_fullscreen: bool,
    is_pending_maximized: bool,
    /// What the app showed about its width and height (see [`Kept`]).
    kept: (Kept, Kept),
    /// How far from its tile the window stays drawn while a shrink waits (see
    /// [`CanvasSpace::shrinking`]).
    held: Point<f64, Logical>,
    /// A new window's way to the size of its cell, until it gets there (see [`Watch`]).
    watch: Option<Watch>,
    /// The saved spot it took in a restore, while its title may still move it (see [`rematch`]).
    slot: Option<Slot>,
}

/// How a new window gets to the size of its cell, for the usage log's `settle` line: when it was
/// mapped, how many sizes were asked of it, and the last one.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Watch {
    since: Duration,
    configures: u32,
    asked: Option<Size<i32, Logical>>,
}

/// How many apps' last spans a canvas remembers.
const SPAN_MEMORY: usize = 64;

/// How long a new window has to take the size of its cell before the usage log calls it
/// unsettled.
const SETTLE_WAIT: Duration = Duration::from_secs(5);

/// What a grid window's app showed about its size along one axis: the size it kept when asked
/// for a smaller one counts as its minimum beside the one it declares, so its window takes the
/// tiles that hold it and is drawn 1:1 without overlapping its neighbours.
///
/// The kept size falls back to none once the app commits a size smaller than it, in answer to a
/// later request: the declared minimum alone applies again. A lower minimum moves no window (the
/// canvas never shrinks a span by itself), so a span grows when its app refuses the tile and
/// shrinks only when the user shrinks it, and never swings between two spans. An app that answers
/// every request with that request plus the same margin would grow without end, so a refusal by
/// the same margin as the one before keeps the size kept.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Kept {
    /// The window size the app kept when asked for less; 0 when none.
    size: i32,
    /// How far the last settled commit overshot its request; 0 when it did not.
    margin: i32,
}

impl Kept {
    /// After a settled commit of `got` in answer to a request for `asked` (0: the app picks).
    fn after(self, asked: i32, got: i32) -> Self {
        if asked <= 0 {
            self
        } else if got > asked {
            let margin = got - asked;
            let size = if margin == self.margin {
                self.size
            } else {
                got
            };
            Self { size, margin }
        } else if got < self.size {
            Self::default()
        } else {
            Self { margin: 0, ..self }
        }
    }
}

impl<W: LayoutElement> Entry<W> {
    pub fn contains(&self, window: &W::Id) -> bool {
        self.tile.window().id() == window
    }

    pub fn is_pending_maximized(&self) -> bool {
        self.is_pending_maximized
    }

    pub fn is_pending_fullscreen(&self) -> bool {
        self.is_pending_fullscreen
    }

    pub fn tiles(&self) -> impl Iterator<Item = (&Tile<W>, Point<f64, Logical>)> {
        std::iter::once((&self.tile, Point::from((0., 0.))))
    }
}

impl<W: LayoutElement> Column<W> {
    pub fn tiles(&self) -> impl Iterator<Item = (&Tile<W>, Point<f64, Logical>)> {
        std::iter::once((&self.tile, Point::from((0., 0.))))
    }
}

/// A tiled window taken out of a canvas, to move to another workspace.
#[derive(Debug)]
pub struct Column<W: LayoutElement> {
    tile: Tile<W>,
    width: ColumnWidth,
    is_full_width: bool,
}

/// The rendered view minus the canvas's view.
#[derive(Debug)]
pub(super) enum ViewOffset {
    Static,
    Animation {
        from: Point<f64, Logical>,
        anim: Animation,
    },
    /// A pan: the canvas's view follows the fingers.
    Gesture(PanGesture),
}

#[derive(Debug)]
pub(super) struct PanGesture {
    tracker_x: SwipeTracker,
    tracker_y: SwipeTracker,
    pos: (f64, f64),
    is_touchpad: bool,
}

impl ViewOffset {
    pub fn current(&self) -> Point<f64, Logical> {
        match self {
            ViewOffset::Animation { from, anim } => from.upscale(anim.value()),
            _ => Point::from((0., 0.)),
        }
    }

    pub fn is_static(&self) -> bool {
        matches!(self, ViewOffset::Static)
    }

    pub fn is_gesture(&self) -> bool {
        matches!(self, ViewOffset::Gesture(_))
    }

    pub fn is_animation_ongoing(&self) -> bool {
        matches!(self, ViewOffset::Animation { anim, .. } if !anim.is_done())
    }
}

fn round_px(x: f64, scale: f64) -> f64 {
    (x * scale).round() / scale
}

fn canvas_config(options: &Options) -> CanvasConfig {
    let c = &options.layout.canvas;
    CanvasConfig {
        tile_w: c.tile_width,
        tile_h: c.tile_height,
        zoom_step: c.zoom_step,
        zoom_min: c.zoom_min,
        zoom_max: c.zoom_max,
        distance_weight: c.distance_weight,
        diagonal_weight: c.diagonal_weight,
    }
}

fn compute_working_area(
    parent_area: Rectangle<f64, Logical>,
    scale: f64,
    struts: niri_config::Struts,
) -> Rectangle<f64, Logical> {
    let mut working_area = parent_area;

    working_area.size.w = f64::max(0., working_area.size.w - struts.left.0 - struts.right.0);
    working_area.loc.x += struts.left.0;

    working_area.size.h = f64::max(0., working_area.size.h - struts.top.0 - struts.bottom.0);
    working_area.loc.y += struts.top.0;

    let loc = working_area
        .loc
        .to_physical_precise_ceil(scale)
        .to_logical(scale);

    let mut size_diff = (loc - working_area.loc).to_size();
    size_diff.w = f64::min(working_area.size.w, size_diff.w);
    size_diff.h = f64::min(working_area.size.h, size_diff.h);

    working_area.size -= size_diff;
    working_area.loc = loc;

    working_area
}

fn compute_toplevel_bounds(
    border_config: niri_config::Border,
    working_area_size: Size<f64, Logical>,
    gaps: f64,
) -> Size<i32, Logical> {
    let mut border = 0.;
    if !border_config.off {
        border = border_config.width * 2.;
    }

    Size::from((
        f64::max(working_area_size.w - gaps * 2. - border, 1.),
        f64::max(working_area_size.h - gaps * 2. - border, 1.),
    ))
    .to_i32_floor()
}

impl<W: LayoutElement> CanvasSpace<W> {
    pub fn new(
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        scale: f64,
        clock: Clock,
        options: Rc<Options>,
    ) -> Self {
        let working_area = compute_working_area(parent_area, scale, options.layout.struts);
        let mut rv = Self {
            entries: Vec::new(),
            canvas: Canvas::new(canvas_config(&options), 1., 1.),
            next_id: 1,
            view_offset: ViewOffset::Static,
            interactive_resize: None,
            interactive_resize_delta: Point::from((0., 0.)),
            resize_box: None,
            minimap: Minimap::default(),
            resize_at: None,
            camera_zoom: None,
            zoom_back: None,
            drop_target: Cell::new(None),
            preview: HashMap::new(),
            preview_at: None,
            closing_windows: Vec::new(),
            shrinking: Vec::new(),
            shrink_deadline: Duration::ZERO,
            placeholders: Vec::new(),
            restore: None,
            rematch_until: None,
            blocks: Vec::new(),
            park: None,
            motion: Motion::default(),
            span_memory: Vec::new(),
            drag: None,
            view_size,
            working_area,
            parent_area,
            scale,
            clock,
            options,
        };
        let (w, h) = rv.canvas_screen();
        rv.canvas.set_screen(w, h);
        rv
    }

    pub fn update_config(
        &mut self,
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        scale: f64,
        options: Rc<Options>,
    ) {
        for e in &mut self.entries {
            e.tile.update_config(view_size, scale, options.clone());
        }

        self.view_size = view_size;
        self.working_area = compute_working_area(parent_area, scale, options.layout.struts);
        self.parent_area = parent_area;
        self.scale = scale;
        self.options = options;

        let config = canvas_config(&self.options);
        let (w, h) = self.canvas_screen();
        let mins: Vec<(Id, (f64, f64))> = self
            .entries
            .iter()
            .map(|e| (e.id, self.min_of(e)))
            .collect();
        self.logged_with(
            "app-grow",
            None,
            None,
            |c| {
                if *c.config() != config {
                    c.set_config(config);
                }
                c.set_screen(w, h);
                let mut placed = true;
                for (id, (min_w, min_h)) in mins {
                    placed &= c.set_min(id, min_w, min_h);
                }
                placed
            },
            |_, placed| json!({ "cause": "config", "pushed": !placed }),
        );
        self.update_tile_sizes(false, None);
    }

    pub fn update_shaders(&mut self) {
        for e in &mut self.entries {
            e.tile.update_shaders();
        }
        for p in &mut self.placeholders {
            p.update_shaders();
        }
        if let Some(b) = &mut self.resize_box {
            b.update_shaders();
        }
    }

    pub fn advance_animations(&mut self) {
        let _span = tracy_client::span!("CanvasSpace::advance_animations");
        if !self.shrinking.is_empty() && self.clock.now_unadjusted() >= self.shrink_deadline {
            self.shrinking.clear();
            self.release_held();
        }
        if let ViewOffset::Animation { anim, .. } = &self.view_offset
            && anim.is_done()
        {
            self.view_offset = ViewOffset::Static;
        }
        if self
            .camera_zoom
            .as_ref()
            .is_some_and(|zoom| zoom.anim.is_done())
        {
            self.camera_zoom = None;
        }

        for e in &mut self.entries {
            e.tile.advance_animations();
        }

        let now = self.clock.now_unadjusted();
        let expired: Vec<Id> = self
            .placeholders
            .iter()
            .filter(|p| p.deadline <= now)
            .map(|p| p.id)
            .collect();
        if !expired.is_empty() {
            let gone: Vec<String> = self
                .placeholders
                .iter()
                .filter(|p| p.deadline <= now)
                .map(|p| apps::canonical(&p.app_id))
                .collect();
            for app in gone {
                self.emit(json!({ "t": now_ms(), "op": "restore-expired", "app": app }));
            }
            self.placeholders.retain(|p| p.deadline > now);
            if let Some(restore) = &mut self.restore {
                restore.waiting.retain(|id| !expired.contains(id));
            }
            self.block_slots_expired(&expired);
            self.cause(Cause::new("placeholder"));
            self.apply(|c| {
                for id in expired {
                    c.remove(id);
                }
            });
            self.update_tile_sizes(true, None);
        }
        self.finish_restore();
        self.settle_blocks();
        self.watch_unsettled(now);

        self.closing_windows.retain_mut(|(closing, _)| {
            closing.advance_animations();
            closing.are_animations_ongoing()
        });
        self.sample_motion();
        self.minimap_watch();
    }

    pub fn are_animations_ongoing(&self) -> bool {
        self.view_offset.is_animation_ongoing()
            || self.minimap_shown()
            || self.camera_zoom.is_some()
            || self.entries.iter().any(|e| e.tile.are_animations_ongoing())
            || !self.closing_windows.is_empty()
            || !self.placeholders.is_empty()
            || !self.shrinking.is_empty()
    }

    pub fn are_transitions_ongoing(&self) -> bool {
        !self.view_offset.is_static()
            || self.camera_zoom.is_some()
            || self
                .entries
                .iter()
                .any(|e| e.tile.are_transitions_ongoing())
            || !self.closing_windows.is_empty()
    }

    pub fn update_render_elements(&mut self, is_active: bool, layer: RenderLayer) {
        let _span = tracy_client::span!("CanvasSpace::update_render_elements");
        let active = self.canvas.focus();
        let positions = self.positions(false);
        let views: Vec<_> = self
            .entries
            .iter()
            .map(|e| self.visible_area(&e.tile))
            .collect();
        for ((e, pos), view) in self.entries.iter_mut().zip(positions).zip(views) {
            if layer.is_normal() == e.tile.is_moving_between_workspaces() {
                continue;
            }

            let is_active = is_active && Some(e.id) == active;
            e.tile
                .update_render_elements(is_active, Rectangle::new(view.loc - pos, view.size));
        }
        if !layer.is_normal() {
            return;
        }
        self.minimap_update();
        let cells = self.placeholder_cells();
        let view = self
            .camera()
            .rect_from_view(Rectangle::from_size(self.view_size));
        let scale = self.scale;
        for (p, cell) in self.placeholders.iter_mut().zip(cells) {
            p.update_render_elements(
                cell.size,
                Rectangle::new(view.loc - cell.loc, view.size),
                scale,
            );
        }
        let cell = self.resize_box_cell();
        if let (Some(b), Some(cell)) = (&mut self.resize_box, cell) {
            b.update_render_elements(
                cell.size,
                Rectangle::new(view.loc - cell.loc, view.size),
                scale,
            );
        }
    }

    pub fn tiles(&self) -> impl Iterator<Item = &Tile<W>> + '_ {
        self.entries.iter().map(|e| &e.tile)
    }

    pub fn tiles_mut(&mut self) -> impl Iterator<Item = &mut Tile<W>> + '_ {
        self.entries.iter_mut().map(|e| &mut e.tile)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn active_idx(&self) -> Option<usize> {
        let id = self.canvas.focus()?;
        self.entries.iter().position(|e| e.id == id)
    }

    fn idx_of(&self, window: &W::Id) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.tile.window().id() == window)
    }

    fn id_of(&self, window: Option<&W::Id>) -> Option<Id> {
        match window {
            Some(window) => self.idx_of(window).map(|idx| self.entries[idx].id),
            None => self.canvas.focus(),
        }
    }

    pub fn active_window(&self) -> Option<&W> {
        self.active_idx().map(|idx| self.entries[idx].tile.window())
    }

    pub fn active_window_mut(&mut self) -> Option<&mut W> {
        let idx = self.active_idx()?;
        Some(self.entries[idx].tile.window_mut())
    }

    pub fn active_tile_mut(&mut self) -> Option<&mut Tile<W>> {
        let idx = self.active_idx()?;
        Some(&mut self.entries[idx].tile)
    }

    pub fn is_active_pending_fullscreen(&self) -> bool {
        self.active_idx()
            .is_some_and(|idx| self.entries[idx].is_pending_fullscreen)
    }

    pub fn new_window_toplevel_bounds(&self, rules: &ResolvedWindowRules) -> Size<i32, Logical> {
        let border_config = self.options.layout.border.merged_with(&rules.border);
        compute_toplevel_bounds(
            border_config,
            self.working_area.size,
            self.options.layout.gaps,
        )
    }

    /// The size a new window's first configure asks for, at 1:1: on an axis a window rule sizes
    /// (`default-column-width`, `default-window-height`), that span's cell; elsewhere 0, so the
    /// app picks its size there and [`Self::insert`] rounds it up to whole tiles at map.
    pub fn new_window_size(
        &self,
        _width: Option<PresetSize>,
        _height: Option<PresetSize>,
        rules: &ResolvedWindowRules,
    ) -> Size<i32, Logical> {
        let border = self.options.layout.border.merged_with(&rules.border);
        let rule = |size: Option<Option<PresetSize>>, horizontal| {
            size.flatten().map(|s| self.preset_span(s, horizontal))
        };
        let (rw, rh) = (
            rule(rules.default_width, true),
            rule(rules.default_height, false),
        );
        let (w, h) = self
            .canvas
            .open_span(0., 0., Some((rw.unwrap_or(1), rh.unwrap_or(1))));
        let mut size = self.cell(Rect::new(0, 0, w, h)).size;
        if !border.off {
            size.w -= border.width * 2.;
            size.h -= border.width * 2.;
        }
        let px = |n: f64, asked: bool| {
            if asked {
                f64::max(n.floor(), 1.) as i32
            } else {
                0
            }
        };
        Size::from((px(size.w, rw.is_some()), px(size.h, rh.is_some())))
    }

    pub fn is_centering_focused_column(&self) -> bool {
        false
    }

    /// Tiles a default size asks for along one axis, at 1:1 whatever the zoom: the nearest whole
    /// number of tiles, at least one, to a fixed size or to a proportion of the canvas's screen.
    fn preset_span(&self, size: PresetSize, horizontal: bool) -> i32 {
        let (tw, th) = self.canvas.base_tile();
        let (sw, sh) = self.canvas.screen();
        let (tile, screen) = if horizontal { (tw, sw) } else { (th, sh) };
        let n = match size {
            PresetSize::Fixed(px) => f64::from(px) / tile,
            PresetSize::Proportion(p) => p * screen / tile,
        };
        (n.round() as i32).clamp(1, MAX_SPAN)
    }

    /// The span a new grid window asks to open at, if it asks: on each axis the size its window
    /// rule sets (`default-column-width`, `default-window-height`; an empty one asks for none),
    /// else the canvas's `default-width` / `default-height`; one tile on an axis nothing sizes.
    fn wanted_span(
        &self,
        width: Option<Option<PresetSize>>,
        height: Option<Option<PresetSize>>,
    ) -> Option<(i32, i32)> {
        let canvas = &self.options.layout.canvas;
        let width = width.unwrap_or(canvas.default_width);
        let height = height.unwrap_or(canvas.default_height);
        (width.is_some() || height.is_some()).then(|| {
            (
                width.map_or(1, |w| self.preset_span(w, true)),
                height.map_or(1, |h| self.preset_span(h, false)),
            )
        })
    }

    /// Gap between a window and its grid lines, on physical pixels.
    fn inset(&self) -> f64 {
        round_px(self.options.layout.gaps / 2., self.scale)
    }

    /// Where grid line 0 sits when the view is at 0, and the point the camera scales about.
    fn canvas_origin(&self) -> Point<f64, Logical> {
        self.working_area.loc + Point::from((self.inset(), self.inset()))
    }

    /// The camera at the scale it draws at now, on the way to the canvas's after a zoom.
    pub fn camera(&self) -> Camera {
        let target = self.canvas.scale();
        let scale = match &self.camera_zoom {
            Some(zoom) => target * (zoom.from / target).powf(zoom.anim.value()),
            None => target,
        };
        Camera {
            origin: self.canvas_origin(),
            scale,
        }
    }

    /// The camera at the canvas's scale, where a zoom animation ends.
    fn target_camera(&self) -> Camera {
        Camera {
            origin: self.canvas_origin(),
            scale: self.canvas.scale(),
        }
    }

    /// The camera `tile` is drawn through, out of `camera`: none for a fullscreen or maximized
    /// tile, which is not on the grid.
    fn camera_of(&self, tile: &Tile<W>, camera: Camera) -> Camera {
        if tile.sizing_mode().is_normal() {
            camera
        } else {
            Camera::unscaled(camera.origin)
        }
    }

    /// The camera `window`'s tile is drawn through now.
    pub fn tile_camera(&self, window: &W::Id) -> Camera {
        match self.idx_of(window) {
            Some(idx) => self.camera_of(&self.entries[idx].tile, self.camera()),
            None => Camera::unscaled(self.canvas_origin()),
        }
    }

    /// Where the view draws the canvas's grid origin now, and the camera's scale: what a
    /// floating window that follows the canvas moves and scales with.
    pub fn drawn_origin(&self) -> (Point<f64, Logical>, f64) {
        let camera = self.camera();
        (
            camera.to_view(Point::from((0., 0.)) - self.view_point()),
            camera.scale,
        )
    }

    /// The part of `tile`'s frame that `shown`, a rectangle of the view, holds.
    fn area_shown(
        &self,
        tile: &Tile<W>,
        shown: Rectangle<f64, Logical>,
    ) -> Rectangle<f64, Logical> {
        self.camera_of(tile, self.camera()).rect_from_view(shown)
    }

    /// The part of `tile`'s frame the view shows.
    fn visible_area(&self, tile: &Tile<W>) -> Rectangle<f64, Logical> {
        self.area_shown(tile, Rectangle::from_size(self.view_size))
    }

    fn canvas_screen(&self) -> (f64, f64) {
        let inset = self.inset();
        (
            f64::max(self.working_area.size.w - inset * 2., 1.),
            f64::max(self.working_area.size.h - inset * 2., 1.),
        )
    }

    /// Where a rectangle's tile goes, in canvas logical pixels at 1:1: both sides on the
    /// physical pixel of their grid line, inset by half the gaps.
    fn cell(&self, r: Rect) -> Rectangle<f64, Logical> {
        let (tw, th) = self.canvas.base_tile();
        let scale = self.scale;
        let ex = |k: i32| round_px(f64::from(k) * tw, scale);
        let ey = |k: i32| round_px(f64::from(k) * th, scale);
        let inset = self.inset();
        let (x0, x1, y0, y1) = (ex(r.i), ex(r.right()), ey(r.j), ey(r.bottom()));
        Rectangle::new(
            Point::from((x0 + inset, y0 + inset)),
            Size::from((
                f64::max(x1 - x0 - inset * 2., 1.),
                f64::max(y1 - y0 - inset * 2., 1.),
            )),
        )
    }

    /// The 1:1 canvas point the camera at `scale` shows at the canvas origin, minus that origin,
    /// so that `cell.loc - view_point` is in the frame grid tiles render in. The canvas's view is
    /// in on-screen pixels at its zoom. While the camera's scale differs from the canvas's (a zoom
    /// animating), the point moves with it so that the 1:1 point under the middle of the screen
    /// stays `(view + offset + screen / 2) / canvas scale`, as `Canvas::zoom_by` keeps it.
    fn view_point_with(&self, offset: Point<f64, Logical>, scale: f64) -> Point<f64, Logical> {
        let (vx, vy) = self.canvas.view();
        let (sw, sh) = self.canvas.screen();
        let target = self.canvas.scale();
        let follow = 1. - target / scale;
        let view = (Point::from((vx + sw / 2. * follow, vy + sh / 2. * follow)) + offset)
            .downscale(target)
            .to_physical_precise_round(self.scale)
            .to_logical(self.scale);
        view - self.canvas_origin()
    }

    fn view_point(&self) -> Point<f64, Logical> {
        self.view_point_with(self.view_offset.current(), self.camera().scale)
    }

    fn target_view_point(&self) -> Point<f64, Logical> {
        self.view_point_with(Point::from((0., 0.)), self.canvas.scale())
    }

    /// The tile's position in the frame it renders in, without its render offset.
    fn entry_pos(&self, e: &Entry<W>, view_point: Point<f64, Logical>) -> Point<f64, Logical> {
        let mode = e.tile.sizing_mode();
        if mode.is_fullscreen() {
            Point::from((0., 0.))
        } else if mode.is_maximized() {
            self.parent_area.loc
        } else {
            let r = self.shown_rect(e.id).unwrap();
            self.cell(r).loc - view_point
        }
    }

    /// Where window `id` is drawn: its preview rectangle during a drag's preview, else its own.
    fn shown_rect(&self, id: Id) -> Option<Rect> {
        self.preview
            .get(&id)
            .copied()
            .or_else(|| self.canvas.rect(id))
    }

    /// Render positions in entry order, each in the frame its tile renders in.
    fn positions(&self, round: bool) -> Vec<Point<f64, Logical>> {
        let _span = tracy_client::span!("CanvasSpace::positions");
        let view_point = self.view_point();
        self.entries
            .iter()
            .map(|e| {
                let pos = self.entry_pos(e, view_point) + e.tile.render_offset() + e.held;
                if round {
                    pos.to_physical_precise_round(self.scale)
                        .to_logical(self.scale)
                } else {
                    pos
                }
            })
            .collect()
    }

    /// Placeholder rectangles in the frame grid tiles render in.
    fn placeholder_cells(&self) -> Vec<Rectangle<f64, Logical>> {
        let view_point = self.view_point();
        self.placeholders
            .iter()
            .map(|p| {
                let mut cell = self.cell(self.canvas.rect(p.id).unwrap());
                cell.loc -= view_point;
                cell
            })
            .collect()
    }

    /// Reserves the spot the next window would take, at the span the default size its rules
    /// give it asks for (see [`Self::wanted_span`]), and draws a placeholder there until a window
    /// with `app_id` maps (it takes the spot) or [`placeholder::LIFETIME`] passes.
    pub fn add_placeholder(
        &mut self,
        app_id: String,
        icon: Option<String>,
        (width, height): RuleSize,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        let prev = self.canvas.focus();
        let want = self.wanted_span(width, height);
        self.apply_for("placeholder", |c| {
            c.add(id, 0., 0., want);
            if let Some(prev) = prev {
                c.focus_id(prev);
            }
        });
        let now = self.clock.now_unadjusted();
        let deadline = now + placeholder::LIFETIME;
        let focus_ring = self.options.layout.focus_ring;
        self.placeholders.push(Placeholder::new(
            id,
            app_id,
            None,
            icon.as_deref(),
            focus_ring,
            deadline,
            now,
        ));
        self.update_tile_sizes(true, None);
    }

    /// Puts a saved canvas back: a placeholder on each window's saved tiles, or the free ones
    /// nearest them, which the next window with its app ID takes (of several, the one with its
    /// title first). Once no placeholder waits, because windows took them or `lifetime` passed and
    /// dropped the rest, the saved window is focused and the camera takes the saved zoom and view.
    /// A second saved canvas on the same canvas keeps the first one's camera.
    pub fn restore(&mut self, saved: RestoreCanvas, lifetime: Duration) {
        let now = self.clock.now_unadjusted();
        let deadline = now + lifetime;
        self.rematch_until = Some(now + lifetime.max(REMATCH_FOR));
        let prev = self.canvas.focus();
        let ids: Vec<Id> = (0..saved.windows.len() as Id)
            .map(|k| self.next_id + k)
            .collect();
        self.next_id += saved.windows.len() as Id;
        self.apply_for("restore", |c| {
            for (&id, w) in ids.iter().zip(&saved.windows) {
                c.add_at(id, 0., 0., w.rect);
            }
            if let Some(prev) = prev {
                c.focus_id(prev);
            }
        });
        let focus_ring = self.options.layout.focus_ring;
        for (&id, w) in ids.iter().zip(saved.windows) {
            self.placeholders.push(Placeholder::new(
                id, w.app_id, w.title, None, focus_ring, deadline, now,
            ));
        }
        let focus = saved.focus.and_then(|k| ids.get(k).copied());
        if let Some(restore) = &mut self.restore {
            restore.waiting.extend(ids);
            restore.focus = restore.focus.or(focus);
            restore.deadline = restore.deadline.max(deadline);
        } else {
            self.restore = Some(Restore {
                waiting: ids,
                focus,
                zoom: saved.zoom,
                view: saved.view,
                deadline,
            });
        }
        self.update_tile_sizes(true, None);
        self.finish_restore();
    }

    /// Whether a saved canvas is being put back: some of its placeholders wait for windows.
    pub fn is_restoring(&self) -> bool {
        self.restore.is_some()
    }

    /// Whether a placeholder waits for a window of the app `app_id` names (see [`apps`]).
    pub fn has_placeholder(&self, app_id: &str) -> bool {
        self.placeholders
            .iter()
            .any(|p| apps::same(&p.app_id, app_id))
    }

    /// Ends a restore once no placeholder waits or its deadline passed: focuses the saved window
    /// and sets the saved zoom and view, moved the least it must to show the focus.
    fn finish_restore(&mut self) {
        let now = self.clock.now_unadjusted();
        let Some(restore) = self
            .restore
            .take_if(|r| r.waiting.is_empty() || r.deadline <= now)
        else {
            return;
        };
        self.apply_for("restore", |c| {
            if let Some(focus) = restore.focus {
                c.focus_id(focus);
            }
            c.zoom_by(restore.zoom - c.zoom());
            let scale = c.scale();
            c.set_view(restore.view.0 * scale, restore.view.1 * scale);
            c.ensure_focus_visible();
        });
        self.update_tile_sizes(true, None);
    }

    /// The grid windows with their tiles, the oldest first.
    pub fn grid_windows(&self) -> impl Iterator<Item = (&W, Rect)> + '_ {
        self.entries
            .iter()
            .filter_map(|e| Some((e.tile.window(), self.canvas.rect(e.id)?)))
    }

    /// Entry indices top to bottom: the focused tile, then fullscreen and maximized ones.
    fn render_order(&self) -> Vec<usize> {
        let active = self.active_idx();
        let mut order: Vec<usize> = active.into_iter().collect();
        let raised = |e: &Entry<W>| !e.tile.sizing_mode().is_normal();
        for pass in [true, false] {
            for (idx, e) in self.entries.iter().enumerate() {
                if Some(idx) != active && raised(e) == pass {
                    order.push(idx);
                }
            }
        }
        order
    }

    /// Whether `tile`, at `pos` in the frame it renders in, shows in `shown`, a rectangle of the
    /// view.
    fn is_shown(
        &self,
        tile: &Tile<W>,
        pos: Point<f64, Logical>,
        shown: Rectangle<f64, Logical>,
    ) -> bool {
        Rectangle::new(pos, tile.tile_size()).overlaps(self.area_shown(tile, shown))
    }

    fn is_visible(&self, tile: &Tile<W>, pos: Point<f64, Logical>) -> bool {
        self.is_shown(tile, pos, Rectangle::from_size(self.view_size))
    }

    /// The entries [`Self::render`] draws for `shown`, top to bottom, at their render positions:
    /// those `shown` holds and those fading in or out.
    fn drawn(&self, shown: Rectangle<f64, Logical>) -> Vec<(usize, Point<f64, Logical>)> {
        let _span = tracy_client::span!("CanvasSpace::drawn");
        let positions = self.positions(true);
        self.render_order()
            .into_iter()
            .map(|idx| (idx, positions[idx]))
            .filter(|&(idx, pos)| {
                let tile = &self.entries[idx].tile;
                self.is_shown(tile, pos, shown) || tile.alpha_animation.is_some()
            })
            .collect()
    }

    /// The windows [`Self::render`] draws for `shown`, top to bottom.
    #[cfg(test)]
    pub fn drawn_windows(&self, shown: Rectangle<f64, Logical>) -> Vec<W::Id> {
        self.drawn(shown)
            .into_iter()
            .map(|(idx, _)| self.entries[idx].tile.window().id().clone())
            .collect()
    }

    /// Runs a canvas operation, then animates the tiles that moved, the view and, when the
    /// zoom changed, the camera's scale. A tile that moves while a shrink waits keeps its drawn
    /// position (see [`Self::shrinking`]). Moves that come with a resize follow the resize
    /// animation: a window grows only once it commits its new size, after the windows it pushes
    /// started to move out of its way along the same curve, so it is never drawn over them.
    fn apply<R>(&mut self, op: impl FnOnce(&mut Canvas) -> R) -> R {
        let _span = tracy_client::span!("CanvasSpace::apply");
        if !self.preview.is_empty() {
            self.clear_preview();
        }
        let drawn = self.motion_before();
        let before: Vec<(Id, Rect, Point<f64, Logical>)> = self
            .entries
            .iter()
            .filter_map(|e| {
                let r = self.canvas.rect(e.id)?;
                Some((e.id, r, self.cell(r).loc))
            })
            .collect();
        let (vx, vy) = self.canvas.view();
        let rendered = Point::from((vx, vy)) + self.view_offset.current();
        let (scale, target) = (self.camera().scale, self.canvas.scale());

        let rv = op(&mut self.canvas);

        let new_target = self.canvas.scale();
        let mut started = new_target != target;
        let rendered = if new_target == target {
            rendered
        } else {
            let config = self.options.animations.horizontal_view_movement.0;
            let anim = Animation::new(self.clock.clone(), 1., 0., 0., config);
            self.camera_zoom = Some(CameraZoom { from: scale, anim });
            let (x, y) = self
                .canvas
                .centred_view((rendered.x, rendered.y), target, new_target);
            Point::from((x, y))
        };

        let mut resized = false;
        for &(id, old, _) in &before {
            let Some(r) = self.canvas.rect(id) else {
                continue;
            };
            resized |= r.w != old.w || r.h != old.h;
            if (r.w < old.w || r.h < old.h)
                && !self.shrinking.contains(&id)
                && self
                    .entries
                    .iter()
                    .find(|e| e.id == id)
                    .is_some_and(|e| !self.fits_cell(e))
            {
                self.shrinking.push(id);
                self.shrink_deadline = self.clock.now_unadjusted() + SHRINK_WAIT;
            }
        }
        let hold = !self.shrinking.is_empty();
        let config = if resized {
            self.options.animations.window_resize.anim
        } else {
            self.options.animations.window_movement.0
        };
        for (id, _, old) in before {
            let Some(r) = self.canvas.rect(id) else {
                continue;
            };
            let new = self.cell(r).loc;
            if old != new
                && let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
            {
                started = true;
                if hold {
                    e.held += old - new;
                } else {
                    e.tile
                        .animate_move_x_from_with_config(old.x - new.x, config);
                    e.tile
                        .animate_move_y_from_with_config(old.y - new.y, config);
                }
            }
        }

        if !self.view_offset.is_gesture() {
            let (vx, vy) = self.canvas.view();
            let from = rendered - Point::from((vx, vy));
            if from.x.abs() < 0.5 / self.scale && from.y.abs() < 0.5 / self.scale {
                if !self.view_offset.is_animation_ongoing() {
                    self.view_offset = ViewOffset::Static;
                }
            } else {
                let config = self.options.animations.horizontal_view_movement.0;
                let anim = Animation::new(self.clock.clone(), 1., 0., 0., config);
                self.view_offset = ViewOffset::Animation { from, anim };
                started = true;
            }
        }

        self.motion_after(drawn, started);
        if self.interactive_resize.is_some() {
            // The canvas changed under a resize drag: preview what letting go does now.
            self.resize_at = None;
            self.preview_resize();
        }
        rv
    }

    /// [`Self::apply`] for an op of `kind` in the usage log's motion episodes.
    fn apply_for<R>(&mut self, kind: &'static str, op: impl FnOnce(&mut Canvas) -> R) -> R {
        self.cause(Cause::new(kind));
        self.apply(op)
    }

    /// The window's minimum for the canvas: the larger of its app's declared minimum and the
    /// size it kept, with its border, gaps and a pixel for rounding.
    fn min_of(&self, e: &Entry<W>) -> (f64, f64) {
        let min = e.tile.min_size_nonfullscreen();
        let kept_w = e.tile.tile_width_for_window_width(f64::from(e.kept.0.size));
        let kept_h = e
            .tile
            .tile_height_for_window_height(f64::from(e.kept.1.size));
        let pad = self.inset() * 2. + 1. / f64::min(self.scale, 1.);
        let (cap_w, cap_h) = self.min_cap();
        (
            min.w.max(kept_w).clamp(1., cap_w) + pad,
            min.h.max(kept_h).clamp(1., cap_h) + pad,
        )
    }

    /// The largest tile size, per axis, a window's minimum grows its tiles to ([`MAX_MIN_PX`], at
    /// most [`MAX_SPAN`] tiles): a tile minimum past it is asked for less than it wants.
    pub(super) fn min_cap(&self) -> (f64, f64) {
        let (tw, th) = self.canvas.base_tile();
        let cap = |tile: f64| MAX_MIN_PX.min(f64::from(MAX_SPAN) * tile);
        (cap(tw), cap(th))
    }

    /// Takes in a grid window's size when it is its answer to the last size asked of it, and
    /// returns whether it was (see [`Kept`]).
    fn observe_size(&mut self, idx: usize) -> bool {
        let e = &mut self.entries[idx];
        let win = e.tile.window();
        if e.is_pending_fullscreen
            || e.is_pending_maximized
            || !win.sizing_mode().is_normal()
            || !win.pending_sizing_mode().is_normal()
            || !win.is_size_settled()
        {
            return false;
        }
        let Some(asked) = win.requested_size() else {
            return false;
        };
        let got = win.size();
        e.kept = (
            e.kept.0.after(asked.w, got.w),
            e.kept.1.after(asked.h, got.h),
        );
        true
    }

    /// Whether the tile is drawn within the cell its rectangle gives it.
    fn fits_cell(&self, e: &Entry<W>) -> bool {
        let Some(r) = self.canvas.rect(e.id) else {
            return true;
        };
        if !e.tile.sizing_mode().is_normal() {
            return true;
        }
        let cell = self.cell(r).size;
        let size = e.tile.tile_size();
        size.w <= cell.w + 0.5 && size.h <= cell.h + 0.5
    }

    /// The shrinking window `id` committed its new size or left: once none is left waiting, the
    /// held windows move into place.
    fn shrink_done(&mut self, id: Id) {
        let waiting = !self.shrinking.is_empty();
        self.shrinking.retain(|&s| s != id);
        if waiting && self.shrinking.is_empty() {
            self.release_held();
        }
    }

    /// Moves the held windows to their tiles along the resize animation, which a shrinking
    /// window's commit starts at the same moment, so they close the gap in step with it.
    fn release_held(&mut self) {
        let config = self.options.animations.window_resize.anim;
        for e in &mut self.entries {
            let held = std::mem::replace(&mut e.held, Point::from((0., 0.)));
            if held != Point::from((0., 0.)) {
                e.tile.animate_move_x_from_with_config(held.x, config);
                e.tile.animate_move_y_from_with_config(held.y, config);
            }
        }
    }

    fn update_tile_sizes(&mut self, animate: bool, transaction: Option<Transaction>) {
        let _span = tracy_client::span!("CanvasSpace::update_tile_sizes");
        let transaction = transaction.unwrap_or_else(Transaction::new);
        let parent_size = self.parent_area.size;
        let cells: Vec<_> = self
            .entries
            .iter()
            .map(|e| self.cell(self.canvas.rect(e.id).unwrap()))
            .collect();
        for (e, cell) in self.entries.iter_mut().zip(cells) {
            let transaction = Some(transaction.clone());
            if e.is_pending_fullscreen {
                e.tile.request_fullscreen(animate, transaction);
            } else if e.is_pending_maximized {
                e.tile.request_maximized(parent_size, animate, transaction);
            } else {
                e.tile.request_tile_size(cell.size, animate, transaction);
            }
            if let Some(w) = &mut e.watch {
                let asked = e.tile.window().requested_size();
                if asked != w.asked {
                    w.configures += 1;
                    w.asked = asked;
                }
            }
        }
    }

    fn cancel_resize_for(&mut self, idx: usize) {
        if self
            .interactive_resize
            .as_ref()
            .is_some_and(|resize| &resize.window == self.entries[idx].tile.window().id())
        {
            self.interactive_resize = None;
            self.clear_resize_preview();
        }
        self.entries[idx]
            .tile
            .window_mut()
            .cancel_interactive_resize();
    }

    /// Remembers the tile under `pos` (relative to the view) as the drop target of an
    /// interactive move.
    pub(super) fn insert_position(&self, pos: Point<f64, Logical>) -> InsertPosition {
        let origin = self.working_area.loc + Point::from((self.inset(), self.inset()));
        let pos = pos - origin;
        self.drop_target
            .set(Some(self.canvas.tile_at(pos.x, pos.y)));
        InsertPosition::NewColumn(0)
    }

    pub fn add_tile(
        &mut self,
        col_idx: Option<usize>,
        tile: Tile<W>,
        activate: bool,
        width: ColumnWidth,
        is_full_width: bool,
        _anim_config: Option<niri_config::Animation>,
    ) {
        let drop_target = self.drop_target.take().filter(|_| col_idx.is_some());
        let id = self.next_id;
        self.insert(tile, None, activate, width, is_full_width);
        if let Some((i, j)) = drop_target {
            let app = self.app_of(id);
            self.logged("drop", Some(id), app, |c| c.move_to(id, i, j));
            self.update_tile_sizes(true, None);
        }
    }

    fn insert(
        &mut self,
        mut tile: Tile<W>,
        next_to: Option<Id>,
        activate: bool,
        width: ColumnWidth,
        is_full_width: bool,
    ) {
        let _span = tracy_client::span!("CanvasSpace::insert");
        tile.update_config(self.view_size, self.scale, self.options.clone());
        let id = self.next_id;
        self.next_id += 1;
        let open = self.arrival_span(&mut tile);
        let is_pending_fullscreen = tile.window().pending_sizing_mode().is_fullscreen();
        let is_pending_maximized = tile.window().pending_sizing_mode().is_maximized();
        let asked = tile.window().requested_size();
        let watch = (TELEMETRY && !open.carried).then(|| Watch {
            since: self.clock.now_unadjusted(),
            configures: u32::from(asked.is_some()),
            asked,
        });
        self.entries.push(Entry {
            tile,
            id,
            width,
            is_full_width,
            is_pending_fullscreen,
            is_pending_maximized,
            kept: Default::default(),
            held: Point::from((0., 0.)),
            watch,
            slot: None,
        });
        let idx = self.entries.len() - 1;
        self.observe_size(idx);
        let (min_w, min_h) = self.min_of(&self.entries[idx]);
        let prev = self.canvas.focus();
        let window = self.entries.last().unwrap().tile.window();
        let (app_id, title) = (window.app_id(), window.title());
        // Of the placeholders of its app, the one whose saved title fits its title best, the
        // earliest of equals.
        // A tile carrying its span from another canvas is a window that was open already, not
        // the one a placeholder waits for.
        let taken = app_id.filter(|_| !open.carried).and_then(|app_id| {
            let (k, score) = self
                .placeholders
                .iter()
                .enumerate()
                .filter(|(_, p)| apps::same(&p.app_id, &app_id))
                .map(|(k, p)| (k, title_score(p.title.as_deref(), title.as_deref())))
                .fold(None, |best: Option<(usize, f64)>, (k, s)| match best {
                    Some((_, b)) if b >= s => best,
                    _ => Some((k, s)),
                })?;
            let matched_by = if score >= TITLE_MATCH { "title" } else { "app" };
            let p = self.placeholders.remove(k);
            let slot = Slot {
                app_id: p.app_id,
                title: p.title,
            };
            Some((p.id, matched_by, p.created, slot))
        });
        if let Some((.., slot)) = &taken {
            self.entries[idx].slot = Some(slot.clone());
        }
        let placeholder = taken.as_ref().map(|t| t.0);
        // A block's window waits for the block to be complete to take the focus.
        let activate = activate && !placeholder.is_some_and(|p| self.is_block_slot(p));
        if let Some(placeholder) = placeholder {
            self.block_slot_taken(placeholder, id);
        }
        if let (Some(restore), Some(placeholder)) = (&mut self.restore, placeholder) {
            restore.waiting.retain(|&w| w != placeholder);
            if restore.focus == Some(placeholder) {
                restore.focus = Some(id);
            }
        }
        let app = self.app_of(id);
        let now = self.clock.now_unadjusted();
        let mut fields = serde_json::Map::new();
        fields.insert(
            "size_source".into(),
            if placeholder.is_some() {
                json!("placeholder")
            } else {
                open.source
            },
        );
        fields.insert("app_px".into(), json!(open.app_px));
        fields.insert("since_last_open_ms".into(), json!(usage::since_last_open()));
        if let Some((_, matched_by, created, _)) = taken {
            let waited_ms = now.saturating_sub(created).as_millis() as u64;
            fields.insert(
                "restore".into(),
                json!({ "matched_by": matched_by, "waited_ms": waited_ms }),
            );
        }
        let want = Some(open.span);
        let open_fields = move |s: &Self, _: &()| {
            fields.insert("span".into(), json!(s.canvas.rect(id).map(|r| [r.w, r.h])));
            fields.insert("focused".into(), json!(s.canvas.focus() == Some(id)));
            Value::Object(fields)
        };
        self.logged_with(
            "open",
            Some(id),
            app,
            |c| match placeholder {
                Some(placeholder) => {
                    c.rename(placeholder, id);
                    c.set_min(id, min_w, min_h);
                    if activate || prev == Some(placeholder) {
                        c.focus_id(id);
                    }
                }
                None => {
                    if let Some(next_to) = next_to {
                        c.focus_id(next_to);
                    }
                    c.add(id, min_w, min_h, want);
                    if !activate && let Some(prev) = prev {
                        c.focus_id(prev);
                    }
                }
            },
            open_fields,
        );
        self.update_tile_sizes(true, None);
        self.finish_restore();
        self.settle_blocks();
        self.check_settled(id);
    }

    pub fn add_tile_to_column(
        &mut self,
        _col_idx: usize,
        _tile_idx: Option<usize>,
        tile: Tile<W>,
        activate: bool,
    ) {
        let width = ColumnWidth::Fixed(tile.tile_expected_or_current_size().w);
        self.insert(tile, None, activate, width, false);
    }

    pub fn add_tile_right_of(
        &mut self,
        right_of: &W::Id,
        tile: Tile<W>,
        activate: bool,
        width: ColumnWidth,
        is_full_width: bool,
        _anim: Option<niri_config::Animation>,
    ) {
        let next_to = self.idx_of(right_of).map(|idx| self.entries[idx].id);
        self.insert(tile, next_to, activate, width, is_full_width);
    }

    pub fn add_column(
        &mut self,
        _idx: Option<usize>,
        column: Column<W>,
        activate: bool,
        _anim_config: Option<niri_config::Animation>,
    ) {
        self.insert(
            column.tile,
            None,
            activate,
            column.width,
            column.is_full_width,
        );
    }

    pub fn remove_active_tile(&mut self, transaction: Transaction) -> Option<RemovedTile<W>> {
        let idx = self.active_idx()?;
        let id = self.entries[idx].tile.window().id().clone();
        Some(self.remove_tile(&id, transaction))
    }

    pub fn remove_tile(&mut self, window: &W::Id, transaction: Transaction) -> RemovedTile<W> {
        let idx = self.idx_of(window).unwrap();
        self.cancel_resize_for(idx);
        let mut e = self.entries.remove(idx);
        self.carry_span(e.id, &mut e.tile);
        let app = e.tile.window().app_id();
        if let (Some(app), Some(span)) = (&app, e.tile.canvas_span) {
            self.remember_span(app.clone(), span);
        }
        if self.drag.is_some_and(|(d, _)| d == e.id) {
            self.drag = None;
        }
        self.logged("close", Some(e.id), app, |c| c.remove(e.id));
        self.shrink_done(e.id);
        self.update_tile_sizes(true, Some(transaction));
        RemovedTile {
            tile: e.tile,
            width: e.width,
            is_full_width: e.is_full_width,
            is_floating: false,
        }
    }

    pub fn remove_active_column(&mut self) -> Option<Column<W>> {
        let removed = self.remove_active_tile(Transaction::new())?;
        Some(Column {
            tile: removed.tile,
            width: removed.width,
            is_full_width: removed.is_full_width,
        })
    }

    pub fn update_window(&mut self, window: &W::Id, serial: Option<Serial>) {
        let Some(idx) = self.idx_of(window) else {
            return;
        };
        let e = &mut self.entries[idx];
        if let Some(serial) = serial {
            e.tile.window_mut().on_commit(serial);
        }
        e.tile.update_window();

        let id = e.id;
        let kept = e.kept;
        let settled = self.observe_size(idx);
        let cause = if self.entries[idx].kept == kept {
            "min-size"
        } else {
            "kept-commit"
        };
        let (min_w, min_h) = self.min_of(&self.entries[idx]);
        let w = self.canvas.window(id).unwrap();
        if (w.min_w, w.min_h) != (min_w, min_h) {
            let app = self.app_of(id);
            self.logged_with(
                "app-grow",
                Some(id),
                app,
                |c| c.set_min(id, min_w, min_h),
                move |_, &placed| json!({ "cause": cause, "pushed": !placed }),
            );
            self.update_tile_sizes(true, None);
        }
        if settled || self.fits_cell(&self.entries[idx]) {
            self.shrink_done(id);
        }
        self.check_settled(id);
    }

    pub fn scroll_amount_to_activate(&self, window: &W::Id) -> f64 {
        let Some(id) = self.id_of(Some(window)) else {
            return 0.;
        };
        if self.canvas.focus() == Some(id) {
            return 0.;
        }
        let mut canvas = self.canvas.clone();
        canvas.focus_id(id);
        let (ax, ay) = self.canvas.view();
        let (bx, by) = canvas.view();
        let (sw, sh) = self.canvas.screen();
        f64::max((bx - ax).abs() / sw, (by - ay).abs() / sh)
    }

    pub fn activate_window(&mut self, window: &W::Id) -> bool {
        let Some(id) = self.id_of(Some(window)) else {
            return false;
        };
        self.apply_for("focus", |c| c.focus_id(id));
        true
    }

    pub fn start_close_animation_for_window(
        &mut self,
        renderer: &mut GlesRenderer,
        window: &W::Id,
        blocker: TransactionBlocker,
    ) {
        let Some(idx) = self.idx_of(window) else {
            return;
        };
        let pos = self.positions(false)[idx] + self.view_point();
        let through_camera = self.entries[idx].tile.sizing_mode().is_normal();
        let tile = &mut self.entries[idx].tile;
        let Some(snapshot) = tile.take_unmap_snapshot() else {
            return;
        };
        let tile_size = tile.tile_size();

        let anim = Animation::new(
            self.clock.clone(),
            0.,
            1.,
            0.,
            self.options.animations.window_close.anim,
        );
        let blocker = if self.options.disable_transactions {
            TransactionBlocker::completed()
        } else {
            blocker
        };
        let scale = Scale::from(self.scale);
        match ClosingWindow::new(renderer, snapshot, scale, tile_size, pos, blocker, anim) {
            Ok(closing) => self.closing_windows.push((closing, through_camera)),
            Err(err) => warn!("error creating a closing window animation: {err:?}"),
        }
    }

    pub fn start_open_animation(&mut self, id: &W::Id) -> bool {
        let Some(idx) = self.idx_of(id) else {
            return false;
        };
        self.entries[idx].tile.start_open_animation();
        true
    }

    pub fn focus_dir(&mut self, dir: Dir) -> bool {
        self.apply_for("focus", |c| c.focus_dir(dir))
    }

    pub fn focus_left(&mut self) -> bool {
        self.focus_dir(Dir::Left)
    }

    pub fn focus_right(&mut self) -> bool {
        self.focus_dir(Dir::Right)
    }

    pub fn focus_up(&mut self) -> bool {
        self.focus_dir(Dir::Up)
    }

    pub fn focus_down(&mut self) -> bool {
        self.focus_dir(Dir::Down)
    }

    /// Moves the focus in `dir` until no window lies further that way.
    fn focus_far(&mut self, dir: Dir) {
        let windows = self.canvas.windows().len();
        repeat_while(windows, || self.focus_dir(dir));
    }

    pub fn focus_column_first(&mut self) {
        self.focus_far(Dir::Left);
    }

    pub fn focus_column_last(&mut self) {
        self.focus_far(Dir::Right);
    }

    pub fn focus_top(&mut self) {
        self.focus_far(Dir::Up);
    }

    pub fn focus_bottom(&mut self) {
        self.focus_far(Dir::Down);
    }

    /// Focuses the window starting the `index`-th tile column (1-based) that has one.
    pub fn focus_column(&mut self, index: usize) {
        let mut windows: Vec<Rect> = self.canvas.windows().iter().map(|w| w.rect).collect();
        windows.sort_by_key(|r| (r.i, r.j));
        let mut columns: Vec<i32> = windows.iter().map(|r| r.i).collect();
        columns.dedup();
        let Some(&i) = columns.get(index.saturating_sub(1)) else {
            return;
        };
        let target = self
            .canvas
            .windows()
            .iter()
            .filter(|w| w.rect.i == i)
            .min_by_key(|w| w.rect.j)
            .map(|w| w.id);
        if let Some(id) = target {
            self.apply_for("focus", |c| c.focus_id(id));
        }
    }

    /// Focuses the `index`-th window (1-based) from the top among those sharing tile columns
    /// with the focused one.
    pub fn focus_window_in_column(&mut self, index: u8) {
        let Some(f) = self.canvas.focus().and_then(|f| self.canvas.rect(f)) else {
            return;
        };
        let mut column: Vec<(i32, Id)> = self
            .canvas
            .windows()
            .iter()
            .filter(|w| w.rect.cols_overlap(&f))
            .map(|w| (w.rect.j, w.id))
            .collect();
        column.sort();
        if let Some(&(_, id)) = column.get(usize::from(index).saturating_sub(1)) {
            self.apply_for("focus", |c| c.focus_id(id));
        }
    }

    pub fn focus_down_or_left(&mut self) {
        if !self.focus_down() {
            self.focus_left();
        }
    }

    pub fn focus_down_or_right(&mut self) {
        if !self.focus_down() {
            self.focus_right();
        }
    }

    pub fn focus_up_or_left(&mut self) {
        if !self.focus_up() {
            self.focus_left();
        }
    }

    pub fn focus_up_or_right(&mut self) {
        if !self.focus_up() {
            self.focus_right();
        }
    }

    /// Exchanges the focused window's tiles with its neighbour's in `dir`.
    pub fn swap_neighbour(&mut self, dir: Dir) -> bool {
        let focus = self.canvas.focus();
        let app = focus.and_then(|f| self.app_of(f));
        let rv = self.logged("swap", focus, app, |c| c.swap(dir));
        if rv {
            self.update_tile_sizes(true, None);
        }
        rv
    }

    /// Takes back the last rearrangement, or the whole of the last park (see [`Self::park`]).
    pub fn undo(&mut self) -> bool {
        let park = self.park_steps(false);
        let rv = self.logged("undo", None, None, |c| {
            (0..park.unwrap_or(1)).all(|_| c.undo())
        });
        if park.is_some() {
            self.park_stepped(false);
        }
        if rv {
            self.update_tile_sizes(true, None);
        }
        rv
    }

    /// Does again the last rearrangement undone, or the whole of the last park.
    pub fn redo(&mut self) -> bool {
        let park = self.park_steps(true);
        let rv = self.logged("redo", None, None, |c| {
            (0..park.unwrap_or(1)).all(|_| c.redo())
        });
        if park.is_some() {
            self.park_stepped(true);
        }
        if rv {
            self.update_tile_sizes(true, None);
        }
        rv
    }

    /// Brings every window into one island, for undo to take back.
    pub fn gather_islands(&mut self) -> bool {
        let rv = self.logged("gather", None, None, |c| c.gather_islands());
        if rv {
            self.update_tile_sizes(true, None);
        }
        rv
    }

    /// Moves the focused window one tile in `dir`, pushing whatever is in the way.
    fn move_by(&mut self, dir: Dir) -> bool {
        let Some(id) = self.canvas.focus() else {
            return false;
        };
        let app = self.app_of(id);
        let rv = self.logged("move", Some(id), app, |c| c.move_by(id, dir));
        if rv {
            self.update_tile_sizes(true, None);
        }
        rv
    }

    /// Moves the focused window one tile in `dir`, as [`Self::move_by`] does, when it is drawn
    /// on the grid: not when it is, or is about to be, fullscreen or maximized.
    pub fn move_focus_on_grid(&mut self, dir: Dir) -> bool {
        let on_grid = self.active_idx().is_some_and(|idx| {
            let e = &self.entries[idx];
            e.tile.sizing_mode().is_normal() && !e.is_pending_fullscreen && !e.is_pending_maximized
        });
        on_grid && self.move_by(dir)
    }

    pub fn move_left(&mut self) -> bool {
        self.move_by(Dir::Left)
    }

    pub fn move_right(&mut self) -> bool {
        self.move_by(Dir::Right)
    }

    pub fn move_up(&mut self) -> bool {
        self.move_by(Dir::Up)
    }

    pub fn move_down(&mut self) -> bool {
        self.move_by(Dir::Down)
    }

    pub fn send_to_edge(&mut self, dir: Dir) -> bool {
        let focus = self.canvas.focus();
        let app = focus.and_then(|f| self.app_of(f));
        self.logged("send-to-edge", focus, app, |c| c.send_to_edge(dir))
    }

    pub fn move_column_to_first(&mut self) {
        self.send_to_edge(Dir::Left);
    }

    pub fn move_column_to_last(&mut self) {
        self.send_to_edge(Dir::Right);
    }

    pub fn move_column_to_index(&mut self, index: usize) {
        if index <= 1 {
            self.send_to_edge(Dir::Left);
        }
    }

    pub fn consume_or_expel_window_left(&mut self, _window: Option<&W::Id>) {}

    pub fn consume_or_expel_window_right(&mut self, _window: Option<&W::Id>) {}

    pub fn consume_into_column(&mut self) {}

    pub fn expel_from_column(&mut self) {}

    pub fn swap_window_in_direction(&mut self, direction: ScrollDirection) {
        match direction {
            ScrollDirection::Left => self.swap_neighbour(Dir::Left),
            ScrollDirection::Right => self.swap_neighbour(Dir::Right),
        };
    }

    pub fn toggle_column_tabbed_display(&mut self) {}

    pub fn set_column_display(&mut self, _display: ColumnDisplay) {}

    /// Centres the view on the focused window (aligned to its top-left when it is larger than
    /// the screen).
    pub fn center_column(&mut self) {
        let Some(r) = self.canvas.focus().and_then(|f| self.canvas.rect(f)) else {
            return;
        };
        let (x, y, w, h) = self.canvas.canvas_px(r);
        let (sw, sh) = self.canvas.screen();
        let center = |start: f64, len: f64, screen: f64| {
            if len > screen {
                start
            } else {
                start + len / 2. - screen / 2.
            }
        };
        let (vx, vy) = (center(x, w, sw), center(y, h, sh));
        self.apply_for("center", |c| c.set_view(vx, vy));
    }

    pub fn center_window(&mut self, window: Option<&W::Id>) {
        if let Some(id) = self.id_of(window) {
            self.apply_for("focus", |c| c.focus_id(id));
            self.center_column();
        }
    }

    pub fn center_visible_columns(&mut self) {
        self.center_column();
    }

    /// Moves one edge of the focused tiled window by `delta` tiles (see [`Canvas::resize`]).
    pub fn resize_edge(&mut self, edge: Edge, delta: i32) -> bool {
        let delta = delta.clamp(-MAX_SPAN, MAX_SPAN);
        let focus = self.canvas.focus();
        let app = focus.and_then(|f| self.app_of(f));
        let rv = self.logged("resize", focus, app, |c| c.resize_focused(edge, delta));
        if rv {
            self.update_tile_sizes(true, None);
        }
        rv
    }

    /// Moves the camera by `delta` zoom steps; no window changes its size or tiles.
    pub fn zoom(&mut self, delta: i32) -> bool {
        self.apply_for("zoom", |c| c.zoom_by(delta))
    }

    /// Returns the camera to 1:1, as [`Self::zoom`] by the steps it is away; a zoom
    /// [`Self::zoom_to_fit`] remembered is dropped, so a held Super's release stays at 1:1.
    pub fn zoom_reset(&mut self) -> bool {
        self.zoom_back = None;
        self.apply_for("zoom", |c| c.zoom_by(-c.zoom()))
    }

    /// Pulls the camera back the fewest zoom steps, up to `zoom-max`, that fit every window on
    /// screen, then moves the view the least it must to show them all, or the focus when they do
    /// not fit. The first call remembers the zoom for [`Self::zoom_back`]; later ones keep the
    /// canvas fitting as it grows.
    pub fn zoom_to_fit(&mut self) {
        self.zoom_back.get_or_insert(self.canvas.zoom());
        let Some(bounds) = self.canvas_bounds() else {
            return;
        };
        self.apply_for("zoom", |c| {
            while !c.fits_on_screen(bounds.w, bounds.h) && c.zoom_by(1) {}
            c.ensure_visible(bounds);
            c.ensure_focus_visible();
        });
    }

    /// Returns the camera to the zoom [`Self::zoom_to_fit`] remembered, showing the focused window
    /// as any focus change does. Returns whether there was one.
    pub fn zoom_back(&mut self) -> bool {
        let Some(zoom) = self.zoom_back.take() else {
            return false;
        };
        self.apply_for("zoom", |c| {
            c.zoom_by(zoom - c.zoom());
            c.ensure_focus_visible();
        });
        true
    }

    /// The span of tiles every window lies in.
    fn canvas_bounds(&self) -> Option<Rect> {
        let mut rects = self.canvas.windows().iter().map(|w| w.rect);
        let r = rects.next()?;
        let (i, j, right, bottom) = rects.fold(
            (r.i, r.j, r.right(), r.bottom()),
            |(i, j, right, bottom), r| {
                (
                    i.min(r.i),
                    j.min(r.j),
                    right.max(r.right()),
                    bottom.max(r.bottom()),
                )
            },
        );
        Some(Rect::new(i, j, right - i, bottom - j))
    }

    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// Resizes a window's span along one axis to `target` tiles, moving `edge`.
    fn set_span(&mut self, id: Id, edge: Edge, target: i32) {
        let Some(r) = self.canvas.rect(id) else {
            return;
        };
        let current = match edge {
            Edge::Left | Edge::Right => r.w,
            Edge::Top | Edge::Bottom => r.h,
        };
        let app = self.app_of(id);
        if target != current
            && self.logged("resize", Some(id), app, |c| {
                c.resize(id, edge, target - current)
            })
        {
            self.update_tile_sizes(true, None);
        }
    }

    fn span_for(&self, change: SizeChange, current: i32, horizontal: bool) -> i32 {
        let (tw, th) = self.canvas.base_tile();
        let (sw, sh) = self.canvas.screen();
        let (tile, screen) = if horizontal { (tw, sw) } else { (th, sh) };
        let tiles = |px: f64| (px / tile).round() as i32;
        let step = |px: f64| match tiles(px) {
            0 if px > 0. => 1,
            0 if px < 0. => -1,
            n => n,
        };
        let target = match change {
            SizeChange::SetFixed(px) => tiles(f64::from(px)),
            SizeChange::SetProportion(p) => tiles(screen * p / 100.),
            SizeChange::AdjustFixed(px) => current.saturating_add(step(f64::from(px))),
            SizeChange::AdjustProportion(p) => current.saturating_add(step(screen * p / 100.)),
        };
        target.clamp(1, MAX_SPAN)
    }

    fn full_span(&self, horizontal: bool) -> i32 {
        let (tw, th) = self.canvas.base_tile();
        let (sw, sh) = self.canvas.screen();
        let n = if horizontal { sw / tw } else { sh / th };
        (n + 1e-6).floor().max(1.) as i32
    }

    fn min_span(&self, id: Id) -> (i32, i32) {
        let w = self.canvas.window(id).unwrap();
        self.canvas.need(w.min_w, w.min_h)
    }

    fn span(&self, id: Id) -> (i32, i32) {
        let r = self.canvas.rect(id).unwrap();
        (r.w, r.h)
    }

    pub fn toggle_width(&mut self, forwards: bool) {
        self.toggle_window_width(None, forwards);
    }

    pub fn toggle_full_width(&mut self) {
        let Some(id) = self.canvas.focus() else {
            return;
        };
        let full = self.full_span(true);
        let target = if self.span(id).0 < full {
            full
        } else {
            self.min_span(id).0
        };
        self.set_span(id, Edge::Right, target);
    }

    pub fn set_window_width(&mut self, window: Option<&W::Id>, change: SizeChange) {
        let Some(id) = self.id_of(window) else {
            return;
        };
        let target = self.span_for(change, self.span(id).0, true);
        self.set_span(id, Edge::Right, target);
    }

    pub fn set_window_height(&mut self, window: Option<&W::Id>, change: SizeChange) {
        let Some(id) = self.id_of(window) else {
            return;
        };
        let target = self.span_for(change, self.span(id).1, false);
        self.set_span(id, Edge::Bottom, target);
    }

    pub fn reset_window_height(&mut self, window: Option<&W::Id>) {
        let Some(id) = self.id_of(window) else {
            return;
        };
        let target = self.min_span(id).1;
        self.set_span(id, Edge::Bottom, target);
    }

    pub fn toggle_window_width(&mut self, window: Option<&W::Id>, forwards: bool) {
        let Some(id) = self.id_of(window) else {
            return;
        };
        let target = self.span(id).0 + if forwards { 1 } else { -1 };
        self.set_span(id, Edge::Right, target);
    }

    pub fn toggle_window_height(&mut self, window: Option<&W::Id>, forwards: bool) {
        let Some(id) = self.id_of(window) else {
            return;
        };
        let target = self.span(id).1 + if forwards { 1 } else { -1 };
        self.set_span(id, Edge::Bottom, target);
    }

    pub fn expand_column_to_available_width(&mut self) {
        let Some(id) = self.canvas.focus() else {
            return;
        };
        let full = self.full_span(true);
        self.set_span(id, Edge::Right, full);
    }

    pub fn set_fullscreen(&mut self, window: &W::Id, is_fullscreen: bool) -> bool {
        let idx = self.idx_of(window).unwrap();
        if self.entries[idx].is_pending_fullscreen == is_fullscreen {
            return false;
        }
        self.cancel_resize_for(idx);
        self.entries[idx].is_pending_fullscreen = is_fullscreen;
        self.update_tile_sizes(true, None);
        true
    }

    pub fn set_maximized(&mut self, window: &W::Id, maximize: bool) -> bool {
        let idx = self.idx_of(window).unwrap();
        if self.entries[idx].is_pending_maximized == maximize {
            return false;
        }
        self.cancel_resize_for(idx);
        self.entries[idx].is_pending_maximized = maximize;
        self.update_tile_sizes(true, None);
        true
    }

    pub fn fullscreen_window_above_top_layer(&self) -> Option<&W> {
        if !self.view_offset.is_static() {
            return None;
        }
        let tile = &self.entries[self.active_idx()?].tile;
        tile.sizing_mode().is_fullscreen().then(|| tile.window())
    }

    pub fn tiles_with_render_positions(
        &self,
    ) -> impl Iterator<Item = (&Tile<W>, Point<f64, Logical>, bool)> {
        let _span = tracy_client::span!("CanvasSpace::tiles_with_render_positions");
        let positions = self.positions(true);
        self.render_order()
            .into_iter()
            .map(|idx| {
                let tile = &self.entries[idx].tile;
                let pos = positions[idx];
                (tile, pos, self.is_visible(tile, pos))
            })
            .collect::<Vec<_>>()
            .into_iter()
    }

    pub fn tiles_with_render_positions_mut(
        &mut self,
        round: bool,
    ) -> impl Iterator<Item = (&mut Tile<W>, Point<f64, Logical>)> {
        let positions = self.positions(round);
        self.entries
            .iter_mut()
            .zip(positions)
            .map(|(e, pos)| (&mut e.tile, pos))
    }

    pub fn tiles_with_ipc_layouts(&self) -> impl Iterator<Item = (&Tile<W>, WindowLayout)> {
        let min_i = self.canvas.windows().iter().map(|w| w.rect.i).min();
        let min_j = self.canvas.windows().iter().map(|w| w.rect.j).min();
        let view_point = self.target_view_point();
        self.entries.iter().map(move |e| {
            let r = self.canvas.rect(e.id).unwrap();
            let col = (r.i - min_i.unwrap_or(0) + 1) as usize;
            let row = (r.j - min_j.unwrap_or(0) + 1) as usize;
            let camera = self.camera_of(&e.tile, self.target_camera());
            let pos = camera
                .to_view(self.entry_pos(e, view_point))
                .to_physical_precise_round(self.scale)
                .to_logical(self.scale);
            let layout = WindowLayout {
                pos_in_scrolling_layout: Some((col, row)),
                tiles: Some((r.i, r.j, r.w, r.h)),
                tile_pos_in_workspace_view: Some(pos.into()),
                ..e.tile.ipc_layout_template()
            };
            (&e.tile, layout)
        })
    }

    pub(super) fn insert_hint_area(
        &self,
        _position: InsertPosition,
    ) -> Option<Rectangle<f64, Logical>> {
        None
    }

    /// Where the focused tile shows in the view once the view and the camera settle.
    pub fn active_window_visual_rectangle(&self) -> Option<Rectangle<f64, Logical>> {
        let e = &self.entries[self.active_idx()?];
        let pos = self.entry_pos(e, self.target_view_point()) + e.tile.window_loc();
        let camera = self.camera_of(&e.tile, self.target_camera());
        let rect = camera.rect_to_view(Rectangle::new(pos, e.tile.window_size()));
        Rectangle::from_size(self.view_size).intersection(rect)
    }

    /// The canvas gives every popup the working area, so `overflow` changes nothing.
    pub fn popup_target_rect(
        &self,
        id: &W::Id,
        _overflow: bool,
    ) -> Option<Rectangle<f64, Logical>> {
        let e = &self.entries[self.idx_of(id)?];
        let pos = self.entry_pos(e, self.target_view_point());
        let camera = self.camera_of(&e.tile, self.target_camera());
        let mut target = camera.rect_from_view(self.working_area);
        target.loc -= pos;
        target.loc -= e.tile.window_loc();
        Some(target)
    }

    /// Pushes the canvas's elements; those of grid tiles, placeholders and their closing windows
    /// go through the camera when it is zoomed. `shown` is the rectangle of the view the output
    /// shows: tiles outside it are not drawn.
    pub fn render<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        xray_pos: XrayPos,
        shown: Rectangle<f64, Logical>,
        focus_ring: bool,
        layer: RenderLayer,
        push: &mut dyn FnMut(CanvasSpaceRenderElement<R>),
    ) {
        let _span = tracy_client::span!("CanvasSpace::render");
        let camera = self.camera();
        let origin = camera.origin.to_physical_precise_round(self.scale);
        let mut push = |through_camera: bool, elem: CanvasRenderElement<R>| {
            if through_camera && camera.is_zoomed() {
                push(RescaleRenderElement::from_element(elem, origin, camera.scale).into());
            } else {
                push(elem.into());
            }
        };

        let scale = Scale::from(self.scale);
        let view_point = self.view_point();
        let area = camera.rect_from_view(Rectangle::from_size(self.view_size));
        let far = area.loc + area.size.to_point();
        let camera_view = Rectangle::new(view_point, Size::from((far.x.max(0.), far.y.max(0.))));
        if layer.is_normal() {
            if let Some(elem) = self.minimap_render(ctx.renderer) {
                push(false, elem.into());
            }
            for (closing, through_camera) in self.closing_windows.iter().rev() {
                let view_rect = if *through_camera {
                    camera_view
                } else {
                    Rectangle::new(view_point, self.view_size)
                };
                if let Some(elem) = closing.render(ctx.as_gles(), view_rect, scale) {
                    push(*through_camera, elem.into());
                }
            }
        }

        let active = self.canvas.focus();
        if layer.is_normal() {
            if let (Some(b), Some(cell)) = (&self.resize_box, self.resize_box_cell()) {
                b.render(ctx.renderer, cell.loc, &mut |elem| push(true, elem.into()));
            }
            let cells = self.placeholder_cells();
            for (p, cell) in self.placeholders.iter().zip(cells) {
                if let Some(elem) = p.render_icon(ctx.renderer, cell.loc, cell.size, self.scale) {
                    push(true, elem.into());
                }
                p.render_box(ctx.renderer, cell.loc, &mut |elem| push(true, elem.into()));
            }
        }
        for (idx, pos) in self.drawn(shown) {
            let e = &self.entries[idx];
            if layer.is_normal() == e.tile.is_moving_between_workspaces() {
                continue;
            }

            let focus_ring = focus_ring && Some(e.id) == active;
            let tile_camera = self.camera_of(&e.tile, camera);
            let xray_pos = tile_camera.xray(xray_pos).offset(pos);
            let through_camera = tile_camera.is_zoomed();
            e.tile
                .render(ctx.r(), pos, xray_pos, focus_ring, &mut |elem| {
                    push(through_camera, elem.into())
                });
        }
    }

    /// The window under `pos` in the view, among those `shown`, the rectangle of the view the
    /// output shows, holds. Under a zoomed camera a hit only activates: the pointer picks the
    /// window and reaches none of its surfaces. With `magnified-input`, a camera that magnifies
    /// passes the hit on, mapped through the camera.
    pub fn window_under(
        &self,
        pos: Point<f64, Logical>,
        shown: Rectangle<f64, Logical>,
    ) -> Option<(&W, HitType)> {
        let _span = tracy_client::span!("CanvasSpace::window_under");
        let positions = self.positions(true);
        let camera = self.camera();
        for idx in self.render_order() {
            let tile = &self.entries[idx].tile;
            let tile_pos = positions[idx];
            if !self.is_shown(tile, tile_pos, shown) {
                continue;
            }
            let tile_camera = self.camera_of(tile, camera);
            let canvas_pos = tile_camera.from_view(pos);
            if let Some((win, hit)) = HitType::hit_tile(tile, tile_pos, canvas_pos) {
                let hit = if !tile_camera.is_zoomed() {
                    hit
                } else if tile_camera.scale > 1. && self.options.layout.canvas.magnified_input {
                    hit.through_magnifier(pos, canvas_pos)
                } else {
                    hit.to_activate()
                };
                return Some((win, hit));
            }
        }
        None
    }

    pub fn view_offset_gesture_begin(&mut self, is_touchpad: bool) {
        if self.entries.is_empty() || self.interactive_resize.is_some() {
            return;
        }
        let (vx, vy) = self.canvas.view();
        let current = Point::from((vx, vy)) + self.view_offset.current();
        self.canvas.set_view(current.x, current.y);
        self.view_offset = ViewOffset::Gesture(PanGesture {
            tracker_x: SwipeTracker::new(),
            tracker_y: SwipeTracker::new(),
            pos: (0., 0.),
            is_touchpad,
        });
        let before = self.drawn_now(None);
        self.begin_motion(Cause::new(motion::pan_kind(is_touchpad)), before);
    }

    pub fn view_offset_gesture_update(
        &mut self,
        delta_x: f64,
        timestamp: Duration,
        is_touchpad: bool,
    ) -> Option<bool> {
        self.pan_gesture_update(delta_x, 0., timestamp, is_touchpad)
    }

    /// Pans the view by a gesture's movement in both axes.
    pub fn pan_gesture_update(
        &mut self,
        delta_x: f64,
        delta_y: f64,
        timestamp: Duration,
        is_touchpad: bool,
    ) -> Option<bool> {
        let norm = self.gesture_norm(is_touchpad);
        let ViewOffset::Gesture(gesture) = &mut self.view_offset else {
            return None;
        };
        if gesture.is_touchpad != is_touchpad {
            return None;
        }
        gesture.tracker_x.push(delta_x, timestamp);
        gesture.tracker_y.push(delta_y, timestamp);
        gesture.pos.0 += delta_x;
        gesture.pos.1 += delta_y;
        self.canvas.scroll_by(delta_x * norm, delta_y * norm);
        Some(true)
    }

    /// The touchpad motion a pan takes to scroll the view one tile along each axis, at the zoom
    /// the camera goes to.
    pub fn touchpad_tile_motion(&self) -> (f64, f64) {
        let norm = self.gesture_norm(true);
        let (tw, th) = self.canvas.tile_size();
        (tw / norm, th / norm)
    }

    fn gesture_norm(&self, is_touchpad: bool) -> f64 {
        if is_touchpad {
            self.working_area.size.w / VIEW_GESTURE_WORKING_AREA_MOVEMENT
        } else {
            1.
        }
    }

    /// Ends a pan: the focus goes to the window nearest the middle of the screen and the view
    /// settles to show it.
    pub fn view_offset_gesture_end(&mut self, is_touchpad: Option<bool>) -> bool {
        let ViewOffset::Gesture(gesture) = &self.view_offset else {
            return false;
        };
        if is_touchpad.is_some_and(|t| t != gesture.is_touchpad) {
            return false;
        }
        // A mouse pan keeps the share of its momentum `pan-momentum` gives it; 0 stops it
        // where the hand stopped.
        let norm = self.gesture_norm(gesture.is_touchpad)
            * if gesture.is_touchpad {
                1.
            } else {
                self.options.layout.canvas.pan_momentum
            };
        let kind = motion::pan_kind(gesture.is_touchpad);
        let extra_x = (gesture.tracker_x.projected_end_pos() - gesture.pos.0) * norm;
        let extra_y = (gesture.tracker_y.projected_end_pos() - gesture.pos.1) * norm;
        self.view_offset = ViewOffset::Static;
        self.cause(Cause::carry(kind));
        // The fling glides on from where the fingers left the view: moving the view to the
        // projected end before `apply` would draw it there at once.
        self.apply(|c| {
            c.scroll_by(extra_x, extra_y);
            c.focus_view_center();
        });
        if let ViewOffset::Animation { from, anim } = &mut self.view_offset {
            // It slows the way the swipe tracker projects it: at that rate, a glide that ends
            // on the projected point starts at the speed the fingers left with. It stops once
            // it is slower than 1.5 px/s, half a pixel short of the end.
            let rate = DECELERATION_TOUCHPAD;
            let start = 1000. * rate.ln();
            let len = from.x.hypot(from.y);
            if len > 0.5 {
                *anim = Animation::decelerate(self.clock.clone(), 1., start, rate, 1.5 / len);
            }
        }
        true
    }

    pub fn dnd_scroll_gesture_begin(&mut self) {}

    pub fn dnd_scroll_gesture_scroll(&mut self, _delta: f64) -> bool {
        false
    }

    pub fn dnd_scroll_gesture_end(&mut self) {}

    pub fn interactive_resize_begin(&mut self, window: W::Id, edges: ResizeEdge) -> bool {
        if self.interactive_resize.is_some() {
            return false;
        }
        let Some(idx) = self.idx_of(&window) else {
            return false;
        };
        let original_window_size = self.entries[idx].tile.window_size();
        self.interactive_resize = Some(InteractiveResize {
            window,
            original_window_size,
            data: InteractiveResizeData { edges },
        });
        self.interactive_resize_delta = Point::from((0., 0.));
        true
    }

    pub fn interactive_resize_update(
        &mut self,
        window: &W::Id,
        delta: Point<f64, Logical>,
    ) -> bool {
        match &self.interactive_resize {
            Some(resize) if &resize.window == window => {
                self.interactive_resize_delta = delta;
                self.preview_resize();
                true
            }
            _ => false,
        }
    }

    /// Ends a resize drag: each dragged edge moves by the whole tiles the pointer travelled,
    /// as its preview showed (see [`resize`]).
    pub fn interactive_resize_end(&mut self, window: Option<&W::Id>) {
        let Some(resize) = &self.interactive_resize else {
            return;
        };
        if window.is_some_and(|window| window != &resize.window) {
            return;
        }
        let moves = self.resize_moves();
        let resize = self.interactive_resize.take().unwrap();
        self.resize_box = None;
        self.resize_at = None;
        let shown = std::mem::take(&mut self.preview);
        let Some(id) = self.id_of(Some(&resize.window)) else {
            self.preview = shown;
            self.clear_preview();
            return;
        };
        let before = self.before_release(&shown);
        let mut changed = false;
        for (edge, delta) in moves {
            let app = self.app_of(id);
            changed |= self.logged("resize", Some(id), app, |c| c.resize(id, edge, delta));
        }
        self.keep_previewed(&shown, &before);
        if changed {
            self.update_tile_sizes(true, None);
        }
    }

    pub fn refresh(&mut self, is_active: bool, is_focused: bool) {
        let _span = tracy_client::span!("CanvasSpace::refresh");
        let active = self.canvas.focus();
        for e in &mut self.entries {
            let win = e.tile.window_mut();

            win.set_active_in_column(true);
            win.set_floating(false);

            let mut is_active = is_active && Some(e.id) == active;
            if self.options.deactivate_unfocused_windows {
                is_active &= is_focused;
            }
            win.set_activated(is_active);

            let resize_data = self
                .interactive_resize
                .as_ref()
                .filter(|resize| &resize.window == win.id())
                .map(|resize| resize.data);
            win.set_interactive_resize(resize_data);

            let border_config = self.options.layout.border.merged_with(&win.rules().border);
            let bounds = compute_toplevel_bounds(
                border_config,
                self.working_area.size,
                self.options.layout.gaps,
            );
            win.set_bounds(bounds);

            let intent = if self.options.disable_resize_throttling {
                ConfigureIntent::CanSend
            } else {
                win.configure_intent()
            };
            if matches!(
                intent,
                ConfigureIntent::CanSend | ConfigureIntent::ShouldSend
            ) {
                win.send_pending_configure();
            }

            win.refresh();
        }
    }

    pub fn view_pos(&self) -> f64 {
        self.view_point().x
    }

    pub fn view_size(&self) -> Size<f64, Logical> {
        self.view_size
    }

    pub fn parent_area(&self) -> Rectangle<f64, Logical> {
        self.parent_area
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn options(&self) -> &Rc<Options> {
        &self.options
    }

    pub fn active_column_idx(&self) -> usize {
        self.active_idx().unwrap_or(0)
    }

    #[cfg(test)]
    pub(super) fn view_offset(&self) -> &ViewOffset {
        &self.view_offset
    }

    pub fn columns(&self) -> impl Iterator<Item = &Entry<W>> {
        self.entries.iter()
    }

    #[cfg(test)]
    pub fn verify_invariants(&self) {
        assert!(self.view_size.w > 0.);
        assert!(self.view_size.h > 0.);
        assert!(self.scale > 0.);
        assert!(self.scale.is_finite());
        assert_eq!(
            self.working_area,
            compute_working_area(self.parent_area, self.scale, self.options.layout.struts)
        );
        assert_eq!(
            self.entries.len() + self.placeholders.len(),
            self.canvas.windows().len()
        );
        assert_eq!(self.canvas.screen(), self.canvas_screen());
        assert_eq!(*self.canvas.config(), canvas_config(&self.options));

        for e in &self.entries {
            assert!(self.canvas.rect(e.id).is_some());
            assert!(Rc::ptr_eq(&e.tile.options, &self.options));
            e.tile.verify_invariants();
        }

        if let Some(resize) = &self.interactive_resize {
            assert!(self.idx_of(&resize.window).is_some());
        }

        for id in &self.shrinking {
            assert!(self.entries.iter().any(|e| e.id == *id));
        }
        if self.shrinking.is_empty() {
            for e in &self.entries {
                assert_eq!(e.held, Point::from((0., 0.)), "held with no shrink waiting");
            }
        }

        if !self.view_offset.is_gesture() {
            if let Err(err) = self.canvas.check() {
                panic!("canvas invariant broken: {err}");
            }
            self.verify_focus_drawn();
        }
    }

    /// The focused window, once the view, the camera and moves settle, is drawn whole within the
    /// working area wherever its tiles fit on screen, at the size it committed.
    #[cfg(test)]
    fn verify_focus_drawn(&self) {
        let Some((rect, area)) = self.focus_drawn() else {
            return;
        };
        if rect.size.w > area.size.w + 2. || rect.size.h > area.size.h + 2. {
            return;
        }
        let far = rect.loc + rect.size.to_point();
        let area_far = area.loc + area.size.to_point();
        assert!(
            rect.loc.x >= area.loc.x - 2.
                && rect.loc.y >= area.loc.y - 2.
                && far.x <= area_far.x + 2.
                && far.y <= area_far.y + 2.,
            "focused window drawn at {rect:?}, outside the working area {area:?}"
        );
    }

    /// The 1:1 canvas point drawn under the middle of the screen now, while the camera animates
    /// too.
    #[cfg(test)]
    pub fn drawn_middle(&self) -> Point<f64, Logical> {
        let (sw, sh) = self.canvas.screen();
        let middle = self.canvas_origin() + Point::from((sw / 2., sh / 2.));
        self.camera().from_view(middle) + self.view_point()
    }

    /// The focused grid window's drawn rectangle (see [`Self::drawn_window_rect`]) and the
    /// working area, when its tiles fit on screen.
    #[cfg(test)]
    pub fn focus_drawn(&self) -> Option<(Rectangle<f64, Logical>, Rectangle<f64, Logical>)> {
        let idx = self.active_idx()?;
        let r = self.canvas.rect(self.entries[idx].id)?;
        if !self.canvas.fits_on_screen(r.w, r.h) {
            return None;
        }
        Some((self.drawn_window_rect(idx)?, self.working_area))
    }

    /// Where a grid window shows in the view once the view, the camera and moves settle, at the
    /// size it committed in answer to the last size asked of it; none while it has not answered.
    #[cfg(test)]
    fn drawn_window_rect(&self, idx: usize) -> Option<Rectangle<f64, Logical>> {
        let e = &self.entries[idx];
        let win = e.tile.window();
        if e.is_pending_fullscreen
            || e.is_pending_maximized
            || !win.sizing_mode().is_normal()
            || !win.is_size_settled()
        {
            return None;
        }
        let pos = self.entry_pos(e, self.target_view_point()) + e.tile.window_loc();
        let rect = Rectangle::new(pos, e.tile.window_size());
        Some(self.target_camera().rect_to_view(rect))
    }
}

impl<W: LayoutElement> Layout<W> {
    fn active_canvas(&mut self) -> Option<&mut CanvasSpace<W>> {
        self.active_workspace_mut()?.canvas_mut(true)
    }

    /// Whether `window` is drawn through a zoomed canvas camera.
    pub fn is_drawn_zoomed(&self, window: &W::Id) -> bool {
        self.workspaces()
            .find(|(_, _, ws)| ws.has_window(window))
            .and_then(|(_, _, ws)| ws.tile_view_geo(window))
            .is_some_and(|(_, _, scale)| scale != 1.)
    }

    /// Pans the canvas with an ongoing view gesture in both axes.
    pub fn canvas_pan_update(
        &mut self,
        delta_x: f64,
        delta_y: f64,
        timestamp: Duration,
        is_touchpad: bool,
    ) -> Option<Option<Output>> {
        let zoom = self.overview_zoom();
        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return None;
        };
        for monitor in monitors {
            for ws in &mut monitor.workspaces {
                let canvas = ws.canvas_mut(false).unwrap();
                if let Some(refresh) = canvas.pan_gesture_update(
                    delta_x / zoom,
                    delta_y / zoom,
                    timestamp,
                    is_touchpad,
                ) {
                    return Some(refresh.then(|| monitor.output.clone()));
                }
            }
        }
        None
    }

    pub fn canvas_resize_edge(&mut self, edge: Edge, delta: i32) {
        if let Some(canvas) = self.active_canvas() {
            canvas.resize_edge(edge, delta);
        }
    }

    pub fn canvas_launch_placeholder(
        &mut self,
        app_id: String,
        icon: Option<String>,
        size: RuleSize,
    ) {
        if let Some(canvas) = self
            .active_workspace_mut()
            .and_then(|ws| ws.canvas_mut(false))
        {
            canvas.add_placeholder(app_id, icon, size);
        }
    }

    /// Puts a saved canvas back (see [`CanvasSpace::restore`]) on the workspace at `idx` on the
    /// output named `output`: that output's active workspace when it has no workspace `idx`, the
    /// active output when no output has that name.
    pub fn canvas_restore(
        &mut self,
        output: Option<&str>,
        idx: usize,
        saved: RestoreCanvas,
        lifetime: Duration,
    ) {
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            return;
        };
        let mon_idx = output
            .and_then(|name| monitors.iter().position(|m| m.output_name() == name))
            .unwrap_or(*active_monitor_idx);
        let mon = &mut monitors[mon_idx];
        let ws_idx = if idx < mon.workspaces.len() {
            idx
        } else {
            mon.active_workspace_idx()
        };
        if let Some(canvas) = mon.workspaces[ws_idx].canvas_mut(false) {
            canvas.restore(saved, lifetime);
        }
    }

    /// Whether a saved canvas is being put back on any workspace.
    pub fn canvas_restoring(&self) -> bool {
        self.workspaces()
            .any(|(_, _, ws)| ws.scrolling().is_restoring())
    }

    /// The workspace on an output with a placeholder waiting for a window with `app_id`.
    pub(super) fn placeholder_workspace(&self, app_id: &str) -> Option<WorkspaceId> {
        self.workspaces()
            .find(|(mon, _, ws)| mon.is_some() && ws.scrolling().has_placeholder(app_id))
            .map(|(_, _, ws)| ws.id())
    }

    /// Focuses `window` on its canvas and centres the view on it, when it is a grid window: the
    /// Alt+Tab switcher's confirm with `center-on-switch`.
    pub fn canvas_center_window(&mut self, window: &W::Id) {
        if let Some(canvas) = self
            .workspaces_mut()
            .find(|ws| ws.has_window(window))
            .and_then(|ws| ws.canvas_mut(false))
        {
            canvas.center_window(Some(window));
        }
    }

    /// `window` got a new title: during a restore it may trade spots (see
    /// [`CanvasSpace::retitled`]).
    pub fn canvas_retitled(&mut self, window: &W::Id) {
        if let Some(canvas) = self
            .workspaces_mut()
            .find(|ws| ws.has_window(window))
            .and_then(|ws| ws.canvas_mut(false))
        {
            canvas.retitled(window);
        }
    }

    pub fn canvas_zoom(&mut self, delta: i32) {
        if let Some(canvas) = self.active_canvas() {
            canvas.zoom(delta);
        }
    }

    /// The active canvas's zoom, with the bounds and step it zooms by.
    pub fn canvas_zoom_state(&mut self) -> Option<CanvasZoom> {
        let canvas = self.active_canvas()?.canvas();
        let config = canvas.config();
        Some(CanvasZoom {
            zoom: canvas.zoom(),
            step: config.zoom_step,
            min: config.zoom_min,
            max: config.zoom_max,
        })
    }

    /// Returns the active canvas's camera to 1:1 (see [`CanvasSpace::zoom_reset`]).
    pub fn canvas_zoom_reset(&mut self) {
        if let Some(canvas) = self.active_canvas() {
            canvas.zoom_reset();
        }
    }

    /// Pulls the active canvas's camera back to show every window (see
    /// [`CanvasSpace::zoom_to_fit`]).
    pub fn canvas_zoom_to_fit(&mut self) {
        if let Some(canvas) = self
            .active_workspace_mut()
            .and_then(|ws| ws.canvas_mut(false))
        {
            canvas.zoom_to_fit();
        }
    }

    /// Returns every canvas [`Self::canvas_zoom_to_fit`] pulled back to the zoom it had. Returns
    /// whether there was one.
    pub fn canvas_zoom_back(&mut self) -> bool {
        let mut rv = false;
        for ws in self.workspaces_mut() {
            if let Some(canvas) = ws.canvas_mut(false) {
                rv |= canvas.zoom_back();
            }
        }
        rv
    }

    /// Moves the focus of the active canvas to the neighbour in `dir`, as Super+H/J/K/L do;
    /// nothing when the focus is floating.
    pub fn canvas_focus(&mut self, dir: Dir) -> bool {
        self.active_canvas()
            .is_some_and(|canvas| canvas.focus_dir(dir))
    }

    /// Moves the focused tiled window of the active canvas one tile in `dir`, as
    /// Super+Shift+H/J/K/L do; nothing when the focus is floating, fullscreen or maximized.
    pub fn canvas_move_focus(&mut self, dir: Dir) -> bool {
        self.active_canvas()
            .is_some_and(|canvas| canvas.move_focus_on_grid(dir))
    }

    /// The touchpad motion a pan takes to scroll the active canvas one tile along each axis, at
    /// the zoom the camera goes to.
    pub fn canvas_touchpad_tile_motion(&self) -> Option<(f64, f64)> {
        Some(self.active_workspace()?.scrolling().touchpad_tile_motion())
    }

    pub fn canvas_send_to_edge(&mut self, dir: Dir) {
        if let Some(canvas) = self.active_canvas() {
            canvas.send_to_edge(dir);
        }
    }

    pub fn canvas_swap_neighbour(&mut self, dir: Dir) -> bool {
        self.active_canvas()
            .is_some_and(|canvas| canvas.swap_neighbour(dir))
    }

    /// Takes back the active canvas's last rearrangement.
    pub fn canvas_undo(&mut self) -> bool {
        self.active_canvas().is_some_and(|canvas| canvas.undo())
    }

    /// Does again the active canvas's last rearrangement undone.
    pub fn canvas_redo(&mut self) -> bool {
        self.active_canvas().is_some_and(|canvas| canvas.redo())
    }

    /// Brings the active canvas's windows into one island.
    pub fn canvas_gather_islands(&mut self) -> bool {
        self.active_canvas()
            .is_some_and(|canvas| canvas.gather_islands())
    }

    /// How floating windows follow the canvas camera now.
    pub fn floating_follow(&self) -> FloatingFollow {
        self.options.floating_follow
    }

    /// Makes floating windows follow the canvas camera as `mode` says, until the config changes
    /// its `floating-follow`.
    pub fn set_floating_follow(&mut self, mode: FloatingFollow) {
        if self.options.floating_follow != mode {
            let mut options = Options::clone(&self.options);
            options.floating_follow = mode;
            self.update_options(options);
        }
    }

    /// Switches floating windows to the next way of following the canvas camera.
    pub fn cycle_floating_follow(&mut self) {
        self.set_floating_follow(self.options.floating_follow.next());
    }
}

/// Runs `step` until it answers false, at most `limit` times: a walk across `limit` canvas
/// windows takes at most that many steps.
fn repeat_while(limit: usize, mut step: impl FnMut() -> bool) {
    for _ in 0..limit {
        if !step() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{repeat_while, Kept};

    #[test]
    fn a_far_focus_walks_to_the_last_canvas_window() {
        // Five windows in a row, the focus on the first: four steps reach the last, and the
        // walk stops there.
        let (windows, mut focus, mut steps) = (5, 0, 0);
        repeat_while(windows, || {
            steps += 1;
            let moved = focus + 1 < windows;
            focus += usize::from(moved);
            moved
        });
        assert_eq!((focus, steps), (4, 5));
        let mut steps = 0;
        repeat_while(3, || {
            steps += 1;
            true
        });
        assert_eq!(steps, 3, "bounded by the window count");
    }

    #[test]
    fn kept_size_follows_refusals_and_falls_back() {
        let kept = Kept::default().after(704, 704);
        assert_eq!(kept, Kept::default(), "the app took the size asked");
        let kept = kept.after(704, 800);
        assert_eq!(kept.size, 800, "asked for 704, it kept 800");
        let kept = kept.after(1424, 1424);
        assert_eq!(
            kept,
            Kept {
                size: 800,
                margin: 0
            },
            "a larger tile keeps the minimum"
        );
        let kept = kept.after(1424, 1000);
        assert_eq!(
            kept.size, 800,
            "smaller than asked, still above what it kept"
        );
        assert_eq!(
            kept.after(1424, 700),
            Kept::default(),
            "it accepts less than it kept"
        );
        assert_eq!(kept.after(0, 5000), kept, "no size asked, nothing refused");
    }

    #[test]
    fn kept_size_stops_an_app_that_chases_its_tile() {
        let kept = Kept::default().after(704, 714);
        assert_eq!(kept.size, 714);
        let kept = kept.after(1424, 1434);
        assert_eq!(
            kept.size, 714,
            "the same 10 px over the larger tile: no growth"
        );
        let kept = kept.after(1424, 1500);
        assert_eq!(kept.size, 1500, "a new refusal grows it again");
    }
}
