use crate::rt::object::{self, Object};
use crate::rt::{self, Access, Synchronize};

use std::sync::atomic::Ordering::{Acquire, Release};

#[derive(Debug, Copy, Clone)]
pub(crate) struct Notify {
    obj: Object,
}

#[derive(Debug)]
pub(super) struct State {
    /// When true, notification is sequentiall consistent.
    seq_cst: bool,

    /// `true` if there is a pending notification to consume.
    notified: bool,

    /// Tracks access to the notify object
    last_access: Option<Access>,

    /// Causality transfers between threads
    synchronize: Synchronize,
}

impl Notify {
    pub(crate) fn new(seq_cst: bool) -> Notify {
        super::execution(|execution| {
            let obj = execution.objects.insert_notify(State {
                seq_cst,
                notified: false,
                last_access: None,
                synchronize: Synchronize::new(execution.max_threads),
            });

            Notify { obj }
        })
    }

    pub(crate) fn notify(self) {
        self.obj.branch();

        rt::execution(|execution| {
            {
                let state = self.get_state(&mut execution.objects);

                state
                    .synchronize
                    .sync_store(&mut execution.threads, Release);

                if state.seq_cst {
                    execution.threads.seq_cst();
                }

                state.notified = true;
            }

            let (active, inactive) = execution.threads.split_active();

            for thread in inactive {
                let obj = thread
                    .operation
                    .as_ref()
                    .map(|operation| operation.object());

                if obj == Some(self.obj) {
                    thread.unpark(active);
                }
            }
        });
    }

    pub(crate) fn wait(self) {
        let notified = rt::execution(|execution| self.get_state(&mut execution.objects).notified);

        if notified {
            self.obj.branch();
        } else {
            // This should become branch_disable
            self.obj.branch_acquire(true)
        }

        // Thread was notified
        super::execution(|execution| {
            let state = self.get_state(&mut execution.objects);

            assert!(state.notified);

            state.synchronize.sync_load(&mut execution.threads, Acquire);

            if state.seq_cst {
                // Establish sequential consistency between locks
                execution.threads.seq_cst();
            }

            state.notified = false;
        });
    }

    fn get_state<'a>(self, store: &'a mut object::Store) -> &'a mut State {
        self.obj.notify_mut(store).unwrap()
    }
}

impl State {
    pub(crate) fn last_dependent_accesses<'a>(&'a self) -> Box<dyn Iterator<Item = &Access> + 'a> {
        Box::new(self.last_access.iter())
    }

    pub(crate) fn set_last_access(&mut self, access: Access) {
        self.last_access = Some(access);
    }
}
