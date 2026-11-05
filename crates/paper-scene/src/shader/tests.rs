use super::*;

#[test]
fn precision_qualified_uniforms_compile() {
    let source = "uniform mediump float g_Alpha;\nuniform lowp vec3 g_Color;\nvoid main(){gl_FragColor=vec4(g_Color,g_Alpha);}";
    let translated = translate(source, Stage::Fragment, &BTreeMap::new());
    assert_eq!(translated.uniforms.len(), 2);
    assert!(compile(&translated.source, Stage::Fragment, "precision-uniforms").is_ok());
}

#[test]
fn geometry_stage_compiles() {
    let source = r"#version 450
layout(points) in;
layout(triangle_strip, max_vertices = 3) out;
layout(location = 0) in vec4 incoming[];
layout(location = 0) out vec4 outgoing;
void main() {
    outgoing = incoming[0];
    for (int index = 0; index < 3; ++index) {
        gl_Position = gl_in[0].gl_Position;
        EmitVertex();
    }
    EndPrimitive();
}";
    assert!(compile(source, Stage::Geometry, "geometry-stage").is_ok());
}

#[test]
fn particle_geometry_translates() {
    let source = r"in vec4 v_Color;
in vec4 gl_Position;
out vec4 v_Color;
out vec4 gl_Position;
[maxvertexcount(3)]
void main() {
    PS_INPUT vertex;
    vertex.v_Color = IN[0].v_Color;
    vertex.gl_Position = IN[0].gl_Position;
    OUT.Append(vertex);
    OUT.Append(vertex);
    OUT.Append(vertex);
}";
    let combos = BTreeMap::new();
    let (input, output) = geometry_interfaces(source);
    let input = varying_map("varying vec4 v_Color;", &input, &combos);
    let output = varying_map(&output, "varying vec4 v_Color;", &combos);
    let translated = translate_geometry(source, &combos, &input, &output).unwrap();
    assert!(
        compile(&translated.source, Stage::Geometry, "particle-geometry").is_ok(),
        "{}",
        translated.source
    );
}

#[test]
fn geometry_hlsl_scopes() {
    let source = r"in vec4 gl_Position;
out vec4 gl_Position;
out vec3 v_WorldPos;
uniform mat4 g_ModelMatrix;
void ComputeParticleTrailTangents(vec3 position, vec3 velocity, out vec3 right, out vec3 up) {
    right = position + velocity;
    up = position - velocity;
}
[maxvertexcount(3)]
void main() {
    vec3 position = mul(vec4(1.0), g_ModelMatrix);
    vec3 right, up;
    ComputeParticleTrailTangents(IN[0].gl_Position, position, right, up);
    PS_INPUT vertex;
    vertex.v_WorldPos = mul(vec4(position, 1.0), g_ModelMatrix);
    for (int index = 0; index < 3; ++index)
    {
        float index = 0.5;
        vertex.gl_Position = vec4(mix(position, vertex.v_WorldPos, index) + right + up, 1.0);
        OUT.Append(vertex);
    }
}";
    let combos = BTreeMap::new();
    let (input, output) = geometry_interfaces(source);
    let input = varying_map("", &input, &combos);
    let output = varying_map(&output, "varying vec3 v_WorldPos;", &combos);
    let translated = translate_geometry(source, &combos, &input, &output).unwrap();
    compile(&translated.source, Stage::Geometry, "geometry-scopes").unwrap();
}

#[test]
fn one_line_fragment_output() {
    let source = "varying vec3 v_Normal;\nvoid main(){gl_FragColor=vec4(v_Normal,1.0);}";
    let translated = translate(source, Stage::Fragment, &BTreeMap::new());
    assert!(translated.source.contains("#define gl_FragColor skwd_FragColor"));
    compile(&translated.source, Stage::Fragment, "one-line-fragment-output").unwrap();
}

#[test]
fn float_condition_compiles() {
    let source = "uniform float u_Mask;\nvoid main() { float mask = u_Mask; if(mask) gl_FragColor = vec4(1.0); else if (u_Mask > 0.0) gl_FragColor = vec4(0.5); else gl_FragColor = vec4(mix(vec2(0.0), vec2(1.0), u_Mask > 0.0), 0.0, 1.0); }";
    let translated = translate(source, Stage::Fragment, &BTreeMap::new());
    assert!(translated.source.contains("if(bool(mask))"));
    assert!(translated.source.contains("if (bool(u_Mask > 0.0))"));
    compile(&translated.source, Stage::Fragment, "float-condition").unwrap();
}

