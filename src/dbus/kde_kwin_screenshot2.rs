//! KWin's screenshot interface, which is the only way Spectacle captures on Wayland.
//!
//! A capture is replied to with its metadata, and the pixels follow through the pipe the caller
//! passed, as `QImage::Format_RGBA8888_Premultiplied` rows without padding.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::thread;

use zbus::interface;
use zbus::zvariant::{self, OwnedValue, Value};

use super::{request_name, Start};

/// `QImage::Format_RGBA8888_Premultiplied`.
const QIMAGE_FORMAT_RGBA8888_PREMULTIPLIED: u32 = 18;

pub struct ScreenShot2 {
    to_niri: calloop::channel::Sender<Request>,
}

pub struct Request {
    pub target: Target,
    pub include_cursor: bool,
    pub reply: async_channel::Sender<Result<Capture, CaptureError>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// An output by its connector name.
    Screen(String),
    ActiveScreen,
    ScreenUnderCursor,
    ActiveWindow,
    WindowUnderCursor,
    /// A window by its niri id.
    Window(u64),
    /// A rectangle in global logical coordinates.
    Area {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    /// All outputs.
    Workspace,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub screen: Option<String>,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureError {
    Cancelled,
    InvalidScreen,
    InvalidWindow,
    InvalidArea,
    Failed,
}

/// One output's pixels at its place in the global logical space.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.kde.KWin.ScreenShot2.Error")]
pub enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    Cancelled(String),
    InvalidScreen(String),
    InvalidWindow(String),
    InvalidArea(String),
    Failed(String),
}

impl From<CaptureError> for Error {
    fn from(err: CaptureError) -> Self {
        match err {
            CaptureError::Cancelled => Self::Cancelled("nothing to capture".to_owned()),
            CaptureError::InvalidScreen => Self::InvalidScreen("no such screen".to_owned()),
            CaptureError::InvalidWindow => Self::InvalidWindow("no such window".to_owned()),
            CaptureError::InvalidArea => Self::InvalidArea("the area is off screen".to_owned()),
            CaptureError::Failed => Self::Failed("internal error".to_owned()),
        }
    }
}

type Options = HashMap<String, OwnedValue>;
type Results = HashMap<String, OwnedValue>;

#[interface(name = "org.kde.KWin.ScreenShot2")]
impl ScreenShot2 {
    #[zbus(property)]
    fn version(&self) -> u32 {
        4
    }

    async fn capture_screen(
        &self,
        name: String,
        options: Options,
        pipe: zvariant::OwnedFd,
    ) -> Result<Results, Error> {
        self.capture(Target::Screen(name), options, pipe).await
    }

    async fn capture_active_screen(
        &self,
        options: Options,
        pipe: zvariant::OwnedFd,
    ) -> Result<Results, Error> {
        self.capture(Target::ActiveScreen, options, pipe).await
    }

    async fn capture_active_window(
        &self,
        options: Options,
        pipe: zvariant::OwnedFd,
    ) -> Result<Results, Error> {
        self.capture(Target::ActiveWindow, options, pipe).await
    }

    async fn capture_window(
        &self,
        handle: String,
        options: Options,
        pipe: zvariant::OwnedFd,
    ) -> Result<Results, Error> {
        let id = handle
            .parse()
            .map_err(|_| Error::from(CaptureError::InvalidWindow))?;
        self.capture(Target::Window(id), options, pipe).await
    }

    /// Captures what is under the pointer: there is no picking step, and `kind` is 0 for a window
    /// and 1 for a screen.
    async fn capture_interactive(
        &self,
        kind: u32,
        options: Options,
        pipe: zvariant::OwnedFd,
    ) -> Result<Results, Error> {
        let target = match kind {
            0 => Target::WindowUnderCursor,
            _ => Target::ScreenUnderCursor,
        };
        self.capture(target, options, pipe).await
    }

    async fn capture_area(
        &self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        options: Options,
        pipe: zvariant::OwnedFd,
    ) -> Result<Results, Error> {
        let target = Target::Area {
            x,
            y,
            width,
            height,
        };
        self.capture(target, options, pipe).await
    }

    async fn capture_workspace(
        &self,
        options: Options,
        pipe: zvariant::OwnedFd,
    ) -> Result<Results, Error> {
        self.capture(Target::Workspace, options, pipe).await
    }
}

impl ScreenShot2 {
    pub fn new(to_niri: calloop::channel::Sender<Request>) -> Self {
        Self { to_niri }
    }

