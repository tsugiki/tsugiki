//! GitHub's time, worked out from the `Date` of its replies, so a wrong local clock can't make a refresh miss
//! a change (ADR 0015).

use time::format_description::well_known::Rfc2822;
use time::{Duration, OffsetDateTime};

/// A skew past this goes to the log.
const LARGE_SKEW: Duration = Duration::minutes(1);

/// The time in a reply's `Date` header, such as `Thu, 06 Nov 2026 10:00:00 GMT`.
pub(crate) fn http_date(text: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(text, &Rfc2822).ok()
}

/// GitHub's time as the last dated reply told it. Before any reply, the local clock stands in.
#[derive(Debug, Clone, Copy, Default)]
pub struct GitHubClock {
    /// GitHub's time less the local time, as the last dated reply showed it.
    skew: Option<Duration>,
    /// The skew the log last heard of, while the skew stays past a minute.
    logged: Option<Duration>,
}

impl GitHubClock {
    /// GitHub's time now. It goes by the local clock and the skew, so a computer that slept comes back with
    /// the right time.
    pub fn now(self) -> OffsetDateTime {
        let local = OffsetDateTime::now_utc();
        self.skew.map_or(local, |skew| local + skew)
    }

    /// This clock after a reply GitHub dated `date`, which Tsugiki read when the local clock said `local`.
    /// Gives the skew too when the log should hear of it: when it passes a minute, and again when it moves
    /// by more than a minute from what the log last heard.
    pub(crate) fn after(
        self,
        date: OffsetDateTime,
        local: OffsetDateTime,
    ) -> (Self, Option<Duration>) {
        let skew = date - local;
        let is_large = skew.abs() > LARGE_SKEW;
        let moved = self
            .logged
            .is_none_or(|logged| (skew - logged).abs() > LARGE_SKEW);
        let news = (is_large && moved).then_some(skew);
        let logged = if is_large { news.or(self.logged) } else { None };
        let clock = Self {
            skew: Some(skew),
            logged,
        };
        (clock, news)
    }
}

/// The log's words for a skew of `skew`, GitHub's time less the local time.
pub(crate) fn skew_warning(skew: Duration) -> String {
    let seconds = skew.whole_seconds();
    let side = if seconds > 0 { "behind" } else { "ahead of" };
    format!(
        "This computer's clock is {} seconds {side} GitHub's, so Tsugiki goes by GitHub's",
        seconds.unsigned_abs()
    )
}

#[cfg(test)]
mod tests;
