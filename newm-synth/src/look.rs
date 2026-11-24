//! The look: every knob of the picture, with the values the user tuned in the WebGL playground
//! as defaults, and the seeded tables the picture's randomness comes from.

/// Entries in the ridge noise table. Value noise on it repeats every `RIDGE_TABLE` cells, and
/// each octave's offset is wrapped by it, so the ridges drift forever without a seam.
pub const RIDGE_TABLE: usize = 1024;
/// Star cells per side of the star table: the star field repeats every 256 cells (3.7 screen
/// heights), wider than any output's width at 70 cells per screen height up to a 3.6:1 aspect.
pub const STAR_TABLE: usize = 256;
/// The tables as one RGBA texture: `STAR_TABLE` rows of stars, then the ridge table as
/// `RIDGE_TABLE / TABLE_WIDTH` rows of 16-bit values (high byte in red, low in green).
pub const TABLE_WIDTH: usize = 256;
pub const TABLE_HEIGHT: usize = STAR_TABLE + RIDGE_TABLE / TABLE_WIDTH;
/// The most ridge layers the shader draws.
pub const MAX_LAYERS: usize = 6;

/// Every knob of the picture. The names and meanings follow the playground's sliders; `p` below
/// is a point on the screen in screen heights from the centre, y up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Seeds the stars and the ridges.
    pub seed: u32,
    /// The horizon's height, from -0.5 (bottom) to 0.5 (top).
    pub horizon: f32,
    /// How fast the sky darkens from the horizon glow upward.
    pub sky_fall: f32,
    /// How many star cells hold a star: 1 is 12%.
    pub stars: f32,
    /// The sun's radius in screen heights.
    pub sun_size: f32,
    /// The sun's centre above the horizon.
    pub sun_y: f32,
    /// The sun's centre across, from the middle.
    pub sun_x: f32,
    /// Horizontal cuts across the sun's lower part; under 0.5 none.
    pub stripes: f32,
    /// How wide the cuts get toward the bottom.
    pub stripe_width: f32,
    /// How far the glow around the sun reaches; 0 none.
    pub halo: f32,
    /// How strongly the floor reflects the sun.
    pub reflection: f32,
    /// Ridge layers, farthest first, up to 6.
    pub layers: f32,
    /// The nearest ridge's height in screen heights.
    pub ridge_height: f32,
    /// How much each finer octave of the ridges weighs against the one before.
    pub roughness: f32,
    /// Ridge noise cells per screen height.
    pub ridge_frequency: f32,
    /// How fast the ridges drift sideways.
    pub ridge_speed: f32,
    /// How much the far ridges fade toward the glow colour.
    pub haze: f32,
    /// How strongly the ridge tops are lined with the grid colour.
    pub rim: f32,
    /// The floor's perspective: the camera height over the grid.
    pub perspective: f32,
    /// Grid lines across, per floor unit.
    pub grid_x: f32,
    /// Grid lines in depth, per floor unit.
    pub grid_z: f32,
    /// Grid line width in pixels at their distance.
    pub line_width: f32,
    /// The soft glow around the grid lines.
    pub grid_glow: f32,
    /// How fast the grid comes toward the viewer (and the sun's cuts move down).
    pub grid_speed: f32,
    /// How much the floor bends down at the sides.
    pub curve: f32,
    /// How fast the slow cycle runs: one cycle takes `60 / cycle_speed` seconds.
    pub cycle_speed: f32,
    /// How far the cycle moves the sun up and down.
    pub cycle_sun: f32,
    /// How far, in degrees, the cycle turns the hue.
    pub cycle_hue: f32,
    /// A fixed hue turn in degrees.
    pub hue: f32,
    /// Saturation: 1 as is, 0 grey.
    pub saturation: f32,
    /// The factor every colour is multiplied by.
    pub brightness: f32,
    /// Scanline darkening, 0 to 1.
    pub scanlines: f32,
    /// Bits per channel; under 7.5 the picture is dithered down to that many.
    pub bits: f32,
    /// How much the sun grows with sound.
    pub audio_sun: f32,
    /// How much the grid brightens with sound.
    pub audio_grid: f32,
    /// How much the ridges rise with sound.
    pub audio_ridge: f32,
    /// Colours, sRGB 0 to 1.
    pub sky_color: [f32; 3],
    pub glow_color: [f32; 3],
    pub grid_color: [f32; 3],
    pub sun_top_color: [f32; 3],
    pub sun_bottom_color: [f32; 3],
    pub ridge_color: [f32; 3],
}

const fn hex(v: u32) -> [f32; 3] {
    [
        ((v >> 16) & 0xff) as f32 / 255.,
        ((v >> 8) & 0xff) as f32 / 255.,
        (v & 0xff) as f32 / 255.,
    ]
}

