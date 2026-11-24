//! Wallpaper loading and cover-cropping: scale to fill the surface, centre-crop the overflow.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use crate::render::Rgb;

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

pub fn load_png(path: &Path, matte: Rgb) -> Result<Image, String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = File::open(path).map_err(|e| fail(&e))?;
    let limits = png::Limits { bytes: 1 << 30 };
    let mut decoder = png::Decoder::new_with_limits(BufReader::new(file), limits);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| fail(&e))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| fail(&"image too large"))?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| fail(&e))?;
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(fail(&"unexpanded palette")),
    };
    let (w, h) = (info.width as usize, info.height as usize);
    let mut rgb = Vec::with_capacity(w * h * 3);
    for row in buf.chunks(info.line_size).take(h) {
        for px in row[..w * channels].chunks_exact(channels) {
            let (color, alpha) = match channels {
                1 => ([px[0]; 3], 255),
                2 => ([px[0]; 3], px[1]),
                3 => ([px[0], px[1], px[2]], 255),
                _ => ([px[0], px[1], px[2]], px[3]),
            };
            let a = alpha as f32 / 255.0;
            let m = [matte.0, matte.1, matte.2];
            for i in 0..3 {
                rgb.push((color[i] as f32 * a + m[i] as f32 * (1.0 - a)).round() as u8);
            }
        }
    }
    Ok(Image {
        width: info.width,
        height: info.height,
        rgb,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn cover_crop(src: (u32, u32), dst: (u32, u32)) -> Crop {
    let (sw, sh) = (src.0 as f64, src.1 as f64);
    let (dw, dh) = (dst.0 as f64, dst.1 as f64);
    let scale = (dw / sw).max(dh / sh);
    let w = (dw / scale).min(sw);
    let h = (dh / scale).min(sh);
    Crop {
        x: (sw - w) / 2.0,
        y: (sh - h) / 2.0,
        w,
        h,
    }
}

fn taps(src_len: u32, start: f64, span: f64, dst_len: u32) -> Vec<Vec<(u32, f32)>> {
    let ratio = span / dst_len as f64;
    let radius = ratio.max(1.0);
    let last = src_len as i64 - 1;
    (0..dst_len)
        .map(|i| {
            let center = start + (i as f64 + 0.5) * ratio - 0.5;
            let lo = (center - radius).floor() as i64 + 1;
            let hi = (center + radius).ceil() as i64 - 1;
            let mut out: Vec<(u32, f32)> = Vec::new();
            let mut total = 0.0;
            for j in lo..=hi {
                let w = 1.0 - (j as f64 - center).abs() / radius;
                if w <= 0.0 {
                    continue;
                }
                let idx = j.clamp(0, last) as u32;
                match out.last_mut() {
                    Some((prev, pw)) if *prev == idx => *pw += w as f32,
                    _ => out.push((idx, w as f32)),
                }
                total += w;
            }
            for t in &mut out {
                t.1 /= total as f32;
            }
            out
        })
        .collect()
}

fn filter_row(image: &Image, row: u32, xtaps: &[Vec<(u32, f32)>]) -> Vec<f32> {
    let line = &image.rgb[row as usize * image.width as usize * 3..][..image.width as usize * 3];
    let mut out = Vec::with_capacity(xtaps.len() * 3);
    for taps in xtaps {
        let mut acc = [0f32; 3];
        for &(x, w) in taps {
            let px = &line[x as usize * 3..][..3];
            for c in 0..3 {
                acc[c] += px[c] as f32 * w;
            }
        }
        out.extend_from_slice(&acc);
    }
    out
}

/// Returns `width * height` pixels as XRGB8888 little-endian bytes.
pub fn cover(image: &Image, width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0u8; width as usize * height as usize * 4];
    if width == 0 || height == 0 || image.width == 0 || image.height == 0 {
        return out;
    }
    let crop = cover_crop((image.width, image.height), (width, height));
    let xtaps = taps(image.width, crop.x, crop.w, width);
    let ytaps = taps(image.height, crop.y, crop.h, height);
    let mut rows: Vec<(u32, Vec<f32>)> = Vec::new();
    let mut acc = vec![0f32; width as usize * 3];
    for (y, taps) in ytaps.iter().enumerate() {
        let first = taps[0].0;
        rows.retain(|(r, _)| *r >= first);
        acc.fill(0.0);
        for &(r, wy) in taps {
            let idx = match rows.iter().position(|(k, _)| *k == r) {
                Some(i) => i,
                None => {
                    rows.push((r, filter_row(image, r, &xtaps)));
                    rows.len() - 1
                }
            };
            for (a, v) in acc.iter_mut().zip(&rows[idx].1) {
                *a += wy * v;
            }
        }
        let line = &mut out[y * width as usize * 4..][..width as usize * 4];
        for (px, c) in line.chunks_exact_mut(4).zip(acc.chunks_exact(3)) {
            let q = |v: f32| v.round().clamp(0.0, 255.0) as u8;
            px.copy_from_slice(&[q(c[2]), q(c[1]), q(c[0]), 0xff]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Image {
        let mut rgb = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgb.extend_from_slice(&f(x, y));
            }
        }
        Image { width, height, rgb }
    }

    fn pixel(buf: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
        let p = &buf[((y * width + x) * 4) as usize..][..4];
        [p[2], p[1], p[0]]
    }

    #[test]
    fn crop_same_size_is_identity() {
        let c = cover_crop((1920, 1080), (1920, 1080));
        assert_eq!(
            c,
            Crop {
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0
            }
        );
    }

    #[test]
    fn crop_wide_image_on_square_output_trims_the_sides() {
        let c = cover_crop((1920, 1080), (1080, 1080));
        assert_eq!(
            c,
            Crop {
                x: 420.0,
                y: 0.0,
                w: 1080.0,
                h: 1080.0
            }
        );
    }

    #[test]
    fn crop_square_image_on_wide_output_trims_top_and_bottom() {
        let c = cover_crop((1000, 1000), (2000, 1000));
        assert_eq!(
            c,
            Crop {
                x: 0.0,
                y: 250.0,
                w: 1000.0,
                h: 500.0
            }
        );
    }

    #[test]
    fn crop_small_image_on_large_output_keeps_aspect() {
        let c = cover_crop((800, 600), (3840, 2160));
        assert!((c.w / c.h - 3840.0 / 2160.0).abs() < 1e-9);
        assert_eq!(c.w, 800.0);
        assert_eq!(c.h, 450.0);
        assert_eq!(c.y, 75.0);
    }

    #[test]
    fn same_size_cover_copies_pixels() {
        let img = image(5, 3, |x, y| [x as u8 * 40, y as u8 * 80, 7]);
        let out = cover(&img, 5, 3);
        for y in 0..3 {
            for x in 0..5 {
                assert_eq!(pixel(&out, 5, x, y), [x as u8 * 40, y as u8 * 80, 7]);
            }
        }
        assert!(out.chunks(4).all(|p| p[3] == 0xff));
    }

    #[test]
    fn cover_takes_the_centre() {
        let img = image(4, 2, |x, _| if x < 2 { [255, 0, 0] } else { [0, 0, 255] });
        let out = cover(&img, 2, 2);
        assert_eq!(pixel(&out, 2, 0, 0), [255, 0, 0]);
        assert_eq!(pixel(&out, 2, 1, 1), [0, 0, 255]);
    }

    #[test]
    fn uniform_colour_survives_any_scale() {
        let img = image(7, 5, |_, _| [12, 200, 99]);
        for (w, h) in [(1, 1), (3, 9), (40, 13), (7, 5)] {
            let out = cover(&img, w, h);
            for y in 0..h {
                for x in 0..w {
                    assert_eq!(pixel(&out, w, x, y), [12, 200, 99]);
                }
            }
        }
    }

    #[test]
    fn downscale_averages() {
        let img = image(
            2,
            2,
            |x, y| if (x + y) % 2 == 0 { [0; 3] } else { [254; 3] },
        );
        let out = cover(&img, 1, 1);
        assert_eq!(pixel(&out, 1, 0, 0), [127; 3]);
    }

    #[test]
    fn taps_are_normalised() {
        for (src, start, span, dst) in [(100, 0.0, 100.0, 37), (10, 2.5, 5.0, 80), (3, 0.0, 3.0, 3)]
        {
            for t in taps(src, start, span, dst) {
                let sum: f32 = t.iter().map(|(_, w)| w).sum();
                assert!((sum - 1.0).abs() < 1e-5);
                assert!(t.iter().all(|(i, _)| *i < src));
            }
        }
    }

    fn write_png(
        name: &str,
        w: u32,
        h: u32,
        color: png::ColorType,
        data: &[u8],
    ) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("newm-rs-lock-{}-{name}.png", std::process::id()));
        let file = File::create(&path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
        enc.set_color(color);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(data).unwrap();
        path
    }

    #[test]
    fn loads_rgba_over_the_matte() {
        let path = write_png(
            "rgba",
            2,
            1,
            png::ColorType::Rgba,
            &[255, 0, 0, 255, 0, 0, 255, 0],
        );
        let img = load_png(&path, Rgb(10, 20, 30)).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.rgb, vec![255, 0, 0, 10, 20, 30]);
    }

    #[test]
    fn loads_grayscale_as_rgb() {
        let path = write_png("gray", 1, 2, png::ColorType::Grayscale, &[0, 200]);
        let img = load_png(&path, Rgb(0, 0, 0)).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(img.rgb, vec![0, 0, 0, 200, 200, 200]);
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(load_png(Path::new("/nonexistent/wall.png"), Rgb(0, 0, 0)).is_err());
    }

    #[test]
    fn empty_target_is_empty() {
        let img = image(2, 2, |_, _| [1, 2, 3]);
        assert!(cover(&img, 0, 10).is_empty());
    }
}