    async fn capture(
        &self,
        target: Target,
        options: Options,
        pipe: zvariant::OwnedFd,
    ) -> Result<Results, Error> {
        let include_cursor = options
            .get("include-cursor")
            .is_some_and(|value| matches!(value.downcast_ref::<bool>(), Ok(true)));

        let (reply, from_niri) = async_channel::bounded(1);
        let request = Request {
            target,
            include_cursor,
            reply,
        };
        if let Err(err) = self.to_niri.send(request) {
            warn!("error sending message to niri: {err:?}");
            return Err(CaptureError::Failed.into());
        }

        let capture = match from_niri.recv().await {
            Ok(res) => res?,
            Err(err) => {
                warn!("error receiving message from niri: {err:?}");
                return Err(CaptureError::Failed.into());
            }
        };

        let mut results = HashMap::new();
        let mut insert = |key: &str, value: Value<'_>| {
            results.insert(key.to_owned(), value.try_to_owned().unwrap());
        };
        insert("type", Value::from("raw"));
        insert("width", Value::from(capture.width));
        insert("height", Value::from(capture.height));
        insert("stride", Value::from(capture.width * 4));
        insert("format", Value::from(QIMAGE_FORMAT_RGBA8888_PREMULTIPLIED));
        insert("scale", Value::from(capture.scale));
        if let Some(screen) = &capture.screen {
            insert("screen", Value::from(screen.as_str()));
        }

        // The caller reads the pipe only after this reply, and a screen does not fit in the pipe's
        // buffer, so the pixels are written from a thread of their own.
        let mut pipe = File::from(std::os::fd::OwnedFd::from(pipe));
        let pixels = capture.pixels;
        let res = thread::Builder::new()
            .name("KWin Screenshot Writer".to_owned())
            .spawn(move || {
                if let Err(err) = pipe.write_all(&pixels) {
                    warn!("error writing screenshot to the pipe: {err:?}");
                }
            });
        if let Err(err) = res {
            warn!("error spawning the screenshot writer thread: {err:?}");
            return Err(CaptureError::Failed.into());
        }

        Ok(results)
    }
}

/// Joins outputs into one image at the largest of their scales.
///
/// Returns the image with the logical position of its top left corner.
pub fn composite(mut parts: Vec<Part>) -> Option<(i32, i32, Capture)> {
    if parts.len() == 1 {
        let part = parts.pop()?;
        let capture = Capture {
            width: part.width,
            height: part.height,
            scale: part.scale,
            screen: None,
            pixels: part.pixels,
        };
        return Some((part.x, part.y, capture));
    }

    let scale = parts.iter().map(|part| part.scale).reduce(f64::max)?;
    let min_x = parts.iter().map(|part| part.x).min()?;
    let min_y = parts.iter().map(|part| part.y).min()?;

    // Where each part lands in the image: left, top, width, height.
    let rects: Vec<[usize; 4]> = parts
        .iter()
        .map(|part| {
            let px = |logical: f64| (logical * scale).round() as usize;
            [
                px(f64::from(part.x - min_x)),
                px(f64::from(part.y - min_y)),
                px(f64::from(part.width) / part.scale),
                px(f64::from(part.height) / part.scale),
            ]
        })
        .collect();
    let width = rects.iter().map(|[x, _, w, _]| x + w).max()?;
    let height = rects.iter().map(|[_, y, _, h]| y + h).max()?;

    let mut pixels = vec![0; width * height * 4];
    for (part, [left, top, w, h]) in parts.iter().zip(rects) {
        if part.width == 0 || part.height == 0 {
            continue;
        }
        let (src_w, src_h) = (part.width as usize, part.height as usize);
        for row in 0..h {
            let src_row = (row * src_h / h).min(src_h - 1);
            let src = &part.pixels[src_row * src_w * 4..][..src_w * 4];
            let dst = &mut pixels[((top + row) * width + left) * 4..][..w * 4];
            if w == src_w {
                dst.copy_from_slice(src);
                continue;
            }
            for (col, px) in dst.chunks_exact_mut(4).enumerate() {
                let src_col = (col * src_w / w).min(src_w - 1);
                px.copy_from_slice(&src[src_col * 4..][..4]);
            }
        }
    }

    let capture = Capture {
        width: width as u32,
        height: height as u32,
        scale,
        screen: None,
        pixels,
    };
    Some((min_x, min_y, capture))
}

