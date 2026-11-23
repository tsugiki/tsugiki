use super::*;
use crate::filters::FilterQuery;

/// An item drawn `hidden`, of which `is` holds.
fn standing(hidden: Option<Hidden>, is: &[Is]) -> Standing {
    Standing {
        hidden,
        is: is.iter().copied().collect(),
    }
}

fn filter() -> WorkbenchView {
    WorkbenchView::Filter {
        query: FilterQuery::try_from("label:bug".to_owned()).unwrap(),
    }
}

#[test]
fn mark_seen_takes_an_item_out_of_the_stale_reviews_and_off_the_workbench_while_only_its_waiting_held_it()
 {
    let waiting = standing(
        None,
        &[Is::NeedingAttention, Is::Waiting, Is::Stale, Is::Requested],
    );
    let held = standing(None, &[Is::NeedingAttention, Is::Waiting, Is::Held]);

    let seen = |view: &WorkbenchView, standing: &Standing| outcomes_of(view, standing).mark_seen;

    // A Review request stands, acknowledged or not, so its view keeps the item (#865).
    assert_eq!(
        [
            seen(&WorkbenchView::StaleReviews, &waiting),
            seen(&WorkbenchView::Whole, &waiting),
            seen(&WorkbenchView::ReviewRequests, &waiting),
            seen(&WorkbenchView::Whole, &held),
        ],
        [
            Outcome::Leaves,
            Outcome::Leaves,
            Outcome::Stays,
            Outcome::Stays
        ],
    );
}

#[test]
fn mark_seen_puts_a_closed_item_back_in_the_archive_which_a_filters_view_keeps_drawn_archived() {
    let closed = standing(
        None,
        &[
            Is::NeedingAttention,
            Is::Waiting,
            Is::Held,
            Is::Found,
            Is::RestingArchived,
        ],
    );

    assert_eq!(
        outcomes_of(&WorkbenchView::Whole, &closed).mark_seen,
        Outcome::Leaves
    );
    assert_eq!(outcomes_of(&filter(), &closed).mark_seen, Outcome::Stays);
}

#[test]
fn archive_keeps_a_pin_in_the_pinned_issues_and_a_requested_pr_in_the_review_requests() {
    let pin = standing(None, &[Is::Pinned, Is::Issue]);
    let requested = standing(None, &[Is::Requested, Is::Asked, Is::Held]);

    assert_eq!(
        outcomes_of(&WorkbenchView::Pinned, &pin).archive,
        Outcome::Stays
    );
    assert_eq!(
        outcomes_of(&WorkbenchView::Whole, &pin).archive,
        Outcome::Leaves
    );
    assert_eq!(
        outcomes_of(&WorkbenchView::ReviewRequests, &requested).archive,
        Outcome::Stays
    );
    assert_eq!(
        outcomes_of(&WorkbenchView::StaleReviews, &standing(None, &[Is::Stale])).archive,
        Outcome::Leaves
    );
}

#[test]
fn mute_keeps_a_pr_whose_request_stands_in_place_and_folds_the_rest_of_a_filters_view() {
    let requested = standing(None, &[Is::Requested, Is::Asked, Is::Found, Is::Held]);
    let found = standing(None, &[Is::Found, Is::Held]);

    assert_eq!(
        outcomes_of(&WorkbenchView::ReviewRequests, &requested).mute,
        Outcome::Stays
    );
    assert_eq!(outcomes_of(&filter(), &requested).mute, Outcome::Stays);
    assert_eq!(outcomes_of(&filter(), &found).mute, Outcome::Folds);
    assert_eq!(
        outcomes_of(&WorkbenchView::Whole, &found).mute,
        Outcome::Leaves
    );
}

#[test]
fn an_action_that_does_nothing_to_the_item_leaves_it_unchanged() {
    let muted = standing(Some(Hidden::Muted), &[Is::Found, Is::Issue]);
    let standalone = standing(None, &[Is::Held]);

    let outcomes = outcomes_of(&filter(), &muted);
    let mine = outcomes_of(&WorkbenchView::Whole, &standalone);

    assert_eq!(
        [
            outcomes.mark_seen,
            outcomes.archive,
            outcomes.mute,
            outcomes.unarchive,
            outcomes.unpin
        ],
        [Outcome::Unchanged; 5],
    );
    assert_eq!([mine.pin, mine.mute_author], [Outcome::Unchanged; 2]);
}

#[test]
fn mute_author_takes_the_item_its_author_opened_out_of_any_view_but_a_filters() {
    let theirs = standing(None, &[Is::OfferingMuteAuthor, Is::Held, Is::Found]);

    assert_eq!(
        outcomes_of(&WorkbenchView::Whole, &theirs).mute_author,
        Outcome::Leaves
    );
    assert_eq!(outcomes_of(&filter(), &theirs).mute_author, Outcome::Folds);
}

#[test]
fn a_pin_and_unarchive_take_an_issue_out_of_archived_alone() {
    let archived = standing(Some(Hidden::Archived), &[Is::Issue, Is::Found]);

    let in_archived = outcomes_of(&WorkbenchView::Archived, &archived);
    let in_filter = outcomes_of(&filter(), &archived);

    assert_eq!(
        [in_archived.pin, in_archived.unarchive],
        [Outcome::Leaves; 2]
    );
    assert_eq!([in_filter.pin, in_filter.unarchive], [Outcome::Stays; 2]);
}

#[test]
fn an_unpin_takes_the_issue_off_the_workbench_while_only_the_pin_held_it_or_as_it_archives_it() {
    let alone = standing(None, &[Is::Pinned, Is::Issue]);
    let held = standing(None, &[Is::Pinned, Is::Issue, Is::Held]);
    let closed = standing(
        None,
        &[
            Is::Pinned,
            Is::Issue,
            Is::Held,
            Is::ClosedWhilePinned,
            Is::Stale,
        ],
    );
    let waiting_closed = standing(
        None,
        &[
            Is::Pinned,
            Is::Issue,
            Is::ClosedWhilePinned,
            Is::NeedingAttention,
            Is::Waiting,
        ],
    );

    let unpin = |view: &WorkbenchView, standing: &Standing| outcomes_of(view, standing).unpin;

    assert_eq!(
        [
            unpin(&WorkbenchView::Whole, &alone),
            unpin(&WorkbenchView::Whole, &held),
            unpin(&WorkbenchView::Whole, &closed),
            unpin(&WorkbenchView::StaleReviews, &closed),
            unpin(&WorkbenchView::Whole, &waiting_closed),
            unpin(&WorkbenchView::Pinned, &held),
        ],
        [
            Outcome::Leaves,
            Outcome::Stays,
            Outcome::Leaves,
            Outcome::Leaves,
            Outcome::Stays,
            Outcome::Leaves,
        ],
    );
}

#[test]
fn an_unpin_leaves_a_muted_pin_muted_though_it_closed_while_pinned() {
    let muted = standing(
        Some(Hidden::Muted),
        &[
            Is::Pinned,
            Is::Issue,
            Is::ClosedWhilePinned,
            Is::NeedingAttention,
            Is::Asked,
        ],
    );

    // Muted hides more than the archive the unpin would make, and the fresh ask keeps it on the Workbench.
    assert_eq!(
        outcomes_of(&WorkbenchView::Whole, &muted).unpin,
        Outcome::Stays
    );
}
