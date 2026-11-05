use super::*;
use serde_json::json;

fn particle(id: u64, x: f32) -> Particle {
    Particle { id, pos: [x, 0.0, 0.0], lifetime: 10.0, ..EMPTY }
}

fn alignment() -> Boids {
    Boids::parse(
        &json!({"flags":0,"separationfactor":0,"alignmentfactor":1,"cohesionfactor":0,"neighborthreshold":1000}),
        Defaults::ORTHOGRAPHIC,
    )
}

#[test]
fn later_batches_observe_writes() {
    let mut particles: Vec<_> = (0..8).map(|id| particle(id, id as f32 * 10.0)).collect();
    for particle in &mut particles[..4] {
        particle.vel[0] = 10.0;
    }
    alignment().apply(&mut particles, 8, 0, 1.0);
    let first = 30.0 * reciprocal(7.0);
    let second = first * 4.0 * reciprocal(7.0);
    assert!((particles[0].vel[0] - first).abs() < 0.00001);
    assert!((particles[4].vel[0] - second).abs() < 0.00001);
}

#[test]
fn lifetime_mask_stays_fixed() {
    let mut particles =
        std::array::from_fn::<_, 4, _>(|index| particle(index as u64, (index + 1) as f32 * 10.0));
    particles[1].lifetime = 0.0;
    particles[1].vel[0] = 100.0;
    alignment().apply(&mut particles, 4, 0, 1.0);
    assert_eq!(particles[0].vel[0], 100.0 * reciprocal(3.0));
    assert_eq!(particles[1].vel[0], 100.0);
    assert_eq!(particles[2].vel[0], particles[0].vel[0]);
}

#[test]
fn padding_affects_cohesion() {
    let mut particles = [EMPTY; 4];
    particles[0] = particle(0, 10.0);
    let boids = Boids::parse(
        &json!({"flags":0,"separationfactor":0,"alignmentfactor":0,"cohesionfactor":1}),
        Defaults::ORTHOGRAPHIC,
    );
    boids.apply(&mut particles, 1, 0, 1.0);
    assert_eq!(particles[0].vel[0], -10.0);
}

#[test]
fn slots_reuse_first_hole() {
    let mut pool = Pool::new();
    pool.sync(&[particle(0, 10.0), particle(1, 20.0), particle(2, 30.0)], 0.1);
    pool.particles[1].vel[0] = 7.0;
    pool.sync(&[particle(0, 10.0), particle(2, 30.0)], 0.1);
    assert_eq!(pool.count, 3);
    assert_eq!(pool.particles.len(), 4);
    assert_eq!(pool.particles[1].pos[0], 20.0);
    assert_eq!(pool.particles[1].vel[0], 7.0);
    assert_eq!(pool.particles[1].lifetime, 0.0);
    pool.sync(&[particle(0, 10.0), particle(2, 30.0), particle(3, 40.0)], 0.1);
    assert_eq!(pool.particles[1].id, 3);
    assert_eq!(pool.count, 3);
}

#[test]
fn sync_survives_slot_order() {
    let mut pool = Pool::new();
    pool.sync(&[particle(0, 10.0), particle(1, 20.0), particle(2, 30.0)], 0.1);
    let live = [particle(0, 10.0), particle(3, 40.0), particle(2, 30.0)];
    for _ in 0..2 {
        pool.sync(&live, 0.1);
        assert_eq!(pool.count, 3);
        let ids: Vec<_> = pool.particles[..3].iter().map(|pooled| pooled.id).collect();
        assert_eq!(ids, [0, 3, 2]);
        assert_eq!(pool.indices[..3], [Some(0), Some(1), Some(2)]);
        assert!(pool.particles[..3].iter().all(|pooled| pooled.lifetime > 0.0));
    }
}

#[test]
fn speed_limit_preserves_faster() {
    let mut particles = [EMPTY; 4];
    particles[0] = particle(0, 0.0);
    particles[0].vel[0] = 100.0;
    let boids = Boids::parse(
        &json!({"maxspeed":10,"separationfactor":0,"alignmentfactor":0,"cohesionfactor":0}),
        Defaults::ORTHOGRAPHIC,
    );
    boids.apply(&mut particles, 1, 0, 0.1);
    assert_eq!(particles[0].vel[0], 100.0);
}

#[test]
fn pool_reuses_allocations() {
    let mut pool = Pool::new();
    pool.sync(&[particle(0, 10.0)], 0.1);
    let capacities = (pool.particles.capacity(), pool.indices.capacity(), pool.previous.capacity());
    for id in 1..1000 {
        pool.sync(&[particle(id, 10.0)], 0.1);
        assert_eq!(pool.count, 1);
        assert_eq!(pool.particles.len(), 4);
        assert_eq!(
            (pool.particles.capacity(), pool.indices.capacity(), pool.previous.capacity()),
            capacities
        );
    }
}
