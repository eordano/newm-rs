// newm's synth background: the user's WebGL playground shader (tests/lab.frag), restructured
// for a compositor. The maths per pixel is the playground's; what changed:
// - the hashes read a seeded table instead of fract(sin(x) * 43758.5453), which no two GPU
//   drivers agree on (define NEWM_LAB_HASH to get the playground's hashes back, for tests);
// - every time term arrives wrapped to its own period, and each ridge octave's offset wrapped
//   to the noise table, so nothing loses precision however long the session runs;
// - the floor replaces everything behind it, so its pixels skip the sky, stars, sun and ridges;
//   pixels above the highest a ridge or its rim can reach skip the ridges; switched-off terms
//   (no stars, no halo, no reflection, no grid glow, no scanlines, 8 bits) cost nothing;
// - the hue turn, saturation and brightness are one matrix made once per frame;
// - the pixel position comes from niri_v_coords, y up as gl_FragCoord is in the playground.

precision highp float;

#if defined(DEBUG_FLAGS)
uniform float niri_tint;
#endif

uniform float niri_alpha;
varying vec2 niri_v_coords;

uniform sampler2D synth_tables;
uniform vec2 synth_res;
uniform vec4 synth_phase;
uniform vec4 synth_scene;
uniform vec3 synth_sky;
uniform vec3 synth_glow;
uniform vec3 synth_grid;
uniform vec3 synth_sun_top;
uniform vec3 synth_sun_bottom;
uniform vec4 synth_sun;
uniform vec4 synth_stripes;
uniform float synth_layers;
uniform vec4 synth_octaves;
uniform float synth_octave4;
uniform mat4 synth_layer[6];
uniform vec4 synth_floor;
uniform vec4 synth_lines;
uniform mat3 synth_colour;
uniform vec4 synth_post;
uniform float synth_lab_seed;

// The table texture: 256 star rows, then 4 rows of ridge noise.
const vec2 TABLE = vec2(256.0, 260.0);

vec4 table(vec2 texel) {
    return texture2D(synth_tables, (texel + 0.5) / TABLE);
}

float byte_(float v) {
    return floor(v * 255.0 + 0.5);
}

float ridge_hash(float n) {
#if defined(NEWM_LAB_HASH)
    return fract(sin(n * 127.1 + synth_lab_seed * 17.3) * 43758.5453);
#else
    float i = n - 1024.0 * floor(n / 1024.0);
    float row = floor(i / 256.0);
    vec4 v = table(vec2(i - row * 256.0, 256.0 + row));
    return (byte_(v.r) * 256.0 + byte_(v.g)) / 65536.0;
#endif
}

vec3 star_hash(vec2 cell) {
#if defined(NEWM_LAB_HASH)
    float s = synth_lab_seed * 7.7;
    return vec3(
        fract(sin(dot(cell, vec2(127.1, 311.7)) + s) * 43758.5453),
        fract(sin(dot(cell + 3.1, vec2(127.1, 311.7)) + s) * 43758.5453),
        fract(sin(dot(cell + 7.3, vec2(127.1, 311.7)) + s) * 43758.5453));
#else
    vec2 c = cell - 256.0 * floor(cell / 256.0);
    vec4 v = table(c);
    return vec3(byte_(v.r), byte_(v.g), byte_(v.b)) / 256.0;
#endif
}

float vnoise(float x) {
    float i = floor(x);
    float f = fract(x);
    f = f * f * (3.0 - 2.0 * f);
    return mix(ridge_hash(i), ridge_hash(i + 1.0), f);
}

// The playground's ridge(): five octaves, each input 2.03 × the last plus 11.7, written as the
// screen part (xs, scaled by 2.03 per octave) plus each octave's offset from the CPU.
float ridge(float xs, mat4 l) {
    float s = synth_octaves.x * vnoise(xs + l[0].y);
    xs *= 2.03;
    s += synth_octaves.y * vnoise(xs + l[0].z);
    xs *= 2.03;
    s += synth_octaves.z * vnoise(xs + l[0].w);
    xs *= 2.03;
    s += synth_octaves.w * vnoise(xs + l[1].x);
    xs *= 2.03;
    s += synth_octave4 * vnoise(xs + l[1].y);
    return s;
}

float bayer2(vec2 a) {
    a = floor(a);
    return fract(a.x / 2.0 + a.y * a.y * 0.75);
}

float bayer4(vec2 a) {
    return bayer2(0.5 * a) * 0.25 + bayer2(a);
}

