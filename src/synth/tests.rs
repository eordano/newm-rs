use std::time::Instant;

use smithay::backend::egl::native::EGLSurfacelessDisplay;
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::renderer::gles::ffi;
use smithay::utils::Transform;

use super::*;
use crate::render_helpers::{render_to_vec, resources, shaders};

/// The playground's shader and the user's settings for it, as they sent them.
const LAB: &str = include_str!("../../newm-synth/tests/lab.frag");
const SETTINGS: &str = include_str!("../../newm-synth/tests/lab-settings.json");

fn renderer() -> GlesRenderer {
    let mut renderer = unsafe {
        let display = EGLDisplay::new(EGLSurfacelessDisplay).unwrap();
        let context = EGLContext::new(&display).unwrap();
        GlesRenderer::new(context).unwrap()
    };
    resources::init(&mut renderer);
    shaders::init(&mut renderer);
    renderer
}

fn gl_name(renderer: &mut GlesRenderer) -> String {
    renderer
        .with_context(|gl| unsafe {
            let p = gl.GetString(ffi::RENDERER);
            std::ffi::CStr::from_ptr(p.cast())
                .to_string_lossy()
                .into_owned()
        })
        .unwrap()
}

/// Draws one element with `program` in the synth slot and returns RGBA bytes, top row first.
fn draw(
    renderer: &mut GlesRenderer,
    program: &ShaderProgram,
    uniforms: Rc<[Uniform<'static>]>,
    w: i32,
    h: i32,
) -> Vec<u8> {
    let texture = upload_tables(renderer, Look::default().seed).unwrap();
    let previous = Shaders::get(renderer).synth.replace(Some(program.clone()));
    let size: Size<i32, Physical> = Size::from((w, h));
    let logical: Size<f64, Logical> = size.to_f64().to_logical(Scale::from(1.));
    let elem = ShaderRenderElement::new(
        ProgramType::Synth,
        logical,
        None,
        1.,
        1.,
        uniforms,
        HashMap::from([("synth_tables".to_owned(), texture)]),
        Kind::Unspecified,
    );
    let out = render_to_vec(
        renderer,
        size,
        Scale::from(1.),
        Transform::Normal,
        Fourcc::Abgr8888,
        std::iter::once(elem),
    )
    .unwrap();
    Shaders::get(renderer).synth.replace(previous);
    out
}

fn ours(renderer: &mut GlesRenderer, frame: &Frame, w: i32, h: i32) -> Vec<u8> {
    let program = compile(renderer, newm_synth::FRAGMENT_SHADER).unwrap();
    draw(renderer, &program, uniforms(frame), w, h)
}

/// Our shader with the playground's sine hashes, and the time terms unwrapped.
fn ours_lab_hash(renderer: &mut GlesRenderer, time: f64, audio: f32, w: i32, h: i32) -> Vec<u8> {
    let src = format!("#define NEWM_LAB_HASH\n{}", newm_synth::FRAGMENT_SHADER);
    let program = compile(renderer, &src).unwrap();
    let frame = Frame::unwrapped(&Look::default(), w as usize, h as usize, time, audio);
    draw(renderer, &program, uniforms(&frame), w, h)
}

/// The playground's shader itself, fed the user's settings file, turned the right way up: in
/// smithay's offscreen render `gl_FragCoord.y` counts from the image's top row.
fn lab(renderer: &mut GlesRenderer, time: f32, audio: f32, w: i32, h: i32) -> Vec<u8> {
    let settings: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(SETTINGS).unwrap();
    let mut names = vec![
        UniformName::new("uRes", UniformType::_2f),
        UniformName::new("uTime", UniformType::_1f),
        UniformName::new("uAudio", UniformType::_1f),
    ];
    let mut values = vec![
        Uniform::new("uRes", [w as f32, h as f32]),
        Uniform::new("uTime", time),
        Uniform::new("uAudio", audio),
    ];
    for (key, value) in &settings {
        let name = format!("u_{key}");
        if let Some(hex) = value.as_str() {
            let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
            let c = [16, 8, 0].map(|s| ((v >> s) & 0xff) as f32 / 255.);
            names.push(UniformName::new(name.clone(), UniformType::_3f));
            values.push(Uniform::new(name, UniformValue::_3f(c[0], c[1], c[2])));
        } else {
            names.push(UniformName::new(name.clone(), UniformType::_1f));
            values.push(Uniform::new(name, value.as_f64().unwrap() as f32));
        }
    }
    let src = LAB.strip_prefix("#version 100\n").unwrap();
    // The tables are bound for every draw; the playground does not read them.
    let program = ShaderProgram::compile(renderer, src, &names, &["synth_tables"]).unwrap();
    let out = draw(renderer, &program, values.into(), w, h);
    let row = w as usize * 4;
    out.chunks_exact(row).rev().flatten().copied().collect()
}

fn cpu(frame: &Frame, w: usize, h: usize) -> Vec<u8> {
    let mut px = vec![0u32; w * h];
    newm_synth::render(&Tables::new(Look::default().seed), frame, &mut px, w, h, w);
    px.iter().flat_map(|p| p.to_le_bytes()).collect()
}

struct Diff {
    /// The share of pixels whose largest channel difference is more than the levels asked.
    off: f64,
    /// The mean channel difference.
    mean: f64,
    max: u8,
}

fn compare(a: &[u8], b: &[u8], levels: u8) -> Diff {
    assert_eq!(a.len(), b.len());
    let (mut off, mut sum, mut max) = (0usize, 0u64, 0u8);
    for (p, q) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        let d = (0..3).map(|i| p[i].abs_diff(q[i])).max().unwrap();
        sum += (0..3).map(|i| u64::from(p[i].abs_diff(q[i]))).sum::<u64>();
        max = max.max(d);
        if d > levels {
            off += 1;
        }
    }
    let n = a.len() / 4;
    Diff {
        off: off as f64 / n as f64,
        mean: sum as f64 / (n * 3) as f64,
        max,
    }
}

