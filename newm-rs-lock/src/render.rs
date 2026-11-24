//! Software drawing of the password box into XRGB8888 buffers.

use crate::field::Phase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(s: &str) -> Option<Rgb> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }

    pub fn xrgb(self) -> [u8; 4] {
        [self.2, self.1, self.0, 0xff]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    pub bg: Rgb,
    pub fg: Rgb,
    pub accent: Rgb,
    pub error: Rgb,
}

impl Default for Colors {
    fn default() -> Self {
        Colors {
            bg: Rgb(0x0b, 0x0f, 0x14),
            fg: Rgb(0xd8, 0xde, 0xe9),
            accent: Rgb(0x7e, 0xf0, 0xa0),
            error: Rgb(0xe0, 0x6c, 0x75),
        }
    }
}

const BOX_W: f64 = 300.0;
const BOX_H: f64 = 44.0;
const MARGIN: f64 = 16.0;
const BORDER: f64 = 2.0;
const LINE: f64 = 1.0;
const PAD: f64 = 14.0;
const DOT: f64 = 10.0;
const GAP: f64 = 8.0;
const CURSOR_W: f64 = 2.0;
const CURSOR_H: f64 = 20.0;
const FILL_ALPHA: f32 = 0.85;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub border: i32,
    pub line: i32,
    pub pad: i32,
    pub dot: i32,
    pub gap: i32,
    pub cursor_w: i32,
    pub cursor_h: i32,
}

impl Layout {
    pub fn new(width: u32, height: u32, scale: f64) -> Layout {
        let px = |v: f64| ((v * scale).round() as i32).max(1);
        let h = px(BOX_H);
        let w = px(BOX_W).min(width as i32 - 2 * px(MARGIN)).max(h);
        Layout {
            x: (width as i32 - w) / 2,
            y: (height as i32 - h) / 2,
            w,
            h,
            border: px(BORDER),
            line: px(LINE),
            pad: px(PAD),
            dot: px(DOT),
            gap: px(GAP),
            cursor_w: px(CURSOR_W),
            cursor_h: px(CURSOR_H),
        }
    }

    pub fn max_dots(&self) -> usize {
        let room = self.w - 2 * (self.border + self.pad) - self.cursor_w;
        (room.max(0) / (self.dot + self.gap)) as usize
    }

    fn left(&self) -> i32 {
        self.x + self.border + self.pad
    }

    pub fn dots(&self, count: usize) -> Vec<(i32, i32)> {
        let y = self.y + (self.h - self.dot) / 2;
        (0..count.min(self.max_dots()))
            .map(|i| (self.left() + i as i32 * (self.dot + self.gap), y))
            .collect()
    }

    pub fn cursor(&self, count: usize) -> (i32, i32, i32, i32) {
        let n = count.min(self.max_dots()) as i32;
        let x = self.left() + n * (self.dot + self.gap);
        (
            x,
            self.y + (self.h - self.cursor_h) / 2,
            self.cursor_w,
            self.cursor_h,
        )
    }
}

pub struct Canvas<'a> {
    pub data: &'a mut [u8],
    pub width: u32,
    pub height: u32,
}

impl Canvas<'_> {
    fn blend(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || a <= 0.0 {
            return;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let px = &mut self.data[i..i + 4];
        if a >= 1.0 {
            px.copy_from_slice(&c.xrgb());
            return;
        }
        let under = Rgb(px[2], px[1], px[0]);
        px.copy_from_slice(&under.mix(c, a).xrgb());
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb, a: f32) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.width as i32);
        let y1 = (y + h).min(self.height as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                self.blend(px, py, c, a);
            }
        }
    }

    pub fn fill_circle(&mut self, cx: f64, cy: f64, r: f64, c: Rgb) {
        let y0 = (cy - r).floor() as i32;
        let y1 = (cy + r).ceil() as i32;
        let x0 = (cx - r).floor() as i32;
        let x1 = (cx + r).ceil() as i32;
        for py in y0..y1 {
            for px in x0..x1 {
                let d = ((px as f64 + 0.5 - cx).powi(2) + (py as f64 + 0.5 - cy).powi(2)).sqrt();
                self.blend(px, py, c, (r - d + 0.5).clamp(0.0, 1.0) as f32);
            }
        }
    }
}