void main() {
    vec2 res = synth_res;
    vec2 pixel = floor(niri_v_coords * res);
    vec2 frag = vec2(pixel.x + 0.5, res.y - pixel.y - 0.5);
    vec2 p = (frag - 0.5 * res) / res.y;
    float px = 1.0 / res.y;
    float h = synth_scene.x;
    vec2 sc = synth_sun.xy;
    float sr = synth_sun.z;

    vec3 col;
    float yy = p.y - synth_floor.x * p.x * p.x;
    float dy = h - yy;
    if (dy > 0.0) {
        // The floor, which covers everything behind it.
        float persp = synth_floor.y;
        float gx = synth_floor.z;
        float gz = synth_floor.w;
        float lw = synth_lines.x;
        float z = persp / dy;
        float xw = p.x * z;
        float pz = z * px;
        float distx = (0.5 - abs(fract(xw * gx) - 0.5)) / gx;
        float lx = 1.0 - smoothstep(lw * pz * 0.5, lw * pz * 0.5 + pz, distx);
        float pzz = z * z * px / persp;
        float distz = (0.5 - abs(fract(z * gz + synth_phase.w) - 0.5)) / gz;
        float lz = 1.0 - smoothstep(lw * pzz * 0.5, lw * pzz * 0.5 + pzz, distz);
        lz *= clamp(1.0 / (pzz * gz * 3.0), 0.0, 1.0);
        lx *= clamp(1.0 / (pz * gx * 3.0), 0.0, 1.0);
        float glow = 0.0;
        if (synth_lines.y != 0.0) {
            glow = synth_lines.y * 0.35 * (exp(-distx / (pz * 6.0)) + exp(-distz / (pzz * 6.0)));
        }
        float fade = exp(-z * 0.12);
        vec3 ground = mix(synth_sky * 0.7, synth_glow * 0.5, exp(-dy * synth_scene.y * 2.0));
        ground += synth_grid * (max(lx, lz) + glow) * fade * synth_lines.z;
        if (synth_stripes.z != 0.0) {
            vec2 pm = vec2(p.x, 2.0 * h - yy) - sc;
            float refl = smoothstep(sr * 1.4, sr * 0.2, length(pm * vec2(1.0, 0.55)));
            if (refl > 0.0) {
                ground += mix(synth_sun_bottom, synth_sun_top, 0.3) * refl * synth_stripes.z * 0.5
                    * (0.6 + 0.4 * sin(z * 40.0 + synth_phase.y));
            }
        }
        col = ground;
    } else {
        // The sky.
        float up = max(p.y - h, 0.0);
        col = mix(synth_glow, synth_sky, 1.0 - exp(-up * synth_scene.y));

        // Stars.
        if (synth_scene.z <= 1.0) {
            vec2 sg = p * 70.0;
            vec2 cell = floor(sg);
            vec3 hs = star_hash(cell);
            if (hs.x >= synth_scene.z) {
                vec2 sp = fract(sg) - 0.5 - (hs.yz - 0.5) * 0.6;
                float star = smoothstep(0.11, 0.0, length(sp));
                star *= (0.6 + 0.4 * sin(synth_phase.x * (1.0 + 2.0 * hs.x) + hs.x * 40.0))
                    * smoothstep(0.0, 0.2, up);
                col += star * vec3(0.85, 0.92, 1.0);
            }
        }

        // The sun, its cuts and its halo.
        vec2 d = p - sc;
        float dl = length(d);
        if (synth_sun.w >= 0.001) {
            float halo = exp(-max(dl - sr, 0.0) / max(synth_sun.w, 0.0005)) * 0.45;
            col += mix(synth_sun_bottom, synth_glow, 0.5) * halo;
        }
        float disc = smoothstep(sr + px, sr - px, dl);
        if (disc > 0.0) {
            float sv = clamp(d.y / max(sr, 0.0001) * 0.5 + 0.5, 0.0, 1.0);
            vec3 sunc = mix(synth_sun_bottom, synth_sun_top, sv);
            float stripes = synth_stripes.x;
            if (stripes >= 0.5) {
                float sy = 1.0 - sv;
                float band = fract(sy * stripes - synth_phase.z);
                float edge = synth_stripes.y * sy;
                float aa = stripes * px / max(2.0 * sr, 0.0001);
                float cut = smoothstep(edge + aa, edge - aa, band) * step(0.42, sy);
                disc *= 1.0 - cut;
            }
            col = mix(col, sunc, disc);
        }

        // Ridges, far to near, only where one can reach.
        if (p.y <= synth_scene.w) {
            for (int i = 0; i < 6; i++) {
                if (float(i) >= synth_layers) {
                    break;
                }
                mat4 l = synth_layer[i];
                float xs = p.x * l[0].x;
                float lift = 1.0;
                if (synth_stripes.w != 0.0) {
                    lift += synth_stripes.w * (0.5 + 0.5 * sin(3.0 * xs + l[2].w));
                }
                float top = h + l[1].z * ridge(xs, l) * lift;
                float m = smoothstep(top + px, top - px, p.y);
                col = mix(col, l[2].xyz, m);
                col += synth_grid * exp(-abs(p.y - top) / (2.5 * px)) * l[1].w;
            }
        }
    }
    col += synth_glow * exp(-abs(yy - h) / 0.006) * 0.5;

    // The finish: hue, saturation and brightness, scanlines, then the bit depth.
    col = synth_colour * col;
    if (synth_post.x != 0.0) {
        col *= 1.0 - synth_post.x * 0.5 * (0.5 + 0.5 * sin(frag.y * 3.14159));
    }
    col = clamp(col, 0.0, 1.0);
    if (synth_post.y < 7.5) {
        col = floor(col * synth_post.z + bayer4(frag)) / synth_post.z;
    }

    vec4 color = vec4(col, 1.0) * niri_alpha;

#if defined(DEBUG_FLAGS)
    if (niri_tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
