//! The minimap: a small plan of the canvas in the bottom-left corner of the screen, shown for a
//! moment whenever the camera, the focus or a window moves.
//!
//! The canvas has no edges, so the plan has none either: it is a window onto the canvas around
//! what the screen shows. The screen's outline sits in the middle at one size whatever the zoom,
//! and the canvas slides under it as the camera pans, its tile corners drawn as dots. The plan
//! has no frame: it fades out toward its sides, and what lies outside the screen's outline is
//! dimmed. Every window is drawn on its tiles with as many letters of its name as fit: its app's
//! name, or for a terminal connected to another machine with ssh, that machine's. A window too
//! far away to show leaves a mark on the side it lies toward.
//!
//! It is painted here, pixel by pixel, with a 3 by 5 pixel font of its own: newm-rs is built
//! without the `text` feature, so there is no font or drawing library to lean on.
//!
//! Two accents only, on near black: the border's active colour (the theme's accent) for the
//! focused window, Nord's frost for the others.

use std::time::Duration;

use newm_canvas::{Id, Rect};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::Kind;
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};

use super::CanvasSpace;
use crate::layout::LayoutElement;
use crate::render_helpers::renderer::NiriRenderer;

/// The plan's size, in logical pixels.
const SIZE: (f64, f64) = (240., 160.);
/// Its distance from the screen's left and bottom edges.
const MARGIN: f64 = 12.;
/// The most of the plan's width and height the screen's outline takes.
const VIEW_SHARE: (f64, f64) = (0.4, 0.45);
/// How far from each side the plan fades out.
const FADE: f64 = 18.;
/// The least distance between tile dots: on a camera pulled far back they are left out.
const DOTS_MIN: f64 = 5.;
/// How far inside the plan the mark of a window beyond it sits.
const MARK_INSET: f64 = 7.;
/// The label font: letters 3 by [`FONT_SIZE`] logical pixels, [`LETTER`] apart.
const FONT_SIZE: f64 = 5.;
const LETTER: f64 = 4.;
/// The room between a window's edge and its label.
const LABEL_PAD: f64 = 2.;
/// How long the plan stays after the last move.
const SHOWN: Duration = Duration::from_millis(750);

/// Nord's frost, nord8 (#88C0D0).
const FROST: [f64; 3] = [0x88 as f64 / 255., 0xc0 as f64 / 255., 0xd0 as f64 / 255.];
/// The plan's near black (#0B0C16) and the outline's near white (#DDF7FF).
const DARK: [f64; 3] = [0x0b as f64 / 255., 0x0c as f64 / 255., 0x16 as f64 / 255.];
const LIGHT: [f64; 3] = [0xdd as f64 / 255., 0xf7 as f64 / 255., 1.];

/// A window on the plan.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PlanWindow {
    /// Where it is drawn, from the plan's top-left corner; it may reach past the plan.
    pub rect: Rectangle<f64, Logical>,
    pub focused: bool,
    /// The letters of its app's name that fit in it.
    pub label: Option<String>,
}

/// What the minimap draws, from the plan's top-left corner.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Plan {
    /// The plan's top-left corner on the screen.
    pub loc: Point<f64, Logical>,
    /// The outline of what the screen shows.
    pub view: Rectangle<f64, Logical>,
    /// The windows that reach into the plan, the focused one last.
    pub windows: Vec<PlanWindow>,
    /// A mark for every window beyond the plan, on the side it lies toward, and whether it is
    /// the focused one.
    pub marks: Vec<(Point<f64, Logical>, bool)>,
    /// The first tile corner (at or after the plan's top-left) and the distance between them;
    /// `None` when the corners are closer than [`DOTS_MIN`].
    pub dots: Option<(Point<f64, Logical>, (f64, f64))>,
}

/// What the minimap last drew from: when any of it changes, something moved.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    view: Rectangle<i32, Logical>,
    focus: Option<Id>,
    windows: Vec<(Id, Rect)>,
}

#[derive(Debug, Default)]
pub struct Minimap {
    seen: Option<Seen>,
    until: Option<Duration>,
    /// The plan, accent and scale the buffer was painted from.
    painted: Option<(Plan, [f64; 3], f64)>,
    buffer: Option<MemoryRenderBuffer>,
}

