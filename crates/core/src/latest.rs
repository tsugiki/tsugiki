//! A value that one thread writes and other threads read.

use std::sync::{Arc, Mutex, PoisonError};

/// Starts a value with one writer and any number of readers. Readers can answer while the writer's thread
/// waits on GitHub or a disk, since the window waits on neither (ADR 0017).
///
/// Give the writer to the thread that owns the value. It can't be cloned, so the compiler holds that thread
/// to being the only one that writes. Clone the reader for everyone else.
///
/// Each function takes the lock, does its one action and lets go, and no guard leaves these types. So nobody
/// can hold two of these at once, in any order, or hold one while waiting on something else. `set_then`
/// runs a closure of the writer's under the lock, which holds only while that closure does no more than
/// its doc allows.
///
/// A panic while holding the lock can only come from `T`'s `clone` or a `set_then`'s `then`, which runs
/// once the value is whole, and a set only ever replaces the value, so both sides read through a poisoned
/// lock.
pub fn channel<T>(first: T) -> (LatestWriter<T>, LatestReader<T>) {
    let shared = Arc::new(Mutex::new(first));
    (LatestWriter(Arc::clone(&shared)), LatestReader(shared))
}

/// The one side that can change the value.
pub struct LatestWriter<T>(Arc<Mutex<T>>);

impl<T: Clone> LatestWriter<T> {
    pub fn set(&mut self, value: &T) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone_from(value);
    }

    /// Sets `value` and runs `then` in one step, under the lock, so a reader that finds `value` finds what
    /// `then` did done. The settings owner sends the saves a value counts this way, so the session finds
    /// each save it counts in its queue.
    ///
    /// `then` may only send on an unbounded channel, which waits on nothing. Readers wait while it runs, so
    /// it mustn't wait, take another lock, which two threads could take in different orders, or read this
    /// value, whose lock it holds already. It takes a closure, not a channel, as the settings owner can't
    /// name the session's message, which `fetching`, above it, owns; so the compiler can't hold `then` to a
    /// send.
    pub fn set_then(&mut self, value: &T, then: impl FnOnce()) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.clone_from(value);
        then();
    }
}

/// A read-only view of the value, which keeps the last value after the writer has gone.
#[derive(Clone)]
pub struct LatestReader<T>(Arc<Mutex<T>>);

impl<T: Clone> LatestReader<T> {
    pub fn get(&self) -> T {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl<T> LatestReader<T> {
    /// What `part` takes from the value, without a copy of the rest. It runs while this holds the lock, so it
    /// is the one action: it only reads, and waits on nothing.
    pub fn read<R>(&self, part: impl FnOnce(&T) -> R) -> R {
        part(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[cfg(test)]
mod tests;
