use super::{HlslPair, rewrite};
use crate::shader::{Stage, translate_with, unify_uniforms, varying_map};
use std::collections::BTreeMap;

fn pair() -> HlslPair {
    let vert = "uniform mat4 g_ModelViewProjectionMatrix;\nattribute vec3 a_Position;\nattribute vec2 a_TexCoord;\nvarying vec2 v_TexCoord;\nvarying vec4 v_Extra;\nconst float scale = 2.0;\nvoid main() {\n\tgl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);\n\tv_TexCoord = a_TexCoord * scale;\n\tif (a_TexCoord.x > 2.0) return;\n\tv_Extra = CAST4(1.0);\n}\n";
    let frag = "uniform sampler2D g_Texture0; // {\"hidden\":true}\nuniform float g_Strength; // {\"material\":\"strength\",\"default\":1}\nvarying vec2 v_TexCoord;\nvoid main() {\n\tvec4 albedo = texSample2D(g_Texture0, v_TexCoord);\n\tgl_FragColor = albedo * g_Strength + v_Extra;\n}\n";
    let combos = BTreeMap::from([("BLENDMODE".to_string(), 9i64)]);
    let map = varying_map(vert, frag, &BTreeMap::default());
    let mut vertex = translate_with(vert, Stage::Vertex, &combos, Some(&map));
    let mut fragment = translate_with(frag, Stage::Fragment, &combos, Some(&map));
    unify_uniforms(&mut vertex, &mut fragment);
    rewrite(vert, frag, &combos, &vertex.uniforms, &fragment.samplers)
}

#[test]
fn rewrite_follows_the_engine_structs_and_prefixes() {
    let pair = pair();
    let v = &pair.vertex;
    assert!(v.starts_with("#define HLSL 1\n#define HLSL_SM40 1\n"), "{v}");
    assert!(v.contains("#define BLENDMODE 9"));
    assert!(
        v.contains("cbuffer SkwdParams : register(b0) {\n\tfloat4x4 g_ModelViewProjectionMatrix;\n\tfloat g_Strength;\n};"),
        "{v}"
    );
    assert!(v.contains(
        "Texture2D g_Texture0 : register(t0); SamplerState g_Texture0SamplerState : register(s0);"
    ));
    assert!(v.contains("[[vk::location(0)]] vec3 a_Position : POSITION;"));
    assert!(v.contains("[[vk::location(1)]] vec2 a_TexCoord : TEXCOORD0;"));
    assert!(
        v.contains("struct VS_OUTPUT {\n\tfloat4 gl_Position : SV_POSITION;\n\tvec2 v_TexCoord : TEXCOORD0;\n\tvec4 v_Extra : TEXCOORD1;\n};"),
        "{v}"
    );
    assert!(v.contains("static const float scale = 2.0;"));
    assert!(v.contains("VS_OUTPUT main(VS_INPUT IN) {\n\tVS_OUTPUT OUT = (VS_OUTPUT)0;"));
    assert!(
        v.contains("OUT.gl_Position = mul(vec4(IN.a_Position, 1.0), g_ModelViewProjectionMatrix);")
    );
    assert!(v.contains("OUT.v_TexCoord = IN.a_TexCoord * scale;"));
    assert!(v.contains("if (IN.a_TexCoord.x > 2.0) return OUT;"));
    assert!(v.trim_end().ends_with("return OUT;\n}"), "{v}");
    let f = &pair.fragment;
    assert!(f.contains("PS_OUTPUT main(VS_OUTPUT IN) {\n\tPS_OUTPUT OUT = (PS_OUTPUT)0;"));
    assert!(f.contains("texSample2D(g_Texture0, IN.v_TexCoord)"));
    assert!(f.contains("OUT.gl_FragColor = albedo * g_Strength + IN.v_Extra;"));
    assert!(!f.contains("uniform "));
}

