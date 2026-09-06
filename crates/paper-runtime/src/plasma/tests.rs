#![cfg(test)]

use super::*;
#[test]
fn acknowledgements_cannot_cross_producer_handoffs() {
    let old = packet(3, 1, 1);
    assert_eq!(acknowledged_slot(&old, 1), Some(1));
    assert_eq!(acknowledged_slot(&old, 2), None);
    assert_eq!(acknowledged_slot(&packet(3, 2, 2), 2), Some(2));
    assert_eq!(acknowledged_slot(&packet(3, 3, 2), 2), None);
    assert_eq!(acknowledged_slot(&old[..7], 1), None);
    assert_eq!(acknowledged_slot(&packet(6, 1, 1), 1), None);
    assert_eq!(acknowledged_slot(&packet(3, 0, 0), 0), Some(0));
}
