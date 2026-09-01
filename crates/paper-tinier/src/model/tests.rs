#![cfg(test)]

use super::*;

#[test]
fn rational_rate_carry() {
    let rate = FrameRate::parse("30000/1001").unwrap();
    let mut clock = FrameClock::new(rate, 50);
    for _ in 0..30_000 {
        clock.advance();
    }
    assert_eq!(clock.deadline, 50 + 1_001_000_000_000);
}

#[test]
fn frame_rate_bounds() {
    for value in ["0", "30/0", "241", "1/2/3", "4294967296"] {
        assert!(FrameRate::parse(value).is_err(), "accepted {value}");
    }
    assert_eq!(FrameRate::parse("240/1").unwrap().numerator, 240);
    assert!(FrameRate::new(30, 0).is_err());
    assert!(
        FrameRate::parse("30000/1001").unwrap().equivalent(FrameRate::parse("60000/2002").unwrap())
    );
    assert_eq!(FrameRate::parse("30000/1001").unwrap().label(), "30000/1001");
}

#[test]
fn lag_reanchors() {
    let mut clock = FrameClock::new(FrameRate::parse("2").unwrap(), 10);
    clock.advance();
    assert!(clock.recover_lag(600_000_000));
    assert_eq!(clock.deadline, 1_100_000_000);
    assert!(!clock.recover_lag(1_000_000_000));
}

#[test]
fn geometry_bounds() {
    let rate = FrameRate::parse("30").unwrap();
    assert!(VideoInfo::new(3840, 2160, rate, ColorMatrix::Bt709).is_ok());
    assert!(VideoInfo::new(3841, 2160, rate, ColorMatrix::Bt709).is_err());
    assert!(VideoInfo::new(-1, 1080, rate, ColorMatrix::Bt709).is_err());
}