/// The name a window's app is known by: the last part of its app ID, in lower case
/// (`com.mitchellh.ghostty` is `ghostty`). The font draws it in capitals.
pub(super) fn app_name(app_id: &str) -> String {
    let name = app_id.rsplit('.').find(|part| !part.is_empty());
    name.unwrap_or(app_id).to_lowercase()
}

/// The name a remote machine is known by: the first part of the host the user gave ssh
/// (`lab.example` is `lab`), or all of an address.
pub(super) fn host_name(host: &str) -> String {
    let numeric = |part: &str| part.chars().all(|c| c.is_ascii_digit());
    let first = host.split('.').next().unwrap_or(host);
    let name = if host.contains(':') || numeric(first) {
        host
    } else {
        first
    };
    name.to_lowercase()
}

/// The letters of `name` that fit in a window drawn `size` large: none when it is lower than a
/// line or narrower than a letter.
pub(super) fn label_fit(name: &str, size: Size<f64, Logical>) -> Option<String> {
    if size.h < FONT_SIZE + LABEL_PAD * 2. {
        return None;
    }
    // The last letter needs no gap after it.
    let letters = ((size.w - LABEL_PAD * 2. + 1.) / LETTER).floor();
    if letters < 1. || name.is_empty() {
        return None;
    }
    Some(name.chars().take(letters as usize).collect())
}

/// The minimap on a screen of `screen` size. `windows` are the canvas's windows, each with
/// whether it has the focus and its app's name, and `view` is the part of the canvas the screen
/// shows, all in the canvas's logical pixels; `tile` is the size of one tile there.
///
/// `view` is drawn in the middle, [`VIEW_SHARE`] of the plan, so one scale maps the canvas
/// around it: a camera pulled back keeps the outline's size and draws the windows smaller.
/// Nothing without a window.
pub(super) fn plan(
    windows: &[(Rectangle<f64, Logical>, bool, String)],
    view: Rectangle<f64, Logical>,
    tile: (f64, f64),
    screen: Size<f64, Logical>,
) -> Option<Plan> {
    if windows.is_empty() || view.size.w <= 0. || view.size.h <= 0. {
        return None;
    }
    let k = f64::min(
        SIZE.0 * VIEW_SHARE.0 / view.size.w,
        SIZE.1 * VIEW_SHARE.1 / view.size.h,
    );
    let middle = view.loc + view.size.downscale(2.).to_point();
    let centre = Point::from((SIZE.0 / 2., SIZE.1 / 2.));
    let place = |r: Rectangle<f64, Logical>| {
        Rectangle::new((r.loc - middle).upscale(k) + centre, r.size.upscale(k))
    };
    let whole = Rectangle::from_size(Size::from(SIZE));

    let mut shown = Vec::new();
    let mut marks = Vec::new();
    for (rect, focused, name) in windows {
        let rect = place(*rect);
        if rect.overlaps(whole) {
            shown.push(PlanWindow {
                rect,
                focused: *focused,
                label: label_fit(name, rect.size),
            });
        } else {
            let at = rect.loc + rect.size.downscale(2.).to_point();
            let mark = Point::from((
                at.x.clamp(MARK_INSET, SIZE.0 - MARK_INSET),
                at.y.clamp(MARK_INSET, SIZE.1 - MARK_INSET),
            ));
            marks.push((mark, *focused));
        }
    }
    shown.sort_by_key(|w| w.focused);

    let step = (tile.0 * k, tile.1 * k);
    let dots = (step.0 >= DOTS_MIN && step.1 >= DOTS_MIN).then(|| {
        let origin = place(Rectangle::default()).loc;
        let first = Point::from((origin.x.rem_euclid(step.0), origin.y.rem_euclid(step.1)));
        (first, step)
    });

    Some(Plan {
        loc: Point::from((MARGIN, screen.h - MARGIN - SIZE.1)),
        view: place(view),
        windows: shown,
        marks,
        dots,
    })
}

