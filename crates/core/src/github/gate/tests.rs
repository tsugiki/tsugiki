use super::*;

const NOW: OffsetDateTime = OffsetDateTime::UNIX_EPOCH;

fn points_left(remaining: u32, resets_at: OffsetDateTime) -> HourPoints {
    HourPoints {
        limit: 5000,
        used: 5000 - remaining,
        remaining,
        resets_at,
        spent: 1,
        since: None,
    }
}

#[test]
fn an_open_gate_holds_nothing() {
    assert_eq!(Gate::default().holds_until(None, NOW), None);
}

#[test]
fn the_gate_holds_only_below_the_floor_until_the_reset() {
    let reset = NOW + Duration::minutes(20);
    let gate = Gate::default();

    let at_the_floor = points_left(RATE_LIMIT_FLOOR, reset);
    let below = points_left(RATE_LIMIT_FLOOR - 1, reset);

    assert_eq!(gate.holds_until(Some(at_the_floor), NOW), None);
    assert_eq!(gate.holds_until(Some(below), NOW), Some(reset));
}

#[test]
fn the_gate_opens_at_the_reset() {
    let reset = NOW + Duration::minutes(20);
    let below = points_left(10, reset);

    assert_eq!(Gate::default().holds_until(Some(below), reset), None);
}

#[test]
fn a_refusal_holds_until_the_time_github_named() {
    let reset = NOW + Duration::minutes(5);

    let gate = Gate::default().refused(Some(reset), NOW);

    assert_eq!(gate.holds_until(None, NOW), Some(reset));
    assert_eq!(gate.holds_until(None, reset), None);
}

#[test]
fn a_refusal_with_no_time_or_a_past_one_holds_a_minute() {
    let unnamed = Gate::default().refused(None, NOW);
    let past = Gate::default().refused(Some(NOW - Duration::minutes(5)), NOW);

    assert_eq!(unnamed.holds_until(None, NOW), Some(NOW + REFUSED_WAIT));
    assert_eq!(past.holds_until(None, NOW), Some(NOW + REFUSED_WAIT));
}

#[test]
fn the_gate_holds_until_the_later_of_the_two() {
    let soon = NOW + Duration::minutes(5);
    let later = NOW + Duration::minutes(30);
    let gate = Gate::default().refused(Some(soon), NOW);

    assert_eq!(
        gate.holds_until(Some(points_left(10, later)), NOW),
        Some(later)
    );
}

#[test]
fn the_gate_holds_no_longer_than_an_hour() {
    let far = NOW + Duration::hours(10);

    assert_eq!(
        Gate::default().holds_until(Some(points_left(10, far)), NOW),
        Some(NOW + LONGEST_HOLD)
    );
    assert_eq!(
        Gate::default()
            .refused(Some(far), NOW)
            .holds_until(None, NOW),
        Some(NOW + LONGEST_HOLD)
    );
}