fn mean_level(a: &[u8]) -> f64 {
    let sum: u64 = a
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2]))
        .sum();
    sum as f64 / (a.len() / 4 * 3) as f64
}

const SIZES: [(i32, i32); 2] = [(1440, 960), (2560, 720)];
const MOMENTS: [(f64, f32); 4] = [(0., 0.), (60., 0.), (600., 0.), (0., 0.8)];

#[test]
fn the_defaults_are_the_users_settings() {
    let settings: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(SETTINGS).unwrap();
    assert_eq!(settings.len(), 42);
    let l = Look::default();
    let numbers: [(&str, f32); 36] = [
        ("seed", l.seed as f32),
        ("horizon", l.horizon),
        ("skyFall", l.sky_fall),
        ("stars", l.stars),
        ("sunSize", l.sun_size),
        ("sunY", l.sun_y),
        ("sunX", l.sun_x),
        ("stripes", l.stripes),
        ("stripeW", l.stripe_width),
        ("halo", l.halo),
        ("reflect", l.reflection),
        ("layers", l.layers),
        ("ridgeH", l.ridge_height),
        ("rough", l.roughness),
        ("ridgeFreq", l.ridge_frequency),
        ("ridgeSpeed", l.ridge_speed),
        ("haze", l.haze),
        ("rim", l.rim),
        ("persp", l.perspective),
        ("gridX", l.grid_x),
        ("gridZ", l.grid_z),
        ("lineW", l.line_width),
        ("gridGlow", l.grid_glow),
        ("gridSpeed", l.grid_speed),
        ("curve", l.curve),
        ("cycleSpeed", l.cycle_speed),
        ("cycleSun", l.cycle_sun),
        ("cycleHue", l.cycle_hue),
        ("hue", l.hue),
        ("sat", l.saturation),
        ("bright", l.brightness),
        ("scan", l.scanlines),
        ("bits", l.bits),
        ("aSun", l.audio_sun),
        ("aGrid", l.audio_grid),
        ("aRidge", l.audio_ridge),
    ];
    for (key, value) in numbers {
        assert_eq!(settings[key].as_f64().unwrap() as f32, value, "{key}");
    }
    let colours = [
        ("cSky", l.sky_color),
        ("cGlow", l.glow_color),
        ("cGrid", l.grid_color),
        ("cSunTop", l.sun_top_color),
        ("cSunBot", l.sun_bottom_color),
        ("cRidge", l.ridge_color),
    ];
    for (key, value) in colours {
        let hex = settings[key].as_str().unwrap();
        let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
        let c = [16, 8, 0].map(|s| ((v >> s) & 0xff) as f32 / 255.);
        assert_eq!(c, value, "{key}");
    }
    // And the config block with nothing but a mode draws exactly that look.
    let config = SynthBackground {
        mode: SynthMode::Animated,
        ..SynthBackground::default()
    };
    assert_eq!(look(&config), l);
}

