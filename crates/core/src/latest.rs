//! A value that one thread writes and other threads read.

use std::sync::{Arc, Mutex, PoisonError};

/// Starts a value with one writer and any number of readers. Readers can answer while the writer's thread
/// waits on GitHub or a disk, since the window waits on neither (ADR 0017).
///
/// Give the writer to the thread that owns the value. It can't be cloned, so the compiler holds that thread
/// to being the only one that writes. Clone the reader for everyone else.
///
/// Each function takes the lock, does its one action and lets go, and no guard leaves these types. So nobody
/// can hold two of these at once, in any order, or hold one while waiting on something else.
///
/// A panic while holding the lock can only come from `T`'s `clone`, and `set` only ever replaces the value,
/// so both sides read through a poisoned lock.
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

#[cfg(test)]
mod tests;
