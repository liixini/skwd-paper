use super::*;
use paper_scene::effect_lifetime as lifetime;
use paper_scene::effects::EffectBind;

const FULL: FxTargetClass = FxTargetClass { width: 64, height: 64, repeat: false, format: 97 };
const HALF: FxTargetClass = FxTargetClass { width: 32, height: 32, ..FULL };

#[derive(Default)]
struct Fake {
    created: usize,
    live: usize,
    budget: Option<usize>,
}

impl TargetSource for Fake {
    type Target = usize;

    fn create(&mut self, _: FxTargetClass) -> Result<usize> {
        anyhow::ensure!(self.budget != Some(self.live), "out of memory");
        self.created += 1;
        self.live += 1;
        Ok(self.created)
    }

    fn destroy(&mut self, _: usize) {
        self.live -= 1;
    }
}

fn pool() -> (FxPool<usize>, Fake) {
    (FxPool::default(), Fake::default())
}

fn indices(leases: &[Lease]) -> Vec<usize> {
    leases.iter().map(|lease| lease.index).collect()
}

#[test]
fn reuses_free_entry() {
    let (mut pool, mut source) = pool();
    let first = pool.lease(&mut source, &[FULL]).unwrap();
    pool.settle(first[0], false);
    assert_eq!(indices(&pool.lease(&mut source, &[FULL]).unwrap()), indices(&first));
    assert_eq!(source.created, 1);
}

#[test]
fn held_entry_exclusive() {
    let (mut pool, mut source) = pool();
    let first = pool.lease(&mut source, &[FULL]).unwrap();
    pool.settle(first[0], true);
    let second = pool.lease(&mut source, &[FULL]).unwrap();
    assert_ne!(indices(&first), indices(&second));
    pool.release_all();
    let both = pool.lease(&mut source, &[FULL, FULL]).unwrap();
    assert_eq!(indices(&both), [first[0].index, second[0].index]);
    assert_eq!(source.created, 2);
}

#[test]
fn classes_never_mix() {
    let (mut pool, mut source) = pool();
    let full = pool.lease(&mut source, &[FULL]).unwrap();
    pool.release(&full);
    let half = pool.lease(&mut source, &[HALF]).unwrap();
    assert_ne!(indices(&full), indices(&half));
    assert_eq!(source.created, 2);
}

#[test]
fn failed_lease_rolls_back() {
    let (mut pool, mut source) = pool();
    let kept = pool.lease(&mut source, &[FULL]).unwrap();
    pool.release(&kept);
    source.budget = Some(2);
    assert!(pool.lease(&mut source, &[FULL, FULL, FULL]).is_err());
    assert_eq!((source.live, pool.len()), (1, 1));
    assert_eq!(indices(&pool.lease(&mut source, &[FULL]).unwrap()), indices(&kept));
    assert_eq!(source.created, 2);
}

#[test]
fn take_frees_slot() {
    let (mut pool, mut source) = pool();
    let leases = pool.lease(&mut source, &[FULL, HALF]).unwrap();
    assert_eq!(pool.take(leases[0]), Some(1));
    assert_eq!(pool.len(), 1);
    assert!(pool.target(leases[0]).is_none());
    pool.release_all();
    let refill = pool.lease(&mut source, &[FULL]).unwrap();
    assert_eq!(indices(&refill), [leases[0].index]);
    assert!(pool.target(leases[0]).is_none());
    assert_eq!(source.created, 3);
}

#[test]
fn stale_lease_unreadable() {
    let (mut pool, mut source) = pool();
    let first = pool.lease(&mut source, &[FULL]).unwrap();
    pool.release_all();
    let second = pool.lease(&mut source, &[FULL]).unwrap();
    assert_eq!(indices(&first), indices(&second));
    assert!(pool.target(first[0]).is_none());
    assert_eq!(pool.take(first[0]), None);
    pool.settle(first[0], false);
    assert_eq!(pool.target(second[0]), Some(&1));
    assert_eq!(indices(&pool.lease(&mut source, &[FULL]).unwrap()), [1]);
}

