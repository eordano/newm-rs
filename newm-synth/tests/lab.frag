#version 100

precision highp float;
uniform vec2 uRes;
uniform float uTime;
uniform float uAudio;
uniform float u_seed;
uniform float u_horizon;
uniform float u_skyFall;
uniform float u_stars;
uniform float u_sunSize;
uniform float u_sunY;
uniform float u_sunX;
uniform float u_stripes;
uniform float u_stripeW;
uniform float u_halo;
uniform float u_reflect;
uniform float u_layers;
uniform float u_ridgeH;
uniform float u_rough;
uniform float u_ridgeFreq;
uniform float u_ridgeSpeed;
uniform float u_haze;
uniform float u_rim;
uniform float u_persp;
uniform float u_gridX;
uniform float u_gridZ;
uniform float u_lineW;
uniform float u_gridGlow;
uniform float u_gridSpeed;
uniform float u_curve;
uniform float u_cycleSpeed;
uniform float u_cycleSun;
uniform float u_cycleHue;
uniform float u_hue;
uniform float u_sat;
uniform float u_bright;
uniform float u_scan;
uniform float u_bits;
uniform float u_aSun;
uniform float u_aGrid;
uniform float u_aRidge;
uniform vec3 u_cSky;
uniform vec3 u_cGlow;
uniform vec3 u_cGrid;
uniform vec3 u_cSunTop;
uniform vec3 u_cSunBot;
uniform vec3 u_cRidge;

float hash11(float n) { return fract(sin(n * 127.1 + u_seed * 17.3) * 43758.5453); }
float hash21(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7)) + u_seed * 7.7) * 43758.5453); }
float vnoise(float x) {
  float i = floor(x), f = fract(x);
  f = f * f * (3.0 - 2.0 * f);
  return mix(hash11(i), hash11(i + 1.0), f);
}
float ridge(float x) {
  float a = 0.5, s = 0.0, n = 0.0;
  for (int i = 0; i < 5; i++) { s += a * vnoise(x); n += a; x = x * 2.03 + 11.7; a *= u_rough; }
  return s / n;
}
vec3 hueRotate(vec3 c, float a) {
  vec3 k = vec3(0.57735);
  float ca = cos(a), sa = sin(a);
  return c * ca + cross(k, c) * sa + k * dot(k, c) * (1.0 - ca);
}
float bayer2(vec2 a) { a = floor(a); return fract(a.x / 2.0 + a.y * a.y * 0.75); }
float bayer4(vec2 a) { return bayer2(0.5 * a) * 0.25 + bayer2(a); }