impl Default for Look {
    /// The user's tuned settings (newm-synth-lab-settings.json, 2026-11-21).
    fn default() -> Self {
        Look {
            seed: 50,
            horizon: -0.22,
            sky_fall: 9.3,
            stars: 0.69,
            sun_size: 0.07,
            sun_y: 0.145,
            sun_x: -0.52,
            stripes: 7.,
            stripe_width: 0.5,
            halo: 0.07,
            reflection: 0.45,
            layers: 3.,
            ridge_height: 0.15,
            roughness: 0.5,
            ridge_frequency: 9.,
            ridge_speed: 6.9,
            haze: 0.45,
            rim: 0.4,
            perspective: 0.915,
            grid_x: 2.,
            grid_z: 2.5,
            line_width: 0.7,
            grid_glow: 0.1,
            grid_speed: 0.08,
            curve: 0.03,
            cycle_speed: 0.15,
            cycle_sun: 0.,
            cycle_hue: 22.,
            hue: 0.,
            saturation: 0.67,
            brightness: 0.7,
            scanlines: 0.,
            bits: 8.,
            audio_sun: 0.,
            audio_grid: 0.06,
            audio_ridge: 0.02,
            sky_color: hex(0x0b0c16),
            glow_color: hex(0x2f8f5b),
            grid_color: hex(0x82fb9c),
            sun_top_color: hex(0xddf7ff),
            sun_bottom_color: hex(0x82fb9c),
            ridge_color: hex(0x0d0f1c),
        }
    }
}

impl Look {
    /// Whether the picture changes with time. False only when every time-dependent term is off:
    /// twinkling stars, moving sun cuts, drifting ridges, the moving grid, the shimmering
    /// reflection and the slow cycle (when it moves the sun or the hue). Sound is not counted:
    /// the level is a separate input.
    pub fn moves(&self) -> bool {
        let sun = self.sun_size > 0.;
        let stars = self.stars > 0.;
        let cuts = sun && self.stripes >= 0.5 && self.grid_speed != 0.;
        let ridges = self.layers > 0. && self.ridge_height != 0. && self.ridge_speed != 0.;
        let grid = self.grid_speed != 0.;
        let shimmer = sun && self.reflection != 0.;
        let cycle = self.cycle_speed != 0. && (self.cycle_sun != 0. || self.cycle_hue != 0.);
        stars || cuts || ridges || grid || shimmer || cycle
    }
}

/// The seeded randomness: the star table (3 bytes a cell) and the ridge noise table (16 bits a
/// cell), drawn from a xorshift generator so every machine gets the same bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Tables {
    seed: u32,
    rgba: Vec<u8>,
}

impl std::fmt::Debug for Tables {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tables").field("seed", &self.seed).finish()
    }
}

struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        // Xorshift is stuck at zero; mix the seed so nearby seeds diverge at once.
        let s = seed.wrapping_mul(0x2545_f491) ^ 0x6d2b_79f5;
        let mut rng = Rng(if s == 0 { 1 } else { s });
        for _ in 0..8 {
            rng.next();
        }
        rng
    }

    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

impl Tables {
    pub fn new(seed: u32) -> Self {
        let mut rng = Rng::new(seed);
        let mut rgba = vec![0u8; TABLE_WIDTH * TABLE_HEIGHT * 4];
        for texel in rgba[..STAR_TABLE * TABLE_WIDTH * 4].as_chunks_mut::<4>().0 {
            let [a, b, c, _] = rng.next().to_le_bytes();
            texel.copy_from_slice(&[a, b, c, 0xff]);
        }
        for texel in rgba[STAR_TABLE * TABLE_WIDTH * 4..].as_chunks_mut::<4>().0 {
            let [a, b, _, _] = rng.next().to_le_bytes();
            texel.copy_from_slice(&[a, b, 0, 0xff]);
        }
        Tables { seed, rgba }
    }

    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// The texture's bytes, `TABLE_WIDTH × TABLE_HEIGHT` RGBA, top row first.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// The star hashes of cell `(x, y)`, each in `0..1` in steps of 1/256.
    pub fn star(&self, x: i64, y: i64) -> [f32; 3] {
        let (x, y) = (
            x.rem_euclid(STAR_TABLE as i64) as usize,
            y.rem_euclid(STAR_TABLE as i64) as usize,
        );
        let i = (y * TABLE_WIDTH + x) * 4;
        [0, 1, 2].map(|c| f32::from(self.rgba[i + c]) / 256.)
    }

    /// The ridge noise value of cell `n`, in `0..1` in steps of 1/65536.
    pub fn ridge(&self, n: i64) -> f32 {
        let n = n.rem_euclid(RIDGE_TABLE as i64) as usize;
        let i = ((STAR_TABLE + n / TABLE_WIDTH) * TABLE_WIDTH + n % TABLE_WIDTH) * 4;
        (f32::from(self.rgba[i]) * 256. + f32::from(self.rgba[i + 1])) / 65536.
    }
}