#[test]
fn compilation_sessions_preserve_shader_output_after_errors_and_nested_scopes() {
    let source = "#version 450\nlayout(location=0) out vec4 color; void main() { color=vec4(0.125,0.25,0.5,1.0); }";
    let expected = compile(source, Stage::Fragment, "session-control").unwrap();
    for _ in 0..2 {
        let _session = CompilationSession::new().unwrap();
        assert!(compile("invalid shader", Stage::Fragment, "session-error").is_err());
        {
            let _nested = CompilationSession::new().unwrap();
            let actual = compile(source, Stage::Fragment, "session-nested").unwrap();
            assert_eq!(actual, expected);
        }
        assert!(compile("invalid shader", Stage::Vertex, "session-error-after-nested").is_err());
        assert_eq!(compile(source, Stage::Fragment, "session-after-error").unwrap(), expected);
    }
    assert_eq!(compile(source, Stage::Fragment, "session-after-drop").unwrap(), expected);
}

#[test]
fn sampler_name_overlap() {
    let source = "uniform sampler2D g_Texture1; // {\"combo\":\"OPTION\"}\nuniform sampler2D g_Texture10; // {\"combo\":\"OPTION\"}\nvoid main() { gl_FragColor=texSample2D(g_Texture10,vec2(0.5)); }";
    let mut translated = translate(source, Stage::Fragment, &BTreeMap::new());
    assert_eq!(translated.samplers.len(), 2);
    omit_unused_samplers(&mut translated, &BTreeMap::new(), &[], None);
    assert_eq!(translated.samplers.iter().map(|sampler| sampler.index).collect::<Vec<_>>(), [10]);
    assert!(!translated.source.contains("uniform sampler2D g_Texture1;"));
}

#[test]
fn sampler_macro_use() {
    for body in [
        "#define TEXTURE g_Texture1\nvoid main() { gl_FragColor=texSample2D(TEXTURE,vec2(0.5)); }",
        "#define TEXTURE(n) g_Texture##n\nvoid main() { gl_FragColor=texSample2D(TEXTURE(1),vec2(0.5)); }",
    ] {
        let source = format!("uniform sampler2D g_Texture1; // {{\"combo\":\"OPTION\"}}\n{body}");
        let mut translated = translate(&source, Stage::Fragment, &BTreeMap::new());
        omit_unused_samplers(&mut translated, &BTreeMap::new(), &[], None);
        assert_eq!(translated.samplers.len(), 1);
    }
}

#[test]
fn sampler_probe_metadata() {
    let source =
        "uniform sampler2D g_Texture1; // {\"combo\":\"OPTION\",\"default\":\"b\"}\nvoid main() {}";
    let probe = translate(source, Stage::Fragment, &BTreeMap::new());
    assert_eq!(probe.samplers[0].combo.as_deref(), Some("OPTION"));
    assert_eq!(probe.samplers[0].default.as_deref(), Some("b"));
    let mut final_pass = translate(source, Stage::Fragment, &BTreeMap::new());
    omit_unused_samplers(&mut final_pass, &BTreeMap::new(), &[], None);
    assert!(final_pass.samplers.is_empty());
    assert_eq!(probe.samplers.len(), 1);
}

#[test]
fn hlsl_sampler_use() {
    for use_expr in ["g_Texture1", "TEXTURE(1)"] {
        let source = format!(
            "uniform sampler2D g_Texture1; // {{\"combo\":\"OPTION\"}}\n#define TEXTURE(n) g_Texture##n\n#if HLSL\nvoid main() {{ gl_FragColor=texSample2D({use_expr},vec2(0.5)); }}\n#else\nvoid main() {{ gl_FragColor=vec4(1.0); }}\n#endif"
        );
        let glsl = preprocess(&source, &BTreeMap::from([("GLSL".into(), 1)]));
        let hlsl = preprocess(&source, &BTreeMap::from([("HLSL".into(), 1)]));
        let mut translated = translate(&glsl, Stage::Fragment, &BTreeMap::new());
        omit_unused_samplers(&mut translated, &BTreeMap::new(), &[], Some(&hlsl));
        assert_eq!(translated.samplers.len(), 1);
    }
}

