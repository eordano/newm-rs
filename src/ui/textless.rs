//! Plain panels drawn in place of text when niri is built without the `text` feature.

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::utils::Transform;

use crate::render_helpers::memory::MemoryBuffer;
use crate::render_helpers::texture::TextureBuffer;

/// An opaque ARGB8888 image built from solid shapes.
pub struct Panel {
    width: i32,
    height: i32,
    data: Vec<u8>,
}

impl Panel {
    pub fn new(width: i32, height: i32, color: [f64; 3]) -> Self {
        let mut panel = Self {
            width,
            height,
            data: vec![0; (width * height * 4) as usize],
        };
        panel.fill(|_, _| true, color);
        panel
    }

    /// Paints a frame of `border` pixels along the inside of the edges.
    pub fn frame(&mut self, border: i32, color: [f64; 3]) {
        let (w, h) = (self.width, self.height);
        self.fill(
            |x, y| x < border || y < border || x >= w - border || y >= h - border,
            color,
        );
    }

    pub fn disc(&mut self, center: (f64, f64), radius: f64, color: [f64; 3]) {
        self.fill(
            |x, y| {
                let dx = f64::from(x) + 0.5 - center.0;
                let dy = f64::from(y) + 0.5 - center.1;
                dx * dx + dy * dy <= radius * radius
            },
            color,
        );
    }

    fn fill(&mut self, inside: impl Fn(i32, i32) -> bool, [r, g, b]: [f64; 3]) {
        let [r, g, b] = [r, g, b].map(|c| (c * 255.).round() as u8);
        let width = self.width;
        for (i, pixel) in self.data.chunks_exact_mut(4).enumerate() {
            let i = i as i32;
            if inside(i % width, i / width) {
                pixel.copy_from_slice(&[b, g, r, 255]);
            }
        }
    }

    pub fn into_buffer(self, scale: f64) -> MemoryBuffer {
        MemoryBuffer::new(
            self.data,
            Fourcc::Argb8888,
            (self.width, self.height),
            scale,
            Transform::Normal,
        )
    }

    pub fn into_texture(
        self,
        renderer: &mut GlesRenderer,
        scale: f64,
    ) -> anyhow::Result<TextureBuffer<GlesTexture>> {
        Ok(TextureBuffer::from_memory_buffer(
            renderer,
            &self.into_buffer(scale),
        )?)
    }
}
