use std::time::Duration;

use super::*;

/// A patience that is over by the time a test looks.
const NONE: Duration = Duration::ZERO;

/// A patience that lasts longer than a test.
const MINUTE: Duration = Duration::from_secs(60);

fn sha(digit: char) -> CommitSha {
    CommitSha::parse(&digit.to_string().repeat(40)).unwrap()
}

fn at(number: u32, digit: char) -> PrHead {
    PrHead {
        number,
        commit: sha(digit),
        target_branch: "main".to_owned(),
    }
}

fn numbers(heads: impl Iterator<Item = PrHead>) -> Vec<(u32, CommitSha)> {
    heads.map(|head| (head.number, head.commit)).collect()
}

/// After a minute's patience is over.
fn later() -> Instant {
    Instant::now() + MINUTE * 2
}

/// Notes a fetch of `batch` that succeeded, with each ref at `refs`.
fn fetched(prs: &mut Prs, batch: &[PrHead], refs: &[PrHead], patience: Duration) -> Noted {
    let landed = (refs.iter())
        .map(|head| (head.number, head.commit.clone()))
        .collect();
    prs.note_fetched(batch, Ok(&landed), patience)
}

#[test]
fn a_hand_over_wants_only_the_heads_the_mirror_lacks_and_drops_the_prs_it_leaves_out() {
    let mut prs = Prs::default();
    prs.hand_over(vec![at(1, 'a'), at(2, 'b')]);
    let _ = fetched(&mut prs, &[at(1, 'a')], &[at(1, 'a')], NONE);

    prs.hand_over(vec![at(1, 'a'), at(3, 'c')]);

    assert_eq!(numbers(prs.wanted(Instant::now())), [(3, sha('c'))]);
}

#[test]
fn a_hand_over_leaves_a_head_waiting_its_moment_and_restarts_one_whose_retry_is_spent() {
    let mut prs = Prs::default();
    prs.hand_over(vec![at(1, 'a'), at(2, 'b')]);
    let _ = fetched(&mut prs, &[at(1, 'a')], &[at(1, '9')], MINUTE);
    for _ in 0..2 {
        let _ = fetched(&mut prs, &[at(2, 'b')], &[at(2, '9')], NONE);
    }

    prs.hand_over(vec![at(1, 'a'), at(2, 'b')]);

    assert_eq!(numbers(prs.wanted(Instant::now())), [(2, sha('b'))]);
    assert_eq!(numbers(prs.wanted(later())), [(1, sha('a')), (2, sha('b'))]);
}

#[test]
fn a_fetch_speaks_for_the_users_head_at_its_commit_and_leaves_the_rules_head_at_another() {
    let now = Instant::now();
    let mut prs = Prs::default();
    prs.hand_over(vec![at(5, 'a')]);
    let batch: Vec<PrHead> = prs.wanted(now).collect();
    // The user opens PR 5 at the same commit, then the rules move on during the fetch.
    prs.fetch_first(at(5, 'a'));
    prs.hand_over(vec![at(5, 'b')]);

    let noted = fetched(&mut prs, &batch, &[at(5, 'a')], NONE);

    assert_eq!(noted.arrived, [5]);
    assert!(prs.demanded(now).next().is_none(), "the fetch brought it");
    assert_eq!(numbers(prs.wanted(now)), [(5, sha('b'))]);
}

#[test]
fn the_app_hears_of_a_pr_the_user_opened_only_from_the_fetches_its_head_was_in() {
    let mut prs = Prs::default();
    prs.hand_over(vec![at(5, 'a'), at(6, 'a')]);
    prs.fetch_first(at(5, 'b'));
    let batch = [at(5, 'a'), at(6, 'a')];

    let arrived = fetched(&mut prs, &batch, &[at(5, 'a'), at(6, 'a')], NONE);
    prs.hand_over(vec![at(5, 'c'), at(6, 'c')]);
    let batch = [at(5, 'c'), at(6, 'c')];
    let first = fetched(&mut prs, &batch, &[at(5, '9'), at(6, '9')], NONE);
    let second = fetched(&mut prs, &batch, &[at(5, '9'), at(6, '9')], NONE);

    assert_eq!(arrived.arrived, [6], "PR 5's ref isn't at the user's head");
    assert_eq!(first.stuck, BTreeMap::new());
    let behind = BTreeMap::from([(Unfetched::Behind, vec![6])]);
    assert_eq!(second.stuck, behind);
    assert_eq!(numbers(prs.demanded(Instant::now())), [(5, sha('b'))]);
}

