use super::*;

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