/// A letter of the 3 by 5 font: five rows, top first, the left pixel in bit 2. Letters are
/// capitals; what the font lacks is a low dot.
fn glyph(c: char) -> [u8; 5] {
    let rows: u16 = match c.to_ascii_uppercase() {
        'A' => 0o25755,
        'B' => 0o65656,
        'C' => 0o34443,
        'D' => 0o65556,
        'E' => 0o74647,
        'F' => 0o74644,
        'G' => 0o34553,
        'H' => 0o55755,
        'I' => 0o72227,
        'J' => 0o11152,
        'K' => 0o55655,
        'L' => 0o44447,
        'M' => 0o57755,
        'N' => 0o75555,
        'O' => 0o25552,
        'P' => 0o65644,
        'Q' => 0o25573,
        'R' => 0o65655,
        'S' => 0o34216,
        'T' => 0o72222,
        'U' => 0o55557,
        'V' => 0o55552,
        'W' => 0o55775,
        'X' => 0o55255,
        'Y' => 0o55222,
        'Z' => 0o71247,
        '0' => 0o75557,
        '1' => 0o26227,
        '2' => 0o61247,
        '3' => 0o61216,
        '4' => 0o55711,
        '5' => 0o74616,
        '6' => 0o34652,
        '7' => 0o71222,
        '8' => 0o25252,
        '9' => 0o25316,
        '-' | '_' => 0o00700,
        ':' => 0o02020,
        ' ' => 0,
        _ => 0o00002,
    };
    [12, 9, 6, 3, 0].map(|shift| ((rows >> shift) & 7) as u8)
}

/// The plan's pixels while it is painted: premultiplied RGBA, `scale` pixels to a logical one.
struct Pixels {
    w: usize,
    h: usize,
    scale: f64,
    data: Vec<[f32; 4]>,
}

impl Pixels {
    fn new(scale: f64) -> Self {
        let (w, h) = pixels(scale);
        Self {
            w: w as usize,
            h: h as usize,
            scale,
            data: vec![[0.; 4]; (w * h) as usize],
        }
    }

    /// Paints `colour` at `alpha` over the logical rectangle from `(x0, y0)` to `(x1, y1)`, cut
    /// to the plan; a rectangle thinner than a pixel still takes one.
    fn fill(&mut self, (x0, y0, x1, y1): (f64, f64, f64, f64), colour: [f64; 3], alpha: f64) {
        let px = |v: f64, most: usize| (v * self.scale).round().clamp(0., most as f64) as usize;
        let (left, top) = (px(x0, self.w), px(y0, self.h));
        let right = px(x1, self.w).max(left + 1).min(self.w);
        let bottom = px(y1, self.h).max(top + 1).min(self.h);
        let a = alpha as f32;
        let src = [
            colour[0] as f32 * a,
            colour[1] as f32 * a,
            colour[2] as f32 * a,
            a,
        ];
        for y in top..bottom {
            for dst in &mut self.data[y * self.w + left..y * self.w + right] {
                for (d, s) in dst.iter_mut().zip(src) {
                    *d = s + *d * (1. - a);
                }
            }
        }
    }

    fn rect(&mut self, r: Rectangle<f64, Logical>, colour: [f64; 3], alpha: f64) {
        let far = r.loc + r.size.to_point();
        self.fill((r.loc.x, r.loc.y, far.x, far.y), colour, alpha);
    }

    /// The outline of `r`, `width` thick, inside it.
    fn outline(&mut self, r: Rectangle<f64, Logical>, width: f64, colour: [f64; 3]) {
        let (x0, y0) = (r.loc.x, r.loc.y);
        let (x1, y1) = (x0 + r.size.w, y0 + r.size.h);
        self.fill((x0, y0, x1, y0 + width), colour, 1.);
        self.fill((x0, y1 - width, x1, y1), colour, 1.);
        self.fill((x0, y0 + width, x0 + width, y1 - width), colour, 1.);
        self.fill((x1 - width, y0 + width, x1, y1 - width), colour, 1.);
    }

    /// `text` in the 3 by 5 font, its top-left corner at `at`.
    fn text(&mut self, text: &str, at: Point<f64, Logical>, colour: [f64; 3]) {
        for (n, c) in text.chars().enumerate() {
            let x = at.x + n as f64 * LETTER;
            for (row, bits) in glyph(c).into_iter().enumerate() {
                for col in 0..3 {
                    if bits & (4 >> col) != 0 {
                        let (x, y) = (x + f64::from(col), at.y + row as f64);
                        self.fill((x, y, x + 1., y + 1.), colour, 1.);
                    }
                }
            }
        }
    }