#[test]
fn dxc_compiles_the_pair_when_available() {
    if !super::available() {
        eprintln!("dxc unavailable, skipping");
        return;
    }
    let pair = pair();
    let vert = super::compile(&pair.vertex, Stage::Vertex, "probe.vert").unwrap();
    let frag = super::compile(&pair.fragment, Stage::Fragment, "probe.frag").unwrap();
    assert_eq!(vert[0], 0x0723_0203);
    assert_eq!(frag[0], 0x0723_0203);
}

#[test]
fn degenerate_declarations_are_ignored_not_looped() {
    let vert = "attribute vec3 ;\nvarying vec2 [2];\nvarying vec2 v_Ok;\nvoid main() { v_Ok = vec2(0.0); gl_Position = vec4(0.0); }\n";
    let frag = "void main() { gl_FragColor = vec4(v_Ok, 0.0, 1.0); }\n";
    let pair = rewrite(vert, frag, &BTreeMap::new(), &[], &[]);
    assert!(pair.vertex.contains("OUT.v_Ok = vec2(0.0);"));
    assert!(!pair.vertex.contains("IN.IN."));
    assert!(pair.vertex.len() < 4096, "{}", pair.vertex.len());
}

#[test]
fn non_ascii_comments_survive_every_replacement_unchanged() {
    use std::fmt::Write;
    let mut vert = String::from("// 自动摇摆 注释 sample ✓\nattribute vec3 a_Position;\n");
    for i in 0..48 {
        let _ = writeln!(vert, "varying float v_N{i};");
    }
    vert.push_str("void main() { gl_Position = vec4(a_Position, 1.0); v_N0 = 1.0; }\n");
    let frag = "void main() { gl_FragColor = vec4(v_N0); }\n";
    let pair = rewrite(&vert, frag, &BTreeMap::new(), &[], &[]);
    assert!(pair.vertex.contains("// 自动摇摆 注释 sample ✓"), "{}", pair.vertex.len());
    assert!(pair.vertex.len() < vert.len() + 4096, "{}", pair.vertex.len());
}

#[test]
fn uniform_arrays_keep_a_single_dimension_in_the_cbuffer() {
    let vert = "uniform mat4 g_ModelViewProjectionMatrix;\nuniform float g_AudioSpectrum16Left[16];\nuniform vec4 g_Bounds[3];\nattribute vec3 a_Position;\nvarying float v_Level;\nvoid main() {\n\tgl_Position = mul(vec4(a_Position, 1.0), g_ModelViewProjectionMatrix);\n\tv_Level = g_AudioSpectrum16Left[3] + g_Bounds[1].x;\n}\n";
    let frag = "varying float v_Level;\nvoid main() {\n\tgl_FragColor = CAST4(v_Level);\n}\n";
    let combos = BTreeMap::new();
    let map = varying_map(vert, frag, &BTreeMap::default());
    let mut vertex = translate_with(vert, Stage::Vertex, &combos, Some(&map));
    let mut fragment = translate_with(frag, Stage::Fragment, &combos, Some(&map));
    unify_uniforms(&mut vertex, &mut fragment);
    let pair = rewrite(vert, frag, &combos, &vertex.uniforms, &fragment.samplers);
    assert!(
        pair.vertex.contains("\tfloat g_AudioSpectrum16Left[16];\n\tfloat4 g_Bounds[3];\n"),
        "{}",
        pair.vertex
    );
    assert!(!pair.vertex.contains("[16][16]"), "{}", pair.vertex);
    assert!(pair.vertex.contains("g_AudioSpectrum16Left[3] + g_Bounds[1].x"), "{}", pair.vertex);
}

#[test]
fn only_depth_zero_consts_become_static() {
    let body = "  const float a = 1.0; // {\"x\": 1}\nfloat f(float v) {\nconst float b = v * 2.0;\n\treturn b;\n}\n#if 1\nconst vec2 c = vec2(1.0, 2.0);\n#endif\n";
    let out = super::static_const(body);
    assert!(out.contains("  static const float a = 1.0;"), "{out}");
    assert!(out.contains("\nconst float b = v * 2.0;\n"), "{out}");
    assert!(out.contains("\nstatic const vec2 c = vec2(1.0, 2.0);"), "{out}");
}

