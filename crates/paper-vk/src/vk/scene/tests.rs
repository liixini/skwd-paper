use super::timestamp_delta_ns;

#[test]
fn timestamp_delta_applies_device_period() {
    assert_eq!(timestamp_delta_ns(100, 140, 64, 2.5), 100);
}

#[test]
fn timestamp_delta_wraps_at_valid_bit_width() {
    assert_eq!(timestamp_delta_ns(250, 4, 8, 1.0), 10);
}