void main() {
  vec2 frag = gl_FragCoord.xy;
  vec2 p = (frag - 0.5 * uRes) / uRes.y;
  float px = 1.0 / uRes.y;
  float t = uTime;
  float cyc = sin(t * u_cycleSpeed * 0.10472);
  float h = u_horizon;

  // sky
  float up = max(p.y - h, 0.0);
  vec3 col = mix(u_cGlow, u_cSky, 1.0 - exp(-up * u_skyFall));

  // stars
  vec2 sg = p * 70.0;
  vec2 cell = floor(sg);
  float sh = hash21(cell);
  vec2 sp = fract(sg) - 0.5 - (vec2(hash21(cell + 3.1), hash21(cell + 7.3)) - 0.5) * 0.6;
  float star = step(1.0 - u_stars * 0.12, sh) * smoothstep(0.11, 0.0, length(sp));
  star *= (0.6 + 0.4 * sin(t * (1.0 + 2.0 * sh) + sh * 40.0)) * smoothstep(0.0, 0.2, up);
  col += star * vec3(0.85, 0.92, 1.0);

  // sun
  vec2 sc = vec2(u_sunX, h + u_sunY + cyc * u_cycleSun);
  float sr = u_sunSize * (1.0 + uAudio * u_aSun);
  vec2 d = p - sc;
  float dl = length(d);
  float sv = clamp(d.y / max(sr, 0.0001) * 0.5 + 0.5, 0.0, 1.0);
  vec3 sunc = mix(u_cSunBot, u_cSunTop, sv);
  float disc = smoothstep(sr + px, sr - px, dl);
  float sy = 1.0 - sv;
  float band = fract(sy * u_stripes - t * u_gridSpeed * 0.05);
  float edge = u_stripeW * sy;
  float aa = u_stripes * px / max(2.0 * sr, 0.0001);
  float cut = smoothstep(edge + aa, edge - aa, band) * step(0.42, sy) * step(0.5, u_stripes);
  disc *= 1.0 - cut;
  float halo = exp(-max(dl - sr, 0.0) / max(u_halo, 0.0005)) * 0.45 * step(0.001, u_halo);
  col += mix(u_cSunBot, u_cGlow, 0.5) * halo;
  col = mix(col, sunc, disc);

  // ridges, far to near
  for (int i = 0; i < 6; i++) {
    float fi = float(i);
    if (fi >= u_layers) break;
    float f = u_layers <= 1.0 ? 1.0 : fi / (u_layers - 1.0);
    float x = p.x * u_ridgeFreq * mix(1.7, 0.7, f) + fi * 37.1 + t * u_ridgeSpeed * 0.02 * mix(0.3, 1.0, f);
    float lift = 1.0 + uAudio * u_aRidge * (0.5 + 0.5 * sin(x * 3.0 + fi));
    float top = h + u_ridgeH * mix(1.0, 0.45, f) * ridge(x) * lift;
    float m = smoothstep(top + px, top - px, p.y);
    vec3 rc = mix(u_cRidge, u_cGlow, u_haze * (1.0 - f) * 0.6);
    col = mix(col, rc, m);
    col += u_cGrid * exp(-abs(p.y - top) / (2.5 * px)) * u_rim * (0.25 + 0.75 * f);
  }

  // floor
  float yy = p.y - u_curve * p.x * p.x;
  float dy = h - yy;
  if (dy > 0.0) {
    float z = u_persp / dy;
    float xw = p.x * z;
    float pz = z * px;
    float distx = (0.5 - abs(fract(xw * u_gridX) - 0.5)) / u_gridX;
    float lx = 1.0 - smoothstep(u_lineW * pz * 0.5, u_lineW * pz * 0.5 + pz, distx);
    float pzz = z * z * px / u_persp;
    float distz = (0.5 - abs(fract(z * u_gridZ + t * u_gridSpeed) - 0.5)) / u_gridZ;
    float lz = 1.0 - smoothstep(u_lineW * pzz * 0.5, u_lineW * pzz * 0.5 + pzz, distz);
    lz *= clamp(1.0 / (pzz * u_gridZ * 3.0), 0.0, 1.0);
    lx *= clamp(1.0 / (pz * u_gridX * 3.0), 0.0, 1.0);
    float glow = u_gridGlow * 0.35 * (exp(-distx / (pz * 6.0)) + exp(-distz / (pzz * 6.0)));
    float fade = exp(-z * 0.12);
    vec3 ground = mix(u_cSky * 0.7, u_cGlow * 0.5, exp(-dy * u_skyFall * 2.0));
    ground += u_cGrid * (max(lx, lz) + glow) * fade * (1.0 + uAudio * u_aGrid * 2.0);
    vec2 pm = vec2(p.x, 2.0 * h - yy) - sc;
    float refl = smoothstep(sr * 1.4, sr * 0.2, length(pm * vec2(1.0, 0.55)));
    ground += mix(u_cSunBot, u_cSunTop, 0.3) * refl * u_reflect * 0.5 * (0.6 + 0.4 * sin(z * 40.0 + t));
    col = ground;
  }
  col += u_cGlow * exp(-abs(yy - h) / 0.006) * 0.5;

  // finish
  col = hueRotate(col, radians(u_hue + cyc * u_cycleHue));
  float l = dot(col, vec3(0.299, 0.587, 0.114));
  col = mix(vec3(l), col, u_sat) * u_bright;
  col *= 1.0 - u_scan * 0.5 * (0.5 + 0.5 * sin(frag.y * 3.14159));
  col = clamp(col, 0.0, 1.0);
  float levels = exp2(u_bits) - 1.0;
  if (u_bits < 7.5) col = floor(col * levels + bayer4(frag)) / levels;
  gl_FragColor = vec4(col, 1.0);
}