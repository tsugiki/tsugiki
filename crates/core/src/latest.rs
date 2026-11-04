//! A copy of what one thread owns, for other threads to read.

use std::sync::{Mutex, PoisonError};

/// A read-only copy of what another thread owns. It lets commands answer while that thread waits on GitHub
/// or a disk, since the window waits on neither (ADR 0017).
///
/// Each function takes the lock, does its one action and lets go, and no guard leaves this type. So nobody
/// can hold two of these at once, in any order, or hold one while waiting on something else.
///
/// A panic while holding the lock can only come from `T`'s `clone`, and `set` only ever replaces the value,
/// so these read through a poisoned lock.
#[derive(Default)]
pub struct Latest<T>(Mutex<T>);

impl<T: Clone> Latest<T> {
    pub fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }

    pub fn get(&self) -> T {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set(&self, value: &T) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone_from(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_answers_with_what_was_set_last() {
        let latest = Latest::new(1);
        assert_eq!(latest.get(), 1);

        latest.set(&2);

        assert_eq!(latest.get(), 2);
    }

    #[test]
    fn it_starts_on_the_default_value() {
        assert_eq!(Latest::<u32>::default().get(), 0);
    }

    #[test]
    fn a_panic_while_holding_the_lock_doesnt_stop_it_working() {
        let latest = Latest::new(1);
        std::thread::scope(|scope| {
            let holder = scope.spawn(|| {
                let _guard = latest.0.lock();
                panic!("a panic while holding the lock");
            });
            assert!(holder.join().is_err());
        });
        assert!(latest.0.is_poisoned());

        assert_eq!(latest.get(), 1);
        latest.set(&2);
        assert_eq!(latest.get(), 2);
    }
}
