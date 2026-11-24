//! The shader's per-pixel maths on the CPU, in f32, line for line: the reference for a still
//! (the lock screen) and for compositors that draw without a shader.
//!
//! What depends only on the row (the sky's colour, the star row, the sun's vertical offset) is
//! worked out once per row, and what depends only on the column (each ridge's height) once per
//! column; [`shade`] goes through the same code for one pixel, so [`Painter`] draws exactly the
//! pixels `shade` gives.

use std::ops::Range;

use crate::frame::Frame;
use crate::look::{Tables, MAX_LAYERS};

/// A pixel: `u32::to_le_bytes` gives `[r, g, b, 0xff]`, RGBA8 in memory (DRM `ABGR8888`).
pub const fn pack(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16 | 0xff00_0000
}

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// GLSL `mix`: `x·(1−a) + y·a`.
fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1. - a) + y * a
}

fn mix3(x: V3, y: V3, a: f32) -> V3 {
    [mix(x[0], y[0], a), mix(x[1], y[1], a), mix(x[2], y[2], a)]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// `floor` without a libm call (baseline x86-64 has no rounding instruction): exact for every
/// value this code floors, all well inside ±2³¹.
fn floor(x: f32) -> f32 {
    let i = x as i32 as f32;
    if i > x {
        i - 1.
    } else {
        i
    }
}

fn fract(x: f32) -> f32 {
    x - floor(x)
}

/// `(v · 255).round()` for `v` in 0..1, without a libm call: rounds half away from zero, as
/// `f32::round` does.
fn to_byte(v: f32) -> u8 {
    let s = v * 255.;
    let i = s as i32;
    let up = s - i as f32 >= 0.5;
    (i + i32::from(up)).clamp(0, 255) as u8
}

fn bayer2(a: [f32; 2]) -> f32 {
    let (x, y) = (floor(a[0]), floor(a[1]));
    fract(x / 2. + y * y * 0.75)
}

fn bayer4(a: [f32; 2]) -> f32 {
    bayer2([0.5 * a[0], 0.5 * a[1]]) * 0.25 + bayer2(a)
}

fn vnoise(t: &Tables, x: f32) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let f = f * f * (3. - 2. * f);
    mix(t.ridge(i as i64), t.ridge(i as i64 + 1), f)
}

fn ridge(t: &Tables, fr: &Frame, mut xs: f32, l: &[f32; 16]) -> f32 {
    let w = [
        fr.octaves[0],
        fr.octaves[1],
        fr.octaves[2],
        fr.octaves[3],
        fr.octave4,
    ];
    let offsets = [l[1], l[2], l[3], l[4], l[5]];
    let mut s = 0.;
    for (k, (w, o)) in w.into_iter().zip(offsets).enumerate() {
        if k > 0 {
            xs *= crate::frame::LACUNARITY;
        }
        s += w * vnoise(t, xs + o);
    }
    s
}

/// What a row shares.
struct Row {
    frag_y: f32,
    py: f32,
    sky: V3,
    star_y: f32,
    cell_y: i64,
    star_fade: f32,
    sun_dy: f32,
}

impl Row {
    fn new(fr: &Frame, y: usize) -> Self {
        let hgt = fr.res[1];
        let frag_y = hgt - y as f32 - 0.5;
        let py = (frag_y - 0.5 * hgt) / hgt;
        let up = (py - fr.scene[0]).max(0.);
        let sg = py * 70.;
        Row {
            frag_y,
            py,
            sky: mix3(fr.glow, fr.sky, 1. - (-up * fr.scene[1]).exp()),
            star_y: fract(sg),
            cell_y: floor(sg) as i64,
            star_fade: smoothstep(0., 0.2, up),
            sun_dy: py - fr.sun[1],
        }
    }
}

/// What a column shares.
#[derive(Clone, Copy, Default)]
struct Col {
    frag_x: f32,
    px: f32,
    curve: f32,
    star_x: f32,
    cell_x: i64,
    sun_dx: f32,
    /// Each ridge layer's top.
    tops: [f32; MAX_LAYERS],
}

