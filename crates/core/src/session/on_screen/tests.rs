use std::time::{Duration, Instant};

use super::*;
use crate::repository::RepositoryName;

const MINUTE: Duration = Duration::from_secs(60);

fn issue(number: u32) -> Item {
    Item {
        repository: RepositoryName::parse("python/cpython").unwrap(),
        number,
    }
}

/// Items on screen with Issue 7 opened at `at`.
fn opened_at(at: Instant) -> OnScreen {
    let mut on_screen = OnScreen::new(MINUTE);
    on_screen.moved(ScreenMove::Opened(Fetched::Issue(issue(7))), at);
    on_screen
}

#[test]
fn nothing_is_due_while_nothing_is_on_screen() {
    let on_screen = OnScreen::new(MINUTE);

    assert_eq!(on_screen.wake(), None);
    assert_eq!(on_screen.due(Instant::now() + MINUTE), None);
}

#[test]
fn an_item_on_screen_is_read_again_a_minute_after_it_opened() {
    let start = Instant::now();
    let on_screen = opened_at(start);

    assert_eq!(on_screen.wake(), Some(start + MINUTE));
    assert_eq!(on_screen.due(start + MINUTE / 2), None);
    assert_eq!(
        on_screen.due(start + MINUTE),
        Some(Fetched::Issue(issue(7)))
    );
}

#[test]
fn a_read_puts_the_next_one_a_minute_after_it() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);

    on_screen.read(&issue(7), start + MINUTE);

    assert_eq!(on_screen.wake(), Some(start + 2 * MINUTE));
    assert_eq!(on_screen.due(start + MINUTE), None);
}

#[test]
fn a_read_of_an_item_off_screen_changes_nothing() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);

    on_screen.read(&issue(8), start + MINUTE / 2);

    assert_eq!(on_screen.wake(), Some(start + MINUTE));
}

#[test]
fn a_hidden_item_waits_and_reads_at_once_when_it_shows_after_its_minute() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);

    on_screen.moved(ScreenMove::Hid(issue(7)), start);
    assert_eq!(on_screen.wake(), None);
    assert_eq!(on_screen.due(start + 2 * MINUTE), None);
    on_screen.moved(ScreenMove::Showed(issue(7)), start + 2 * MINUTE);

    assert_eq!(
        on_screen.due(start + 2 * MINUTE),
        Some(Fetched::Issue(issue(7)))
    );
}

#[test]
fn an_item_that_shows_again_before_its_minute_waits_for_it() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);

    on_screen.moved(ScreenMove::Hid(issue(7)), start);
    on_screen.moved(ScreenMove::Showed(issue(7)), start + MINUTE / 2);

    assert_eq!(on_screen.wake(), Some(start + MINUTE));
}

#[test]
fn nothing_is_read_while_the_window_is_hidden_and_what_came_due_reads_once_it_shows() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);

    on_screen.moved(ScreenMove::WindowHid, start);
    assert_eq!(on_screen.wake(), None);
    assert_eq!(on_screen.due(start + 2 * MINUTE), None);
    on_screen.moved(ScreenMove::WindowShowed, start + 2 * MINUTE);

    assert_eq!(
        on_screen.due(start + 2 * MINUTE),
        Some(Fetched::Issue(issue(7)))
    );
}

#[test]
fn a_closed_item_is_read_no_more() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);

    on_screen.moved(ScreenMove::Closed(issue(7)), start);

    assert_eq!(on_screen.wake(), None);
    assert_eq!(on_screen.due(start + MINUTE), None);
}

#[test]
fn closing_one_item_leaves_the_others_on_screen() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);
    let later = start + Duration::from_secs(10);
    on_screen.moved(ScreenMove::Opened(Fetched::Issue(issue(8))), later);

    on_screen.moved(ScreenMove::Closed(issue(7)), later);

    assert_eq!(on_screen.wake(), Some(later + MINUTE));
}

#[test]
fn an_item_stays_on_screen_until_each_view_that_opened_it_has_closed() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);
    on_screen.moved(ScreenMove::Opened(Fetched::Issue(issue(7))), start);

    on_screen.moved(ScreenMove::Closed(issue(7)), start);
    assert_eq!(on_screen.wake(), Some(start + MINUTE));
    on_screen.moved(ScreenMove::Closed(issue(7)), start);

    assert_eq!(on_screen.wake(), None);
}

#[test]
fn a_window_that_loads_afresh_holds_no_item_of_the_page_before_and_shows() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);
    on_screen.moved(ScreenMove::WindowHid, start);

    on_screen.moved(ScreenMove::WindowLoaded, start);
    assert_eq!(on_screen.wake(), None);
    on_screen.moved(ScreenMove::Opened(Fetched::Issue(issue(8))), start);

    assert_eq!(on_screen.wake(), Some(start + MINUTE));
}

#[test]
fn a_move_finds_its_item_whatever_the_case_of_its_repositorys_name() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);
    let shouted = Item {
        repository: RepositoryName::parse("Python/CPython").unwrap(),
        number: 7,
    };

    on_screen.moved(ScreenMove::Closed(shouted), start);

    assert_eq!(on_screen.wake(), None);
}

#[test]
fn opening_a_hidden_item_again_shows_it_and_starts_its_minute_again() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);
    on_screen.moved(ScreenMove::Hid(issue(7)), start);

    on_screen.moved(
        ScreenMove::Opened(Fetched::Issue(issue(7))),
        start + MINUTE / 2,
    );

    assert_eq!(on_screen.wake(), Some(start + MINUTE + MINUTE / 2));
}

#[test]
fn the_item_due_first_reads_first() {
    let start = Instant::now();
    let mut on_screen = opened_at(start + MINUTE / 2);
    let pr = Fetched::PullRequest(issue(8));

    on_screen.moved(ScreenMove::Opened(pr.clone()), start);

    assert_eq!(on_screen.wake(), Some(start + MINUTE));
    assert_eq!(on_screen.due(start + 2 * MINUTE), Some(pr));
}

#[test]
fn once_tsugiki_has_a_token_each_item_on_screen_is_due_at_once() {
    let start = Instant::now();
    let mut on_screen = opened_at(start);

    on_screen.all_due(start + MINUTE / 2);

    assert_eq!(on_screen.wake(), Some(start + MINUTE / 2));
    let read = Some(Fetched::Issue(issue(7)));
    assert_eq!(on_screen.due(start + MINUTE / 2), read);
}
