use super::*;
use time::format_description::well_known::Rfc3339;

/// The time `text` names, such as `2026-11-17T14:32:00Z`.
fn at(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap()
}

fn reset() -> OffsetDateTime {
    at("2026-11-17T14:32:00Z")
}

fn answer(cost: u32, used: u32, reset_at: OffsetDateTime) -> RateLimit {
    RateLimit {
        cost,
        limit: 5000,
        used,
        remaining: 5000 - used,
        reset_at,
    }
}

#[test]
fn the_answers_of_an_hour_add_up() {
    let now = at("2026-11-17T13:40:00Z");
    let first = HourPoints::after(None, answer(3, 3, reset()), now);
    let second = HourPoints::after(Some(first), answer(5, 20, reset()), now);
    assert_eq!(
        second,
        HourPoints {
            limit: 5000,
            used: 20,
            remaining: 4980,
            resets_at: reset(),
            spent: 8,
            since: None,
        }
    );
}

#[test]
fn a_new_hour_starts_the_sum_again() {
    let now = at("2026-11-17T14:00:00Z");
    let first = HourPoints::after(None, answer(3, 3, reset()), now);
    let next_reset = reset() + Duration::hours(1);
    let next = HourPoints::after(Some(first), answer(2, 40, next_reset), reset());
    assert_eq!((next.spent, next.used, next.resets_at), (2, 40, next_reset));
    // Tsugiki was counting when this hour began, so the 38 other points were another tool's.
    assert_eq!(next.since, None);
}

#[test]
fn a_reset_a_second_off_is_the_same_hour() {
    let now = at("2026-11-17T14:00:00Z");
    let first = HourPoints::after(None, answer(3, 3, reset()), now);
    let next = HourPoints::after(Some(first), answer(2, 5, reset() + Duration::SECOND), now);
    assert_eq!(next.spent, 5);
}

#[test]
fn a_run_that_starts_in_a_spent_hour_says_since_when_it_counted() {
    let now = at("2026-11-17T14:05:00Z");
    let first = HourPoints::after(None, answer(4, 3350, reset()), now);
    let later = at("2026-11-17T14:20:00Z");
    let second = HourPoints::after(Some(first), answer(6, 3400, reset()), later);
    assert_eq!((second.spent, second.since), (10, Some(now)));
}

#[test]
fn a_new_hour_forgets_since_when_the_run_counted() {
    let now = at("2026-11-17T14:05:00Z");
    let first = HourPoints::after(None, answer(4, 3350, reset()), now);
    let next = HourPoints::after(
        Some(first),
        answer(1, 1, reset() + Duration::hours(1)),
        reset(),
    );
    assert_eq!(next.since, None);
}