/// The frame stays quiet over the lock picture: a thin line, grey while empty, a dim accent while
/// typing and dimmer still while checking. Only a wrong password draws it loud and thick.
pub fn frame(colors: &Colors, phase: Phase, dots: usize) -> (Rgb, bool) {
    match phase {
        Phase::Idle if dots == 0 => (colors.fg.mix(colors.bg, 0.75), false),
        Phase::Idle => (colors.accent.mix(colors.bg, 0.6), false),
        Phase::Checking => (colors.accent.mix(colors.bg, 0.8), false),
        Phase::Failed => (colors.error, true),
    }
}

pub fn cursor_color(colors: &Colors, phase: Phase) -> Rgb {
    match phase {
        Phase::Idle => colors.accent,
        Phase::Checking => colors.accent.mix(colors.bg, 0.6),
        Phase::Failed => colors.error,
    }
}

pub fn draw_field(
    canvas: &mut Canvas,
    layout: &Layout,
    colors: &Colors,
    phase: Phase,
    dots: usize,
) {
    let l = layout;
    let (frame, loud) = frame(colors, phase, dots);
    let b = if loud { l.border } else { l.line };
    canvas.fill_rect(l.x, l.y, l.w, l.h, colors.bg, FILL_ALPHA);
    canvas.fill_rect(l.x, l.y, l.w, b, frame, 1.0);
    canvas.fill_rect(l.x, l.y + l.h - b, l.w, b, frame, 1.0);
    canvas.fill_rect(l.x, l.y, b, l.h, frame, 1.0);
    canvas.fill_rect(l.x + l.w - b, l.y, b, l.h, frame, 1.0);

    let dot_color = match phase {
        Phase::Checking => colors.fg.mix(colors.bg, 0.5),
        _ => colors.fg,
    };
    let r = l.dot as f64 / 2.0;
    for (x, y) in l.dots(dots) {
        canvas.fill_circle(x as f64 + r, y as f64 + r, r, dot_color);
    }
    let (x, y, w, h) = l.cursor(dots);
    canvas.fill_rect(x, y, w, h, cursor_color(colors, phase), 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colours() {
        assert_eq!(Rgb::parse("#7ef0a0"), Some(Rgb(0x7e, 0xf0, 0xa0)));
        assert_eq!(Rgb::parse("0B0F14"), Some(Rgb(0x0b, 0x0f, 0x14)));
        assert_eq!(Rgb::parse("#fff"), None);
        assert_eq!(Rgb::parse("#gg0000"), None);
        assert_eq!(Rgb::parse("#+12345"), None);
    }

    #[test]
    fn layout_is_centred_and_scaled() {
        let l = Layout::new(1920, 1080, 1.0);
        assert_eq!((l.w, l.h), (300, 44));
        assert_eq!((l.x, l.y), (810, 518));

        let l2 = Layout::new(3840, 2160, 2.0);
        assert_eq!((l2.w, l2.h, l2.dot, l2.gap), (600, 88, 20, 16));
        assert_eq!((l2.x, l2.y), (1620, 1036));
        assert_eq!(l2.max_dots(), l.max_dots());
    }

    #[test]
    fn fractional_scale_rounds_each_size() {
        let l = Layout::new(2560, 1440, 1.25);
        assert_eq!(
            (l.w, l.h, l.border, l.dot, l.gap, l.pad),
            (375, 55, 3, 13, 10, 18)
        );
    }

    #[test]
    fn dots_step_left_to_right_inside_the_box() {
        let l = Layout::new(1920, 1080, 1.0);
        let dots = l.dots(3);
        assert_eq!(dots, vec![(826, 535), (844, 535), (862, 535)]);
        assert_eq!(l.cursor(3), (880, 530, 2, 20));
        assert_eq!(l.cursor(0), (826, 530, 2, 20));
    }

    #[test]
    fn dots_are_capped_at_the_box_width() {
        let l = Layout::new(1920, 1080, 1.0);
        assert_eq!(l.max_dots(), 14);
        assert_eq!(l.dots(100).len(), 14);
        let (cx, _, cw, _) = l.cursor(100);
        assert!(cx + cw <= l.x + l.w - l.border - l.pad);
        let (last_x, _) = *l.dots(100).last().unwrap();
        assert!(last_x + l.dot <= l.x + l.w - l.border - l.pad);
    }

    #[test]
    fn narrow_outputs_shrink_the_box() {
        let l = Layout::new(200, 400, 1.0);
        assert_eq!(l.w, 168);
        assert_eq!(l.x, 16);
        assert_eq!(l.max_dots(), 7);
    }

    fn render(phase: Phase, dots: usize) -> (Vec<u8>, Layout, Colors) {
        let (w, h) = (400, 200);
        let colors = Colors::default();
        let mut data = vec![0; w * h * 4];
        for px in data.chunks_exact_mut(4) {
            px.copy_from_slice(&colors.bg.xrgb());
        }
        let layout = Layout::new(w as u32, h as u32, 1.0);
        let mut canvas = Canvas {
            data: &mut data,
            width: w as u32,
            height: h as u32,
        };
        draw_field(&mut canvas, &layout, &colors, phase, dots);
        (data, layout, colors)
    }

    fn at(data: &[u8], x: i32, y: i32) -> Rgb {
        let i = (y as usize * 400 + x as usize) * 4;
        Rgb(data[i + 2], data[i + 1], data[i])
    }

    #[test]
    fn frame_follows_the_phase_and_only_a_wrong_password_is_loud() {
        let c = Colors::default();
        let mut seen = Vec::new();
        for (phase, dots, want, width) in [
            (Phase::Idle, 0, c.fg.mix(c.bg, 0.75), 1),
            (Phase::Idle, 3, c.accent.mix(c.bg, 0.6), 1),
            (Phase::Checking, 3, c.accent.mix(c.bg, 0.8), 1),
            (Phase::Failed, 0, c.error, 2),
        ] {
            let (data, l, _) = render(phase, dots);
            assert_eq!(at(&data, l.x, l.y), want);
            assert_eq!(at(&data, l.x + l.w - 1, l.y + l.h - 1), want);
            assert_eq!(at(&data, l.x + width - 1, l.y + l.h / 2), want);
            assert_ne!(at(&data, l.x + width, l.y + l.h / 2), want);
            assert!(!seen.contains(&want));
            seen.push(want);
        }
    }

    #[test]
    fn the_cursor_keeps_the_accent_while_idle() {
        let c = Colors::default();
        for (phase, want) in [
            (Phase::Idle, c.accent),
            (Phase::Checking, c.accent.mix(c.bg, 0.6)),
            (Phase::Failed, c.error),
        ] {
            let (data, l, _) = render(phase, 2);
            let (x, y, _, h) = l.cursor(2);
            assert_eq!(at(&data, x, y + h / 2), want);
        }
    }

    #[test]
    fn dots_are_drawn_in_the_foreground_colour() {
        let (data, l, colors) = render(Phase::Idle, 2);
        for (x, y) in l.dots(2) {
            assert_eq!(at(&data, x + l.dot / 2, y + l.dot / 2), colors.fg);
        }
        let (x, _) = l.dots(3)[2];
        assert_ne!(at(&data, x + l.dot / 2, l.y + l.h / 2), colors.fg);
    }

    #[test]
    fn nothing_outside_the_box_is_touched() {
        let (data, l, colors) = render(Phase::Failed, 30);
        assert_eq!(at(&data, l.x - 1, l.y), colors.bg);
        assert_eq!(at(&data, l.x, l.y - 1), colors.bg);
        assert_eq!(at(&data, l.x + l.w, l.y + l.h), colors.bg);
    }
}
