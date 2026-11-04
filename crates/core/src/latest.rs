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
mod tests {
    use super::*;

    #[test]
    fn a_reader_starts_on_the_first_value() {
        let (_writer, reader) = channel(1);
        assert_eq!(reader.get(), 1);
    }

    #[test]
    fn every_reader_sees_what_the_writer_set_last() {
        let (mut writer, reader) = channel(1);
        let another = reader.clone();

        writer.set(&2);

        assert_eq!(reader.get(), 2);
        assert_eq!(another.get(), 2);
    }

    #[test]
    fn a_reader_keeps_the_last_value_after_the_writer_has_gone() {
        let (mut writer, reader) = channel(1);
        writer.set(&2);

        drop(writer);

        assert_eq!(reader.get(), 2);
    }

    #[test]
    fn a_panic_while_holding_the_lock_doesnt_stop_it_working() {
        let (mut writer, reader) = channel(1);
        std::thread::scope(|scope| {
            let holder = scope.spawn(|| {
                let _guard = reader.0.lock();
                panic!("a panic while holding the lock");
            });
            assert!(holder.join().is_err());
        });
        assert!(reader.0.is_poisoned());

        assert_eq!(reader.get(), 1);
        writer.set(&2);
        assert_eq!(reader.get(), 2);
    }
}