#[test]
fn every_knob_reaches_the_look() {
    let mut text = String::from("synth-background {\n seed 9\n");
    for knob in niri_config::SYNTH_KNOBS {
        let kdl = knob.replace('_', "-");
        if knob.ends_with("_color") {
            text += &format!(" {kdl} \"#102030\"\n");
        } else {
            text += &format!(" {kdl} 1\n");
        }
    }
    text += "}\n";
    let config = niri_config::Config::parse_mem(&text).unwrap();
    let l = look(&config.synth_background);
    let debug = format!("{l:?}");
    assert_eq!(l.seed, 9);
    for knob in niri_config::SYNTH_KNOBS {
        let want = if knob.ends_with("_color") {
            format!("{knob}: {:?}", [16f32 / 255., 32. / 255., 48. / 255.])
        } else {
            format!("{knob}: 1.0")
        };
        assert!(debug.contains(&want), "{want} in {debug}");
    }
}

/// The restructured shader draws the playground's picture. With the playground's own hashes
/// switched back on (`NEWM_LAB_HASH`) and the time terms unwrapped, it is compared with the
/// playground's shader itself, fed the user's settings file, on the same GPU, at both output
/// shapes and four moments. What may differ is rounding: the octave inputs summed in another
/// order, the colour finish as one matrix, the halo and rim tails it no longer computes where
/// they add under 1/1024.
#[test]
fn egl_the_restructured_shader_draws_the_playground() {
    let mut renderer = renderer();
    eprintln!("on {}", gl_name(&mut renderer));
    for (w, h) in SIZES {
        for (t, audio) in MOMENTS {
            let a = lab(&mut renderer, t as f32, audio, w, h);
            let b = ours_lab_hash(&mut renderer, t, audio, w, h);
            let d = compare(&a, &b, 2);
            eprintln!(
                "{w}x{h} t={t} audio={audio}: {:.4}% off by >2, mean {:.4}, max {}",
                d.off * 100.,
                d.mean,
                d.max
            );
            assert!(d.off <= 0.001, "{w}x{h} t={t}: {:.4}%", d.off * 100.);
            assert!(d.mean < 0.1, "{w}x{h} t={t}: mean {}", d.mean);
        }
    }
}

/// The seeded tables in place of the sine hashes move the stars and reshape the ridges, and
/// change nothing else: the same sky, sun, floor and colours, and the same overall light.
#[test]
fn egl_the_seeded_hashes_change_only_stars_and_ridges() {
    let mut renderer = renderer();
    let (w, h) = (1440, 960);
    let a = ours_lab_hash(&mut renderer, 0., 0., w, h);
    let frame = Frame::new(&Look::default(), w as usize, h as usize, 0., 0.);
    let b = ours(&mut renderer, &frame, w, h);
    let (ma, mb) = (mean_level(&a), mean_level(&b));
    let d = compare(&a, &b, 2);
    eprintln!(
        "playground hashes vs seeded: mean level {ma:.2} vs {mb:.2}; {:.2}% of pixels differ by >2",
        d.off * 100.
    );
    assert!((ma - mb).abs() < 1., "{ma} vs {mb}");
    // Every differing pixel lies in a star cell or the ridge band, never on the floor.
    let floor_top = (0.5 + 0.22) * f64::from(h);
    let floor_diff = (floor_top.ceil() as usize + 2..h as usize)
        .flat_map(|y| (0..w as usize).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let i = (y * w as usize + x) * 4;
            (0..3).any(|c| a[i + c].abs_diff(b[i + c]) > 2)
        })
        .count();
    assert_eq!(floor_diff, 0, "the floor is the same");
}

/// The CPU port draws what the shader draws, so a still matches the compositor's picture.
/// Tolerance: 0.1% of pixels off by more than one level; the rest is f32 maths
/// in another order and the GPU's own `exp`, `sin` and `sqrt`.
#[test]
fn egl_the_cpu_port_draws_what_the_shader_draws() {
    let mut renderer = renderer();
    for (w, h, t, audio) in [
        (1440, 960, 0., 0.),
        (320, 200, 60., 0.),
        (400, 112, 600., 0.),
        (320, 200, 0., 0.8),
        (256, 160, 86_400.25, 0.),
    ] {
        let frame = Frame::new(&Look::default(), w, h, t, audio);
        let g = ours(&mut renderer, &frame, w as i32, h as i32);
        let c = cpu(&frame, w, h);
        let d = compare(&g, &c, 1);
        eprintln!(
            "{w}x{h} t={t} audio={audio}: {:.4}% off by >1, mean {:.4}, max {}",
            d.off * 100.,
            d.mean,
            d.max
        );
        assert!(d.off <= 0.001, "{w}x{h} t={t}: {:.4}%", d.off * 100.);
    }
}