    /// Fades the plan out toward its four sides, over [`FADE`] from each.
    fn fade(&mut self) {
        let fade = FADE * self.scale;
        let share = |at: usize, length: usize| {
            let from_side = f64::min(at as f64 + 0.5, length as f64 - at as f64 - 0.5);
            (from_side / fade).clamp(0., 1.) as f32
        };
        for y in 0..self.h {
            for x in 0..self.w {
                let f = share(x, self.w) * share(y, self.h);
                for v in &mut self.data[y * self.w + x] {
                    *v *= f;
                }
            }
        }
    }

    /// The pixels as premultiplied ARGB8888 bytes.
    fn bytes(&self) -> Vec<u8> {
        let byte = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
        let bgra = |p: &[f32; 4]| [byte(p[2]), byte(p[1]), byte(p[0]), byte(p[3])];
        self.data.iter().flat_map(bgra).collect()
    }
}

/// The plan's size in physical pixels at `scale`.
fn pixels(scale: f64) -> (i32, i32) {
    (
        (SIZE.0 * scale).ceil() as i32,
        (SIZE.1 * scale).ceil() as i32,
    )
}

/// Paints the plan: the tile dots, the windows with their labels, the dimming of what the screen
/// does not show, the screen's outline, the marks, scanlines, and the fade toward the sides.
fn paint(plan: &Plan, accent: [f64; 3], scale: f64) -> Pixels {
    let mut px = Pixels::new(scale);
    px.fill((0., 0., SIZE.0, SIZE.1), DARK, 0.8);

    if let Some((first, step)) = plan.dots {
        let mut y = first.y;
        while y < SIZE.1 {
            let mut x = first.x;
            while x < SIZE.0 {
                px.fill((x - 0.5, y - 0.5, x + 0.5, y + 0.5), FROST, 0.45);
                x += step.0;
            }
            y += step.1;
        }
    }

    for window in &plan.windows {
        let colour = if window.focused { accent } else { FROST };
        px.rect(window.rect, colour, if window.focused { 0.75 } else { 0.2 });
        px.outline(window.rect, 1., colour);
        if let Some(label) = &window.label {
            let at = window.rect.loc + Point::from((LABEL_PAD, LABEL_PAD));
            px.text(label, at, if window.focused { DARK } else { LIGHT });
        }
    }

    // What the screen does not show is dimmed, so the eye lands on what it does.
    let view = plan.view;
    let (vx0, vy0) = (view.loc.x, view.loc.y);
    let (vx1, vy1) = (vx0 + view.size.w, vy0 + view.size.h);
    px.fill((0., 0., SIZE.0, vy0), DARK, 0.5);
    px.fill((0., vy1, SIZE.0, SIZE.1), DARK, 0.5);
    px.fill((0., vy0, vx0, vy1), DARK, 0.5);
    px.fill((vx1, vy0, SIZE.0, vy1), DARK, 0.5);
    px.outline(view, 1.5, LIGHT);

    for (at, focused) in &plan.marks {
        let colour = if *focused { accent } else { FROST };
        px.fill((at.x - 2., at.y - 2., at.x + 2., at.y + 2.), colour, 1.);
    }

    let mut y = 2.;
    while y < SIZE.1 {
        px.fill((0., y, SIZE.0, y + 1.), [0.; 3], 0.14);
        y += 3.;
    }
    px.fade();
    px
}

impl<W: LayoutElement> CanvasSpace<W> {
    /// The part of the canvas the screen shows, in the canvas's logical pixels.
    fn minimap_view(&self) -> Rectangle<f64, Logical> {
        let mut view = self
            .camera()
            .rect_from_view(Rectangle::from_size(self.view_size));
        view.loc += self.view_point();
        view
    }

    /// Shows the minimap for [`SHOWN`] from now when the camera, the focus or a window moved
    /// since the last call; the first call only looks. Called once a frame.
    pub(super) fn minimap_watch(&mut self) {
        let now = self.clock.now_unadjusted();
        if self.minimap.until.is_some_and(|until| until <= now) {
            self.minimap.until = None;
        }
        let seen = Seen {
            view: self.minimap_view().to_i32_round(),
            focus: self.canvas.focus(),
            windows: self
                .entries
                .iter()
                .filter_map(|e| Some((e.id, self.shown_rect(e.id)?)))
                .collect(),
        };
        if self.minimap.seen.as_ref() == Some(&seen) {
            return;
        }
        if self.minimap.seen.replace(seen).is_some() {
            self.minimap.until = Some(now + SHOWN);
        }
    }

