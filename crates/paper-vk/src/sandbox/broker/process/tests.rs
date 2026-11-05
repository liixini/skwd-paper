use super::*;

#[test]
fn report_size_bounded() {
    for extra in [0, 1] {
        let mut source = std::io::Cursor::new(vec![b' '; REPORT_BYTES + extra]);
        let mut retained = Vec::new();
        let result = read_bounded(&mut source, &mut retained, REPORT_BYTES);
        assert_eq!(result.is_ok(), extra == 0);
        assert!(retained.len() <= REPORT_BYTES);
    }
}
