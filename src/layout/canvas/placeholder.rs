//! A reserved tile drawn where a launching app's window will open.

use std::fs::File;
use std::io::BufReader;
use std::time::Duration;

use anyhow::Context as _;
use newm_canvas::Id;
use niri_config::CornerRadius;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::Kind;
use smithay::utils::{Logical, Point, Rectangle, Size, Transform};

use crate::layout::focus_ring::{FocusRing, FocusRingRenderElement};
use crate::render_helpers::renderer::NiriRenderer;

/// How long a placeholder waits for its window.
pub const LIFETIME: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Placeholder {
    pub id: Id,
    pub app_id: String,
    /// The title of the window a saved session puts back here: of the windows of its app, the
    /// one with this title takes it first.
    pub title: Option<String>,
    pub deadline: Duration,
    /// When it was put down, for the usage log's wait until a window took it.
    pub created: Duration,
    box_: FocusRing,
    icon: Option<(MemoryRenderBuffer, Size<i32, Logical>)>,
}

impl Placeholder {
    pub fn new(
        id: Id,
        app_id: String,
        title: Option<String>,
        icon: Option<&str>,
        focus_ring: niri_config::FocusRing,
        deadline: Duration,
        created: Duration,
    ) -> Self {
        let box_ = FocusRing::new(niri_config::FocusRing {
            off: false,
            width: 0.,
            ..focus_ring
        });
        let icon = icon.and_then(|path| match load_png(path) {
            Ok(icon) => Some(icon),
            Err(err) => {
                warn!("error loading placeholder icon {path}: {err:?}");
                None
            }
        });
        Self {
            id,
            app_id,
            title,
            deadline,
            created,
            box_,
            icon,
        }
    }

    pub fn update_render_elements(
        &mut self,
        size: Size<f64, Logical>,
        view_rect: Rectangle<f64, Logical>,
        scale: f64,
    ) {
        self.box_.update_render_elements(
            size,
            true,
            false,
            false,
            view_rect,
            CornerRadius::from(12f32),
            scale,
            0.5,
        );
    }

    pub fn update_shaders(&mut self) {
        self.box_.update_shaders();
    }

    /// The icon centred in the box, a third of its smaller side.
    pub fn render_icon<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        pos: Point<f64, Logical>,
        size: Size<f64, Logical>,
        scale: f64,
    ) -> Option<MemoryRenderBufferRenderElement<R>> {
        let (buffer, icon_size) = self.icon.as_ref()?;
        let side = f64::min(size.w, size.h) / 3.;
        let fit = side / f64::from(icon_size.w.max(icon_size.h));
        let icon = icon_size.to_f64().upscale(fit);
        let loc = pos + (size.to_point() - icon.to_point()).downscale(2.);
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            loc.to_physical_precise_round(scale),
            buffer,
            None,
            None,
            Some(icon.to_i32_round()),
            Kind::Unspecified,
        )
        .inspect_err(|err| warn!("error importing a placeholder icon: {err:?}"))
        .ok()
    }

    pub fn render_box(
        &self,
        renderer: &mut impl NiriRenderer,
        pos: Point<f64, Logical>,
        push: &mut dyn FnMut(FocusRingRenderElement),
    ) {
        self.box_.render(renderer, pos, push);
    }
}

fn load_png(path: &str) -> anyhow::Result<(MemoryRenderBuffer, Size<i32, Logical>)> {
    let mut decoder = png::Decoder::new(BufReader::new(File::open(path)?));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let len = reader.output_buffer_size().context("image too large")?;
    let mut buf = vec![0; len];
    let info = reader.next_frame(&mut buf)?;
    let pixels = info.width as usize * info.height as usize;
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..pixels * 4].to_vec(),
        png::ColorType::Rgb => buf[..pixels * 3]
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf[..pixels * 2]
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf[..pixels].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => anyhow::bail!("indexed image was not expanded"),
    };
    let size = Size::from((info.width as i32, info.height as i32));
    let buffer = MemoryRenderBuffer::from_slice(
        &rgba,
        Fourcc::Abgr8888,
        (size.w, size.h),
        1,
        Transform::Normal,
        None,
    );
    Ok((buffer, size))
}