#[test]
fn sampler_zero_retained() {
    let source = "uniform sampler2D g_Texture0; // {\"combo\":\"OPTION\"}\nvoid main() {}";
    let mut translated = translate(source, Stage::Fragment, &BTreeMap::new());
    omit_unused_samplers(&mut translated, &BTreeMap::new(), &[], None);
    assert_eq!(translated.samplers[0].index, 0);
}

#[test]
fn rectangular_padded_array() {
    let source =
        "uniform float before;\nuniform mat4x3 palette[2];\nuniform float after;\nvoid main() {}";
    let mut vertex = translate(source, Stage::Vertex, &BTreeMap::new());
    let mut fragment = translate("void main() {}", Stage::Fragment, &BTreeMap::new());
    unify_uniforms(&mut vertex, &mut fragment);
    assert_eq!(vertex.uniforms[1].kind, UniformKind::Mat4x3);
    assert_eq!(vertex.uniforms[1].count, 2);
    assert_eq!(vertex.uniforms[1].std140_span(), 128);
    for translated in [&vertex, &fragment] {
        assert_eq!(
            translated.uniforms.iter().map(|uniform| uniform.offset).collect::<Vec<_>>(),
            [0, 16, 144]
        );
        assert!(translated.source.contains("mat4x3 palette[2];"));
    }
    let palette = vec![
        1.0, 2.0, 3.0, 91.0, 4.0, 5.0, 6.0, 92.0, 7.0, 8.0, 9.0, 93.0, 10.0, 11.0, 12.0, 94.0,
        21.0, 22.0, 23.0, 95.0, 24.0, 25.0, 26.0, 96.0, 27.0, 28.0, 29.0, 97.0, 30.0, 31.0, 32.0,
        98.0,
    ];
    let values = BTreeMap::from([
        ("before".into(), vec![3.0]),
        ("palette[2]".into(), palette.clone()),
        ("after".into(), vec![5.0]),
        ("palette".into(), vec![-1.0; 32]),
    ]);
    let bytes = pack_uniforms(&vertex.uniforms, &values);
    assert_eq!(bytes.len(), 160);
    let lanes: Vec<_> =
        bytes.chunks_exact(4).map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap())).collect();
    assert_eq!(lanes[0], 3.0);
    assert_eq!(&lanes[4..36], palette.as_slice());
    assert_eq!(lanes[36], 5.0);
}

#[test]
fn symbolic_palette_layout() {
    let source = "uniform mat4x3 palette[ELEMENTS];\nuniform float tail;\nvoid main() {}";
    let combos = BTreeMap::from([("ELEMENTS".into(), 2)]);
    let translated = translate(source, Stage::Vertex, &combos);
    assert_eq!(translated.uniforms[0].count, 2);
    assert_eq!(translated.uniforms[0].std140_span(), 128);
    assert_eq!(translated.uniforms[1].offset, 128);
    let mut palette = vec![1.0; 16];
    palette.extend([2.0; 16]);
    let values =
        BTreeMap::from([("palette[ELEMENTS]".into(), palette), ("tail".into(), vec![7.0])]);
    let bytes = pack_uniforms(&translated.uniforms, &values);
    assert_eq!(bytes.len(), 144);
    assert_eq!(&bytes[64..68], &2.0_f32.to_le_bytes());
    assert_eq!(&bytes[128..132], &7.0_f32.to_le_bytes());
    let unresolved = translate(source, Stage::Vertex, &BTreeMap::new());
    assert_eq!(unresolved.uniforms[0].count, 1);
    assert!(unresolved.source.contains("#error unresolved uniform array size"));
}