#[test]
fn globals_buffer_is_rejected_when_dxc_is_available() {
    if !super::available() {
        return;
    }
    let src = "const float gk = 2.0;\nstruct PS_OUTPUT { float4 gl_FragColor : SV_Target0; };\nPS_OUTPUT main() { PS_OUTPUT OUT; OUT.gl_FragColor = float4(gk, 0.0, 0.0, 1.0); return OUT; }\n";
    let err = super::compile(src, Stage::Fragment, "globals-test").expect_err("non-static global");
    assert!(format!("{err:#}").contains("$Globals"), "{err:#}");
    let fixed = src.replacen("const float gk", "static const float gk", 1);
    super::compile(&fixed, Stage::Fragment, "globals-test-static").expect("static global compiles");
}

fn rewrite_pair(vert: &str, frag: &str) -> HlslPair {
    let combos = BTreeMap::new();
    let map = varying_map(vert, frag, &BTreeMap::default());
    let mut vertex = translate_with(vert, Stage::Vertex, &combos, Some(&map));
    let mut fragment = translate_with(frag, Stage::Fragment, &combos, Some(&map));
    unify_uniforms(&mut vertex, &mut fragment);
    rewrite(vert, frag, &combos, &vertex.uniforms, &fragment.samplers)
}

#[test]
fn fragment_only_varyings_stay_locals_and_the_vertex_list_defines_the_struct() {
    let vert = "attribute vec3 a_Position;\nvarying vec2 v_TexCoord;\nvoid main() {\n\tgl_Position = vec4(a_Position, 1.0);\n\tv_TexCoord = a_Position.xy;\n}\n";
    let frag = "varying vec2 v_TexCoord;\nvarying float timer;\nvoid main() {\n\tfloat timer = v_TexCoord.x;\n\tgl_FragColor = CAST4(timer);\n}\n";
    let pair = rewrite_pair(vert, frag);
    let expected = "struct VS_OUTPUT {\n\tfloat4 gl_Position : SV_POSITION;\n\tvec2 v_TexCoord : TEXCOORD0;\n};";
    assert!(pair.vertex.contains(expected), "{}", pair.vertex);
    assert!(pair.fragment.contains(expected), "{}", pair.fragment);
    assert!(!pair.fragment.contains("timer : TEXCOORD"), "{}", pair.fragment);
    assert!(pair.fragment.contains("float timer = IN.v_TexCoord.x;"), "{}", pair.fragment);
    assert!(pair.fragment.contains("CAST4(timer)"), "{}", pair.fragment);
    assert!(!pair.fragment.contains("IN.timer"), "{}", pair.fragment);
}

#[test]
fn vertex_declared_type_wins_over_the_fragment_redeclaration() {
    let vert = "attribute vec3 a_Position;\nvarying vec4 v_Color;\nvoid main() {\n\tgl_Position = vec4(a_Position, 1.0);\n\tv_Color = CAST4(1.0);\n}\n";
    let frag =
        "varying vec3 v_Color;\nvoid main() {\n\tgl_FragColor = vec4(v_Color.xyz, 1.0);\n}\n";
    let pair = rewrite_pair(vert, frag);
    assert!(pair.fragment.contains("\tvec4 v_Color : TEXCOORD0;"), "{}", pair.fragment);
    assert!(!pair.fragment.contains("vec3 v_Color"), "{}", pair.fragment);
    assert!(pair.fragment.contains("IN.v_Color.xyz"), "{}", pair.fragment);
}

#[test]
fn dxc_accepts_a_fragment_local_that_shadows_a_fragment_only_varying() {
    if !super::available() {
        return;
    }
    let vert = "attribute vec3 a_Position;\nvarying vec2 v_TexCoord;\nvoid main() {\n\tgl_Position = vec4(a_Position, 1.0);\n\tv_TexCoord = a_Position.xy;\n}\n";
    let frag = "varying vec2 v_TexCoord;\nvarying float timer;\nvoid main() {\n\tfloat timer = v_TexCoord.x;\n\tgl_FragColor = CAST4(timer);\n}\n";
    let pair = rewrite_pair(vert, frag);
    super::compile(&pair.vertex, Stage::Vertex, "sd4-v").expect("vertex compiles");
    super::compile(&pair.fragment, Stage::Fragment, "sd4-f").expect("fragment compiles");
}

