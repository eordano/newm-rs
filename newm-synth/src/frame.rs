//! A frame's uniforms: what the shader and the CPU port need beyond the pixel's position,
//! worked out once per frame in f64 so that hours of running time lose no precision.

use std::f64::consts::TAU;

use crate::look::{Look, MAX_LAYERS, RIDGE_TABLE};

/// The lacunarity of the ridge noise: each octave's input is the previous one's × 2.03 + 11.7.
pub(crate) const LACUNARITY: f32 = 2.03;
const OCTAVE_SHIFT: f64 = 11.7;
pub(crate) const OCTAVES: usize = 5;

/// Every uniform of [`crate::FRAGMENT_SHADER`] for one frame, named as the shader names them
/// (`synth_<field>`). Colours are sRGB 0 to 1; `p` is the screen point in screen heights from the
/// centre, y up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The picture's width and height in pixels.
    pub res: [f32; 2],
    /// The time terms, each wrapped to its own period so that it is exact at any running time:
    /// the twinkle clock (mod 2π·128 s), the shimmer clock (mod 2π s), the sun cuts' phase and
    /// the grid's phase (both mod 1).
    pub phase: [f32; 4],
    /// The horizon, the sky fall, the star threshold (above 1 when there are no stars) and the
    /// highest `p.y` a ridge or its rim can touch.
    pub scene: [f32; 4],
    pub sky: [f32; 3],
    pub glow: [f32; 3],
    pub grid: [f32; 3],
    pub sun_top: [f32; 3],
    pub sun_bottom: [f32; 3],
    /// The sun's centre x and y, its radius (with sound) and the halo's reach.
    pub sun: [f32; 4],
    /// Sun cuts, cut width, reflection strength, and the ridges' sound lift.
    pub stripes: [f32; 4],
    /// How many ridge layers to draw.
    pub layers: f32,
    /// The weights of octaves 0 to 3, and of octave 4.
    pub octaves: [f32; 4],
    pub octave4: f32,
    /// One column-major `mat4` per layer:
    /// - column 0: x scale, octave offsets 0, 1, 2;
    /// - column 1: octave offsets 3, 4, height, rim strength;
    /// - column 2: colour, the sound lift's phase;
    /// - column 3: unused.
    pub layer: [[f32; 16]; MAX_LAYERS],
    /// The floor's curve, perspective, lines across and lines in depth.
    pub floor: [f32; 4],
    /// Line width, grid glow, the grid's sound factor, unused.
    pub lines: [f32; 4],
    /// Hue turn, saturation and brightness as one column-major 3×3 matrix.
    pub colour: [f32; 9],
    /// Scanlines, bits, the levels `2^bits - 1`, unused.
    pub post: [f32; 4],
    /// The seed as the playground's `u_seed`, for the playground's own hashes in tests.
    pub lab_seed: f32,
}

fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f64) -> [f32; 3] {
    [0, 1, 2].map(|i| mix(f64::from(a[i]), f64::from(b[i]), t) as f32)
}

impl Frame {
    /// The uniforms to draw `look` at `width × height` pixels, `time` seconds in, with the sound
    /// level `audio` (0 to 1).
    pub fn new(look: &Look, width: usize, height: usize, time: f64, audio: f32) -> Self {
        Self::build(look, width, height, time, audio, true)
    }

    /// As [`Frame::new`], with the time terms unwrapped, as the playground passes them. Only for
    /// comparing against the playground's shader at short running times.
    #[doc(hidden)]
    pub fn unwrapped(look: &Look, width: usize, height: usize, time: f64, audio: f32) -> Self {
        Self::build(look, width, height, time, audio, false)
    }