#[test]
fn matrix_array_packing() {
    for (declaration, lanes) in [("mat3", 12), ("mat4", 16), ("mat4x3", 16)] {
        let source = format!(
            "uniform {declaration} transforms[2];\nuniform float sentinel;\nvoid main() {{}}"
        );
        let translated = translate(&source, Stage::Vertex, &BTreeMap::new());
        let matrices: Vec<_> = (1..=lanes * 2).map(|lane| lane as f32).collect();
        let values = BTreeMap::from([
            ("transforms[2]".into(), matrices.clone()),
            ("sentinel".into(), vec![123.0]),
        ]);
        let bytes = pack_uniforms(&translated.uniforms, &values);
        let actual: Vec<_> = bytes
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(&actual[..lanes * 2], matrices.as_slice());
        assert_eq!(actual[lanes * 2], 123.0);
    }
}

#[test]
fn rectangular_scalar_not_splatted() {
    let translated =
        translate("uniform mat4x3 palette;\nvoid main() {}", Stage::Vertex, &BTreeMap::new());
    let bytes =
        pack_uniforms(&translated.uniforms, &BTreeMap::from([("palette".into(), vec![7.0])]));
    assert_eq!(&bytes[..4], &7.0_f32.to_le_bytes());
    assert!(bytes[4..].iter().all(|byte| *byte == 0));
    assert_eq!(pack_uniforms(&translated.uniforms, &BTreeMap::new()), vec![0; 64]);
}

#[test]
fn material_bindings_parse_only() {
    let source = r#"uniform sampler2D g_Texture0;
uniform float u_probeLevel; // {"material":"probeLevel","default":0.25}
uniform vec3 g_Color; // {"material":"color","default":"1 1 1"}
void main(){gl_FragColor=texture2D(g_Texture0,vec2(0.0))*vec4(u_probeLevel);}"#;
    let names = material_bindings(source);
    assert_eq!(names.len(), 2);
    assert_eq!(names["probeLevel"].uniform, "u_probeLevel");
    assert_eq!(names["probeLevel"].value, Some(vec![0.25]));
    assert_eq!(names["color"].uniform, "g_Color");
    assert_eq!(names["color"].value, Some(vec![1.0; 3]));
    let translated = translate(source, Stage::Fragment, &BTreeMap::new());
    for uniform in translated.uniforms.iter().filter(|uniform| uniform.material.is_some()) {
        assert_eq!(names[uniform.material.as_ref().unwrap()].uniform, uniform.name);
    }
}

#[test]
fn singleton_array_layout() {
    for (kind, values) in [("float", vec![3.0]), ("vec2", vec![3.0, 4.0])] {
        let source = format!(
            "uniform float before;\nuniform {kind} values[1];\nuniform float tail;\nvoid main() {{}}"
        );
        let translated = translate(&source, Stage::Vertex, &BTreeMap::new());
        assert_eq!(
            translated.uniforms.iter().map(|uniform| uniform.offset).collect::<Vec<_>>(),
            [0, 16, 32]
        );
        assert_eq!(translated.uniforms[1].std140_align(), 16);
        assert_eq!(translated.uniforms[1].std140_span(), 16);
        let bytes = pack_uniforms(
            &translated.uniforms,
            &BTreeMap::from([
                ("before".into(), vec![2.0]),
                ("values[1]".into(), values.clone()),
                ("tail".into(), vec![5.0]),
            ]),
        );
        assert_eq!(bytes.len(), 48);
        assert_eq!(&bytes[16..20], &3.0_f32.to_le_bytes());
        if values.len() == 2 {
            assert_eq!(&bytes[20..24], &4.0_f32.to_le_bytes());
        }
        assert_eq!(&bytes[32..36], &5.0_f32.to_le_bytes());
    }
}

#[test]
fn local_macro_layout() {
    let source = "#define SIZE 2\n#define N SIZE\nuniform float before;\nuniform mat4x3 bones[N];\nuniform float tail;\nvoid main() {}";
    let translated = translate(source, Stage::Vertex, &BTreeMap::new());
    assert_eq!(translated.uniforms[1].count, 2);
    assert_eq!(translated.uniforms[1].std140_span(), 128);
    assert_eq!(translated.uniforms[2].offset, 144);
    assert!(translated.source.contains("mat4x3 bones[2];"));
    let mut bones = vec![1.0; 16];
    bones.extend([2.0; 16]);
    let bytes = pack_uniforms(
        &translated.uniforms,
        &BTreeMap::from([("bones[N]".into(), bones), ("tail".into(), vec![7.0])]),
    );
    assert_eq!(&bytes[80..84], &2.0_f32.to_le_bytes());
    assert_eq!(&bytes[144..148], &7.0_f32.to_le_bytes());
}