fn member_offsets(words: &[u32]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut at = 5;
    while at < words.len() {
        let count = (words[at] >> 16) as usize;
        if count == 0 {
            break;
        }
        if words[at] & 0xffff == 72 && words[at + 3] == 35 {
            out.push((words[at + 2], words[at + 4]));
        }
        at += count;
    }
    out
}

#[test]
fn dxc_gl_layout_matches_std140_array_stride_and_tail() {
    if !super::available() {
        return;
    }
    let src = "cbuffer SkwdParams : register(b0) { float a; float arr[64]; float b; float2 c; float3 d; float e; };\nstruct PS_OUTPUT { float4 gl_FragColor : SV_Target0; };\nPS_OUTPUT main() { PS_OUTPUT OUT; OUT.gl_FragColor = float4(a + arr[3] + b + c.x + d.y + e, 0.0, 0.0, 1.0); return OUT; }\n";
    let words = super::compile(src, Stage::Fragment, "layout-test").expect("compiles");
    let offsets = member_offsets(&words);
    assert_eq!(
        offsets,
        vec![(0, 0), (1, 16), (2, 1040), (3, 1048), (4, 1056), (5, 1068)],
        "{offsets:?}"
    );
}

#[test]
fn array_varyings_are_copied_to_locals_in_the_fragment() {
    let vert = "attribute vec3 a_Position;\nvarying vec2 v_TexCoord;\nvarying float audioValue[4];\nvoid main() {\n\tgl_Position = vec4(a_Position, 1.0);\n\tv_TexCoord = a_Position.xy;\n\tfor (int i = 0; i < 4; i++) { audioValue[i] = float(i); }\n}\n";
    let frag = "varying vec2 v_TexCoord;\nvarying float audioValue[4];\nvoid main() {\n\tfloat sum = 0.0;\n\tfor (int i = 0; i < 4; i++) { sum += audioValue[i]; }\n\tgl_FragColor = vec4(v_TexCoord, sum, 1.0);\n}\n";
    let pair = rewrite_pair(vert, frag);
    assert!(pair.vertex.contains("\tfloat audioValue[4] : TEXCOORD1;"), "{}", pair.vertex);
    assert!(pair.vertex.contains("OUT.audioValue[i] = float(i);"), "{}", pair.vertex);
    assert!(
        pair.fragment
            .contains("PS_OUTPUT OUT = (PS_OUTPUT)0;\n\tfloat audioValue[4] = IN.audioValue;\n"),
        "{}",
        pair.fragment
    );
    assert!(pair.fragment.contains("sum += audioValue[i];"), "{}", pair.fragment);
    assert!(!pair.fragment.contains("IN.audioValue[i]"), "{}", pair.fragment);
    assert!(pair.fragment.contains("IN.v_TexCoord"), "{}", pair.fragment);
}

#[test]
fn long_scalar_varying_arrays_are_packed_into_vec4_slots() {
    let vert = "attribute vec3 a_Position;\nvarying vec2 v_TexCoord;\nvarying float audioValue[6];\nvoid main() {\n\tgl_Position = vec4(a_Position, 1.0);\n\tv_TexCoord = a_Position.xy;\n\tfor (int i = 0; i < 6; i++) { audioValue[i] = float(i); }\n}\n";
    let frag = "varying vec2 v_TexCoord;\nvarying float audioValue[6];\nvoid main() {\n\tfloat sum = 0.0;\n\tfor (int i = 0; i < 6; i++) { sum += audioValue[i] * audioValue[i + 1]; }\n\tgl_FragColor = vec4(v_TexCoord, sum, 1.0);\n}\n";
    let pair = rewrite_pair(vert, frag);
    assert!(pair.vertex.contains("\tfloat4 audioValue[2] : TEXCOORD1;"), "{}", pair.vertex);
    assert!(
        pair.vertex.contains("OUT.audioValue[int(i) / 4][int(i) % 4] = float(i);"),
        "{}",
        pair.vertex
    );
    assert!(
        pair.fragment.contains("\tfloat4 audioValue[2] = IN.audioValue;\n"),
        "{}",
        pair.fragment
    );
    assert!(
        pair.fragment.contains(
            "audioValue[int(i) / 4][int(i) % 4] * audioValue[int(i + 1) / 4][int(i + 1) % 4]"
        ),
        "{}",
        pair.fragment
    );
    if super::available() {
        super::compile(&pair.vertex, Stage::Vertex, "pack-v").expect("vertex compiles");
        super::compile(&pair.fragment, Stage::Fragment, "pack-f").expect("fragment compiles");
    }
}

