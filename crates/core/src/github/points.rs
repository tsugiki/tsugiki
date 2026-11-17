//! The hour's GraphQL points: what GitHub's last answer counted, and the sum of what Tsugiki's answers cost
//! (ADR 0015).

use serde::Serialize;
use specta::Type;
use time::{Duration, OffsetDateTime};

use super::pull_requests::RateLimit;

/// A reset this close to the last one is the same hour, so a second's rounding doesn't start the sum again.
/// GitHub's next hour starts only once this one has reset, so its reset is much later.
const SAME_HOUR: Duration = Duration::minutes(1);

/// The hour's points, as GitHub's last answer counted them, with the points Tsugiki's answers cost. It lives
/// in memory only, so a restart counts again from its first answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct HourPoints {
    /// The points an hour gives.
    pub limit: u32,
    /// The points every tool that uses the token spent this hour, Tsugiki among them.
    pub used: u32,
    pub remaining: u32,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub resets_at: OffsetDateTime,
    /// The points Tsugiki's answers cost this hour.
    pub spent: u32,
    /// The time Tsugiki started counting, if its first answer of the run came after the hour had spent
    /// points: what it spent before a restart is gone. Nothing, when the count holds all Tsugiki spent this
    /// hour.
    #[serde(with = "time::serde::rfc3339::option")]
    #[specta(type = Option<String>)]
    pub since: Option<OffsetDateTime>,
}

impl HourPoints {
    /// The count after an answer that reported `answer` at `now`, GitHub's time. `last` is the count before
    /// it, nothing before the run's first answer. An answer from a new hour starts the sum again. An answer
    /// whose hour has already reset by `now` belongs to a count that is over, so it gives nothing.
    pub(crate) fn after(
        last: Option<Self>,
        answer: RateLimit,
        now: OffsetDateTime,
    ) -> Option<Self> {
        let same_hour = last.filter(|last| answer.reset_at <= last.resets_at + SAME_HOUR);
        if answer.reset_at <= now || same_hour.is_some_and(|last| last.resets_at <= now) {
            return None;
        }
        let (spent, since, resets_at) = match (last, same_hour) {
            // An answer that says the hour resets a little earlier doesn't move the reset back.
            (_, Some(last)) => (
                last.spent.saturating_add(answer.cost),
                last.since,
                last.resets_at.max(answer.reset_at),
            ),
            // Tsugiki was counting when the hour began, so whatever came before its answer was not its own.
            (Some(_), None) => (answer.cost, None, answer.reset_at),
            (None, None) => (
                answer.cost,
                (answer.used > answer.cost).then_some(now),
                answer.reset_at,
            ),
        };
        Some(Self {
            limit: answer.limit,
            used: answer.used,
            remaining: answer.remaining,
            resets_at,
            spent,
            since,
        })
    }
}

#[cfg(test)]
mod tests;
