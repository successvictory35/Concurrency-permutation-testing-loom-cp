//! Future related synchronization primitives.

mod atomic_waker;

pub use self::atomic_waker::AtomicWaker;
pub use crate::rt::wait_future as block_on;
pub use crate::rt::poll_future;

use crate::rt::thread;

use arc_waker::Wake;
use std::sync::Arc;
use std::task::Waker;

struct ThreadWaker {
    thread: thread::Id,
}

pub(crate) fn current_waker() -> Waker {
    use std::sync::Arc;

    let thread = thread::Id::current();
    let waker = Arc::new(ThreadWaker { thread });
    arc_waker::waker(waker)

}

impl Wake for ThreadWaker {
    fn wake_by_ref(me: &Arc<Self>) {
        me.thread.future_notify()
    }
}
