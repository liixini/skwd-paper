pub(in crate::effects) const CROSSWARP_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
void main() {
    float x = u_progress;
    x = smoothstep(0.0, 1.0, (x * 2.0 + v_uv.x - 1.0));
    frag = mix(
        texture(u_tex_old, (v_uv - 0.5) * (1.0 - x) + 0.5),
        texture(u_tex_new, (v_uv - 0.5) * x + 0.5),
        x
    );
}
";

pub(in crate::effects) const MORPH_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const float strength_v = 0.15;
void main() {
    vec4 ca = texture(u_tex_old, v_uv);
    vec4 cb = texture(u_tex_new, v_uv);
    vec2 oa = (((ca.rg + ca.b) * 0.5) * 2.0 - 1.0);
    vec2 ob = (((cb.rg + cb.b) * 0.5) * 2.0 - 1.0);
    vec2 oc = mix(oa, ob, 0.5) * strength_v;
    float w0 = u_progress;
    float w1 = 1.0 - w0;
    frag = mix(texture(u_tex_old, v_uv + oc * w0), texture(u_tex_new, v_uv - oc * w1), u_progress);
}
";

pub(in crate::effects) const CIRCLE_CROP_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const float ratio = 1.7777;
const vec4 bgcolor = vec4(0.0, 0.0, 0.0, 1.0);

vec2 ratio2 = vec2(1.0, 1.0 / ratio);
float s = pow(2.0 * abs(u_progress - 0.5), 3.0);