#[test]
fn macro_redefinition_layout() {
    let source = "#define N 1\n#undef N\n#define N 2\nuniform float values[N];\nvoid main() {}";
    let combos = BTreeMap::from([("N".into(), 3)]);
    let translated = translate(source, Stage::Vertex, &combos);
    assert_eq!(translated.uniforms[0].count, 2);
    assert!(translated.source.contains("float values[2];"));
    assert!(translated.source.contains("#undef N\n#define N 1"));
    let unresolved =
        translate("#undef N\nuniform float values[N];\nvoid main() {}", Stage::Vertex, &combos);
    assert!(unresolved.source.contains("#error unresolved uniform array size"));
}

#[test]
fn conditional_macro_layout() {
    let source = "#if MODE\n#define N 2\n#else\n#define N 3\n#endif\nuniform mat4x3 bones[N];\nvoid main() {}";
    let combos = BTreeMap::from([("MODE".into(), 1)]);
    let active = preprocess(source, &combos);
    let translated = translate(&active, Stage::Vertex, &combos);
    assert_eq!(translated.uniforms[0].count, 2);
    assert!(translated.source.contains("mat4x3 bones[2];"));
}

#[test]
fn authored_mod_compiles() {
    let vertex = "attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}";
    let fragment = "float mod(float x, float y) { return x-y*floor(x/y); }\nvoid main(){float value=mod(-1.0,2.0);gl_FragColor=vec4(value,value,value,1.0);}";
    let combos = BTreeMap::new();
    let translated = translate(fragment, Stage::Fragment, &combos);
    compile(&translated.source, Stage::Fragment, "authored mod").unwrap();
    let pair = crate::hlsl::rewrite(vertex, fragment, &combos, &[], &[]);
    crate::hlsl::compile(&pair.fragment, Stage::Fragment, "authored mod").unwrap();
    let builtin = "void main(){gl_FragColor=vec4(mod(3.0,2.0));}";
    assert_eq!(rename_authored_mod(builtin), builtin);
}

#[test]
fn compact_conditionals() {
    let source = "#define PICK 1\n#if(PICK == 1)\nfloat selected = 1.0;\n#elif(PICK == 2)\nfloat selected = 2.0;\n#else\nfloat selected = 3.0;\n#endif";
    let translated = preprocess(source, &BTreeMap::new());
    assert!(translated.contains("selected = 1.0"));
    assert!(!translated.contains("selected = 2.0"));
    assert!(!translated.contains("selected = 3.0"));
    assert!(!translated.contains("#if"));
}

#[test]
fn audio_array_alias() {
    let vertex = "attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}";
    let fragment = "uniform float g_AudioSpectrum32Left[32];\nvoid main(){\nfloat bins[] = g_AudioSpectrum32Left;\ngl_FragColor=vec4(bins[0],bins[1],0.0,1.0);\n}";
    let combos = BTreeMap::new();
    let translated = translate(fragment, Stage::Fragment, &combos);
    let pair = crate::hlsl::rewrite(vertex, fragment, &combos, &translated.uniforms, &[]);
    crate::hlsl::compile(&pair.fragment, Stage::Fragment, "array alias").unwrap();
}

#[test]
fn unsigned_uniform_array() {
    let translated = translate(
        "uniform uint g_Offsets[2];\nattribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position+float(g_Offsets[CASTU(0.0)]),1.0);}",
        Stage::Vertex,
        &BTreeMap::new(),
    );
    assert_eq!(translated.uniforms[0].kind, UniformKind::Uint);
    let bytes = pack_uniforms(
        &translated.uniforms,
        &BTreeMap::from([("g_Offsets[2]".into(), vec![1.0, 13.0])]),
    );
    assert_eq!(&bytes[..4], &1_u32.to_le_bytes());
    assert_eq!(&bytes[16..20], &13_u32.to_le_bytes());
    compile(&translated.source, Stage::Vertex, "uint-array").unwrap();
}

