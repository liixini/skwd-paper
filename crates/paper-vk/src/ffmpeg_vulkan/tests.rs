use super::queue_sync_from_status;

#[test]
fn status_queue_sync() {
    assert_eq!(queue_sync_from_status(1), Some(false));
    assert_eq!(queue_sync_from_status(2), Some(true));
}

#[test]
fn status_unknown_values() {
    assert_eq!(queue_sync_from_status(0), None);
    assert_eq!(queue_sync_from_status(-1), None);
    assert_eq!(queue_sync_from_status(3), None);
}
