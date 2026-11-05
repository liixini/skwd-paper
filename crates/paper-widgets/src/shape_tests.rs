use super::*;

#[test]
fn rim_walks_clockwise() {
    let centre = [10.0, 10.0];
    assert_eq!(rim_at(centre, 4.0, 0.0), [10.0, 6.0]);
    assert_eq!(rim_at(centre, 4.0, 0.25), [14.0, 10.0]);
    assert_eq!(rim_at(centre, 4.0, 0.125), [12.0, 8.0]);
    assert_eq!(rim_at(centre, 4.0, 1.0), [10.0, 6.0]);
}

#[test]
fn sweep_grows_with_time() {
    assert_eq!(sweep([0.0, 0.0], 4.0, 0.1).len() + 2, sweep([0.0, 0.0], 4.0, 0.6).len());
}