#[test]
fn trim_keeps_live() {
    let (mut pool, mut source) = pool();
    let leases = pool.lease(&mut source, &[FULL, FULL, FULL]).unwrap();
    pool.settle(leases[0], false);
    pool.settle(leases[1], false);
    pool.trim(&mut source, &leases[1..2]);
    assert_eq!((source.live, pool.len()), (2, 2));
    assert!(pool.target(leases[0]).is_none());
    pool.release_all();
    pool.trim(&mut source, &[]);
    assert_eq!((source.live, pool.len()), (0, 0));
}

#[test]
fn trim_drops_stale() {
    let (mut pool, mut source) = pool();
    let stale = pool.lease(&mut source, &[FULL]).unwrap();
    pool.release_all();
    let fresh = pool.lease(&mut source, &[FULL]).unwrap();
    pool.release_all();
    pool.trim(&mut source, &stale);
    assert_eq!(pool.len(), 0);
    assert!(pool.target(fresh[0]).is_none());
}

#[test]
fn destroy_releases_all() {
    let (mut pool, mut source) = pool();
    pool.lease(&mut source, &[FULL, HALF]).unwrap();
    pool.destroy(&mut source);
    assert_eq!((source.live, pool.len()), (0, 0));
}

#[test]
fn initialises_once() {
    let (mut pool, mut source) = pool();
    let first = pool.lease(&mut source, &[FULL, HALF]).unwrap();
    let mut seen = Vec::new();
    pool.initialise(|target| {
        seen.push(*target);
        Ok(())
    })
    .unwrap();
    pool.release(&first);
    pool.lease(&mut source, &[FULL, FULL]).unwrap();
    pool.initialise(|target| {
        seen.push(*target);
        Ok(())
    })
    .unwrap();
    assert_eq!(seen, [1, 2, 3]);
}

#[test]
fn failed_initialise_retries() {
    let (mut pool, mut source) = pool();
    pool.lease(&mut source, &[FULL]).unwrap();
    assert!(pool.initialise(|_| anyhow::bail!("lost device")).is_err());
    let mut seen = 0;
    pool.initialise(|_| {
        seen += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(seen, 1);
}

#[test]
fn backoff_caps() {
    let mut starved = Starvation::after(None);
    let mut waits = vec![starved.wait];
    for _ in 0..8 {
        starved = Starvation::after(Some(starved));
        waits.push(starved.wait);
    }
    assert_eq!(waits, [0, 1, 2, 4, 8, 16, 32, 64, 64]);
}

#[test]
fn pixel_keepers() {
    for (source, keeps) in [
        ("if(a<0.5) discard;", true),
        ("clip (alpha - 0.5);", true),
        ("clip\n(alpha);", true),
        ("#define KILL discard\nvoid main(){KILL;}", true),
        ("#define CUT(x) clip(x)", true),
        ("texkill r0;", true),
        ("demote;", true),
        ("#include \"common.h\"", true),
        ("float clipped = clipSpace(p);", false),
        ("bool discarded = noclip(x);", false),
        ("#define SKWD_SAMPLER(s) s ## SamplerState", false),
        ("gl_FragColor = vec4(1.0);", false),
        ("#define K(a) dis ## a\nK(card);", false),
    ] {
        assert_eq!(keeps_pixels(source), keeps, "{source}");
    }
}

fn cache_scratch(owners: &[usize], overwrites: bool) -> bool {
    let plan = lifetime::plan_targets_with(
        &["cache".to_owned()],
        &[None],
        &[],
        &[Some("cache".to_owned()), None],
        &[vec![], vec![(0, EffectBind::Named("cache".into()))]],
        owners,
    )
    .unwrap();
    scratch_targets(&plan, owners, &[overwrites, true])[FxTargetId::Fbo(0).index()]
}

#[test]
fn scratch_cache_rules() {
    for (owners, overwrites, scratch) in
        [([0, 0], true, true), ([0, 1], true, false), ([0, 0], false, false)]
    {
        assert_eq!(cache_scratch(&owners, overwrites), scratch, "{owners:?}");
    }
}

#[test]
fn feedback_stays_owned() {
    let plan = lifetime::plan_targets_with(
        &["history".to_owned()],
        &[None],
        &[],
        &[None, Some("history".to_owned())],
        &[vec![(1, EffectBind::Named("history".into()))], vec![]],
        &[0, 0],
    )
    .unwrap();
    let scratch = scratch_targets(&plan, &[0, 0], &[true; 2]);
    assert_eq!(scratch, [true, true, false]);
}
