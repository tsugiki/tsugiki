//! The rate limit's gate, which every request to GitHub passes (ADR 0015). The hour's points belong to the
//! user, and other tools share them, so below a floor of them nothing asks GitHub until the reset, whichever
//! job asks. After GitHub refuses a request for its rate limit, nothing asks until the time it named.

use time::{Duration, OffsetDateTime};

use super::points::HourPoints;

/// Below this many points left, nothing asks GitHub until the reset, because the hourly 5,000 are also the
/// user's other tools'. We haven't measured what those tools need, and GitHub's `remaining` is approximate.
pub const RATE_LIMIT_FLOOR: u32 = 500;

/// The wait after GitHub refused a request for its rate limit and named no reset, or one already past. Its
/// docs ask a client to wait at least a minute then.
const REFUSED_WAIT: Duration = Duration::minutes(1);

/// The rate limit resets every hour, so a longer hold can only come from a reset that's wrong.
const LONGEST_HOLD: Duration = Duration::hours(1);

/// The gate's own state, beside the hour's points, which the client keeps: the refusal GitHub gave for its
/// rate limit, if any, and when it lifts.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Gate {
    /// The time by GitHub's clock its rate limit lifts, after it refused a request for it.
    refused_until: Option<OffsetDateTime>,
}

impl Gate {
    /// The gate after GitHub refused a request at `now`, its time, for its rate limit, which lifts at
    /// `resets_at` if it said.
    pub(crate) fn refused(self, resets_at: Option<OffsetDateTime>, now: OffsetDateTime) -> Self {
        let until = resets_at
            .filter(|at| *at > now)
            .unwrap_or(now + REFUSED_WAIT);
        Self {
            refused_until: Some(until.min(now + LONGEST_HOLD)),
        }
    }

    /// The time by GitHub's clock the gate opens, while it holds at `now`, GitHub's time. It holds while the
    /// hour's `points` are below the floor, until they reset, and after a refusal, until the time GitHub
    /// named.
    pub(crate) fn holds_until(
        self,
        points: Option<HourPoints>,
        now: OffsetDateTime,
    ) -> Option<OffsetDateTime> {
        let low = points
            .filter(|points| points.remaining < RATE_LIMIT_FLOOR)
            .map(|points| points.resets_at);
        let until = (low.into_iter().chain(self.refused_until))
            .filter(|at| *at > now)
            .max()?;
        Some(until.min(now + LONGEST_HOLD))
    }
}

#[cfg(test)]
mod tests;
