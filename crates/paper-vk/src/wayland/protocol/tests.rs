use super::parse_dmabuf_format_table;

#[test]
fn format_table_entries() {
    let mut table = Vec::new();
    table.extend_from_slice(&0x3231_564eu32.to_ne_bytes());
    table.extend_from_slice(&[0; 4]);
    table.extend_from_slice(&0x0300_0000_0060_6010u64.to_ne_bytes());
    table.extend_from_slice(&0x3432_5258u32.to_ne_bytes());
    table.extend_from_slice(&[0; 4]);
    table.extend_from_slice(&0u64.to_ne_bytes());
    assert_eq!(
        parse_dmabuf_format_table(&table),
        vec![(0x3231_564e, 0x0300_0000_0060_6010), (0x3432_5258, 0)]
    );
}

#[test]
fn partial_trailing_entry() {
    assert!(parse_dmabuf_format_table(&[0; 15]).is_empty());
}