vec4 transition(vec2 p) {
  float dist = length((vec2(p) - 0.5) * ratio2);
  return mix(
    u_progress < 0.5 ? texture(u_tex_old, p) : texture(u_tex_new, p), // branching is ok here as we statically depend on u_progress uniform (branching won't change over pixels)
    bgcolor,
    step(s, dist)
  );
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const COLOUR_DISTANCE_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const float power = 5.0;

vec4 transition(vec2 p) {
  vec4 fTex = texture(u_tex_old, p);
  vec4 tTex = texture(u_tex_new, p);
  float m = step(distance(fTex, tTex), u_progress);
  return mix(
    mix(fTex, tTex, m),
    tTex,
    pow(u_progress, power)
  );
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const DIRECTIONAL_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const vec2 direction = vec2(0.0, 1.0);

vec4 transition (vec2 uv) {
  vec2 p = uv + u_progress * sign(direction);
  vec2 f = fract(p);
  return mix(
    texture(u_tex_new, f),
    texture(u_tex_old, f),
    step(0.0, p.y) * step(p.y, 1.0) * step(0.0, p.x) * step(p.x, 1.0)
  );
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const DIRECTIONAL_SCALED_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
#define PI acos(-1.0)

const vec2 direction = vec2(0.0, 1.0);
const float scale = .7;

float parabola(float x) {
  float y = pow(sin(x * PI), 1.);
  return y;
}

vec4 transition (vec2 uv) {
  float easedProgress = pow(sin(u_progress  * PI / 2.), 3.);
  vec2 p = uv + easedProgress * sign(direction);
  vec2 f = fract(p);
  
  float s = 1. - (1. - (1. / scale)) * parabola(u_progress);
  f = (f - 0.5) * s  + 0.5;
  
  float mixer = step(0.0, p.y) * step(p.y, 1.0) * step(0.0, p.x) * step(p.x, 1.0);
  vec4 col = mix(texture(u_tex_new, f), texture(u_tex_old, f), mixer);
  
  float border = step(0., f.x) * step(0., (1. - f.x)) * step(0., f.y) * step(0., 1. - f.y);
  col *= border;
  
  return col;
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const GLITCH_DISPLACE_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
float random(vec2 co)
{
    float a = 12.9898;
    float b = 78.233;
    float c = 43758.5453;
    float dt= dot(co.xy ,vec2(a,b));
    float sn= mod(dt,3.14);
    return fract(sin(sn) * c);
}
float voronoi( in vec2 x ) {
    vec2 p = floor( x );
    vec2 f = fract( x );
    float res = 8.0;
    for( float j=-1.; j<=1.; j++ )
    for( float i=-1.; i<=1.; i++ ) {
        vec2  b = vec2( i, j );
        vec2  r = b - f + random( p + b );
        float d = dot( r, r );
        res = min( res, d );
    }
    return sqrt( res );
}

vec2 displace(vec4 tex, vec2 texCoord, float dotDepth, float textureDepth, float strength) {
    float b = voronoi(.003 * texCoord + 2.0);
    float g = voronoi(0.2 * texCoord);
    float r = voronoi(texCoord - 1.0);
    vec4 dt = tex * 1.0;
    vec4 dis = dt * dotDepth + 1.0 - tex * textureDepth;

    dis.x = dis.x - 1.0 + textureDepth*dotDepth;
    dis.y = dis.y - 1.0 + textureDepth*dotDepth;
    dis.x *= strength;
    dis.y *= strength;
    vec2 res_uv = texCoord ;
    res_uv.x = res_uv.x + dis.x - 0.0;
    res_uv.y = res_uv.y + dis.y;
    return res_uv;
}

float ease1(float t) {
  return t == 0.0 || t == 1.0
    ? t
    : t < 0.5
      ? +0.5 * pow(2.0, (20.0 * t) - 10.0)
      : -0.5 * pow(2.0, 10.0 - (t * 20.0)) + 1.0;
}
float ease2(float t) {
  return t == 1.0 ? t : 1.0 - pow(2.0, -10.0 * t);
}


vec4 transition(vec2 uv) {
  vec2 p = uv.xy / vec2(1.0).xy;
  vec4 color1 = texture(u_tex_old, p);
  vec4 color2 = texture(u_tex_new, p);
  vec2 disp = displace(color1, p, 0.33, 0.7, 1.0-ease1(u_progress));
  vec2 disp2 = displace(color2, p, 0.33, 0.5, ease2(u_progress));
  vec4 dColor1 = texture(u_tex_new, disp);
  vec4 dColor2 = texture(u_tex_old, disp2);
  float val = ease1(u_progress);
  vec3 gray = vec3(dot(min(dColor2, dColor1).rgb, vec3(0.299, 0.587, 0.114)));
  dColor2 = vec4(gray, 1.0);
  dColor2 *= 2.0;
  color1 = mix(color1, dColor2, smoothstep(0.0, 0.5, u_progress));
  color2 = mix(color2, dColor1, smoothstep(1.0, 0.5, u_progress));
  return mix(color1, color2, val);
  //gl_FragColor = mix(gl_FragColor, dColor, smoothstep(0.0, 0.5, u_progress));

   //gl_FragColor = mix(texture(u_tex_old, p), texture(u_tex_new, p), u_progress);
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const POLKA_DOTS_CURTAIN_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const float SQRT_2 = 1.414213562373;
const float dots = 20.0;
const vec2 center = vec2(0, 0);

vec4 transition(vec2 uv) {
  if (u_progress >= 1.0) return texture(u_tex_new, uv);
  bool nextImage = distance(fract(uv * dots), vec2(0.5, 0.5)) < ( u_progress / distance(uv, center));
  return nextImage ? texture(u_tex_new, uv) : texture(u_tex_old, uv);
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const PUZZLE_RIGHT_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const ivec2 size = ivec2(4, 4);
const float pause = 0.1;
const float dividerWidth = 0.005;

float rand(vec2 co) {
  return fract(sin(dot(co, vec2(12.9898, 78.233))) * 43758.5453);
}

float getDelta(vec2 p) {
  vec2 rectangleSize = 1.0 / vec2(size);
  vec2 rectanglePos = floor(vec2(size) * p);
  float top = rectangleSize.y * (rectanglePos.y + 1.0);
  float bottom = rectangleSize.y * rectanglePos.y;
  float left = rectangleSize.x * rectanglePos.x;
  float right = rectangleSize.x * (rectanglePos.x + 1.0);
  float minX = min(abs(p.x - left), abs(p.x - right));
  float minY = min(abs(p.y - top), abs(p.y - bottom));
  return min(minX, minY);
}

vec4 transition(vec2 uv) {
  if (u_progress < pause) {
    float currentProg = u_progress / pause;
    float a = 1.0;
    if (getDelta(uv) < dividerWidth) { a = 1.0 - currentProg; }
    return mix(vec4(0.0, 0.0, 0.0, 1.0), texture(u_tex_old, uv), a);
  } else if (u_progress < 1.0 - pause) {
    if (getDelta(uv) < dividerWidth) {
      return vec4(0.0, 0.0, 0.0, 1.0);
    }
    float currentProg = (u_progress - pause) / (1.0 - pause * 2.0);
    vec2 rectanglePos = floor(vec2(size) * uv);
    float r = rand(rectanglePos) - 0.1;
    float cp = smoothstep(0.0, 1.0 - r, currentProg);
    float rectangleSize = 1.0 / float(size.x);
    float delta = rectanglePos.x * rectangleSize;
    float offset = rectangleSize / 2.0 + delta;
    vec2 p = uv;
    p.x = (p.x - offset) / abs(cp - 0.5) * 0.5 + offset;
    vec4 a = texture(u_tex_old, p);
    vec4 b = texture(u_tex_new, p);
    float s = step(abs(float(size.x) * (uv.x - delta) - 0.5), abs(cp - 0.5));
    return vec4(mix(b, a, step(cp, 0.5)).rgb * s, 1.0);
  } else {
    float currentProg = (u_progress - 1.0 + pause) / pause;
    float a = 1.0;
    if (getDelta(uv) < dividerWidth) { a = currentProg; }
    return mix(vec4(0.0, 0.0, 0.0, 1.0), texture(u_tex_new, uv), a);
  }
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const CROSSHATCH_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const vec2 center = vec2(0.5);
const float threshold = 3.0;
const float fadeEdge = 0.1;

float rand(vec2 co) {
  return fract(sin(dot(co.xy ,vec2(12.9898,78.233))) * 43758.5453);
}
vec4 transition(vec2 p) {
  float dist = distance(center, p) / threshold;
  float r = u_progress - min(rand(vec2(p.y, 0.0)), rand(vec2(0.0, p.x)));
  return mix(texture(u_tex_old, p), texture(u_tex_new, p), mix(0.0, mix(step(dist, r), 1.0, smoothstep(1.0-fadeEdge, 1.0, u_progress)), smoothstep(0.0, fadeEdge, u_progress)));    
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const DIRECTIONAL_WIPE_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const vec2 direction = vec2(1.0, -1.0);
const float smoothness = 0.5;
 
const vec2 center = vec2(0.5, 0.5);
 
vec4 transition (vec2 uv) {
  vec2 v = normalize(direction);
  v /= abs(v.x)+abs(v.y);
  float d = v.x * center.x + v.y * center.y;
  float m =
    (1.0-step(u_progress, 0.0)) * // there is something wrong with our formula that makes m not equals 0.0 with u_progress is 0.0
    (1.0 - smoothstep(-smoothness, 0.0, v.x * uv.x + v.y * uv.y - (d-0.5+u_progress*(1.+smoothness))));
  return mix(texture(u_tex_old, uv), texture(u_tex_new, uv), m);
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const FADECOLOR_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const vec3 color = vec3(0.0);
const float colorPhase = 0.4; // if 0.0, there is no black phase, if 0.9, the black phase is very important
vec4 transition (vec2 uv) {
  return mix(
    mix(vec4(color, 1.0), texture(u_tex_old, uv), smoothstep(1.0-colorPhase, 0.0, u_progress)),
    mix(vec4(color, 1.0), texture(u_tex_new, uv), smoothstep(    colorPhase, 1.0, u_progress)),
    u_progress);
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const PARAMETRIC_GLITCH_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const float ampx = 1.0;
const float ampy = 1.0;

vec4 transition (vec2 uv) {
  vec4 from = texture(u_tex_old, uv);
  vec4 to = texture(u_tex_new, uv);
  float r = from.r;
  float g = from.g;
  float b = from.b;
  float sphere = r*r + g*g + b*b - 1.0; //3 to 1
  float spiralX = cos(sphere - uv.x/(u_progress + .01));
  float spiralY = sin(sphere - uv.y/(u_progress+.01));
  vec2 st = uv;
  st.x = fract(ampx*st.x*spiralX); //1 to 2
  st.y = fract(ampy*st.y*spiralY);
  vec2 diff = uv - st;
  from = texture(u_tex_old, uv + u_progress*diff);
  return mix(from, to, u_progress);
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const PERLIN_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const float scale = 4.0;
const float smoothness = 0.01;

const float seed = 12.9898;

// http://byteblacksmith.com/improvements-to-the-canonical-one-liner-glsl-rand-for-opengl-es-2-0/
float random(vec2 co)
{
    float a = seed;
    float b = 78.233;
    float c = 43758.5453;
    float dt= dot(co.xy ,vec2(a,b));
    float sn= mod(dt,3.14);
    return fract(sin(sn) * c);
}

// 2D Noise based on Morgan McGuire @morgan3d
// https://www.shadertoy.com/view/4dS3Wd
float noise (in vec2 st) {
    vec2 i = floor(st);
    vec2 f = fract(st);

    // Four corners in 2D of a tile
    float a = random(i);
    float b = random(i + vec2(1.0, 0.0));
    float c = random(i + vec2(0.0, 1.0));
    float d = random(i + vec2(1.0, 1.0));

    // Smooth Interpolation

    // Cubic Hermine Curve.  Same as SmoothStep()
    vec2 u = f*f*(3.0-2.0*f);
    // u = smoothstep(0.,1.,f);

    // Mix 4 coorners porcentages
    return mix(a, b, u.x) +
            (c - a)* u.y * (1.0 - u.x) +
            (d - b) * u.x * u.y;
}

vec4 transition (vec2 uv) {
  vec4 from = texture(u_tex_old, uv);
  vec4 to = texture(u_tex_new, uv);
  float n = noise(uv * scale);

  float p = mix(-smoothness, 1.0 + smoothness, u_progress);
  float lower = p - smoothness;
  float higher = p + smoothness;

  float q = smoothstep(lower, higher, n);

  return mix(
    from,
    to,
    1.0 - q
  );
}
void main() {
    frag = transition(v_uv);
}
";

pub(in crate::effects) const RANDOMSQUARES_FRAG: &str = "#version 330 core
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex_old;
uniform sampler2D u_tex_new;
uniform float u_progress;
const ivec2 size = ivec2(10, 10);
const float smoothness = 0.5;
 
float rand (vec2 co) {
  return fract(sin(dot(co.xy ,vec2(12.9898,78.233))) * 43758.5453);
}

vec4 transition(vec2 p) {
  float r = rand(floor(vec2(size) * p));
  float m = smoothstep(0.0, -smoothness, r - (u_progress * (1.0 + smoothness)));
  return mix(texture(u_tex_old, p), texture(u_tex_new, p), m);
}
void main() {
    frag = transition(v_uv);
}
";
