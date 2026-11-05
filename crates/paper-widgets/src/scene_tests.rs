use super::*;

fn at(weekday: i32) -> Moment {
    Moment::civil(9, 5, weekday, 27, 10)
}

#[test]
fn weeks_start_monday() {
    assert_eq!(at(1).weekday, 0);
    assert_eq!(at(0).weekday, 6);
    assert_eq!(at(2).month, 9);
}

#[test]
fn pads_digits() {
    assert_eq!((at(1).hours(), at(1).minutes()), ("09".to_string(), "05".to_string()));
}