/// Writes the look through the real shader path to `$NEWM_SYNTH_LOOK_DIR`, with the CPU still
/// and the playground's own picture beside it for comparison:
/// `NEWM_SYNTH_LOOK_DIR=/tmp/newm-synth-look cargo test --features canvas --lib look_pngs --
/// --ignored`.
#[test]
#[ignore = "writes PNGs for people to look at"]
fn look_pngs() {
    let dir = std::path::PathBuf::from(std::env::var("NEWM_SYNTH_LOOK_DIR").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut renderer = renderer();
    let write = |name: String, rgba: &[u8], w: i32, h: i32| {
        let px: Vec<u32> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| u32::from_le_bytes([p[0], p[1], p[2], 0xff]))
            .collect();
        newm_synth::png::write(&dir.join(&name), &px, w as usize, h as usize).unwrap();
        eprintln!("wrote {name} (mean level {:.2})", mean_level(rgba));
    };
    for (w, h) in SIZES {
        for (t, audio) in MOMENTS {
            let frame = Frame::new(&Look::default(), w as usize, h as usize, t, audio);
            let g = ours(&mut renderer, &frame, w, h);
            let name = if audio > 0. {
                format!("gpu-{w}x{h}-t{t}-audio{audio}.png")
            } else {
                format!("gpu-{w}x{h}-t{t}.png")
            };
            write(name, &g, w, h);
        }
        let l = lab(&mut renderer, 0., 0., w, h);
        write(format!("playground-{w}x{h}-t0.png"), &l, w, h);
    }
    let frame = Frame::new(&Look::default(), 1440, 960, 0., 0.);
    let c = cpu(&frame, 1440, 960);
    write("cpu-1440x960-t0.png".into(), &c, 1440, 960);
    let g = ours(&mut renderer, &frame, 1440, 960);
    let d = compare(&g, &c, 1);
    eprintln!(
        "cpu vs gpu 1440x960 t=0: {:.4}% off by >1, mean {:.4}, max {}",
        d.off * 100.,
        d.mean,
        d.max
    );
}

#[test]
fn egl_compiling_the_program_takes() {
    let mut renderer = renderer();
    let start = Instant::now();
    ensure_program(&mut renderer).unwrap();
    let compile = start.elapsed();
    let start = Instant::now();
    let _texture = upload_tables(&mut renderer, 50).unwrap();
    let upload = start.elapsed();
    let start = Instant::now();
    let _tables = Tables::new(50);
    eprintln!(
        "synth shader compile {compile:?}, table upload {upload:?} (tables made in {:?}) on {}",
        start.elapsed(),
        gl_name(&mut renderer)
    );
}

#[test]
fn only_a_lit_unlocked_uncovered_output_shows_it() {
    let shown = Visibility {
        monitors_active: true,
        locked: false,
        screenshot_ui: false,
        covered: false,
    };
    assert!(shown.shows());
    for hidden in [
        Visibility {
            monitors_active: false,
            ..shown
        },
        Visibility {
            locked: true,
            ..shown
        },
        Visibility {
            screenshot_ui: true,
            ..shown
        },
        Visibility {
            covered: true,
            ..shown
        },
    ] {
        assert!(!hidden.shows(), "{hidden:?}");
    }
}

#[test]
fn off_holds_nothing_and_still_holds_a_look() {
    let off = Synth::new(&SynthBackground::default());
    assert!(!off.is_allocated());
    assert_eq!(off.mode(), SynthMode::Off);

    let mut synth = Synth::new(&SynthBackground {
        mode: SynthMode::Still,
        ..SynthBackground::default()
    });
    assert!(synth.is_allocated());
    assert!(synth.apply_config(&SynthBackground::default()));
    assert!(!synth.is_allocated());
}