impl Col {
    fn new(t: &Tables, fr: &Frame, x: usize) -> Self {
        let [w, hgt] = fr.res;
        let frag_x = x as f32 + 0.5;
        let px = (frag_x - 0.5 * w) / hgt;
        let sg = px * 70.;
        let mut tops = [0.; MAX_LAYERS];
        for (top, l) in tops.iter_mut().zip(&fr.layer).take(fr.layers as usize) {
            let xs = px * l[0];
            let mut lift = 1.;
            if fr.stripes[3] != 0. {
                lift += fr.stripes[3] * (0.5 + 0.5 * (3. * xs + l[11]).sin());
            }
            *top = fr.scene[0] + l[6] * ridge(t, fr, xs, l) * lift;
        }
        Col {
            frag_x,
            px,
            curve: fr.floor[0] * px * px,
            star_x: fract(sg),
            cell_x: floor(sg) as i64,
            sun_dx: px - fr.sun[0],
            tops,
        }
    }
}

fn pixel(t: &Tables, fr: &Frame, row: &Row, c: &Col) -> u32 {
    let px = 1. / fr.res[1];
    let h = fr.scene[0];
    let sc = [fr.sun[0], fr.sun[1]];
    let sr = fr.sun[2];
    let p = [c.px, row.py];

    let mut col: V3;
    let yy = p[1] - c.curve;
    let dy = h - yy;
    if dy > 0. {
        let [_, persp, gx, gz] = fr.floor;
        let lw = fr.lines[0];
        let z = persp / dy;
        let xw = p[0] * z;
        let pz = z * px;
        let distx = (0.5 - (fract(xw * gx) - 0.5).abs()) / gx;
        let mut lx = 1. - smoothstep(lw * pz * 0.5, lw * pz * 0.5 + pz, distx);
        let pzz = z * z * px / persp;
        let distz = (0.5 - (fract(z * gz + fr.phase[3]) - 0.5).abs()) / gz;
        let mut lz = 1. - smoothstep(lw * pzz * 0.5, lw * pzz * 0.5 + pzz, distz);
        lz *= (1. / (pzz * gz * 3.)).clamp(0., 1.);
        lx *= (1. / (pz * gx * 3.)).clamp(0., 1.);
        let glow = if fr.lines[1] != 0. {
            fr.lines[1] * 0.35 * ((-distx / (pz * 6.)).exp() + (-distz / (pzz * 6.)).exp())
        } else {
            0.
        };
        let fade = (-z * 0.12).exp();
        let mut ground = mix3(
            scale(fr.sky, 0.7),
            scale(fr.glow, 0.5),
            (-dy * fr.scene[1] * 2.).exp(),
        );
        ground = add(
            ground,
            scale(fr.grid, (lx.max(lz) + glow) * fade * fr.lines[2]),
        );
        if fr.stripes[2] != 0. {
            let pm = [p[0] - sc[0], 2. * h - yy - sc[1]];
            let q = pm[1] * 0.55;
            let len = (pm[0] * pm[0] + q * q).sqrt();
            let refl = smoothstep(sr * 1.4, sr * 0.2, len);
            if refl > 0. {
                let shimmer = 0.6 + 0.4 * (z * 40. + fr.phase[1]).sin();
                ground = add(
                    ground,
                    scale(
                        mix3(fr.sun_bottom, fr.sun_top, 0.3),
                        refl * fr.stripes[2] * 0.5 * shimmer,
                    ),
                );
            }
        }
        col = ground;
    } else {
        col = row.sky;

        if fr.scene[2] <= 1. {
            let hs = t.star(c.cell_x, row.cell_y);
            if hs[0] >= fr.scene[2] {
                let sp = [
                    c.star_x - 0.5 - (hs[1] - 0.5) * 0.6,
                    row.star_y - 0.5 - (hs[2] - 0.5) * 0.6,
                ];
                let mut star = smoothstep(0.11, 0., (sp[0] * sp[0] + sp[1] * sp[1]).sqrt());
                star *= (0.6 + 0.4 * (fr.phase[0] * (1. + 2. * hs[0]) + hs[0] * 40.).sin())
                    * row.star_fade;
                col = add(col, scale([0.85, 0.92, 1.], star));
            }
        }

        let d = [c.sun_dx, row.sun_dy];
        let dl = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if fr.sun[3] >= 0.001 {
            let halo = (-(dl - sr).max(0.) / fr.sun[3].max(0.0005)).exp() * 0.45;
            col = add(col, scale(mix3(fr.sun_bottom, fr.glow, 0.5), halo));
        }
        let mut disc = smoothstep(sr + px, sr - px, dl);
        if disc > 0. {
            let sv = (d[1] / sr.max(0.0001) * 0.5 + 0.5).clamp(0., 1.);
            let sunc = mix3(fr.sun_bottom, fr.sun_top, sv);
            let stripes = fr.stripes[0];
            if stripes >= 0.5 {
                let sy = 1. - sv;
                let band = fract(sy * stripes - fr.phase[2]);
                let edge = fr.stripes[1] * sy;
                let aa = stripes * px / (2. * sr).max(0.0001);
                let step = if sy >= 0.42 { 1. } else { 0. };
                let cut = smoothstep(edge + aa, edge - aa, band) * step;
                disc *= 1. - cut;
            }
            col = mix3(col, sunc, disc);
        }

        if p[1] <= fr.scene[3] {
            for (l, &top) in fr.layer.iter().zip(&c.tops).take(fr.layers as usize) {
                let m = smoothstep(top + px, top - px, p[1]);
                col = mix3(col, [l[8], l[9], l[10]], m);
                col = add(
                    col,
                    scale(fr.grid, (-(p[1] - top).abs() / (2.5 * px)).exp() * l[7]),
                );
            }
        }
    }
    col = add(col, scale(fr.glow, (-(yy - h).abs() / 0.006).exp() * 0.5));

    let m = &fr.colour;
    let mut c3 = [0, 1, 2].map(|r| m[r] * col[0] + m[3 + r] * col[1] + m[6 + r] * col[2]);
    if fr.post[0] != 0. {
        // The playground's 3.14159, not π: the scanlines follow it exactly.
        #[allow(clippy::approx_constant)]
        let s = 1. - fr.post[0] * 0.5 * (0.5 + 0.5 * (row.frag_y * 3.14159).sin());
        c3 = scale(c3, s);
    }
    c3 = c3.map(|v| v.clamp(0., 1.));
    if fr.post[1] < 7.5 {
        let b = bayer4([c.frag_x, row.frag_y]);
        c3 = c3.map(|v| floor(v * fr.post[2] + b) / fr.post[2]);
    }
    let [r, g, b] = c3.map(to_byte);
    pack(r, g, b)
}