#[test]
fn a_head_the_user_opened_waiting_its_moment_takes_no_try_from_the_rules_fetch() {
    let mut prs = Prs::default();
    prs.fetch_first(at(5, 'a'));
    let _ = fetched(&mut prs, &[at(5, 'a')], &[at(5, '9')], MINUTE);
    prs.hand_over(vec![at(5, 'a')]);

    let noted = fetched(&mut prs, &[at(5, 'a')], &[at(5, '9')], MINUTE);

    assert_eq!(noted, Noted::default(), "GitHub may move the ref yet");
    assert!(prs.demanded(Instant::now()).next().is_none());
    assert_eq!(numbers(prs.demanded(later())), [(5, sha('a'))]);
}

#[test]
fn a_head_github_lacks_at_its_retry_meets_the_rules_but_not_the_user() {
    let mut prs = Prs::default();
    prs.hand_over(vec![at(5, 'a')]);
    prs.fetch_first(at(5, 'a'));
    let batch = [at(5, 'a')];

    let _ = fetched(&mut prs, &batch, &[], NONE);
    let noted = fetched(&mut prs, &batch, &[], NONE);

    let missing = BTreeMap::from([(Unfetched::NotOnGitHub, vec![5])]);
    assert_eq!(noted.stuck, missing);
    prs.hand_over(vec![at(5, 'a')]);
    assert!(prs.wanted(Instant::now()).next().is_none());
    assert!(!prs.asks_wait(), "its retry is spent");
}

#[test]
fn the_users_look_at_another_commit_leaves_where_the_ref_stands() {
    let mut prs = Prs::default();
    prs.hand_over(vec![at(5, 'a')]);
    let _ = fetched(&mut prs, &[at(5, 'a')], &[at(5, 'a')], NONE);

    prs.fetch_first(at(5, 'b'));
    prs.hand_over(vec![at(5, 'a')]);

    let now = Instant::now();
    assert!(
        prs.wanted(now).next().is_none(),
        "the mirror holds the rules' head"
    );
    assert_eq!(numbers(prs.demanded(now)), [(5, sha('b'))]);
}

#[test]
fn a_saved_head_of_an_awaited_pr_is_asked_for_unless_the_mirror_holds_it() {
    let mut prs = Prs::default();
    prs.hand_over(vec![at(7, 'a')]);
    let _ = fetched(&mut prs, &[at(7, 'a')], &[at(7, 'a')], NONE);
    prs.await_head(7);
    prs.await_head(8);
    prs.opened(9);
    // Opened while the mirror held its head, then its head went from the store.
    prs.opened(10);
    prs.await_head(10);

    let asked = prs.saved(vec![at(7, 'a'), at(8, 'b'), at(9, 'c'), at(10, 'd')]);

    assert!(asked);
    let demanded = numbers(prs.demanded(Instant::now()));
    assert_eq!(demanded, [(8, sha('b')), (10, sha('d'))]);
}

#[test]
fn a_new_clone_fetches_again_each_head_fetched_since_launch_with_its_target_branch() {
    let mut prs = Prs::default();
    let backport = PrHead {
        target_branch: "3.14".to_owned(),
        ..at(1, 'a')
    };
    prs.hand_over(vec![backport.clone(), at(2, 'b')]);
    let batch = [backport.clone(), at(2, 'b')];
    let _ = fetched(&mut prs, &batch, &[at(1, 'a'), at(2, '9')], MINUTE);

    prs.fetch_again();

    let again: Vec<PrHead> = prs.wanted(Instant::now()).collect();
    assert_eq!(again, [backport, at(2, 'b')]);
}

#[test]
fn upkeep_prunes_only_prs_out_of_reach_that_the_user_hasnt_opened() {
    let mut prs = Prs::default();
    assert!(prs.out_of_reach().is_none(), "nothing before a hand-over");
    prs.hand_over(vec![at(1, 'a')]);
    prs.opened(2);

    let out_of_reach = prs.out_of_reach().unwrap();
    let pruned: Vec<u32> = [1, 2, 3].into_iter().filter(out_of_reach).collect();
    assert_eq!(pruned, [3]);
}

#[test]
fn a_pr_back_in_reach_whose_ref_upkeep_pruned_meanwhile_is_fetched_again() {
    let mut prs = Prs::default();
    prs.hand_over(vec![at(1, 'a'), at(2, 'b')]);
    let batch = [at(1, 'a'), at(2, 'b')];
    let _ = fetched(&mut prs, &batch, &[at(1, 'a'), at(2, 'b')], NONE);
    // Upkeep plans to prune both. PR 1 comes back while its git runs.
    prs.hand_over(vec![]);
    prs.hand_over(vec![at(1, 'a')]);

    prs.pruned(&[1, 2]);

    assert_eq!(numbers(prs.wanted(Instant::now())), [(1, sha('a'))]);
}
