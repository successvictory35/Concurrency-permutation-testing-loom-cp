use crate::rt::execution;
use crate::rt::object::Operation;
use crate::rt::vv::VersionVec;

use std::{fmt, ops};

#[derive(Debug)]
pub struct Thread {
    pub id: Id,

    /// If the thread is runnable, blocked, or terminated.
    pub state: State,

    /// True if the thread is in a critical section
    pub critical: bool,

    /// The operation the thread is about to take
    pub operation: Option<Operation>,

    /// Tracks observed causality
    pub causality: VersionVec,

    /// Tracks DPOR relations
    pub dpor_vv: VersionVec,

    /// Version at which the thread last yielded
    pub last_yield: Option<usize>,

    /// Number of times the thread yielded
    pub yield_count: usize,
}

#[derive(Debug)]
pub struct Set {
    /// Unique execution identifier
    execution_id: execution::Id,

    /// Set of threads
    threads: Vec<Thread>,

    /// Currently scheduled thread.
    ///
    /// `None` signifies that no thread is runnable.
    active: Option<usize>,

    /// Sequential consistency causality. All sequentially consistent operations
    /// synchronize with this causality.
    pub seq_cst_causality: VersionVec,
}

#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct Id {
    execution_id: execution::Id,
    id: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum State {
    Runnable,
    Blocked,
    Yield,
    Terminated,
}

impl Thread {
    fn new(id: Id, max_threads: usize) -> Thread {
        Thread {
            id,
            state: State::Runnable,
            critical: false,
            operation: None,
            causality: VersionVec::new(max_threads),
            dpor_vv: VersionVec::new(max_threads),
            last_yield: None,
            yield_count: 0,
        }
    }

    pub fn is_runnable(&self) -> bool {
        match self.state {
            State::Runnable => true,
            _ => false,
        }
    }

    pub fn set_runnable(&mut self) {
        self.state = State::Runnable;
    }

    pub fn set_blocked(&mut self) {
        self.state = State::Blocked;
    }

    pub fn is_blocked(&self) -> bool {
        match self.state {
            State::Blocked => true,
            _ => false,
        }
    }

    pub fn is_yield(&self) -> bool {
        match self.state {
            State::Yield => true,
            _ => false,
        }
    }

    pub fn set_yield(&mut self) {
        self.state = State::Yield;
        self.last_yield = Some(self.causality[self.id]);
        self.yield_count += 1;
    }

    pub fn is_terminated(&self) -> bool {
        match self.state {
            State::Terminated => true,
            _ => false,
        }
    }

    pub fn set_terminated(&mut self) {
        self.state = State::Terminated;
    }

    pub(crate) fn unpark(&mut self, unparker: &Thread) {
        self.causality.join(&unparker.causality);

        if self.is_blocked() || self.is_yield() {
            self.set_runnable();
        }
    }
}

impl Set {
    /// Create an empty thread set.
    ///
    /// The set may contain up to `max_threads` threads.
    pub fn new(execution_id: execution::Id, max_threads: usize) -> Set {
        let mut threads = Vec::with_capacity(max_threads);

        // Push initial thread
        threads.push(Thread::new(Id::new(execution_id, 0), max_threads));

        Set {
            execution_id,
            threads,
            active: Some(0),
            seq_cst_causality: VersionVec::new(max_threads),
        }
    }

    pub fn execution_id(&self) -> execution::Id {
        self.execution_id
    }

    /// Create a new thread
    pub fn new_thread(&mut self) -> Id {
        assert!(self.threads.len() < self.max());

        // Get the identifier for the thread about to be created
        let id = self.threads.len();
        let max_threads = self.threads.capacity();

        // Push the thread onto the stack
        self.threads
            .push(Thread::new(Id::new(self.execution_id, id), max_threads));

        Id::new(self.execution_id, id)
    }

    pub fn max(&self) -> usize {
        self.threads.capacity()
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn active_id(&self) -> Id {
        Id::new(self.execution_id, self.active.unwrap())
    }

    pub fn active(&self) -> &Thread {
        &self.threads[self.active.unwrap()]
    }

    pub fn set_active(&mut self, id: Option<Id>) {
        self.active = id.map(Id::as_usize);
    }

    pub fn active_mut(&mut self) -> &mut Thread {
        &mut self.threads[self.active.unwrap()]
    }

    /// Get the active thread and second thread
    pub fn active2_mut(&mut self, other: Id) -> (&mut Thread, &mut Thread) {
        let active = self.active.unwrap();
        let other = other.id;

        if other >= active {
            let (l, r) = self.threads.split_at_mut(other);

            (&mut l[active], &mut r[0])
        } else {
            let (l, r) = self.threads.split_at_mut(active);

            (&mut r[0], &mut l[other])
        }
    }

    pub fn active_causality_inc(&mut self) {
        let id = self.active_id();
        self.active_mut().causality.inc(id);
    }

    pub fn active_atomic_version(&self) -> usize {
        let id = self.active_id();
        self.active().causality[id]
    }

    pub(crate) fn unpark(&mut self, id: Id) {
        if id == self.active_id() {
            return;
        }

        // Synchronize memory
        let (active, th) = self.active2_mut(id);
        th.unpark(&active);
    }

    /// Insert a point of sequential consistency
    pub fn seq_cst(&mut self) {
        self.threads[self.active.unwrap()]
            .causality
            .join(&self.seq_cst_causality);
        self.seq_cst_causality
            .join(&self.threads[self.active.unwrap()].causality);
    }

    pub fn clear(&mut self, execution_id: execution::Id) {
        self.threads.clear();
        self.threads.push(Thread::new(
            Id::new(execution_id, 0),
            self.threads.capacity(),
        ));

        self.execution_id = execution_id;
        self.active = Some(0);
        self.seq_cst_causality = VersionVec::new(self.max());
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = (Id, &'a Thread)> + 'a {
        let execution_id = self.execution_id;
        self.threads
            .iter()
            .enumerate()
            .map(move |(id, thread)| (Id::new(execution_id, id), thread))
    }

    pub fn iter_mut<'a>(&'a mut self) -> Box<dyn Iterator<Item = (Id, &'a mut Thread)> + 'a> {
        let execution_id = self.execution_id;
        Box::new({
            self.threads
                .iter_mut()
                .enumerate()
                .map(move |(id, thread)| (Id::new(execution_id, id), thread))
        })
    }

    /// Split the set of threads into the active thread and an iterator of all
    /// other threads.
    pub fn split_active(&mut self) -> (&mut Thread, impl Iterator<Item = &mut Thread>) {
        let active = self.active.unwrap();
        let (one, two) = self.threads.split_at_mut(active);
        let (active, two) = two.split_at_mut(1);

        let iter = one.iter_mut().chain(two.iter_mut());

        (&mut active[0], iter)
    }
}

impl ops::Index<Id> for Set {
    type Output = Thread;

    fn index(&self, index: Id) -> &Thread {
        &self.threads[index.id]
    }
}

impl ops::IndexMut<Id> for Set {
    fn index_mut(&mut self, index: Id) -> &mut Thread {
        &mut self.threads[index.id]
    }
}

impl Id {
    pub fn new(execution_id: execution::Id, id: usize) -> Id {
        Id { execution_id, id }
    }

    pub fn as_usize(self) -> usize {
        self.id
    }
}

impl fmt::Display for Id {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.id.fmt(fmt)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "Id({})", self.id)
    }
}
