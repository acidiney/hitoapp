use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

struct Tracked(Arc<AtomicUsize>);
impl Drop for Tracked {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn one_read_remains_owned_across_multiple_control_ticks_until_it_completes() {
    let constructions = AtomicUsize::new(0);
    let drops = Arc::new(AtomicUsize::new(0));
    let ticks = AtomicUsize::new(0);
    let gate = Arc::new(Notify::new());
    constructions.fetch_add(1, Ordering::SeqCst);
    let guard = Tracked(drops.clone());
    let input = gate.clone();
    let operation = async move {
        let _guard = guard;
        tokio::time::sleep(Duration::from_millis(60)).await;
        input.notified().await;
        42
    };
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        interruptible(operation, &SourceStop::new(), &mut || {
            if ticks.fetch_add(1, Ordering::SeqCst) >= 3 {
                gate.notify_one();
            }
            false
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result, 42);
    assert!(ticks.load(Ordering::SeqCst) >= 4);
    assert_eq!(constructions.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn interrupting_a_pending_read_drops_that_one_owned_operation() {
    let drops = Arc::new(AtomicUsize::new(0));
    let guard = Tracked(drops.clone());
    let operation = async move {
        let _guard = guard;
        tokio::time::sleep(Duration::from_secs(10)).await;
    };
    let mut ticks = 0;
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        interruptible(operation, &SourceStop::new(), &mut || {
            ticks += 1;
            ticks >= 3
        }),
    )
    .await
    .unwrap();
    assert_eq!(result, Err(Error::Interrupted));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
