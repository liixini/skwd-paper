const float PI = 3.14159265;
const float PAPER_START = DRAWING_PAPER_START;
const float DRAW_END = DRAWING_BUILD_END;
const float FOCUS_SCALE = 65535.0;
const uint FOCUS_MASK = 65535u;
const uint FOCUS_BITS = 16u;
const int RAINBOW_INK = 1;
const vec3 SPECTRUM_PHASE = vec3(0.0, 0.33, 0.67);
const float SPECTRUM_RUN = 0.55;
const float SPECTRUM_DRIFT = 0.04;
const float SPECTRUM_LEVEL = 0.82;
const float RAINBOW_DENSITY = 1.15;
const float WET_DARKEN = 0.55;
const float WET_WEIGHT = 0.6;
#define aspect (u_res.x/u_res.y)
#define focus ((vec2(uint(u_pid_base)&FOCUS_MASK,uint(u_pid_base)>>FOCUS_BITS)/FOCUS_SCALE-u_uv_b.zw)/u_uv_b.xy)
vec2 drawingUv(vec2 p) { return fill_wrap(clamp(p,vec2(0),vec2(1))*u_uv_b.xy+u_uv_b.zw); }
vec4 drawing(vec2 p) { return texture(chroma_b,drawingUv(p)); }
vec4 strokes(vec2 p) { return texture(chroma_a,drawingUv(p)); }
  vec3 drawingPaper() { return unpackUnorm4x8(u_paper).rgb; }
  vec3 inkColour() { return unpackUnorm4x8(u_ink).rgb; }
  vec3 photo(vec2 p) { return texB(clamp(p,vec2(0),vec2(1))).rgb; }
  vec3 spectrum(float hue) { return 0.5+0.5*cos(2.0*PI*(hue+SPECTRUM_PHASE)); }
  vec3 inked(vec2 p, float amount) {
    if(u_ink_style!=RAINBOW_INK) return mix(drawingPaper(), inkColour(), amount);
    vec4 trace=strokes(p);
    vec3 colour=spectrum(trace.g+trace.b*SPECTRUM_RUN+u_clock*SPECTRUM_DRIFT)*SPECTRUM_LEVEL;
    colour=mix(colour,colour*WET_DARKEN,trace.a*WET_WEIGHT);
    return mix(drawingPaper(), colour, clamp(amount*RAINBOW_DENSITY,0.0,1.0));
  }
  vec3 sketch(vec2 p) { return inked(p, drawing(p).r); }
  vec2 metric(vec2 p) { return p*vec2(aspect,1); }
  float reach() {
    return max(max(length(metric(focus)),length(metric(vec2(1,0)-focus))),max(length(metric(vec2(0,1)-focus)),length(metric(vec2(1)-focus))));
  }
struct LiquidStyle { float lobeCount; float rippleCount; float ripplePhase; float rippleWeight; float rimWidth; float speed; float delay; float inside; float edge; float normalFloor; float widthFloor; float bend; float redShift; float blueShift; float inkBend; float normalZ; float lightAxis; float specPower; float shadow; float specular; float complete; };
const LiquidStyle liquidStyle = LiquidStyle(3.0,5.0,1.1,0.38,0.035,1.25,0.07,0.2,0.25,0.0001,0.001,0.018,1.15,0.85,0.006,0.8,0.6,7.0,0.09,0.24,0.91);
  vec3 liquid(float t) {
    vec2 p=metric(v_uv-focus);
    float d=length(p);
    float angle=atan(p.y,p.x);
    float maxRadius=reach();
    float organic=(sin(angle*liquidStyle.lobeCount+0.5)+sin(angle*liquidStyle.rippleCount-liquidStyle.ripplePhase)*liquidStyle.rippleWeight)*liquidStyle.rimWidth*sin(t*PI);
    float radius=(t*liquidStyle.speed-liquidStyle.delay)*maxRadius;
    float sdf=d-radius-organic;
    float width=liquidStyle.rimWidth*maxRadius;
    float inside=1.0-smoothstep(-width*liquidStyle.inside,width*liquidStyle.edge,sdf);
    vec2 n=p/max(d,liquidStyle.normalFloor)/vec2(aspect,1);
    float rim=exp(-pow(sdf/max(width,liquidStyle.widthFloor),2.0))*sin(t*PI);
    vec2 bend=n*rim*liquidStyle.bend;
    vec3 glass=photo(v_uv-bend);
    glass.r=mix(glass.r,photo(v_uv-bend*liquidStyle.redShift).r,rim*liquidStyle.edge);
    glass.b=mix(glass.b,photo(v_uv-bend*liquidStyle.blueShift).b,rim*liquidStyle.edge);
    vec3 result=mix(sketch(v_uv+n*rim*liquidStyle.inkBend),glass,inside);
    float spec=pow(max(0.0,dot(normalize(vec3(p/max(d,liquidStyle.normalFloor),liquidStyle.normalZ)),normalize(vec3(-liquidStyle.lightAxis,-liquidStyle.normalZ,liquidStyle.lightAxis)))),liquidStyle.specPower);
    result=result*(1.0-rim*liquidStyle.shadow)+vec3(spec*rim*liquidStyle.specular);
    return mix(result,photo(v_uv),smoothstep(liquidStyle.complete,1.0,t));
  }
void main() {
    float t=u_progress;
    if(t<=0.0) { frag=texA(v_uv); return; }
    if(t>=1.0) { frag=texB(v_uv); return; }
    if(t<DRAW_END) {
        vec3 ink=inked(v_uv,drawing(v_uv).r*strokes(v_uv).r);
        frag=vec4(mix(texA(v_uv).rgb,ink,smoothstep(0.0,PAPER_START,t)),1);
        return;
    }
    t=clamp((t-DRAW_END)/(1.0-DRAW_END),0.0,1.0);
    frag=vec4(liquid(t),1);
}
