//! Reading a channel until a deadline.

use std::iter;
use std::sync::mpsc::Receiver;
use std::time::Instant;

/// What `receiver` hears as it comes, until `until` or until its senders have all gone. It reads nothing
/// once `until` has passed, though more is queued, since `recv_timeout` with a wait of zero would still hand
/// out each queued message, for as long as a busy sender keeps the queue full.
pub fn received_until<T>(receiver: &Receiver<T>, until: Instant) -> impl Iterator<Item = T> {
    iter::from_fn(move || {
        let wait = (until.checked_duration_since(Instant::now())).filter(|wait| !wait.is_zero())?;
        receiver.recv_timeout(wait).ok()
    })
}
