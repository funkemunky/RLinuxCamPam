use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use pam_linuxcampam::scoped_worker::ScopedWorker;

#[test]
fn executes_and_joins() {
    let executed = Arc::new(AtomicBool::new(false));
    let exec_clone = Arc::clone(&executed);
    {
        let worker = ScopedWorker::new(move || {
            exec_clone.store(true, Ordering::SeqCst);
        });
        assert!(worker.is_joinable());
    } // Destructor joins
    assert!(executed.load(Ordering::SeqCst));
}

#[test]
fn manual_join() {
    let executed = Arc::new(AtomicBool::new(false));
    let exec_clone = Arc::clone(&executed);
    let mut worker = ScopedWorker::new(move || {
        exec_clone.store(true, Ordering::SeqCst);
    });

    assert!(worker.is_joinable());
    worker.join();
    assert!(!worker.is_joinable());
    assert!(executed.load(Ordering::SeqCst));
}

#[test]
#[allow(unused_assignments)]
fn reassignment_joins_first() {
    let executed1 = Arc::new(AtomicBool::new(false));
    let executed2 = Arc::new(AtomicBool::new(false));

    let e1 = Arc::clone(&executed1);
    let mut worker1 = ScopedWorker::new(move || {
        thread::sleep(Duration::from_millis(10));
        e1.store(true, Ordering::SeqCst);
    });

    let e2 = Arc::clone(&executed2);
    let worker2 = ScopedWorker::new(move || {
        e2.store(true, Ordering::SeqCst);
    });

    // Reassigning worker1 replaces and drops worker1, which joins it
    worker1 = worker2;

    assert!(executed1.load(Ordering::SeqCst));
    assert!(worker1.is_joinable());

    worker1.join();
    assert!(executed2.load(Ordering::SeqCst));
}