/// Cuts a logical rectangle, relative to the image's top left corner, out of an image.
///
/// Returns `None` when the rectangle misses the image.
pub fn crop(capture: Capture, x: i64, y: i64, width: u32, height: u32) -> Option<Capture> {
    let px = |logical: i64, limit: u32| {
        let px = (logical as f64 * capture.scale).round();
        px.clamp(0., f64::from(limit)) as usize
    };
    let left = px(x, capture.width);
    let right = px(x + i64::from(width), capture.width);
    let top = px(y, capture.height);
    let bottom = px(y + i64::from(height), capture.height);
    if left >= right || top >= bottom {
        return None;
    }

    let stride = capture.width as usize * 4;
    let mut pixels = Vec::with_capacity((right - left) * (bottom - top) * 4);
    for row in top..bottom {
        let row = &capture.pixels[row * stride..][..stride];
        pixels.extend_from_slice(&row[left * 4..right * 4]);
    }

    Some(Capture {
        width: (right - left) as u32,
        height: (bottom - top) as u32,
        scale: capture.scale,
        screen: None,
        pixels,
    })
}

impl Start for ScreenShot2 {
    fn start(self, monitor: bool) -> anyhow::Result<zbus::blocking::Connection> {
        let conn = zbus::blocking::Connection::session()?;
        conn.object_server().at("/org/kde/KWin/ScreenShot2", self)?;
        request_name(&conn, "org.kde.KWin.ScreenShot2", monitor)?;

        Ok(conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(x: i32, y: i32, scale: f64, width: u32, height: u32, fill: u8) -> Part {
        Part {
            x,
            y,
            scale,
            width,
            height,
            pixels: vec![fill; (width * height * 4) as usize],
        }
    }

    fn pixel(capture: &Capture, x: u32, y: u32) -> u8 {
        capture.pixels[((y * capture.width + x) * 4) as usize]
    }

    #[test]
    fn one_output_is_returned_as_is() {
        let (x, y, capture) = composite(vec![part(10, 20, 1.5, 3, 2, 7)]).unwrap();
        assert_eq!((x, y), (10, 20));
        assert_eq!((capture.width, capture.height, capture.scale), (3, 2, 1.5));
        assert_eq!(capture.pixels, vec![7; 24]);
    }

    #[test]
    fn outputs_side_by_side_keep_their_places() {
        let parts = vec![part(0, 0, 1., 4, 2, 1), part(4, 0, 1., 2, 3, 2)];
        let (x, y, capture) = composite(parts).unwrap();
        assert_eq!((x, y), (0, 0));
        assert_eq!((capture.width, capture.height), (6, 3));
        assert_eq!(pixel(&capture, 3, 1), 1);
        assert_eq!(pixel(&capture, 4, 2), 2);
        // Below the shorter output nothing is drawn.
        assert_eq!(pixel(&capture, 0, 2), 0);
    }

    #[test]
    fn a_lower_scale_output_is_stretched_to_the_largest_scale() {
        // 2x2 logical at scale 2 to the left of 2x2 logical at scale 1.
        let parts = vec![part(-2, 0, 2., 4, 4, 1), part(0, 0, 1., 2, 2, 2)];
        let (x, y, capture) = composite(parts).unwrap();
        assert_eq!((x, y), (-2, 0));
        assert_eq!((capture.width, capture.height, capture.scale), (8, 4, 2.));
        assert_eq!(pixel(&capture, 3, 3), 1);
        assert_eq!(pixel(&capture, 4, 0), 2);
        assert_eq!(pixel(&capture, 7, 3), 2);
    }

    #[test]
    fn crop_cuts_a_logical_rectangle_in_pixels() {
        let (_, _, capture) = composite(vec![part(0, 0, 2., 8, 8, 0)]).unwrap();
        let mut capture = capture;
        capture.pixels[(2 * 8 + 4) * 4] = 9;
        let cut = crop(capture, 2, 1, 2, 2).unwrap();
        assert_eq!((cut.width, cut.height, cut.scale), (4, 4, 2.));
        assert_eq!(pixel(&cut, 0, 0), 9);
    }

    #[test]
    fn crop_takes_any_area_a_caller_names() {
        let (_, _, capture) = composite(vec![part(100, 0, 1., 4, 4, 3)]).unwrap();
        let x = i64::from(i32::MIN) - 100;
        assert_eq!(
            crop(capture.clone(), x, 0, u32::MAX, 2).map(|c| c.width),
            Some(4)
        );
        assert_eq!(
            crop(capture, i64::from(i32::MAX) - 100, 0, u32::MAX, 2),
            None
        );
    }

    #[test]
    fn crop_clips_to_the_image_and_refuses_a_miss() {
        let (_, _, capture) = composite(vec![part(0, 0, 1., 4, 4, 3)]).unwrap();
        let cut = crop(capture.clone(), -2, 2, 4, 10).unwrap();
        assert_eq!((cut.width, cut.height), (2, 2));
        assert_eq!(crop(capture.clone(), 4, 0, 2, 2), None);
        assert_eq!(crop(capture, 0, 0, 0, 2), None);
    }
}