#[test]
fn comparison_sampler_compiles() {
    let vertex = "attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}";
    let fragment = "uniform sampler2DComparison g_Texture6;\nvoid main(){float value=texSample2DCompare(g_Texture6,vec2(0.5,0.5),0.25).r;gl_FragColor=vec4(value,value,value,1.0);}";
    let combos = BTreeMap::new();
    let translated = translate(fragment, Stage::Fragment, &combos);
    assert!(translated.samplers[0].comparison);
    compile(&translated.source, Stage::Fragment, "comparison sampler").unwrap();
    let pair =
        crate::hlsl::rewrite(vertex, fragment, &combos, &translated.uniforms, &translated.samplers);
    crate::hlsl::compile(&pair.fragment, Stage::Fragment, "comparison sampler").unwrap();
}

#[test]
fn sampler_component_order() {
    let source = "uniform sampler2D g_Texture2; // {\"components\":[{\"combo\":\"R\"},{},{\"combo\":\"B\"}]}\nvoid main() {}";
    let pass = translate(source, Stage::Fragment, &BTreeMap::new());
    assert_eq!(pass.samplers[0].clone().components, [Some("R".into()), None, Some("B".into())]);
}

#[test]
fn sampler_parameters_compile() {
    let vertex = "attribute vec3 a_Position;\nvoid main(){gl_Position=vec4(a_Position,1.0);}";
    let fragment = "uniform sampler2D g_Texture0;\nvec3 fetch(DECLARE_SAMPLER2D_PARAMETER(source), vec2 uv) { return texSample2DLod(source, uv, 0.0).rgb; }\nvoid main(){gl_FragColor=vec4(fetch(MAKE_SAMPLER2D_ARGUMENT(g_Texture0), vec2(0.5, 0.5)),1.0);}";
    let combos = BTreeMap::new();
    let translated = translate(fragment, Stage::Fragment, &combos);
    compile(&translated.source, Stage::Fragment, "sampler parameters").unwrap();
    let pair =
        crate::hlsl::rewrite(vertex, fragment, &combos, &translated.uniforms, &translated.samplers);
    crate::hlsl::compile(&pair.fragment, Stage::Fragment, "sampler parameters").unwrap();
}

#[test]
fn hlsl_redefined_macro() {
    let source = "uniform sampler2D g_Texture1; // {\"combo\":\"NORMALMAP\"}\nvoid main() { gl_FragColor=vec4(1.0); }";
    let hlsl = "#define mod fmod\nvoid main() { gl_FragColor=vec4(mod(1.0,2.0)); }";
    let mut translated = translate(source, Stage::Fragment, &BTreeMap::new());
    omit_unused_samplers(&mut translated, &BTreeMap::new(), &[], Some(hlsl));
    assert!(translated.samplers.is_empty());
}

#[test]
fn sampler_read_detection() {
    let source = "uniform sampler2D g_Texture1; // {\"combo\":\"MASK\",\"default\":\"util/white\"}\n#if MASK\nvec4 kept = texSample2D(g_Texture1, v_TexCoord);\n#endif\n// g_Texture1\nvoid main() {}";
    let unguarded = "uniform sampler2D g_Texture1; // {\"combo\":\"MASK\",\"default\":\"util/white\"}\nvoid main() { float mask = texSample2D(g_Texture1, v_TexCoord).r; }";
    let mut combos = BTreeMap::new();
    combos.insert("MASK".to_string(), 0);
    assert!(!reads_sampler(source, &combos, "g_Texture1"));
    assert!(reads_sampler(unguarded, &combos, "g_Texture1"));
    combos.insert("MASK".to_string(), 1);
    assert!(reads_sampler(source, &combos, "g_Texture1"));
}

#[test]
fn pruning_reuses_preprocess() {
    let first = "#version 450\nvoid main() { float first_word = 1.0; }\n";
    let second = "#version 450\nvoid main() { float second_word = 1.0; }\n";
    let words = prune_words(first, shaderc::SourceLanguage::GLSL).unwrap();
    assert!(words.iter().any(|word| word == "first_word"));
    let other = prune_words(second, shaderc::SourceLanguage::GLSL).unwrap();
    assert!(other.iter().any(|word| word == "second_word"));
    assert!(!other.iter().any(|word| word == "first_word"));
    let again = prune_words(first, shaderc::SourceLanguage::GLSL).unwrap();
    assert!(std::rc::Rc::ptr_eq(&words, &again));
}