    pub(super) fn minimap_shown(&self) -> bool {
        self.minimap.until.is_some()
    }

    /// Paints the minimap for the frame about to be drawn, when what it shows changed.
    pub(super) fn minimap_update(&mut self) {
        let plan = self.minimap_shown().then(|| {
            let focus = self.canvas.focus();
            let windows: Vec<_> = self
                .entries
                .iter()
                .filter_map(|e| {
                    let cell = self.cell(self.shown_rect(e.id)?);
                    let window = e.tile.window();
                    let name = match window.remote_host() {
                        Some(host) => host_name(&host),
                        None => app_name(&window.app_id().unwrap_or_default()),
                    };
                    Some((cell, Some(e.id) == focus, name))
                })
                .collect();
            let tile = self.canvas.base_tile();
            plan(&windows, self.minimap_view(), tile, self.view_size)
        });
        let Some(plan) = plan.flatten() else {
            self.minimap.painted = None;
            self.minimap.buffer = None;
            return;
        };
        let [r, g, b, _] = self.options.layout.border.active_color.to_array_unpremul();
        let painted = (plan, [r, g, b].map(f64::from), self.scale);
        if self.minimap.painted.as_ref() == Some(&painted) {
            return;
        }
        let px = paint(&painted.0, painted.1, painted.2);
        self.minimap.buffer = Some(MemoryRenderBuffer::from_slice(
            &px.bytes(),
            Fourcc::Argb8888,
            (px.w as i32, px.h as i32),
            1,
            Transform::Normal,
            None,
        ));
        self.minimap.painted = Some(painted);
    }