    fn build(look: &Look, width: usize, height: usize, t: f64, audio: f32, wrap: bool) -> Self {
        let l = look;
        let (w, h) = (width.max(1) as f64, height.max(1) as f64);
        let px = 1. / h;
        let audio = if audio.is_finite() {
            f64::from(audio.clamp(0., 1.))
        } else {
            0.
        };
        let t = if t.is_finite() { t } else { 0. };
        let g = |x: f32| f64::from(x);
        let wrap_to = |x: f64, period: f64| if wrap { x.rem_euclid(period) } else { x };

        let cyc = (t * g(l.cycle_speed) * 0.10472).sin();
        let horizon = g(l.horizon);
        let sun_y = horizon + g(l.sun_y) + cyc * g(l.cycle_sun);
        let sun_r = g(l.sun_size) * (1. + audio * g(l.audio_sun));

        let mut weights = [0.; OCTAVES];
        let (mut a, mut sum) = (0.5, 0.);
        for w in &mut weights {
            *w = a;
            sum += a;
            a *= g(l.roughness);
        }
        let weights = weights.map(|w| (w / sum) as f32);

        let n = g(l.layers);
        let mut layer = [[0.; 16]; MAX_LAYERS];
        let mut highest: f64 = 0.;
        let mut rim_total = 0.;
        let mut ridges_below = false;
        let grid_max = l.grid_color.iter().fold(0f32, |m, &c| m.max(c.abs()));
        let drawn = (0..MAX_LAYERS).take_while(|&i| (i as f64) < n).count();
        for (i, m) in layer.iter_mut().enumerate().take(drawn) {
            let fi = i as f64;
            let f = if n <= 1. { 1. } else { fi / (n - 1.) };
            let scale = g(l.ridge_frequency) * mix(1.7, 0.7, f);
            let base = fi * 37.1 + t * g(l.ridge_speed) * 0.02 * mix(0.3, 1., f);
            let mut offsets = [0.; OCTAVES];
            let mut o = base;
            for slot in &mut offsets {
                *slot = wrap_to(o, RIDGE_TABLE as f64) as f32;
                o = o * g(LACUNARITY) + OCTAVE_SHIFT;
            }
            let amp = g(l.ridge_height) * mix(1., 0.45, f);
            let rim = g(l.rim) * (0.25 + 0.75 * f);
            let colour = mix3(l.ridge_color, l.glow_color, g(l.haze) * (1. - f) * 0.6);
            let lift_phase = (3. * base + fi).rem_euclid(TAU);
            highest = highest.max(amp);
            ridges_below |= amp < 0.;
            rim_total += rim.abs() * g(grid_max);
            *m = [
                scale as f32,
                offsets[0],
                offsets[1],
                offsets[2],
                offsets[3],
                offsets[4],
                amp as f32,
                rim as f32,
                colour[0],
                colour[1],
                colour[2],
                lift_phase as f32,
                0.,
                0.,
                0.,
                0.,
            ];
        }
        // Above the tallest possible ridge, the ridges' only trace is their rim's exponential
        // tail; past the height where all the rims together add under 1/1024, skip them.
        let band_top = if ridges_below || drawn == 0 {
            if drawn == 0 {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        } else {
            let lift = 1. + audio * g(l.audio_ridge).max(0.);
            let tail = 2.5 * px * (rim_total * 1024.).max(1.).ln();
            horizon + highest * lift + px + tail
        };

        let star_threshold = if l.stars > 0. {
            1. - g(l.stars) * 0.12
        } else {
            2.
        };

        Frame {
            res: [w as f32, h as f32],
            phase: [
                wrap_to(t, TAU * 128.) as f32,
                wrap_to(t, TAU) as f32,
                (t * g(l.grid_speed) * 0.05).rem_euclid(1.) as f32,
                (t * g(l.grid_speed)).rem_euclid(1.) as f32,
            ],
            scene: [
                l.horizon,
                l.sky_fall,
                star_threshold as f32,
                band_top.clamp(-1e30, 1e30) as f32,
            ],
            sky: l.sky_color,
            glow: l.glow_color,
            grid: l.grid_color,
            sun_top: l.sun_top_color,
            sun_bottom: l.sun_bottom_color,
            sun: [l.sun_x, sun_y as f32, sun_r as f32, l.halo],
            stripes: [
                l.stripes,
                l.stripe_width,
                l.reflection,
                (audio * g(l.audio_ridge)) as f32,
            ],
            layers: drawn as f32,
            octaves: [weights[0], weights[1], weights[2], weights[3]],
            octave4: weights[4],
            layer,
            floor: [
                l.curve,
                l.perspective.max(1e-4),
                l.grid_x.max(1e-4),
                l.grid_z.max(1e-4),
            ],
            lines: [
                l.line_width,
                l.grid_glow,
                (1. + audio * g(l.audio_grid) * 2.) as f32,
                0.,
            ],
            colour: colour_matrix(l, cyc),
            post: [l.scanlines, l.bits, l.bits.exp2() - 1., 0.],
            lab_seed: l.seed as f32,
        }
    }
}

/// The playground's finish as one matrix: the hue turn about the grey axis (Rodrigues), then
/// the mix toward luma by saturation, then brightness. All three are linear in the colour.
fn colour_matrix(l: &Look, cyc: f64) -> [f32; 9] {
    let a = (f64::from(l.hue) + cyc * f64::from(l.cycle_hue)).to_radians();
    let (sa, ca) = a.sin_cos();
    let k = 1. / 3f64.sqrt();
    // Rows of R = ca·I + sa·[k]× + (1 − ca)·k kᵀ.
    let mut r = [[0.; 3]; 3];
    let cross = [[0., -k, k], [k, 0., -k], [-k, k, 0.]];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            let id = if i == j { 1. } else { 0. };
            *v = ca * id + sa * cross[i][j] + (1. - ca) * k * k;
        }
    }
    let luma = [0.299, 0.587, 0.114];
    let (s, b) = (f64::from(l.saturation), f64::from(l.brightness));
    let mut m = [[0.; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            // S = s·I + (1 − s)·[luma; luma; luma], then M = b · S · R.
            *v = b
                * (0..3)
                    .map(|q| {
                        let sv = s * if i == q { 1. } else { 0. } + (1. - s) * luma[q];
                        sv * r[q][j]
                    })
                    .sum::<f64>();
        }
    }
    // Column-major for GLSL.
    let mut out = [0.; 9];
    for c in 0..3 {
        for row in 0..3 {
            out[c * 3 + row] = m[row][c] as f32;
        }
    }
    out
}