/// The colour of the pixel at column `x`, row `y` (from the top) of a frame.
pub fn shade(t: &Tables, fr: &Frame, x: usize, y: usize) -> u32 {
    pixel(t, fr, &Row::new(fr, y), &Col::new(t, fr, x))
}

/// Draws frames, keeping its per-column table between them so drawing allocates only when the
/// width grows.
#[derive(Default)]
pub struct Painter {
    cols: Vec<Col>,
}

impl Painter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Draws `rows` (from the top) of a frame `width` pixels wide into `px`, which holds just
    /// those rows, `stride` pixels apart. `frame` must be made for the whole picture's size.
    pub fn paint_rows(
        &mut self,
        t: &Tables,
        frame: &Frame,
        px: &mut [u32],
        width: usize,
        rows: Range<usize>,
        stride: usize,
    ) {
        assert!(width > 0 && stride >= width);
        let n = rows.len();
        if n == 0 {
            return;
        }
        assert!(px.len() >= stride * (n - 1) + width);
        self.cols.clear();
        self.cols.extend((0..width).map(|x| Col::new(t, frame, x)));
        for (i, y) in rows.enumerate() {
            let row = Row::new(frame, y);
            for (p, c) in px[i * stride..i * stride + width]
                .iter_mut()
                .zip(&self.cols)
            {
                *p = pixel(t, frame, &row, c);
            }
        }
    }
}

/// Draws a frame into `px`, `width × height` pixels with `stride` pixels per row, top row
/// first. `frame` must be made for this size.
pub fn render(
    t: &Tables,
    frame: &Frame,
    px: &mut [u32],
    width: usize,
    height: usize,
    stride: usize,
) {
    assert!(height > 0);
    Painter::new().paint_rows(t, frame, px, width, 0..height, stride);
}
