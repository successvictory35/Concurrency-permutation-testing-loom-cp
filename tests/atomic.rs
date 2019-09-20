#![deny(warnings, rust_2018_idioms)]

use loom::sync::atomic::AtomicUsize;
use loom::thread;

use std::cell::UnsafeCell;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Release, SeqCst};
use std::sync::Arc;

#[test]
fn valid_get_mut() {
    loom::model(|| {
        let v1 = Arc::new(UnsafeCell::new(AtomicUsize::new(0)));
        let v2 = v1.clone();

        let th = thread::spawn(move || unsafe {
            (*v2.get()).store(1, SeqCst);
        });

        th.join().unwrap();

        let v = unsafe { *(*v1.get()).get_mut() };
        assert_eq!(1, v);
    });
}

#[test]
#[should_panic]
fn invalid_get_mut() {
    loom::model(|| {
        let v1 = Arc::new(UnsafeCell::new(AtomicUsize::new(0)));
        let v2 = v1.clone();

        thread::spawn(move || unsafe {
            (*v2.get()).store(1, SeqCst);
        });

        let _ = unsafe { *(*v1.get()).get_mut() };
    });
}

#[test]
#[ignore]
#[should_panic]
fn wut() {
    loom::model(|| {
        let a = Arc::new(AtomicUsize::new(0));
        let b = Arc::new(AtomicUsize::new(0));

        let a2 = a.clone();
        let b2 = b.clone();

        let th = thread::spawn(move || {
            a2.store(1, Release);
            b2.compare_and_swap(0, 2, AcqRel);
        });

        b.store(1, Release);
        a.compare_and_swap(0, 2, AcqRel);

        th.join().unwrap();

        let a_val = a.load(Acquire);
        let b_val = b.load(Acquire);

        if a_val == 2 && b_val == 2 {
            panic!();
        }
    });
}