#[test]
fn egl_a_config_change_reuploads_only_a_new_seed() {
    let mut renderer = renderer();
    let mut config = SynthBackground {
        mode: SynthMode::Still,
        ..SynthBackground::default()
    };
    let mut synth = Synth::new(&config);
    let size = Size::from((64, 40));
    synth
        .render(&mut renderer, "test", size, Scale::from(1.))
        .unwrap();
    let tex = |s: &Synth| {
        s.active
            .as_ref()
            .unwrap()
            .texture
            .borrow()
            .as_ref()
            .map(|t| t.tex_id())
    };
    let first = tex(&synth).unwrap();

    config.brightness = Some(0.3);
    assert!(synth.apply_config(&config));
    assert_eq!(
        tex(&synth),
        Some(first),
        "a colour change keeps the texture"
    );

    config.seed = Some(99);
    assert!(synth.apply_config(&config));
    assert_eq!(tex(&synth), None, "a new seed drops it");
    assert!(
        !synth.apply_config(&config),
        "the same config changes nothing"
    );
}

/// GPU time per full frame at 2880×1920, for the playground's shader and ours, on this
/// machine's GPU: `cargo test --release --features canvas --lib gpu_cost -- --ignored
/// --nocapture`. Each draw is followed by `glFinish`; texture setup is included in all alike.
#[test]
#[ignore = "a timing, not a check"]
fn gpu_cost() {
    use crate::render_helpers::render_to_texture;

    let mut renderer = renderer();
    let (w, h) = (2880, 1920);
    let settings: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(SETTINGS).unwrap();
    let mut lab_names = vec![
        UniformName::new("uRes", UniformType::_2f),
        UniformName::new("uTime", UniformType::_1f),
        UniformName::new("uAudio", UniformType::_1f),
    ];
    let mut lab_values = vec![
        Uniform::new("uRes", [w as f32, h as f32]),
        Uniform::new("uTime", 60f32),
        Uniform::new("uAudio", 0f32),
    ];
    for (key, value) in &settings {
        let name = format!("u_{key}");
        if let Some(hex) = value.as_str() {
            let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
            let c = [16, 8, 0].map(|s| ((v >> s) & 0xff) as f32 / 255.);
            lab_names.push(UniformName::new(name.clone(), UniformType::_3f));
            lab_values.push(Uniform::new(name, UniformValue::_3f(c[0], c[1], c[2])));
        } else {
            lab_names.push(UniformName::new(name.clone(), UniformType::_1f));
            lab_values.push(Uniform::new(name, value.as_f64().unwrap() as f32));
        }
    }
    let lab = ShaderProgram::compile(
        &mut renderer,
        LAB.strip_prefix("#version 100\n").unwrap(),
        &lab_names,
        &["synth_tables"],
    )
    .unwrap();
    let ours = compile(&mut renderer, newm_synth::FRAGMENT_SHADER).unwrap();
    let frame = Frame::new(&Look::default(), w as usize, h as usize, 60., 0.);
    let texture = upload_tables(&mut renderer, 50).unwrap();

    for (name, program, values) in [
        (
            "playground",
            &lab,
            Rc::<[Uniform<'static>]>::from(lab_values),
        ),
        ("ours", &ours, uniforms(&frame)),
    ] {
        let previous = Shaders::get(&mut renderer)
            .synth
            .replace(Some(program.clone()));
        let size: Size<i32, Physical> = Size::from((w, h));
        let logical: Size<f64, Logical> = size.to_f64().to_logical(Scale::from(1.));
        let mut time = |n: u32| {
            let start = Instant::now();
            for _ in 0..n {
                let elem = ShaderRenderElement::new(
                    ProgramType::Synth,
                    logical,
                    None,
                    1.,
                    1.,
                    values.clone(),
                    HashMap::from([("synth_tables".to_owned(), texture.clone())]),
                    Kind::Unspecified,
                );
                let _ = render_to_texture(
                    &mut renderer,
                    size,
                    Scale::from(1.),
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    std::iter::once(elem),
                )
                .unwrap();
                renderer.with_context(|gl| unsafe { gl.Finish() }).unwrap();
            }
            start.elapsed() / n
        };
        time(5);
        let per = time(40);
        eprintln!(
            "{name}: {per:?} per {w}x{h} frame ({:.2} ns/px)",
            per.as_secs_f64() * 1e9 / f64::from(w * h)
        );
        Shaders::get(&mut renderer).synth.replace(previous);
    }
}