#[test]
fn cpu_std140_offsets_match_dxc_after_a_vec3() {
    let vert = "attribute vec3 a_Position;\nvoid main() { gl_Position = vec4(a_Position, 1.0); }\n";
    let frag = "uniform vec3 u_Color;\nuniform float g_Time;\nuniform vec2 g_PointerPosition;\nuniform float u_Speed;\nuniform float g_Bands[3];\nuniform float u_Radius;\nvoid main() { gl_FragColor = vec4(u_Color * g_Time + g_PointerPosition.x + u_Speed + g_Bands[1] + u_Radius, 1.0); }\n";
    let combos = BTreeMap::new();
    let map = varying_map(vert, frag, &BTreeMap::default());
    let mut vertex = translate_with(vert, Stage::Vertex, &combos, Some(&map));
    let mut fragment = translate_with(frag, Stage::Fragment, &combos, Some(&map));
    unify_uniforms(&mut vertex, &mut fragment);
    let cpu: Vec<(String, usize)> =
        fragment.uniforms.iter().map(|u| (u.name.clone(), u.offset)).collect();
    assert_eq!(
        cpu,
        vec![
            ("u_Color".to_string(), 0),
            ("g_Time".to_string(), 12),
            ("g_PointerPosition".to_string(), 16),
            ("u_Speed".to_string(), 24),
            ("g_Bands[3]".to_string(), 32),
            ("u_Radius".to_string(), 80),
        ]
    );
    assert_eq!(fragment.block_size, 96);
    if !super::available() {
        return;
    }
    let pair = rewrite(vert, frag, &combos, &fragment.uniforms, &fragment.samplers);
    let words = super::compile(&pair.fragment, Stage::Fragment, "std140-f").expect("compiles");
    let dxc: Vec<u32> = member_offsets(&words).into_iter().map(|(_, offset)| offset).collect();
    assert_eq!(dxc, vec![0, 12, 16, 24, 32, 80], "{dxc:?}");
}

#[test]
fn dynamically_indexed_varying_arrays_keep_the_fragment_on_glsl() {
    let dynamic = "varying vec2 v_TexCoord;\nvarying float audioValue[32];\nvoid main() {\n\tfloat s = 0.0;\n\tfor (int i = 0; i < 32; i++) { s += audioValue[i]; }\n\tgl_FragColor = vec4(s);\n}\n";
    let constant = "varying vec2 v_TexCoord;\nvarying float audioValue[32];\nvoid main() {\n\tgl_FragColor = vec4(audioValue[3] + audioValue[31]);\n}\n";
    let plain = "varying vec2 v_TexCoord;\nvoid main() {\n\tgl_FragColor = vec4(v_TexCoord, 0.0, 1.0);\n}\n";
    let vectors = "varying vec4 v_Taps[8];\nvoid main() {\n\tvec4 s = vec4(0.0);\n\tfor (int i = 0; i < 8; i++) { s += v_Taps[i]; }\n\tgl_FragColor = s;\n}\n";
    assert!(super::fragment_indexes_varying_arrays_dynamically(dynamic));
    assert!(!super::fragment_indexes_varying_arrays_dynamically(constant));
    assert!(!super::fragment_indexes_varying_arrays_dynamically(plain));
    assert!(!super::fragment_indexes_varying_arrays_dynamically(vectors));
}
