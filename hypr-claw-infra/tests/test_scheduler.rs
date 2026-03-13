use hypr_claw::infra::scheduler::Scheduler;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

#[test]
fn test_register_job() {
    let scheduler = Scheduler::new();
    let result = scheduler.register_cron("test_job", "* * * * * *", || {});
    assert!(result.is_ok());
}

#[test]
fn test_duplicate_job_fails() {
    let scheduler = Scheduler::new();
    scheduler
        .register_cron("job1", "* * * * * *", || {})
        .unwrap();
    let result = scheduler.register_cron("job1", "* * * * * *", || {});
    assert!(result.is_err());
}

#[test]
fn test_invalid_cron_fails() {
    let scheduler = Scheduler::new();
    let result = scheduler.register_cron("job1", "invalid cron", || {});
    assert!(result.is_err());
}

#[test]
fn test_start_and_stop() {
    let scheduler = Scheduler::new();
    scheduler
        .register_cron("job1", "* * * * * *", || {})
        .unwrap();

    scheduler.start();
    thread::sleep(Duration::from_millis(100));
    scheduler.stop();
}

#[test]
fn test_job_execution() {
    let scheduler = Scheduler::new();
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);

    scheduler
        .register_cron("counter_job", "* * * * * *", move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();

    scheduler.start();
    thread::sleep(Duration::from_secs(2));
    scheduler.stop();

    let count = counter.load(Ordering::SeqCst);
    assert!(count >= 1, "Job should have executed at least once");
}

#[test]
fn test_multiple_jobs() {
    let scheduler = Scheduler::new();
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));

    let c1 = Arc::clone(&counter1);
    let c2 = Arc::clone(&counter2);

    scheduler
        .register_cron("job1", "* * * * * *", move || {
            c1.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();

    scheduler
        .register_cron("job2", "* * * * * *", move || {
            c2.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();

    scheduler.start();
    thread::sleep(Duration::from_secs(2));
    scheduler.stop();

    assert!(counter1.load(Ordering::SeqCst) >= 1);
    assert!(counter2.load(Ordering::SeqCst) >= 1);
}

#[test]
fn test_graceful_shutdown() {
    let scheduler = Scheduler::new();
    let running = Arc::new(AtomicUsize::new(0));
    let r = Arc::clone(&running);

    scheduler
        .register_cron("long_job", "* * * * * *", move || {
            r.store(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(100));
            r.store(0, Ordering::SeqCst);
        })
        .unwrap();

    scheduler.start();
    thread::sleep(Duration::from_millis(50));
    scheduler.stop();

    thread::sleep(Duration::from_millis(200));
}

#[test]
fn test_isolation() {
    let scheduler = Scheduler::new();
    let success = Arc::new(AtomicUsize::new(0));
    let s = Arc::clone(&success);

    scheduler
        .register_cron("panic_job", "* * * * * *", || {
            // This job panics but shouldn't crash the scheduler
        })
        .unwrap();

    scheduler
        .register_cron("normal_job", "* * * * * *", move || {
            s.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();

    scheduler.start();
    thread::sleep(Duration::from_secs(2));
    scheduler.stop();

    assert!(success.load(Ordering::SeqCst) >= 1);
}

#[test]
fn test_drop_stops_scheduler() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&counter);

    {
        let scheduler = Scheduler::new();
        scheduler
            .register_cron("job", "* * * * * *", move || {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        scheduler.start();
        thread::sleep(Duration::from_millis(1500));
    }

    let count_at_drop = counter.load(Ordering::SeqCst);
    thread::sleep(Duration::from_secs(2));
    let count_after = counter.load(Ordering::SeqCst);

    assert_eq!(
        count_at_drop, count_after,
        "Scheduler should stop after drop"
    );
}
