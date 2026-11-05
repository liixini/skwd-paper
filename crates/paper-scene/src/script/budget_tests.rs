use super::super::budget::{ENGINE_WATCHDOG, LOAD_BUDGET};
use super::{Budget, Limits, drain_jobs};
use rquickjs::{Context, Function, Runtime};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const SHORT: Duration = Duration::from_millis(100);

fn runtime(budget: &Rc<Budget>) -> (Runtime, Context) {
    let runtime = Runtime::new().unwrap();
    let interrupt = budget.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= interrupt.get())));
    let context = Context::full(&runtime).unwrap();
    (runtime, context)
}

#[test]
fn production_limits_pinned() {
    assert_eq!(ENGINE_WATCHDOG, Duration::from_secs(15));
    assert_eq!(LOAD_BUDGET, Duration::from_secs(5));
    assert_eq!(Limits::default(), Limits { load: LOAD_BUDGET, compose: ENGINE_WATCHDOG });
}

#[test]
fn entries_share_allowance() {
    let budget = Budget::new(SHORT);
    drop(budget.enter());
    let first = budget.enter();
    std::thread::sleep(SHORT / 2);
    drop(first);
    let second = budget.enter();
    assert!(budget.get() < Instant::now() + SHORT / 2);
    std::thread::sleep(SHORT);
    drop(second);
    assert!(budget.spent());
    budget.renew(SHORT);
    assert!(!budget.spent());
}

#[test]
fn nested_entry_keeps_deadline() {
    let budget = Budget::new(SHORT);
    let outer = budget.enter();
    let deadline = budget.get();
    std::thread::sleep(SHORT / 4);
    drop(budget.enter());
    assert_eq!(budget.get(), deadline);
    drop(outer);
}

#[test]
fn spent_entry_interrupts() {
    let budget = Budget::new(Duration::ZERO);
    let (_runtime, context) = runtime(&budget);
    let _entry = budget.enter();
    context.with(|ctx| assert!(ctx.eval::<(), _>("while (true) {}").is_err()));
    assert!(budget.exhausted());
    assert!(budget.host().is_err());
}

#[test]
fn repeated_hosts_stop() {
    let budget = Budget::new(SHORT);
    let (_runtime, context) = runtime(&budget);
    let calls = Rc::new(Cell::new(0));
    context.with(|ctx| {
        let host = budget.clone();
        let count = calls.clone();
        ctx.globals()
            .set(
                "host",
                Function::new(ctx.clone(), move || {
                    let Ok(_work) = host.host() else {
                        return;
                    };
                    count.set(count.get() + 1);
                    std::thread::sleep(Duration::from_millis(30));
                })
                .unwrap(),
            )
            .unwrap();
        let start = Instant::now();
        let _entry = budget.enter();
        assert!(ctx.eval::<(), _>("while (true) { host(); }").is_err());
        assert!((1..=5).contains(&calls.get()));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(budget.host().is_err());
    });
}

#[test]
fn jobs_stop_when_spent() {
    let budget = Budget::new(SHORT);
    let (_runtime, context) = runtime(&budget);
    let _entry = budget.enter();
    context.with(|ctx| {
        ctx.eval::<(), _>("Promise.resolve().then(() => 1); Promise.resolve().then(() => 2);")
            .unwrap();
        std::thread::sleep(SHORT);
        assert!(drain_jobs(&ctx, &budget).is_err());
    });
}

#[test]
fn cleanup_stays_bounded() {
    let budget = Budget::new(Duration::ZERO);
    let _entry = budget.enter();
    assert!(budget.exhausted());
    budget.cleanup();
    assert!(!budget.exhausted());
}

#[test]
#[ignore = "requires pinned dynamic image package"]
fn native_load_loop_stops() {
    let path = std::env::var_os("SKWD_DYNAMIC_IMAGE_PACKAGE").expect("pinned image package");
    let package = crate::pkg::Package::open(std::path::Path::new(&path)).unwrap();
    let assets = crate::effects::Assets::discover(Some("/nonexistent"));
    let images = crate::model::ImageResources::new(&package, &assets);
    let mut scene = serde_json::json!({"general":{"orthogonalprojection":{"width":64,"height":64}},"objects":[{"id":1,"alpha":{"value":1,"script":"export function init(){for(;;){try{thisScene.createLayer({image:'models/ordinary.json'});}catch(error){}}}"}}]});
    let start = Instant::now();
    let host = super::SceneScripts::load_with_storage(
        &mut scene,
        &crate::model::Properties::new(),
        &serde_json::Value::Null,
        super::Load {
            images: Some(images),
            limits: Limits { load: Duration::from_secs(1), compose: SHORT },
            ..super::Load::default()
        },
    )
    .unwrap()
    .unwrap();
    assert!(host.diagnostics.iter().any(|message| message.contains("interrupted")));
    assert!(start.elapsed() < Duration::from_secs(4));
    assert!(host.image_resources().unwrap().payload_bytes() > 0);
}
