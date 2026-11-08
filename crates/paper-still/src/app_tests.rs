use super::*;

#[test]
fn caps_raster_size() {
    assert_eq!(parse_size("3840x2160").unwrap(), (3840, 2160));
    for size in ["0x200", "15x200", "8193x16", "5120x2880"] {
        assert!(parse_size(size).is_err());
    }
}

#[test]
fn accepts_logical_size() {
    assert_eq!(parse_dimensions("5120x2880").unwrap(), (5120, 2880));
    for size in ["0x200", "100x0", "100x", "-1x200", "invalid"] {
        assert!(parse_dimensions(size).is_err());
    }
}
