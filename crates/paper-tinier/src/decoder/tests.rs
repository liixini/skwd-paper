#![cfg(test)]

use super::*;

#[test]
fn decoder_opens() {
    assert!(Decoder::open().is_ok());
}
