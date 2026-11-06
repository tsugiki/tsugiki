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
mod tests {
    use super::*;

    const FIVE_MINUTES: Duration = Duration::minutes(5);

    #[test]
    fn a_date_header_reads_as_its_time() {
        assert_eq!(
            http_date("Thu, 06 Nov 2026 10:00:00 GMT"),
            OffsetDateTime::from_unix_timestamp(1_793_959_200).ok()
        );
        assert_eq!(http_date("yesterday"), None);
    }

    #[test]
    fn with_no_reply_the_local_clock_stands_in() {
        let before = OffsetDateTime::now_utc();
        let now = GitHubClock::default().now();
        assert!(before <= now && now <= OffsetDateTime::now_utc());
    }

    #[test]
    fn githubs_time_goes_by_its_last_reply_whatever_the_local_clock_says() {
        for local_is_off_by in [FIVE_MINUTES, -FIVE_MINUTES] {
            let github = OffsetDateTime::now_utc();
            let (clock, _) = GitHubClock::default().after(github, github + local_is_off_by);

            let before = OffsetDateTime::now_utc();
            let now = clock.now();
            let after = OffsetDateTime::now_utc();

            assert!(before - local_is_off_by <= now && now <= after - local_is_off_by);
        }
    }

    #[test]
    fn the_log_hears_of_a_skew_past_a_minute_and_again_when_it_moves() {
        let local = OffsetDateTime::now_utc();
        let clock = GitHubClock::default();
        let nearly = FIVE_MINUTES + Duration::seconds(30);

        let (clock, first) = clock.after(local + FIVE_MINUTES, local);
        let (clock, again) = clock.after(local + nearly, local);
        let (clock, moved) = clock.after(local - FIVE_MINUTES, local);
        let (clock, fixed) = clock.after(local, local);
        let (_, back) = clock.after(local + FIVE_MINUTES, local);

        assert_eq!(first, Some(FIVE_MINUTES));
        assert_eq!(again, None);
        assert_eq!(moved, Some(-FIVE_MINUTES));
        assert_eq!(fixed, None);
        assert_eq!(back, Some(FIVE_MINUTES));
    }

    #[test]
    fn a_skew_of_a_minute_or_less_is_no_news() {
        let local = OffsetDateTime::now_utc();
        let (_, news) = GitHubClock::default().after(local - LARGE_SKEW, local);
        assert_eq!(news, None);
    }

    #[test]
    fn the_warning_says_which_clock_is_ahead_and_by_how_much() {
        assert_eq!(
            skew_warning(FIVE_MINUTES),
            "This computer's clock is 300 seconds behind GitHub's, so Tsugiki goes by GitHub's"
        );
        assert_eq!(
            skew_warning(-FIVE_MINUTES),
            "This computer's clock is 300 seconds ahead of GitHub's, so Tsugiki goes by GitHub's"
        );
    }
}