    /// The minimap's element; none while it is hidden.
    pub(super) fn minimap_render<R: NiriRenderer>(
        &self,
        renderer: &mut R,
    ) -> Option<MemoryRenderBufferRenderElement<R>> {
        let buffer = self.minimap.buffer.as_ref()?;
        let (plan, ..) = self.minimap.painted.as_ref()?;
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            plan.loc.to_physical_precise_round(self.scale),
            buffer,
            None,
            // The whole buffer, which holds physical pixels, drawn at the plan's logical size.
            Some(Rectangle::from_size(
                Size::from(pixels(self.scale)).to_f64(),
            )),
            Some(Size::from(SIZE).to_i32_round()),
            Kind::Unspecified,
        )
        .inspect_err(|err| warn!("error importing the minimap: {err:?}"))
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new(Point::from((x, y)), Size::from((w, h)))
    }

    fn win(r: Rectangle<f64, Logical>, focused: bool) -> (Rectangle<f64, Logical>, bool, String) {
        (r, focused, "ghostty".to_owned())
    }

    const SCREEN: (f64, f64) = (1440., 960.);
    const TILE: (f64, f64) = (119., 113.);

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn plan_of(
        windows: &[(Rectangle<f64, Logical>, bool, String)],
        view: Rectangle<f64, Logical>,
    ) -> Plan {
        plan(windows, view, TILE, SCREEN.into()).unwrap()
    }

    #[test]
    fn no_windows_no_minimap() {
        let view = rect(0., 0., 1440., 960.);
        assert_eq!(plan(&[], view, TILE, SCREEN.into()), None);
    }

    #[test]
    fn sits_in_the_bottom_left_corner() {
        let windows = [win(rect(0., 0., 1000., 800.), true)];
        let plan = plan_of(&windows, rect(0., 0., 1440., 960.));
        assert_eq!(plan.loc, Point::from((MARGIN, SCREEN.1 - MARGIN - SIZE.1)));
    }

    #[test]
    fn the_screen_stays_in_the_middle_at_one_size_wherever_the_camera_is() {
        let windows = [win(rect(0., 0., 1000., 800.), true)];
        let views = [
            rect(0., 0., 1440., 960.),
            rect(-5000., 9000., 1440., 960.),
            rect(300., 300., 2880., 1920.),
        ]
        .map(|view| plan_of(&windows, view).view);
        let near = |a: Rectangle<f64, Logical>, b: Rectangle<f64, Logical>| {
            close(a.loc.x, b.loc.x)
                && close(a.loc.y, b.loc.y)
                && close(a.size.w, b.size.w)
                && close(a.size.h, b.size.h)
        };
        assert!(
            near(views[0], views[1]),
            "a pan moves the canvas, not the outline"
        );
        assert!(
            near(views[0], views[2]),
            "a camera pulled back keeps the outline's size"
        );
        let view = views[0];
        assert!(close(view.size.w, SIZE.0 * VIEW_SHARE.0));
        assert!(close(view.loc.x + view.size.w / 2., SIZE.0 / 2.));
        assert!(close(view.loc.y + view.size.h / 2., SIZE.1 / 2.));
        assert!(view.size.h <= SIZE.1 * VIEW_SHARE.1 + 1e-9);
    }

    #[test]
    fn windows_keep_their_place_and_proportions_around_the_screen() {
        let view = rect(1000., 1000., 1440., 960.);
        let windows = [
            win(rect(1000., 1000., 720., 960.), true),
            win(rect(1720., 1000., 720., 480.), false),
        ];
        let plan = plan_of(&windows, view);
        let [other, focused] = &plan.windows[..] else {
            panic!("two windows, the focused one last, drawn over the others");
        };
        assert!(focused.focused && !other.focused);
        assert!(close(focused.rect.loc.x, plan.view.loc.x));
        assert!(close(focused.rect.loc.y, plan.view.loc.y));
        assert!(close(focused.rect.size.w, plan.view.size.w / 2.));
        assert!(close(focused.rect.size.h, plan.view.size.h));
        assert!(close(
            other.rect.loc.x,
            focused.rect.loc.x + focused.rect.size.w
        ));
        assert!(close(other.rect.size.h, focused.rect.size.h / 2.));
        assert!(plan.marks.is_empty());
    }

    #[test]
    fn a_camera_pulled_back_draws_the_windows_smaller() {
        let windows = [win(rect(0., 0., 1440., 960.), true)];
        let near = plan_of(&windows, rect(0., 0., 1440., 960.));
        let far = plan_of(&windows, rect(-720., -480., 2880., 1920.));
        assert!(close(
            far.windows[0].rect.size.w,
            near.windows[0].rect.size.w / 2.
        ));
    }

    #[test]
    fn a_window_beyond_the_plan_leaves_a_mark_on_its_side() {
        let view = rect(0., 0., 1440., 960.);
        let windows = [
            win(rect(0., 0., 1000., 800.), false),
            win(rect(20000., 200., 1000., 800.), true),
            win(rect(200., -30000., 1000., 800.), false),
        ];
        let plan = plan_of(&windows, view);
        assert_eq!(plan.windows.len(), 1, "only the near window is drawn");
        let [(right, right_focused), (up, up_focused)] = plan.marks[..] else {
            panic!("a mark for each far window");
        };
        assert!(right_focused && !up_focused);
        assert!(
            close(right.x, SIZE.0 - MARK_INSET),
            "far to the right: on the right side"
        );
        assert!(right.y > MARK_INSET && right.y < SIZE.1 - MARK_INSET);
        assert!(close(up.y, MARK_INSET), "far above: on the top side");
    }

    #[test]
    fn tile_dots_slide_with_the_canvas_and_go_when_too_close() {
        let windows = [win(rect(0., 0., 1000., 800.), true)];
        let at = |x: f64| plan_of(&windows, rect(x, 0., 1440., 960.)).dots.unwrap();
        let (first, step) = at(0.);
        assert!(close(step.0, TILE.0 * SIZE.0 * VIEW_SHARE.0 / 1440.));
        assert!(first.x >= 0. && first.x < step.0 && first.y >= 0. && first.y < step.1);
        let (moved, _) = at(TILE.0 / 2.);
        let slid = (first.x - moved.x).rem_euclid(step.0);
        assert!(
            close(slid, step.0 / 2.),
            "half a tile of pan slides the dots half a step"
        );
        let (whole, _) = at(TILE.0);
        assert!(
            (whole.x - first.x).abs() < 1e-6,
            "a whole tile brings them back"
        );
        let far = rect(0., 0., 14400., 9600.);
        assert_eq!(plan_of(&windows, far).dots, None);
    }

    #[test]
    fn app_names_are_the_last_part_of_the_app_id() {
        assert_eq!(app_name("com.mitchellh.ghostty"), "ghostty");
        assert_eq!(app_name("chromium-browser"), "chromium-browser");
        assert_eq!(app_name("Alacritty"), "alacritty");
        assert_eq!(app_name("org.kde.dolphin."), "dolphin");
        assert_eq!(app_name(""), "");
    }

    #[test]
    fn host_names_are_the_first_part_or_the_whole_address() {
        assert_eq!(host_name("desk"), "desk");
        assert_eq!(host_name("Lab.Example.org"), "lab");
        assert_eq!(host_name("100.64.0.24"), "100.64.0.24");
        assert_eq!(host_name("fd7a::1"), "fd7a::1");
    }

    #[test]
    fn labels_take_the_letters_that_fit() {
        let fit = |w: f64, h: f64| label_fit("ghostty", Size::from((w, h)));
        assert_eq!(fit(100., 40.).as_deref(), Some("ghostty"));
        let three = LABEL_PAD * 2. + LETTER * 3. - 1.;
        assert_eq!(
            fit(three, 40.).as_deref(),
            Some("gho"),
            "the last needs no gap"
        );
        assert_eq!(fit(three - 0.1, 40.).as_deref(), Some("gh"));
        assert_eq!(fit(LABEL_PAD * 2. + 3., 40.).as_deref(), Some("g"));
        assert_eq!(
            fit(LABEL_PAD * 2. + 2.9, 40.),
            None,
            "narrower than a letter"
        );
        assert_eq!(fit(100., FONT_SIZE + LABEL_PAD), None, "lower than a line");
        assert_eq!(label_fit("", Size::from((100., 40.))), None);
    }

    #[test]
    fn the_font_draws_capitals_digits_and_a_dot_for_the_rest() {
        assert_eq!(glyph('a'), glyph('A'));
        assert_eq!(glyph('A'), [0b010, 0b101, 0b111, 0b101, 0b101]);
        assert_eq!(glyph('L'), [0b100, 0b100, 0b100, 0b100, 0b111]);
        assert_eq!(glyph('1'), [0b010, 0b110, 0b010, 0b010, 0b111]);
        assert_eq!(glyph(' '), [0; 5]);
        assert_eq!(glyph('~'), [0, 0, 0, 0, 0b010]);
        let mut seen = std::collections::HashSet::new();
        for c in ('a'..='z').chain('0'..='9') {
            assert!(seen.insert(glyph(c)), "{c} looks like another letter");
        }
    }

    #[test]
    fn text_paints_the_letters_pixels() {
        let mut px = Pixels::new(1.);
        px.text("L", Point::from((10., 20.)), [1., 1., 1.]);
        let lit = |x: usize, y: usize| px.data[y * px.w + x][3] > 0.5;
        assert!(
            lit(10, 20) && lit(10, 24) && lit(12, 24),
            "the L's stem and foot"
        );
        assert!(!lit(11, 20) && !lit(12, 22) && !lit(13, 24));
        let mut twice = Pixels::new(2.);
        twice.text("L", Point::from((10., 20.)), [1., 1., 1.]);
        let lit = |x: usize, y: usize| twice.data[y * twice.w + x][3] > 0.5;
        assert!(
            lit(20, 40) && lit(21, 41) && lit(25, 49),
            "each font pixel is two across"
        );
        assert!(!lit(22, 40));
    }

    #[test]
    fn a_plan_paints_at_any_scale_and_fades_out_at_its_sides() {
        let view = rect(0., 0., 1440., 960.);
        let windows = [
            win(rect(0., 0., 720., 960.), true),
            win(rect(720., 0., 720., 480.), false),
            win(rect(20000., 0., 720., 480.), false),
        ];
        let plan = plan_of(&windows, view);
        for scale in [1., 1.25, 2.] {
            let px = paint(&plan, [0.5, 1., 0.6], scale);
            assert_eq!((px.w as i32, px.h as i32), pixels(scale));
            let bytes = px.bytes();
            assert_eq!(bytes.len(), px.w * px.h * 4);
            let alpha = |x: usize, y: usize| bytes[(y * px.w + x) * 4 + 3];
            assert!(alpha(0, px.h / 2) < 16, "faded out at the side");
            assert!(alpha(px.w / 2, 0) < 16, "and at the top");
            assert!(alpha(px.w / 2, px.h / 2) > 200, "solid in the middle");
            for p in bytes.chunks_exact(4) {
                assert!(
                    p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3],
                    "premultiplied"
                );
            }
        }
    }
}
