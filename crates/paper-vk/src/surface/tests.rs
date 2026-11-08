use super::*;

#[test]
fn policy_stays_on_thread() {
    let fresh = std::thread::spawn(effects).join().unwrap();
    set(&paper_control::SurfacePolicy { namespace: "test".into(), blur: 20, dim: 10 });
    assert_eq!(effects(), (20.0, 0.1));
    assert_eq!(std::thread::spawn(effects).join().unwrap(), fresh);
}
